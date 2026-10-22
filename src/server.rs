//! Routing requests to `route` declarations, and the HTTP server.

use crate::ast::Seg;
use crate::interp::{coerce, Ctrl, Interp, Mode};
use crate::shared::Shared;
use crate::value::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub remote: String,
}

pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn plain(status: u16, msg: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![("content-type".into(), "text/plain; charset=utf-8".into())],
        body: msg.as_bytes().to_vec(),
    }
}

pub fn percent_decode(s: &str, plus_is_space: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    while i < b.len() {
        match b[i] {
            b'+' if plus_is_space => out.push(b' '),
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h * 16 + l);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_urlencoded(s: &str) -> Map {
    let mut m = Map::new();
    for pair in s.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        m.insert(percent_decode(k, true), Value::str(percent_decode(v, true)));
    }
    m
}

fn match_path(def: &[Seg], segs: &[String]) -> Option<Vec<(String, Value)>> {
    let mut out = Vec::new();
    for (i, seg) in def.iter().enumerate() {
        match seg {
            Seg::Rest(name) => {
                out.push((name.clone(), Value::str(segs.get(i..).map(|s| s.join("/")).unwrap_or_default())));
                return Some(out);
            }
            Seg::Lit(l) => {
                if segs.get(i)? != l {
                    return None;
                }
            }
            Seg::Param(name, ty) => {
                let s = segs.get(i)?;
                let v = match ty {
                    Some(t) => coerce(t, s)?,
                    None => Value::str(s),
                };
                out.push((name.clone(), v));
            }
        }
    }
    if def.len() == segs.len() {
        Some(out)
    } else {
        None
    }
}

/// The media type of a content-type header, without its parameters:
/// "Application/JSON; charset=utf-8" is "application/json".
fn essence(ctype: &str) -> String {
    ctype.split(';').next().unwrap_or("").trim().to_ascii_lowercase()
}

/// `application/json`, or a JSON-based type such as `application/ld+json`.
fn is_json_type(ctype: &str) -> bool {
    let e = essence(ctype);
    e == "application/json" || (e.starts_with("application/") && e.ends_with("+json"))
}

/// Whether a request was sent by a page on another site. Browsers say so in
/// `Sec-Fetch-Site`; older ones only send `Origin`, which then has to name
/// the host the request was sent to.
fn cross_site(req: &HttpRequest) -> bool {
    let header = |name: &str| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim());
    if let Some(site) = header("sec-fetch-site") {
        return !matches!(site.to_ascii_lowercase().as_str(), "same-origin" | "none");
    }
    match (header("origin"), header("host")) {
        (None, _) => false,
        (Some(origin), host) => {
            let origin_host = origin.split_once("://").map_or(origin, |(_, h)| h).trim_end_matches('/');
            host.map_or(true, |h| !origin_host.eq_ignore_ascii_case(h))
        }
    }
}

/// The `request` value a route sees.
fn build_request(req: &HttpRequest, path: &str, query: &Map) -> Map {
    let mut headers = Map::new();
    for (k, v) in &req.headers {
        headers.insert(k.to_ascii_lowercase(), Value::str(v));
    }
    let header = |name: &str| headers.get(name).map(|v| v.display()).unwrap_or_default();
    let ctype = header("content-type");
    let mut cookies = Map::new();
    for part in header("cookie").split(';') {
        if let Some((k, v)) = part.trim().split_once('=') {
            cookies.insert(k.to_string(), Value::str(percent_decode(v, false)));
        }
    }
    let body = String::from_utf8_lossy(&req.body).into_owned();
    let trimmed = body.trim_start();
    // A body is JSON when it says so, or says nothing and looks like it
    // (curl, scripts). Not when it says it is something else: a page on
    // another site can send `text/plain` without asking first, and must
    // not reach routes that expect JSON.
    let looks_json = trimmed.starts_with('{') || trimmed.starts_with('[');
    let json = if is_json_type(&ctype) || (ctype.trim().is_empty() && looks_json) {
        serde_json::from_str::<serde_json::Value>(&body).map(|j| Value::from_json(&j)).unwrap_or(Value::Nil)
    } else {
        Value::Nil
    };
    let form = if ctype.contains("application/x-www-form-urlencoded") { parse_urlencoded(&body) } else { Map::new() };

    let mut m = Map::new();
    m.insert("method".into(), Value::str(&req.method));
    m.insert("path".into(), Value::str(path));
    m.insert("query".into(), Value::map(query.clone()));
    m.insert("params".into(), Value::map(Map::new()));
    m.insert("headers".into(), Value::map(headers.clone()));
    m.insert("cookies".into(), Value::map(cookies));
    m.insert("body".into(), Value::str(body));
    m.insert("json".into(), json);
    m.insert("form".into(), Value::map(form));
    m.insert("ip".into(), Value::str(&req.remote));
    m
}

impl Interp {
    /// Runs the first route whose method, path and patterns all match.
    pub fn handle(&mut self, req: &HttpRequest) -> HttpResponse {
        // A mounted program answers everything under its prefix, seeing the
        // path without it.
        let path = req.url.split('?').next().unwrap_or("");
        for (prefix, sub) in self.mounts.iter_mut() {
            if path == prefix.as_str() || path.starts_with(&format!("{}/", prefix)) {
                let rest = &req.url[prefix.len()..];
                let mut inner = req.clone();
                inner.url = if rest.is_empty() || rest.starts_with('?') { format!("/{}", rest) } else { rest.to_string() };
                return sub.handle(&inner);
            }
        }
        self.rendered_island = false;
        let mut resp = self.handle_inner(req);
        if !self.base.is_empty() {
            for (k, v) in resp.headers.iter_mut() {
                if k.eq_ignore_ascii_case("location") {
                    *v = self.with_base(v);
                }
            }
        }
        // Pages with components load the browser bundle.
        let html = resp.header("content-type").map_or(false, |c| c.starts_with("text/html"));
        if html && req.method != "HEAD" && std::mem::take(&mut self.rendered_island) {
            let has = |needle: &[u8]| resp.body.windows(needle.len()).rposition(|w| w == needle);
            if has(b"/_quillo/quillo.js").is_none() {
                let tag = format!("<script src=\"{}/_quillo/quillo.js\" defer></script>", self.base);
                let at = has(b"</body>").unwrap_or(resp.body.len());
                resp.body.splice(at..at, tag.into_bytes());
            }
        }
        resp
    }

    fn handle_inner(&mut self, req: &HttpRequest) -> HttpResponse {
        let (path, qs) = req.url.split_once('?').unwrap_or((req.url.as_str(), ""));
        if let Some(rest) = path.strip_prefix("/_quillo/") {
            return self.handle_internal(rest, req, path, qs);
        }
        let segs: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(|s| percent_decode(s, false)).collect();
        let method = if req.method == "HEAD" { "GET" } else { req.method.as_str() };
        let query = parse_urlencoded(qs);
        let query_v = Value::map(query.clone());
        let base = build_request(req, path, &query);

        let mut path_matched = false;
        let mut method_matched = false;
        let routes = self.routes.clone();
        for r in &routes {
            let Some(params) = match_path(&r.path, &segs) else { continue };
            path_matched = true;
            if r.method != "ANY" && r.method != method {
                continue;
            }
            method_matched = true;

            let mut reqmap = base.clone();
            reqmap.insert("params".into(), Value::map(params.iter().cloned().collect()));
            let reqv = Value::map(reqmap);
            let scope = Scope::child(&self.globals);
            define(&scope, "request", reqv.clone());
            for (k, v) in params {
                define(&scope, &k, v);
            }

            let mut binds = Vec::new();
            let checks = [(r.query.as_ref(), &query_v, Mode::Lenient), (r.body_pat.as_ref(), &reqv, Mode::Request)];
            let mut matched = true;
            for (pat, val, mode) in checks {
                let Some(pat) = pat else { continue };
                match self.match_pat(pat, val, mode, &scope, &mut binds) {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        matched = false;
                        break;
                    }
                    Err(e) => return finish(Err(e)),
                }
            }
            if !matched {
                continue;
            }
            for (k, v) in binds {
                define(&scope, &k, v);
            }

            self.current_request = Some(reqv);
            let mut resp = finish(self.eval(&r.body, &scope));
            self.current_request = None;
            if req.method == "HEAD" {
                resp.body.clear();
            }
            return resp;
        }
        if method_matched {
            plain(400, "bad request: no route pattern matches this request\n")
        } else if path_matched {
            plain(405, "method not allowed\n")
        } else {
            plain(404, "not found\n")
        }
    }
}

impl Interp {
    /// `/_quillo/...`: the browser bundle and `server fn` calls.
    fn handle_internal(&mut self, rest: &str, req: &HttpRequest, path: &str, qs: &str) -> HttpResponse {
        if rest == "quillo.js" {
            // The browser keeps its copy and asks whether it is still current.
            let etag = format!("\"{:016x}\"", {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                self.client_js.hash(&mut h);
                h.finish()
            });
            let fresh = req.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("if-none-match") && v.contains(&etag));
            return HttpResponse {
                status: if fresh { 304 } else { 200 },
                headers: vec![
                    ("content-type".into(), "text/javascript; charset=utf-8".into()),
                    ("cache-control".into(), "no-cache".into()),
                    ("etag".into(), etag),
                ],
                body: if fresh { Vec::new() } else { self.client_js.as_bytes().to_vec() },
            };
        }
        if rest == "live" {
            // Without waiting; the server's worker threads answer `live`
            // requests themselves and wait for a change first.
            let (since, server, topics) = live_query(qs);
            let (seq, changed) = self.shared.wait_live(since, server.as_deref(), &topics, std::time::Duration::ZERO);
            return json_response(200, serde_json::json!({ "seq": seq, "server": self.shared.id, "changed": changed }));
        }
        let Some(name) = rest.strip_prefix("rpc/") else {
            return plain(404, "not found\n");
        };
        if req.method != "POST" {
            return plain(405, "method not allowed\n");
        }
        // Only the program's own pages may call its server fns. A JSON
        // content type cannot be sent across sites without the browser
        // asking first (which nothing here answers), and the browser says
        // where a request came from.
        let ctype = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.as_str());
        if essence(ctype.unwrap_or("")) != "application/json" {
            return rpc_error(415, "server fn calls must send JSON (content-type: application/json)");
        }
        if cross_site(req) {
            return rpc_error(403, "server fns can only be called from the program's own pages");
        }
        if !self.server_fns.iter().any(|n| n == name) {
            return rpc_error(404, &format!("no server fn `{}`", name));
        }
        let Some(Value::Func(c)) = lookup(&self.globals, name) else {
            return rpc_error(404, &format!("no server fn `{}`", name));
        };
        let args = match serde_json::from_slice::<serde_json::Value>(&req.body) {
            Ok(serde_json::Value::Array(a)) => a.iter().map(Value::from_json).collect::<Vec<_>>(),
            _ => return rpc_error(400, "the body must be a JSON array of arguments"),
        };
        if let Err(msg) = check_args(&c.def, &args) {
            return rpc_error(400, &msg);
        }
        let query = parse_urlencoded(qs);
        self.current_request = Some(Value::map(build_request(req, path, &query)));
        // A `live` refresh: also report what the function read.
        let live = req.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("x-quillo-live"));
        let seq = self.shared.live_seq();
        if live {
            self.reads = Some(Default::default());
        }
        let res = self.call_closure(&c, args.into_iter().map(Some).collect(), 0);
        let topics = self.reads.take().map(|r| {
            let mut t: Vec<String> = r.into_iter().collect();
            t.sort();
            t
        });
        self.current_request = None;
        let with_topics = |mut j: serde_json::Value| {
            if let Some(t) = &topics {
                j["topics"] = serde_json::json!(t);
                j["seq"] = serde_json::json!(seq);
                j["server"] = serde_json::json!(self.shared.id);
            }
            j
        };
        match res {
            Ok(v) | Err(Ctrl::Return(v)) => match v {
                Value::Response(r) => render_response(&r),
                Value::Err(e) => json_response(200, with_topics(serde_json::json!({ "err": e.to_json() }))),
                v => json_response(200, with_topics(serde_json::json!({ "ok": v.to_json() }))),
            },
            Err(Ctrl::Reply(r)) => render_response(&r),
            Err(Ctrl::Error(e)) => {
                eprintln!("quillo: error in server fn `{}`: {}", name, e);
                rpc_error(500, &format!("internal error in `{}`", name))
            }
        }
    }
}

/// `/_quillo/live?since=N&s=S&t=a,b`: the sequence number the page has
/// seen, the server it came from, and the topics it is showing.
fn live_query(qs: &str) -> (u64, Option<String>, Vec<String>) {
    let q = parse_urlencoded(qs);
    let since = q.get("since").and_then(|v| v.display().parse::<u64>().ok()).unwrap_or(0);
    let server = q.get("s").map(|v| v.display()).filter(|s| !s.is_empty());
    let topics: Vec<String> = q
        .get("t")
        .map(|v| v.display().split(',').filter(|s| !s.is_empty()).take(MAX_TOPICS).map(String::from).collect())
        .unwrap_or_default();
    (since, server, topics)
}

/// Most topics one page can wait on; each change looks through them all.
const MAX_TOPICS: usize = 256;

/// Most pages that can wait for changes at once (live data and `--watch`
/// reloads). Each has a thread; beyond this, pages are told to try later
/// rather than let anyone with a browser start threads without end.
pub const MAX_WAITERS: usize = 4096;

/// Stack for the threads that wait for changes; they run no Quillo code.
const WAITER_STACK: usize = 256 << 10;

static WAITERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A place among the `MAX_WAITERS`, given back when dropped.
struct Waiter;

impl Waiter {
    fn take() -> Option<Waiter> {
        use std::sync::atomic::Ordering::SeqCst;
        if WAITERS.fetch_add(1, SeqCst) >= MAX_WAITERS {
            WAITERS.fetch_sub(1, SeqCst);
            return None;
        }
        Some(Waiter)
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        WAITERS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Starts `f` on a thread of its own, holding one of the waiting places;
/// answers 503 when they are all taken.
fn spawn_waiter(name: &str, rq: tiny_http::Request, f: impl FnOnce(tiny_http::Request) + Send + 'static) {
    let Some(place) = Waiter::take() else {
        let mut resp = tiny_http::Response::from_string("too many pages are waiting for changes; try again later\n")
            .with_status_code(503);
        resp.add_header(tiny_http::Header::from_bytes(&b"retry-after"[..], &b"5"[..]).unwrap());
        let _ = rq.respond(resp);
        return;
    };
    // `rq` must come back if the thread cannot start, so it goes through a slot.
    let slot = Arc::new(Mutex::new(Some(rq)));
    let inner = slot.clone();
    let started = std::thread::Builder::new().name(name.into()).stack_size(WAITER_STACK).spawn(move || {
        let _place = place;
        if let Some(rq) = inner.lock().unwrap_or_else(|e| e.into_inner()).take() {
            f(rq);
        }
    });
    if started.is_err() {
        if let Some(rq) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = rq.respond(tiny_http::Response::empty(503));
        }
    }
}

fn live_json(shared: &Shared, seq: u64, changed: &[String]) -> String {
    serde_json::json!({ "seq": seq, "server": shared.id, "changed": changed }).to_string()
}

/// Answers `/_quillo/live` on a thread of its own, so open pages do not hold
/// on to request workers. Over a WebSocket, each change to the topics is
/// sent as `{seq, server, changed}` for as long as the page stays; as a
/// plain request, the answer is the first change, or none after a while.
fn serve_live(shared: Arc<Shared>, rq: tiny_http::Request) {
    let (mut since, server, topics) = live_query(rq.url().split_once('?').map_or("", |(_, q)| q));
    spawn_waiter("quillo-live", rq, move |rq| {
        if crate::ws::is_upgrade(&rq) {
            let Some(mut socket) = crate::ws::Socket::accept(rq) else { return };
            let mut server = server;
            loop {
                let (seq, changed) = shared.wait_live(since, server.as_deref(), &topics, Duration::from_secs(20));
                // A page that did not know the sequence number, or knew one
                // from another server or from before a restart, is told the
                // current one.
                let alive = if !changed.is_empty() || since == 0 || since > seq || server.is_some() {
                    socket.send(&live_json(&shared, seq, &changed))
                } else {
                    socket.ping()
                };
                if !alive {
                    return;
                }
                since = seq;
                server = None;
            }
        }
        let (seq, changed) = shared.wait_live(since, server.as_deref(), &topics, Duration::from_secs(25));
        let mut resp = tiny_http::Response::from_string(live_json(&shared, seq, &changed));
        for (k, v) in [("content-type", "application/json"), ("cache-control", "no-store")] {
            resp.add_header(tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap());
        }
        let _ = rq.respond(resp);
    });
}

/// Checks a browser's arguments against a `server fn`'s parameters.
fn check_args(def: &crate::ast::FnDef, args: &[Value]) -> Result<(), String> {
    if args.len() > def.params.len() {
        return Err(format!("`{}` takes {} arguments, got {}", def.name, def.params.len(), args.len()));
    }
    for (i, p) in def.params.iter().enumerate() {
        match args.get(i) {
            None if p.default.is_none() => return Err(format!("missing argument `{}` for `{}`", p.name, def.name)),
            Some(v) => {
                if let Some(ty) = &p.ty {
                    if !crate::interp::type_matches(ty, v) {
                        return Err(format!("argument `{}` of `{}` must be {}, got {}", p.name, def.name, ty.text(), v.type_name()));
                    }
                }
            }
            None => {}
        }
    }
    Ok(())
}

fn json_response(status: u16, j: serde_json::Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![("content-type".into(), "application/json".into())],
        body: j.to_string().into_bytes(),
    }
}

fn rpc_error(status: u16, msg: &str) -> HttpResponse {
    json_response(status, serde_json::json!({ "error": msg }))
}

/// Turns what a route produced into an HTTP response.
pub fn finish(res: Result<Value, Ctrl>) -> HttpResponse {
    match res {
        Ok(v) | Err(Ctrl::Return(v)) => value_response(v),
        Err(Ctrl::Reply(r)) => render_response(&r),
        Err(Ctrl::Error(e)) => {
            eprintln!("quillo: error: {}", e);
            // The message names files and lines of the program; it is for
            // its author (`--watch`), not for whoever sent the request.
            if SHOW_ERRORS.load(std::sync::atomic::Ordering::Relaxed) {
                plain(500, &format!("internal error: {}\n", e))
            } else {
                plain(500, "internal error\n")
            }
        }
    }
}

/// Whether a failed request is answered with the error message (in
/// development, with `--watch`) or only "internal error".
pub static SHOW_ERRORS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn value_response(v: Value) -> HttpResponse {
    match v {
        Value::Response(r) => render_response(&r),
        other => {
            let status = match &other {
                Value::Nil => 204,
                Value::Err(_) => 500,
                _ => 200,
            };
            render_response(&Response { status, headers: Vec::new(), body: Body::Value(other) })
        }
    }
}

/// Body encoding follows the value: html -> text/html, str -> text/plain,
/// null -> empty, anything else -> JSON.
fn render_response(r: &Response) -> HttpResponse {
    let (ctype, body): (Option<&str>, Vec<u8>) = match &r.body {
        Body::Bytes(b) => (None, b.clone()),
        Body::Value(v) => match v {
            Value::Nil => (None, Vec::new()),
            Value::Html(h) => (Some("text/html; charset=utf-8"), h.as_bytes().to_vec()),
            Value::Str(s) => (Some("text/plain; charset=utf-8"), s.as_bytes().to_vec()),
            Value::Err(e) => {
                (Some("application/json"), serde_json::json!({ "error": e.to_json() }).to_string().into_bytes())
            }
            other => (Some("application/json"), other.to_json().to_string().into_bytes()),
        },
    };
    let mut headers = r.headers.clone();
    if let Some(ct) = ctype {
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
            headers.push(("content-type".into(), ct.into()));
        }
    }
    HttpResponse { status: r.status, headers, body }
}

/// A program as currently loaded, or the error that stopped it from loading.
pub enum Loaded {
    Ready(Interp),
    Failed(String),
}

/// The running program, which `--watch` swaps out when its files change.
pub struct App {
    slot: Mutex<(u64, Arc<Loaded>)>,
    /// Signalled when `slot` changes, for `/_quillo/events`.
    changed: Condvar,
    shared: Arc<Shared>,
    main_file: PathBuf,
    base_dir: PathBuf,
    /// Development mode: reload on change and refresh open pages.
    pub watch: bool,
    /// Run the static checker before each reload.
    pub static_check: bool,
    /// One of several servers (`--register`): the key announcements from
    /// the others must carry.
    pub cluster_key: Option<String>,
    /// When limer has no admin secret: the addresses announcements may come
    /// from (besides this machine).
    pub cluster_sources: Option<Vec<std::net::IpAddr>>,
    /// Largest request body accepted, in bytes.
    pub max_body: usize,
    /// A desktop app (`quillo app`): only requests that carry this token
    /// are answered (see `launch_gate`).
    pub launch_token: Option<String>,
}

impl App {
    /// `loaded` must have been loaded with `shared`; the caller holds the lock.
    pub fn new(loaded: Loaded, shared: Arc<Shared>, main_file: &Path, base_dir: &Path, watch: bool) -> Self {
        App {
            slot: Mutex::new((1, Arc::new(loaded))),
            changed: Condvar::new(),
            shared,
            main_file: main_file.to_path_buf(),
            base_dir: base_dir.to_path_buf(),
            watch,
            static_check: true,
            cluster_key: None,
            cluster_sources: None,
            max_body: MAX_BODY,
            launch_token: None,
        }
    }

    /// What the program's interpreters share.
    pub fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
    }

    fn current(&self) -> (u64, Arc<Loaded>) {
        let g = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        (g.0, g.1.clone())
    }

    /// Loads the program again from disk and makes it current. A program that
    /// fails to load does not replace one that works.
    pub fn reload(&self) {
        let mut problems = String::new();
        if self.static_check {
            let analysis = crate::check::check_file(&self.main_file);
            for d in &analysis.diagnostics {
                eprintln!("{}", d);
            }
            problems = analysis.errors().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
        }
        self.shared.gil.acquire();
        let mut it = Interp::with_shared(&self.base_dir, self.shared.clone());
        let res = if problems.is_empty() {
            it.load_file(&self.main_file)
        } else {
            Err(crate::error::QuilloError::new(problems, 0))
        };
        self.shared.gil.release();
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        match res {
            Ok(_) => {
                eprintln!("quillo: reloaded {} ({} routes)", self.main_file.display(), it.routes.len());
                *slot = (slot.0 + 1, Arc::new(Loaded::Ready(it)));
            }
            Err(e) => {
                eprintln!("quillo: {}", e);
                if matches!(*slot.1, Loaded::Failed(_)) {
                    *slot = (slot.0 + 1, Arc::new(Loaded::Failed(e.to_string())));
                } else {
                    eprintln!("quillo: still serving the previous version");
                }
            }
        }
        self.changed.notify_all();
    }

    /// Waits up to `timeout` for a version newer than `seen`.
    fn wait_for_change(&self, seen: u64, timeout: Duration) -> u64 {
        let slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        let (slot, _) = self
            .changed
            .wait_timeout_while(slot, timeout, |s| s.0 == seen)
            .unwrap_or_else(|e| e.into_inner());
        slot.0
    }

    fn watched_files(&self) -> Vec<PathBuf> {
        let mut files = vec![self.main_file.clone()];
        if let Loaded::Ready(it) = &*self.current().1 {
            files.extend(it.files());
        }
        files
    }
}

fn mtimes(files: &[PathBuf]) -> Vec<Option<std::time::SystemTime>> {
    files.iter().map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok()).collect()
}

/// Polls the program's files and reloads it when one changes.
fn watch(app: Arc<App>) {
    let mut files = app.watched_files();
    let mut seen = mtimes(&files);
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let now = mtimes(&files);
        if now != seen {
            // Editors often write in several steps; let them finish.
            std::thread::sleep(Duration::from_millis(50));
            app.reload();
            files = app.watched_files();
            seen = mtimes(&files);
        }
    }
}

/// Reloads the page when the program changes. The page starts out knowing
/// the version it was rendered from, and waits on a WebSocket to
/// `/_quillo/events?v=N`, which sends the new version once it is no longer N.
/// A change made between rendering and connecting is therefore not missed;
/// and if the server restarts, the page sees a different version and reloads
/// too. Where the WebSocket cannot open, the page asks the same URL as a
/// plain request, which answers after a change or a while, and tries the
/// WebSocket again after that.
fn dev_script(version: u64) -> String {
    format!(
        "<script>(()=>{{const v='{}',u='/_quillo/events?v='+v,go=()=>{{let o=0;const s=new WebSocket(location.origin.replace(/^http/,'ws')+u);s.onopen=()=>o=1;s.onmessage=e=>{{if(e.data!==v)location.reload()}};s.onclose=()=>o?setTimeout(go,1000):fetch(u,{{cache:'no-store'}}).then(r=>r.text()).then(t=>t!==v?location.reload():go(),()=>setTimeout(go,1000))}};go()}})()</script>",
        version
    )
}

/// Answers `/_quillo/events[?v=N]` on a thread of its own, so pages waiting
/// for a change do not hold on to request workers.
fn serve_events(app: Arc<App>, rq: tiny_http::Request) {
    let seen = rq.url().split_once("?v=").and_then(|(_, v)| v.parse::<u64>().ok());
    spawn_waiter("quillo-events", rq, move |rq| {
        if crate::ws::is_upgrade(&rq) {
            let Some(mut socket) = crate::ws::Socket::accept(rq) else { return };
            let seen = seen.unwrap_or(0);
            loop {
                let v = app.wait_for_change(seen, Duration::from_secs(20));
                if v != seen {
                    socket.send(&v.to_string());
                    return socket.close();
                }
                if !socket.ping() {
                    return;
                }
            }
        }
        let v = match seen {
            Some(seen) => app.wait_for_change(seen, Duration::from_secs(25)),
            None => app.current().0,
        };
        let mut resp = tiny_http::Response::from_string(v.to_string());
        resp.add_header(tiny_http::Header::from_bytes(&b"cache-control"[..], &b"no-store"[..]).unwrap());
        let _ = rq.respond(resp);
    });
}

/// In watch mode, HTML pages get a script that reloads them after a change.
fn inject_dev_script(resp: &mut HttpResponse, version: u64) {
    let html = resp.header("content-type").map_or(false, |c| c.starts_with("text/html"));
    if !html {
        return;
    }
    let body = String::from_utf8_lossy(&resp.body).into_owned();
    resp.body = match body.rfind("</body>") {
        Some(i) => format!("{}{}{}", &body[..i], dev_script(version), &body[i..]),
        None => format!("{}{}", body, dev_script(version)),
    }
    .into_bytes();
}

fn error_page(msg: &str) -> HttpResponse {
    HttpResponse {
        status: 500,
        headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
        body: format!(
            "<!doctype html><title>Quillo error</title><body style=\"font-family:monospace\"><h1>The program did not load</h1><pre>{}</pre></body>",
            crate::interp::escape(msg)
        )
        .into_bytes(),
    }
}

/// Reads a request, or `None` when its body is larger than `max_body`.
fn to_request(rq: &mut tiny_http::Request, max_body: usize) -> Option<HttpRequest> {
    use std::io::Read;
    if rq.body_length().map_or(false, |n| n > max_body) {
        return None;
    }
    let mut body = Vec::new();
    let _ = Read::take(rq.as_reader(), max_body as u64 + 1).read_to_end(&mut body);
    if body.len() > max_body {
        return None;
    }
    Some(HttpRequest {
        method: rq.method().as_str().to_ascii_uppercase(),
        url: rq.url().to_string(),
        headers: rq.headers().iter().map(|h| (h.field.as_str().as_str().to_string(), h.value.as_str().to_string())).collect(),
        body,
        remote: rq.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default(),
    })
}

/// Largest request body accepted by default (`--max-body`), in bytes.
pub const MAX_BODY: usize = 10 << 20;

/// Whether a header can be sent as it is: a name made of token characters,
/// and a value without line breaks or other control characters, which would
/// let text from a request add headers of its own.
pub fn valid_header(name: &str, value: &str) -> bool {
    let token = |c: u8| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c);
    !name.is_empty()
        && name.bytes().all(token)
        && value.bytes().all(|c| c == b'\t' || (c >= 0x20 && c != 0x7f))
}

/// Runs a request in the program, holding the lock. A panic (a bug in
/// Quillo itself) answers 500 instead of stopping this worker for good
/// with the lock still held, which would stop the whole server.
fn run_program(app: &App, it: &mut Interp, req: &HttpRequest) -> Option<HttpResponse> {
    app.shared.gil.acquire();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| it.handle(req)));
    app.shared.gil.release();
    r.ok()
}

/// Gzips text responses of 1 KiB or more for clients that accept it.
fn compress(req: &HttpRequest, resp: &mut HttpResponse) {
    use std::io::Write;
    if resp.body.len() < 1024 || resp.header("content-encoding").is_some() {
        return;
    }
    let ctype = resp.header("content-type").unwrap_or("");
    let text = ctype.starts_with("text/") || ctype.contains("json") || ctype.contains("javascript") || ctype.contains("svg");
    let accepts = req
        .headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("accept-encoding") && v.split(',').any(|e| e.trim().starts_with("gzip")));
    if !text || !accepts {
        return;
    }
    let mut enc = flate2::write::GzEncoder::new(Vec::with_capacity(resp.body.len() / 3), flate2::Compression::default());
    if enc.write_all(&resp.body).is_err() {
        return;
    }
    if let Ok(gz) = enc.finish() {
        resp.body = gz;
        resp.headers.push(("content-encoding".into(), "gzip".into()));
        resp.headers.push(("vary".into(), "accept-encoding".into()));
    }
}

fn worker(app: Arc<App>, server: Arc<tiny_http::Server>) {
    let mut generation = 0;
    let mut local: Option<Interp> = None;
    let mut program = app.current().1;
    while let Ok(mut rq) = server.recv() {
        if let Some(token) = &app.launch_token {
            if let Some(resp) = launch_gate(&rq, token) {
                let _ = rq.respond(resp);
                continue;
            }
        }
        if app.watch && rq.url().split('?').next() == Some("/_quillo/events") {
            serve_events(app.clone(), rq);
            continue;
        }
        if rq.url().split('?').next().map_or(false, |p| p.ends_with("/_quillo/live")) {
            serve_live(app.shared.clone(), rq);
            continue;
        }
        if let (Some(key), "/_quillo/announce") = (&app.cluster_key, rq.url()) {
            // Without a secret, the key is no secret either: only limer's
            // own host may announce.
            let from = rq.remote_addr().map(|a| a.ip());
            if let Some(allowed) = &app.cluster_sources {
                if !from.map_or(false, |ip| ip.is_loopback() || allowed.contains(&ip)) {
                    let _ = rq.respond(tiny_http::Response::empty(403));
                    continue;
                }
            }
            let mut body = Vec::new();
            let _ = std::io::Read::read_to_end(&mut std::io::Read::take(rq.as_reader(), 1 << 16), &mut body);
            let status = crate::cluster::announced(&app.shared, key, &body);
            let _ = rq.respond(tiny_http::Response::empty(status));
            continue;
        }
        let start = Instant::now();
        let Some(req) = to_request(&mut rq, app.max_body) else {
            eprintln!("{} {} -> 413 (body larger than {} bytes)", rq.method(), rq.url(), app.max_body);
            let resp = tiny_http::Response::from_string("request body too large\n").with_status_code(413);
            let _ = rq.respond(resp.with_header(tiny_http::Header::from_bytes(&b"connection"[..], &b"close"[..]).unwrap()));
            continue;
        };
        let (g, current) = app.current();
        if g != generation {
            generation = g;
            local = match &*current {
                Loaded::Ready(it) => Some(it.fork()),
                Loaded::Failed(_) => None,
            };
            program = current;
        }
        let mut resp = if app.watch && req.url == "/_quillo/dev" {
            plain(200, &generation.to_string())
        } else {
            match (&mut local, &*program) {
                (Some(it), _) => match run_program(&app, it, &req) {
                    Some(r) => r,
                    None => {
                        eprintln!("quillo: internal error (a bug in Quillo) while handling {} {}", req.method, req.url);
                        // Its state may be half-updated; start afresh.
                        local = None;
                        generation = 0;
                        plain(500, "internal error\n")
                    }
                },
                (None, Loaded::Failed(e)) => error_page(e),
                (None, Loaded::Ready(_)) => unreachable!(),
            }
        };
        if app.watch {
            inject_dev_script(&mut resp, generation);
        }
        // Chrome asks every site for this while DevTools is open; it is not
        // the program's business, so it is not logged.
        let devtools_probe = req.url == "/.well-known/appspecific/com.chrome.devtools.json";
        if !devtools_probe {
            eprintln!(
                "{} {} -> {} ({:.1}ms)",
                req.method,
                req.url,
                resp.status,
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        compress(&req, &mut resp);
        if resp.header("x-content-type-options").is_none() {
            resp.headers.push(("x-content-type-options".into(), "nosniff".into()));
        }
        let mut out = tiny_http::Response::from_data(resp.body).with_status_code(resp.status);
        for (k, v) in &resp.headers {
            if !valid_header(k, v) {
                eprintln!("quillo: header `{}` not sent: its name or value has characters headers cannot have", k.escape_debug());
                continue;
            }
            if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
                out.add_header(h);
            }
        }
        let _ = rq.respond(out);
    }
}

/// Serves the app with `workers` threads. The calling thread must hold the
/// interpreter lock; it gives it up here.
pub fn serve(app: App, addr: &str, workers: usize) -> Result<(), String> {
    serve_with(app, addr, workers, |_| {})
}

/// `serve`, calling `listening` with the address once it is bound (with
/// port 0, the port the system chose).
pub fn serve_with(app: App, addr: &str, workers: usize, listening: impl FnOnce(std::net::SocketAddr)) -> Result<(), String> {
    let server = tiny_http::Server::http(addr).map_err(|e| format!("cannot listen on {}: {}", addr, e))?;
    let bound = server.server_addr().to_ip();
    let shown = bound.map_or_else(|| addr.to_string(), |a| a.to_string());
    let server = Arc::new(server);
    let app = Arc::new(app);
    if let Loaded::Ready(it) = &*app.current().1 {
        let n = it.routes.len();
        eprintln!("quillo: {} route{} listening on http://{}", n, if n == 1 { "" } else { "s" }, shown);
    }
    if app.watch {
        eprintln!("quillo: watching for changes");
    }
    SHOW_ERRORS.store(app.watch, std::sync::atomic::Ordering::Relaxed);
    app.shared.gil.release();
    let mut handles = Vec::new();
    for i in 0..workers.max(1) {
        let (app, server) = (app.clone(), server.clone());
        let h = std::thread::Builder::new()
            .name(format!("quillo-worker-{}", i))
            .stack_size(WORKER_STACK)
            .spawn(move || {
                worker(app, server)
            })
            .map_err(|e| format!("cannot start worker thread: {}", e))?;
        handles.push(h);
    }
    if app.watch {
        let app = app.clone();
        std::thread::Builder::new()
            .name("quillo-watch".into())
            .stack_size(WORKER_STACK)
            .spawn(move || {
                watch(app)
            })
            .map_err(|e| format!("cannot start file watcher: {}", e))?;
    }
    if let Some(a) = bound {
        listening(a);
    }
    for h in handles {
        let _ = h.join();
    }
    Ok(())
}

/// The cookie that admits a desktop app's window.
const LAUNCH_COOKIE: &str = "quillo_launch";

/// For a desktop app's server, which listens on this machine where other
/// programs and web pages could reach it: `None` lets the request through
/// when it carries the launch cookie. The window's first request carries the
/// token in the URL instead (`?_quillo_launch=...`); it is answered with the
/// cookie and a redirect to the same URL without the token. Anything else is
/// refused. The cookie is `SameSite=Strict` and bound to 127.0.0.1, so pages
/// on other sites (even ones whose names resolve to 127.0.0.1) never send it.
pub fn launch_gate(rq: &tiny_http::Request, token: &str) -> Option<tiny_http::ResponseBox> {
    let refuse = || Some(tiny_http::Response::from_string("forbidden\n").with_status_code(403).boxed());
    let cookies = rq.headers().iter().filter(|h| h.field.equiv("cookie")).flat_map(|h| h.value.as_str().split(';'));
    let mut has_cookie = false;
    for c in cookies {
        if let Some((k, v)) = c.trim().split_once('=') {
            if k == LAUNCH_COOKIE && same_secret(v, token) {
                has_cookie = true;
            }
        }
    }
    let (path, query) = rq.url().split_once('?').unwrap_or((rq.url(), ""));
    let param = "_quillo_launch=";
    let offered = query.split('&').find_map(|kv| kv.strip_prefix(param));
    match offered {
        None if has_cookie => None,
        None => refuse(),
        Some(v) if !same_secret(v, token) => refuse(),
        Some(_) => {
            let rest: Vec<&str> = query.split('&').filter(|kv| !kv.starts_with(param) && !kv.is_empty()).collect();
            let location = if rest.is_empty() { path.to_string() } else { format!("{}?{}", path, rest.join("&")) };
            let cookie = format!("{}={}; Path=/; HttpOnly; SameSite=Strict", LAUNCH_COOKIE, token);
            let resp = tiny_http::Response::empty(303)
                .with_header(tiny_http::Header::from_bytes(&b"location"[..], location.as_bytes()).ok()?)
                .with_header(tiny_http::Header::from_bytes(&b"set-cookie"[..], cookie.as_bytes()).ok()?)
                .with_header(tiny_http::Header::from_bytes(&b"cache-control"[..], &b"no-store"[..]).ok()?);
            Some(resp.boxed())
        }
    }
}

/// Compares in time that does not depend on where the first difference is.
fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

/// A new launch token: 32 random bytes, in hex.
pub fn launch_token() -> Result<String, String> {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).map_err(|e| format!("no random numbers for the launch token: {}", e))?;
    Ok(crate::ws::hex(&b))
}

/// Stack for threads that run Quillo code. Calls grow the stack on demand
/// beyond this; it only has to cover deeply nested source and data.
pub const WORKER_STACK: usize = 16 << 20;
