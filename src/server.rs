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
        m.insert(percent_decode(k, true).into(), Value::str(percent_decode(v, true)));
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

/// Whether a request's body is one another site's page can send without
/// the browser asking first: a form, multipart, plain text, or none.
fn sends_form(req: &HttpRequest) -> bool {
    let ctype = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.as_str()).unwrap_or("");
    matches!(essence(ctype).as_str(), "" | "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain")
}

/// The `request` value a route sees.
/// What `build_request` needs, kept until the request map is wanted.
pub(crate) struct PendingRequest {
    /// The request being handled; set only while it is handled.
    req: *const HttpRequest,
    path: String,
    query: Value,
    params: Value,
}

// SAFETY: it points at the request its handler is answering, and is
// cleared before the handler returns.
unsafe impl Send for PendingRequest {}
unsafe impl Sync for PendingRequest {}

impl PendingRequest {
    pub(crate) fn make(&self) -> Value {
        // SAFETY: see above.
        let req = unsafe { &*self.req };
        let mut m = build_request(req, &self.path, &self.query);
        m.insert("params".into(), self.params.clone());
        Value::map(m)
    }
}

fn build_request(req: &HttpRequest, path: &str, query: &Value) -> Map {
    let header = |name: &str| {
        req.headers.iter().rev().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str()).unwrap_or("")
    };
    let ctype = header("content-type").to_string();
    let mut cookies = Map::new();
    for part in header("cookie").split(';') {
        if let Some((k, v)) = part.trim().split_once('=') {
            cookies.insert(k.into(), Value::str(percent_decode(v, false)));
        }
    }
    // Files: their bytes stay in the body (see `upload`), which is no text.
    let multipart = crate::upload::boundary(&ctype);
    let body = if multipart.is_some() { String::new() } else { String::from_utf8_lossy(&req.body).into_owned() };
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
    let (form, files) = match &multipart {
        Some(b) => crate::upload::parse(b, &req.body),
        None if ctype.contains("application/x-www-form-urlencoded") => (parse_urlencoded(&body), Map::new()),
        None => (Map::new(), Map::new()),
    };

    let mut headers = Map::with_capacity(req.headers.len());
    for (k, v) in &req.headers {
        headers.insert(k.to_ascii_lowercase().into(), Value::str(v));
    }
    let mut m = Map::with_capacity(10);
    m.insert("method".into(), Value::str(&req.method));
    m.insert("path".into(), Value::str(path));
    m.insert("query".into(), query.clone());
    m.insert("params".into(), Value::map(Map::new()));
    m.insert("headers".into(), Value::map(headers));
    m.insert("cookies".into(), Value::map(cookies));
    m.insert("body".into(), Value::str(body));
    m.insert("json".into(), json);
    m.insert("form".into(), Value::map(form));
    m.insert("files".into(), Value::map(files));
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
        self.island_names.clear();
        self.tenant = None;
        self.tenant_named = false;
        self.upload_body = None;
        self.set_cookies.clear();
        self.session = None;
        self.locale = None;
        if !req.body.is_empty() {
            let ctype = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map_or("", |(_, v)| v.as_str());
            if crate::upload::boundary(ctype).is_some() {
                self.upload_body = Some(Arc::from(req.body.as_slice()));
            }
        }
        let mut resp = self.handle_inner(req);
        for c in self.set_cookies.drain(..) {
            resp.headers.push(("set-cookie".into(), c));
        }
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
            let has = |body: &[u8], needle: &[u8]| body.windows(needle.len()).rposition(|w| w == needle);
            let mut tags = String::new();
            // Components translate in the browser too: the page carries its
            // locale's text, read when the (deferred) bundle runs.
            if let (Some(i18n), Some(l)) = (self.site.i18n.clone(), self.locale()) {
                tags.push_str(&format!("<script>window.__quillo_i18n={}</script>", i18n.browser_json(&l)));
            }
            // Only the code of the components on the page (and what they
            // use), at an address that changes when it does.
            if has(&resp.body, b"/_quillo/quillo.js").is_none() {
                let names = std::mem::take(&mut self.island_names);
                let src = match self.bundle_for_page(&names) {
                    Some(hash) => format!("{}/_quillo/b/{}.js", self.base, hash),
                    None => format!("{}/_quillo/quillo.js", self.base),
                };
                tags.push_str(&format!("<script>{}</script>", island_loader(&src)));
            }
            let at = has(&resp.body, b"</body>").unwrap_or(resp.body.len());
            resp.body.splice(at..at, tags.into_bytes());
        }
        resp
    }

    fn handle_inner(&mut self, req: &HttpRequest) -> HttpResponse {
        let (path, qs) = req.url.split_once('?').unwrap_or((req.url.as_str(), ""));
        if let Some(rest) = path.strip_prefix("/_quillo/") {
            return self.handle_internal(rest, req, path, qs);
        }
        // A page on another site can have the browser send a form (or text)
        // here, with this site's cookies: refused for anything but reading,
        // unless the program allows it for the path. Another site's JSON
        // needs the browser to ask first, which nothing here answers, and
        // other programs (webhooks, scripts) send no Origin.
        if !matches!(req.method.as_str(), "GET" | "HEAD" | "OPTIONS")
            && sends_form(req)
            && cross_site(req)
            && !self.site.takes_cross_site(path)
        {
            return plain(403, "this site does not take form posts from other sites\n");
        }
        let segs: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(|s| percent_decode(s, false)).collect();
        let method = if req.method == "HEAD" { "GET" } else { req.method.as_str() };
        let query_v = Value::map(parse_urlencoded(qs));
        // Built when a route's path matches, at most once.
        let mut base: Option<Map> = None;

        let mut path_matched = false;
        let mut method_matched = false;
        let routes = self.routes.clone();
        for r in routes.iter() {
            let Some(params) = match_path(&r.path, &segs) else { continue };
            path_matched = true;
            if r.method != "ANY" && r.method != method {
                continue;
            }
            method_matched = true;

            let params_v = Value::map(params.iter().map(|(k, v)| (k.into(), v.clone())).collect());
            let scope = Scope::child(&self.globals);
            // The request map only for a route that can see it; for others
            // it is made if something asks (`url()`, a server fn).
            let reqv = if r.uses_request {
                let mut reqmap = match base.take() {
                    Some(m) => m,
                    None => build_request(req, path, &query_v),
                };
                reqmap.insert("params".into(), params_v);
                let reqv = Value::map(reqmap);
                define(&scope, "request", reqv.clone());
                reqv
            } else {
                self.pending_request =
                    Some(PendingRequest { req: req as *const HttpRequest, path: path.to_string(), query: query_v.clone(), params: params_v });
                Value::Nil
            };
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
                self.pending_request = None;
                continue;
            }
            for (k, v) in binds {
                define(&scope, &k, v);
            }

            self.current_request = r.uses_request.then_some(reqv);
            let mut resp = finish(self.eval(&r.body, &scope));
            // The page's components will need the locale (see `handle`).
            if self.rendered_island && self.site.i18n.is_some() {
                self.locale();
            }
            self.current_request = None;
            self.pending_request = None;
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
    /// A scheduled job, asked for by the scheduler (see `jobs`).
    fn run_job(&mut self, name: &str, req: &HttpRequest, qs: &str) -> HttpResponse {
        if req.method != "POST" || !is_job_request(req) {
            return plain(404, "not found\n");
        }
        let Some(job) = self.site.jobs.iter().find(|j| j.name == name).cloned() else {
            return plain(404, "no such job\n");
        };
        let at = qs.split('&').find_map(|kv| kv.strip_prefix("at=")).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        if !crate::db::claim_job(self, name, at) {
            return plain(200, "run by another server\n");
        }
        match finish(self.call(&job.f, Vec::new(), 0)) {
            r if r.status >= 500 => r,
            _ => plain(200, "ok\n"),
        }
    }

    fn handle_internal(&mut self, rest: &str, req: &HttpRequest, path: &str, qs: &str) -> HttpResponse {
        // A page's own bundle: its address names its content, so it is kept.
        // One this program did not make (a page from before a reload) gets
        // the whole bundle, which has every component.
        if let Some(hash) = rest.strip_prefix("b/").and_then(|r| r.strip_suffix(".js")) {
            let (js, cache) = match self.bundle_by_hash(hash) {
                Some(js) => (js, "public, max-age=31536000, immutable"),
                None => (self.client_js.clone(), "no-cache"),
            };
            return HttpResponse {
                status: 200,
                headers: vec![("content-type".into(), "text/javascript; charset=utf-8".into()), ("cache-control".into(), cache.into())],
                body: js.as_bytes().to_vec(),
            };
        }
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
        if let Some(name) = rest.strip_prefix("job/") {
            return self.run_job(name, req, qs);
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
        // A call with files is multipart, which a page on another site could
        // send as a form: it must carry a header only scripts of the page's
        // own site can add (another site's would make the browser ask first).
        let boundary = crate::upload::boundary(ctype.unwrap_or(""));
        let marked = req.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("x-quillo-rpc") && v == "1");
        if essence(ctype.unwrap_or("")) != "application/json" && !(boundary.is_some() && marked) {
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
        // With files: the arguments are the `args` part, and each file
        // (`{"$quillo_file": i}` there) is the upload in part `file<i>`.
        let (json_args, files) = match &boundary {
            Some(b) => {
                let (form, files) = crate::upload::parse(b, &req.body);
                (form.get("args").map(|v| v.display().into_bytes()).unwrap_or_default(), files)
            }
            None => (req.body.clone(), Map::new()),
        };
        let args = match serde_json::from_slice::<serde_json::Value>(&json_args) {
            Ok(serde_json::Value::Array(a)) => a
                .iter()
                .map(|j| match j.get("$quillo_file").and_then(|i| i.as_u64()) {
                    Some(i) if j.as_object().map_or(false, |o| o.len() == 1) => files.get(format!("file{}", i).as_str()).cloned().unwrap_or(Value::Nil),
                    _ => Value::from_json(j),
                })
                .collect::<Vec<_>>(),
            _ => return rpc_error(400, "the body must be a JSON array of arguments"),
        };
        if let Err(msg) = check_args(&c.def, &args) {
            return rpc_error(400, &msg);
        }
        let query = Value::map(parse_urlencoded(qs));
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

/// What the server does with a request.
pub(crate) enum Answer {
    Respond(HttpResponse),
    /// The connection goes to a thread of its own (pages waiting for live
    /// data or reloads, large files), which answers on it directly.
    Detach(crate::httpd::Detached),
}

/// Writes, on a connection a thread now owns, the answers to the requests
/// before this one; `None` if the client does not take them.
fn write_first(mut stream: std::net::TcpStream, before: &[u8]) -> Option<std::net::TcpStream> {
    use std::io::Write;
    if before.is_empty() {
        return Some(stream);
    }
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    stream.write_all(before).ok()?;
    let _ = stream.set_write_timeout(None);
    Some(stream)
}

/// Sends a large file from disk on a thread of its own (see `statics`),
/// within the places for such sends; 503 when they are taken.
fn send_file(resp: HttpResponse, what: (std::path::PathBuf, u64, u64), head: bool, ip: &str) -> Answer {
    let Some(place) = crate::statics::SendPlace::take(ip) else {
        let mut busy = plain(503, "too many downloads at once; try again later\n");
        busy.headers.push(("retry-after".into(), "5".into()));
        return Answer::Respond(busy);
    };
    Answer::Detach(Box::new(move |stream, before| {
        let _ = std::thread::Builder::new().name("quillo-file".into()).stack_size(WAITER_STACK).spawn(move || {
            let _place = place;
            if let Some(stream) = write_first(stream, &before) {
                crate::statics::send(stream, &resp, what, head);
            }
        });
    }))
}

/// Hands the connection to `f` on a thread of its own, holding one of the
/// waiting places; answers 503 when they are all taken.
fn waiter(name: &'static str, f: impl FnOnce(std::net::TcpStream) + Send + 'static) -> Answer {
    let Some(place) = Waiter::take() else {
        let mut resp = plain(503, "too many pages are waiting for changes; try again later\n");
        resp.headers.push(("retry-after".into(), "5".into()));
        return Answer::Respond(resp);
    };
    Answer::Detach(Box::new(move |stream, before| {
        // If the thread cannot start, the place and the connection go with
        // the closure.
        let _ = std::thread::Builder::new().name(name.into()).stack_size(WAITER_STACK).spawn(move || {
            let _place = place;
            if let Some(stream) = write_first(stream, &before) {
                f(stream);
            }
        });
    }))
}

/// Writes a whole answer on a connection a waiting thread owns, and closes it.
fn send_on(mut stream: std::net::TcpStream, resp: HttpResponse) {
    use std::io::Write;
    let mut out = Vec::new();
    crate::httpd::write_response(&mut out, &resp, false, true);
    let _ = stream.write_all(&out);
}

fn live_json(shared: &Shared, seq: u64, changed: &[String]) -> String {
    serde_json::json!({ "seq": seq, "server": shared.id, "changed": changed }).to_string()
}

/// Answers `/_quillo/live` on a thread of its own, so open pages do not hold
/// on to the server's threads. Over a WebSocket, each change to the topics
/// is sent as `{seq, server, changed}` for as long as the page stays; as a
/// plain request, the answer is the first change, or none after a while.
fn serve_live(shared: Arc<Shared>, req: HttpRequest) -> Answer {
    let (mut since, server, topics) = live_query(req.url.split_once('?').map_or("", |(_, q)| q));
    waiter("quillo-live", move |stream| {
        if crate::ws::is_upgrade(&req) {
            let Some(mut socket) = crate::ws::Socket::accept(&req, stream) else { return };
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
        let mut resp = json_response(200, serde_json::from_str(&live_json(&shared, seq, &changed)).unwrap_or_default());
        resp.headers.push(("cache-control".into(), "no-store".into()));
        send_on(stream, resp);
    })
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
            other => {
                let mut out = Vec::new();
                other.json_into(&mut out);
                (Some("application/json"), out)
            }
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
    /// `--reload`: load the program again when its files change, without
    /// `--watch`'s development aids (errors on pages, page refreshes).
    pub reload: bool,
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
            reload: false,
        }
    }

    /// What the program's interpreters share.
    pub fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
    }

    pub(crate) fn shared_ref(&self) -> &Shared {
        &self.shared
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
/// for a change do not hold on to the server's threads.
fn serve_events(app: Arc<App>, req: HttpRequest) -> Answer {
    let seen = req.url.split_once("?v=").and_then(|(_, v)| v.parse::<u64>().ok());
    waiter("quillo-events", move |stream| {
        if crate::ws::is_upgrade(&req) {
            let Some(mut socket) = crate::ws::Socket::accept(&req, stream) else { return };
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
        let mut resp = plain(200, &v.to_string());
        resp.headers.push(("cache-control".into(), "no-store".into()));
        send_on(stream, resp);
    })
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

/// Runs a request in the program (the caller holds the lock). A panic (a
/// bug in Quillo itself) answers 500 instead of stopping the server.
fn run_program(it: &mut Interp, req: &HttpRequest) -> Option<HttpResponse> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| it.handle(req))).ok()
}

/// Gzips text responses of 1 KiB or more for clients that accept it.
fn compress(req: &HttpRequest, resp: &mut HttpResponse) {
    use std::io::Write;
    // A part of a file stays as it is: its byte range is of the file.
    if resp.status != 200 || resp.body.len() < 1024 || resp.header("content-encoding").is_some() {
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
    // A file's gzipped copy is kept (by its ETag) rather than made again.
    let etag = resp.header("etag").map(str::to_string);
    let kept = etag.as_deref().and_then(|e| crate::statics::gzipped(e, &resp.body));
    let gz = match kept {
        Some(gz) => gz.to_vec(),
        None => {
            let mut enc = flate2::write::GzEncoder::new(Vec::with_capacity(resp.body.len() / 3), flate2::Compression::default());
            if enc.write_all(&resp.body).is_err() {
                return;
            }
            let Ok(gz) = enc.finish() else { return };
            if let Some(e) = &etag {
                crate::statics::keep_gzipped(e, &resp.body, Arc::from(gz.as_slice()));
            }
            gz
        }
    };
    resp.body = gz;
    resp.headers.push(("content-encoding".into(), "gzip".into()));
    resp.headers.push(("vary".into(), "accept-encoding".into()));
}

/// A server thread's own handle on the program, replaced when `--watch`
/// loads a new version.
pub(crate) struct Worker {
    generation: u64,
    local: Option<Interp>,
    program: Arc<Loaded>,
}

impl Worker {
    pub(crate) fn new(app: &App) -> Worker {
        Worker { generation: 0, local: None, program: app.current().1 }
    }

    /// Answers a request. The caller holds the interpreter lock.
    pub(crate) fn answer(&mut self, app: &Arc<App>, req: HttpRequest) -> Answer {
        if let Some(token) = &app.launch_token {
            if let Some(resp) = launch_gate(&req, token).filter(|_| !is_job_request(&req)) {
                return Answer::Respond(resp);
            }
        }
        let path = req.url.split('?').next().unwrap_or("");
        if app.watch && path == "/_quillo/events" {
            return serve_events(app.clone(), req);
        }
        if path.ends_with("/_quillo/live") {
            return serve_live(app.shared.clone(), req);
        }
        if let (Some(key), "/_quillo/announce") = (&app.cluster_key, req.url.as_str()) {
            // Without a secret, the key is no secret either: only limer's
            // own host may announce.
            let from: Option<std::net::IpAddr> = req.remote.parse().ok();
            if let Some(allowed) = &app.cluster_sources {
                if !from.map_or(false, |ip| ip.is_loopback() || allowed.contains(&ip)) {
                    return Answer::Respond(HttpResponse { status: 403, headers: Vec::new(), body: Vec::new() });
                }
            }
            let body = &req.body[..req.body.len().min(1 << 16)];
            let status = crate::cluster::announced(&app.shared, key, body);
            return Answer::Respond(HttpResponse { status, headers: Vec::new(), body: Vec::new() });
        }
        if path == "/_quillo/metrics" && crate::metrics::ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
            let mut r = plain(200, &crate::metrics::render());
            r.headers = vec![("content-type".into(), "text/plain; version=0.0.4".into())];
            return Answer::Respond(r);
        }
        // For a load balancer: whether to send requests here.
        if path == "/_quillo/health" {
            let ready = matches!(&*app.current().1, Loaded::Ready(_)) && !crate::httpd::stopping();
            return Answer::Respond(if ready { plain(200, "ok\n") } else { plain(503, "unavailable\n") });
        }
        let start = Instant::now();
        let _busy = crate::metrics::ENABLED.load(std::sync::atomic::Ordering::Relaxed).then(crate::metrics::InProgress::new);
        let (g, current) = app.current();
        if g != self.generation {
            self.generation = g;
            self.local = match &*current {
                Loaded::Ready(it) => Some(it.fork()),
                Loaded::Failed(_) => None,
            };
            self.program = current;
        }
        let mut resp = if app.watch && req.url == "/_quillo/dev" {
            plain(200, &self.generation.to_string())
        } else {
            match (&mut self.local, &*self.program) {
                (Some(it), _) => match run_program(it, &req) {
                    Some(r) => r,
                    None => {
                        eprintln!("quillo: internal error (a bug in Quillo) while handling {} {}", req.method, req.url);
                        // Its state may be half-updated; start afresh.
                        self.local = None;
                        self.generation = 0;
                        plain(500, "internal error\n")
                    }
                },
                (None, Loaded::Failed(e)) => error_page(e),
                (None, Loaded::Ready(_)) => unreachable!(),
            }
        };
        if app.watch {
            inject_dev_script(&mut resp, self.generation);
        }
        // Chrome asks every site for this while DevTools is open; it is not
        // the program's business, so it is not logged.
        let took = start.elapsed().as_secs_f64();
        crate::metrics::answered(resp.status, took);
        if req.url != "/.well-known/appspecific/com.chrome.devtools.json" {
            match crate::metrics::LOG.load(std::sync::atomic::Ordering::Relaxed) {
                crate::metrics::TEXT => {
                    crate::httpd::log_fmt(format_args!("{} {} -> {} ({:.1}ms)\n", req.method, req.url, resp.status, took * 1000.0))
                }
                crate::metrics::JSON => {
                    let tenant = match self.local.as_ref().and_then(|it| it.tenant.clone().flatten()) {
                        Some(t) => format!(",\"tenant\":{}", serde_json::Value::from(&*t)),
                        None => String::new(),
                    };
                    crate::httpd::log_fmt(format_args!(
                        "{{\"time\":\"{}\",\"method\":{},\"path\":{},\"status\":{},\"ms\":{:.2},\"ip\":{}{}}}\n",
                        crate::httpd::iso_now(),
                        serde_json::Value::from(req.method.as_str()),
                        serde_json::Value::from(req.url.as_str()),
                        resp.status,
                        took * 1000.0,
                        serde_json::Value::from(req.remote.as_str()),
                        tenant
                    ))
                }
                _ => {}
            }
        }
        if resp.header("x-content-type-options").is_none() {
            resp.headers.push(("x-content-type-options".into(), "nosniff".into()));
        }
        // A large file, sent from disk on a thread of its own.
        if let Some(i) = resp.headers.iter().position(|(k, _)| k.eq_ignore_ascii_case(crate::statics::SEND_HEADER)) {
            let (_, mark) = resp.headers.remove(i);
            let Some(what) = crate::statics::marked(&mark) else {
                return Answer::Respond(plain(500, "internal error\n"));
            };
            return send_file(resp, what, req.method == "HEAD", &req.remote);
        }
        compress(&req, &mut resp);
        Answer::Respond(resp)
    }
}

pub(crate) fn plain_response(status: u16, msg: &str) -> HttpResponse {
    plain(status, msg)
}

/// Serves the app. `workers` is how many requests may wait (for the
/// database, the network) at once; each waiting one has a thread. The
/// calling thread must hold the interpreter lock; it gives it up here.
pub fn serve(app: App, addr: &str, workers: usize) -> Result<(), String> {
    serve_with(app, addr, workers, |_| {})
}

/// `serve`, calling `listening` with the address once it is bound (with
/// port 0, the port the system chose).
pub fn serve_with(app: App, addr: &str, workers: usize, listening: impl FnOnce(std::net::SocketAddr)) -> Result<(), String> {
    let listener = std::net::TcpListener::bind(addr).map_err(|e| format!("cannot listen on {}: {}", addr, e))?;
    let bound = listener.local_addr().ok();
    let shown = bound.map_or_else(|| addr.to_string(), |a| a.to_string());
    let app = Arc::new(app);
    if let Loaded::Ready(it) = &*app.current().1 {
        let n = it.routes.len();
        eprintln!("quillo: {} route{} listening on http://{}", n, if n == 1 { "" } else { "s" }, shown);
    }
    if app.watch {
        eprintln!("quillo: watching for changes");
    }
    SHOW_ERRORS.store(app.watch, std::sync::atomic::Ordering::Relaxed);
    crate::session::PLAIN_HTTP.store(app.watch || app.launch_token.is_some(), std::sync::atomic::Ordering::Relaxed);
    app.shared.gil.release();
    if app.watch || app.reload {
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
        start_jobs(&app, a);
        listening(a);
    }
    crate::httpd::serve(app, listener, workers + 1)
}

/// Serves several instances of one program (`--instances`), each with an
/// event loop of its own on a thread of its own, all taking connections
/// from the same socket. The calling thread must hold the instances'
/// interpreter locks; it gives them up here.
pub fn serve_many(apps: Vec<App>, addr: &str, workers: usize) -> Result<(), String> {
    let listener = std::net::TcpListener::bind(addr).map_err(|e| format!("cannot listen on {}: {}", addr, e))?;
    let shown = listener.local_addr().map_or_else(|_| addr.to_string(), |a| a.to_string());
    let apps: Vec<Arc<App>> = apps.into_iter().map(Arc::new).collect();
    if let Some(Loaded::Ready(it)) = apps.first().map(|a| a.current().1).as_deref() {
        let n = it.routes.len();
        eprintln!(
            "quillo: {} route{} listening on http://{} ({} instances)",
            n,
            if n == 1 { "" } else { "s" },
            shown,
            apps.len()
        );
    }
    for app in &apps {
        app.shared.gil.release();
    }
    if apps.is_empty() {
        return Err("no instances".into());
    }
    if let Ok(a) = listener.local_addr() {
        start_jobs(&apps[0], a);
    }
    crate::httpd::serve_instances(apps, listener, workers + 1)
}

/// The cookie that admits a desktop app's window.
const LAUNCH_COOKIE: &str = "quillo_launch";

/// Whether a request is the scheduler's (it carries this process's secret).
fn is_job_request(req: &HttpRequest) -> bool {
    req.headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("x-quillo-job") && crate::crypto::same(v.as_bytes(), crate::jobs::token().as_bytes()))
}

/// Starts the job scheduler for the program `app` runs (see `jobs`).
fn start_jobs(app: &Arc<App>, addr: std::net::SocketAddr) {
    let app = app.clone();
    crate::jobs::start(addr, move || match &*app.current().1 {
        Loaded::Ready(it) => it.site.jobs.iter().map(|j| (j.name.clone(), j.when.clone())).collect(),
        Loaded::Failed(_) => Vec::new(),
    });
}

/// For a desktop app's server, which listens on this machine where other
/// programs and web pages could reach it: `None` lets the request through
/// when it carries the launch cookie. The window's first request carries the
/// token in the URL instead (`?_quillo_launch=...`); it is answered with the
/// cookie and a redirect to the same URL without the token. Anything else is
/// refused. The cookie is `SameSite=Strict` and bound to 127.0.0.1, so pages
/// on other sites (even ones whose names resolve to 127.0.0.1) never send it.
pub fn launch_gate(req: &HttpRequest, token: &str) -> Option<HttpResponse> {
    let refuse = || Some(plain(403, "forbidden\n"));
    let cookies = req.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("cookie")).flat_map(|(_, v)| v.split(';'));
    let mut has_cookie = false;
    for c in cookies {
        if let Some((k, v)) = c.trim().split_once('=') {
            if k == LAUNCH_COOKIE && same_secret(v, token) {
                has_cookie = true;
            }
        }
    }
    let (path, query) = req.url.split_once('?').unwrap_or((req.url.as_str(), ""));
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
            Some(HttpResponse {
                status: 303,
                headers: vec![
                    ("location".into(), location),
                    ("set-cookie".into(), cookie),
                    ("cache-control".into(), "no-store".into()),
                ],
                body: Vec::new(),
            })
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

/// The script a page with components carries instead of its bundle: it
/// loads the bundle when it is needed — someone reaches for a component
/// (hovers, touches or focuses it), or the page is idle and has one that
/// runs by itself (`data-e`, `data-l`) or is the page (`<body>` holds it).
/// A click or a form sent before the bundle runs waits for it, and is done
/// then (see `boot` in runtime.js).
fn island_loader(src: &str) -> String {
    let src = serde_json::to_string(src).unwrap_or_default().replace("</", "<\\/");
    format!(
        concat!(
            "(()=>{{const d=document,q=window.__quillo_q=[];let go=0;",
            "const load=()=>{{if(go)return;go=1;const t=d.createElement(\"script\");t.src={};t.onerror=()=>{{window.__quillo_booted=1}};d.head.appendChild(t)}};",
            "const isl=(e)=>e.target&&e.target.closest&&e.target.closest(\"quillo-island\");",
            "for(const v of[\"pointerover\",\"pointerdown\",\"touchstart\",\"focusin\",\"keydown\"])d.addEventListener(v,(e)=>{{if(isl(e)){{window.__quillo_at=e.target;load()}}}},{{capture:true,passive:true}});",
            "for(const v of[\"click\",\"submit\"])d.addEventListener(v,(e)=>{{if(!window.__quillo_booted&&isl(e)){{e.preventDefault();e.stopPropagation();q.push(e);load()}}}},true);",
            "const idle=()=>{{if(d.querySelector(\"quillo-island[data-e],quillo-island[data-l],body>quillo-island\"))(window.requestIdleCallback||setTimeout)(load)}};",
            "d.readyState===\"loading\"?d.addEventListener(\"DOMContentLoaded\",idle):idle()}})()"
        ),
        src
    )
}
