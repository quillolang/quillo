//! Routing requests to `route` declarations, and the HTTP server.

use crate::ast::Seg;
use crate::interp::{coerce, Ctrl, Interp, Mode};
use crate::shared::Shared;
use crate::value::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub remote: String,
}

#[derive(Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// `header`, but not one the program named `null` (`DROPPED`), which holds only its name's
    /// place: what is sent (FEEDBACK55.md, Q-710; branch feedback55).
    pub fn sent(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, v)| k.eq_ignore_ascii_case(name) && v != DROPPED).map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

pub fn plain(status: u16, msg: &str) -> HttpResponse {
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

/// `req` as `request`, its form as `all_values` makes it, for `{ form: { tag: [str] } }`.
fn form_lists(pat: Option<&crate::ast::Pattern>, reqv: &Value, req: &HttpRequest) -> Option<Value> {
    let Some(crate::ast::Pattern::Record(fields)) = pat else { return None };
    let Value::Map(m) = reqv else { return None };
    let mut out = None;
    if let Some(f) = fields.iter().find(|f| f.path.len() == 1 && f.path[0] == "form") {
        if let Some(form) = m.get("form").and_then(|v| all_values(&f.pat, v, &String::from_utf8_lossy(&req.body))) {
            out.get_or_insert_with(|| (**m).clone()).insert("form".into(), form);
        }
    }
    // `{ files: { docs: [map] } }`: every file of that field as a list, one file or none
    // included, as a field holds one file's record when one is sent (FEEDBACK30.md, Q-365;
    // branch feedback27-35-fixes).
    if let Some(f) = fields.iter().find(|f| f.path.len() == 1 && f.path[0] == "files") {
        if let Some(files) = file_lists(&f.pat, m.get("files").unwrap_or(&Value::Nil)) {
            out.get_or_insert_with(|| (**m).clone()).insert("files".into(), files);
        }
    }
    out.map(Value::map)
}

fn file_lists(pat: &crate::ast::Pattern, v: &Value) -> Option<Value> {
    let crate::ast::Pattern::Record(fields) = pat else { return None };
    let names: Vec<&String> = fields.iter().filter(|f| f.path.len() == 1 && matches!(f.pat, crate::ast::Pattern::List(_, None))).map(|f| &f.path[0]).collect();
    if names.is_empty() {
        return None;
    }
    let mut out = if let Value::Map(m) = v { (**m).clone() } else { Map::new() };
    for n in names {
        let all = match out.get(n.as_str()) {
            None | Some(Value::Nil) => Value::list(Vec::new()),
            Some(one @ Value::Map(_)) => Value::list(vec![one.clone()]),
            Some(other) => other.clone(),
        };
        out.insert(n.as_str().into(), all);
    }
    Some(Value::map(out))
}

/// `m` with the names `pat` asks a list of holding all their values in `raw`, and
/// none when they are absent, as a group of checkboxes with nothing ticked sends
/// nothing (FEEDBACK26.md, Q-200; branch feedback26-fixes).
fn all_values(pat: &crate::ast::Pattern, m: &Value, raw: &str) -> Option<Value> {
    let crate::ast::Pattern::Record(fields) = pat else { return None };
    let Value::Map(map) = m else { return None };
    let names: Vec<&String> = fields.iter().filter(|f| f.path.len() == 1 && matches!(f.pat, crate::ast::Pattern::List(_, None)) && f.pat.is_type_test()).map(|f| &f.path[0]).collect();
    if names.is_empty() {
        return None;
    }
    let mut out = (**map).clone();
    let pairs: Vec<(String, String)> =
        raw.split('&').filter(|p| !p.is_empty()).map(|p| p.split_once('=').unwrap_or((p, ""))).map(|(k, v)| (percent_decode(k, true), percent_decode(v, true))).collect();
    for n in names {
        // `tag[]=a&tag[]=b`, as PHP- and Rails-shaped forms name a list, when there is no `tag`:
        // it gave an empty list, and the route went on without them (FEEDBACK47.md, Q-543).
        let bracketed = format!("{}[]", n);
        let key = if pairs.iter().any(|(k, _)| k == n) { n.as_str() } else { bracketed.as_str() };
        out.insert(n.as_str().into(), Value::list(pairs.iter().filter(|(k, _)| k == key).map(|(_, v)| Value::str(v)).collect()));
    }
    Some(Value::map(out))
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
            Seg::Param(name, ty, prefix) => {
                // `/@:name`: the piece starts with the prefix, and the rest is the value, which
                // `/@` alone has none of (FEEDBACK53.md, Q-683; branch main).
                let s = segs.get(i)?.strip_prefix(prefix.as_str())?;
                if s.is_empty() && !prefix.is_empty() {
                    return None;
                }
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

/// A path as routes read it: split at `/`, empty pieces dropped, each decoded. `//admin/users`
/// and `/%61dmin/users` are `/admin/users`, which a route under `/admin` answers, so what is
/// said about `/admin` must hold for them too (FEEDBACK47.md, Q-537; branch feedback40-43).
pub fn route_path(path: &str) -> String {
    // A `%2F` stays inside its piece, as routes keep it: `/payments%2Freturn` is one piece, not
    // `/payments/return`, which a rule of that path would then cover (found in review).
    let pieces: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(|s| percent_decode(s, false).replace('/', "%2F")).collect();
    format!("/{}", pieces.join("/"))
}

/// A `Host` as it names a host: lower case, without a trailing dot or the default port.
/// `API.example.com`, `api.example.com:443` and `api.example.com.` are `api.example.com`, which
/// a pattern on `headers.host` missed (FEEDBACK47.md, Q-545).
pub fn host_name(host: &str) -> String {
    let h = host.trim().to_ascii_lowercase();
    let h = h.strip_suffix(":80").or_else(|| h.strip_suffix(":443")).unwrap_or(&h).to_string();
    match h.rsplit_once(':') {
        // `example.com.:8080`
        Some((name, port)) if !name.ends_with(']') && port.bytes().all(|b| b.is_ascii_digit()) => format!("{}:{}", name.trim_end_matches('.'), port),
        _ => h.trim_end_matches('.').to_string(),
    }
}

/// One `name=value` of a `Cookie` header, trimmed around `=`, its value out of one pair of quotes
/// (FEEDBACK55.md, Q-718; branch feedback55).
pub(crate) fn cookie_pair(part: &str) -> Option<(&str, &str)> {
    let (k, v) = part.split_once('=')?;
    let v = v.trim();
    let v = v.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(v);
    Some((k.trim(), v))
}

fn build_request(req: &HttpRequest, path: &str, query: &Value) -> Map {
    let header = |name: &str| {
        req.headers.iter().rev().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str()).unwrap_or("")
    };
    let ctype = header("content-type").to_string();
    // As RFC 6265 reads them: the first of a name, which a browser sends with the longest path, so
    // `/admin`'s is not lost to `/`'s; the name and value without spaces around `=`; and one pair
    // of quotes around the value taken off (FEEDBACK55.md, Q-718; branch feedback55).
    let mut cookies = Map::new();
    for part in header("cookie").split(';') {
        if let Some((k, v)) = cookie_pair(part) {
            if !cookies.contains_key(k) {
                cookies.insert(k.into(), Value::str(percent_decode(v, false)));
            }
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
        // RFC 8259 lets a reader skip a byte-order mark (FEEDBACK31.md, Q-386; branch
        // feedback27-35-fixes).
        // Read straight into values, with a budget of them (FEEDBACK51.md, Q-660).
        Value::parse_json(body.strip_prefix('\u{FEFF}').unwrap_or(&body)).unwrap_or(Value::Nil)
    } else {
        Value::Nil
    };
    let (form, files) = match &multipart {
        Some(b) => crate::upload::parse(b, &req.body),
        // The type in any case, with or without parameters (FEEDBACK55.md, Q-717; branch feedback55).
        None if essence(&ctype) == "application/x-www-form-urlencoded" => (parse_urlencoded(&body), Map::new()),
        None => (Map::new(), Map::new()),
    };

    let mut headers = Map::with_capacity(req.headers.len());
    for (k, v) in &req.headers {
        let k = k.to_ascii_lowercase();
        let v = if k == "host" { host_name(v) } else { v.clone() };
        headers.insert(k.into(), Value::str(v));
    }
    let mut m = Map::with_capacity(12);
    m.insert("method".into(), Value::str(&req.method));
    m.insert("path".into(), Value::str(path));
    m.insert("route_path".into(), Value::str(route_path(path)));
    m.insert("host".into(), Value::str(host_name(header("host"))));
    m.insert("query".into(), query.clone());
    // As it came: what a repeated name's others were, and what a signed URL signed.
    m.insert("query_string".into(), Value::str(req.url.split_once('?').map_or("", |(_, q)| q)));
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
        // A service with a prefix that runs elsewhere: passed on to it.
        if let Some((prefix, url)) = self.remote_mounts.iter().find(|(p, _)| path == p.as_str() || path.starts_with(&format!("{}/", p))).cloned() {
            // Its readiness, for a gateway that routes by path (FEEDBACK10.md,
            // Q-076; branch feedback10-fixes); Quillo's other addresses are not
            // passed on.
            if path == format!("{}/_quillo/ready", prefix) {
                let name = self.declared_services.iter().find(|n| crate::service::urls_of(&self.shared, n).contains(&url)).cloned();
                let (ok, detail) = crate::ready::service(self, name.as_deref().unwrap_or(""), &url);
                let mut r = json_response(if ok { 200 } else { 503 }, detail);
                r.headers.push(("cache-control".into(), "no-store".into()));
                return r;
            }
            return crate::service::pass_on(self, &url, &prefix, req);
        }
        self.pending_stream = None;
        // One a request made and nothing took (a job that gave back a stream).
        drop(crate::stream::take_made());
        for (prefix, sub) in self.mounts.iter_mut() {
            if path == prefix.as_str() || path.starts_with(&format!("{}/", prefix)) {
                let rest = &req.url[prefix.len()..];
                let mut inner = req.clone();
                inner.url = if rest.is_empty() || rest.starts_with('?') { format!("/{}", rest) } else { rest.to_string() };
                sub.streams_direct = self.streams_direct;
                let mut resp = sub.handle(&inner);
                self.pending_stream = sub.pending_stream.take();
                // A mounted program's deferred parts are its own to render: whole
                // here (branch defer).
                if let Some(page) = self.pending_stream.as_mut().and_then(|p| p.page.take()) {
                    self.pending_stream = None;
                    resp.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(crate::stream::MARK));
                    resp.body = crate::defer::whole(sub, page);
                }
                return resp;
            }
        }
        self.rendered_island = false;
        self.island_names.clear();
        self.deferred.clear();
        self.tenant = None;
        self.tenant_named = false;
        self.no_such_tenant = false;
        self.answering = true;
        self.upload_body = None;
        self.route_limit = None;
        self.set_cookies.clear();
        self.session = None;
        self.visitor_read = false;
        self.locale = None;
        if !req.body.is_empty() {
            let ctype = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map_or("", |(_, v)| v.as_str());
            if crate::upload::boundary(ctype).is_some() {
                self.upload_body = Some(Arc::from(req.body.as_slice()));
            }
        }
        // A page asked for ahead of a click: made without changing anything.
        let prefetch = is_prefetch(req).then(|| Arc::new(std::sync::atomic::AtomicBool::new(false)));
        self.prefetching = prefetch.clone();
        let changed = (req.method != "GET" && req.method != "HEAD").then(|| Arc::new(std::sync::atomic::AtomicBool::new(false)));
        self.changed = changed.clone();
        let mut resp = self.handle_inner(req);
        // A transaction begun by hand and left open (FEEDBACK44.md, Q-580).
        crate::db::left_open(self, &format!("{} {}", req.method, path));
        // A prefetch's deferred parts: worked out now, as part of the prefetch, which
        // may change nothing (branch defer).
        if prefetch.is_some() && !self.deferred.is_empty() {
            let jobs = std::mem::take(&mut self.deferred);
            if resp.header("content-type").is_some_and(|c| c.starts_with("text/html")) && resp.status == 200 {
                let page = crate::defer::split(std::mem::take(&mut resp.body), jobs);
                resp.body = crate::defer::whole(self, page);
            }
        }
        self.prefetching = None;
        self.changed = None;
        // A host that names no customer is a page that is not there, where the route met
        // that and did not answer for itself. A route that needs no database still
        // answers, as `www.example.com` does (FEEDBACK20.md, Q-167; branch feedback20-fixes).
        if self.no_such_tenant && resp.status >= 500 {
            resp = plain(404, "not found\n");
        }
        // A streamed answer: its function, for the server to run after the
        // head; or, with no server to, run now into the body.
        if resp.header(crate::stream::MARK).is_some() {
            let made = crate::stream::take_made();
            if self.streams_direct {
                self.pending_stream = made;
            } else {
                resp.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(crate::stream::MARK));
                if let (Some(p), false) = (made, req.method == "HEAD") {
                    let mut out = crate::stream::Buffer(Vec::new());
                    if let crate::stream::Ended::Failed = crate::stream::run(self, p, &mut out, false) {
                        resp = plain(500, "internal error\n");
                    } else {
                        resp.body = out.0;
                    }
                }
            }
        }
        let sets_cookie = !self.set_cookies.is_empty();
        for c in self.set_cookies.drain(..) {
            resp.headers.push(("set-cookie".into(), c));
        }
        self.site_headers_into(path, sets_cookie, &mut resp);
        if !self.base.is_empty() {
            for (k, v) in resp.headers.iter_mut() {
                if k.eq_ignore_ascii_case("location") && *v != DROPPED {
                    *v = self.with_base(v);
                }
            }
        }
        // A prefetch that would have changed something (or sets a cookie):
        // not prefetched; the page is made when the link is followed. One
        // that went well is kept by the browser for the click.
        if let Some(refused) = prefetch {
            if refused.load(std::sync::atomic::Ordering::Relaxed) || resp.sent("set-cookie").is_some() {
                // Not a 2xx, which Chrome's speculation rules would keep and
                // answer the click with (a 204 leaves the browser where it
                // was): with a 503 it drops the prefetch and loads the page.
                return HttpResponse { status: 503, headers: vec![("cache-control".into(), "no-store".into())], body: Vec::new() };
            }
            // Only this page's own fetch needs the HTTP cache: Chrome keeps a speculation
            // rule's answer itself, and would keep it there too. `Vary`, so that it does not
            // outlive the session it was made for (FEEDBACK22.md, Q-176; branch
            // feedback22-fixes).
            let own = req.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("x-quillo-prefetch"));
            if own && resp.status == 200 && resp.header("cache-control").is_none() && !crate::idempotency::holds_form_key(&resp.body) {
                resp.headers.push(("cache-control".into(), "private, max-age=30".into()));
                resp.headers.push(("vary".into(), "Cookie".into()));
            }
        }
        // A page with a key in a form is for this visitor, this once.
        if resp.header("cache-control").is_some_and(|c| c.contains("public") || c.contains("max-age")) && crate::idempotency::holds_form_key(&resp.body) {
            resp.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("cache-control"));
            resp.headers.push(("cache-control".into(), "no-store".into()));
        }
        // What this changed may be in a page kept for a click: Chrome lets those go on
        // this. Not on every request: a server fn that reads is a POST as well, and a `GET`
        // may count a visit. A session changing is always one: logging in or out by a link
        // leaves pages of the old one (FEEDBACK22.md, Q-177; branch feedback22-fixes).
        let wrote = changed.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed));
        let session = resp.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("set-cookie") && v.contains("quillo_session="));
        if (wrote || session) && PREFETCH.load(std::sync::atomic::Ordering::Relaxed) && resp.header("clear-site-data").is_none() {
            resp.headers.push(("clear-site-data".into(), "\"prefetchCache\", \"prerenderCache\"".into()));
        }
        // Pages with components load the browser bundle.
        let html = resp.header("content-type").map_or(false, |c| c.starts_with("text/html"));
        // Every page prefetches the links pointed at: a page a route made, not a file as it is on
        // disk, whose ETag and byte ranges are of the file (FEEDBACK36.md, Q-447; branch
        // feedback36-39).
        let html = html && !as_written(&resp);
        if html && req.method != "HEAD" && PREFETCH.load(std::sync::atomic::Ordering::Relaxed) {
            let has = |body: &[u8], needle: &[u8]| body.windows(needle.len()).rposition(|w| w == needle);
            if let (Some(at), false) = (has(&resp.body, b"</body>"), script_with(&resp.body, &PREFETCH_JS.as_bytes()[..40])) {
                resp.body.splice(at..at, format!("<script>{}</script>{}", PREFETCH_JS, used_script(&self.base)).into_bytes());
            }
        }
        // A page that plays a stream loads the player.
        if html && req.method != "HEAD" {
            let has = |body: &[u8], needle: &[u8]| body.windows(needle.len()).rposition(|w| w == needle);
            if has(&resp.body, b"<quillo-media").is_some() && !script_tag_for(&resp.body, b"/_quillo/media.js") {
                let at = has(&resp.body, b"</body>").unwrap_or(resp.body.len());
                resp.body.splice(at..at, format!("<script src=\"{}/_quillo/media.js\" defer></script>", self.base).into_bytes());
            }
        }
        // The styles of what the page's bundle can show, or all of them for
        // the whole program's bundle.
        let mut bundle_styles: Vec<String> = Vec::new();
        let mut hints = String::new();
        if html && req.method != "HEAD" && std::mem::take(&mut self.rendered_island) {
            let has = |body: &[u8], needle: &[u8]| body.windows(needle.len()).rposition(|w| w == needle);
            let mut tags = String::new();
            // Components translate in the browser too: the page carries its
            // locale's text, read when the (deferred) bundle runs.
            let i18n = match (self.site.i18n.clone(), self.locale()) {
                (Some(i18n), Some(l)) => Some((i18n, l)),
                _ => None,
            };
            // Only the code of the components on the page (and what they
            // use), at an address that changes when it does.
            if !script_tag_for(&resp.body, b"/_quillo/quillo.js") {
                let names = std::mem::take(&mut self.island_names);
                // Its bundle, after the runtime file every page shares when
                // it takes the runtime from there (FEEDBACK7.md, Q-064;
                // branch feedback7-fixes).
                let srcs = match self.bundle_for_page(&names) {
                    Some(page) => {
                        // The libraries its components use first (branch js-libraries).
                        let mut s = page.libs.clone();
                        // Its translations, the keys its code can ask for, in a file of their
                        // own the browser keeps (FEEDBACK30.md, Q-372; branch feedback27-35-fixes).
                        if let Some((i18n, l)) = &i18n {
                            let hash = crate::i18n::keep_file(i18n.browser_json(l, page.t_keys.as_deref().map(|v| v.as_slice())));
                            s.insert(0, format!("{}/_quillo/t/{}.js", self.base, hash));
                        }
                        if let Some((rt, _)) = self.runtime_file().filter(|_| page.split) {
                            s.push(format!("{}/_quillo/r/{}.js", self.base, rt));
                        }
                        s.push(format!("{}/_quillo/b/{}.js", self.base, page.hash));
                        bundle_styles = page.styles;
                        hints = page.hints;
                        s
                    }
                    None => {
                        bundle_styles = self.styles.lock().unwrap_or_else(std::sync::PoisonError::into_inner).ids();
                        hints = crate::client::all_hints(self);
                        let mut s = crate::client::all_libs(self);
                        if let Some((i18n, l)) = &i18n {
                            s.insert(0, format!("{}/_quillo/t/{}.js", self.base, crate::i18n::keep_file(i18n.browser_json(l, None))));
                        }
                        s.push(format!("{}/_quillo/quillo.js", self.base));
                        s
                    }
                };
                tags.push_str(&format!("<script>{}</script>", island_loader(&srcs)));
            } else {
                bundle_styles = self.styles.lock().unwrap_or_else(std::sync::PoisonError::into_inner).ids();
                if let Some((i18n, l)) = &i18n {
                    tags.push_str(&format!("<script>{}.i18n={}</script>", crate::client::HANDOFF_JS, i18n.browser_json(l, None)));
                }
            }
            let at = has(&resp.body, b"</body>").unwrap_or(resp.body.len());
            resp.body.splice(at..at, tags.into_bytes());
        }
        // The page's sheet: the styles of the components and views on it,
        // and of those its bundle can show, at an address that changes when
        // they do, in its <head> (FEEDBACK8.md, Q-068; branch
        // feedback8-fixes).
        let unmarked = std::mem::take(&mut self.styles_used);
        if html && req.method != "HEAD" {
            let mut ids = crate::styles::ids_in(&resp.body);
            ids.extend(bundle_styles);
            ids.extend(unmarked);
            // The libraries named `with preload` or `with preconnect`, before the
            // sheet (branch library-hints).
            let mut link = hints;
            if let Some(hash) = self.sheet_for_page(&ids) {
                link.push_str(&format!("<link rel=\"stylesheet\" href=\"{}/_quillo/s/{}.css\">", self.base, hash));
            }
            if !link.is_empty() {
                let find = |needle: &[u8]| resp.body.windows(needle.len()).position(|w| w.eq_ignore_ascii_case(needle));
                let at = match (find(b"</head>"), find(b"<body")) {
                    (Some(h), _) => h,
                    (None, Some(b)) => resp.body[b..].iter().position(|&c| c == b'>').map_or(0, |e| b + e + 1),
                    (None, None) => 0,
                };
                resp.body.splice(at..at, link.into_bytes());
            }
        }
        // Waiting components (src/defer.rs; branch defer): the page now, each of them
        // after it in the same answer; whole when there is no server to stream it.
        if !self.deferred.is_empty() {
            let jobs = std::mem::take(&mut self.deferred);
            if html && req.method != "HEAD" && resp.status == 200 {
                let page = crate::defer::split(std::mem::take(&mut resp.body), jobs);
                if self.streams_direct {
                    resp.headers.push((crate::stream::MARK.into(), "1".into()));
                    resp.headers.push(("x-accel-buffering".into(), "no".into()));
                    self.pending_stream = Some(crate::stream::Pending { f: Value::Nil, events: false, page: Some(page) });
                } else {
                    resp.body = crate::defer::whole(self, page);
                }
            }
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
            && !self.site.takes_cross_site(&route_path(path))
        {
            return plain(403, "this site does not take form posts from other sites\n");
        }
        let segs: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(|s| percent_decode(s, false)).collect();
        let method = if req.method == "HEAD" { "GET" } else { req.method.as_str() };
        let query_v = Value::map(parse_urlencoded(qs));
        // Built when a route's path matches, at most once.

        let mut path_matched = false;
        let mut method_matched = false;
        // The first route whose method and path took the request, and the pattern it missed,
        // for what the 400 says (FEEDBACK47.md, Q-544).
        let mut closest: Option<(Arc<crate::ast::RouteDef>, crate::ast::Pattern, Value, &'static str, Mode)> = None;
        // The groups whose middleware gave null (see `no_match_of`).
        let mut excluded: Vec<Vec<(Option<Arc<str>>, u32)>> = Vec::new();
        // The methods the path takes, for `Allow` (FEEDBACK36.md, Q-440; branch feedback36-39).
        let mut allowed: Vec<&str> = Vec::new();
        let routes = self.routes.clone();
        if req.url == "*" && req.method == "OPTIONS" {
            let mut all: Vec<&str> = routes.iter().map(|r| r.method.as_str()).filter(|m| *m != "ANY" && *m != crate::ast::GUARD_METHOD).collect();
            return options_answer(&mut all);
        }
        for r in routes.iter() {
            // A guarded group's own, for what no route answers (see below).
            if r.method == crate::ast::GUARD_METHOD {
                continue;
            }
            let Some(params) = match_path(&r.path, &segs) else { continue };
            path_matched = true;
            if r.method != "ANY" && r.method != method {
                allowed.push(r.method.as_str());
                continue;
            }
            method_matched = true;

            let params_v = Value::map(params.iter().map(|(k, v)| (k.into(), v.clone())).collect());
            let scope = Scope::child(&self.globals);
            // The request map only for a route that can see it; for others
            // it is made if something asks (`url()`, a server fn).
            let reqv = if r.uses_request {
                let mut reqmap = build_request(req, path, &query_v);
                reqmap.insert("params".into(), params_v);
                if let Some(id) = &self.request_id {
                    reqmap.insert("id".into(), Value::str(&**id));
                }
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
            // A list in a pattern takes all of a name's values.
            let query_all = r.query.as_ref().and_then(|p| all_values(p, &query_v, qs));
            let query_v = query_all.as_ref().unwrap_or(&query_v);
            let form_all = form_lists(r.body_pat.as_ref(), &reqv, req);
            let reqv = form_all.unwrap_or(reqv);
            // The `routes` groups' patterns too, outermost first (branch route-groups).
            let checks = std::iter::once((r.query.as_ref(), query_v, Mode::Lenient, "query"))
                .chain(r.guards.iter().map(|g| (Some(g), &reqv, Mode::Request, "")))
                .chain(std::iter::once((r.body_pat.as_ref(), &reqv, Mode::Request, "")));
            let mut matched = true;
            for (pat, val, mode, place) in checks {
                let Some(pat) = pat else { continue };
                match self.match_pat(pat, val, mode, &scope, &mut binds) {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        if closest.is_none() {
                            closest = Some((r.clone(), pat.clone(), val.clone(), place, mode));
                        }
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
            // The request as sent, for a group's middleware: a pattern binding `request` binds the
            // route's own name, never this one (FEEDBACK47.md, Q-535; branch feedback40-43). After
            // the patterns' names, so not even `? { __request: str }` replaces it (found in review).
            if r.uses_request {
                define(&scope, "__request", reqv.clone());
            }

            self.current_request = r.uses_request.then_some(reqv);
            let file = std::mem::replace(&mut self.cur_file, r.file.clone());
            // `route ... with db`: its body runs as a `with` function's, narrowed, never widened
            // (branch feedback40-43).
            // Kept for the request: a stream's function and deferred parts run after the body.
            self.route_limit = r.limit.clone();
            let ran = self.under_route(|it| if crate::debug::on() { it.debug_route(&r, &scope) } else { it.eval(&r.body, &scope) });
            self.cur_file = file;
            // A middleware gave null: its group's routes do not match, and the next route is
            // tried (FEEDBACK51.md, Q-651).
            if let Some(group) = no_match_of(&r, &ran) {
                excluded.push(group);
                self.current_request = None;
                self.pending_request = None;
                continue;
            }
            let resp = finish(crate::interp::in_file(ran, &r.file));
            // The page's components will need the locale (see `handle`).
            if self.rendered_island && self.site.i18n.is_some() {
                self.locale();
            }
            self.current_request = None;
            self.pending_request = None;
            // A `HEAD` is answered with the GET's headers, its length included; the body is left
            // out where the answer is written (FEEDBACK31.md, Q-393; branch feedback27-35-fixes).
            return resp;
        }
        // Under a guarded group, its middleware first: a stranger learns nothing of what is
        // there, its paths, methods and parameters, from the server's own answers (FEEDBACK47.md,
        // Q-539; branch feedback40-43).
        if let Some(refused) = self.guard_first(&routes, req, path, &segs, &query_v, &mut excluded) {
            return refused;
        }
        // The groups whose middleware said their routes do not match: the server answers as if
        // they were not there, a 404 where no other route has the path, and no `Allow` naming
        // theirs (FEEDBACK51.md, Q-651).
        if !excluded.is_empty() {
            let out = |r: &crate::ast::RouteDef| {
                let calls = middleware_calls(r);
                excluded.iter().any(|g| calls.starts_with(g))
            };
            (path_matched, method_matched) = (false, false);
            allowed.clear();
            for r in routes.iter().filter(|r| r.method != crate::ast::GUARD_METHOD && !out(r)) {
                if match_path(&r.path, &segs).is_none() {
                    continue;
                }
                path_matched = true;
                if r.method != "ANY" && r.method != method {
                    allowed.push(r.method.as_str());
                } else {
                    method_matched = true;
                }
            }
            if closest.as_ref().is_some_and(|c| out(&c.0)) {
                closest = None;
            }
        }
        if method_matched {
            // Which route came closest, and its first miss: in the log always, and under `--watch`
            // in the answer too, as a 400 nobody can explain is a guess (Q-544).
            let why = closest.map(|(r, pat, val, place, mode)| {
                // The file as from where the program was started, as other messages give it.
                let at = match r.file.as_deref() {
                    Some(file) => {
                        let cwd = std::env::current_dir().ok();
                        let file = cwd.as_ref().and_then(|d| std::path::Path::new(file).strip_prefix(d).ok()).map_or(file.to_string(), |p| p.display().to_string());
                        format!("{}:{}", file, r.line)
                    }
                    None => format!("line {}", r.line),
                };
                format!("{} ({}): {}", r.shown(), at, self.why_no_match(&pat, &val, place, mode))
            });
            if let Some(why) = &why {
                crate::httpd::warning_line(&format!("400: {}", why));
            }
            match why.filter(|_| SHOW_ERRORS.load(std::sync::atomic::Ordering::Relaxed)) {
                Some(why) => plain(400, &format!("bad request: no route pattern matches this request\n{}\n", why)),
                None => plain(400, "bad request: no route pattern matches this request\n"),
            }
        } else if !KNOWN_METHODS.contains(&method) && !routes.iter().any(|r| r.method == method && r.method != crate::ast::GUARD_METHOD) {
            // A method nothing here knows is a 501, not a 405 (RFC 9110 §9.1).
            plain(501, "unknown method\n")
        } else if path_matched && method == "OPTIONS" {
            options_answer(&mut allowed)
        } else if path_matched {
            // A 405 names the methods the path takes (RFC 9110 §15.5.6).
            let mut resp = plain(405, "method not allowed\n");
            resp.headers.push(("allow".into(), allow_list(&mut allowed)));
            resp
        } else {
            plain(404, "not found\n")
        }
    }
}

impl Interp {
    /// The middleware of the innermost guarded group whose path the request is under, when no
    /// route answered it: its answer if it refuses, else none, and the server's answer stands.
    fn guard_first(
        &mut self,
        routes: &[Arc<crate::ast::RouteDef>],
        req: &HttpRequest,
        path: &str,
        segs: &[String],
        query_v: &Value,
        excluded: &mut Vec<Vec<(Option<Arc<str>>, u32)>>,
    ) -> Option<HttpResponse> {
        // Every guarded group the request is under, outer and inner, overlapping or not, each
        // in the order declared: the first that refuses answers (found in review: only the
        // first was asked). A group's own request pattern, a host's, says whether it is under it.
        // An inner group's guard runs its outer groups' middleware too: their own guards are
        // not run again after it (found by the suite). Told by the middleware calls, where they
        // are written: an inner guard's begin with its outer groups', and a group that only
        // overlaps (`/api/v2` and `/api`) has its own (found in review).
        let mut ran: Vec<Vec<(Option<Arc<str>>, u32)>> = Vec::new();
        for r in routes.iter().filter(|r| r.method == crate::ast::GUARD_METHOD) {
            let own = guard_calls(r);
            if ran.iter().any(|inner| inner.len() > own.len() && inner[..own.len()] == own[..]) {
                continue;
            }
            // A group a middleware already said the request is not for, or one inside it: not
            // asked again (Q-651).
            if excluded.iter().any(|g| own.starts_with(g)) {
                continue;
            }
            // Its parameters typed when they are of their type, as the middleware expects them
            // (`member_of(team)` compares an int); as text when not, so `/t/abc/x` under
            // `/t/:team(int)` goes through it too, or the 404 would say the type (found in review).
            let untyped: Vec<crate::ast::Seg> = r
                .path
                .iter()
                .map(|seg| match seg {
                    crate::ast::Seg::Param(n, Some(_), p) => crate::ast::Seg::Param(n.clone(), None, p.clone()),
                    other => other.clone(),
                })
                .collect();
            let Some(params) = match_path(&r.path, segs).or_else(|| match_path(&untyped, segs)) else { continue };
            let scope = Scope::child(&self.globals);
            let mut reqmap = build_request(req, path, query_v);
            reqmap.insert("params".into(), Value::map(params.iter().map(|(k, v)| (k.into(), v.clone())).collect()));
            if let Some(id) = &self.request_id {
                reqmap.insert("id".into(), Value::str(&**id));
            }
            let reqv = Value::map(reqmap);
            let mut binds = Vec::new();
            let mut under = true;
            for g in &r.guards {
                match self.match_pat(g, &reqv, Mode::Request, &scope, &mut binds) {
                    Ok(Some(_)) => {}
                    _ => {
                        under = false;
                        break;
                    }
                }
            }
            if !under {
                continue;
            }
            define(&scope, "request", reqv.clone());
            define(&scope, "__request", reqv.clone());
            for (k, v) in params {
                define(&scope, &k, v);
            }
            self.current_request = Some(reqv);
            let file = std::mem::replace(&mut self.cur_file, r.file.clone());
            self.route_limit = r.limit.clone();
            let answered = self.under_route(|it| it.eval(&r.body, &scope));
            self.cur_file = file;
            self.current_request = None;
            if let Some(group) = no_match_of(r, &answered) {
                excluded.push(group);
                continue;
            }
            if answered.is_err() {
                return Some(finish(crate::interp::in_file(answered, &r.file)));
            }
            ran.push(own);
        }
        None
    }
}

/// What `__answer` gives for a middleware's null: no answer, but that the routes of the group
/// whose middleware is at `line` do not match (see `Interp::handle_inner`; FEEDBACK51.md, Q-651).
pub(crate) fn no_match(line: u32) -> crate::value::Response {
    crate::value::Response { status: 0, headers: vec![(NO_MATCH.into(), line.to_string())], body: crate::value::Body::Value(Value::Nil) }
}

const NO_MATCH: &str = "x-quillo-no-match";

/// The group a middleware's null came from, as the calls of its middleware down to that one:
/// none if `ran` is no such null.
fn no_match_of(r: &crate::ast::RouteDef, ran: &Result<Value, Ctrl>) -> Option<Vec<(Option<Arc<str>>, u32)>> {
    let Err(Ctrl::Reply(resp)) = ran else { return None };
    if resp.status != 0 {
        return None;
    }
    let line: u32 = resp.headers.iter().find(|(k, _)| k == NO_MATCH).and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    let calls = middleware_calls(r);
    let upto = calls.iter().position(|c| c.1 == line).map_or(calls.len(), |i| i + 1);
    Some(calls[..upto].to_vec())
}

/// A route's middleware calls, those of its groups that start its body, by their file and line.
fn middleware_calls(r: &crate::ast::RouteDef) -> Vec<(Option<Arc<str>>, u32)> {
    let crate::ast::Expr::Block(stmts) = &r.body else { return Vec::new() };
    stmts
        .iter()
        .map_while(|s| match s {
            crate::ast::Stmt::Expr(crate::ast::Expr::Call(f, _, line)) if matches!(&**f, crate::ast::Expr::Ident(n, _) if &**n == "__answer") => Some((r.file.clone(), *line)),
            _ => None,
        })
        .collect()
}

/// A guard route's middleware calls, by their file and line.
fn guard_calls(r: &crate::ast::RouteDef) -> Vec<(Option<Arc<str>>, u32)> {
    let crate::ast::Expr::Block(stmts) = &r.body else { return Vec::new() };
    stmts
        .iter()
        .filter_map(|s| match s {
            crate::ast::Stmt::Expr(crate::ast::Expr::Call(_, _, line)) => Some((r.file.clone(), *line)),
            _ => None,
        })
        .collect()
}

/// Methods RFC 9110 and RFC 5789 define: one of them no route takes is a 405 or a 404, any
/// other a 501.
const KNOWN_METHODS: [&str; 9] = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "CONNECT", "TRACE"];

/// `Allow`'s value: the methods given, with `HEAD` where `GET` is and `OPTIONS`, which is
/// answered for every path a route has.
fn allow_list(methods: &mut Vec<&str>) -> String {
    if methods.contains(&"GET") {
        methods.push("HEAD");
    }
    methods.push("OPTIONS");
    methods.sort_unstable();
    methods.dedup();
    methods.join(", ")
}

/// `OPTIONS` on a path no route takes it for, or `OPTIONS *`: 204 with `Allow`, as RFC 9110
/// §9.3.7 says, rather than a 405, which marks the server down for a health check that sends
/// it (HAProxy's `option httpchk`) (FEEDBACK36.md, Q-440; branch feedback36-39).
fn options_answer(methods: &mut Vec<&str>) -> HttpResponse {
    HttpResponse { status: 204, headers: vec![("allow".into(), allow_list(methods))], body: Vec::new() }
}

impl Interp {
    /// `/_quillo/...`: the browser bundle and `server fn` calls.
    /// A scheduled job, asked for by the scheduler (see `jobs`).
    fn run_job(&mut self, name: &str, req: &HttpRequest, qs: &str) -> HttpResponse {
        if req.method != "POST" || !is_job_request(req) {
            return plain(404, "not found\n");
        }
        // A job or a tick runs for no one (see `wt::reach`).
        self.answering = false;
        // A job of a service this process runs, `<service>~<name>`, runs in the
        // service (FEEDBACK12.md, Q-083; branch feedback12-fixes).
        if let Some((service, job)) = name.split_once('~') {
            let found = self.services.loaded.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(service).map(|(_, s)| s.fork());
            return match found {
                Some(mut sub) => sub.run_job(job, req, qs),
                None => plain(404, "no such service\n"),
            };
        }
        let Some(job) = self.site.jobs.iter().find(|j| j.name == name).cloned() else {
            return plain(404, "no such job\n");
        };
        let at = qs.split('&').find_map(|kv| kv.strip_prefix("at=")).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        if !crate::db::claim_job(self, name, at) {
            return plain(200, "run by another server\n");
        }
        // Its `sleep` ends at a stop or a reload (FEEDBACK12.md, Q-081 and Q-082;
        // branch feedback12-sleep).
        self.in_job = true;
        let ran = self.call(&job.f, Vec::new(), 0);
        self.in_job = false;
        match finish(ran) {
            r if r.status >= 500 => r,
            _ => plain(200, "ok\n"),
        }
    }

    /// A tick (`every` at the top of a program), asked for by its thread (see
    /// `jobs`): run here, never claimed, as each process ticks its own
    /// (FEEDBACK12.md, Q-080; branch feedback12-fixes).
    fn run_tick(&mut self, name: &str, req: &HttpRequest) -> HttpResponse {
        if req.method != "POST" || !is_job_request(req) {
            return plain(404, "not found\n");
        }
        // A job or a tick runs for no one (see `wt::reach`).
        self.answering = false;
        if let Some((service, tick)) = name.split_once('~') {
            let found = self.services.loaded.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(service).map(|(_, s)| s.fork());
            return match found {
                Some(mut sub) => sub.run_tick(tick, req),
                None => plain(404, "no such service\n"),
            };
        }
        let Some(tick) = self.site.ticks.iter().find(|t| t.name == name).cloned() else {
            return plain(404, "no such tick\n");
        };
        // As a job's (FEEDBACK12.md, Q-081 and Q-082; branch feedback12-sleep).
        self.in_job = true;
        let ran = self.call(&tick.f, Vec::new(), 0);
        self.in_job = false;
        match finish(ran) {
            r if r.status >= 500 => r,
            _ => plain(200, "ok\n"),
        }
    }

    /// What every answer carries that its route did not set: the program's `site_headers`, then
    /// Quillo's own defaults. A page made from who the visitor is (it read the session or a
    /// cookie) is `Cache-Control: private` with `Vary: Cookie`, so a shared cache does not hand
    /// it to the next visitor, and a page is not framed by another site, `X-Frame-Options:
    /// SAMEORIGIN` (FEEDBACK31.md, Q-391; branch feedback27-35-fixes).
    fn site_headers_into(&self, path: &str, sets_cookie: bool, resp: &mut HttpResponse) {
        // On the path as routes read it (Q-537).
        let path = &route_path(path);
        let under = |p: &str| {
            let p = route_path(p);
            p == "/" || *path == p || path.starts_with(&format!("{}/", p))
        };
        let mut want: Vec<(String, Option<String>)> = Vec::new();
        let mut rules: Vec<&(String, Vec<(String, Option<String>)>)> = self.site.headers.iter().filter(|(p, _)| under(p)).collect();
        rules.sort_by_key(|(p, _)| if p == "/" { 0 } else { p.len() });
        for (_, hs) in rules {
            for (k, v) in hs {
                match want.iter_mut().find(|(n, _)| n == k) {
                    Some(w) => w.1 = v.clone(),
                    None => want.push((k.clone(), v.clone())),
                }
            }
        }
        let html = resp.header("content-type").is_some_and(|c| c.starts_with("text/html"));
        let framed_by_policy = resp.header("content-security-policy").is_some_and(|c| c.contains("frame-ancestors"))
            || want.iter().any(|(k, v)| k == "content-security-policy" && v.as_deref().is_some_and(|c| c.contains("frame-ancestors")));
        if html && !framed_by_policy && !want.iter().any(|(k, _)| k == "x-frame-options") {
            want.push(("x-frame-options".into(), Some("SAMEORIGIN".into())));
        }
        let personal = (self.visitor_read || sets_cookie) && resp.status < 400;
        if personal && !want.iter().any(|(k, _)| k == "cache-control") {
            want.push(("cache-control".into(), Some("private".into())));
        }
        for (k, v) in want {
            if resp.header(&k).is_some() {
                continue;
            }
            if let Some(v) = v {
                resp.headers.push((k, v));
            }
        }
        // Not when the program said `vary: null` (FEEDBACK55.md, Q-710; branch feedback55).
        if personal && !resp.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("vary") && (v == DROPPED || v.to_ascii_lowercase().contains("cookie"))) {
            resp.headers.push(("vary".into(), "Cookie".into()));
        }
    }

    fn handle_internal(&mut self, rest: &str, req: &HttpRequest, path: &str, qs: &str) -> HttpResponse {
        // A page's sheet; one no longer kept (or from before a reload) is
        // every style.
        if let Some(hash) = rest.strip_prefix("s/").and_then(|r| r.strip_suffix(".css")) {
            let (css, kept) = self.sheet_by_hash(hash);
            return HttpResponse {
                status: 200,
                headers: vec![
                    ("content-type".into(), "text/css; charset=utf-8".into()),
                    ("cache-control".into(), if kept { "public, max-age=31536000, immutable" } else { "no-cache" }.into()),
                ],
                body: css.as_bytes().to_vec(),
            };
        }
        // A page's translations (FEEDBACK30.md, Q-372; branch feedback27-35-fixes). One no longer
        // kept is every key of the default locale, which does no harm.
        if let Some(hash) = rest.strip_prefix("t/").and_then(|r| r.strip_suffix(".js")) {
            let (js, cache) = match crate::i18n::file(hash) {
                Some(js) => (js.to_string(), "public, max-age=31536000, immutable"),
                None => (self.site.i18n.as_ref().map_or_else(String::new, |i| format!("{}.i18n={};", crate::client::HANDOFF_JS, i.browser_json(&i.default.clone(), None))), "no-cache"),
            };
            return HttpResponse {
                status: 200,
                headers: vec![("content-type".into(), "text/javascript; charset=utf-8".into()), ("cache-control".into(), cache.into())],
                body: js.into_bytes(),
            };
        }
        // The runtime file pages' bundles share; another than this program's
        // (a page from before a reload) gets this one, which does no harm:
        // its bundle is not kept either, so it is sent the whole bundle.
        if let Some(hash) = rest.strip_prefix("r/").and_then(|r| r.strip_suffix(".js")) {
            let (js, cache) = match self.runtime_file() {
                Some((h, js)) if h == hash => (js, "public, max-age=31536000, immutable"),
                Some((_, js)) => (js, "no-cache"),
                None => (Arc::new(String::new()), "no-cache"),
            };
            return HttpResponse {
                status: 200,
                headers: vec![("content-type".into(), "text/javascript; charset=utf-8".into()), ("cache-control".into(), cache.into())],
                body: js.as_bytes().to_vec(),
            };
        }
        // A page's own bundle: its address names its content, so it is kept.
        // One this program did not make (a page from before a reload) gets
        // the whole bundle, which has every component.
        if let Some(hash) = rest.strip_prefix("b/").and_then(|r| r.strip_suffix(".js")) {
            let (js, cache) = match self.bundle_by_hash(hash) {
                Some(js) => (js, "public, max-age=31536000, immutable"),
                None => (self.client_js(), "no-cache"),
            };
            return HttpResponse {
                status: 200,
                headers: vec![("content-type".into(), "text/javascript; charset=utf-8".into()), ("cache-control".into(), cache.into())],
                body: js.as_bytes().to_vec(),
            };
        }
        // In development, the files a page's bundle loads (see `client::DEV`).
        if let Some((hash, file)) = rest.strip_prefix("src/").and_then(|r| r.split_once('/')) {
            let file = percent_decode(file, false);
            return match crate::client::dev_file(hash, &file) {
                Some(js) => HttpResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "text/javascript; charset=utf-8".into()), ("cache-control".into(), "no-cache".into())],
                    body: js.into_bytes(),
                },
                None => plain(404, "not found\n"),
            };
        }
        if rest == "media.js" {
            // With a validator, as quillo.js has, so that a page that plays a stream does not
            // fetch its 39 KB again each time (FEEDBACK36.md, Q-441; branch feedback36-39).
            let body = crate::client::media_script(&self.base);
            let etag = format!("\"{:016x}\"", {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                body.hash(&mut h);
                h.finish()
            });
            return HttpResponse {
                status: 200,
                headers: vec![
                    ("content-type".into(), "text/javascript; charset=utf-8".into()),
                    ("cache-control".into(), "no-cache".into()),
                    ("etag".into(), etag),
                ],
                body: body.into_bytes(),
            };
        }
        if rest == "quillo.js" {
            // The browser keeps its copy and asks whether it is still current.
            let etag = format!("\"{:016x}\"", {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                self.client_js().hash(&mut h);
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
                body: if fresh { Vec::new() } else { self.client_js().as_bytes().to_vec() },
            };
        }
        // Whether what the program needs is there (FEEDBACK10.md, Q-076;
        // branch feedback10-fixes).
        if rest == "ready" {
            return ready_response(self, req);
        }
        // A copy of one of its services joining (FEEDBACK10.md, Q-074;
        // branch feedback10-fixes).
        if rest == "join" {
            return crate::service::join_here(self, req);
        }
        // The program's API, for a gateway or a reader, under `--describe`
        // (FEEDBACK10.md, Q-077; branch feedback10-fixes).
        if rest == "describe" && crate::describe::ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
            // The file's name, as `quillo docs` titles it, however the program was started
            // (FEEDBACK32.md, Q-398; branch feedback27-35-fixes).
            let title = crate::service::served_as().map(str::to_string).filter(|t| !t.is_empty()).unwrap_or_else(|| "api".into());
            return json_response(200, crate::describe::of_interp(self, &title));
        }
        if rest == "live" {
            // Without waiting; the server's worker threads answer `live`
            // requests themselves and wait for a change first.
            let (since, server, topics) = live_query(qs);
            // The tenant is the request's: without it set, none was found, and a tenant's own
            // topics were dropped too (FEEDBACK51.md, Q-666; branch feedback51-fixes).
            self.current_request = Some(Value::map(build_request(req, path, &Value::map(Map::new()))));
            let tenant = self.tenant();
            self.current_request = None;
            let topics = own_topics(topics, self.site.tenant_rule.is_some(), tenant.as_deref());
            let (seq, changed) = self.shared.live_now(since, server.as_deref(), &topics);
            return json_response(200, serde_json::json!({ "seq": seq, "server": self.shared.id, "changed": changed }));
        }
        if let Some(name) = rest.strip_prefix("job/") {
            return self.run_job(name, req, qs);
        }
        if let Some(name) = rest.strip_prefix("tick/") {
            return self.run_tick(name, req);
        }
        // What pages sent on a channel (see `wt`).
        if let Some(name) = rest.strip_prefix("channel/") {
            if req.method != "POST" || !is_job_request(req) {
                return plain(404, "not found\n");
            }
            return crate::wt::answer(self, name, &req.body);
        }
        // A page of the program's, about to connect for channels: who it is.
        if rest == "wt-ticket" {
            if req.method != "POST" {
                return plain(405, "method not allowed\n");
            }
            if cross_site(req) {
                return plain(403, "only the program's own pages connect\n");
            }
            self.current_request = Some(Value::map(build_request(req, path, &Value::map(Map::new()))));
            let r = crate::wt::ticket(self);
            self.current_request = None;
            return r;
        }
        // Packets that came to a port the program listens on (see `udp`).
        if let Some(port) = rest.strip_prefix("udp/") {
            if req.method != "POST" || !is_job_request(req) {
                return plain(404, "not found\n");
            }
            return crate::udp::answer(self, port, &req.body);
        }
        // The scheduler, asking that the calls made later that are due be made.
        if rest == "later" {
            if req.method != "POST" || !is_job_request(req) {
                return plain(404, "not found\n");
            }
            return crate::service::deliver(self);
        }
        // A service's channels under --split: the app hands it what pages sent
        // on them, and it sends to the pages through the app (FEEDBACK12.md,
        // Q-087; branch open-fixes).
        if let Some(name) = rest.strip_prefix("service-channel/") {
            if let Err(r) = crate::service::signed_for(req, &format!("_channel:{}", name), true) {
                return r;
            }
            return crate::wt::answer(self, name, &req.body);
        }
        if rest == "service-channels" {
            if let Err(r) = crate::service::signed_for(req, "_channels", true) {
                return r;
            }
            return crate::wt::names_here(self);
        }
        if rest == "channel-relay" {
            if let Err(r) = crate::service::signed_for(req, "_channel-relay", false) {
                return r;
            }
            return crate::wt::relay_here(&req.body);
        }
        if let Some(name) = rest.strip_prefix("service/") {
            return crate::service::answer(self, name, req);
        }
        // `markdown` in a page: made here, so that it is the server's HTML
        // (see B.markdown in runtime.js). A call from the page's own site,
        // as a server fn's is.
        if rest == "markdown" {
            if req.method != "POST" {
                return plain(405, "method not allowed\n");
            }
            let ctype = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map_or("", |(_, v)| v.as_str());
            if essence(ctype) != "application/json" {
                return rpc_error(415, "send JSON (content-type: application/json)");
            }
            if cross_site(req) {
                return rpc_error(403, "only the program's own pages make markdown here");
            }
            let args = match serde_json::from_slice::<serde_json::Value>(&req.body) {
                Ok(serde_json::Value::Array(a)) if (1..=2).contains(&a.len()) => a.iter().map(Value::from_json).collect(),
                _ => return rpc_error(400, "the body must be a JSON array: [text, options]"),
            };
            return match crate::builtins::markdown(self, args, 0) {
                Ok(v) => json_response(200, serde_json::json!({ "ok": v.display() })),
                Err(Ctrl::Error(e)) => rpc_error(400, &e.msg),
                Err(_) => rpc_error(500, "markdown failed"),
            };
        }
        let Some(name) = rest.strip_prefix("rpc/") else {
            return plain(404, "not found\n");
        };
        // A service's server fns are for other services, whose calls are
        // signed (`service/` above): never answered unsigned.
        if self.is_service {
            return rpc_error(404, &format!("no server fn `{}` (a service's server fns are called by other services, not by pages)", name));
        }
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
        // `for [...]` names the services that may call it, over their signed path: no page may,
        // nor anyone with curl. It was checked on that path only (FEEDBACK51.md, Q-641; branch
        // feedback51-fixes).
        if let Some(callers) = self.fn_callers.get(name) {
            return rpc_error(404, &format!("no server fn `{}` for pages: it takes calls from {} only", name, callers.join(", ")));
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
        // A file anywhere in them, a list's or a map's too (FEEDBACK30.md, Q-364; branch
        // feedback27-35-fixes).
        fn arg_of(j: &serde_json::Value, files: &Map) -> Value {
            match j {
                serde_json::Value::Object(o) if o.len() == 1 && o.contains_key("$quillo_file") => match o["$quillo_file"].as_u64() {
                    Some(i) => files.get(format!("file{}", i).as_str()).cloned().unwrap_or(Value::Nil),
                    None => Value::Nil,
                },
                serde_json::Value::Array(a) => Value::list(a.iter().map(|x| arg_of(x, files)).collect()),
                // Decimals and large ints as the page sends them ({"$dec": "12.50"}).
                serde_json::Value::Object(o) if o.len() == 1 && o.keys().all(|k| matches!(k.as_str(), "$dec" | "$int" | "$quillo_err")) => crate::interp::from_island_json(j),
                serde_json::Value::Object(o) => Value::map(o.iter().map(|(k, v)| (k.as_str().into(), arg_of(v, files))).collect()),
                other => crate::interp::from_island_json(other),
            }
        }
        let args = match serde_json::from_slice::<serde_json::Value>(&json_args) {
            Ok(serde_json::Value::Array(a)) => a.iter().map(|j| arg_of(j, &files)).collect::<Vec<_>>(),
            _ => return rpc_error(400, "the body must be a JSON array of arguments"),
        };
        let args = match take_args(self, &c.def, args) {
            Ok(a) => a,
            Err(msg) => return rpc_error(400, &msg),
        };
        let query = Value::map(parse_urlencoded(qs));
        let mut reqmap = build_request(req, path, &query);
        if let Some(id) = &self.request_id {
            reqmap.insert("id".into(), Value::str(&**id));
        }
        self.current_request = Some(Value::map(reqmap));
        // A `live` refresh: also report what the function read. A run made
        // for another page moments ago, still good, is that page's answer
        // too; and a page that says what it has is sent what changed
        // (FEEDBACK2.md, Q-028; branch feedback6-fixes).
        let hdr = |n: &str| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v.as_str());
        let live = hdr("x-quillo-live").is_some();
        if !live {
            let res = self.call_closure(&c, args.into_iter().map(Some).collect(), 0);
            self.current_request = None;
            return match res {
                Ok(v) | Err(Ctrl::Return(v)) => match for_visitor(v) {
                    Value::Response(r) => render_response(&r),
                    Value::Err(e, _, _) => json_response(200, serde_json::json!({ "err": crate::interp::island_json(&e) })),
                    v => json_response(200, serde_json::json!({ "ok": crate::interp::island_json(&v) })),
                },
                Err(Ctrl::Reply(r)) => render_response(&r),
                Err(Ctrl::Error(e)) => {
                    eprintln!("quillo: error in server fn `{}`: {:#}", name, e);
                    rpc_error(500, &format!("internal error in `{}`", name))
                }
            };
        }
        let had = hdr("x-quillo-had").and_then(crate::livecache::parse_hash);
        let key = boundary.is_none().then(|| (name.to_string(), json_args.clone(), hdr("host").unwrap_or("").to_string()));
        let viewer = viewer_of(&req.remote, |n| hdr(n).map(str::to_string));
        let (found, mut flying) = match &key {
            Some(k) => self.live_turn(k, viewer),
            None => (None, None),
        };
        let (answer, topics, seq) = match found {
            Some(f) => {
                crate::metrics::live_run(true);
                self.current_request = None;
                f
            }
            None => {
                crate::metrics::live_run(false);
                let seq = self.shared.live_seq();
                self.reads = Some(Default::default());
                self.read_request = false;
                self.own_seqs.clear();
                let cookies = self.set_cookies.len();
                let res = self.call_closure(&c, args.into_iter().map(Some).collect(), 0);
                let mut topics: Vec<String> = self.reads.take().unwrap_or_default().into_iter().collect();
                // Every change since the run began its own (it marked what it
                // shows as read, say): the answer has them already, so the
                // page is told it has seen them, and the loader does not wake
                // itself for ever (FEEDBACK13.md, Q-090; branch feedback13-fixes).
                let own = std::mem::take(&mut self.own_seqs);
                let end = self.shared.live_seq();
                let seen = if end > seq && (seq + 1..=end).all(|s| own.contains(&s)) { end } else { seq };
                topics.sort();
                self.shared.track(&topics);
                self.current_request = None;
                let answer = Arc::new(match res {
                    Ok(v) | Err(Ctrl::Return(v)) => match for_visitor(v) {
                        Value::Response(r) => return render_response(&r),
                        Value::Err(e, _, _) => crate::livecache::Answer::new(crate::interp::island_json(&e), true),
                        v => crate::livecache::Answer::new(crate::interp::island_json(&v), false),
                    },
                    Err(Ctrl::Reply(r)) => return render_response(&r),
                    Err(Ctrl::Error(e)) => {
                        eprintln!("quillo: error in server fn `{}`: {:#}", name, e);
                        return rpc_error(500, &format!("internal error in `{}`", name));
                    }
                });
                if let Some(f) = &mut flying {
                    f.personal = self.read_request;
                }
                // Not shared: a run that wrote, or set a cookie, or read what
                // changed as it ran. What others wrote elsewhere has nothing
                // to do with it (FEEDBACK24.md, Q-191; branch feedback24-fixes).
                if let Some(k) = key {
                    if self.set_cookies.len() == cookies && own.is_empty() && self.shared.unchanged_since(&topics, seq) {
                        let mut cache = self.shared.live_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        cache.share(k, self.read_request.then_some(viewer), seq, topics.clone(), answer.clone());
                    }
                }
                (answer, topics, seen)
            }
        };
        let mut patch = None;
        if !answer.err {
            let mut cache = self.shared.live_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            cache.keep(answer.h, &answer.text);
            patch = had.and_then(|from| cache.patch(from, answer.h, &answer.value)).filter(|p| p.len() < answer.text.len());
        }
        // Past what a page should be sent, whoever wrote the loader should hear of it.
        if answer.text.len() > LIVE_LARGE {
            static SAID: Mutex<Option<std::collections::HashSet<String>>> = Mutex::new(None);
            if SAID.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert_with(Default::default).insert(name.to_string()) {
                eprintln!("quillo: `{}` gives {} MB to each live page: a loader should give what the page shows, not the table", name, answer.text.len() >> 20);
            }
        }
        crate::metrics::live_answer(if had == Some(answer.h) { 0 } else if patch.is_some() { 1 } else { 2 });
        let (field, value) = match &patch {
            Some(p) => ("patch", &**p),
            None => (if answer.err { "err" } else { "ok" }, &*answer.text),
        };
        let mut resp = json_response(200, serde_json::Value::Null);
        resp.body = format!(
            "{{\"{}\":{},\"h\":\"{}\",\"topics\":{},\"seq\":{},\"server\":{}}}",
            field,
            value,
            crate::livecache::hash_text(answer.h),
            serde_json::json!(topics),
            seq,
            serde_json::json!(self.shared.id)
        )
        .into_bytes();
        // A whole answer many pages share is gzipped once.
        if patch.is_none() && resp.body.len() >= LIVE_GZ_KEPT {
            let mut kept = answer.gz.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match &*kept {
                Some((s, gz)) if *s == seq && accepts_gzip(req) => {
                    resp.body = gz.to_vec();
                    resp.headers.push(("content-encoding".into(), "gzip".into()));
                    resp.headers.push(("vary".into(), "accept-encoding".into()));
                }
                _ => {
                    compress(req, &mut resp);
                    if resp.header("content-encoding").is_some() {
                        *kept = Some((seq, Arc::new(resp.body.clone())));
                    }
                }
            }
        }
        resp
    }

    /// What a page load shares a run by: the same as a refresh's, where it can
    /// tell who is asking.
    pub(crate) fn live_key(&mut self, name: &str, args: &[Value]) -> Option<(crate::livecache::RunKey, u64)> {
        let read = self.read_request;
        let req = self.request_value();
        self.read_request = read;
        let Some(Value::Map(req)) = req else { return None };
        let Some(Value::Map(headers)) = req.get("headers") else { return None };
        let hdr = |n: &str| headers.get(n).map(|v| v.display());
        let ip = req.get("ip").map_or(String::new(), |v| v.display());
        let viewer = viewer_of(&ip, hdr);
        let json = serde_json::Value::Array(args.iter().map(crate::interp::island_json).collect()).to_string().into_bytes();
        Some(((name.to_string(), json, hdr("host").unwrap_or_default()), viewer))
    }

    /// A call of a live loader: a run it may have, or else it runs, its turn taken
    /// (the landing, for those that wait for it). One run at a time of a call.
    pub(crate) fn live_turn(&mut self, key: &crate::livecache::RunKey, viewer: u64) -> (Option<crate::livecache::Shared>, Option<Landing>) {
        let shared = self.shared.clone();
        let find = || {
            let mut cache = shared.live_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            cache.shared(key, viewer, |t, seq| shared.unchanged_since(t, seq))
        };
        if let Some(f) = find() {
            return (Some(f), None);
        }
        for _ in 0..3 {
            let begun = shared.live_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).begin(key);
            match begun {
                crate::livecache::Begun::Run => return (None, Some(Landing { shared: shared.clone(), key: key.clone(), personal: false })),
                crate::livecache::Begun::Alone => break,
                crate::livecache::Begun::Wait(f) => {
                    if crate::httpd::wait_elsewhere(move || f.wait(crate::livecache::SHARED_FOR)).is_err() {
                        break;
                    }
                    if let Some(f) = find() {
                        return (Some(f), None);
                    }
                }
            }
        }
        (None, None)
    }
}

/// Who asks, for a run that read the request: the address, without the port each
/// connection has its own of, and what identifies them.
fn viewer_of(remote: &str, hdr: impl Fn(&str) -> Option<String>) -> u64 {
    let mut text = remote.parse::<std::net::SocketAddr>().map_or_else(|_| remote.to_string(), |a| a.ip().to_string());
    for n in ["cookie", "authorization", "accept-language", "user-agent"] {
        text.push('\n');
        text.push_str(&hdr(n).unwrap_or_default());
    }
    crate::livecache::hash(&text)
}

/// A loader's answer past this is said in the log, once for each.
const LIVE_LARGE: usize = 4 << 20;
/// Whole answers from this size keep their gzipped copy.
const LIVE_GZ_KEPT: usize = 16 << 10;

/// The end of a run others may be waiting for, however it ends.
pub(crate) struct Landing {
    shared: Arc<Shared>,
    key: crate::livecache::RunKey,
    pub personal: bool,
}

impl Drop for Landing {
    fn drop(&mut self) {
        self.shared.live_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).land(&self.key, self.personal);
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

/// The topics a page may follow: not another tenant's, `acme@dm:alice:bob` from tenant `evil`,
/// which told it when Bob wrote Alice (FEEDBACK51.md, Q-666; branch feedback51-fixes). A
/// topic is a tenant's when it starts with a tenant's name and `@` (after a mount's `base|`).
fn own_topics(topics: Vec<String>, tenants: bool, tenant: Option<&str>) -> Vec<String> {
    if !tenants {
        return topics;
    }
    let owner = |t: &str| {
        // A mounted program's topics start `/prefix|`; only that is taken off, as a tenant's
        // own topic may hold a `|` (`acme@inbox:x|evil@y` was evil's; found in review).
        let t = if t.starts_with('/') { t.split_once('|').map_or(t, |(_, rest)| rest) } else { t };
        t.split_once('@').map(|(who, _)| who.to_string()).filter(|who| crate::site::tenant_name_ok(who))
    };
    topics.into_iter().filter(|t| owner(t).is_none_or(|who| Some(who.as_str()) == tenant)).collect()
}

/// The tenant a live request comes from, and whether the program has tenants: worked out by the
/// program, as for any request.
fn live_tenant(app: &App, req: &HttpRequest, path: &str) -> (bool, Option<String>) {
    let current = app.current().1;
    let Loaded::Ready(it) = &*current else { return (false, None) };
    if it.site.tenant_rule.is_none() {
        return (false, None);
    }
    let mut it = it.fork();
    it.current_request = Some(Value::map(build_request(req, path, &Value::map(Map::new()))));
    (true, it.tenant().map(|t| t.to_string()))
}

/// Most pages that can wait for changes at once (live data and `--watch`
/// reloads). Each has a thread; beyond this, pages are told to try later
/// rather than let anyone with a browser start threads without end.
pub const MAX_WAITERS: usize = 4096;

/// Stack for the threads that wait for changes; they run no Quillo code.
const WAITER_STACK: usize = 256 << 10;

/// Most of those one client can hold (an address, or an IPv6 /64), so that
/// one cannot take them all and leave the others' pages without updates.
pub const MAX_WAITERS_PER_CLIENT: usize = 256;

/// `--live-per-client`: that limit, where one address really is many visitors.
pub static LIVE_PER_CLIENT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(MAX_WAITERS_PER_CLIENT);

static WAITERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// The places taken, by client.
static WAITING: Mutex<Option<std::collections::HashMap<String, usize>>> = Mutex::new(None);

/// A place among the `MAX_WAITERS`, given back when dropped.
struct Waiter(String);

impl Waiter {
    fn take(ip: &str) -> Option<Waiter> {
        use std::sync::atomic::Ordering::SeqCst;
        let who = crate::statics::client_of(ip);
        let mut g = WAITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let m = g.get_or_insert_with(Default::default);
        let mine = m.get(&who).copied().unwrap_or(0);
        if mine >= LIVE_PER_CLIENT.load(SeqCst) {
            // Behind a proxy every visitor is one address, and pages past the
            // limit show old data with no sign of it (FEEDBACK24.md, Q-189;
            // branch feedback24-fixes).
            static SAID: std::sync::Once = std::sync::Once::new();
            SAID.call_once(|| {
                eprintln!("quillo: {} holds {} pages waiting for changes, and more are refused: behind a proxy, name it with --trust-proxy; for one address with many visitors, raise --live-per-client", who, mine);
            });
            return None;
        }
        if WAITERS.fetch_add(1, SeqCst) >= MAX_WAITERS {
            WAITERS.fetch_sub(1, SeqCst);
            return None;
        }
        m.insert(who.clone(), mine + 1);
        Some(Waiter(who))
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        WAITERS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        let mut g = WAITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(n) = g.as_mut().and_then(|m| m.get_mut(&self.0)) {
            *n -= 1;
            if *n == 0 {
                g.as_mut().map(|m| m.remove(&self.0));
            }
        }
    }
}

/// What the server does with a request.
pub(crate) enum Answer {
    Respond(HttpResponse),
    /// The head of a large file's answer to a HEAD, with the length a GET would send, and no
    /// body: it takes no place among the large sends (FEEDBACK55.md, Q-725; branch feedback55).
    Head(HttpResponse, u64),
    /// The connection goes to a thread of its own (pages waiting for live
    /// data or reloads, large files), which answers on it directly.
    Detach(crate::httpd::Detached),
    /// Sent already, as it was made (`stream`, `events`); `close` when the
    /// connection cannot carry another answer.
    Streamed { close: bool },
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
fn send_file(resp: HttpResponse, what: (std::path::PathBuf, u64, u64, Option<crate::statics::Hosted>), head: bool, ip: &str) -> Answer {
    // Only what is sent is counted: a HEAD sends no body, so it is answered here, where eight
    // large sends to the client had it refused (FEEDBACK55.md, Q-725; branch feedback55).
    if head {
        return Answer::Head(resp, what.2);
    }
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
/// waiting places; answers 503 when they are all taken, or the client at
/// `ip` holds its share of them.
fn waiter(name: &'static str, ip: &str, f: impl FnOnce(std::net::TcpStream) + Send + 'static) -> Answer {
    let Some(place) = Waiter::take(ip) else {
        crate::metrics::LIVE_REFUSED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

/// How often a waiting page's socket is looked at, how often it is pinged, and
/// how long after a change the next may go.
const LIVE_LOOK: Duration = Duration::from_secs(5);
const LIVE_PING: Duration = Duration::from_secs(20);
const LIVE_GATHER: Duration = Duration::from_millis(100);

/// Answers `/_quillo/live` on a thread of its own, so open pages do not hold
/// on to the server's threads. Over a WebSocket, each change to the topics
/// is sent as `{seq, server, changed}` for as long as the page stays; as a
/// plain request, the answer is the first change, or none after a while.
fn serve_live(shared: Arc<Shared>, req: HttpRequest, (tenants, tenant): (bool, Option<String>)) -> Answer {
    let (mut since, server, topics) = live_query(req.url.split_once('?').map_or("", |(_, q)| q));
    let topics = own_topics(topics, tenants, tenant.as_deref());
    waiter("quillo-live", &req.remote.clone(), move |stream| {
        let _page = crate::metrics::LivePage::new();
        let following = shared.follow(topics);
        if crate::ws::is_upgrade(&req) {
            let Some(mut socket) = crate::ws::Socket::accept(&req, stream) else { return };
            // A page that does not read is let go, not waited on for good (FEEDBACK49.md,
            // Q-594; found in review).
            socket.write_timeout(Duration::from_secs(30));
            let mut server = server;
            let mut quiet = Duration::ZERO;
            let mut next = Instant::now();
            loop {
                let (mut seq, mut changed) = following.wait(since, server.as_deref(), LIVE_LOOK);
                // A page that did not know the sequence number, or knew one
                // from another server or from before a restart, is told the
                // current one.
                if changed.is_empty() && since != 0 && since <= seq && server.is_none() {
                    quiet += LIVE_LOOK;
                    if socket.gone() {
                        return;
                    }
                    if quiet >= LIVE_PING {
                        if !socket.ping() {
                            return;
                        }
                        quiet = Duration::ZERO;
                    }
                } else {
                    // The first change goes at once; what follows it closely is
                    // gathered into one frame, so a burst of writes is one
                    // refresh (FEEDBACK24.md, Q-190; branch feedback24-fixes).
                    let early = next.saturating_duration_since(Instant::now());
                    if !early.is_zero() {
                        std::thread::sleep(early);
                        (seq, changed) = following.wait(since, server.as_deref(), Duration::ZERO);
                    }
                    if !socket.send(&live_json(&shared, seq, &changed)) {
                        return;
                    }
                    next = Instant::now() + LIVE_GATHER + Duration::from_millis(fastrand::u64(..50));
                    quiet = Duration::ZERO;
                }
                since = seq;
                server = None;
            }
        }
        let (seq, changed) = following.wait(since, server.as_deref(), Duration::from_secs(25));
        let mut resp = json_response(200, serde_json::from_str(&live_json(&shared, seq, &changed)).unwrap_or_default());
        resp.headers.push(("cache-control".into(), "no-store".into()));
        send_on(stream, resp);
    })
}

/// A page's channels over a WebSocket (where WebTransport cannot be had),
/// on a thread of its own for as long as the page stays.
fn serve_channels(req: HttpRequest) -> Answer {
    let Some(ticket) = crate::wt::ticket_of(req.url.split_once('?').map_or("", |(_, q)| q)) else {
        return Answer::Respond(plain(403, "no ticket, or an old one\n"));
    };
    waiter("quillo-ch", &req.remote.clone(), move |stream| {
        if let Some(socket) = crate::ws::Socket::accept(&req, stream) {
            crate::wt::over_websocket(socket, ticket);
        }
    })
}

/// The arguments a browser (or another service) calls a `server fn` with,
/// checked all the way in (list items, map values and record fields, since
/// the function trusts its types) and made into its parameters' types as
/// `Type(v)` makes them: a record keeps only its own fields, so what is
/// sent cannot add `is_admin` to one, and fields left out get their defaults.
/// A `dec` (a parameter or a record's field) takes a decimal, an int, a
/// number or numeric text ("12.50"), as a `dec` pattern takes it.
pub(crate) fn take_args(it: &mut Interp, def: &crate::ast::FnDef, args: Vec<Value>) -> Result<Vec<Value>, String> {
    if args.len() > def.params.len() {
        return Err(format!("`{}` takes {} arguments, got {}", def.name, def.params.len(), args.len()));
    }
    if let Some(p) = def.params.get(args.len()..).and_then(|rest| rest.iter().find(|p| p.default.is_none())) {
        return Err(format!("missing argument `{}` for `{}`", p.name, def.name));
    }
    let mut out = Vec::with_capacity(args.len());
    for (v, p) in args.into_iter().zip(&def.params) {
        let Some(ty) = &p.ty else {
            out.push(v);
            continue;
        };
        match crate::record::make(it, ty, &v, false) {
            Ok(Ok(x)) => out.push(x),
            Ok(Err(why)) => return Err(format!("argument `{}` of `{}` must be {}, got {}{}", p.name, def.name, ty.text(), v.type_name(), crate::interp::because(&why))),
            Err(Ctrl::Error(e)) => return Err(format!("argument `{}` of `{}`: {}", p.name, def.name, e)),
            Err(_) => return Err(format!("argument `{}` of `{}` could not be made", p.name, def.name)),
        }
    }
    Ok(out)
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

/// Whether a page has a `<script>` element whose
/// code starts with `code`: one put in already, as a mounted program's page
/// has it, not text that quotes it, which is escaped.
fn script_with(body: &[u8], code: &[u8]) -> bool {
    let mut from = 0;
    while let Some(i) = body[from..].windows(code.len()).position(|w| w == code).map(|i| i + from) {
        from = i + code.len();
        let before = &body[..i];
        if before.ends_with(b"<script>") {
            return true;
        }
    }
    false
}

/// Whether a page has a `<script>` element whose `src` ends with `path`
/// (the program wrote it, or a mounted program's page already has it).
/// Only an element counts: text, which is escaped, cannot start one, nor
/// can `markdown` (whose `<img src=...>` could name the same path), so a
/// comment saying `/_quillo/quillo.js` does not stop the page loading it.
fn script_tag_for(body: &[u8], path: &[u8]) -> bool {
    let mut from = 0;
    while let Some(i) = body[from..].windows(path.len()).position(|w| w == path).map(|i| i + from) {
        from = i + path.len();
        if !matches!(body.get(from), Some(b'"' | b'\'' | b'?' | b'#')) {
            continue;
        }
        let Some(lt) = body[..i].iter().rposition(|b| *b == b'<') else { continue };
        let tag = &body[lt..i];
        if tag.len() > 7 && tag[..7].eq_ignore_ascii_case(b"<script") && matches!(tag[7], b' ' | b'\t' | b'\n' | b'/') && !tag.contains(&b'>') {
            return true;
        }
    }
    false
}

/// Whether a value has an error in it that a visitor is not to read, at any depth.
fn holds_hidden_error(v: &Value) -> bool {
    match v {
        Value::Err(e, own, _) => !own || holds_hidden_error(e),
        Value::List(l) => l.iter().any(holds_hidden_error),
        Value::Map(m) => m.iter().any(|(_, x)| holds_hidden_error(x)),
        _ => false,
    }
}

/// What a hidden error said, for the log.
pub(crate) fn log_hidden(e: &Value, at: &Place) {
    let text = crate::value::told(|| e.display());
    // Anyone can try a subdomain: that is not worth a line each.
    if !text.starts_with("db: there is no tenant") {
        crate::httpd::error_line(&at.before(&text));
    }
}

fn without_hidden_errors(v: &Value) -> Value {
    match v {
        Value::Err(e, false, at) => {
            log_hidden(e, at);
            Value::err(Value::str("internal error"))
        }
        Value::Err(e, true, _) => Value::own_err(without_hidden_errors(e)),
        Value::List(l) => Value::list(l.iter().map(without_hidden_errors).collect()),
        Value::Map(m) => Value::map(m.iter().map(|(k, x)| (k.clone(), without_hidden_errors(x))).collect()),
        other => other.clone(),
    }
}

/// What a visitor may see of `v`, when that is less than `v`: an error a builtin made says
/// `internal error`, and its text, a database's SQL or an address with its key, goes to the
/// log. `err(...)` written by the program is meant to be read and stays. Its author sees
/// all of it under `--watch`, and services, which are the program's own, get it whole
/// (FEEDBACK20.md, Q-161, and FEEDBACK21.md, Q-171; branch feedback20-fixes).
pub fn hidden(v: &Value) -> Option<Value> {
    if SHOW_ERRORS.load(std::sync::atomic::Ordering::Relaxed) || !holds_hidden_error(v) {
        return None;
    }
    Some(without_hidden_errors(v))
}

/// `hidden`, for a value owned here.
pub fn for_visitor(v: Value) -> Value {
    hidden(&v).unwrap_or(v)
}

/// Turns what a route produced into an HTTP response.
pub fn finish(res: Result<Value, Ctrl>) -> HttpResponse {
    match res {
        Ok(v) | Err(Ctrl::Return(v)) => value_response(v),
        Err(Ctrl::Reply(r)) => render_response(&r),
        Err(Ctrl::Error(e)) => {
            // A prefetch stopped before it changed something is expected.
            if !e.msg.contains(crate::interp::PREFETCH_REFUSED) {
                let head = format!("{}", e);
                let full = format!("{:#}", e);
                let calls: Vec<String> = full.get(head.len()..).unwrap_or("").lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect();
                crate::httpd::error_line_with(&head, &calls);
            }
            // The message names files and lines of the program; it is for
            // its author (`--watch`), not for whoever sent the request.
            if SHOW_ERRORS.load(std::sync::atomic::Ordering::Relaxed) {
                plain(500, &format!("internal error: {:#}\n", e))
            } else {
                plain(500, "internal error\n")
            }
        }
    }
}

/// Whether a failed request is answered with the error message (in
/// development, with `--watch`) or only "internal error".
pub static SHOW_ERRORS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Pages prefetch the links pointed at (cleared by `--no-prefetch`; see `PREFETCH_JS`).
pub static PREFETCH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// What every page carries to prefetch the pages it links to: a link of
/// this site pointed at (for 65 ms), pressed, touched or tabbed to is asked
/// for ahead of the click. Browsers with speculation rules (Chrome, Edge)
/// keep it for the navigation themselves; others get it with `fetch`, made
/// cacheable for 30 s for this (see `Interp::handle`), so the click is
/// answered from their cache. Not: links to other sites, with `target` or
/// `download`, to this same page, inside `data-no-prefetch`, those a page's
/// components follow themselves, or on a connection saving data.
pub const PREFETCH_JS: &str = concat!(
    "(()=>{const d=document,k=Symbol.for(\"quillo\"),w=window[k]||(window[k]={}),seen=new Set();if(w.pf)return;w.pf=1;",
    "const c=navigator.connection;if(c&&(c.saveData||/2g/.test(c.effectiveType||\"\")))return;",
    "const spec=HTMLScriptElement.supports&&HTMLScriptElement.supports(\"speculationrules\");",
    "const link=(e)=>e.target&&e.target.closest&&e.target.closest(\"a[href]\");",
    "const want=(a)=>{if(a.target&&a.target!==\"_self\"||a.hasAttribute(\"download\")||a.closest(\"[data-no-prefetch]\"))return null;",
    "if(w.nav&&a.closest(\"quillo-island,[data-qi]\"))return null;",
    "const u=new URL(a.href,location.href);if(u.origin!==location.origin||!/^https?:$/.test(u.protocol))return null;",
    "if(u.pathname===location.pathname&&u.search===location.search)return null;u.hash=\"\";return u.href};",
    "const go=(a)=>{const u=a&&want(a);if(!u||seen.has(u))return;seen.add(u);",
    "if(spec){const s=d.createElement(\"script\");s.type=\"speculationrules\";s.textContent=JSON.stringify({prefetch:[{source:\"list\",urls:[u]}]});d.head.appendChild(s)}",
    "else fetch(u,{credentials:\"same-origin\",headers:{\"x-quillo-prefetch\":\"1\"}}).catch(()=>{})};",
    "let t=0;const o={capture:true,passive:true};",
    "d.addEventListener(\"pointerover\",(e)=>{const a=link(e);clearTimeout(t);if(a)t=setTimeout(()=>go(a),65)},o);",
    "for(const v of[\"pointerdown\",\"touchstart\",\"focusin\"])d.addEventListener(v,(e)=>go(link(e)),o)})()"
);

/// With metrics on: a page landed on from a prefetch says so, as only it can tell. The page
/// that asked for it is told nothing, and a click answered from a prefetch never reaches the
/// server (FEEDBACK22.md, Q-181; branch feedback22-fixes).
fn used_script(base: &str) -> String {
    if !crate::metrics::ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        return String::new();
    }
    format!(
        "<script>addEventListener(\"load\",()=>{{const n=performance.getEntriesByType(\"navigation\")[0];if(n&&n.deliveryType===\"navigational-prefetch\")navigator.sendBeacon(\"{}/_quillo/prefetch-used\")}})</script>",
        base
    )
}

/// Whether a request is a prefetch: this page's (`PREFETCH_JS`) or the
/// browser's own (speculation rules, `<link rel=prefetch>`).
pub fn is_prefetch(req: &HttpRequest) -> bool {
    req.method == "GET"
        && PREFETCH.load(std::sync::atomic::Ordering::Relaxed)
        && req.headers.iter().any(|(k, v)| {
            (k.eq_ignore_ascii_case("x-quillo-prefetch") && v == "1") || (k.eq_ignore_ascii_case("sec-purpose") && v.contains("prefetch"))
        })
}

fn value_response(v: Value) -> HttpResponse {
    match v {
        Value::Response(r) => render_response(&r),
        other => {
            // So that no 500 is without a reason in the log (FEEDBACK21.md, Q-171; branch
            // feedback20-fixes).
            if let Value::Err(e, true, at) = &other {
                crate::httpd::error_line(&at.before(&e.display()));
            }
            let status = match &other {
                Value::Nil => 204,
                Value::Err(..) => 500,
                _ => 200,
            };
            render_response(&Response { status, headers: Vec::new(), body: Body::Value(other) })
        }
    }
}

/// Markup's type: XML when its root says it is in a namespace other than HTML's (a sitemap's
/// `<urlset xmlns=...>`, a feed), or it starts `<?xml`; SVG when its root is `<svg>`; else HTML
/// (FEEDBACK36.md, Q-451; branch feedback36-39).
fn markup_type(h: &str) -> &'static str {
    let t = h.trim_start();
    if t.starts_with("<?xml") {
        return "application/xml; charset=utf-8";
    }
    let Some(root) = t.strip_prefix('<') else { return "text/html; charset=utf-8" };
    let tag: String = root.chars().take_while(|c| c.is_alphanumeric() || *c == ':' || *c == '-').collect();
    let head = &root[..root.find('>').unwrap_or(root.len())];
    match tag.as_str() {
        "svg" if head.contains("xmlns=") => "image/svg+xml",
        "html" | "svg" | "" => "text/html; charset=utf-8",
        _ if head.contains("xmlns=") || head.contains("xmlns:") => {
            match tag.as_str() {
                "rss" => "application/rss+xml; charset=utf-8",
                "feed" => "application/atom+xml; charset=utf-8",
                _ => "application/xml; charset=utf-8",
            }
        }
        "rss" => "application/rss+xml; charset=utf-8",
        _ => "text/html; charset=utf-8",
    }
}

/// Body encoding follows the value: html -> text/html, str -> text/plain,
/// null -> empty, anything else -> JSON.
pub(crate) fn render_response(r: &Response) -> HttpResponse {
    let (ctype, body): (Option<&str>, Vec<u8>) = match &r.body {
        Body::Bytes(b) => (None, b.clone()),
        // Its function runs once the head is sent (see `Interp::handle`).
        Body::Stream { f, events } => {
            crate::stream::made(f.clone(), *events);
            let mut headers = r.headers.clone();
            headers.push((crate::stream::MARK.into(), "1".into()));
            return HttpResponse { status: r.status, headers, body: Vec::new() };
        }
        Body::Value(v) => match v {
            Value::Nil => (None, Vec::new()),
            Value::Html(h) => (Some(markup_type(h)), h.as_bytes().to_vec()),
            Value::Str(s) => (Some("text/plain; charset=utf-8"), s.as_bytes().to_vec()),
            other => {
                // An answer too large or too deep to write is the program's error, not the
                // server's end, said as it is written (Q-606, Q-592).
                let mut out = Vec::new();
                if let Err(why) = hidden(other).as_ref().unwrap_or(other).json_into_within(&mut out, crate::builtins::JSON_MOST, crate::builtins::JSON_DEEPEST) {
                    crate::httpd::error_line_with(&format!("the answer's value cannot be written as JSON: {}", why), &[]);
                    return HttpResponse { status: 500, headers: vec![("content-type".into(), "application/json".into())], body: br#"{"error":"internal error"}"#.to_vec() };
                }
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
    /// the others are signed with (see `cluster::announced`).
    pub cluster_key: Option<String>,
    /// Largest request body accepted, in bytes.
    pub max_body: usize,
    /// The path the program is served under (`--base`), kept when it is loaded again.
    pub base: String,
    /// Run as a service (`--as-service`): see `Interp::is_service`.
    pub is_service: bool,
    /// A desktop app (`quillo app`): only requests that carry this token
    /// are answered (see `launch_gate`).
    pub launch_token: Option<String>,
    /// `--reload`: load the program again when its files change, without
    /// `--watch`'s development aids (errors on pages, page refreshes).
    pub reload: bool,
    /// Where the server listens, once it does: for background work a
    /// version loaded later may need (see `start_background`).
    addr: Mutex<Option<std::net::SocketAddr>>,
    /// Stylesheets the program served that changed under `--watch`: how
    /// many times, and the paths they were asked for at the last time
    /// (FEEDBACK11.md, Q-078; branch feedback11-fixes).
    css: Mutex<(u64, Vec<String>)>,
}

impl App {
    /// `loaded` must have been loaded with `shared`; the caller holds the lock.
    pub fn new(loaded: Loaded, shared: Arc<Shared>, main_file: &Path, base_dir: &Path, watch: bool) -> Self {
        App {
            slot: Mutex::new((1, {
                let loaded = Arc::new(loaded);
                // Whether errors are hidden, as the server sets it when it starts: the development
                // bundle made now says it to the browser, under the debugger without `--watch` as
                // well (FEEDBACK55.md, Q-773; branch feedback55).
                crate::value::HIDING.store(!watch, std::sync::atomic::Ordering::Relaxed);
                warm_client(&loaded);
                loaded
            })),
            changed: Condvar::new(),
            shared,
            main_file: main_file.to_path_buf(),
            base_dir: base_dir.to_path_buf(),
            watch,
            static_check: true,
            cluster_key: None,
            max_body: MAX_BODY,
            base: String::new(),
            is_service: false,
            launch_token: None,
            reload: false,
            addr: Mutex::new(None),
            css: Mutex::new((0, Vec::new())),
        }
    }

    /// The pages swap these stylesheets, as they are.
    fn css_changed(&self, paths: Vec<String>) {
        eprintln!("quillo: {} changed: swapped in open pages", paths.join(", "));
        let mut c = self.css.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *c = (c.0 + 1, paths);
        drop(c);
        // Wakes the pages' connections (they wait on `changed`).
        let _g = self.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.changed.notify_all();
    }

    /// The pages refresh, the program as it is: a file it served changed.
    fn refresh_pages(&self, paths: Vec<String>) {
        eprintln!("quillo: {} changed: refreshing open pages", paths.join(", "));
        let mut slot = self.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = (slot.0 + 1, slot.1.clone());
        self.changed.notify_all();
    }

    fn css_seq(&self) -> (u64, Vec<String>) {
        self.css.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// What the program's interpreters share.
    pub fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
    }

    pub(crate) fn shared_ref(&self) -> &Shared {
        &self.shared
    }

    fn current(&self) -> (u64, Arc<Loaded>) {
        let g = self.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
        // Services are loaded again with it (into a table of its own: see
        // `shared::Services`).
        let mut it = Interp::with_shared(&self.base_dir, self.shared.clone());
        it.base = self.base.clone();
        it.is_service = self.is_service;
        // Its settings and files as they are now, a `.env` saved anew included (its files are,
        // as they load: FEEDBACK55.md, Q-701; branch feedback55).
        crate::builtins::protect_settings(&self.base_dir, None);
        let res = if problems.is_empty() {
            it.load_file(&self.main_file)
        } else {
            Err(crate::error::QuilloError::new(problems, 0))
        };
        self.shared.gil.release();
        let mut slot = self.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match res {
            Ok(_) => {
                eprintln!("quillo: reloaded {} ({} routes)", self.main_file.display(), it.routes.iter().filter(|r| r.method != crate::ast::GUARD_METHOD).count());
                // A version that is the first to use services or `db.atomic`
                // needs what delivers and recovers in the background.
                let addr = *self.addr.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(addr) = addr {
                    start_background(&it, addr);
                }
                // The version replaced: its jobs and ticks still running end
                // at their next `sleep` (FEEDBACK12.md, Q-082; branch feedback12-fixes).
                if let Loaded::Ready(old) = &*slot.1 {
                    retire(old);
                }
                let loaded = Arc::new(Loaded::Ready(it));
                warm_client(&loaded);
                *slot = (slot.0 + 1, loaded);
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

    /// Waits up to `timeout` for a version newer than `seen`, or a
    /// stylesheet changed after `css_seen`:
    /// the version, and the stylesheets' paths when they changed.
    fn wait_for_change_or_css(&self, seen: u64, css_seen: u64, timeout: Duration) -> (u64, Option<(u64, Vec<String>)>) {
        let slot = self.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (slot, _) = self
            .changed
            .wait_timeout_while(slot, timeout, |s| s.0 == seen && self.css.lock().map_or(true, |c| c.0 == css_seen))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let v = slot.0;
        drop(slot);
        let css = self.css_seq();
        (v, (css.0 != css_seen).then_some(css))
    }

    fn watched_files(&self) -> Vec<PathBuf> {
        let mut files = vec![self.main_file.clone()];
        if let Loaded::Ready(it) = &*self.current().1 {
            files.extend(it.files());
        }
        files
    }
}

/// Each file's time, and where it leads: a link pointed elsewhere is a change (FEEDBACK31.md,
/// Q-383; branch feedback27-35-fixes).
fn mtimes(files: &[PathBuf]) -> Vec<(Option<std::time::SystemTime>, Option<PathBuf>)> {
    files.iter().map(|f| (std::fs::metadata(f).and_then(|m| m.modified()).ok(), f.canonicalize().ok())).collect()
}

/// Polls the program's files and reloads it when one changes. Under
/// `--watch`, also the files it served (a stylesheet another tool writes,
/// such as Tailwind's): a stylesheet is swapped in the open pages, and
/// anything else refreshes them (FEEDBACK11.md, Q-078; branch
/// feedback11-fixes).
/// Where the system tells of changes to files, the watcher waits for them
/// instead of looking every 50 ms: kqueue on macOS, inotify on Linux.
/// Elsewhere, and when the system refuses (a file missing, too many open),
/// it looks every 50 ms as before. Under events it still looks once a
/// second, for a filesystem that tells nothing (a network mount)
/// (FEEDBACK15.md, Q-099; branch file-events).
mod events {
    use std::path::PathBuf;
    use std::time::Duration;

    #[cfg(target_os = "macos")]
    pub struct Events {
        kq: i32,
        fds: Vec<i32>,
    }

    #[cfg(target_os = "macos")]
    impl Events {
        /// Watches `files`, or none when one cannot be watched.
        pub fn open(files: &[PathBuf]) -> Result<Events, String> {
            let kq = unsafe { libc::kqueue() };
            if kq < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let mut ev = Events { kq, fds: Vec::with_capacity(files.len()) };
            for f in files {
                let c = std::ffi::CString::new(f.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())?;
                let fd = unsafe { libc::open(c.as_ptr(), libc::O_EVTONLY | libc::O_CLOEXEC) };
                if fd < 0 {
                    return Err(format!("{}: {}", f.display(), std::io::Error::last_os_error()));
                }
                ev.fds.push(fd);
                let k = libc::kevent {
                    ident: fd as libc::uintptr_t,
                    filter: libc::EVFILT_VNODE,
                    flags: libc::EV_ADD | libc::EV_CLEAR,
                    fflags: libc::NOTE_WRITE | libc::NOTE_EXTEND | libc::NOTE_DELETE | libc::NOTE_RENAME | libc::NOTE_ATTRIB,
                    data: 0,
                    udata: std::ptr::null_mut(),
                };
                if unsafe { libc::kevent(kq, &k, 1, std::ptr::null_mut(), 0, std::ptr::null()) } < 0 {
                    return Err(format!("{}: {}", f.display(), std::io::Error::last_os_error()));
                }
            }
            Ok(ev)
        }

        /// Waits up to `timeout` for a change to one of the files.
        pub fn wait(&self, timeout: Duration) {
            let ts = libc::timespec { tv_sec: timeout.as_secs() as libc::time_t, tv_nsec: timeout.subsec_nanos() as libc::c_long };
            let mut out: [libc::kevent; 32] = unsafe { std::mem::zeroed() };
            unsafe { libc::kevent(self.kq, std::ptr::null(), 0, out.as_mut_ptr(), out.len() as libc::c_int, &ts) };
        }
    }

    #[cfg(target_os = "macos")]
    impl Drop for Events {
        fn drop(&mut self) {
            for fd in &self.fds {
                unsafe { libc::close(*fd) };
            }
            unsafe { libc::close(self.kq) };
        }
    }

    #[cfg(target_os = "linux")]
    pub struct Events {
        fd: i32,
        /// What is watched: each file's folder, and the file's name in it (an
        /// editor's save is often a new file renamed over the old one, which
        /// a watch on the file itself would lose).
        names: std::collections::HashSet<(i32, Vec<u8>)>,
    }

    #[cfg(target_os = "linux")]
    impl Events {
        pub fn open(files: &[PathBuf]) -> Result<Events, String> {
            let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let mut ev = Events { fd, names: Default::default() };
            let mut dirs: std::collections::HashMap<PathBuf, i32> = Default::default();
            for f in files {
                let dir = f.parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
                let wd = match dirs.get(&dir) {
                    Some(wd) => *wd,
                    None => {
                        let c = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())?;
                        let mask = libc::IN_MODIFY | libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_CREATE | libc::IN_DELETE | libc::IN_ATTRIB | libc::IN_DELETE_SELF | libc::IN_MOVE_SELF;
                        let wd = unsafe { libc::inotify_add_watch(fd, c.as_ptr(), mask) };
                        if wd < 0 {
                            return Err(format!("{}: {}", dir.display(), std::io::Error::last_os_error()));
                        }
                        dirs.insert(dir, wd);
                        wd
                    }
                };
                if let Some(name) = f.file_name() {
                    ev.names.insert((wd, name.as_encoded_bytes().to_vec()));
                }
            }
            Ok(ev)
        }

        /// Waits up to `timeout` for a change to one of the files (a change
        /// to another file in one of their folders wakes it too, and is then
        /// found to have changed nothing).
        pub fn wait(&self, timeout: Duration) {
            let mut p = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
            let n = unsafe { libc::poll(&mut p, 1, timeout.as_millis() as libc::c_int) };
            if n <= 0 {
                return;
            }
            // Drain what came, so the next wait is for what comes after.
            let mut buf = [0u8; 8192];
            loop {
                let got = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
                if got <= 0 {
                    break;
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for Events {
        fn drop(&mut self) {
            unsafe { libc::close(self.fd) };
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub struct Events;

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    impl Events {
        pub fn open(_files: &[PathBuf]) -> Result<Events, String> {
            Err("not on this system".into())
        }
        pub fn wait(&self, _timeout: Duration) {}
    }
}

/// The program's files and their times, read now, on the caller's thread, then the loop that
/// watches them. The server says it is listening only after this, so a file saved after that is
/// a change however late the watcher's own thread starts: read on that thread, a save made
/// before it ran was taken for the files as they were, and never loaded (a race CI found in
/// no_check_reloads_what_loads, after FEEDBACK15.md, Q-098; branch main).
pub(crate) fn watcher(app: Arc<App>) -> impl FnOnce() + Send + 'static {
    let files = app.watched_files();
    let seen = mtimes(&files);
    move || watch(app, files, seen)
}

fn watch(app: Arc<App>, mut files: Vec<PathBuf>, mut seen: Vec<(Option<std::time::SystemTime>, Option<PathBuf>)>) {
    let mut served: std::collections::HashMap<PathBuf, Option<std::time::SystemTime>> = Default::default();
    // Told of changes by the system where it can be (see `events`); else a
    // stat of each file every 50 ms, cheap for the hundreds a program has
    // (FEEDBACK15.md, Q-098 and Q-099; branches dev-maps-once, file-events).
    let mut said = false;
    let open = |files: &[PathBuf], served: &std::collections::HashMap<PathBuf, Option<std::time::SystemTime>>, said: &mut bool| {
        let all: Vec<PathBuf> = files.iter().cloned().chain(served.keys().cloned()).collect();
        match events::Events::open(&all) {
            Ok(ev) => Some(ev),
            Err(why) => {
                if !*said && cfg!(any(target_os = "macos", target_os = "linux")) {
                    eprintln!("quillo: looking for changed files every 50 ms (the system cannot tell of them: {})", why);
                    *said = true;
                }
                None
            }
        }
    };
    let mut events = open(&files, &served, &mut said);
    loop {
        match &events {
            Some(ev) => ev.wait(Duration::from_secs(1)),
            None => std::thread::sleep(Duration::from_millis(50)),
        }
        let now = mtimes(&files);
        if now != seen {
            // Editors often write in several steps; let them finish.
            std::thread::sleep(Duration::from_millis(30));
            app.reload();
            files = app.watched_files();
            seen = mtimes(&files);
            // A save that renamed a new file over the old one left the
            // old one watched: watch the files as they are now.
            events = open(&files, &served, &mut said);
            continue;
        }
        if !app.watch {
            continue;
        }
        let (mut css, mut other) = (Vec::new(), Vec::new());
        let mut grew = false;
        for (file, path) in crate::statics::served_files() {
            if files.contains(&file) {
                continue;
            }
            let m = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
            match served.insert(file.clone(), m) {
                Some(before) if before != m => {
                    if file.extension().is_some_and(|x| x.eq_ignore_ascii_case("css")) {
                        css.push(path);
                    } else {
                        other.push(path);
                    }
                }
                None => grew = true,
                _ => {}
            }
        }
        if events.is_some() && (grew || !css.is_empty() || !other.is_empty()) {
            events = open(&files, &served, &mut said);
        }
        if !other.is_empty() {
            app.refresh_pages(other);
        } else if !css.is_empty() {
            app.css_changed(css);
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
///
/// A message `css:["/assets/app.css"]` is a stylesheet the program served
/// that changed: the page swaps each `<link>` to it for one asked for
/// afresh, keeping its state (FEEDBACK11.md, Q-078; branch
/// feedback11-fixes).
fn dev_script(version: u64) -> String {
    format!(
        "<script>(()=>{{const v='{}';let c=0;const css=j=>{{const p=JSON.parse(j);for(const l of document.querySelectorAll('link[rel=stylesheet]')){{const u=new URL(l.href,location.href);if(u.origin===location.origin&&p.includes(u.pathname)){{u.searchParams.set('quillo',Date.now());const n=l.cloneNode();n.href=u.pathname+u.search;n.onload=()=>l.remove();l.after(n)}}}}}};const seen=t=>{{if(t.startsWith('css:')){{const i=t.indexOf(':',4);c=+t.slice(4,i)||c;css(t.slice(i+1))}}else if(t!==v)location.reload()}};const u=()=>'/_quillo/events?v='+v+'&c='+c,go=()=>{{let o=0;const s=new WebSocket(location.origin.replace(/^http/,'ws')+u());s.onopen=()=>o=1;s.onmessage=e=>seen(e.data);s.onclose=()=>o?setTimeout(go,1000):fetch(u(),{{cache:'no-store'}}).then(r=>r.text()).then(t=>{{seen(t);go()}},()=>setTimeout(go,1000))}};go()}})()</script>",
        version
    )
}

/// Answers `/_quillo/events[?v=N]` on a thread of its own, so pages waiting
/// for a change do not hold on to the server's threads.
fn serve_events(app: Arc<App>, req: HttpRequest) -> Answer {
    let q = parse_urlencoded(req.url.split_once('?').map_or("", |(_, q)| q));
    let seen = q.get("v").and_then(|v| v.display().parse::<u64>().ok());
    // The stylesheet changes the page has taken (`css:N:[...]` messages);
    // none given: the ones so far.
    let css_seen = q.get("c").and_then(|v| v.display().parse::<u64>().ok()).filter(|c| *c > 0).unwrap_or_else(|| app.css_seq().0);
    let css_text = |(n, paths): (u64, Vec<String>)| format!("css:{}:{}", n, serde_json::json!(paths));
    waiter("quillo-events", &req.remote.clone(), move |stream| {
        if crate::ws::is_upgrade(&req) {
            let Some(mut socket) = crate::ws::Socket::accept(&req, stream) else { return };
            // A page that does not read is let go, not waited on for good (FEEDBACK49.md,
            // Q-594; found in review).
            socket.write_timeout(Duration::from_secs(30));
            let seen = seen.unwrap_or(0);
            let mut css_seen = css_seen;
            loop {
                let (v, css) = app.wait_for_change_or_css(seen, css_seen, Duration::from_secs(20));
                if v != seen {
                    socket.send(&v.to_string());
                    return socket.close();
                }
                if let Some(c) = css {
                    css_seen = c.0;
                    if !socket.send(&css_text(c)) {
                        return;
                    }
                    continue;
                }
                if !socket.ping() {
                    return;
                }
            }
        }
        let (v, css) = match seen {
            Some(seen) => app.wait_for_change_or_css(seen, css_seen, Duration::from_secs(25)),
            None => (app.current().0, None),
        };
        let text = match css {
            Some(c) if Some(v) == seen => css_text(c),
            _ => v.to_string(),
        };
        let mut resp = plain(200, &text);
        resp.headers.push(("cache-control".into(), "no-store".into()));
        send_on(stream, resp);
    })
}

/// In watch mode, HTML pages get a script that reloads them after a change.
/// An answer whose bytes are named by a validator or a range (`file()`'s, or a route's own
/// `etag`): sent as it is, nothing added to it (FEEDBACK36.md, Q-447; branch feedback36-39).
fn as_written(resp: &HttpResponse) -> bool {
    // A header named `null` is none (FEEDBACK55.md, Q-710; branch feedback55).
    resp.status == 206 || resp.sent("etag").is_some() || resp.sent("content-range").is_some()
}

fn inject_dev_script(resp: &mut HttpResponse, version: u64) {
    let html = resp.header("content-type").map_or(false, |c| c.starts_with("text/html"));
    if !html || as_written(resp) {
        return;
    }
    let body = String::from_utf8_lossy(&resp.body).into_owned();
    let script = dev_script(version);
    resp.body = match body.rfind("</body>") {
        Some(i) => format!("{}{}{}", &body[..i], script, &body[i..]),
        None => format!("{}{}", body, script),
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
/// The value of a header the program named `null` (`respond`, `stream`, `events`): it holds the
/// name's place, so that Quillo adds none of its own for it, and is not written (FEEDBACK55.md,
/// Q-710; branch feedback55).
pub const DROPPED: &str = "\0";

pub fn valid_header(name: &str, value: &str) -> bool {
    let token = |c: u8| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c);
    !name.is_empty()
        && name.bytes().all(token)
        && value.bytes().all(|c| c == b'\t' || (c >= 0x20 && c != 0x7f))
}

/// Whether the request is a multipart form of more parts than are read (FEEDBACK55.md, Q-716;
/// branch feedback55).
fn too_many_parts(req: &HttpRequest) -> bool {
    if req.body.is_empty() {
        return false;
    }
    let ctype = req.headers.iter().rev().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map_or("", |(_, v)| v.as_str());
    crate::upload::boundary(ctype).is_some_and(|b| crate::upload::too_many_parts(&b, &req.body))
}

/// Runs a request in the program (the caller holds the lock). A panic (a
/// bug in Quillo itself) answers 500 instead of stopping the server.
fn run_program(it: &mut Interp, req: &HttpRequest) -> Option<HttpResponse> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| it.handle(req))).ok()
}

/// Gzips text responses of 1 KiB or more for clients that accept it.
/// Whether `Accept-Encoding` takes gzip, read as RFC 9110 §12.5.3 says: names in any case,
/// `x-gzip` the same, `q=0` refusing, and `*` standing for gzip when gzip is not named
/// (FEEDBACK36.md, Q-439; branch feedback36-39).
pub(crate) fn accepts_gzip(req: &HttpRequest) -> bool {
    let mut star = None;
    for (_, v) in req.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("accept-encoding")) {
        for item in v.split(',') {
            let mut parts = item.split(';');
            let name = parts.next().unwrap_or("").trim().to_ascii_lowercase();
            let q = parts.find_map(|p| {
                let (k, v) = p.split_once('=')?;
                k.trim().eq_ignore_ascii_case("q").then(|| v.trim().parse::<f64>().unwrap_or(0.0))
            });
            let wanted = q.map_or(true, |q| q > 0.0);
            match name.as_str() {
                "gzip" | "x-gzip" => return wanted,
                "*" => star = Some(wanted),
                _ => {}
            }
        }
    }
    star.unwrap_or(false)
}

fn compress(req: &HttpRequest, resp: &mut HttpResponse) {
    use std::io::Write;
    // A part of a file stays as it is: its byte range is of the file.
    if resp.status != 200 || resp.body.len() < 1024 || resp.header("content-encoding").is_some() {
        return;
    }
    let ctype = resp.header("content-type").unwrap_or("");
    let text = ctype.starts_with("text/") || ctype.contains("json") || ctype.contains("javascript") || ctype.contains("svg");
    if !text {
        return;
    }
    // The copy not compressed varies by the header too: a shared cache that kept it from a
    // request without `Accept-Encoding` gave it to everyone (FEEDBACK55.md, Q-724; branch
    // feedback55).
    if !accepts_gzip(req) {
        vary_by_encoding(resp);
        return;
    }
    // A file's gzipped copy is kept (by its ETag) rather than made again.
    let etag = resp.sent("etag").map(str::to_string);
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
    vary_by_encoding(resp);
}

/// `vary: accept-encoding`, unless a `vary` says it already, or is `*`, or the program said
/// `vary: null` (FEEDBACK55.md, Q-710 and Q-724; branch feedback55).
fn vary_by_encoding(resp: &mut HttpResponse) {
    if !resp.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("vary") && (v == DROPPED || v.trim() == "*" || v.to_ascii_lowercase().contains("accept-encoding"))) {
        resp.headers.push(("vary".into(), "accept-encoding".into()));
    }
}

/// `--allowed-hosts`: the names a server under `--watch` answers besides `localhost`
/// (`*.localhost`) and addresses; one starting with `.` takes its subdomains too.
pub static ALLOWED_HOSTS: OnceLock<Vec<String>> = OnceLock::new();

/// Whether the request's `Host` is one a server under `--watch` answers (see `ALLOWED_HOSTS`).
fn local_host(req: &HttpRequest) -> bool {
    let Some((_, host)) = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("host")) else { return true };
    let host = host.trim().to_ascii_lowercase();
    // Without the port: `[::1]:8080`, `localhost:8080`.
    let name = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => host.rsplit_once(':').filter(|(_, p)| p.bytes().all(|b| b.is_ascii_digit())).map_or(host.as_str(), |(h, _)| h),
    };
    let name = name.trim_end_matches('.');
    name.parse::<std::net::IpAddr>().is_ok()
        || name == "localhost"
        || name.ends_with(".localhost")
        || ALLOWED_HOSTS.get().is_some_and(|all| all.iter().any(|a| name == a || a.starts_with('.') && (name.ends_with(a.as_str()) || name == &a[1..])))
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

    /// Whether it holds a version of the program no longer current (and,
    /// idle, should go, so that the old version's database connections close).
    pub(crate) fn outdated(&self, app: &App) -> bool {
        !Arc::ptr_eq(&self.program, &app.current().1)
    }

    /// Answers a request, a streamed answer on `sink` (the connection) as it
    /// is made. The caller holds the interpreter lock.
    pub(crate) fn answer(&mut self, app: &Arc<App>, req: HttpRequest, sink: Option<&mut dyn crate::stream::Sink>) -> Answer {
        if let Some(token) = &app.launch_token {
            if let Some(resp) = launch_gate(&req, token).filter(|_| !is_job_request(&req)) {
                return Answer::Respond(resp);
            }
        }
        // Under --watch, a developer's server: a request named for another host is a page
        // elsewhere that had its name point here (DNS rebinding), and called the server fns
        // and `/_quillo/dev` with the developer's data (FEEDBACK51.md, Q-663; branch
        // feedback51-fixes).
        if app.watch && !local_host(&req) && !is_job_request(&req) {
            return Answer::Respond(plain(421, "this server, run with --watch, answers localhost and addresses; name another host with --allowed-hosts\n"));
        }
        let path = req.url.split('?').next().unwrap_or("");
        // A WebSocket is not kept to its site by the browser: a page on
        // another site could open one here and hear the topics changing.
        // Only the program's own pages connect, as only they call server fns.
        let waits = (app.watch && path == "/_quillo/events") || path.ends_with("/_quillo/live") || path.ends_with("/_quillo/wt");
        if waits && cross_site(&req) {
            return Answer::Respond(plain(403, "only the program's own pages connect\n"));
        }
        if app.watch && path == "/_quillo/events" {
            return serve_events(app.clone(), req);
        }
        if path.ends_with("/_quillo/prefetch-used") && crate::metrics::ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
            crate::metrics::prefetch_used();
            return Answer::Respond(plain(204, ""));
        }
        if path.ends_with("/_quillo/live") {
            let tenant = live_tenant(app, &req, path);
            return serve_live(app.shared.clone(), req, tenant);
        }
        if path.ends_with("/_quillo/wt") && crate::ws::is_upgrade(&req) {
            return serve_channels(req);
        }
        if let (Some(key), "/_quillo/announce") = (&app.cluster_key, req.url.as_str()) {
            let body = &req.body[..req.body.len().min(1 << 16)];
            let status = crate::cluster::announced(&app.shared, key, body);
            return Answer::Respond(HttpResponse { status, headers: Vec::new(), body: Vec::new() });
        }
        if path == "/_quillo/metrics" && crate::metrics::ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
            let mut r = plain(200, &crate::metrics::render());
            r.headers = vec![("content-type".into(), "text/plain; version=0.0.4".into())];
            return Answer::Respond(r);
        }
        // How long queries take (src/querystats.rs; branch query-stats): the figures
        // for anyone while on; each statement, and switching on or off, with the
        // health token (under --watch, from this machine too).
        if path == "/_quillo/health/queries" {
            let auth = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("authorization")).map(|(_, v)| v.as_str());
            let local = app.watch && (req.remote == "127.0.0.1" || req.remote == "::1");
            let trusted = crate::querystats::has_token(auth) || local;
            if req.method == "POST" {
                if !trusted {
                    return Answer::Respond(plain(403, "send the health token (QUILLO_HEALTH_TOKEN) as Authorization: Bearer ...\n"));
                }
                let q = req.url.split_once('?').map_or("", |(_, q)| q);
                let on = !q.split('&').any(|kv| kv == "on=0" || kv == "on=false" || kv == "off");
                crate::querystats::set(on);
            }
            let mut r = plain(200, &serde_json::to_string_pretty(&crate::querystats::report(trusted)).unwrap_or_default());
            r.headers = vec![("content-type".into(), "application/json".into()), ("cache-control".into(), "no-store".into())];
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
        // The request's id: given (another service calling, a proxy), or made.
        let rid = crate::service::request_id(req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("x-request-id")).map(|(_, v)| v.as_str()));
        crate::httpd::request_began(format!("{} {} (rid {})", req.method, log_url(&req.url), rid));
        let prefetch = is_prefetch(&req);
        let mut resp = if app.watch && req.url == "/_quillo/dev" {
            plain(200, &self.generation.to_string())
        } else if prefetch && crate::httpd::others_waiting() {
            // A guess is work nobody has asked for yet, so it does not go before anyone. The
            // browser drops the 503, and a click is made as usual (FEEDBACK22.md, Q-179;
            // branch feedback22-fixes).
            HttpResponse { status: 503, headers: vec![("cache-control".into(), "no-store".into())], body: Vec::new() }
        } else if too_many_parts(&req) {
            // As a body past the limit is, before the route runs: the parts past it were dropped
            // and the answer was a 200 (FEEDBACK55.md, Q-716; branch feedback55).
            plain(413, &format!("a form of more than {} parts\n", crate::upload::MAX_PARTS))
        } else {
            match (&mut self.local, &*self.program) {
                (Some(it), _) => match {
                    it.streams_direct = sink.is_some();
                    it.request_id = Some(rid.clone());
                    run_program(it, &req)
                } {
                    Some(r) => r,
                    // A stop cuts off a request still running past the drain, a plugin's call
                    // say: that is no bug (FEEDBACK27.md, Q-330; branch feedback27-35-fixes).
                    None if crate::httpd::stopping() => {
                        eprintln!("quillo: stopped with {} {} still running", req.method, log_url(&req.url));
                        self.local = None;
                        self.generation = 0;
                        plain(503, "stopping\n")
                    }
                    None => {
                        eprintln!("quillo: internal error (a bug in Quillo) while handling {} {}", req.method, log_url(&req.url));
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
        crate::httpd::request_done();
        if prefetch && crate::metrics::ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
            crate::metrics::prefetched(resp.status);
        }
        if !resp.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("x-request-id")) {
            resp.headers.push(("x-request-id".into(), rid.to_string()));
        }
        // Streamed: the head now, then what the function sends.
        let mut streamed = None;
        if let Some(i) = resp.headers.iter().position(|(k, _)| k.eq_ignore_ascii_case(crate::stream::MARK)) {
            resp.headers.remove(i);
            let pending = self.local.as_mut().and_then(|it| it.pending_stream.take());
            match (sink, pending, self.local.as_mut()) {
                (Some(sink), Some(mut p), Some(it)) => {
                    // A deferred page keeps the development script (branch defer).
                    if let (true, Some(page)) = (app.watch, p.page.as_mut()) {
                        let script = dev_script(self.generation);
                        page.tail.splice(0..0, script.into_bytes());
                    }
                    // Unless the program sent its own, or none (FEEDBACK55.md, Q-710; branch
                    // feedback55).
                    if resp.header("x-content-type-options").is_none() {
                        resp.headers.push(("x-content-type-options".into(), "nosniff".into()));
                    }
                    let chunked = sink.chunked();
                    let close = if !sink.start(&resp, chunked) {
                        true
                    } else if req.method == "HEAD" || crate::stream::bodyless(resp.status as i64) {
                        // A head that says there is no body has none after it (FEEDBACK55.md,
                        // Q-709; branch feedback55).
                        !chunked
                    } else {
                        // Unchunked, the end of the answer is the end of the connection.
                        !matches!(crate::stream::run(it, p, sink, chunked), crate::stream::Ended::Done) || !chunked
                    };
                    streamed = Some(close);
                }
                _ => resp = plain(500, "internal error\n"),
            }
        }
        if app.watch && streamed.is_none() {
            inject_dev_script(&mut resp, self.generation);
        }
        // Chrome asks every site for this while DevTools is open; it is not
        // the program's business, so it is not logged.
        // Nor are the batches of UDP packets handed to the program: they are
        // counted as packets (see `udp`), not requests.
        // Nor the scheduler's own call each second, asking that calls made
        // `later` be sent (only it has the token: others get a 404, logged)
        // (FEEDBACK4.md, Q-043; branch bank-fixes).
        let took = start.elapsed().as_secs_f64();
        // Nor ticks, which may come 60 times a second: one that fails is
        // logged (FEEDBACK12.md, Q-080; branch feedback12-fixes).
        let delivery = req.url.starts_with("/_quillo/udp/") || req.url.starts_with("/_quillo/channel/")
            || (req.url == "/_quillo/later" && resp.status == 200)
            || (req.url.starts_with("/_quillo/tick/") && resp.status == 200)
            // Channels between the app and a service apart, as channel
            // messages are (FEEDBACK12.md, Q-087; branch open-fixes); a refusal is logged.
            || ((req.url.starts_with("/_quillo/service-channel") || req.url == "/_quillo/channel-relay") && resp.status == 200);
        if !delivery {
            crate::metrics::answered(resp.status, took);
        }
        if req.url != "/.well-known/appspecific/com.chrome.devtools.json" && !delivery {
            match crate::metrics::LOG.load(std::sync::atomic::Ordering::Relaxed) {
                crate::metrics::TEXT => {
                    // A page nobody has seen, marked so that what counts the log does not
                    // count it (FEEDBACK22.md, Q-181; branch feedback22-fixes).
                    let mark = if prefetch { " prefetch" } else { "" };
                    crate::httpd::log_fmt(format_args!("{} {} -> {} ({:.1}ms){}\n", req.method, log_url(&req.url), resp.status, took * 1000.0, mark))
                }
                crate::metrics::JSON => {
                    let tenant = match self.local.as_ref().and_then(|it| it.tenant.clone().flatten()) {
                        Some(t) => format!(",\"tenant\":{}", serde_json::Value::from(&*t)),
                        None => String::new(),
                    };
                    crate::httpd::log_fmt(format_args!(
                        "{{\"time\":\"{}\",\"method\":{},\"path\":{},\"status\":{},\"ms\":{:.2},\"ip\":{}{}{},\"rid\":{}}}\n",
                        crate::httpd::iso_now(),
                        serde_json::Value::from(req.method.as_str()),
                        serde_json::Value::from(&*log_url(&req.url)),
                        resp.status,
                        took * 1000.0,
                        serde_json::Value::from(req.remote.as_str()),
                        tenant,
                        if prefetch { ",\"prefetch\":true" } else { "" },
                        serde_json::Value::from(&*rid)
                    ))
                }
                _ => {}
            }
        }
        if let Some(close) = streamed {
            return Answer::Streamed { close };
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
        not_modified(&req, &mut resp);
        compress(&req, &mut resp);
        Answer::Respond(resp)
    }
}

/// A route's own `etag` that the browser already holds: a 304, as `file()` and `quillo.js`
/// give, rather than the whole answer again (FEEDBACK36.md, Q-441; branch feedback36-39).
fn not_modified(req: &HttpRequest, resp: &mut HttpResponse) {
    if resp.status != 200 || !matches!(req.method.as_str(), "GET" | "HEAD") {
        return;
    }
    let (Some(etag), Some(asked)) = (resp.sent("etag"), req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("if-none-match"))) else { return };
    if crate::statics::matches(&asked.1, etag) {
        resp.status = 304;
        resp.body.clear();
    }
}

pub(crate) fn plain_response(status: u16, msg: &str) -> HttpResponse {
    plain(status, msg)
}

/// Serves the app. `workers` is how many requests may wait (for the
/// database, the network) at once; each waiting one has a thread. The
/// calling thread must hold the interpreter lock; it gives it up here.
/// Takes all the file descriptors the system allows: each page holds two, a
/// socket for live data and a connection, and past the soft limit, often 1024,
/// new visitors hang while those here carry on (FEEDBACK24.md, Q-195; branch
/// feedback24-fixes). A limit still too low for the pages allowed is said.
#[cfg(unix)]
fn take_descriptors() {
    let mut r = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: `r` is a live rlimit for the call to write and read.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut r) } != 0 {
        return;
    }
    // macOS refuses more than OPEN_MAX whatever the hard limit says.
    let most = if cfg!(target_os = "macos") { r.rlim_max.min(10_240) } else { r.rlim_max };
    if r.rlim_cur < most {
        let raised = libc::rlimit { rlim_cur: most, rlim_max: r.rlim_max };
        // SAFETY: as above.
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raised) } == 0 {
            r.rlim_cur = most;
        }
    }
    // Room left for the files, databases and services the program opens, and for the
    // connections handed on, which are not counted then: pages waiting for live data and
    // large files being sent (256 at once) (Q-672; found in review).
    if crate::httpd::MAX_CONNECTIONS.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        let kept = 256 + MAX_WAITERS + 256;
        crate::httpd::MAX_CONNECTIONS.store((r.rlim_cur as usize).saturating_sub(kept).max(64), std::sync::atomic::Ordering::SeqCst);
    }
    if (r.rlim_cur as usize) < 2 * MAX_WAITERS {
        eprintln!("quillo: {} file descriptors allowed, about {} pages with live data: raise `ulimit -n` for more", r.rlim_cur, r.rlim_cur / 2);
    }
}

/// On Linux a process limit counts threads, for the whole account on a shared server (CloudLinux's
/// NPROC): a limit that leaves less than the server's own threads is said at start (FEEDBACK41.md,
/// Q-480; branch feedback40-43).
#[cfg(unix)]
fn threads_allowed(workers: usize) {
    let mut r = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: `r` is a live rlimit for the call to write.
    if unsafe { libc::getrlimit(libc::RLIMIT_NPROC, &mut r) } != 0 || r.rlim_cur == libc::RLIM_INFINITY {
        return;
    }
    // Its event loop, the watch, accept and job threads, and a wait thread for each worker.
    let needs = workers + 1 + 8;
    if (r.rlim_cur as usize) < needs {
        eprintln!(
            "quillo: the process limit (ulimit -u) is {}, and this server may use {} threads: requests may wait for a thread; raise it, or give --workers fewer",
            r.rlim_cur, needs
        );
    }
}

/// `; the process limit (ulimit -u) is N ...` where the system sets one, for a message about a
/// thread that could not be made.
pub fn process_limit_note() -> String {
    #[cfg(unix)]
    {
        let mut r = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: `r` is a live rlimit for the call to write.
        if unsafe { libc::getrlimit(libc::RLIMIT_NPROC, &mut r) } == 0 && r.rlim_cur != libc::RLIM_INFINITY {
            return format!("; the process limit (ulimit -u) is {}, which counts every thread of this account's processes: raise it", r.rlim_cur);
        }
    }
    String::new()
}

#[cfg(not(unix))]
fn take_descriptors() {}

#[cfg(not(unix))]
fn threads_allowed(_: usize) {}

pub fn serve(app: App, addr: &str, workers: usize) -> Result<(), String> {
    serve_with(app, addr, workers, |_| {})
}

/// `serve`, calling `listening` with the address once it is bound (with
/// port 0, the port the system chose).
pub fn serve_with(app: App, addr: &str, workers: usize, listening: impl FnOnce(std::net::SocketAddr)) -> Result<(), String> {
    take_descriptors();
    let listener = std::net::TcpListener::bind(addr).map_err(|e| format!("cannot listen on {}: {}", addr, e))?;
    let bound = listener.local_addr().ok();
    if let Some(b) = bound {
        crate::platform::set_listening(&b.ip().to_string(), b.port());
    }
    let shown = format!("http://{}", bound.map_or_else(|| addr.to_string(), |a| a.to_string()));
    serve_from(app, crate::httpd::Listener::Tcp(listener), bound, &shown, workers, listening)
}

/// `serve` on a Unix socket at `path`, made for this process's user alone (`mode`, 0600 unless
/// said), and gone when the server stops: on a shared server a TCP port on 127.0.0.1 is every
/// account's, a socket in a folder of one's own is not (FEEDBACK41.md, Q-479; branch
/// feedback40-43).
#[cfg(unix)]
pub fn serve_unix(app: App, path: &Path, mode: u32, workers: usize) -> Result<(), String> {
    take_descriptors();
    let listener = unix_listener(path, mode)?;
    let mine = file_id(path);
    crate::jobs::listening_at(path);
    let shown = format!("unix:{}", path.display());
    // Its jobs and own calls reach it through the socket; the address stands for it.
    let stand_in: std::net::SocketAddr = ([127, 0, 0, 1], 0).into();
    let r = serve_from(app, crate::httpd::Listener::Other(crate::httpd::unix_connections(listener)), Some(stand_in), &shown, workers, |_| {});
    remove_if_mine(path, mine);
    r
}

/// The device and inode of `path`: which file it is, whatever its name.
#[cfg(unix)]
fn file_id(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// Removes the socket this server made, and not one a server started since has made there.
#[cfg(unix)]
fn remove_if_mine(path: &Path, mine: Option<(u64, u64)>) {
    if mine.is_some() && file_id(path) == mine {
        let _ = std::fs::remove_file(path);
    }
}

/// A FastCGI socket at `path`, for `quillo fcgi --listen unix:PATH`.
#[cfg(unix)]
pub fn fcgi_unix(path: &Path, mode: u32) -> Result<crate::fcgi::Source, String> {
    unix_listener(path, mode).map(crate::fcgi::Source::Unix)
}

/// `quillo fcgi`: the program behind a FastCGI process manager. It answers no HTTP of its own;
/// its jobs and its own calls reach it through a socket only this process knows of.
#[cfg(unix)]
pub fn serve_fcgi(app: App, source: crate::fcgi::Source, made: Option<&Path>, workers: usize) -> Result<(), String> {
    take_descriptors();
    let shown = match &source {
        crate::fcgi::Source::Unix(_) => format!("FastCGI{}", made.map_or(String::new(), |p| format!(" at unix:{}", p.display()))),
        crate::fcgi::Source::Tcp(l) => format!("FastCGI at {}", l.local_addr().map_or("?".to_string(), |a| a.to_string())),
    };
    // Its own calls: a private socket beside its connections, in a folder only it can enter, made
    // new under a name no one can guess (one already there is someone else's, and refused).
    use std::os::unix::fs::DirBuilderExt;
    let dir = std::env::temp_dir().join(format!("quillo-fcgi-{}", crate::service::new_id(16)));
    std::fs::DirBuilder::new().mode(0o700).create(&dir).map_err(|e| format!("cannot make the private folder {}: {}", dir.display(), e))?;
    let own = dir.join("self.sock");
    let own_listener = unix_listener(&own, 0o600)?;
    crate::jobs::listening_at(&own);
    let made_id = made.and_then(file_id);
    let fcgi = crate::fcgi::connections(source, app.max_body);
    let selfs = crate::httpd::unix_connections(own_listener);
    let both: Box<dyn FnOnce(crate::httpd::Hand) + Send> = Box::new(move |hand: crate::httpd::Hand| {
        let h = hand.clone();
        let _ = std::thread::Builder::new().name("quillo-accept-self".into()).spawn(move || selfs(h));
        fcgi(hand);
    });
    let stand_in: std::net::SocketAddr = ([127, 0, 0, 1], 0).into();
    let r = serve_from(app, crate::httpd::Listener::Other(both), Some(stand_in), &shown, workers, |_| {});
    let _ = std::fs::remove_dir_all(&dir);
    if let Some(p) = made {
        remove_if_mine(p, made_id);
    }
    r
}

/// A Unix socket listened on at `path`, with `mode`: one left behind by a server that did not stop
/// cleanly is taken over; one a server answers on, or a file that is not a socket, is not.
#[cfg(unix)]
fn unix_listener(path: &Path, mode: u32) -> Result<std::os::unix::net::UnixListener, String> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    // One left by a server that did not stop cleanly is taken over; one a server answers on is not.
    if let Ok(m) = std::fs::symlink_metadata(path) {
        if !m.file_type().is_socket() {
            return Err(format!("cannot listen on unix:{}: it is there, and not a socket", path.display()));
        }
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(format!("cannot listen on unix:{}: a server is listening there", path.display()));
        }
        let _ = std::fs::remove_file(path);
    }
    // The system keeps a socket's path in about 100 bytes (`sun_path`).
    if path.as_os_str().len() > 100 {
        return Err(format!("cannot listen on unix:{}: a socket's path is at most 100 bytes; use a shorter one, or one relative to here", path.display()));
    }
    // Made for its owner alone from the start (then given `mode`): with the process's umask it
    // would be open to all until the chmod, and a connection made then would be served.
    // SAFETY: umask sets and gives back the process's mask; nothing else here makes files now.
    let before = unsafe { libc::umask(0o077) };
    let bound = std::os::unix::net::UnixListener::bind(path);
    unsafe { libc::umask(before) };
    let listener = bound.map_err(|e| format!("cannot listen on unix:{}: {}", path.display(), e))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|e| format!("cannot set unix:{}'s mode: {}", path.display(), e))?;
    Ok(listener)
}

fn serve_from(
    app: App,
    listener: crate::httpd::Listener,
    bound: Option<std::net::SocketAddr>,
    shown: &str,
    workers: usize,
    listening: impl FnOnce(std::net::SocketAddr),
) -> Result<(), String> {
    threads_allowed(workers);
    let app = Arc::new(app);
    if let Loaded::Ready(it) = &*app.current().1 {
        let n = it.routes.iter().filter(|r| r.method != crate::ast::GUARD_METHOD).count();
        eprintln!("quillo: {} route{} listening on {}", n, if n == 1 { "" } else { "s" }, shown);
        if let Some(db) = &*crate::db::OPENED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
            eprintln!("quillo: database {}", db);
        }
    }
    if app.watch {
        eprintln!("quillo: watching for changes");
    }
    SHOW_ERRORS.store(app.watch, std::sync::atomic::Ordering::Relaxed);
    crate::value::HIDING.store(!app.watch, std::sync::atomic::Ordering::Relaxed);
    crate::value::SERVING.store(true, std::sync::atomic::Ordering::Relaxed);
    // `--plain-http` too (FEEDBACK13.md, Q-089; branch feedback13-fixes).
    let asked = crate::session::PLAIN_HTTP_ASKED.load(std::sync::atomic::Ordering::Relaxed);
    crate::session::PLAIN_HTTP.store(app.watch || app.launch_token.is_some() || asked, std::sync::atomic::Ordering::Relaxed);
    app.shared.gil.release();
    if app.watch || app.reload {
        // Before the first request, not on the watcher's thread: it reads every file first, and a
        // file served meanwhile would never be watched (FEEDBACK16.md, Q-149; branch feedback20-fixes).
        crate::statics::WATCH_SERVED.store(app.watch, std::sync::atomic::Ordering::Relaxed);
        std::thread::Builder::new()
            .name("quillo-watch".into())
            .stack_size(WORKER_STACK)
            .spawn(watcher(app.clone()))
            .map_err(|e| format!("cannot start file watcher: {}", e))?;
    }
    if let Some(a) = bound {
        start_jobs(&app, a);
        start_builds(&app, a);
        listening(a);
    }
    crate::httpd::serve_on(vec![app], listener, workers + 1)
}

/// The paths of the program's `build` declarations (routes whose body is `__built`).
pub fn build_paths(it: &Interp) -> Vec<String> {
    it.routes
        .iter()
        .filter_map(|r| match &r.body {
            crate::ast::Expr::Call(f, args, _) if matches!(&**f, crate::ast::Expr::Ident(n, _) if n == "__built") => match args.first() {
                Some(crate::ast::Expr::Lit(crate::ast::Lit::Str(p))) => Some(p.to_string()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Asks for each `build` once the port is open, so that it is being made before a page asks
/// for it (FEEDBACK37.md, Q-454; branch feedback36-39).
fn start_builds(app: &Arc<App>, addr: std::net::SocketAddr) {
    let paths = match &*app.current().1 {
        Loaded::Ready(it) => build_paths(it),
        _ => Vec::new(),
    };
    let addr = crate::jobs::loopback(addr);
    for p in paths {
        let _ = std::thread::Builder::new().name("quillo-build".into()).spawn(move || {
            use std::io::{Read, Write};
            if let Ok(mut s) = crate::jobs::dial(addr, std::time::Duration::from_secs(5)) {
                let _ = write!(s, "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", p);
                let _ = s.read_to_end(&mut Vec::new());
            }
        });
    }
}

/// Serves several instances of one program (`--instances`), each with an
/// event loop of its own on a thread of its own, all taking connections
/// from the same socket. The calling thread must hold the instances'
/// interpreter locks; it gives them up here.
pub fn serve_many(apps: Vec<App>, addr: &str, workers: usize) -> Result<(), String> {
    take_descriptors();
    crate::value::HIDING.store(true, std::sync::atomic::Ordering::Relaxed);
    crate::value::SERVING.store(true, std::sync::atomic::Ordering::Relaxed);
    let listener = std::net::TcpListener::bind(addr).map_err(|e| format!("cannot listen on {}: {}", addr, e))?;
    if let Ok(b) = listener.local_addr() {
        crate::platform::set_listening(&b.ip().to_string(), b.port());
    }
    let shown = listener.local_addr().map_or_else(|_| addr.to_string(), |a| a.to_string());
    let apps: Vec<Arc<App>> = apps.into_iter().map(Arc::new).collect();
    if let Some(Loaded::Ready(it)) = apps.first().map(|a| a.current().1).as_deref() {
        let n = it.routes.iter().filter(|r| r.method != crate::ast::GUARD_METHOD).count();
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
/// The query parameter a desktop window's first request carries the token
/// in; its value is never written to a log (see `log_url`).
pub const LAUNCH_PARAM: &str = "_quillo_launch";

/// A URL as the log writes it: a desktop window's launch token masked.
pub fn log_url(u: &str) -> std::borrow::Cow<'_, str> {
    let Some((path, q)) = u.split_once('?') else { return std::borrow::Cow::Borrowed(u) };
    if !q.split('&').any(|p| p.split_once('=').map_or(false, |(k, v)| !v.is_empty() && percent_decode(k, true) == LAUNCH_PARAM)) {
        return std::borrow::Cow::Borrowed(u);
    }
    let parts: Vec<String> = q
        .split('&')
        .map(|p| match p.split_once('=') {
            Some((k, v)) if !v.is_empty() && percent_decode(k, true) == LAUNCH_PARAM => format!("{}=[hidden]", k),
            _ => p.to_string(),
        })
        .collect();
    std::borrow::Cow::Owned(format!("{}?{}", path, parts.join("&")))
}

/// Marks a version of the program as replaced by a reload: itself, what it
/// mounts, and the services it loaded (a table of the version's own, which they
/// share, so it is gone through once) (FEEDBACK12.md, Q-082; branch feedback12-fixes).
fn retire(it: &Interp) {
    fn mark(it: &Interp) {
        it.retired.store(true, std::sync::atomic::Ordering::Relaxed);
        for (_, m) in &it.mounts {
            mark(m);
        }
    }
    mark(it);
    for (_, (_, s)) in it.services.loaded.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter() {
        mark(s);
    }
}

/// Whether a request is the scheduler's (it carries this process's secret).
fn is_job_request(req: &HttpRequest) -> bool {
    req.headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("x-quillo-job") && crate::crypto::same(v.as_bytes(), crate::jobs::token().as_bytes()))
}

/// Calls made later (`billing.later.invoice(...)`) delivered, and
/// transactions over several databases recovered, for a program that may
/// have them (started once; see `service::start_deliveries`).
/// In development the whole program's bundle is made on a thread of its own
/// once the program is up, so that a reload does not wait for it and a
/// component that cannot run in the browser is still reported in the log
/// right after (FEEDBACK15.md, Q-098; branch dev-maps-once).
fn warm_client(loaded: &Arc<Loaded>) {
    if !crate::client::DEV.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let loaded = loaded.clone();
    std::thread::Builder::new()
        .name("quillo-client".into())
        .spawn(move || {
            if let Loaded::Ready(it) = &*loaded {
                it.client_js();
            }
        })
        .ok();
}

fn start_background(it: &Interp, addr: std::net::SocketAddr) {
    if crate::service::uses_services(it) || crate::db::uses_atomic(it) {
        crate::service::start_deliveries(addr);
    }
}

/// Starts handing UDP packets to the program `app` runs (see `udp`).
fn start_udp(app: &Arc<App>, addr: std::net::SocketAddr) {
    let app = app.clone();
    crate::udp::start(addr, move || match &*app.current().1 {
        Loaded::Ready(it) => crate::udp::ports(it),
        Loaded::Failed(_) => Vec::new(),
    });
}

/// A program inside an app (src/embed.rs): its jobs, ticks and calls made later, with no port.
pub(crate) fn start_embedded(app: &Arc<App>) {
    start_jobs(app, std::net::SocketAddr::from(([127, 0, 0, 1], 0)));
}

/// Starts the job scheduler for the program `app` runs (see `jobs`).
fn start_jobs(app: &Arc<App>, addr: std::net::SocketAddr) {
    *app.addr.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(addr);
    if let Loaded::Ready(it) = &*app.current().1 {
        start_background(it, addr);
    }
    // Also for a program that listens on no port yet: a reload may add one.
    start_udp(app, addr);
    // On a Unix socket, no WebTransport: pages could not reach its port. A program inside an app,
    // with no port either, has channels over the bridge, which need it.
    if !crate::jobs::on_unix_socket() {
        crate::wt::serving(addr);
    }
    // The program's jobs and ticks, and those of the services it runs in its
    // process, as `<service>~<name>` (FEEDBACK12.md, Q-080 and Q-083; branch
    // feedback12-fixes).
    let jobs_of = app.clone();
    crate::jobs::start(addr, move || match &*jobs_of.current().1 {
        Loaded::Ready(it) => {
            let mut all: Vec<_> = it.site.jobs.iter().map(|j| (j.name.clone(), j.when.clone())).collect();
            for (service, (_, s)) in it.services.loaded.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter() {
                all.extend(s.site.jobs.iter().map(|j| (format!("{}~{}", service, j.name), j.when.clone())));
            }
            all
        }
        Loaded::Failed(_) => Vec::new(),
    });
    let ticks_of = app.clone();
    crate::jobs::start_ticks(addr, move || match &*ticks_of.current().1 {
        Loaded::Ready(it) => {
            let mut all: Vec<_> = it.site.ticks.iter().map(|t| (t.name.clone(), t.ms)).collect();
            for (service, (_, s)) in it.services.loaded.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter() {
                all.extend(s.site.ticks.iter().map(|t| (format!("{}~{}", service, t.name), t.ms)));
            }
            all
        }
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
    let param = &format!("{}=", LAUNCH_PARAM);
    let offered = query.split('&').find_map(|kv| kv.strip_prefix(param.as_str()));
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
/// then (see `boot` in runtime.js); presses, releases, double, right and
/// middle clicks are kept beside them and made again as they were made
/// (branch early-events). The idea is others': Flickr's actionQueue, Google's
/// JSAction (and Angular's event replay on it), Angular Universal's preboot,
/// Qwik's Qwikloader and React's selective hydration (README.md,
/// Acknowledgements; branch acknowledgements).
/// `/_quillo/ready`: 200 or 503, and each thing the program needs.
fn ready_response(it: &mut Interp, req: &HttpRequest) -> HttpResponse {
    let (ok, detail) = crate::ready::report(it);
    // What each check says, the services' internal addresses among them, with the health token;
    // to anyone else, whether it is ready. Not by the address it comes from: behind a proxy
    // on the same machine that adds no `X-Forwarded-For` (nginx's `proxy_pass` as it comes),
    // every visitor is 127.0.0.1 (FEEDBACK51.md, Q-655; branch feedback51-fixes).
    let auth = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("authorization")).map(|(_, v)| v.as_str());
    let detail = if crate::querystats::has_token(auth) { detail } else { serde_json::json!({ "ready": ok }) };
    let mut r = json_response(if ok { 200 } else { 503 }, detail);
    r.headers.push(("cache-control".into(), "no-store".into()));
    r
}

fn island_loader(srcs: &[String]) -> String {
    let src = serde_json::to_string(srcs).unwrap_or_default().replace("</", "<\\/");
    format!(
        concat!(
            "(()=>{{const d=document,W=window,k=Symbol.for(\"quillo\"),w=W[k]||(W[k]={{}}),q=w.q=[];let go=0;",
            // In order: the runtime file first, when there is one.
            "const load=()=>{{if(go)return;go=1;for(const s of {}){{const t=d.createElement(\"script\");t.src=s;t.async=false;t.onerror=fail;d.head.appendChild(t)}}}};",
            "const isl=(e)=>e.target&&e.target.closest&&e.target.closest(\"quillo-island,[data-qi]\");",
            // What a mouse or pointer event said, to make it again: where, with
            // which buttons and keys, how many clicks (branch early-events).
            "const K=\"clientX clientY screenX screenY button buttons detail ctrlKey shiftKey altKey metaKey pointerId pointerType isPrimary width height pressure\".split(\" \");",
            // An event kept, done again on its element: a form sent, else the
            // same event made anew (a click on a link or a checkbox does what
            // it does).
            // A finger lifted before then is no longer a pointer: capturing it
            // (as a drag's `@pointerdown` does) is let go, not an error.
            "const redo=w.redo=(r)=>{{const el=r.el;if(!el.isConnected)return;if(r.t===\"submit\"){{const f=el.closest(\"form\")||el;f.requestSubmit?f.requestSubmit():f.dispatchEvent(new Event(\"submit\",{{bubbles:true,cancelable:true}}));return}}",
            "const P=Element.prototype,sc=P.setPointerCapture;if(r.p)P.setPointerCapture=function(id){{try{{sc.call(this,id)}}catch(_){{}}}};",
            "try{{el.dispatchEvent(new(r.p?PointerEvent:MouseEvent)(r.t,Object.assign({{bubbles:true,cancelable:true,composed:true,view:W}},r.i)))}}finally{{P.setPointerCapture=sc}}}};",
            // No bundle: the clicks and forms kept are done as the page alone does them.
            "const fail=()=>{{w.booted=1;for(const r of q.splice(0))if(r.t===\"click\"||r.t===\"submit\")redo(r)}};",
            "for(const v of[\"pointerover\",\"pointerdown\",\"touchstart\",\"focusin\",\"keydown\"])d.addEventListener(v,(e)=>{{if(isl(e)){{w.at=e.target;load()}}}},{{capture:true,passive:true}});",
            // Kept until the bundle runs, in order: what the page alone would do
            // for a click or a form waits too; a press, a double click or a menu
            // goes on as the browser does it.
            "for(const v of[\"pointerdown\",\"mousedown\",\"pointerup\",\"mouseup\",\"click\",\"auxclick\",\"dblclick\",\"contextmenu\",\"submit\"])d.addEventListener(v,(e)=>{{if(w.booted||!isl(e))return;const i={{}};for(const k of K)if(k in e)i[k]=e[k];q.push({{t:v,el:e.target,i,p:typeof PointerEvent!==\"undefined\"&&e instanceof PointerEvent}});if(v===\"click\"||v===\"submit\"){{e.preventDefault();e.stopPropagation()}}load()}},true);",
            "const idle=()=>{{if(d.querySelector(\"quillo-island[data-e],quillo-island[data-l],[data-qi][data-e],[data-qi][data-l],body>quillo-island\"))(window.requestIdleCallback||setTimeout)(load)}};",
            "d.readyState===\"loading\"?d.addEventListener(\"DOMContentLoaded\",idle):idle()}})()"
        ),
        src
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_client_cannot_take_every_waiting_place() {
        // What the review saw: one address holding all of `MAX_WAITERS`.
        let mine: Vec<Waiter> = (0..MAX_WAITERS_PER_CLIENT).map(|_| Waiter::take("192.0.2.7").expect("a place")).collect();
        assert!(Waiter::take("192.0.2.7").is_none());
        assert!(Waiter::take("::ffff:192.0.2.7").is_none());
        // Its IPv6 neighbours in the same /64 are the same client.
        let six: Vec<Waiter> = (0..MAX_WAITERS_PER_CLIENT).map(|i| Waiter::take(&format!("2001:db8::{:x}", i + 1)).expect("a place")).collect();
        assert!(Waiter::take("2001:db8::ffff").is_none());
        assert!(Waiter::take("192.0.2.8").is_some());
        drop(mine);
        drop(six);
        assert!(Waiter::take("192.0.2.7").is_some());
    }
}
