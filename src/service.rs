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

/// Addresses of a host and port, none when it cannot be looked up.
trait SocketAddrsSafe {
    fn to_socket_addrs_safe(&self) -> Vec<std::net::SocketAddr>;
}

impl SocketAddrsSafe for (&str, u16) {
    fn to_socket_addrs_safe(&self) -> Vec<std::net::SocketAddr> {
        std::net::ToSocketAddrs::to_socket_addrs(self).map(|a| a.collect()).unwrap_or_default()
    }
}
use std::time::{Duration, Instant};

type R = Result<Value, Ctrl>;

/// How old a signed call may be (seconds), either way: clocks differ a little.
const WINDOW: u64 = 300;

/// The secret services sign their calls with, if set: the first of those given, while any
/// of them is taken (FEEDBACK38.md, Q-461; branch feedback36-39).
fn secret() -> Option<String> {
    crate::svcauth::secrets().into_iter().next()
}

/// The shared secret `sig` was made with by `make`, if one was.
fn shared_signed(sig: &str, make: impl Fn(&str) -> String) -> Option<String> {
    crate::svcauth::secrets().into_iter().find(|k| crate::crypto::same(make(k).as_bytes(), sig.as_bytes()))
}

/// Which services may join, as `--no-join` and `--allow-join` say (none set: any declared).
static JOIN_POLICY: OnceLock<Option<Vec<String>>> = OnceLock::new();

/// `--no-join` (an empty list) or `--allow-join billing,risk` (FEEDBACK38.md, Q-459).
pub fn set_join_policy(allowed: Option<Vec<String>>) {
    let _ = JOIN_POLICY.set(allowed);
}

/// `--join-as https://billing-3.internal:8443`: the address a copy is called at.
static JOIN_AS: OnceLock<String> = OnceLock::new();

pub fn join_as(url: &str) {
    let _ = JOIN_AS.set(url.trim_end_matches('/').to_string());
}

/// The service this process serves as (`--service-name`, or the program's
/// file name): a signed call names the service it is for, and one for
/// another is refused here. Not set (a program run by a test, or embedded),
/// a call for any service is taken.
static SERVED_AS: OnceLock<String> = OnceLock::new();

/// Says which service this process is (see `SERVED_AS`); the first one set holds.
pub fn serve_as(name: &str) {
    let _ = SERVED_AS.set(name.to_string());
}

/// What this process serves as: its file's name (`main` for main.qwe) or `--service-name`.
pub fn served_as() -> Option<&'static str> {
    SERVED_AS.get().map(String::as_str)
}

/// `QUILLO_SERVICE_BILLING` for `billing`.
pub fn env_name(name: &str) -> String {
    format!("QUILLO_SERVICE_{}", name.to_ascii_uppercase())
}

/// Where service `name` runs, when not in this process: its addresses
/// (`http://a:8081,http://b:8081`, the same service on several servers),
/// and the copies that joined while the program runs.
pub fn urls_of(shared: &crate::shared::Shared, name: &str) -> Vec<String> {
    let mut urls = configured_urls(shared, name);
    for u in joined_urls(name) {
        if !urls.contains(&u) {
            urls.push(u);
        }
    }
    urls
}

/// The addresses service `name` was given (`--service-url`, `QUILLO_SERVICE_...`).
pub fn configured_urls(shared: &crate::shared::Shared, name: &str) -> Vec<String> {
    let given = match shared.service_urls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(name) {
        Some(u) => u.clone(),
        None => std::env::var(env_name(name)).unwrap_or_default(),
    };
    given.split(',').map(|u| u.trim().trim_end_matches('/').to_string()).filter(|u| !u.is_empty()).collect()
}

// ---- copies that join (`--join`) -------------------------------------------------------------

/// How long a joined copy is kept without a heartbeat.
const JOINED_FOR: Duration = Duration::from_secs(15);
/// How often a copy says it is still there.
const HEARTBEAT: Duration = Duration::from_secs(5);

/// The copies of each service that joined this process, when each was
/// last heard from (FEEDBACK10.md, Q-074; branch feedback10-fixes), and
/// whether it proved, as it joined, that it holds the service's app key
/// (FEEDBACK56.md, Part F, second review).
type Copies = HashMap<String, HashMap<String, (Instant, bool)>>;

fn joined() -> &'static Mutex<Copies> {
    static J: OnceLock<Mutex<Copies>> = OnceLock::new();
    J.get_or_init(Default::default)
}

/// The copies of service `name` that joined and are still heard from.
pub fn joined_urls(name: &str) -> Vec<String> {
    joined_where(name, false)
}

/// Those of them that proved they hold its app's key: the only ones a tool's call is sent to, as
/// it carries the person's session and arguments, and only billing may run billing's tools.
pub fn joined_proved_urls(name: &str) -> Vec<String> {
    joined_where(name, true)
}

fn joined_where(name: &str, proved: bool) -> Vec<String> {
    let mut j = joined().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(copies) = j.get_mut(name) else { return Vec::new() };
    copies.retain(|_, (at, _)| at.elapsed() < JOINED_FOR);
    let mut urls: Vec<String> = copies.iter().filter(|(_, (_, p))| *p || !proved).map(|(u, _)| u.clone()).collect();
    urls.sort();
    urls
}

/// What a copy signs its join with to prove it holds its service's app key (`x-quillo-app`).
fn sign_join(key: &str, t: u64, service: &str, nonce: &str, body: &[u8]) -> String {
    sign_app(key, t, service, "_join", "", nonce, "", "", "", body)
}

/// `POST /_quillo/join` on the app: a copy of one of its services joining
/// (or, `leave: true`, leaving), signed with the services' secret. Its
/// address is the connection's, with the port it gives, as limer takes it.
pub fn join_here(it: &Interp, req: &HttpRequest) -> HttpResponse {
    if req.method != "POST" {
        return refused(405, "joining is a POST");
    }
    let keyed = it.accept.as_ref().is_some_and(|a| !a.keys.is_empty());
    if secret().is_none() && !keyed {
        return refused(403, "joining needs QUILLO_SERVICE_SECRET, on both sides");
    }
    let header = |name: &str| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    let sig = crate::svcauth::signed(header("x-quillo-service").unwrap_or(""));
    let (t, given) = (sig.t, sig.sig);
    let Some(nonce) = header("x-quillo-call").filter(|c| !c.is_empty() && c.len() <= 64 && c.bytes().all(|b| b.is_ascii_alphanumeric())) else {
        return refused(401, "a join carries its nonce (x-quillo-call)");
    };
    let Ok(body) = crate::value::json_value(&req.body) else {
        return refused(400, "the body is JSON: {\"service\": ..., \"port\": ...}");
    };
    let service = body["service"].as_str().unwrap_or("").to_string();
    let port = body["port"].as_u64().filter(|p| (1..=65535).contains(p));
    // Who may join: a copy of `billing` proves it with billing's own key, where the app knows
    // one, and not with the secret every service has (FEEDBACK38.md, Q-459; branch feedback36-39).
    if let Some(Some(allowed)) = JOIN_POLICY.get() {
        if !allowed.iter().any(|a| *a == service) {
            return refused(403, &if allowed.is_empty() { "this app takes no copies (--no-join)".to_string() } else { format!("this app takes copies of {} only (--allow-join)", allowed.join(", ")) });
        }
    }
    let theirs = it.accept.as_ref().and_then(|a| a.keys.get(&service)).cloned();
    let ok = match (&theirs, sig.kid) {
        (_, kid) if !kid.is_empty() && kid != service => return refused(401, &format!("a copy joins with its own service's key: this one is `{}`, not `{}`", kid, service)),
        (Some(keys), kid) if !kid.is_empty() => keys.iter().any(|k| crate::crypto::same(crate::svcauth::sign3(k, t, "_join", &service, "", nonce, kid, "", "", "", &req.body).as_bytes(), given.as_bytes())),
        (Some(_), _) => return refused(401, &format!("this app takes copies of `{}` signed with its key (QUILLO_SERVICE_KEY on the copy)", service)),
        (None, kid) if !kid.is_empty() => return refused(401, &format!("this app has no key for `{}` (accept_calls {{ keys }})", service)),
        (None, _) => shared_signed(given, |k| signature(k, t, "_join", &service, "", nonce, &req.body)).is_some(),
    };
    if now_secs().abs_diff(t) > WINDOW || !ok {
        let whose = if sig.kid.is_empty() { "this server's QUILLO_SERVICE_SECRET".to_string() } else { format!("`{}`'s key", service) };
        return refused(401, &format!("not signed with {} (or too old)", whose));
    }
    {
        let mut seen = seen().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if seen.contains_key(nonce) {
            return refused(409, "this join was sent before");
        }
        seen.insert(nonce.to_string(), (Instant::now(), None));
    }
    let Some(port) = port else { return refused(400, "a join gives the copy's port") };
    if !it.declared_services.iter().any(|n| *n == service) {
        return refused(404, &format!("this program has no service `{}`", service));
    }
    let ip = req.remote.parse::<std::net::SocketAddr>().map(|a| a.ip()).or_else(|_| req.remote.parse::<std::net::IpAddr>());
    let Ok(ip) = ip else { return refused(400, "the copy's address is not known") };
    // The address it gives (`--join-as https://...`), when it names where it joined from: then a
    // copy elsewhere is called over HTTPS, not always `http://` (Q-459).
    let url = match body["url"].as_str() {
        Some(u) => match url::Url::parse(u) {
            Ok(p) if matches!(p.scheme(), "http" | "https") && p.host_str().is_some() => {
                let host = p.host_str().unwrap_or("").trim_start_matches('[').trim_end_matches(']').to_string();
                let port = p.port_or_known_default().unwrap_or(80);
                let here = (host.as_str(), port).to_socket_addrs_safe().iter().any(|a| a.ip() == ip || (a.ip().is_loopback() && ip.is_loopback()));
                if !here {
                    return refused(400, &format!("the copy says it is at {}, which is not where it joined from ({})", u, ip));
                }
                u.trim_end_matches('/').to_string()
            }
            _ => return refused(400, "the copy's url is an http(s) address"),
        },
        None => match ip {
            std::net::IpAddr::V6(v6) => format!("http://[{}]:{}", v6, port),
            v4 => format!("http://{}:{}", v4, port),
        },
    };
    // It proves it holds the service's app key, and is sent its tools' calls; a join that says it
    // does and is not signed with it is refused (FEEDBACK56.md, Part F, second review).
    let proved = match req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("x-quillo-app")).map(|(_, v)| v.as_str()) {
        None => false,
        Some(given) => match app_keys(&service) {
            Ok(Some(keys)) if signed_by_any(given, &keys, &|k, t| sign_join(k, t, &service, nonce, &req.body)) => true,
            Ok(Some(_)) => return refused(401, &format!("this copy's join is not signed with `{}`'s app key ({})", service, app_key_env(&service))),
            Ok(None) | Err(_) => false,
        },
    };
    let mut j = joined().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let copies = j.entry(service.clone()).or_default();
    if body["leave"].as_bool() == Some(true) {
        if copies.remove(&url).is_some() {
            eprintln!("quillo: service {}: {} left", service, url);
        }
    } else if copies.insert(url.clone(), (Instant::now(), proved)).is_none() {
        eprintln!("quillo: service {}: {} joined", service, url);
    }
    json(200, serde_json::json!({ "service": service, "at": url }))
}

/// A copy of a service joining an app (`quillo run risk.qwe --as-service
/// --join http://app:8080`): it says it is there every few seconds, and
/// that it is leaving when it stops.
pub struct Joining {
    app: String,
    service: String,
    port: u16,
    leaving: std::sync::atomic::AtomicBool,
}

impl Joining {
    pub fn start(app: String, service: String, port: u16) -> Arc<Joining> {
        let j = Arc::new(Joining { app, service, port, leaving: Default::default() });
        let beat = j.clone();
        std::thread::spawn(move || {
            let mut was_in = false;
            while !beat.leaving.load(Ordering::SeqCst) {
                match beat.post(false) {
                    Ok(()) if !was_in => {
                        eprintln!("quillo: joined {} as a copy of `{}`", beat.app, beat.service);
                        was_in = true;
                    }
                    Err(e) if was_in || !e.is_empty() => {
                        if was_in {
                            eprintln!("quillo: cannot reach {} to stay a copy of `{}`: {}", beat.app, beat.service, e);
                        } else {
                            eprintln!("quillo: cannot join {} as a copy of `{}`: {}", beat.app, beat.service, e);
                        }
                        was_in = false;
                    }
                    _ => {}
                }
                std::thread::sleep(HEARTBEAT);
            }
        });
        j
    }

    /// Leaves the app, so it sends no more calls here (when the copy stops).
    pub fn leave(&self) {
        self.leaving.store(true, Ordering::SeqCst);
        match self.post(true) {
            Ok(()) => eprintln!("quillo: left {} (`{}`)", self.app, self.service),
            Err(e) => eprintln!("quillo: cannot leave {}: {} (it drops this copy when the heartbeat stops)", self.app, e),
        }
    }

    fn post(&self, leave: bool) -> Result<(), String> {
        let mut body = serde_json::json!({ "service": self.service, "port": self.port, "leave": leave });
        if let Some(u) = JOIN_AS.get() {
            body["url"] = serde_json::json!(u);
        }
        let body = body.to_string();
        let t = now_secs();
        let nonce = new_id(16);
        // With this service's own key when it has one, which proves it is this service.
        let header = match crate::svcauth::own_key() {
            Some(k) => format!("v=3,kid={},t={},sig={}", self.service, t, crate::svcauth::sign3(&k, t, "_join", &self.service, "", &nonce, &self.service, "", "", "", body.as_bytes())),
            None => {
                let secret = secret().ok_or("QUILLO_SERVICE_SECRET (or QUILLO_SERVICE_KEY) is not set")?;
                format!("t={},sig={}", t, signature(&secret, t, "_join", &self.service, "", &nonce, body.as_bytes()))
            }
        };
        let mut r = agent()
            .post(&format!("{}/_quillo/join", self.app))
            .timeout(Duration::from_secs(3))
            .set("content-type", "application/json")
            .set("x-quillo-service", &header)
            .set("x-quillo-call", &nonce);
        // Holding its app's key, it proves it, and is sent its tools' calls (FEEDBACK56.md, Part F,
        // second review).
        if let Ok(Some((k, _))) = app_keys_in(APP_KEY, &|v| std::env::var(v).ok()) {
            r = r.set("x-quillo-app", &format!("t={},sig={}", t, sign_join(&k, t, &self.service, &nonce, body.as_bytes())));
        }
        let r = r.send_string(&body);
        match r {
            Ok(_) => Ok(()),
            Err(ureq::Error::Status(code, resp)) => Err(format!("{} {}", code, resp.into_string().unwrap_or_default().trim())),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// The first address of service `name`: where its channels are, so that one
/// copy (under `--scale`) holds a channel's pages and state (FEEDBACK12.md,
/// Q-087; branch open-fixes).
pub fn first_url_of(shared: &crate::shared::Shared, name: &str) -> Option<String> {
    urls_of(shared, name).first().cloned()
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
    opts_table().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(name).copied().unwrap_or_default()
}

/// The longest wait taken: a century, as long as for ever.
const LONGEST: Duration = Duration::from_secs(100 * 365 * 86_400);

/// A duration given as milliseconds (`1500`) or text (`"1.5s"`, `"500ms"`, `"2m"`). One no
/// `Duration` holds (`1e300`, `math.inf`, `"99999999999999999999s"`) is no duration, as `"1.5h"`
/// is not: `from_secs_f64` stopped the program as it loaded. One longer than a century is a
/// century: an `Instant` holds less than a `Duration`, so `now + cooldown` stopped the program and
/// every call with such a timeout failed, "Request deadline overflowed" (FEEDBACK55.md, Q-830;
/// branch feedback55).
fn duration(v: &Value) -> Option<Duration> {
    given_duration(v).map(|d| d.min(LONGEST))
}

fn given_duration(v: &Value) -> Option<Duration> {
    match v {
        Value::Int(n) if *n >= 0 => Some(Duration::from_millis(*n as u64)),
        Value::Float(f) if *f >= 0.0 => Duration::try_from_secs_f64(f / 1000.0).ok(),
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
            Duration::try_from_secs_f64(ms / 1000.0).ok()
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
            // How the call proves who makes it (src/svcauth.rs), read where it is declared.
            "auth" | "forward" => {}
            other => {
                return fail(line, format!("service {}: no option `{}` (timeout, retries, failures, cooldown, auth, forward)", name, other));
            }
        }
    }
    opts_table().lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(name.to_string(), o);
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
    let mut h = health().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let mut h = health().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if h.get(url).map_or(false, |st| st.fails > 0 || st.until.is_some()) {
        if h.get(url).map_or(false, |st| st.until.is_some()) {
            eprintln!("quillo: {} answers again", url);
        }
        h.remove(url);
    }
}

fn went_wrong(url: &str, o: &Opts) {
    let mut h = health().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let st = h.entry(url.to_string()).or_default();
    st.fails += 1;
    if st.fails >= o.failures {
        let times = 1u32 << (st.fails - o.failures).min(4);
        let wait = o.cooldown.saturating_mul(times);
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
    let healthy = |u: &&String| health().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(u.as_str()).map_or(true, |st| st.until.map_or(true, |t| Instant::now() >= t));
    let fresh = order.iter().copied().find(|u| !tried.contains(u) && healthy(u));
    let any = || order.iter().copied().find(|u| healthy(u));
    let u = fresh.or_else(any)?;
    usable(u, o).then(|| u.clone())
}

// ---- what a service offers ---------------------------------------------------------------

/// The statements of a program and the files it `use`s. Every file's paths count from
/// `base`, the program's folder (FEEDBACK19.md, Q-154; branch feedback19-fixes).
fn statements(path: &Path, base: &Path, seen: &mut HashSet<PathBuf>, out: &mut Vec<Stmt>) -> Result<(), String> {
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !seen.insert(key) {
        return Ok(());
    }
    let src = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    // What the app needs of a service is its `server fn`s and the services it declares: a file
    // that does not parse is read for those alone, so an error in a body or a route of the
    // service's does not stop the app, which the service's own check reports (FEEDBACK40.md,
    // Q-471; branch feedback40-43).
    let stmts = match Parser::new(&src).parse_program() {
        Ok(s) => s,
        Err(e) => {
            let found = headers_only(&src);
            if found.is_empty() {
                return Err(format!("{}: {}", path.display(), e));
            }
            found
        }
    };
    for s in stmts {
        if let Stmt::Use(file, _, _, options) = &s {
            if crate::ast::is_script(file) {
                out.push(s);
                continue;
            }
            // A path written without its extension is the file there; one written with it is
            // that file. A Rust file has no `server fn` or `service` of its own (the service
            // builds it as it loads): it was read as Quillo, and the app did not start
            // (FEEDBACK55.md, Q-819; branch feedback55).
            let used = crate::modules::resolve_use(base, path, file, options.implied);
            if crate::native::is_rust(&used) {
                out.push(s);
                continue;
            }
            statements(&base.join(used), base, seen, out)?;
        }
        out.push(s);
    }
    Ok(())
}

/// What a running service says its `server fn`s are: a call of `_contracts`, made as its calls
/// are (its `auth`, each address in turn), whose answer is each function's header and contract.
fn functions_of(it: &mut Interp, name: &str, urls: &[String]) -> Result<(Vec<Arc<FnDef>>, Vec<String>), String> {
    let list = match remote(it, urls, name, CONTRACTS, Vec::new(), None, None, new_id(16)) {
        Outcome::Ran(Value::List(l)) => l,
        Outcome::Ran(other) => return Err(format!("its answer is not a list of functions: {}", short(&other))),
        Outcome::Missed(why) | Outcome::Refused(_, why) => return Err(why),
        Outcome::Stopped | Outcome::Tool(_) => return Err("its answer is not a list of functions".into()),
    };
    let mut defs = Vec::new();
    let mut contracts = Vec::new();
    for f in list.iter() {
        let Value::Map(m) = f else { continue };
        let (Some(Value::Str(header)), Some(Value::Str(contract))) = (m.get("header"), m.get("contract")) else { continue };
        let stmts = Parser::new(header).parse_program().map_err(|e| format!("cannot read `{}`: {}", header, e))?;
        if let Some(Stmt::Fn(d)) = stmts.into_iter().next() {
            defs.push(d);
            contracts.push(contract.to_string());
        }
    }
    Ok((defs, contracts))
}

/// The function an app that does not hold a service's file calls to learn its functions.
const CONTRACTS: &str = "_contracts";

/// This program's `server fn`s, as headers an app can read and their contracts: what a call of
/// `_contracts` gets, once it has proved its caller as any call does.
fn contract_list(it: &Interp, who: &str) -> serde_json::Value {
    let mut out = Vec::new();
    // Those it may call: one kept to other callers (`for [...]`) is not named to it.
    for n in it.server_fns.iter().filter(|n| it.fn_callers.get(n.as_str()).map_or(true, |a| a.iter().any(|c| c == who))) {
        let Some(Value::Func(c)) = lookup(&it.globals, n) else { continue };
        let d = &c.def;
        let params: Vec<String> = d
            .params
            .iter()
            .map(|p| format!("{}{}{}", p.name, p.ty.as_ref().map_or(String::new(), |t| format!(": {}", t.text())), if p.default.is_some() { " = null" } else { "" }))
            .collect();
        let header = format!(
            "server fn {}({}){}{} = null",
            d.name,
            params.join(", "),
            d.ret.as_ref().map_or(String::new(), |t| format!(" -> {}", t.text())),
            d.err.as_ref().map_or(String::new(), |t| format!(" ! {}", t.text()))
        );
        out.push(serde_json::json!({ "header": header, "contract": crate::describe::contract(d) }));
    }
    serde_json::json!({ "ok": out })
}

/// The top-level `server fn` headers, `use`s and `service`s of a file that does not parse as a
/// whole: each read alone, its body left out.
fn headers_only(src: &str) -> Vec<Stmt> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let at_top = !l.starts_with([' ', '\t']);
        // `public server fn` and `private server fn` too.
        let bare = l.strip_prefix("public ").or_else(|| l.strip_prefix("private ")).unwrap_or(l);
        let words: Vec<&str> = bare.split_whitespace().take(2).collect();
        let take = |text: &str| Parser::new(text).parse_program().ok();
        if at_top && words == ["server", "fn"] {
            // The header: up to the `=` or `{` after its parameters that leaves a header that
            // parses; a `{` that does not is a shape's (`-> { title: str }`), and is gone past.
            let mut header = String::new();
            let mut depth = 0i32;
            let mut seen_params = false;
            let mut found = None;
            'lines: for line in &lines[i..] {
                for (k, c) in line.char_indices() {
                    match c {
                        '(' | '[' => depth += 1,
                        ')' | ']' => {
                            depth -= 1;
                            if depth == 0 {
                                seen_params = true;
                            }
                        }
                        '{' | '=' if depth == 0 && seen_params && !line[..k].ends_with(['-', '!', '<', '>', '=']) && !line[k + 1..].starts_with(['>', '=']) => {
                            if let Some(stmts) = take(&format!("{}{} = null\n", header, &line[..k])) {
                                found = Some(stmts);
                                break 'lines;
                            }
                            if c == '{' {
                                depth += 1;
                            }
                        }
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                header.push_str(line);
                header.push('\n');
            }
            if let Some(stmts) = found {
                out.extend(stmts.into_iter().filter(|s| matches!(s, Stmt::Fn(_))));
            }
        } else if at_top && (l.starts_with("use ") || l.starts_with("import ") || l.starts_with("service ")) {
            if let Some(stmts) = take(l) {
                out.extend(stmts.into_iter().filter(|s| matches!(s, Stmt::Use(..) | Stmt::Service(..))));
            }
        }
        i += 1;
    }
    out
}

/// The `server fn`s a service's program offers (its own and its `use`d files').
pub fn offered(path: &Path) -> Result<Vec<Arc<FnDef>>, String> {
    let mut stmts = Vec::new();
    statements(path, path.parent().unwrap_or(Path::new(".")), &mut HashSet::new(), &mut stmts)?;
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
        statements(&p, p.parent().unwrap_or(Path::new(".")), &mut HashSet::new(), &mut stmts)?;
        let dir = p.parent().unwrap_or(Path::new(".")).to_path_buf();
        for s in stmts {
            if let Stmt::Service(name, prefix, file, _, _, mcp) = s {
                // `service mcp crm = "https://..."` is no Quillo service to start (FEEDBACK56.md,
                // Q-853; branch feedback56).
                if mcp && prefix.is_none() {
                    continue;
                }
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
        let params: Vec<String> = d.params.iter().map(|p| if p.default.is_some() { format!("{} = null", p.name) } else { p.name.clone().to_string() }).collect();
        let names: Vec<&str> = d.params.iter().map(|p| p.name.as_str()).collect();
        let required = d.params.iter().filter(|p| p.default.is_none()).count();
        let mut made = |builtin: &str| -> R {
            let src = format!("({}) => {}({:?}, {:?}, [{}], {})", params.join(", "), builtin, name, d.name, names.join(", "), required);
            let lambda = Parser::new(&src).parse_expr().map_err(|e| Ctrl::Error(Box::new(crate::error::QuilloError::new(e.msg, line))))?;
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
    match make(it, &name, &f, sent, tenant, rid, None, line) {
        // A tool's call stopped in the service, to ask its user or at a question refused there:
        // it stops here too, as at a question of its own (FEEDBACK56.md, Part F; branch feedback56).
        Outcome::Stopped => fail(line, crate::mcp::ASKING),
        done => Ok(done.value()),
    }
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
    /// The service answered, with no value: a status, and why.
    Refused(u16, String),
    /// A call that carried a tool's call's asking stopped there: to ask the user, or at a question
    /// refused, as the asking it gave back says (FEEDBACK56.md, Part F; branch feedback56).
    Stopped,
    /// A service's tool answered its app's door: `tools/call`'s result, as it made it.
    Tool(serde_json::Value),
}

impl Outcome {
    fn value(self) -> Value {
        match self {
            Outcome::Ran(v) => v,
            Outcome::Missed(why) | Outcome::Refused(_, why) => Value::err(Value::str(why)),
            Outcome::Stopped => Value::err(Value::str(crate::mcp::ASKING)),
            Outcome::Tool(_) => Value::Nil,
        }
    }
}

/// A call, here or where the service runs, with call id `call` (made if
/// not given: the same id for each try, so the call runs once).
#[allow(clippy::too_many_arguments)]
fn make(it: &mut Interp, name: &str, f: &str, args: Vec<Value>, tenant: Option<Arc<str>>, rid: Option<Arc<str>>, call: Option<String>, line: u32) -> Outcome {
    let configured = configured_urls(&it.shared, name);
    let joined = joined_urls(name);
    if configured.is_empty() {
        // The copies that joined first, in turn, as the guide says: a copy is added to take the
        // load off the app, so while one answers the app runs none itself, and it runs a call
        // when no copy answers it (FEEDBACK32.md, Q-399; branch feedback27-35-fixes).
        if joined.is_empty() {
            return local(it, name, f, args, tenant, rid, call.unwrap_or_else(|| call_id(it)), line);
        }
        let id = call.unwrap_or_else(|| call_id(it));
        let mut sent = false;
        return match remote_hop(it, &joined, name, f, args.clone(), tenant.clone(), rid.clone(), id.clone(), None, &mut sent) {
            // In a tool's call that may ask, one sent and not answered may have run there: it is
            // not run again here, where one yes would be used twice, and it is a change
            // (`remote_hop`; FEEDBACK56.md, Part F, second review). Outside one, as before.
            Outcome::Missed(why) if sent && it.asking.is_some() => Outcome::Missed(format!("the call to {} got no answer: it may have run ({})", name, why)),
            Outcome::Missed(_) => local(it, name, f, args, tenant, rid, id, line),
            ran => ran,
        };
    }
    // The addresses the app was given and the copies that joined, all taken in turn.
    let mut urls = configured;
    urls.extend(joined.into_iter().filter(|u| !urls.contains(u)).collect::<Vec<_>>());
    remote(it, &urls, name, f, args, tenant, rid, call.unwrap_or_else(|| call_id(it)))
}

/// The first character of the id of a call made in a tool's call that has no asking
/// (`Interp::in_tool`): the service it reaches runs it as part of that call, a question refused
/// there, as `carry_in` has it in this process. The id is signed with the call, and a service of a
/// Quillo before tools takes it as any id, the body still a list of arguments: such a service has
/// no `confirm` to answer (FEEDBACK56.md, the coverage review's security review; branch
/// feedback56). Before, such a call sent an asking in an object, which that service refused.
const UNSEEN_CALL: char = 'u';

/// A new call's id: `UNSEEN_CALL` first in a tool's call that has no asking.
fn call_id(it: &Interp) -> String {
    match it.asking.is_none() && it.in_tool {
        true => format!("{}{}", UNSEEN_CALL, new_id(15)),
        false => new_id(16),
    }
}

/// A random id of `n` hex digits. Call ids are the nonces of signed calls,
/// so they come from the system's random bytes when it gives them.
pub fn new_id(n: usize) -> String {
    match crate::crypto::id_bytes(n.div_ceil(2)) {
        Some(b) => crate::crypto::hex(&b)[..n].to_string(),
        None => (0..n).map(|_| char::from_digit(fastrand::u32(..16), 16).unwrap()).collect(),
    }
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
/// error stays one, as the answer of a call says so; a decimal stays one).
fn crossed(v: &Value) -> Value {
    from_wire(&to_wire(v))
}

/// JSON for what a service call takes and gives: as for a page's server
/// fns (`island_json`), so a decimal crosses as `{"$dec": "12.50"}`, scale
/// and all, and comes out a decimal, not text, and an error stays one.
fn to_wire(v: &Value) -> serde_json::Value {
    crate::interp::island_json(v)
}

fn from_wire(j: &serde_json::Value) -> Value {
    crate::interp::from_island_json(j)
}

/// `request` in a function a service answers with: the id of the request it
/// came from, as far as it goes with the call, and the call's own, the same
/// for each try (FEEDBACK23.md, Q-188; branch feedback23-fixes).
fn step_request(rid: Option<&str>, call: &str) -> Value {
    let mut m = Map::new();
    if let Some(id) = rid {
        m.insert("id".into(), Value::str(id));
    }
    m.insert("call".into(), Value::str(call));
    Value::map(m)
}

/// A function's value as a service's answer says it.
pub(crate) fn wire_answer(v: &Value) -> serde_json::Value {
    match v {
        // Whether the program wrote it goes along, for the caller's visitors.
        Value::Err(e, own, _) => serde_json::json!({ "err": to_wire(e), "own": own }),
        v => serde_json::json!({ "ok": to_wire(v) }),
    }
}

/// The value in such an answer.
pub(crate) fn from_answer(j: &serde_json::Value) -> Value {
    match j.get("err") {
        Some(e) if j["own"] == serde_json::json!(true) => Value::own_err(from_wire(e)),
        Some(e) => Value::err(from_wire(e)),
        None => from_wire(&j["ok"]),
    }
}

/// A call of the service `name`'s `f` in this process.
/// A tool's call that may ask its user carries its asking into the service's handle, and takes it
/// back: a question there is asked through the tool's door, in the same state, its place in the
/// service's file; what it changes is the call's, so that a question after it is refused; and
/// where it stopped, the call stops (`Outcome::Stopped`). Before, the service's handle had none,
/// and its `confirm` gave true with nobody asked (FEEDBACK56.md, Part F; branch feedback56).
#[allow(clippy::too_many_arguments)]
fn local(it: &mut Interp, name: &str, f: &str, args: Vec<Value>, tenant: Option<Arc<str>>, rid: Option<Arc<str>>, call: String, line: u32) -> Outcome {
    let found = it.services.loaded.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(name).map(|(_, s)| s.fork());
    let Some(mut svc) = found else {
        return Outcome::Missed(format!("service {}: not running here, and no address for it ({} or --service-url)", name, env_name(name)));
    };
    let Some(Value::Func(c)) = lookup(&svc.globals, f).filter(|_| svc.server_fns.iter().any(|n| n == f)) else {
        return Outcome::Refused(404, format!("service {}: no server fn `{}`", name, f));
    };
    let args: Vec<Value> = args.iter().map(crossed).collect();
    let args = match crate::server::take_args(&mut svc, &c.def, args) {
        Ok(a) => a,
        Err(msg) => return Outcome::Refused(400, format!("service {}: {}", name, msg)),
    };
    svc.tenant = Some(tenant.clone());
    svc.tenant_named = tenant.is_some();
    svc.current_request = Some(step_request(rid.as_deref(), &call));
    svc.request_id = rid;
    let err = |msg: String| Outcome::Refused(500, msg);
    let Some(carried) = carry_in(it, &mut svc, name, it.in_tool || call.starts_with(UNSEEN_CALL)) else { return Outcome::Stopped };
    let r = svc.call_closure(&c, args.into_iter().map(Some).collect(), line);
    let label = format!("{}.{}", name, f);
    match (carry_back(it, &mut svc, carried, name, &label), r) {
        // Stopped to ask, or at a question refused there, said in its log as a builtin's error is.
        (Back::Stopped { failed }, r) => {
            if let (true, Err(Ctrl::Error(e))) = (failed, &r) {
                eprintln!("quillo: service {}: error in `{}`: {:#}", name, f, e);
            }
            Outcome::Stopped
        }
        (_, Ok(v) | Err(Ctrl::Return(v))) => match v {
            Value::Response(_) => err(format!("service {}: `{}` answered with a page, not a value", name, f)),
            // Whether the program wrote it goes along, as it does over the wire.
            Value::Err(e, true, _) => Outcome::Ran(Value::own_err(crossed(&e))),
            v => Outcome::Ran(crossed(&v)),
        },
        (_, Err(Ctrl::Reply(_))) => err(format!("service {}: `{}` replied with a page, not a value", name, f)),
        (_, Err(Ctrl::Error(e))) => {
            eprintln!("quillo: service {}: error in `{}`: {:#}", name, f, e);
            err(format!("service {}: internal error in `{}`", name, f))
        }
    }
}

/// `f` run in `it` with the asking of a tool's call, `asking`, which is taken back whatever
/// happens, a panic too: the handle answers other requests after, which must not run as that
/// call's (FEEDBACK56.md, Part F; branch feedback56).
fn with_asking<T>(it: &mut Interp, asking: Option<Box<crate::mcp::Asking>>, f: impl FnOnce(&mut Interp) -> T) -> (T, Option<Box<crate::mcp::Asking>>) {
    let was = std::mem::replace(&mut it.asking, asking);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(it)));
    let back = std::mem::replace(&mut it.asking, was);
    match r {
        Ok(v) => (v, back),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Whether a call carried the asking of the tool's call it is made in (FEEDBACK56.md, Part F;
/// branch feedback56).
struct Carried(bool);

/// How a call that carried an asking came back.
enum Back {
    /// It carried none.
    None,
    Ran,
    /// It stopped there; `failed`: at a question refused, or a refusal of its own.
    Stopped { failed: bool },
}

/// The asking of the tool's call `it` makes, given to the handle on the service `name`, `svc`, for
/// its call: what the signed call sends (`Asking::for_service`), so that the service runs the same
/// in this process as apart, and reads only the answers to its own questions.
/// None where it could not be given: the call stops, and the service is not run with none
/// (second review).
fn carry_in(it: &mut Interp, svc: &mut Interp, name: &str, unseen: bool) -> Option<Carried> {
    let Some(a) = it.asking.as_ref() else {
        // A call made in a tool's call with no asking (`unseen`: `Interp::in_tool`, or a call id that
        // says so, `UNSEEN_CALL`): the service runs as part of it, and refuses any question, where it
        // had answered as outside any tool's call, `confirm` true (FEEDBACK56.md, found in the
        // coverage review, and its security review; branch feedback56).
        svc.in_tool = unseen;
        return Some(Carried(false));
    };
    svc.asking = crate::mcp::Asking::from_wire(&a.for_service(name));
    if svc.asking.is_none() {
        if let Some(a) = it.asking.as_mut() {
            a.fail();
        }
        return None;
    }
    Some(Carried(true))
}

/// What `svc`'s call of `label` (`billing.charge`) of the service `name` did with the asking, taken
/// back by `it` as a signed call's reply is (`Asking::take_reply`): only what the service may say.
fn carry_back(it: &mut Interp, svc: &mut Interp, carried: Carried, name: &str, label: &str) -> Back {
    if !carried.0 {
        // Run as part of a tool's call with no asking (`carry_in`): a question it refused stops
        // this call too.
        let why = svc.asking.take().and_then(|a| a.unseen_why());
        return match why.is_some_and(|w| crate::mcp::refused_there(it, name, &w)) {
            true => Back::Stopped { failed: true },
            false => Back::None,
        };
    }
    let reply = svc.asking.take().map(|a| a.reply());
    let Some(a) = it.asking.as_mut() else { return Back::None };
    if !reply.is_some_and(|r| a.take_reply(name, label, &r)) {
        a.fail();
    }
    if a.stopped() {
        Back::Stopped { failed: a.failed() }
    } else {
        Back::Ran
    }
}

// ---- a service's tools, at its app's door (FEEDBACK56.md, Part F; branch feedback56) ----------

/// The tool `tool` of the service `name`, called at its app's door with `args` (each made to its
/// type there, or left out for its default) and `given`, the same as the client sent them: its
/// answer, `tools/call`'s result. In one process the call goes straight to the service; where it
/// runs elsewhere, over the signed service call, which offers the service's tools to its app's
/// door alone (`answer`): no plain call reaches a `fn mcp` that is not a `server fn`. Either way
/// the call is the caller's, its tenant, its request's id, `request.mcp` and its session (the
/// token or the cookie checked once, here), and runs with the call's asking, as `local` and
/// `remote_hop` carry it. A service out of reach is `internal error`, the reason in the log.
pub(crate) fn call_tool(it: &mut Interp, name: &str, tool: &crate::mcp::Tool, args: Vec<Option<Value>>, given: &serde_json::Map<String, serde_json::Value>) -> serde_json::Value {
    let configured = configured_urls(&it.shared, name);
    // Only the copies that proved they hold billing's app key (second review).
    let joined = joined_proved_urls(name);
    let out = if configured.is_empty() && joined.is_empty() {
        return local_tool(it, name, tool, args);
    } else {
        let mut urls = configured;
        let only_joined = urls.is_empty();
        urls.extend(joined.into_iter().filter(|u| !urls.contains(u)).collect::<Vec<_>>());
        let session = crate::session::current(it);
        let hop = ToolHop {
            arguments: given.clone(),
            session: to_wire(&session),
            owner: crate::session::current_owner(it),
            client: to_wire(&mcp_client(it)),
        };
        let tenant = it.tenant();
        let rid = it.request_id.clone();
        let mut sent = false;
        match remote_hop(it, &urls, name, tool.own_name(), Vec::new(), tenant, rid, new_id(16), Some(&hop), &mut sent) {
            // The copies that joined first, and this process when none could be reached (`make`).
            // One sent that got no answer may have run there: never run again here, where it
            // would charge twice (FEEDBACK56.md, Part F, security review).
            Outcome::Missed(_) if only_joined && !sent => return local_tool(it, name, tool, args),
            Outcome::Missed(why) if sent => Outcome::Missed(format!("the call to {} got no answer: it may have run ({})", name, why)),
            out => out,
        }
    };
    match out {
        Outcome::Tool(r) => r,
        // Its answer is the question, or the refusal, the asking it gave back holds (`mcp::run`).
        Outcome::Stopped => crate::mcp::tool_error("internal error"),
        Outcome::Missed(why) | Outcome::Refused(_, why) => {
            eprintln!("quillo: error in tool `{}`: {}", tool.name, why);
            crate::mcp::tool_error("internal error")
        }
        Outcome::Ran(_) => {
            eprintln!("quillo: error in tool `{}`: service {} answered a tool's call as a server fn's", tool.name, name);
            crate::mcp::tool_error("internal error")
        }
    }
}

/// `request.mcp` of the request being answered: who the client says it is, or null.
fn mcp_client(it: &mut Interp) -> Value {
    match it.request_value() {
        Some(Value::Map(m)) => m.get("mcp").cloned().unwrap_or(Value::Nil),
        _ => Value::Nil,
    }
}

/// `request` in a service's tool: as a service call's (`step_request`), and `mcp`, who the client
/// says it is, as at its app's door.
fn tool_request(rid: Option<&str>, call: &str, client: Value) -> Value {
    let mut m = match step_request(rid, call) {
        Value::Map(m) => (*m).clone(),
        _ => Map::new(),
    };
    m.insert("mcp".into(), client);
    Value::map(m)
}

/// `call_tool` in this process: the tool run in a handle on the service, as its own door runs one
/// (`mcp::answer_tool`), the call's asking carried in and back. What it sets in a cookie or the
/// session goes nowhere: the caller's cookies are its app's.
fn local_tool(it: &mut Interp, name: &str, tool: &crate::mcp::Tool, args: Vec<Option<Value>>) -> serde_json::Value {
    let found = it.services.loaded.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(name).map(|(_, s)| s.fork());
    let Some(mut svc) = found else {
        eprintln!("quillo: error in tool `{}`: service {}: not running here, and no address for it ({} or --service-url)", tool.name, name, env_name(name));
        return crate::mcp::tool_error("internal error");
    };
    let tenant = it.tenant();
    svc.tenant_named = tenant.is_some();
    svc.tenant = Some(tenant);
    let rid = it.request_id.clone();
    svc.current_request = Some(tool_request(rid.as_deref(), &new_id(16), mcp_client(it)));
    svc.request_id = rid;
    let session = crate::session::current(it);
    let owner = crate::session::current_owner(it);
    crate::session::take_on(&mut svc, session, owner.as_deref().unwrap_or(""));
    svc.answering = true;
    let in_tool = it.in_tool;
    let Some(carried) = carry_in(it, &mut svc, name, in_tool) else { return crate::mcp::tool_error("internal error") };
    let r = crate::mcp::answer_tool(&mut svc, tool, args);
    carry_back(it, &mut svc, carried, name, &tool.name);
    r
}

/// The functions of each service that are safe to call twice, as its file says: they use
/// `idempotent` (FEEDBACK49.md, Q-595; branch main).
fn repeatable() -> &'static Mutex<HashSet<(String, String)>> {
    static R: OnceLock<Mutex<HashSet<(String, String)>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

/// Each service's functions as this program's copy of it declares them
/// (`describe::contract`): sent with each call, so a service deployed apart
/// from its caller says when they disagree (FEEDBACK10.md, Q-077; branch
/// feedback10-fixes).
fn contracts() -> &'static Mutex<HashMap<(String, String), String>> {
    static C: OnceLock<Mutex<HashMap<(String, String), String>>> = OnceLock::new();
    C.get_or_init(Default::default)
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

/// The signature of a call: the time, the service it is for, the function,
/// the tenant, the call id (which the service remembers, so the same call
/// sent again is not run again) and the arguments, under the shared secret.
pub fn signature(secret: &str, t: u64, to: &str, f: &str, tenant: &str, call: &str, body: &[u8]) -> String {
    let mut msg = format!("quillo-service-2\n{}\n{}\n{}\n{}\n{}\n", t, to, f, tenant, call).into_bytes();
    msg.extend_from_slice(body);
    crate::crypto::hex(&crate::crypto::hmac_sha256(secret.as_bytes(), &msg))
}

// ---- channels across processes (FEEDBACK12.md, Q-087; branch open-fixes) ----

/// Under `--split`, where the app listens, for its services' processes: what
/// they broadcast on a channel goes to the app's pages through it.
static APP_URL: OnceLock<String> = OnceLock::new();

/// A signed POST between the app and a service's process, for channels:
/// `to` the service (or "" for the app), `f` what it is for. The answer's body.
pub fn signed_post(base: &str, path: &str, to: &str, f: &str, body: &[u8]) -> Result<Vec<u8>, String> {
    let secret = secret().ok_or("QUILLO_SERVICE_SECRET is not set")?;
    let t = now_secs();
    let nonce = new_id(16);
    let sig = signature(&secret, t, to, f, "", &nonce, body);
    let r = agent()
        .post(&format!("{}{}", base.trim_end_matches('/'), path))
        .timeout(Duration::from_secs(10))
        .set("content-type", "application/json")
        .set("x-quillo-service", &format!("t={},sig={}", t, sig))
        .set("x-quillo-call", &nonce)
        .set("x-quillo-to", to)
        .send_bytes(body);
    match r {
        Ok(resp) => {
            let mut out = Vec::new();
            resp.into_reader().read_to_end(&mut out).map_err(|e| e.to_string())?;
            Ok(out)
        }
        Err(ureq::Error::Status(code, resp)) => Err(format!("{} {}", code, resp.into_string().unwrap_or_default().trim())),
        Err(e) => Err(e.to_string()),
    }
}

/// Whether `req` was signed by `signed_post` for `f`, lately, with the shared
/// secret; and, when `named`, for this process's `--service-name` (what the
/// app sends a service), else for the app ("", what a service sends it).
pub fn signed_for(req: &HttpRequest, f: &str, named: bool) -> Result<(), HttpResponse> {
    if secret().is_none() {
        return Err(refused(403, "channels between processes need QUILLO_SERVICE_SECRET, on both sides"));
    }
    let header = |name: &str| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    let sig = header("x-quillo-service").unwrap_or("");
    let t = sig.split(',').find_map(|p| p.strip_prefix("t=")).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let given = sig.split(',').find_map(|p| p.strip_prefix("sig=")).unwrap_or("");
    let to = header("x-quillo-to").unwrap_or("");
    let c = header("x-quillo-call").unwrap_or("");
    let ok = shared_signed(given, |k| signature(k, t, to, f, "", c, &req.body)).is_some();
    if req.method != "POST" || now_secs().abs_diff(t) > WINDOW || !ok {
        return Err(refused(401, "not signed with this server's QUILLO_SERVICE_SECRET (or too old)"));
    }
    if (named && SERVED_AS.get().is_some_and(|me| *me != to)) || (!named && !to.is_empty()) {
        return Err(refused(401, "this is for another process"));
    }
    Ok(())
}

/// In a service's process started by `--split`: where its app listens.
pub fn app_url() -> Option<&'static str> {
    static U: OnceLock<Option<String>> = OnceLock::new();
    U.get_or_init(|| std::env::var("QUILLO_APP_URL").ok().filter(|u| !u.is_empty())).as_deref()
}

/// How one try went.
enum Try {
    /// The status, the body, and the answer's signature, if it has one, and its signature with
    /// the app's key, for a tool's call (FEEDBACK56.md, Part F).
    Answer(u16, String, Option<String>, Option<String>),
    /// No connection: the call was not sent.
    NotSent(String),
    /// Sent, but no answer (in time): it may have run.
    NoAnswer(String),
}

/// The name a call says it comes from: the key's name, this process's service name, or `app`.
fn caller_name(a: &crate::svcauth::Auth) -> String {
    a.key.as_ref().map(|(me, _)| me.clone()).or_else(|| served_as().map(str::to_string)).unwrap_or_else(|| "app".into())
}

/// The visitor's headers a call passes on (`forward`), from the request it is made in.
fn forwarded(it: &mut Interp, names: &[String]) -> Vec<(String, String)> {
    if names.is_empty() {
        return Vec::new();
    }
    let Some(Value::Map(r)) = it.request_value() else { return Vec::new() };
    let Some(Value::Map(h)) = r.get("headers") else { return Vec::new() };
    // What comes back may be theirs (FEEDBACK55.md, Q-886; branch feedback55).
    it.visitor_read = true;
    names.iter().filter_map(|n| h.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| (format!("x-quillo-forward-{}", n), v.display()))).collect()
}

/// A call to where the service runs: tried at its addresses in turn (one
/// left alone when it keeps failing) until one answers, as often as its
/// options say. A try that got no connection, or a busy or missing server
/// (502, 503, 504), is made again elsewhere; one sent but not answered in
/// time, again at the same address, with the same call id, which that
/// server remembers, so the function runs once. It proves who makes it as
/// the declaration's `auth` says (src/svcauth.rs).
#[allow(clippy::too_many_arguments)]
fn remote(it: &mut Interp, urls: &[String], name: &str, f: &str, args: Vec<Value>, tenant: Option<Arc<str>>, rid: Option<Arc<str>>, call: String) -> Outcome {
    remote_hop(it, urls, name, f, args, tenant, rid, call, None, &mut false)
}

/// A call of a service's tool, made by its app's door (FEEDBACK56.md, Part F; branch feedback56):
/// its arguments as the client sent them, by name, and who the call is made by, the session's
/// data and its owner, and the client, so that the token or the cookie is checked once, at the
/// app, and `session()` and `request.mcp` in the tool are the caller's.
pub(crate) struct ToolHop {
    arguments: serde_json::Map<String, serde_json::Value>,
    session: serde_json::Value,
    owner: Option<String>,
    client: serde_json::Value,
}

/// `remote`, of a tool (`tool`) or of a server fn, with the asking of the tool's call it is made
/// in, where there is one, sent along in the signed body (the service's part of it,
/// `Asking::for_service`) and its reply taken back from the signed answer, whatever the service
/// answered (FEEDBACK56.md, Part F; branch feedback56): a question there is asked through the
/// tool's door, and a change there is the call's. Only an answer signed back, carrying the reply,
/// says what it did; any other answer from a call that was sent (refused, unsigned, not JSON, a
/// 500, none in time) may have come after it ran, and is a change, so that no question comes
/// after it (security review). `sent`: some try of it was sent.
#[allow(clippy::too_many_arguments)]
fn remote_hop(it: &mut Interp, urls: &[String], name: &str, f: &str, args: Vec<Value>, tenant: Option<Arc<str>>, rid: Option<Arc<str>>, call: String, tool: Option<&ToolHop>, sent: &mut bool) -> Outcome {
    let carried = it.asking.as_ref().map(|a| a.for_service(name));
    let mut back = None;
    let out = remote_tries(it, urls, name, f, args, tenant, rid, call, tool, carried.as_ref(), &mut back, sent);
    // Made in a tool's call with no asking (`UNSEEN_CALL`): a question the service refused stops
    // this call too, as its signed answer says.
    if carried.is_none() {
        if let Some(why) = back.as_ref().and_then(|b| b.get("refused")).and_then(serde_json::Value::as_str) {
            crate::mcp::refused_there(it, name, why);
        }
    }
    let (Some(_), Some(a)) = (carried, it.asking.as_mut()) else { return out };
    let label = format!("{}.{}", name, f);
    match back {
        Some(j) => {
            if !a.take_reply(name, &label, &j) {
                eprintln!("quillo: service {}: what `{}` gave back of the tool's call is not what Quillo sends, so the call stops", name, f);
                a.fail();
            }
        }
        None if *sent => a.changed(format!("a call of `{}` whose answer does not say what it did, which may have run", label)),
        None => {}
    }
    if a.stopped() {
        return Outcome::Stopped;
    }
    out
}

/// What a service's app keeps the key its tools' calls are signed with in, for the service
/// `name`: `QUILLO_APP_KEY_BILLING`, the service keeping the same as `QUILLO_APP_KEY` (`--split`
/// sets both; FEEDBACK56.md, Part F, security review).
pub fn app_key_env(name: &str) -> String {
    format!("QUILLO_APP_KEY_{}", name.to_ascii_uppercase())
}

/// What a service keeps its app's key in (`app_key_env`).
pub const APP_KEY: &str = "QUILLO_APP_KEY";

/// The signature of a tool's call by its app's door, under the app's key: the time, the service,
/// the tool, the tenant, the call's id and the body.
/// Everything that decides what runs or whom it is for is signed: the caller's name, the deadline,
/// and the contract and the visitor's headers passed on (`covered`, as a version 3 signature
/// covers them), so that whoever holds the services' shared secret can neither re-send it under
/// another caller's name nor change any of it (FEEDBACK56.md, Part F, second review).
#[allow(clippy::too_many_arguments)]
pub fn sign_app(key: &str, t: u64, to: &str, f: &str, tenant: &str, call: &str, caller: &str, deadline: &str, covered: &str, body: &[u8]) -> String {
    let mut msg = format!("quillo-app-2\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n", t, to, f, tenant, call, caller, deadline, covered).into_bytes();
    msg.extend_from_slice(body);
    crate::crypto::hex(&crate::crypto::hmac_sha256(key.as_bytes(), &msg))
}

/// The fewest bytes an app's key has: a secret as long as `--split` makes one (32 random bytes,
/// written as 64 hex digits), so that it cannot be guessed (second review).
pub const APP_KEY_LEAST: usize = 32;

/// An app's key in `var`, and the one before it in `var_PREVIOUS`, taken while keys are changed
/// machine by machine and never signing (second review): None where `var` is not set; an error
/// where either is shorter than `APP_KEY_LEAST`.
fn app_keys_in(var: &str, read: &dyn Fn(&str) -> Option<String>) -> Result<Option<(String, Option<String>)>, String> {
    let get = |v: &str| read(v).filter(|k| !k.is_empty());
    let Some(key) = get(var) else { return Ok(None) };
    let previous_var = format!("{}_PREVIOUS", var);
    let previous = get(&previous_var);
    for (v, k) in [(var, Some(&key)), (previous_var.as_str(), previous.as_ref())] {
        if let Some(k) = k.filter(|k| k.len() < APP_KEY_LEAST) {
            return Err(format!("{} is {} bytes: an app's key is a secret of at least {} bytes (`quillo run --split` makes one of 64)", v, k.len(), APP_KEY_LEAST));
        }
    }
    Ok(Some((key, previous)))
}

/// The app's keys for its service `name`: `QUILLO_APP_KEY_BILLING`, and its `_PREVIOUS`.
fn app_keys(name: &str) -> Result<Option<(String, Option<String>)>, String> {
    app_keys_in(&app_key_env(name), &|v| std::env::var(v).ok())
}

/// This service's keys of its app's: `QUILLO_APP_KEY`, and its `_PREVIOUS`, from its environment or
/// its `.env`.
fn own_app_keys(it: &Interp) -> Result<Option<(String, Option<String>)>, String> {
    app_keys_in(APP_KEY, &|v| crate::dotenv::var(&it.env_file, v))
}

/// Whether `given` (`t=...,sig=...`) signs `sign(key, t)` with one of `keys`, lately.
fn signed_by_any(given: &str, keys: &(String, Option<String>), sign: &dyn Fn(&str, u64) -> String) -> bool {
    let g = crate::svcauth::signed(given);
    !g.sig.is_empty() && now_secs().abs_diff(g.t) <= WINDOW && [Some(&keys.0), keys.1.as_ref()].into_iter().flatten().any(|k| crate::crypto::same(sign(k, g.t).as_bytes(), g.sig.as_bytes()))
}

/// Whether a call's body is a tool's call by the app's door (`ToolHop`).
fn tool_call_body(body: &[u8]) -> bool {
    body.first() == Some(&b'{') && matches!(crate::value::json_value(body), Ok(serde_json::Value::Object(m)) if m.contains_key("tool") && m.contains_key("mcp"))
}

/// A tool's call, checked before anything is answered, a remembered answer too: signed with this
/// service's app's key, all that decides it covered (`sign_app`), lately. The key its answer is
/// signed back with; else the refusal.
fn from_its_app(it: &Interp, req: &HttpRequest, f: &str, tenant: &str) -> Result<String, HttpResponse> {
    let header = |name: &str| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    let to = header("x-quillo-to").unwrap_or("");
    let keys = match own_app_keys(it) {
        Ok(Some(k)) => k,
        Ok(None) => return Err(refused(403, &no_app_key(to))),
        Err(why) => return Err(refused(403, &why)),
    };
    let (call, caller, deadline) = (header("x-quillo-call").unwrap_or(""), header("x-quillo-caller").unwrap_or(""), header("x-quillo-deadline").unwrap_or(""));
    let covered = format!("{}\n{}", header("x-quillo-contract").unwrap_or(""), crate::svcauth::forwarded_text(req.headers.iter().map(|(k, v)| (k.as_str(), v.as_str()))));
    if !signed_by_any(header("x-quillo-app").unwrap_or(""), &keys, &|k, t| sign_app(k, t, to, f, tenant, call, caller, deadline, &covered, &req.body)) {
        return Err(refused(401, "a tool's call is taken from this service's app alone, signed with its key (QUILLO_APP_KEY): this one is not, or is too old"));
    }
    Ok(keys.0)
}

/// `r`, a tool's answer, signed back with the app's key `key` for the call `call`.
fn app_signed(mut r: HttpResponse, key: &str, call: &str) -> HttpResponse {
    r.headers.retain(|(h, _)| h != "x-quillo-app-answer");
    let sig = crate::svcauth::sign_answer(key, call, r.status, &r.body);
    r.headers.push(("x-quillo-app-answer".into(), format!("sig={}", sig)));
    r
}

/// What a call of a service's tool is refused with where the service has no key of its app's.
pub fn no_app_key(name: &str) -> String {
    format!(
        "this service runs its tools only for its app's door, whose calls of them are signed with the app's key: set {} here and {} where the app runs, to the same secret (`quillo run --split` sets both)",
        APP_KEY,
        app_key_env(name)
    )
}

/// What the app is told of a service that runs a Quillo before this one: it refuses a call made in
/// a tool's call, which it cannot read (FEEDBACK56.md, Part F, security review).
pub fn older_quillo(name: &str) -> String {
    format!("service {}: it runs an older Quillo, which refuses a call made in a tool's call (one that may ask the person): upgrade {} before the app", name, name)
}

/// `remote_hop`'s tries: what it sends, the reply a verified answer gave back (`back`), and
/// whether a try was sent (`sent`).
#[allow(clippy::too_many_arguments)]
fn remote_tries(
    it: &mut Interp,
    urls: &[String],
    name: &str,
    f: &str,
    args: Vec<Value>,
    tenant: Option<Arc<str>>,
    rid: Option<Arc<str>>,
    call: String,
    tool: Option<&ToolHop>,
    carried: Option<&serde_json::Value>,
    back: &mut Option<serde_json::Value>,
    sent: &mut bool,
) -> Outcome {
    let a = crate::svcauth::auth_of(name);
    // A tool's call is signed with its app's key too, and its answer signed back with it: only
    // the app's door runs a service's tools (FEEDBACK56.md, Part F, security review).
    let app_key = match tool {
        Some(_) => match app_keys(name) {
            Ok(Some(k)) => Some(k),
            Ok(None) => return Outcome::Refused(403, format!("service {}: {} is not set, the key its tools' calls are signed with (`quillo run --split` sets it)", name, app_key_env(name))),
            Err(why) => return Outcome::Refused(403, format!("service {}: {}", name, why)),
        },
        None => None,
    };
    let shared = secret();
    // What signs the call: its own key, else the shared secret (FEEDBACK38.md, Q-458; branch
    // feedback36-39). A token or a certificate alone proves it too.
    // A call that proves itself with a token is not also signed with the shared secret, which the
    // callee would take first, and the token would never be looked at.
    let token = a.bearer.is_some() || a.oauth.is_some();
    let (sign_key, kid) = match (&a.key, &shared) {
        (Some((me, k)), _) => (Some(k.clone()), me.clone()),
        (None, Some(s)) if !token => (Some(s.clone()), String::new()),
        _ => (None, String::new()),
    };
    if sign_key.is_none() && a.bearer.is_none() && a.oauth.is_none() && a.mtls.is_none() {
        return Outcome::Missed(format!("service {}: runs at {}, but QUILLO_SERVICE_SECRET is not set to sign the call", name, urls.join(",")));
    }
    let bearer = match &a.bearer {
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => match it.call(f, Vec::new(), 0) {
            Ok(Value::Str(t)) if !t.is_empty() => Some(t.to_string()),
            Ok(Value::Err(e, ..)) => return Outcome::Missed(format!("service {}: its `bearer` gave no token: {}", name, e.display())),
            Ok(other) => return Outcome::Missed(format!("service {}: its `bearer` gives a token as text, not {}", name, short(&other))),
            Err(Ctrl::Error(e)) => return Outcome::Missed(format!("service {}: its `bearer` failed: {}", name, e.msg)),
            Err(_) => return Outcome::Missed(format!("service {}: its `bearer` gave no token", name)),
        },
        Some(Value::Str(t)) => Some(t.to_string()),
        _ => None,
    };
    let agent = match crate::svcauth::agent_for(a.mtls.as_ref()) {
        Ok(ag) => ag,
        Err(e) => return Outcome::Missed(format!("service {}: its `mtls`: {}", name, e)),
    };
    let passed = forwarded(it, &a.forward);
    let o = opts(name);
    // A call that carries a tool's call sends an object, which a service of a Quillo before this
    // one refuses rather than runs without it (FEEDBACK56.md, Part F; branch feedback56).
    let listed = serde_json::Value::Array(args.iter().map(to_wire).collect());
    let body = match (tool, carried) {
        (Some(t), _) => serde_json::json!({ "tool": f, "arguments": t.arguments, "mcp": { "asking": carried, "session": t.session, "owner": t.owner, "client": t.client } }),
        (None, Some(asking)) => serde_json::json!({ "args": listed, "mcp": { "asking": asking } }),
        (None, None) => listed,
    }
    .to_string()
    .into_bytes();
    let tenant = tenant.map(|t| t.to_string()).unwrap_or_default();
    let rid = rid.map(|r| r.to_string()).unwrap_or_default();
    let caller = caller_name(&a);
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
        // No longer than the call this one is part of may still wait
        // (Q-056): a chain gives up together, not each link on its own.
        let left = it.deadline.map(|d| d.saturating_duration_since(Instant::now()));
        if left.is_some_and(|l| l.is_zero()) {
            return Outcome::Missed(format!("service {}: not called: the call this one is part of has run out of time", name));
        }
        let (timeout, theirs) = match left {
            Some(l) if l < o.timeout => (l, true),
            _ => (o.timeout, false),
        };
        let t = now_secs();
        let deadline = timeout.as_millis().to_string();
        let mine = contracts().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&(name.to_string(), f.to_string())).cloned().unwrap_or_default();
        let covered = format!("{}\n{}", mine, crate::svcauth::forwarded_text(passed.iter().map(|(k, v)| (k.as_str(), v.as_str()))));
        let sig = sign_key.as_ref().map(|k| format!("v=3,kid={},t={},sig={}", kid, t, crate::svcauth::sign3(k, t, name, f, &tenant, &call, &kid, &caller, &deadline, &covered, &body)));
        let app_sig = app_key.as_ref().map(|(k, _)| format!("t={},sig={}", t, sign_app(k, t, name, f, &tenant, &call, &caller, &deadline, &covered, &body)));
        let endpoint = format!("{}/_quillo/service/{}", url, f);
        let (b, tn, r, c, cl, to, app) = (body.clone(), tenant.clone(), rid.clone(), call.clone(), caller.clone(), name.to_string(), app_sig.clone());
        let (ag, oauth, bearer, passed) = (agent.clone(), a.oauth.clone(), bearer.clone(), passed.clone());
        let got = it.unlocked(move || -> Try {
            let send = |token: Option<&str>| -> Try {
                let mut rq = ag
                    .post(&endpoint)
                    .timeout(timeout)
                    .set("content-type", "application/json")
                    .set("x-quillo-to", &to)
                    .set("x-quillo-call", &c)
                    .set("x-quillo-caller", &cl)
                    .set("x-quillo-deadline", &deadline);
                if let Some(s) = &sig {
                    rq = rq.set("x-quillo-service", s);
                }
                if let Some(s) = &app {
                    rq = rq.set("x-quillo-app", s);
                }
                if let Some(tk) = token {
                    rq = rq.set("authorization", &format!("Bearer {}", tk));
                }
                for (k, v) in &passed {
                    rq = rq.set(k, v);
                }
                if !mine.is_empty() {
                    rq = rq.set("x-quillo-contract", &mine);
                }
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
                let signed = resp.header("x-quillo-answer").map(str::to_string);
                let by_app = resp.header("x-quillo-app-answer").map(str::to_string);
                let mut text = String::new();
                match resp.into_reader().take(64 << 20).read_to_string(&mut text) {
                    Ok(_) => Try::Answer(status, text, signed, by_app),
                    Err(e) => Try::NoAnswer(e.to_string()),
                }
            };
            match &oauth {
                // A token fetched by OAuth, kept until it nearly expires; one refused is
                // fetched again, once.
                Some(o) => {
                    let token = match crate::svcauth::oauth_token(o, false) {
                        Ok(t) => t,
                        Err(e) => return Try::NotSent(e),
                    };
                    match send(Some(&token)) {
                        Try::Answer(401, ..) => match crate::svcauth::oauth_token(o, true) {
                            Ok(t) => send(Some(&t)),
                            Err(e) => Try::NotSent(e),
                        },
                        other => other,
                    }
                }
                None => send(bearer.as_deref()),
            }
        });
        match got {
            Try::Answer(status @ (502..=504), text, ..) => {
                went_wrong(&url, &o);
                *sent = true;
                why = format!("service {}: {} at {} ({})", name, status, url, reason(&text));
                // A proxy's 502 or 504 may come after the copy behind it ran the call: only a
                // function that is safe to run twice goes to another copy, where it ran on both
                // (FEEDBACK49.md, Q-595; branch main). A 503 was not taken.
                if status != 503 && !repeatable().lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(&(name.to_string(), f.to_string())) {
                    log_call(name, f, &url, status, start, attempt + 1, &rid);
                    // Refused, not missed: a call missed is run here when the service's only
                    // copies are ones that joined, and this one may have run there (found in
                    // review).
                    return Outcome::Refused(status, format!("{}; not sent to another copy, as it may have run there (a function that is safe to run twice uses `idempotent`)", why));
                }
                // A call still running there (503) is asked of it again.
                if status != 503 || same.is_none() {
                    tried.push(url);
                    same = None;
                }
            }
            Try::Answer(status, text, signed, by_app) => {
                went_well(&url);
                *sent = true;
                log_call(name, f, &url, status, start, attempt + 1, &rid);
                // The answer is the service's: signed back with the key the call was signed
                // with. A call with a key of its own takes no answer without it (Q-459), nor one
                // under the shared secret, where an unsigned answer from whatever was at the
                // service's address was taken (FEEDBACK49.md, Q-596; branch main).
                let sig_of = |given: &Option<String>, k: &str| {
                    let want = crate::svcauth::sign_answer(k, &call, status, text.as_bytes());
                    given.as_deref().map(|s| s.strip_prefix("sig=").unwrap_or(s)).map(|s| crate::crypto::same(s.as_bytes(), want.as_bytes()))
                };
                let mut verified = false;
                if let Some(k) = &sign_key {
                    let good = sig_of(&signed, k);
                    if good == Some(false) || (good.is_none() && status == 200) {
                        return Outcome::Refused(502, format!("service {}: the answer from {} is not signed by the service", name, url));
                    }
                    verified = good == Some(true);
                }
                // A tool's answer, signed back with the app's key too (FEEDBACK56.md, Part F,
                // security review).
                if let Some((k, previous)) = &app_key {
                    let good = [Some(k), previous.as_ref()].into_iter().flatten().any(|k| sig_of(&by_app, k) == Some(true));
                    if !good && status == 200 {
                        return Outcome::Refused(502, format!("service {}: the answer from {} is not signed with the app's key ({})", name, url, app_key_env(name)));
                    }
                    verified &= good;
                }
                // As deep as every reader takes (FEEDBACK55.md, Q-875's second review; branch
                // feedback55), as the bodies below.
                let read = crate::value::json_value(text.as_bytes()).ok();
                // The tool's call's asking, as the service's verified answer gave it back, whatever
                // its status: from no other is it taken (FEEDBACK56.md, Part F, security review).
                *back = read.as_ref().filter(|_| carried.is_some() && verified).and_then(|j| j.get("asking")).filter(|a| a.is_object()).cloned();
                // A call made in a tool's call with no asking: the question the service refused, as
                // its signed answer says (`UNSEEN_CALL`).
                let unseen = carried.is_none() && tool.is_none() && call.starts_with(UNSEEN_CALL);
                if unseen && verified {
                    *back = read.as_ref().and_then(|j| j.get("refused")).filter(|r| r.is_string()).map(|r| serde_json::json!({ "refused": r }));
                }
                let answer = read.filter(|_| status == 200);
                // What the call published there is announced here too, for
                // pages of this program watching it (FEEDBACK4.md, Q-038;
                // branch bank-fixes).
                if let Some(topics) = answer.as_ref().and_then(|j| j["published"].as_array()) {
                    let topics: Vec<String> = topics.iter().filter_map(|t| t.as_str().map(String::from)).collect();
                    if !topics.is_empty() {
                        it.publish(topics);
                    }
                }
                return match (status, answer) {
                    (200, Some(j)) if tool.is_some() && j.get("tool").is_some_and(|t| t.is_object()) => Outcome::Tool(j["tool"].clone()),
                    (200, Some(j)) if (carried.is_some() || unseen) && verified && j.get("stopped") == Some(&serde_json::json!(true)) => Outcome::Stopped,
                    (200, Some(j)) if j.get("ok").is_some() || j.get("err").is_some() => Outcome::Ran(from_answer(&j)),
                    (200, _) => Outcome::Refused(200, format!("service {}: an answer that is not one", name)),
                    // An older Quillo takes no object for a body (FEEDBACK56.md, Part F, security
                    // review): said so, not as a refusal of the arguments.
                    (400, _) if (tool.is_some() || carried.is_some()) && reason(&text) == "the body must be a JSON array of arguments" => Outcome::Refused(400, older_quillo(name)),
                    _ => Outcome::Refused(status, format!("service {}: {} {}", name, status, reason(&text))),
                };
            }
            Try::NotSent(e) => {
                went_wrong(&url, &o);
                why = format!("service {}: unreachable at {} ({})", name, url, e);
                tried.push(url);
            }
            Try::NoAnswer(e) => {
                went_wrong(&url, &o);
                *sent = true;
                why = if e.contains("timed out") {
                    let secs = timeout.as_secs_f64();
                    format!("service {}: took more than {} s ({}), at {}", name, (secs * 1000.0).round() / 1000.0, if theirs { "the time its caller had left" } else { "its timeout" }, url)
                } else {
                    format!("service {}: no answer from {} ({})", name, url, e)
                };
                same = Some(url);
                if theirs {
                    break;
                }
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
/// program's server fns. Only a call that proves who makes it is answered:
/// signed in the last few minutes with the shared secret or the caller's own
/// key, for this service, or with a token or a certificate `accept_calls`
/// takes (src/svcauth.rs); and only once (its call id is remembered for
/// longer than a signature lasts); it runs in the tenant it names.
pub fn answer(it: &mut Interp, f: &str, req: &HttpRequest) -> HttpResponse {
    if req.method != "POST" {
        return refused(405, "a service call is a POST");
    }
    let accept = it.accept.clone();
    if secret().is_none() && accept.is_none() {
        return refused(403, "service calls need QUILLO_SERVICE_SECRET, on both sides");
    }
    let header = |name: &str| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    let s = crate::svcauth::signed(header("x-quillo-service").unwrap_or(""));
    let tenant = header("x-quillo-tenant").unwrap_or("");
    let to = header("x-quillo-to").unwrap_or("");
    let Some(c) = header("x-quillo-call").filter(|c| !c.is_empty() && c.len() <= 64 && c.bytes().all(|b| b.is_ascii_alphanumeric())) else {
        return refused(401, "a service call carries its call id (x-quillo-call)");
    };
    let (said_caller, deadline, contract) = (header("x-quillo-caller").unwrap_or(""), header("x-quillo-deadline").unwrap_or(""), header("x-quillo-contract").unwrap_or(""));
    // Who called, and the key its answer is signed with (FEEDBACK38.md, Q-458 to Q-462; branch
    // feedback36-39).
    let mut caller: Option<Value> = None;
    let mut answer_key: Option<String> = None;
    let mut why = if secret().is_some() {
        "not signed with this server's QUILLO_SERVICE_SECRET (or too old)".to_string()
    } else {
        "this call proves no caller: a key, a token or a certificate accept_calls takes".to_string()
    };
    if !s.sig.is_empty() {
        let fresh = now_secs().abs_diff(s.t) <= WINDOW;
        let covered = format!("{}\n{}", contract, crate::svcauth::forwarded_text(req.headers.iter().map(|(k, v)| (k.as_str(), v.as_str()))));
        let v3 = |k: &str| crate::svcauth::sign3(k, s.t, to, f, tenant, c, s.kid, said_caller, deadline, &covered, &req.body);
        if !s.kid.is_empty() {
            match accept.as_ref().and_then(|a| a.keys.get(s.kid)) {
                Some(keys) => match keys.iter().find(|k| crate::crypto::same(v3(k).as_bytes(), s.sig.as_bytes())) {
                    Some(k) if fresh => {
                        answer_key = Some(k.clone());
                        caller = Some(crate::svcauth::caller_value(s.kid, "key", Vec::new()));
                    }
                    _ => why = format!("not signed with the key of `{}` (or too old)", s.kid),
                },
                None => why = format!("no key here for caller `{}` (accept_calls {{ keys }})", s.kid),
            }
        } else if accept.as_ref().map_or(true, |a| a.shared) {
            let k = if s.v >= 3 { shared_signed(s.sig, v3) } else { shared_signed(s.sig, |k| signature(k, s.t, to, f, tenant, c, &req.body)) };
            if let (Some(k), true) = (k, fresh) {
                answer_key = Some(k);
                // Version 2 did not sign the caller's name: it is not known then.
                caller = Some(crate::svcauth::caller_value(if s.v >= 3 { said_caller } else { "" }, "quillo", Vec::new()));
            }
        } else {
            why = "this service takes no calls signed with the shared secret (accept_calls { shared: false })".into();
        }
    }
    if caller.is_none() {
        let bearer = header("authorization").and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer "))).map(str::to_string);
        match (accept.as_ref().and_then(|a| a.jwt.clone()), bearer) {
            (Some(rules), Some(token)) => match it.unlocked(move || crate::svcauth::verify_jwt(&token, &rules)) {
                Ok(claims) => match crate::svcauth::jwt_caller(&claims) {
                    Ok(c) => caller = Some(c),
                    Err(e) => why = e,
                },
                Err(e) => why = format!("the bearer token: {}", e),
            },
            (None, Some(_)) if s.sig.is_empty() => why = "a bearer token, and nothing here checks one (accept_calls { jwt })".into(),
            _ => {}
        }
    }
    if caller.is_none() {
        // The certificate a proxy checked, from a proxy `--trust-proxy` names only.
        if let Some(h) = accept.as_ref().and_then(|a| a.client_cert.clone()) {
            let from_proxy = header(crate::httpd::PROXIED).is_some();
            match header(&h) {
                Some(v) if from_proxy => match crate::svcauth::cert_identity(v) {
                    Some((name, extra)) => caller = Some(crate::svcauth::caller_value(&name, "mtls", extra)),
                    None => why = format!("`{}` names no certificate's subject", h),
                },
                Some(_) => why = format!("`{}` is believed only from a proxy --trust-proxy names", h),
                None => {}
            }
        }
    }
    let Some(mut caller) = caller else { return refused(401, &why) };
    if let Some(me) = SERVED_AS.get().filter(|me| *me != to) {
        return refused(401, &format!("this call is for service `{}`; this is `{}` (see --service-name)", to, me));
    }
    if !tenant.is_empty() && !crate::site::tenant_name_ok(tenant) {
        return refused(400, "not a tenant's name");
    }
    // Anything else, in Quillo: what `check` gives is the caller from then on.
    if let Some(check) = accept.as_ref().and_then(|a| a.check.clone()) {
        let given = match it.call(&check, vec![caller.clone()], 0) {
            Ok(v) | Err(Ctrl::Return(v)) => v,
            Err(Ctrl::Reply(r)) => return crate::server::render_response(&r),
            Err(Ctrl::Error(e)) => {
                eprintln!("quillo: error in accept_calls's check: {:#}", e);
                return refused(500, "internal error in accept_calls's check");
            }
        };
        // Only `true` or a caller accepts. Anything else did, null among them: an allow-list
        // written as an `if` with no `else` let every other caller in (FEEDBACK51.md, Q-652;
        // branch feedback51-fixes).
        match given {
            Value::Map(m) => caller = Value::Map(m),
            Value::Bool(true) => {}
            Value::Err(e, ..) => return refused(403, &e.display()),
            _ => return refused(403, "refused by accept_calls's check"),
        }
    }
    let signed_back = |r: HttpResponse| -> HttpResponse {
        let Some(k) = &answer_key else { return r };
        let mut r = r;
        r.headers.retain(|(h, _)| h != "x-quillo-answer");
        r.headers.push(("x-quillo-answer".into(), format!("sig={}", crate::svcauth::sign_answer(k, c, r.status, &r.body))));
        r
    };
    // A call sent again (it got no answer in time, is a call made later
    // delivered twice, or was replayed by someone who saw it): answered as
    // it was, not run again. The same call: by the same caller, to the same function, for the
    // same tenant, with the same arguments. By its id alone, another caller that had the id was
    // given the first one's answer (FEEDBACK51.md, Q-664; branch feedback51-fixes).
    let Value::Map(caller_map) = &caller else { unreachable!("a caller is a map: `caller_value`'s, or `check`'s") };
    let who = caller_map.get("name").map(|n| n.display()).unwrap_or_default();
    // A tool's call by the app's door: its signature with the app's key checked first, before a
    // remembered answer is given, and the call known by its signed id alone, so that one sent
    // again under any caller's name is the same call, answered as it was and never run again
    // (FEEDBACK56.md, Part F, second review).
    let app = match tool_call_body(&req.body) {
        true => match from_its_app(it, req, f, tenant) {
            Ok(k) => Some(k),
            Err(r) => return r,
        },
        false => None,
    };
    let call_id = c.to_string();
    let key = match &app {
        Some(_) => crate::idempotency::sha256_hex(&[b"quillo tool call\n", c.as_bytes()]),
        None => call_key(c, &who, f, tenant, &req.body),
    };
    let c = key.as_str();
    {
        let mut seen = seen().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match seen.get(c) {
            Some((_, Some(r))) => return signed_back(r.clone()),
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
    // Copies of this service behind a balancer each have their own memory:
    // what they answered is kept in the database they share too, so a call
    // sent again to another copy is answered as it was, not run again
    // (FEEDBACK10.md, Q-073; branch feedback10-fixes).
    let now_ms = now_secs() as i64 * 1000;
    let shared = crate::db::call_claim(it, c, now_ms, now_ms - SEEN_FOR.as_millis() as i64);
    match shared {
        Some(crate::db::Claim::Done(status, body)) => {
            let r = HttpResponse { status, headers: vec![("content-type".into(), "application/json".into())], body: body.into_bytes() };
            let r = match &app {
                Some(k) => app_signed(r, k, &call_id),
                None => r,
            };
            seen().lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(c.to_string(), (Instant::now(), Some(r.clone())));
            return signed_back(r);
        }
        Some(crate::db::Claim::Running) => {
            seen().lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(c);
            return refused(503, "this call is still running");
        }
        Some(crate::db::Claim::Mine) | None => {}
    }
    let r = run_call(it, f, tenant, req, caller);
    if shared.is_some() {
        crate::db::call_done(it, c, r.status, &String::from_utf8_lossy(&r.body));
    }
    let mut seen = seen().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // A call that did not finish may be sent again: only an answer of 200
    // ends one, so a 409 before a deploy is not the answer after it
    // (FEEDBACK23.md, Q-184; branch feedback23-fixes).
    if r.status != 200 {
        seen.remove(c);
    } else {
        seen.insert(c.to_string(), (Instant::now(), Some(r.clone())));
    }
    drop(seen);
    signed_back(r)
}

/// The key a call is remembered by, in memory and in `_quillo_calls`: its id, its caller, the
/// function, the tenant and the arguments, hashed to the 64 characters an id there holds.
pub fn call_key(id: &str, caller: &str, f: &str, tenant: &str, body: &[u8]) -> String {
    crate::idempotency::sha256_hex(&[id.as_bytes(), caller.as_bytes(), f.as_bytes(), tenant.as_bytes(), body])
}

/// Calls answered lately, by call id: when, and the answer (none while it runs).
fn seen() -> &'static Mutex<HashMap<String, (Instant, Option<HttpResponse>)>> {
    static S: OnceLock<Mutex<HashMap<String, (Instant, Option<HttpResponse>)>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// How long, and how many, calls are remembered: as long as a signature
/// lasts either way (`WINDOW`), so a call cannot be sent again once
/// forgotten.
const SEEN_FOR: Duration = Duration::from_secs(2 * WINDOW + 60);
const SEEN_MAX: usize = 100_000;

fn run_call(it: &mut Interp, f: &str, tenant: &str, req: &HttpRequest, caller: Value) -> HttpResponse {
    if f == CONTRACTS && !it.server_fns.iter().any(|n| n == f) {
        let who = match &caller {
            Value::Map(m) => m.get("name").map(|n| n.display()).unwrap_or_default(),
            _ => String::new(),
        };
        return json(200, contract_list(it, &who));
    }
    // A call that carries a tool's call is an object: a tool of this service's, called at its
    // app's door, or a server fn with the call's asking (FEEDBACK56.md, Part F; branch
    // feedback56).
    // An object that does not carry one is no call's, said as before.
    let read = crate::value::json_value(&req.body);
    let hop = match &read {
        Ok(serde_json::Value::Object(m)) if m.contains_key("mcp") => Some(m),
        _ => None,
    };
    if let Some(m) = hop.filter(|m| m.contains_key("tool")) {
        return run_tool_hop(it, f, tenant, req, caller, m);
    }
    let Some(Value::Func(c)) = lookup(&it.globals, f).filter(|_| it.server_fns.iter().any(|n| n == f)) else {
        return refused(404, &format!("no server fn `{}`", f));
    };
    // `for ["shop"]`: only those callers, before its arguments are read (FEEDBACK38.md, Q-458;
    // branch feedback36-39).
    let who = match &caller {
        Value::Map(m) => m.get("name").map(|n| n.display()).unwrap_or_default(),
        _ => String::new(),
    };
    if let Some(allowed) = it.fn_callers.get(f) {
        if !allowed.iter().any(|a| *a == who) {
            let who = if who.is_empty() { "a caller whose name is not known".to_string() } else { format!("`{}`", who) };
            return refused(403, &format!("`{}` takes calls from {} only; this is {}", f, allowed.join(", "), who));
        }
    }
    // The caller's copy of this service, deployed apart, may say otherwise.
    let theirs = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("x-quillo-contract")).map(|(_, v)| v.as_str());
    let ours = crate::describe::contract(&c.def);
    if let Some(theirs) = theirs.filter(|t| *t != ours) {
        return refused(409, &format!("`{}` here is `{}`; the caller's copy of this service says `{}`", f, ours, theirs));
    }
    let (args, asking) = match (&read, hop) {
        (Ok(serde_json::Value::Array(a)), _) => (a.iter().map(from_wire).collect::<Vec<_>>(), None),
        (_, Some(m)) => match (m.get("args"), m.get("mcp").and_then(|x| x.get("asking")).map(crate::mcp::Asking::from_wire)) {
            (Some(serde_json::Value::Array(a)), Some(Some(asking))) => (a.iter().map(from_wire).collect::<Vec<_>>(), Some(asking)),
            _ => return refused(400, "a call that carries a tool's call is { args: [...], mcp: { asking } }, as Quillo sends it"),
        },
        _ => return refused(400, "the body must be a JSON array of arguments"),
    };
    let args = match crate::server::take_args(it, &c.def, args) {
        Ok(a) => a,
        Err(msg) => return refused(400, &msg),
    };
    set_called(it, tenant, req, caller);
    let header = |name: &str| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    // The topics this call publishes go back with the answer, so the
    // caller's pages that watch them follow, as they do in one process
    // (FEEDBACK4.md, Q-038; branch bank-fixes).
    it.published = Some(Vec::new());
    // How long the caller waits for it: what this call's own calls to
    // services may wait in turn (FEEDBACK6.md, Q-056; branch feedback6-fixes).
    it.deadline = header("x-quillo-deadline").and_then(|ms| ms.parse::<u64>().ok()).map(|ms| Instant::now() + Duration::from_millis(ms.min(3_600_000)));
    // The tool's call's asking, for its run here, and given back with its answer, whatever it is
    // (FEEDBACK56.md, Part F; branch feedback56).
    let carried = asking.is_some();
    // A call made in a tool's call with no asking, as its id says (`UNSEEN_CALL`): run as part of
    // one, a question refused, and the refusal said back (the coverage review's security review).
    let unseen = !carried && header("x-quillo-call").is_some_and(|c| c.starts_with(UNSEEN_CALL));
    let was_in = it.in_tool;
    it.in_tool |= unseen;
    let (result, back) = with_asking(it, asking, |it| it.call_closure(&c, args.into_iter().map(Some).collect(), 0));
    it.in_tool = was_in;
    it.deadline = None;
    let published = it.published.take().unwrap_or_default();
    // The asking goes back with any answer of a call that carried one; an error's is as before.
    let with_asking = |mut j: serde_json::Value| {
        if carried {
            j["asking"] = back.as_ref().map_or(serde_json::Value::Null, |a| a.reply());
        }
        j
    };
    let refused_with = |status: u16, msg: String| json(status, with_asking(serde_json::json!({ "error": msg })));
    // Sent only when there are some: an answer without any is as before.
    let with_published = |mut j: serde_json::Value| {
        if !published.is_empty() {
            j["published"] = serde_json::json!(published);
        }
        with_asking(j)
    };
    if let Some(why) = back.as_ref().filter(|_| unseen).and_then(|a| a.unseen_why()) {
        eprintln!("quillo: error in server fn `{}` (called by a service, in a tool's call): {}", f, why);
        return json(200, with_published(serde_json::json!({ "stopped": true, "refused": why })));
    }
    if back.as_ref().is_some_and(|a| a.stopped()) {
        if let (true, Err(Ctrl::Error(e))) = (back.as_ref().is_some_and(|a| a.failed()), &result) {
            eprintln!("quillo: error in server fn `{}` (called by a service): {:#}", f, e);
        }
        return json(200, with_published(serde_json::json!({ "stopped": true })));
    }
    match result {
        Ok(v) | Err(Ctrl::Return(v)) => match v {
            Value::Response(_) => refused_with(500, format!("`{}` answered with a page, not a value", f)),
            v => json(200, with_published(wire_answer(&v))),
        },
        Err(Ctrl::Reply(_)) => refused_with(500, format!("`{}` replied with a page, not a value", f)),
        Err(Ctrl::Error(e)) => {
            eprintln!("quillo: error in server fn `{}` (called by a service): {:#}", f, e);
            refused_with(500, format!("internal error in `{}`", f))
        }
    }
}

/// What a call from another service runs in: the tenant it names, and `request`, the request it
/// came from as far as it goes with the call (its id), the call's own id, who called, and the
/// visitor's headers it passed on.
fn set_called(it: &mut Interp, tenant: &str, req: &HttpRequest, caller: Value) {
    it.tenant = Some(if tenant.is_empty() { None } else { Some(Arc::from(tenant)) });
    it.tenant_named = !tenant.is_empty();
    // Called by another service: `request` is the request it came from, as
    // far as it goes with the call (its id), and the call's own id.
    let header = |name: &str| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    let mut step = match step_request(it.request_id.as_deref(), header("x-quillo-call").unwrap_or("")) {
        Value::Map(m) => (*m).clone(),
        _ => Map::new(),
    };
    // Who called, and what the visitor's own headers it passed on say: `request.user` when
    // the visitor's token is one `accept_calls`'s jwt takes (Q-460).
    step.insert("caller".into(), caller);
    let mut passed = Map::new();
    for (k, v) in &req.headers {
        if let Some(n) = k.to_ascii_lowercase().strip_prefix("x-quillo-forward-") {
            passed.insert(n.into(), Value::str(v.as_str()));
        }
    }
    let token = passed.get("authorization").map(|v| v.display()).and_then(|v| v.strip_prefix("Bearer ").map(str::to_string));
    if let (Some(rules), Some(token)) = (it.accept.as_ref().and_then(|a| a.jwt.clone()), token) {
        if let Ok(claims) = it.unlocked(move || crate::svcauth::verify_jwt(&token, &rules)) {
            step.insert("user".into(), Value::from_json(&claims));
        }
    }
    step.insert("forwarded".into(), Value::map(passed));
    it.current_request = Some(Value::map(step));
}

/// A call of this service's tool `f` by its app's door (`call_tool`): offered to that door alone.
/// Signed as any service call is and taken as `accept_calls` says, it is signed with the key of
/// this service's app too (`QUILLO_APP_KEY`, which the app that started it gave it), which no other
/// service, page or token has; without that key here, no tool's call is taken; and its answer is
/// signed back with it (FEEDBACK56.md, Part F, security review). A `fn mcp` that is not a `server
/// fn` is no plain call's. Its arguments are made here as the client sent them, by name, as this
/// program's own door would; it runs in the caller's tenant and session, as the app's door found
/// them, with the call's asking; a tool that may ask sent none runs as for a client that cannot
/// ask, never as outside a tool's call. Its answer is `tools/call`'s result, and the asking's
/// reply (`Asking::reply`).
fn run_tool_hop(it: &mut Interp, f: &str, tenant: &str, req: &HttpRequest, caller: Value, m: &serde_json::Map<String, serde_json::Value>) -> HttpResponse {
    let header = |name: &str| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    // Its signature with the app's key was checked as it came (`answer`, `from_its_app`).
    let key = match own_app_keys(it) {
        Ok(Some((k, _))) => k,
        Ok(None) => return refused(403, &no_app_key(header("x-quillo-to").unwrap_or(""))),
        Err(why) => return refused(403, &why),
    };
    let call = header("x-quillo-call").unwrap_or("");
    let Some(tool) = it.offered_tools.as_ref().and_then(|t| t.iter().find(|t| t.name == f)).cloned() else {
        return refused(404, &format!("no tool `{}`", f));
    };
    let mcp = m.get("mcp").and_then(serde_json::Value::as_object);
    let (Some(serde_json::Value::Object(args)), Some(mcp)) = (m.get("arguments"), mcp) else {
        return refused(400, "a tool's call is { tool, arguments: { ... }, mcp: { ... } }, as Quillo sends it");
    };
    let sent_none = matches!(mcp.get("asking"), None | Some(serde_json::Value::Null));
    let asking = match mcp.get("asking") {
        None | Some(serde_json::Value::Null) => tool.may_ask.then(crate::mcp::Asking::cannot_ask),
        Some(a) => match crate::mcp::Asking::from_wire(a) {
            Some(a) => Some(a),
            None => return refused(400, "this tool's call carries an asking that is not one Quillo sends"),
        },
    };
    set_called(it, tenant, req, caller);
    if let Some(Value::Map(r)) = &mut it.current_request {
        Arc::make_mut(r).insert("mcp".into(), mcp.get("client").map_or(Value::Nil, from_wire));
    }
    let session = mcp.get("session").map_or_else(|| Value::map(Map::new()), from_wire);
    crate::session::take_on(it, session, mcp.get("owner").and_then(serde_json::Value::as_str).unwrap_or(""));
    it.published = Some(Vec::new());
    it.deadline = header("x-quillo-deadline").and_then(|ms| ms.parse::<u64>().ok()).map(|ms| Instant::now() + Duration::from_millis(ms.min(3_600_000)));
    it.answering = true;
    let (mut result, mut back) = with_asking(it, asking, |it| match crate::mcp::by_name(&tool, args) {
        Ok(a) => crate::mcp::answer_tool(it, &tool, a),
        Err(why) => crate::mcp::tool_error(why),
    });
    it.answering = false;
    it.deadline = None;
    it.set_cookies.clear();
    // No door takes the asking back: its answer is here, refused where it would have asked.
    if sent_none {
        if let Some(a) = back.as_mut() {
            a.settled(&mut result);
        }
        back = None;
    }
    let mut j = serde_json::json!({ "tool": result, "asking": back.as_ref().map(|a| a.reply()) });
    let published = it.published.take().unwrap_or_default();
    if !published.is_empty() {
        j["published"] = serde_json::json!(published);
    }
    app_signed(json(200, j), &key, call)
}

/// Quillo's own addresses that are not passed on to a service under its
/// prefix: calls of its functions, and what only its own server or other
/// services ask of it.
const NOT_PASSED_ON: &[&str] = &["rpc", "service", "job", "tick", "later", "channel", "udp", "wt-ticket", "announce", "metrics"];

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

/// What `quillo check` and the run say of `service mcp crm = "https://..."`: calling another MCP
/// server is for later (FEEDBACK56.md, Part G), and the form is kept for it; the path form,
/// `service mcp /billing = "billing.qwe"`, is the one there now (FEEDBACK56.md, Q-853; branch
/// feedback56).
pub fn mcp_calls_later(name: &str) -> String {
    format!(
        "`service mcp {}` would call another MCP server, which Quillo does not do yet: `service mcp /{} = \"{}.qwe\"` offers a Quillo file's tools here, and `service {} = \"{}.qwe\"` calls its server fns",
        name, name, name, name, name
    )
}

/// `service mcp /billing = "billing.qwe" { auth: ... }`: the calls of billing's tools are signed
/// both ways, with the shared secret or a key of the caller's; a bearer token, OAuth or a
/// certificate alone signs neither the call nor its answer, so none of them offers the tools
/// (FEEDBACK56.md, Part F, security review). The way it would prove itself that cannot, if any.
pub fn unsigned_auth(name: &str) -> Option<&'static str> {
    let a = crate::svcauth::auth_of(name);
    match (&a.key, &a.bearer, &a.oauth, &a.mtls) {
        (Some(_), ..) => None,
        (None, Some(_), ..) => Some("bearer"),
        (None, None, Some(_), _) => Some("oauth"),
        (None, None, None, Some(_)) => Some("mtls"),
        _ => None,
    }
}

/// What `quillo check` and the start say of it (`unsigned_auth`).
pub fn tools_unsigned(name: &str, how: &str) -> String {
    format!(
        "`service mcp`: a call of {}'s tools is signed, and its answer too, with the shared secret or `auth: {{ key }}`; `auth: {{ {} }}` signs neither: give it a key, or make it a plain `service`, which offers no tools",
        name, how
    )
}

/// `service name [/prefix] = "file.qwe"`, run by the program declaring it:
/// the name stands for the service's functions; the service is loaded here
/// (once per process) unless it runs elsewhere; under its prefix, its
/// routes are served (here, or passed on to it).
pub fn declare(it: &mut Interp, name: &str, prefix: Option<&str>, file: &str, options: Option<&Value>, line: u32) -> R {
    if !it.declared_services.iter().any(|n| n == name) {
        Arc::make_mut(&mut it.declared_services).push(name.to_string());
    }
    if let Some(o) = options {
        set_opts(name, o, line)?;
        if let Value::Map(m) = o {
            crate::svcauth::set_auth(name, m.get("auth"), m.get("forward"), &it.base_dir).map_err(|e| Ctrl::Error(Box::new(crate::error::QuilloError::new(e, line))))?;
        }
    }
    // From the folder of the file a main file that is a link leads to, as `use` and `mount`
    // paths are (FEEDBACK55.md, Q-817; branch feedback55).
    let full = it.src_dir().join(file);
    let fail_at = |e: String| Ctrl::Error(Box::new(crate::error::QuilloError::new(e, line)));
    // A service that runs elsewhere and whose file is not here: asked for its functions, as it
    // gives them, signed (FEEDBACK40.md, Q-471; branch feedback40-43).
    // `urls_of`, not `url_of`, which takes the addresses in turn: the mount below is given the
    // one it would have been.
    let urls = urls_of(&it.shared, name);
    let (defs, given) = match offered(&full) {
        Ok(d) => (d, None),
        Err(_) if !full.exists() && !urls.is_empty() => {
            let (d, c) = functions_of(it, name, &urls).map_err(|e| fail_at(format!("{} is not here, and service {} did not give its functions: {}", file, name, e)))?;
            (d, Some(c))
        }
        Err(e) => return Err(fail_at(e)),
    };
    {
        let mut c = contracts().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for (k, d) in defs.iter().enumerate() {
            let contract = given.as_ref().and_then(|g: &Vec<String>| g.get(k).cloned()).unwrap_or_else(|| crate::describe::contract(d));
            c.insert((name.to_string(), d.name.clone()), contract);
        }
    }
    // Its files are watched when it runs apart too, so a function added there reaches the app
    // (FEEDBACK40.md, Q-472).
    if full.exists() {
        let mut seen = HashSet::new();
        let mut stmts = Vec::new();
        let _ = statements(&full, full.parent().unwrap_or(Path::new(".")), &mut seen, &mut stmts);
        Arc::make_mut(&mut it.watched).extend(seen);
        // Which of its functions may be sent to another copy after a proxy's 502 or 504 (Q-595).
        let all: HashMap<String, Arc<crate::ast::FnDef>> = stmts.iter().filter_map(|s| match s {
            crate::ast::Stmt::Fn(d) => Some((d.name.clone(), d.clone())),
            _ => None,
        }).collect();
        let mut r = repeatable().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for d in &defs {
            if crate::idempotency::used_by_defs(&all, d) {
                r.insert((name.to_string(), d.name.clone()));
            } else {
                r.remove(&(name.to_string(), d.name.clone()));
            }
        }
    }
    let ns = namespace(it, name, &defs, line)?;
    if let Some(url) = url_of(&it.shared, name) {
        if let Some(p) = prefix {
            it.remote_mounts.retain(|(q, _)| q != p);
            it.remote_mounts.push((p.to_string(), url));
            it.remote_sources.retain(|(q, _)| q != p);
            it.remote_sources.push((p.to_string(), full.clone()));
        }
    } else {
        let key = full.canonicalize().unwrap_or_else(|_| full.clone());
        let existing = it.services.loaded.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(name).map(|(p, s)| (p.clone(), s.fork()));
        let svc = match existing {
            Some((p, s)) if p == key => Some(s),
            Some((p, _)) => return fail(line, format!("service `{}` is {} elsewhere in this app, and {} here", name, p.display(), key.display())),
            None if it.services.loading.lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(name) => None,
            None => {
                let dir = full.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| it.base_dir.to_path_buf());
                // Its own settings, beside its file (FEEDBACK9.md, Q-071); one it cannot read is
                // said, as the program's is (FEEDBACK55.md, Q-835; branch feedback55).
                let own = match crate::dotenv::read(&dir.join(".env")) {
                    Ok(own) => own,
                    Err(e) => return fail(line, format!("service `{}`: {}", name, e)),
                };
                it.services.loading.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(name.to_string());
                // Not served under another name (FEEDBACK55.md, Q-701; branch feedback55).
                crate::builtins::protect_settings(&dir, None);
                let mut sub = Interp::with_shared(dir, it.shared.clone());
                sub.services = it.services.clone();
                sub.is_service = true;
                sub.env_file = std::sync::Arc::new(own);
                let loaded = sub.load_file(&full).map_err(|e| Ctrl::Error(Box::new(e.in_file(&full.display().to_string()))));
                it.services.loading.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(name);
                loaded?;
                it.services.loaded.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(name.to_string(), (key, sub.fork()));
                Some(sub)
            }
        };
        if let Some(svc) = svc {
            // Watched, not loaded: a file the service and this program both
            // `use` still runs here (FEEDBACK4.md, Q-040; branch bank-fixes).
            Arc::make_mut(&mut it.watched).extend(svc.files());
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
///
/// Made in a tool's call that may ask its user, it is made later, when no one is there to ask:
/// its id says so (`FROM_A_TOOL`), kept with it wherever it waits, and a question in it is
/// refused, the call failing for good, never answered by nobody (FEEDBACK56.md, Part F; branch
/// feedback56). Before, its `confirm` gave true.
pub fn later(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (service, f, sent) = call_args(&args, "__service_later", line)?;
    let call = Later {
        // In a tool's call with no asking, the id says so too, the call then sent as any other
        // (`UNSEEN_CALL`; the coverage review's security review).
        id: match (it.asking.is_some(), it.in_tool) {
            (true, _) => format!("{}{}", FROM_A_TOOL, new_id(23)),
            (false, true) => format!("{}{}", UNSEEN_CALL, new_id(23)),
            (false, false) => new_id(24),
        },
        service,
        f,
        args: serde_json::Value::Array(sent.iter().map(to_wire).collect()).to_string(),
        tenant: it.tenant().map(|t| t.to_string()).unwrap_or_default(),
        rid: it.request_id.as_deref().unwrap_or("").to_string(),
        at: now_ms(),
        tries: 0,
    };
    let id = call.id.clone();
    if !crate::db::later_put(it, &call) {
        static SAID: std::sync::Once = std::sync::Once::new();
        SAID.call_once(|| eprintln!("quillo: calls made later are kept in memory (no database could keep them): a restart loses those not made yet"));
        waiting().lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(call);
    }
    Ok(Value::str(id))
}

/// The first character of the id of a call made later in a tool's call that may ask its user
/// (`later`): not a hex digit, so no other call's id starts with it.
const FROM_A_TOOL: char = 't';

/// How long until a call that did not get through is tried again: a second,
/// then twice as long each time, up to five minutes.
fn backoff(tries: i64) -> i64 {
    (1000i64 << tries.clamp(0, 9)).min(300_000)
}

/// How many due calls one go takes, on each database.
const BATCH: usize = 50;
/// How many tries a call the service keeps answering with an error gets,
/// with `backoff`: about a day. A service that does not answer at all is
/// waited for, however long.
const ANSWERED_TRIES: i64 = 290;

/// Makes the calls that are due (asked by the server's scheduler, each
/// second): those that get through are done with; the others wait longer.
pub fn deliver(it: &mut Interp) -> HttpResponse {
    // Every half minute: transactions over several databases left part way.
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let recovering = {
        let mut l = LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let due = l.map_or(true, |t| t.elapsed() >= Duration::from_secs(30));
        if due {
            *l = Some(Instant::now());
        }
        due
    };
    if recovering {
        crate::db::recover(it);
    }
    let now = now_ms();
    let due: Vec<Later> = {
        let mut w = waiting().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (due, rest): (Vec<Later>, Vec<Later>) = std::mem::take(&mut *w).into_iter().partition(|c| c.at <= now);
        *w = rest;
        due
    };
    for mut c in due {
        match make_later(it, &c) {
            Tried::Through(_) => crate::metrics::later(0),
            Tried::Again(why) => {
                c.tries += 1;
                c.at = now_ms() + backoff(c.tries);
                said_again(&c, c.tries, &why);
                waiting().lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(c);
            }
            Tried::Never(why) => said_never(&c, &why),
        }
    }
    let mut more = deliver_kept(it);
    // The services this process runs keep their calls where their work is,
    // and only this thread is there to make them (FEEDBACK23.md, Q-183; branch
    // feedback23-fixes), and to finish their `db.atomic` work.
    let here: Vec<Interp> = it.services.loaded.lock().unwrap_or_else(std::sync::PoisonError::into_inner).values().map(|(_, s)| s.fork()).collect();
    for mut s in here {
        if recovering {
            crate::db::recover(&mut s);
        }
        if !s.declared_services.is_empty() {
            more |= deliver_kept(&mut s);
        }
    }
    // A full go: the next comes at once, not in a second (FEEDBACK23.md,
    // Q-187; branch feedback23-fixes). Each is a request of its own, so what
    // visitors ask for goes between them.
    HttpResponse { status: 200, headers: Vec::new(), body: if more { b"more\n".to_vec() } else { b"ok\n".to_vec() } }
}

/// Makes a program's calls that are due, kept in its databases and its
/// tenants'; true if there may be more at once.
fn deliver_kept(it: &mut Interp) -> bool {
    let mut more = false;
    for on in crate::db::later_homes(it) {
        more |= deliver_from(it, on);
    }
    let tenants = crate::db::later_tenants(it, now_ms(), BATCH);
    more |= tenants.len() == BATCH;
    for (tenant, made) in tenants {
        let was = (it.tenant.take(), it.tenant_named);
        it.tenant = Some(Some(Arc::from(tenant.as_str())));
        it.tenant_named = true;
        more |= deliver_from(it, None);
        let next = crate::db::later_next(it, None);
        (it.tenant, it.tenant_named) = was;
        crate::db::later_tenant_seen(it, &tenant, made, next);
    }
    more
}

/// One go at the calls due on connection `on`, each taken by one server
/// (others may share the database); true if it took all it could.
fn deliver_from(it: &mut Interp, on: Option<usize>) -> bool {
    let due = crate::db::later_due(it, on, now_ms(), BATCH);
    let full = due.len() == BATCH;
    for c in due {
        if !crate::db::later_take(it, on, &c.id, c.at, now_ms() + 60_000) {
            continue;
        }
        match make_later(it, &c) {
            Tried::Through(answer) => {
                crate::metrics::later(0);
                crate::db::later_done(it, on, &c.id, &answer);
            }
            Tried::Again(why) => {
                let tries = c.tries + 1;
                said_again(&c, tries, &why);
                crate::db::later_again(it, on, &c.id, now_ms() + backoff(tries), tries, &why);
            }
            Tried::Never(why) => {
                said_never(&c, &why);
                crate::db::later_failed(it, on, &c.id, &why);
            }
        }
    }
    full
}

fn said_again(c: &Later, tries: i64, why: &str) {
    crate::metrics::later(1);
    eprintln!("quillo: {}.{} (made later, {}) not made yet: {}; again in {}s", c.service, c.f, c.id, why, backoff(tries) / 1000);
}

fn said_never(c: &Later, why: &str) {
    crate::metrics::later(2);
    eprintln!("quillo: {}.{} (made later, {}) failed after {} tries: {}; not tried again", c.service, c.f, c.id, c.tries + 1, why);
}

/// How a try of a call made later went.
enum Tried {
    /// It got through: the answer, as a service's answer says it.
    Through(String),
    /// Not yet: why.
    Again(String),
    /// It cannot, or has failed too long: why.
    Never(String),
}

/// One call made later. Only a value, or an error value the function gave,
/// finishes one: a service that answers with an error of its own may be
/// mid-deploy, and a call deleted then is lost (FEEDBACK23.md, Q-184; branch
/// feedback23-fixes). Arguments it refuses (400) will not do any better.
fn make_later(it: &mut Interp, c: &Later) -> Tried {
    let args = match crate::value::json_value(c.args.as_bytes()) {
        Ok(serde_json::Value::Array(a)) => a.iter().map(from_wire).collect(),
        _ => Vec::new(),
    };
    let tenant = (!c.tenant.is_empty()).then(|| Arc::from(c.tenant.as_str()));
    let rid = (!c.rid.is_empty()).then(|| Arc::from(c.rid.as_str()));
    // Made in a tool's call: run with no one to ask, here and where the service runs.
    let made = match (c.id.starts_with(FROM_A_TOOL), c.id.starts_with(UNSEEN_CALL)) {
        (true, _) => with_asking(it, Some(crate::mcp::Asking::nobody()), |it| make(it, &c.service, &c.f, args, tenant, rid, Some(c.id.clone()), 0)).0,
        // Made in a tool's call with no asking: run as part of one, with no one to ask; what a
        // refusal sets is let go after (the coverage review's security review).
        (false, true) => {
            let was = std::mem::replace(&mut it.in_tool, true);
            let made = with_asking(it, None, |it| make(it, &c.service, &c.f, args, tenant, rid, Some(c.id.clone()), 0)).0;
            it.in_tool = was;
            made
        }
        (false, false) => make(it, &c.service, &c.f, args, tenant, rid, Some(c.id.clone()), 0),
    };
    match made {
        Outcome::Stopped | Outcome::Tool(_) => Tried::Never(format!("it stopped at a question: {}", crate::mcp::NOBODY)),
        Outcome::Ran(v) => {
            if let Value::Err(e, _, _) = &v {
                eprintln!("quillo: {}.{} (made later, {}) gave an error: {}", c.service, c.f, c.id, short(e));
            }
            Tried::Through(wire_answer(&v).to_string())
        }
        Outcome::Refused(400, why) => Tried::Never(why),
        Outcome::Refused(_, why) if c.tries + 1 >= ANSWERED_TRIES => Tried::Never(why),
        Outcome::Refused(_, why) | Outcome::Missed(why) => Tried::Again(why),
    }
}

/// `later.status(id)`: what became of a call made later (FEEDBACK23.md, Q-188;
/// branch feedback23-fixes), or null if nothing is known of it.
fn later_status(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let Some(Value::Str(id)) = args.first() else {
        return fail(line, "`later.status` expects the id a `.later` call gave");
    };
    let state = |state: &str, fields: Vec<(&str, Value)>| {
        let mut m = Map::new();
        m.insert("state".into(), Value::str(state));
        for (k, v) in fields {
            m.insert(k.into(), v);
        }
        Value::map(m)
    };
    // Why a call has not got through names addresses, so it is an error
    // value, which no visitor sees.
    let error = |e: Option<String>| e.map_or(Value::Nil, |e| Value::err(Value::str(e)));
    if let Some(c) = waiting().lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().find(|c| c.id == **id) {
        return Ok(state("waiting", vec![("tries", Value::Int(c.tries)), ("error", Value::Nil)]));
    }
    Ok(match crate::db::later_status(it, id) {
        None => Value::Nil,
        Some((_, _, Some(answer))) => {
            let j = crate::value::json_value(answer.as_bytes()).unwrap_or_default();
            state("done", vec![("result", from_answer(&j))])
        }
        Some((tries, e, None)) if tries < 0 => state("failed", vec![("error", error(e))]),
        Some((tries, e, None)) => state("waiting", vec![("tries", Value::Int(tries)), ("error", error(e))]),
    })
}

pub fn install_later(m: &mut Map) {
    m.insert("status".into(), Value::Builtin("later.status", later_status));
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
    let mut more = false;
    let _ = std::thread::Builder::new().name("quillo-later".into()).spawn(move || loop {
        if crate::httpd::stopping() {
            return;
        }
        if !more {
            std::thread::sleep(Duration::from_secs(1));
        }
        let asked = crate::jobs::post_self(addr, "/_quillo/later", b"").map(|(_, out)| out.ends_with(b"more\n"));
        more = matches!(asked, Ok(true));
    });
}

// ---- running split ---------------------------------------------------------------------

/// The processes `--split` started: kept running while it runs (one that
/// ends is started again, see `supervise`), and stopped with it.
pub struct Split {
    services: Arc<Mutex<Vec<Running>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

/// A service's process, and how to start it again.
struct Running {
    name: String,
    exe: PathBuf,
    args: Vec<String>,
    child: std::process::Child,
    started: Instant,
    /// Before the next start, after it ended: longer each time it ends soon.
    wait: Duration,
    again_at: Option<Instant>,
}

/// The process of each service `--split` started that has not been waited for: a second stop
/// ends them at once (`kill_split_now`). A process is taken off when it is waited for, under this
/// lock, so that its number, free then for another process, is never signalled.
fn split_pids() -> std::sync::MutexGuard<'static, Vec<u32>> {
    static PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
    PIDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether `child` has ended, taken off `split_pids` when it has.
fn ended(child: &mut std::process::Child) -> Option<std::process::ExitStatus> {
    let mut pids = split_pids();
    let status = child.try_wait().ok().flatten()?;
    pids.retain(|p| *p != child.id());
    Some(status)
}

/// A second stop, or `--drain`'s time over: the services `--split` started end now, as the app
/// does (the user's graceful stop, 2026-11-09; branch feedback56).
pub fn kill_split_now() {
    #[cfg(unix)]
    for pid in split_pids().iter() {
        // SAFETY: a plain signal, to a process of ours not waited for yet (`split_pids`).
        unsafe { libc::kill(*pid as libc::pid_t, libc::SIGKILL) };
    }
}

impl Drop for Split {
    /// Once the app's requests in progress are answered: each service is told to stop, by the
    /// pipe on its input closing, and finishes what it is doing (a call made later, a job, a call
    /// from elsewhere) as a first SIGTERM has it, under the app's `--drain`, and the app waits for
    /// it. Before, each was killed, and what it was doing cut off (the user's graceful stop,
    /// 2026-11-09; branch feedback56).
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let mut services = std::mem::take(&mut *self.services.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        for r in services.iter_mut() {
            drop(r.child.stdin.take());
        }
        for r in services.iter_mut() {
            while ended(&mut r.child).is_none() {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

fn free_port() -> std::io::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// Set for a service `--split` starts: its stdin is the app's pipe, and it stops when that
/// closes (main.rs's `stop_with_app`).
pub const STOP_WITH_APP: &str = "QUILLO_STOP_WITH_APP";

/// Starts a service's process, its log lines marked with its name.
fn start_service(exe: &Path, args: &[String], name: &str) -> Result<std::process::Child, String> {
    let mut command = std::process::Command::new(exe);
    // Where the app listens, for the service's channels (FEEDBACK12.md, Q-087;
    // branch open-fixes).
    if let Some(u) = APP_URL.get() {
        command.env("QUILLO_APP_URL", u);
    }
    // The key its app signs its tools' calls with, its own and no other service's: a service
    // started apart runs its app's tools only (FEEDBACK56.md, Part F, security review).
    for (k, _) in std::env::vars().filter(|(k, _)| k.starts_with("QUILLO_APP_KEY_")) {
        command.env_remove(k);
    }
    match std::env::var(app_key_env(name)) {
        Ok(k) => command.env(APP_KEY, k),
        Err(_) => command.env_remove(APP_KEY),
    };
    // Its stdin is a pipe the app holds, which closes however the app's process ends: the
    // service stops then. A service whose app was killed served on, its port taken, for ever
    // (found in review of FEEDBACK55.md's Q-807; branch feedback55).
    command.env(STOP_WITH_APP, "1").stdin(std::process::Stdio::piped());
    // In a process group of its own, so that a Ctrl-C in the terminal reaches the app alone, and
    // the app stops it once its own requests in progress are answered: they may still call it.
    // Reached by the terminal's signal too, it stopped taking calls while the app answered
    // requests that needed it (the user's graceful stop, 2026-11-09; branch feedback56).
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut pids = split_pids();
    let mut child = command
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start service {}: {}", name, e))?;
    pids.push(child.id());
    drop(pids);
    for out in [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)].into_iter().flatten() {
        let tag = name.to_string();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                eprintln!("[{}] {}", tag, l);
            }
        });
    }
    Ok(child)
}

/// Watches the services `--split` started: one whose process ends while the
/// app runs is started again, the same way (same port, same secret), after
/// a second, then twice as long each time it ends again soon (at most 30 s),
/// and from a second again once it has run for a minute. Before, a service
/// that died stayed away until the whole app was restarted (FEEDBACK4.md,
/// Q-042; branch bank-fixes).
fn supervise(services: Arc<Mutex<Vec<Running>>>, stop: Arc<std::sync::atomic::AtomicBool>) {
    let _ = std::thread::Builder::new().name("quillo-split".into()).spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        if stop.load(Ordering::SeqCst) || crate::httpd::stopping() {
            return;
        }
        let now = Instant::now();
        for r in services.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter_mut() {
            match r.again_at {
                None => {
                    if let Some(status) = ended(&mut r.child) {
                        if now.duration_since(r.started) >= Duration::from_secs(60) {
                            r.wait = Duration::from_secs(1);
                        }
                        eprintln!("quillo: service {} stopped ({}); starting it again in {} s", r.name, status, r.wait.as_secs());
                        r.again_at = Some(now + r.wait);
                    }
                }
                Some(at) if now >= at => match start_service(&r.exe, &r.args, &r.name) {
                    Ok(child) => {
                        eprintln!("quillo: service {} started again", r.name);
                        r.child = child;
                        r.started = now;
                        r.again_at = None;
                        r.wait = (r.wait * 2).min(Duration::from_secs(30));
                    }
                    Err(e) => {
                        eprintln!("quillo: {}", e);
                        r.wait = (r.wait * 2).min(Duration::from_secs(30));
                        r.again_at = Some(now + r.wait);
                    }
                },
                Some(_) => {}
            }
        }
    });
}

/// `quillo run app.qwe --split`: each service the app declares (and those
/// they declare) runs as a process of its own, on a port of its own, its
/// log lines marked with its name; the app (and each service) is told
/// where the others are, and they sign their calls with one secret. With
/// `--scale risk=3`, a service runs as that many copies, each on a port of
/// its own, taking calls in turn and sharing its database (FEEDBACK10.md,
/// Q-075; branch feedback10-fixes).
pub fn split(path: &Path, watch: bool, reload: bool, static_check: bool, checked: bool, scale: &[(String, usize)], app_url: &str) -> Result<Split, String> {
    let services = declared(path)?;
    let _ = APP_URL.set(app_url.to_string());
    if let Some((name, _)) = scale.iter().find(|(n, _)| !services.iter().any(|(s, _, _)| s == n)) {
        let names: Vec<&str> = services.iter().map(|(s, _, _)| s.as_str()).collect();
        return Err(format!("--scale {}: no service `{}` (the app declares {})", name, name, if names.is_empty() { "none".to_string() } else { names.join(", ") }));
    }
    if std::env::var("QUILLO_SERVICE_SECRET").map_or(true, |s| s.is_empty()) {
        std::env::set_var("QUILLO_SERVICE_SECRET", crate::crypto::hex(&crate::crypto::secret_bytes(32)));
    }
    // Each copy of each service: its name, prefix, file and port.
    let mut copies = Vec::new();
    for (name, prefix, file) in &services {
        let n = scale.iter().find(|(s, _)| s == name).map_or(1, |(_, n)| *n);
        let mut urls = Vec::new();
        for _ in 0..n {
            let port = free_port().map_err(|e| e.to_string())?;
            urls.push(format!("http://127.0.0.1:{}", port));
            copies.push((name.clone(), prefix.clone(), file.clone(), port));
        }
        std::env::set_var(env_name(name), urls.join(","));
        // The key the app signs a call of the service's tools with, which only it and the service
        // have (FEEDBACK56.md, Part F, security review).
        if std::env::var(app_key_env(name)).map_or(true, |k| k.is_empty()) {
            std::env::set_var(app_key_env(name), crate::crypto::hex(&crate::crypto::secret_bytes(32)));
        }
    }
    // A service's channels, when it runs apart: the pages connect to the app's
    // process, which asks the services which channels are theirs and hands
    // them what pages send there; what a service broadcasts comes back through
    // the app (`QUILLO_APP_URL`) (FEEDBACK12.md, Q-087; branch open-fixes).
    crate::wt::set_split(services.iter().map(|(name, _, _)| name.clone()).collect());
    let ports: Vec<u16> = copies.iter().map(|c| c.3).collect();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut running = Vec::new();
    for (name, prefix, file, port) in &copies {
        let mut args: Vec<String> = vec!["run".into(), file.display().to_string(), "--port".into(), port.to_string(), "--as-service".into(), "--service-name".into(), name.clone()];
        if let Some(p) = prefix {
            args.extend(["--base".to_string(), p.clone()]);
        }
        if watch {
            args.push("--watch".into());
        } else if reload {
            // Each service takes a change to its files, as under --watch (FEEDBACK55.md, Q-842;
            // branch feedback55).
            args.push("--reload".into());
        }
        if crate::native::NO_BUILD.load(std::sync::atomic::Ordering::Relaxed) {
            args.push("--no-build".into());
        }
        // The app's time for a stop is its services' too (the user's graceful stop, 2026-11-09;
        // branch feedback56).
        if let Some(d) = crate::httpd::drain() {
            args.extend(["--drain".to_string(), format!("{}ms", d.as_millis())]);
        }
        // The app's policy is its services' too (the host's they read themselves).
        for f in crate::policy::get().files.iter().filter(|f| f.as_path() != Path::new(crate::policy::HOST_FILE)) {
            args.extend(["--policy".to_string(), f.display().to_string()]);
        }
        if !static_check {
            args.push("--no-check".into());
        } else if checked {
            // Checked by the app already: its first load does not say it all again; a reload
            // under --watch still checks (FEEDBACK40.md, Q-473; branch feedback40-43).
            args.push("--checked".into());
        }
        // Its own document, at `/billing/_quillo/describe` through the app, as in one process
        // (FEEDBACK32.md, Q-397; branch feedback27-35-fixes).
        if crate::describe::ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
            args.push("--describe".into());
        }
        let child = start_service(&exe, &args, name)?;
        eprintln!("quillo: service {} ({}) on http://127.0.0.1:{}{}", name, file.display(), port, prefix.as_ref().map_or(String::new(), |p| format!(", served under {}", p)));
        // Started again later, it checks itself: its file may have changed since.
        let args: Vec<String> = args.into_iter().filter(|a| a != "--checked").collect();
        running.push(Running { name: name.clone(), exe: exe.clone(), args, child, started: Instant::now(), wait: Duration::from_secs(1), again_at: None });
    }
    let split = Split { services: Arc::new(Mutex::new(running)), stop: Arc::new(std::sync::atomic::AtomicBool::new(false)) };
    // Ready when each answers.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    for (i, ((name, _, _, _), port)) in copies.iter().zip(&ports).enumerate() {
        while std::net::TcpStream::connect(("127.0.0.1", *port)).is_err() {
            if std::time::Instant::now() > deadline {
                return Err(format!("service {} did not start (see its lines above)", name));
            }
            // One that has ended is not waited for: said at once (FEEDBACK40.md, Q-474; branch
            // feedback40-43).
            // Waited for as `ended` does, so that its number leaves `split_pids` then (found in
            // review).
            let gone = ended(&mut split.services.lock().unwrap_or_else(std::sync::PoisonError::into_inner)[i].child);
            if let Some(status) = gone {
                std::thread::sleep(Duration::from_millis(100));
                return Err(format!("service {} stopped at start ({}; see its lines above)", name, status));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    supervise(split.services.clone(), split.stop.clone());
    Ok(split)
}
