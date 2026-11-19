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
    // Nothing to decode, as in most paths and queries: the text as it is, which is what the loop
    // gives it then, without going byte by byte and through `from_utf8_lossy` (branch speed-bun,
    // 2026-11-09, the server benchmark).
    if memchr::memchr2(b'%', if plus_is_space { b'+' } else { b'%' }, b).is_none() {
        return s.to_string();
    }
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

/// `parse_urlencoded`, the map held to `--max-memory` as it grows: a body of 1.1 million fields
/// made 211 MB under a 64 MB limit (FEEDBACK71.md, Q-977, found in review; branch security). A
/// field's text is no longer than what it came as.
fn parse_urlencoded_held(s: &str) -> Result<Map, crate::httpd::OverMemory> {
    let mut m = Map::new();
    for pair in s.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        crate::value::insert_held(&mut m, percent_decode(k, true).into(), Value::str(percent_decode(v, true)))?;
    }
    Ok(m)
}

/// What a request's body reads to: its text, its JSON, its form's fields and files. Held to
/// `--max-memory` where `held`: the text made from it (U+FFFD three bytes for each byte that is
/// not UTF-8), its JSON's lists, maps and texts, and a form's fields, each as it is made
/// (FEEDBACK71.md, Q-977, the review's bounded readers; branch security).
fn body_parts(req: &HttpRequest, ctype: &str, held: bool) -> Result<((Value, Value, Value, Value), usize), crate::httpd::OverMemory> {
    // Files: their bytes stay in the body (see `upload`), which is no text.
    let multipart = crate::upload::boundary(ctype);
    let body = if multipart.is_some() {
        String::new()
    } else if held {
        crate::value::lossy_held_of(&req.body)?
    } else {
        String::from_utf8_lossy(&req.body).into_owned()
    };
    let trimmed = body.trim_start();
    // A body is JSON when it says so, or says nothing and looks like it
    // (curl, scripts). Not when it says it is something else: a page on
    // another site can send `text/plain` without asking first, and must
    // not reach routes that expect JSON.
    let looks_json = trimmed.starts_with('{') || trimmed.starts_with('[');
    let mut made = 0;
    let json = if is_json_type(ctype) || (ctype.trim().is_empty() && looks_json) {
        // RFC 8259 lets a reader skip a byte-order mark (FEEDBACK31.md, Q-386; branch
        // feedback27-35-fixes).
        // Read straight into values, with a budget of them (FEEDBACK51.md, Q-660): a visitor's,
        // where each list and map counts its own room and the fewest are what `--max-memory`
        // holds (FEEDBACK75.md, Q-1070; branch feedback75).
        let text = body.strip_prefix('\u{FEFF}').unwrap_or(&body);
        match Value::parse_json_body(text, held)? {
            Ok((v, n)) => {
                made = n;
                v
            }
            Err(_) => Value::Nil,
        }
    } else {
        Value::Nil
    };
    let (form, files) = match &multipart {
        Some(b) => crate::upload::parse_as(b, &req.body, held)?,
        // The type in any case, with or without parameters (FEEDBACK55.md, Q-717; branch feedback55).
        None if essence(ctype) == "application/x-www-form-urlencoded" => (if held { parse_urlencoded_held(&body)? } else { parse_urlencoded(&body) }, Map::new()),
        None => (Map::new(), Map::new()),
    };
    Ok(((Value::str(body), json, Value::map(form), Value::map(files)), made))
}

/// What a request's body read to, held, made once at the start of the request where
/// `--max-memory` is set and the body is long enough to read past 64 KB (`read_body_held`): each
/// `build_request` of it takes it from here, by the body's place and length.
struct BodyRead {
    at: usize,
    len: usize,
    parts: (Value, Value, Value, Value),
    /// The bytes its JSON made, as `--max-memory` counts them (`Value::parse_json_body`).
    made: usize,
}

thread_local! {
    static BODY_READ: std::cell::RefCell<Option<BodyRead>> = const { std::cell::RefCell::new(None) };
}

/// The request's body read, held, as `build_request` reads it, and kept for it: `OverMemory`
/// where a value it reads to would be bigger than `--max-memory` allows.
fn read_body_held(req: &HttpRequest) -> Result<(), crate::httpd::OverMemory> {
    let ctype = req.headers.iter().rev().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map_or("", |(_, v)| v.as_str());
    let (parts, made) = body_parts(req, ctype, true)?;
    BODY_READ.with(|b| *b.borrow_mut() = Some(BodyRead { at: req.body.as_ptr() as usize, len: req.body.len(), parts, made }));
    Ok(())
}

/// What `read_body_held` kept of `req`'s body, if it read it.
fn body_read(req: &HttpRequest) -> Option<(Value, Value, Value, Value)> {
    body_read_made(req).map(|(p, _)| p)
}

/// `body_read`, and the bytes its JSON made.
fn body_read_made(req: &HttpRequest) -> Option<((Value, Value, Value, Value), usize)> {
    if !crate::httpd::MEMORY_LIMITED.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    BODY_READ.with(|b| b.borrow().as_ref().filter(|r| r.at == req.body.as_ptr() as usize && r.len == req.body.len()).map(|r| (r.parts.clone(), r.made)))
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

/// Whether the program has a `GET` (or `ANY`) route at the very path `path`, no parameter in it:
/// Quillo's answer for an app then stays out of its way. One that takes any path (`/*rest`,
/// `/:page`) is tried first, and Quillo answers where it gives a 404 (FEEDBACK67.md, B.1, review;
/// branch pwa).
pub(crate) fn route_at(it: &Interp, path: &str) -> bool {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    it.routes.iter().any(|r| {
        (r.method == "GET" || r.method == "ANY") && r.path.len() == segs.len() && r.path.iter().zip(&segs).all(|(s, p)| matches!(s, Seg::Lit(l) if l == p))
    })
}

/// Whether an answer is a page, `text/html` in any case (`Text/HTML` too): a media type's case is
/// no part of it (RFC 9110 §8.3.1), and a page so marked had none of Quillo's scripts, and none of
/// its offline marks' hashes in its policy (FEEDBACK67.md, B.2, third review; branch pwa).
/// Every answer asks twice, so its first nine bytes are one comparison, each letter's case bit set
/// (`| 0x20` makes an upper-case ASCII letter its lower case, and no other byte one of these
/// letters; the `/` is compared as it is): `trim_start` and `eq_ignore_ascii_case` had cost a
/// text answer 122 instructions more than 0.8.6's `starts_with`, where this costs 12 (speed check
/// of branch pwa). Spaces or tabs before the type, which no answer of Quillo's has, take the long
/// way; other white space is no part of a header's value (RFC 9110 §5.6.3).
pub(crate) fn is_html(resp: &HttpResponse) -> bool {
    const CASE: u64 = u64::from_le_bytes(*b"\x20\x20\x20\x20\x00\x20\x20\x20");
    const TEXT_HTM: u64 = u64::from_le_bytes(*b"text/htm");
    resp.header("content-type").is_some_and(|c| match c.as_bytes().get(..9) {
        Some(t) => {
            let first = u64::from_le_bytes([t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7]]);
            ((first | CASE) == TEXT_HTM && (t[8] | 0x20) == b'l') || (matches!(t[0], b' ' | b'\t') && html_after_spaces(c))
        }
        None => false,
    })
}

/// `is_html` for a type written after spaces or tabs.
#[cold]
#[inline(never)]
fn html_after_spaces(c: &str) -> bool {
    c.trim_start_matches([' ', '\t']).get(..9).is_some_and(|t| t.eq_ignore_ascii_case("text/html"))
}

/// The media type of a content-type header, without its parameters:
/// "Application/JSON; charset=utf-8" is "application/json".
pub(crate) fn essence(ctype: &str) -> String {
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
pub(crate) fn cross_site(req: &HttpRequest) -> bool {
    let header = |name: &str| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim());
    if let Some(site) = header("sec-fetch-site") {
        return !matches!(site.to_ascii_lowercase().as_str(), "same-origin" | "none");
    }
    match (header("origin"), header("host")) {
        (None, _) => false,
        (Some(origin), host) => {
            let (scheme, origin_host) = origin.split_once("://").map_or((None, origin), |(s, h)| (Some(s), h));
            if host.map_or(true, |h| !origin_host.trim_end_matches('/').eq_ignore_ascii_case(h)) {
                return true;
            }
            // An origin is its scheme and its host: where the request's scheme is known, `http://`
            // and `https://` of one host are two sites. Unknown, the hosts are compared alone
            // (FEEDBACK70.md, Q-966; branch security).
            match (scheme, known_scheme(req)) {
                (Some(o), Some(r)) if !o.eq_ignore_ascii_case(r) => {
                    scheme_differs(req, o, r);
                    true
                }
                _ => false,
            }
        }
    }
}

/// The scheme a request came over, where this server knows it: what a proxy `--trust-proxy` names
/// says in `X-Forwarded-Proto`, `http` or `https`: its last header, and the last of a list in it,
/// so that `http, https` and the two in two headers are one (RFC 9110, 5.3). Quillo serves no TLS
/// of its own, and a request from anyone else says nothing that is believed. Without
/// `--trust-proxy` nothing is looked for; the mark the server puts on a proxy's request is its own,
/// put last, of its own spelling (FEEDBACK70.md, Q-966; branch security). Under `quillo fcgi` the
/// web server is that proxy, its `HTTPS` parameter written as the header (Q-987).
pub(crate) fn known_scheme(req: &HttpRequest) -> Option<&'static str> {
    scheme_by(|name| req.headers.iter().rev().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str()), &req.remote)
}

/// `known_scheme`'s rule, which `session::came_over_https` reads the request map by too, so that a
/// form's gate, a deletion's `Secure` and a push subscription's origin take one scheme from one
/// word: `last(name)` is the value of the request's last header of that name. Believed from a proxy
/// `--trust-proxy` names (the mark the server put on its request) or, under `quillo fcgi`, from the
/// web server; from anyone else, nothing is (FEEDBACK70.md, Q-986, Q-987; branch security).
/// Where the header would have counted but is not believed, the log says once which address to
/// name (`peer`, the request's: the connection's, for a peer no proxy names). Looked at here and
/// not in the parser, so that a request that never asks pays nothing (Q-986). A request an app
/// passed on to a service carries the app's own verdict, signed (`service::scheme_of_word`): a
/// service `--split` started, given no `--trust-proxy` of its own, believes that from a peer on
/// this machine, and never the visitor's header the app passed on with it (Q-986, review). No other
/// server looks at the word: the app's own verdict is its proxy's, and a word a service echoed to a
/// visitor must decide nothing there (recheck).
pub(crate) fn scheme_by<S: AsRef<str>>(mut last: impl FnMut(&str) -> Option<S>, peer: &str) -> Option<&'static str> {
    let fcgi = crate::httpd::web_server_scheme();
    if crate::httpd::takes_app_word() && local_peer(peer) {
        if let Some(scheme) = last(crate::service::SCHEME_WORD).and_then(|w| crate::service::scheme_of_word(w.as_ref())) {
            return Some(scheme);
        }
    }
    let believed = fcgi || (crate::httpd::trusts_proxies() && last(crate::httpd::PROXIED).is_some());
    if !believed {
        if !crate::httpd::proto_said() && last("x-forwarded-proto").is_some() {
            crate::httpd::forwarded_proto_not_believed(peer);
        }
        return None;
    }
    let v = last("x-forwarded-proto")?;
    forwarded_scheme(v.as_ref())
}

/// Whether a request's address is this machine's, as a service's app's is: loopback, IPv4 in IPv6's
/// clothes too (FEEDBACK70.md, Q-986, recheck; branch security).
fn local_peer(peer: &str) -> bool {
    match peer.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(a)) => a.is_loopback() || a.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback()),
        Ok(a) => a.is_loopback(),
        Err(_) => false,
    }
}

/// The scheme an `X-Forwarded-Proto` value says, `http` or `https`, by its last item, so that
/// `http, https` and the two in two headers are one (RFC 9110, 5.3); anything else, none.
pub(crate) fn forwarded_scheme(v: &str) -> Option<&'static str> {
    let last = v.rfind(',').map_or(v, |i| &v[i + 1..]).trim();
    match last.len() {
        5 if last.eq_ignore_ascii_case("https") => Some("https"),
        4 if last.eq_ignore_ascii_case("http") => Some("http"),
        _ => None,
    }
}

/// Said once: a write taken as another site's only for the scheme. Browsers send no
/// `Sec-Fetch-Site` to an `http://` page, so a proxy that says `https` for its port-80 site has
/// every browser's write from an `http://` page refused; one that says its own hop's `http` behind
/// another TLS hop, older browsers' writes from `https://` pages (FEEDBACK70.md, Q-966, second
/// review; branch security).
#[cold]
fn scheme_differs(req: &HttpRequest, origin: &str, proxy: &str) {
    static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let origin: String = origin.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')).take(16).collect();
    // Under `quillo fcgi` it is the web server that said, by the parameter fcgi.rs names
    // (FEEDBACK70.md, Q-987, review; branch security).
    if crate::httpd::web_server_scheme() {
        let from = req.headers.iter().rev().find(|(k, _)| k == crate::fcgi::SCHEME_FROM).map_or("HTTPS", |(_, v)| if v == "REQUEST_SCHEME" { "REQUEST_SCHEME" } else { "HTTPS" });
        let why = if proxy == "https" {
            format!("Browsers send no Sec-Fetch-Site to an http:// page, so if this site is served over http:// too and the web server says https there (its {} parameter), every browser's write from an http:// page is refused: have it say http there, or send http:// to https://", from)
        } else {
            format!("If browsers reach this site over https://, TLS ends in front of the web server and its {} parameter is its own hop's scheme, and older browsers' writes are all refused: have it set HTTPS from the front's header (Apache: SetEnvIf X-Forwarded-Proto https HTTPS=on)", from)
        };
        eprintln!("quillo: warning: a request was taken as another site's: its Origin is {}:// and the web server's {} parameter says it came over {}. {}. Said once", origin, from, proxy, why);
        return;
    }
    let why = if proxy == "https" {
        "Browsers send no Sec-Fetch-Site to an http:// page, so if this site is served over http:// too and the proxy says https there (a fixed X-Forwarded-Proto on its port-80 site), every browser's write from an http:// page is refused: have it say http there, or send http:// to https://"
    } else {
        "If browsers reach this site over https://, the proxy is saying its own hop's scheme (as one behind another TLS hop does: Cloudflare's Flexible mode, nginx's $scheme there), and older browsers' writes are all refused: have it pass on the scheme the browser used"
    };
    eprintln!("quillo: warning: a request was taken as another site's: its Origin is {}:// and the proxy (--trust-proxy) says it came over {}. {}. Said once", origin, proxy, why);
}

/// Whether a request's body is one another site's page can send without
/// the browser asking first: a form, multipart, plain text, or none.
fn sends_form(req: &HttpRequest) -> bool {
    let ctype = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.as_str()).unwrap_or("");
    matches!(essence(ctype).as_str(), "" | "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain")
}

/// The `request` value a route sees.
/// What `build_request` needs but the request, kept until the request map is wanted.
pub(crate) struct PendingRequest {
    path: String,
    query: Value,
    params: Value,
}

impl PendingRequest {
    /// A request as no route has taken it yet: its path and query, no parameters (the page's
    /// states, made before the routes; FEEDBACK68.md, Q-985; branch pwa).
    pub(crate) fn of_url(url: &str) -> PendingRequest {
        let (path, qs) = url.split_once('?').unwrap_or((url, ""));
        PendingRequest { path: path.to_string(), query: Value::map(parse_urlencoded(qs)), params: Value::map(Map::new()) }
    }

    pub(crate) fn make(&self, req: &HttpRequest) -> Value {
        let mut m = build_request(req, &self.path, &self.query);
        m.insert("params".into(), self.params.clone());
        Value::map(m)
    }
}

/// The request being answered, for what is worked out from it only if something asks (its map,
/// who asks). The one pointer to it: only `Answering` sets it and clears it, so it cannot outlive
/// the `&HttpRequest` it was made from (FEEDBACK55.md, Q-883; branch feedback55).
pub(crate) struct Asked(*const HttpRequest);

// SAFETY: see `Answering`, the only place one is made: it is cleared before the reference it was
// made from ends. A request's `parallel` functions on other threads are handed what it holds,
// never this.
unsafe impl Send for Asked {}
unsafe impl Sync for Asked {}

/// `handle_inner` as it answers a request. What the request left on the interpreter goes when this
/// does, however the answer ended: a return, a `?`, a panic. A route whose query pattern failed
/// returned with `pending_request` still pointing at its request, which was freed, and the next
/// thing on that worker that read the request read through it, a job handed the last visitor's
/// query and whatever was in that memory; and a service's call left its `current_request`, the
/// forwarded token of a visitor, for the next job (FEEDBACK55.md, Q-883; branch feedback55).
struct Answering<'a>(&'a mut Interp);

impl<'a> Answering<'a> {
    fn begin(it: &'a mut Interp, req: &HttpRequest) -> Self {
        it.asked = Some(Asked(req as *const HttpRequest));
        Answering(it)
    }
}

impl Drop for Answering<'_> {
    fn drop(&mut self) {
        self.0.asked = None;
        self.0.pending_request = None;
        self.0.current_request = None;
        self.0.who = None;
    }
}

impl Interp {
    /// The request being answered, while it is (see `Answering`).
    pub(crate) fn asked(&self) -> Option<&HttpRequest> {
        // SAFETY: `Answering` set it from a reference that outlives it, and clears it.
        self.asked.as_ref().map(|a| unsafe { &*a.0 })
    }
}

/// A path as routes read it: split at `/`, empty pieces dropped, each decoded. `//admin/users`
/// and `/%61dmin/users` are `/admin/users`, which a route under `/admin` answers, so what is
/// said about `/admin` must hold for them too (FEEDBACK47.md, Q-537; branch feedback40-43).
pub fn route_path(path: &str) -> String {
    // Already in the form this gives, as nearly every path is: `/` and pieces, none empty, no
    // `%` to decode. The same text, without splitting, joining and `format!` (branch speed-bun,
    // 2026-11-09, the server benchmark).
    let b = path.as_bytes();
    if b.first() == Some(&b'/') && (b.len() == 1 || b[b.len() - 1] != b'/') && memchr::memchr(b'%', b).is_none() && memchr::memmem::find(b, b"//").is_none() {
        return path.to_string();
    }
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

pub(crate) fn build_request(req: &HttpRequest, path: &str, query: &Value) -> Map {
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
    // Read at the request's start, held, where it could read past `--max-memory`; else here.
    let (body, json, form, files) = match body_read(req) {
        Some(parts) => parts,
        None => body_parts(req, &ctype, false).map(|(p, _)| p).unwrap_or((Value::str(""), Value::Nil, Value::map(Map::new()), Value::map(Map::new()))),
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
    m.insert("body".into(), body);
    m.insert("json".into(), json);
    m.insert("form".into(), form);
    m.insert("files".into(), files);
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
            let mut resp = crate::service::pass_on(self, &url, &prefix, req);
            if self.site.offline.is_some() {
                crate::pwa::unmarked(&mut resp);
            }
            return resp;
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
                // Only the main program's worker marks are Quillo's (FEEDBACK67.md, B.2, third
                // review; branch pwa).
                if self.site.offline.is_some() {
                    crate::pwa::unmarked(&mut resp);
                }
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
        self.session_id = None;
        self.session_ends = None;
        self.visitor_read = false;
        self.visitor_other = false;
        self.offline_seen = None;
        self.locale_read = false;
        self.locale = None;

        if !req.body.is_empty() {
            let ctype = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map_or("", |(_, v)| v.as_str());
            if crate::upload::boundary(ctype).is_some() {
                self.upload_body = Some(Arc::from(req.body.as_slice()));
            }
        }
        // A page asked for ahead of a click: made without changing anything; and the old version's
        // render of a page under --watch, a print to compare (FEEDBACK54.md, found in review).
        // Quillo's worker keeping a program's offline pages as it installs asks for them so, under
        // --no-prefetch too: a GET that would write is refused, not run (FEEDBACK67.md, B.2, second
        // review; branch pwa).
        let prefetch = (is_prefetch(req) || self.read_only || (self.site.offline.is_some() && precache(req))).then(|| Arc::new(std::sync::atomic::AtomicBool::new(false)));
        self.prefetching = prefetch.clone();
        let changed = (req.method != "GET" && req.method != "HEAD").then(|| Arc::new(std::sync::atomic::AtomicBool::new(false)));
        self.changed = changed.clone();
        // The page's states, once the request is the one being answered: an initializer that read
        // `session()` found none, and so did every `session()` of that request after it, the
        // session kept as none (FEEDBACK68.md, Q-985, found in FEEDBACK67's third review; branch
        // pwa).
        let mut resp = {
            let a = Answering::begin(self, req);
            a.0.page_start(req);
            a.0.handle_inner(req)
        };
        // An app's, from its app.toml (src/pwa.rs): its manifest and Apple icon where a route that
        // takes any path answered 404, and its tags in a page's head. One check here, all a
        // program without one pays (FEEDBACK67.md, B.1 and its review; branch pwa).
        if self.site.pwa.is_some() {
            crate::pwa::after_route(self, req, path, &mut resp);
        }
        // Before anything that can wait: another request on the thread would find it
        // (FEEDBACK55.md, Q-883; branch feedback55).
        let made = crate::stream::take_made();
        // A transaction begun by hand and left open (FEEDBACK44.md, Q-580).
        crate::db::left_open(self, || format!("{} {}", req.method, path));
        // A prefetch's deferred parts: worked out now, as part of the prefetch, which
        // may change nothing (branch defer).
        // Their ids, for their scripts' hashes in the page's policy (FEEDBACK59.md, Q-904, found
        // in review: a prefetched page kept its waiting components waiting).
        let mut early_parts: Vec<usize> = Vec::new();
        if prefetch.is_some() && !self.deferred.is_empty() {
            let jobs = std::mem::take(&mut self.deferred);
            if is_html(&resp) && resp.status == 200 {
                early_parts = jobs.iter().map(|d| d.id).collect();
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
                let mut headers = vec![("cache-control".into(), "no-store".into())];
                // Said so to Quillo's worker, which keeps such a page on no try, where a proxy's 503
                // is tried again (FEEDBACK67.md, B.2, second review; branch pwa).
                if self.site.offline.is_some() {
                    headers.push(("x-quillo-prefetch".into(), "refused".into()));
                }
                return HttpResponse { status: 503, headers, body: Vec::new() };
            }
            // Only this page's own fetch needs the HTTP cache: Chrome keeps a speculation
            // rule's answer itself, and would keep it there too. `Vary`, so that it does not
            // outlive the session it was made for (FEEDBACK22.md, Q-176; branch
            // feedback22-fixes).
            let own = req.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("x-quillo-prefetch"));
            // A visitor's page, `private` by Quillo, too: else the link's page was made twice, as
            // nothing could keep it (found in review).
            let quillos = resp.header("cache-control").is_none() || resp.header("cache-control") == Some("private");
            if own && resp.status == 200 && quillos && !crate::idempotency::holds_form_key(&resp.body) {
                resp.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("cache-control"));
                resp.headers.push(("cache-control".into(), "private, max-age=30".into()));
                if !resp.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("vary") && v.to_ascii_lowercase().contains("cookie")) {
                    resp.headers.push(("vary".into(), "Cookie".into()));
                }
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
        // A program with routes tagged `offline` (src/pwa.rs, `Offline`): the marks its service
        // worker reads, the session a page of the visitor's and each sign-in or sign-out leaves,
        // and an offline route's page as one to keep, whose page then registers the worker.
        // `Clear-Site-Data: "storage"` would have dropped the visitor's kept pages at a sign-out,
        // and the worker with them. One check here, all a program without the tag pays
        // (FEEDBACK67.md, B.2 and its review; branch pwa).
        let marked = if self.site.offline.is_some() {
            let offline = std::mem::take(&mut self.offline_page);
            let parts = !self.deferred.is_empty() || !early_parts.is_empty();
            crate::pwa::mark(self, req, offline, session, parts, &mut resp)
        } else {
            crate::pwa::Marked::default()
        };
        // Pages with components load the browser bundle.
        let html = is_html(&resp);
        // Every page prefetches the links pointed at: a page a route made, not a file as it is on
        // disk, whose ETag and byte ranges are of the file (FEEDBACK36.md, Q-447; branch
        // feedback36-39).
        let html = html && !as_written(&resp);
        // What the answer's content policy refuses of what Quillo writes into the page itself:
        // its scripts are then hashed into the policy, and an island's `display:contents` is a
        // rule of a `<style>` (FEEDBACK59.md, Q-904; branch cache-and-migrate).
        let refused = if html && req.method != "HEAD" { policy_refuses(&resp) } else { Refused::default() };
        let mut own: Vec<String> = Vec::new();
        // Only a page with something to prefetch: a link, a script that may make one, or a
        // component. One with none carried 1,300 bytes that never ran (the Fortunes page,
        // 2,560 bytes against Bun's 1,226; branch page-size). With --metrics, such a page still
        // says whether it was landed on from a prefetch.
        // What the page has that these look for: links and scripts, a stream's player; one pass
        // for both (branch speed-fortunes).
        let tags = if html && req.method != "HEAD" { page_tags(&resp.body) } else { PageTags::default() };
        // The script that registers Quillo's worker, on an offline route's page; let through the
        // page's policy as Quillo's other scripts are. A page kept for a session is shown again
        // behind the worker's gate, whose text is the same everywhere: the policy kept with the
        // page lets it through, so that the worker changes no policy (FEEDBACK67.md, B.2 and its
        // review; branch pwa).
        if marked.register && html && req.method != "HEAD" {
            if let Some(o) = self.site.offline.clone() {
                let at = rfind_tag(&resp.body, b"</body>").unwrap_or(resp.body.len());
                insert_at(&mut resp.body, at, o.register.as_bytes());
                if refused.scripts {
                    own.push(o.register_js.clone());
                    if marked.private {
                        own.push(crate::pwa::gate().to_string());
                    }
                }
            }
        }
        if html && req.method != "HEAD" && PREFETCH.load(std::sync::atomic::Ordering::Relaxed) {
            let has = rfind_tag;
            if let Some(at) = has(&resp.body, b"</body>") {
                if self.rendered_island || tags.follow {
                    if !script_with(&resp.body, &PREFETCH_JS.as_bytes()[..40]) {
                        if refused.scripts {
                            // Chrome's speculation rules, written for each link, pass no policy
                            // that names hashes: the links are fetched instead, as other browsers
                            // do (found in review).
                            let t = format!("<script>{}</script>{}", prefetch_by_fetch(), used_script(&self.base));
                            own.extend(script_texts(t.as_bytes()));
                            insert_at(&mut resp.body, at, t.as_bytes());
                        } else {
                            insert_at(&mut resp.body, at, &prefetch_tags(&self.base));
                        }
                    }
                } else if crate::metrics::ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
                    let t = used_script(&self.base);
                    if refused.scripts {
                        own.extend(script_texts(t.as_bytes()));
                    }
                    insert_at(&mut resp.body, at, t.as_bytes());
                }
            }
        }
        // A page that plays a stream loads the player.
        if html && req.method != "HEAD" {
            if tags.media && !script_tag_for(&resp.body, b"/_quillo/media.js") {
                let at = rfind_tag(&resp.body, b"</body>").unwrap_or(resp.body.len());
                if refused.scripts {
                    // Made by a script of Quillo's, hashed: a policy with `'strict-dynamic'` takes
                    // what it makes, and refuses a `<script src>` in the page (found in review).
                    let load = format!("(d=>{{const s=d.createElement(\"script\");s.src=\"{}/_quillo/media.js\";s.async=false;d.head.appendChild(s)}})(document)", self.base);
                    insert_at(&mut resp.body, at, format!("<script>{}</script>", load).as_bytes());
                    own.push(load);
                } else {
                    insert_at(&mut resp.body, at, format!("<script src=\"{}/_quillo/media.js\" defer></script>", self.base).as_bytes());
                }
            }
        }
        // The styles of what the page's bundle can show, or all of them for
        // the whole program's bundle.
        let mut bundle_styles: Vec<String> = Vec::new();
        let mut hints = String::new();
        if html && req.method != "HEAD" && std::mem::take(&mut self.rendered_island) {
            let has = rfind_tag;
            let mut tags = String::new();
            // The page's states as its render left them, for its components to start from, before
            // the code that reads them (FEEDBACK68.md; branch global-state).
            let page = if self.page_states.is_empty() { Vec::new() } else { crate::value::page_values(&self.globals) };
            if !page.is_empty() {
                let mut m = crate::value::Map::new();
                for (name, v) in page {
                    m.insert(name.as_str().into(), v);
                }
                // As a component's `data-s` is: JSON, read by the runtime's `islandJSON`.
                let json = crate::interp::island_json(&crate::server::for_visitor(Value::map(m))).to_string();
                let text = serde_json::Value::String(json).to_string();
                // No `<` and no `&` in it, as in the i18n script and a component's JSON: read as
                // an SVG script (a page whose raw markup left an <svg> open), `<img` would be an
                // element and `&quot;` a quote that ends the string; `<` and `&` are the
                // same characters to JavaScript wherever it is read (coverage of 0.8.7, Q-1116;
                // branch coverage-087).
                tags.push_str(&format!("<script>{}.g={}</script>", crate::client::HANDOFF_JS, text.replace('<', "\\u003c").replace('&', "\\u0026")));
            }
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
            if refused.scripts {
                own.extend(script_texts(tags.as_bytes()));
            }
            insert_at(&mut resp.body, at, tags.as_bytes());
        }
        // The page's sheet: the styles of the components and views on it,
        // and of those its bundle can show, at an address that changes when
        // they do, in its <head> (FEEDBACK8.md, Q-068; branch
        // feedback8-fixes).
        let unmarked = std::mem::take(&mut self.styles_used);
        if html && req.method != "HEAD" {
            // Only the program's own styles' ids, each found once by a set: a visitor's comment
            // of `data-q-` words held every view of its page for seconds (FEEDBACK75.md, Q-1054;
            // branch feedback75).
            // The lock only for a page with a mark: one with none pays a search, as before.
            let mut ids = if crate::styles::marked(&resp.body) {
                let known = self.styles.lock().unwrap_or_else(std::sync::PoisonError::into_inner).known();
                crate::styles::ids_known(&resp.body, &known)
            } else {
                Vec::new()
            };
            ids.extend(bundle_styles);
            ids.extend(unmarked);
            // The libraries named `with preload` or `with preconnect`, before the
            // sheet (branch library-hints).
            let mut link = hints;
            if let Some(hash) = self.sheet_for_page(&ids) {
                link.push_str(&format!("<link rel=\"stylesheet\" href=\"{}/_quillo/s/{}.css\">", self.base, hash));
            }
            if !link.is_empty() {
                let find = |needle: &[u8]| find_ascii_ci(&resp.body, needle);
                let at = match (find(b"</head>"), find(b"<body")) {
                    (Some(h), _) => h,
                    (None, Some(b)) => resp.body[b..].iter().position(|&c| c == b'>').map_or(0, |e| b + e + 1),
                    (None, None) => 0,
                };
                insert_at(&mut resp.body, at, link.as_bytes());
            }
        }
        // Quillo's own scripts and its islands' style, let through the page's policy, waiting
        // components' parts included, which come after the head (FEEDBACK59.md, Q-904; branch
        // cache-and-migrate).
        let parts = html && req.method != "HEAD" && resp.status == 200 && !self.deferred.is_empty();
        if refused.scripts && (parts || !early_parts.is_empty()) {
            own.extend(script_texts(crate::defer::SWAP.as_bytes()));
            own.push(crate::defer::ONE_SWAP.to_string());
            // A prefetch's parts, in the page already: each swapped in by the one line.
            if !early_parts.is_empty() {
                let mut body = std::mem::take(&mut resp.body);
                for id in &early_parts {
                    let line = crate::defer::part(*id, "");
                    let line = &line[line.find("<script>").unwrap_or(0)..];
                    body = replace_bytes(&body, line.as_bytes(), format!("<script>{}</script>", crate::defer::ONE_SWAP).as_bytes());
                }
                resp.body = body;
            }
        }
        // The rule only where an island is, or may come in a part.
        let islands = refused.attr && (plain_islands(&mut resp.body) || parts);
        if refused.scripts || islands {
            allow_own(&mut resp, &own, &[], islands);
        }
        // Waiting components (src/defer.rs; branch defer): the page now, each of them
        // after it in the same answer; whole when there is no server to stream it.
        if !self.deferred.is_empty() {
            let jobs = std::mem::take(&mut self.deferred);
            if html && req.method != "HEAD" && resp.status == 200 {
                let mut page = crate::defer::split(std::mem::take(&mut resp.body), jobs);
                page.plain_islands = refused.attr;
                page.one_swap = refused.scripts;
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
            let mut resp = self.handle_internal(rest, req, path, qs);
            // Says this came from the server fn's own server, whatever it holds: `respond` and
            // `reply` answer as they like, and the page has to tell them from a sign-in page or a
            // proxy's (FEEDBACK49.md, Q-598; branch feedback61).
            if rest.starts_with("rpc/") && !resp.headers.iter().any(|(k, _)| k == "x-quillo-answer") {
                resp.headers.push(("x-quillo-answer".into(), "1".into()));
            }
            return resp;
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
                self.pending_request = Some(PendingRequest { path: path.to_string(), query: query_v.clone(), params: params_v });
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
            let reads = r.reads_visitor || self.middleware_reads(&r);
            self.visitor_read |= reads;
            // What Quillo's worker keeps: a read past the session (FEEDBACK67.md, B.2, review;
            // branch pwa).
            self.visitor_other |= r.reads_visitor || (reads && self.site.offline.is_some() && self.middleware_beyond(&r));
            let file = std::mem::replace(&mut self.cur_file, r.file.clone());
            // `route ... with db`: its body runs as a `with` function's, narrowed, never widened
            // (branch feedback40-43).
            // Kept for the request: a stream's function and deferred parts run after the body.
            self.route_limit = r.limit.clone();
            let ran = self.under_route(|it| if crate::debug::on() { it.debug_route(&r, &scope) } else { it.eval(&r.body, &scope) });
            // Its parameters' scope ended as a block's: a lambda assigned to one held it for ever
            // (FEEDBACK65.md, Q-941 review 4; branch feedback65).
            Scope::done(scope);
            self.cur_file = file;
            // A middleware gave null: its group's routes do not match, and the next route is
            // tried (FEEDBACK51.md, Q-651).
            if let Some(group) = no_match_of(&r, &ran) {
                excluded.push(group);
                self.current_request = None;
                self.pending_request = None;
                continue;
            }
            // The page's components will need the locale (see `handle`). Before `finish`, which
            // may leave a stream for `handle`, as a rule that waits lets another request run
            // (FEEDBACK55.md, Q-883; branch feedback55).
            if self.rendered_island && self.site.i18n.is_some() {
                self.locale();
            }
            let resp = finish(crate::interp::in_file(ran, &r.file));
            self.current_request = None;
            self.pending_request = None;
            // For Quillo's service worker, once the answer is made (see `handle`; FEEDBACK67.md,
            // B.2; branch pwa).
            self.offline_page = r.offline;
            // A `HEAD` is answered with the GET's headers, its length included; the body is left
            // out where the answer is written (FEEDBACK31.md, Q-393; branch feedback27-35-fixes).
            return resp;
        }
        // An app's manifest and Apple's icon, from its app.toml: where no route of the program's
        // takes the path (see `pwa::attach`), so here, after them, where a request a route answers
        // never comes; and before a guarded group's middleware, as a browser asks for them without
        // its cookies and they hold nothing of anyone's (FEEDBACK67.md, B.1; branch pwa).
        if self.site.pwa.is_some() {
            if let Some(resp) = crate::pwa::answer(self, path, req) {
                return resp;
            }
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
    /// Whether the middleware of a route's groups can read the visitor. Each is read as the route
    /// calls it, in its body: there the file's own names are what they run as, where the group
    /// wrote `gate`, a file's `private fn gate` lost to the program's, and the page lost
    /// `private`. What is not one function of the program's, a call that makes one say, cannot
    /// be read, and counts (FEEDBACK55.md, Q-886; branch feedback55).
    fn middleware_reads(&self, r: &crate::ast::RouteDef) -> bool {
        use crate::ast::{Expr, Stmt};
        let Expr::Block(stmts) = &r.body else { return false };
        *r.middleware_reads.get_or_init(|| {
            stmts.iter().any(|s| {
                let Stmt::Expr(Expr::Call(f, args, _)) = s else { return false };
                if !matches!(&**f, Expr::Ident(n, _) if n == "__answer") {
                    return false;
                }
                match args.first() {
                    Some(Expr::Call(m, _, _)) => match &**m {
                        Expr::Lambda(d) => crate::parser::middleware_reads_visitor(d),
                        Expr::Ident(n, _) => match lookup(&self.globals, n) {
                            Some(Value::Func(c)) => crate::parser::middleware_reads_visitor(&c.def),
                            _ => true,
                        },
                        _ => true,
                    },
                    _ => true,
                }
            })
        })
    }

    /// `middleware_reads`, as `parser::middleware_reads_beyond` reads the middleware: past the
    /// session, for what Quillo's service worker keeps (FEEDBACK67.md, B.2, review; branch pwa).
    #[cold]
    fn middleware_beyond(&self, r: &crate::ast::RouteDef) -> bool {
        use crate::ast::{Expr, Stmt};
        let Expr::Block(stmts) = &r.body else { return false };
        *r.middleware_beyond.get_or_init(|| {
            stmts.iter().any(|s| {
                let Stmt::Expr(Expr::Call(f, args, _)) = s else { return false };
                if !matches!(&**f, Expr::Ident(n, _) if n == "__answer") {
                    return false;
                }
                match args.first() {
                    Some(Expr::Call(m, _, _)) => match &**m {
                        Expr::Lambda(d) => crate::parser::middleware_reads_beyond(d),
                        Expr::Ident(n, _) => match lookup(&self.globals, n) {
                            Some(Value::Func(c)) => crate::parser::middleware_reads_beyond(&c.def),
                            _ => true,
                        },
                        _ => true,
                    },
                    _ => true,
                }
            })
        })
    }

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
            let reads = r.reads_visitor || self.middleware_reads(r);
            self.visitor_read |= reads;
            self.visitor_other |= r.reads_visitor || (reads && self.site.offline.is_some() && self.middleware_beyond(r));
            let file = std::mem::replace(&mut self.cur_file, r.file.clone());
            self.route_limit = r.limit.clone();
            let answered = self.under_route(|it| it.eval(&r.body, &scope));
            // As a route's (FEEDBACK65.md, Q-941 review 4; branch feedback65).
            Scope::done(scope);
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
        let html = is_html(resp);
        let framed_by_policy = resp.header("content-security-policy").is_some_and(|c| c.contains("frame-ancestors"))
            || want.iter().any(|(k, v)| k == "content-security-policy" && v.as_deref().is_some_and(|c| c.contains("frame-ancestors")));
        if html && !framed_by_policy && !want.iter().any(|(k, _)| k == "x-frame-options") {
            want.push(("x-frame-options".into(), Some("SAMEORIGIN".into())));
        }
        // Whatever the status: a 403 or a 404 naming the visitor is theirs too, and a cache keeps
        // those as well (FEEDBACK55.md, Q-886; branch feedback55).
        let personal = self.visitor_read || sets_cookie;
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
        // An answer made from who the visitor is stays theirs, whatever its route, `file`'s
        // `cache` or a `site_headers` rule said: `public` (or a time a shared cache takes,
        // `s-maxage`, a `max-age` alone) let a cache in front give it to the next visitor, as
        // Cloudflare ignores `Vary` and keeps a `.pdf` or a `.png` with no rule of its own. The
        // time stays, for the visitor's own browser, as a page with a form key has its `public`
        // turned to `no-store` (the user's choice of 2026-11-16; FEEDBACK59.md, Q-903; branch
        // cache-and-migrate).
        // The fields a CDN reads before `Cache-Control` (`CDN-Cache-Control` and its `Cloudflare-`
        // kind, RFC 9213, `Surrogate-Control`, nginx's `X-Accel-Expires`) go, leaving it the
        // decision (found in review). A `cache-control: null` stays none, as the user decided for
        // `null` (FEEDBACK55.md, Q-710).
        // Nothing is made for an answer that needs no change, as each one a visitor gets comes here.
        if personal {
            // A targeted field (RFC 9213: `CDN-`, `Cloudflare-CDN-`, `Akamai-Cache-Control`), and the
            // older ones: Fastly's and Varnish's `Surrogate-Control`, Akamai's `Edge-Control`, nginx's
            // `X-Accel-Expires`.
            let cdn = |k: &str| {
                let b = k.as_bytes();
                (b.len() > 14 && b[b.len() - 14..].eq_ignore_ascii_case(b"-cache-control"))
                    || k.eq_ignore_ascii_case("surrogate-control")
                    || k.eq_ignore_ascii_case("edge-control")
                    || k.eq_ignore_ascii_case("x-accel-expires")
            };
            // Whether to say what changed: asked only when something does.
            let mut tell: Option<bool> = None;
            let mut changed: Vec<(String, String, Option<String>)> = Vec::new();
            if resp.headers.iter().any(|(k, _)| cdn(k)) {
                let tell = *tell.get_or_insert_with(|| !said_here(path));
                resp.headers.retain(|(k, v)| {
                    if tell && cdn(k) && v != DROPPED {
                        changed.push((k.to_ascii_lowercase(), v.clone(), None));
                    }
                    !cdn(k)
                });
            }
            for (k, v) in resp.headers.iter_mut() {
                if !k.eq_ignore_ascii_case("cache-control") || v == "private" || v == DROPPED {
                    continue;
                }
                if let Some(own) = kept_by_visitor_alone(v) {
                    if *tell.get_or_insert_with(|| !said_here(path)) {
                        changed.push(("cache-control".into(), std::mem::replace(v, own.clone()), Some(own)));
                    } else {
                        *v = own;
                    }
                }
            }
            if !changed.is_empty() {
                said_private(path, || {
                    changed
                        .iter()
                        .map(|(k, v, own)| match own {
                            Some(own) => format!("`{}: {}` sent as `{}`", k, v, own),
                            None => format!("`{}: {}` left out", k, v),
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                });
            }
        }
        // Not when the program said `vary: null` (FEEDBACK55.md, Q-710; branch feedback55).
        let vary = |resp: &mut HttpResponse, name: &str| {
            let lower = name.to_ascii_lowercase();
            if !resp.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("vary") && (v == DROPPED || v.to_ascii_lowercase().contains(&lower))) {
                resp.headers.push(("vary".into(), name.into()));
            }
        };
        // A page in the visitor's language is told by what chose it, the `locale` cookie or
        // `Accept-Language`, and a page with components carries its locale's text: a cache gave
        // the first visitor's language to everyone (FEEDBACK55.md, Q-886; branch feedback55).
        let translated = self.site.i18n.is_some() && (self.locale_read || self.rendered_island);
        if personal || translated {
            vary(resp, "Cookie");
        }
        if translated {
            vary(resp, "Accept-Language");
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
        // An app's icons, named by what they are made of (FEEDBACK67.md, B.1; branch pwa).
        if let Some(hash) = rest.strip_prefix("i/").and_then(|r| r.strip_suffix(".png")) {
            return crate::pwa::icon_answer(self, hash);
        }
        // In development, the files a page's bundle loads (see `client::DEV`).
        if let Some((hash, file)) = rest.strip_prefix("src/").and_then(|r| r.split_once('/')) {
            let file = percent_decode(file, false);
            // Its address names its text (FEEDBACK54.md, Q-688; branch hmr): kept, as a page's
            // bundle is.
            return match crate::client::dev_file(hash, &file) {
                Some(js) => HttpResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "text/javascript; charset=utf-8".into()), ("cache-control".into(), "public, max-age=31536000, immutable".into())],
                    body: js.as_bytes().to_vec(),
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
        // Quillo's service worker, for a program with routes tagged `offline`, and under --watch the
        // one that takes itself away (FEEDBACK67.md, B.2; branch pwa).
        if rest == "sw.js" {
            if let Some(resp) = crate::pwa::worker_answer(self, req) {
                return resp;
            }
        }
        // A page's push subscription, for a program that uses `notification` (FEEDBACK67.md, B.3;
        // branch pwa); bound to who the request's session says.
        if rest == "push" && self.site.notification {
            return crate::push::endpoint(self, req, path);
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
            let (since, server, topics, later) = live_query(qs);
            let topics = if later {
                match later_topics(&req.body) {
                    Ok(t) => t,
                    Err(r) => return r,
                }
            } else {
                topics
            };
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
            // What a page previews as someone writes: a megabyte, more than a person writes in
            // one, where any request could send `--max-body`'s 10 MB to every server
            // (FEEDBACK55.md, Q-706; branch feedback55).
            if req.body.len() > MARKDOWN_BODY {
                return rpc_error(413, "markdown here takes at most 1 MB of JSON: make a longer text's in a route or a server fn");
            }
            // Read straight into values with a visitor's budget, not as serde's and then copied
            // (FEEDBACK75.md, Q-1069; branch feedback75).
            let held = crate::httpd::MEMORY_LIMITED.load(std::sync::atomic::Ordering::Relaxed);
            let args = match std::str::from_utf8(&req.body).map(|t| Value::parse_json_body(t, held)) {
                Ok(Ok(Ok((Value::List(a), _)))) if (1..=2).contains(&a.len()) => a.to_vec(),
                Ok(Err(_)) => return over_memory_answer(),
                _ => return rpc_error(400, "the body must be a JSON array: [text, options]"),
            };
            // What pulldown-cmark's tree will take, worked out from the text before it is made,
            // held to `--max-memory`: 1 MB of `[` (1.2 KB of gzip) made 178 MB under 8 MB
            // (FEEDBACK75.md, Q-1069; branch feedback75).
            if let Some(Value::Str(text)) = args.first() {
                if crate::httpd::making(markdown_room(text)).is_err() {
                    return over_memory_answer();
                }
            }
            return match crate::builtins::markdown(self, args, 0) {
                // Written once, from the HTML: it was copied to a text, to serde's string and to
                // the answer, three more of a text that is many times the request (FEEDBACK75.md,
                // Q-1069; branch feedback75).
                Ok(v) => {
                    let html = match &v {
                        Value::Html(h) => h.clone(),
                        other => Arc::from(other.display()),
                    };
                    let mut body = Vec::with_capacity(html.len() + 10);
                    body.extend_from_slice(b"{\"ok\":");
                    let _ = serde_json::to_writer(&mut body, &*html);
                    body.push(b'}');
                    HttpResponse { status: 200, headers: vec![("content-type".into(), "application/json".into())], body }
                }
                // Past `--max-compute`: the server's limit, a 500 as a route's (Q-706); the policy's
                // too, known by the stop's own words, its file said in the log and not to the page
                // (FEEDBACK55.md, Q-849; branch feedback55).
                Err(Ctrl::Error(e)) if crate::httpd::is_compute_stop(&e.msg) => {
                    crate::httpd::error_line(&e.msg);
                    rpc_error(500, &crate::httpd::over_compute_said_to_pages())
                }
                // Past `--max-memory`, as past `--max-compute`, told by its kind (FEEDBACK71.md,
                // Q-977; branch security).
                Err(Ctrl::Error(e)) if e.memory_stop => {
                    crate::httpd::error_line(&e.msg);
                    rpc_error(500, &crate::httpd::over_memory_said_to_pages())
                }
                Err(Ctrl::Error(e)) => rpc_error(400, &e.msg),
                Err(_) => rpc_error(500, "markdown failed"),
            };
        }
        // The program's tools for AI clients, MCP over HTTP (FEEDBACK56.md, C.2; branch feedback56).
        if rest == "mcp" {
            return crate::mcp::serve_http(self, req, path);
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
        // What the request's start read of the body, held, under `--max-memory` (`read_body_held`):
        // the arguments are taken from it, not read a second time.
        // Not a body with two content types, of which the start read the last and this call reads
        // the first (as 0.8.8 did: FEEDBACK75.md, Q-1052's review; branch feedback75).
        let one_type = req.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("content-type")).count() == 1;
        let read = body_read_made(req).filter(|_| one_type);
        // With files: the arguments are the `args` part, and each file
        // (`{"$quillo_file": i}` there) is the upload in part `file<i>`.
        let (form_args, files) = match (&boundary, &read) {
            (Some(_), Some(((_, _, Value::Map(form), Value::Map(files)), _))) => (Some(form.get("args").map(|v| v.display()).unwrap_or_default()), (**files).clone()),
            (Some(b), _) => {
                let (form, files) = crate::upload::parse(b, &req.body);
                (Some(form.get("args").map(|v| v.display()).unwrap_or_default()), files)
            }
            (None, _) => (None, Map::new()),
        };
        let json_args: &[u8] = form_args.as_deref().map_or(&req.body, str::as_bytes);
        // Read straight into values, with a visitor's budget of them and each held to
        // `--max-memory`, then their marks (a file anywhere in them, a list's or a map's too:
        // FEEDBACK30.md, Q-364; a decimal, an int, an error, a map of one `$` key: FEEDBACK55.md,
        // Q-875's review) made into what they stand for (`value::island_marks`). They were read
        // as serde's values with no budget and copied: one call of 10 KB of gzip made 1 GB, past
        // `--max-memory`, before its arguments were checked (FEEDBACK75.md, Q-1052; branch
        // feedback75). As deep as every reader takes, and said so (FEEDBACK55.md, Q-875's second
        // review; branch feedback55).
        let bad = || rpc_error(400, "the body must be a JSON array of arguments");
        if crate::value::json_depth(json_args) > crate::value::JSON_NESTING {
            return rpc_error(400, &format!("the arguments' JSON {}", crate::value::json_too_deep()));
        }
        let Ok(text) = std::str::from_utf8(json_args) else { return bad() };
        let held = crate::httpd::MEMORY_LIMITED.load(std::sync::atomic::Ordering::Relaxed);
        // A call with no arguments, as most live loaders are, has nothing to read: the reader's own
        // setup cost such a call 2% (FEEDBACK75.md, Q-1052's second review; branch feedback75).
        let args = if text.trim_matches(|c: char| c.is_ascii_whitespace()) == "[]" {
            Vec::new()
        } else {
            // The body as JSON, as the request's start read it; but not a body it read otherwise (a
            // byte-order mark).
            let (parsed, made) = match &read {
                Some(((_, json, _, _), made)) if boundary.is_none() && !text.starts_with('\u{FEFF}') => (json.clone(), *made),
                _ => match Value::parse_json_body(text, held) {
                    Ok(Ok(v)) => v,
                    Ok(Err(_)) => return bad(),
                    Err(_) => return over_memory_answer(),
                },
            };
            // What the marks make is counted with what the JSON made, against `--max-memory`: 3.8 KB
            // of gzip, `[[{"$":["k",0]},...]]`, made 152 MB under 64 MB (FEEDBACK75.md, Q-1052's
            // review; branch feedback75).
            let room = if held { usize::try_from(crate::httpd::memory_limit()).unwrap_or(usize::MAX).saturating_sub(made) } else { usize::MAX };
            let Ok(marked) = crate::value::island_marks_held(&parsed, Some(&files), room) else {
                return over_memory_answer();
            };
            drop(parsed);
            match marked {
                Value::List(a) => Arc::try_unwrap(a).unwrap_or_else(|a| (*a).clone()),
                _ => return bad(),
            }
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
                    // Held as a route's answer is (FEEDBACK71.md, Q-977; branch security).
                    Value::Err(e, _, _) => answer_held("err", &e),
                    v => answer_held("ok", &v),
                },
                Err(Ctrl::Reply(r)) => render_response(&r),
                Err(Ctrl::Error(e)) => {
                    eprintln!("quillo: error in server fn `{}`: {:#}", name, e);
                    rpc_failed(name, &e)
                }
            };
        }
        let had = hdr("x-quillo-had").and_then(crate::livecache::parse_hash);
        let scope = self.run_scope();
        let everyone = crate::livecache::everyone(&scope);
        let key = boundary.is_none().then(|| (scope, name.to_string(), json_args.to_vec(), hdr("host").unwrap_or("").to_string()));
        let (found, mut flying) = match &key {
            Some(k) => self.live_turn(k),
            None => (None, None),
        };
        let (answer, topics, seq, personal) = match found {
            Some((answer, topics, seq)) => {
                crate::metrics::live_run(true);
                self.current_request = None;
                (answer, topics, seq, false)
            }
            None => {
                crate::metrics::live_run(false);
                let seq = self.shared.live_seq();
                self.reads = Some(Default::default());
                self.read_request = false;
                self.acted = false;
                self.own_seqs.clear();
                let cookies = self.set_cookies.len();
                self.in_run = true;
                let res = self.call_closure(&c, args.into_iter().map(Some).collect(), 0);
                self.in_run = false;
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
                        // Held to `--max-memory` where it is set (FEEDBACK71.md, Q-977; branch security).
                        Value::Err(e, _, _) => match crate::livecache::Answer::of(&e, true) {
                            Some(a) => a,
                            None => return over_memory_answer(),
                        },
                        v => match crate::livecache::Answer::of(&v, false) {
                            Some(a) => a,
                            None => return over_memory_answer(),
                        },
                    },
                    Err(Ctrl::Reply(r)) => return render_response(&r),
                    Err(Ctrl::Error(e)) => {
                        eprintln!("quillo: error in server fn `{}`: {:#}", name, e);
                        return rpc_failed(name, &e);
                    }
                });
                // Neither one that read the viewer nor one that acted (see `viewcache::Kind`) can be
                // given to anyone, even the same viewer's other tab, so nothing waits for it
                // either: telling viewers apart took a list of what identifies nobody, and Bob
                // was given `Referer: ...?token=ALICE` (FEEDBACK55.md, Q-881 and Q-888; branch
                // feedback55).
                let nobody_elses = self.read_request || self.acted;
                if let Some(f) = &mut flying {
                    f.personal = nobody_elses;
                }
                // Not shared: a run that wrote, or set a cookie, or read what
                // changed as it ran. What others wrote elsewhere has nothing
                // to do with it (FEEDBACK24.md, Q-191; branch feedback24-fixes).
                if let Some(k) = key {
                    if !nobody_elses && self.set_cookies.len() == cookies && own.is_empty() && self.shared.unchanged_since(&topics, seq) {
                        let mut cache = self.shared.live_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        cache.share(k, seq, topics.clone(), answer.clone());
                    }
                }
                (answer, topics, seen, self.read_request)
            }
        };
        let mut patch = None;
        if !answer.err {
            let owner = if personal { self.who().unwrap_or(everyone) } else { everyone };
            let mut cache = self.shared.live_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            cache.keep(answer.h, &answer.text, owner);
            patch = had.and_then(|from| cache.patch(from, answer.h, &answer.value, [owner, everyone])).filter(|p| p.len() < answer.text.len());
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
    pub(crate) fn live_key(&mut self, name: &str, args: &[Value], scope: &str) -> Option<crate::livecache::RunKey> {
        let read = self.read_request;
        let req = self.request_value();
        self.read_request = read;
        let Some(Value::Map(req)) = req else { return None };
        let Some(Value::Map(headers)) = req.get("headers") else { return None };
        let host = headers.get("host").map(|v| v.display()).unwrap_or_default();
        // What a key cannot hold, folded to null, was any other such argument: a closure over one
        // visitor's value went to the next (FEEDBACK55.md, Q-881; branch feedback55).
        if !args.iter().all(fits_a_key) {
            return None;
        }
        let json = serde_json::Value::Array(args.iter().map(crate::interp::island_json).collect()).to_string().into_bytes();
        Some((scope.to_string(), name.to_string(), json, host))
    }

    /// Whose a run is, for its key: the program, as two mounted programs' `load` are two
    /// functions and `/two` was given `/one`'s run (FEEDBACK55.md, Q-882; branch feedback55), and
    /// the tenant in force, however it was named: `with_tenant(session().org, ...)` around a page
    /// gave Globex "secret of acme" (FEEDBACK55.md, Q-879, found in review; branch feedback55).
    /// With it here a run that used the tenant is that tenant's, read from the request or not.
    pub(crate) fn run_scope(&mut self) -> String {
        match self.tenant() {
            None if self.base.is_empty() => String::new(),
            t => format!("{}\0{}", self.base, t.as_deref().unwrap_or("")),
        }
    }

    /// Who a viewer's kept value is theirs by (see `LiveCache::keep`), worked out if one is kept.
    pub(crate) fn who(&mut self) -> Option<u64> {
        if self.who.is_none() {
            self.who = self.asked().map(who_of);
        }
        self.who
    }

    /// A call of a live loader: a run it may have, or else it runs, its turn taken
    /// (the landing, for those that wait for it). One run at a time of a call.
    pub(crate) fn live_turn(&mut self, key: &crate::livecache::RunKey) -> (Option<crate::livecache::Shared>, Option<Landing>) {
        let shared = self.shared.clone();
        // Each hash of the key made once for the whole turn, and only where it is asked for
        // (FEEDBACK75.md, Q-1057's second review; branch feedback75).
        let ids = crate::livecache::RunIds::of(key);
        let find = || {
            let mut cache = shared.live_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            cache.shared_by(&ids, |t, seq| shared.unchanged_since(t, seq))
        };
        if let Some(f) = find() {
            return (Some(f), None);
        }
        for _ in 0..3 {
            let begun = shared.live_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).begin_by(&ids);
            match begun {
                crate::livecache::Begun::Run => return (None, Some(Landing { shared: shared.clone(), mark: ids.mark(), personal: false })),
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

/// Whether an argument of a loader comes out of its JSON as itself.
fn fits_a_key(v: &Value) -> bool {
    match v {
        Value::Nil | Value::Bool(_) | Value::Int(_) | Value::Dec(_) | Value::Str(_) => true,
        Value::Float(f) => f.is_finite(),
        Value::List(l) => l.iter().all(fits_a_key),
        Value::Map(m) => m.values().all(fits_a_key),
        Value::Func(_) | Value::Builtin(..) | Value::Html(_) | Value::Response(_) | Value::Err(..) => false,
    }
}

/// Whose a kept value is: the address, without the port each connection has its own of, every
/// `Cookie` line and `Authorization`, the same for a page and its refreshes. Two people this
/// cannot tell apart are told apart by the hash of the value, which nobody can work out.
fn who_of(req: &HttpRequest) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = crate::livecache::hasher();
    req.remote.parse::<std::net::SocketAddr>().map_or_else(|_| req.remote.clone(), |a| a.ip().to_string()).hash(&mut h);
    for (k, v) in &req.headers {
        if k.eq_ignore_ascii_case("cookie") || k.eq_ignore_ascii_case("authorization") {
            v.hash(&mut h);
        }
    }
    h.finish()
}

/// A loader's answer past this is said in the log, once for each.
const LIVE_LARGE: usize = 4 << 20;
/// Whole answers from this size keep their gzipped copy.
const LIVE_GZ_KEPT: usize = 16 << 10;

/// The end of a run others may be waiting for, however it ends.
pub(crate) struct Landing {
    shared: Arc<Shared>,
    mark: crate::livecache::RunMark,
    pub personal: bool,
}

impl Drop for Landing {
    fn drop(&mut self) {
        self.shared.live_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).land_by(self.mark, self.personal);
    }
}

/// `/_quillo/live?since=N&s=S&t=a,b`: the sequence number the page has
/// seen, the server it came from, and the topics it is showing; with `tm=1`, the page sends its
/// topics after the address (see `serve_live`).
fn live_query(qs: &str) -> (u64, Option<String>, Vec<String>, bool) {
    let q = parse_urlencoded(qs);
    let since = q.get("since").and_then(|v| v.display().parse::<u64>().ok()).unwrap_or(0);
    let server = q.get("s").map(|v| v.display()).filter(|s| !s.is_empty());
    let topics = q.get("t").map_or_else(Vec::new, |v| topics_of(&v.display()));
    (since, server, topics, q.get("tm").is_some_and(|v| v.display() == "1"))
}

/// The topics a page follows, as `liveLoop` writes them: joined by commas, each with its `%` and
/// `,` written `%25` and `%2C`, so a topic with a comma in it is one topic, where it was two and
/// its page never heard of it (FEEDBACK55.md, Q-801; branch feedback55).
/// A topic longer than any a change can be told of is dropped, as `liveKey` drops it (found in
/// review).
fn topics_of(t: &str) -> Vec<String> {
    t.split(',')
        .filter(|s| !s.is_empty() && s.len() <= MAX_PAGE_TOPIC * 3)
        .map(|s| if s.contains('%') { unescape_topic(s) } else { s.to_string() })
        .filter(|s| s.len() <= MAX_PAGE_TOPIC)
        .take(MAX_TOPICS)
        .collect()
}

fn unescape_topic(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('%') {
        out.push_str(&rest[..at]);
        let code = rest.get(at + 1..at + 3).unwrap_or("");
        let c = if code.eq_ignore_ascii_case("2c") { Some(',') } else if code == "25" { Some('%') } else { None };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[at + 3..];
            }
            None => {
                out.push('%');
                rest = &rest[at + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The longest topic a page follows that a change can be told of: the longest a program names,
/// with room before it for a tenant's name and a mount's path (found in review of FEEDBACK55.md,
/// Q-801; branch feedback55).
const MAX_PAGE_TOPIC: usize = crate::shared::MAX_TOPIC + 192;

/// The most of a page's topics taken after its address (`tm=1`): 256 of the longest, each `%` and
/// `,` in them written as three bytes, as `liveKey` sends them. One page's socket was let hold 8
/// MiB with 4 MiB, and one visitor 2 GiB with 256 sockets (found in review).
const MAX_TOPICS_TEXT: usize = MAX_TOPICS * (MAX_PAGE_TOPIC * 3 + 1);

/// Topics read as they come, a part at a time, as `topics_of` reads them all: a topic too long to be
/// told of is skipped as it comes, not kept (found in review).
#[derive(Default)]
struct TopicReader {
    topics: Vec<String>,
    cur: Vec<u8>,
    skip: bool,
}

impl TopicReader {
    fn take(&mut self, part: &[u8]) {
        for &b in part {
            if b == b',' {
                self.topic();
            } else if !self.skip {
                if self.cur.len() == MAX_PAGE_TOPIC * 3 {
                    self.cur.clear();
                    self.skip = true;
                } else {
                    self.cur.push(b);
                }
            }
        }
    }

    fn topic(&mut self) {
        if !self.skip && !self.cur.is_empty() && self.topics.len() < MAX_TOPICS {
            let t = String::from_utf8_lossy(&self.cur);
            let t = if t.contains('%') { unescape_topic(&t) } else { t.into_owned() };
            if t.len() <= MAX_PAGE_TOPIC {
                self.topics.push(t);
            }
        }
        self.cur.clear();
        self.skip = false;
    }

    fn end(mut self) -> Vec<String> {
        self.topic();
        self.topics
    }
}

/// What a waiting page sent down its socket (`page_said`).
enum PageSaid {
    Nothing,
    Gone,
    /// New topics to follow, from the sequence number and server it gives, as its address gives
    /// them on opening.
    Topics(u64, Option<String>, Vec<String>),
}

/// The longest head of a message of new topics: `since=N&s=S`.
const MAX_TOPICS_HEAD: usize = 128;

/// What a waiting page sent, read now that it rang: a page that moves to a page following other
/// topics sends them down the socket it has, as a line `since=N&s=S` and the topics as its address
/// would give them, where it opened a socket for each page (FEEDBACK61.md, Q-900; branch
/// feedback61). Another text message is passed over, as before; the last set sent counts. A message
/// longer than the most a page follows ends the socket, as a first message does (`ws::Heard::Gone`),
/// so its page opens another and is told there, where it was passed over and the page, taking its
/// topics for followed, heard nothing of them (found in review). Not
/// inlined into the waiting thread's frame, which would hold its reader's buffers (see
/// `ws::Socket::heard`).
/// Its reading's buffers are held only while it reads (see `ws::LiveSocket::heard`). The waiting
/// pages share one thread now, whose memory is used again by the next page, so a large message
/// no longer has its thread's memory given back after it (FEEDBACK66.md, Q-954; branch q954).
async fn page_said(socket: &mut crate::ws::LiveSocket) -> PageSaid {
    let mut said = PageSaid::Nothing;
    loop {
        let mut m = NewTopics::default();
        match socket.heard(MAX_TOPICS_HEAD + MAX_TOPICS_TEXT, Duration::from_secs(10), |part| m.take(part)).await {
            crate::ws::Heard::Nothing => return said,
            crate::ws::Heard::Gone => return PageSaid::Gone,
            crate::ws::Heard::Text(false) => {}
            crate::ws::Heard::Text(true) => {
                if let Some((since, server, topics)) = m.end() {
                    said = PageSaid::Topics(since, server, topics);
                }
            }
        }
    }
}

/// A message of new topics, read a part at a time as `TopicReader` reads them, after its head.
#[derive(Default)]
struct NewTopics {
    head: Vec<u8>,
    headed: bool,
    bad: bool,
    size: usize,
    topics: TopicReader,
}

impl NewTopics {
    fn take(&mut self, mut part: &[u8]) {
        self.size += part.len();
        if !self.headed {
            let end = memchr::memchr(b'\n', part);
            let line = end.map_or(part, |i| &part[..i]);
            if self.head.len() + line.len() > MAX_TOPICS_HEAD {
                self.bad = true;
            } else {
                self.head.extend_from_slice(line);
            }
            let Some(i) = end else { return };
            self.headed = true;
            part = &part[i + 1..];
        }
        if !self.bad {
            self.topics.take(part);
        }
    }

    /// The sequence number, the server and the topics; none for a message that is not one of
    /// topics.
    fn end(self) -> Option<(u64, Option<String>, Vec<String>)> {
        let head = std::str::from_utf8(&self.head).ok().filter(|h| !self.bad && self.headed && h.starts_with("since="))?;
        let (since, server, _, _) = live_query(head);
        Some((since, server, self.topics.end()))
    }
}

/// A live request's topics sent after its address, in its body: past `MAX_TOPICS_TEXT`, a 413.
fn later_topics(body: &[u8]) -> Result<Vec<String>, HttpResponse> {
    if body.len() > MAX_TOPICS_TEXT {
        return Err(plain(413, "more topics than a page follows\n"));
    }
    Ok(topics_of(&String::from_utf8_lossy(body)))
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

/// Most pages that can wait for changes at once (live data, channels and `--watch` reloads).
/// Beyond this, pages are told to try later rather than let anyone with a browser hold
/// connections without end: a page with live data or on a channel is a task now (FEEDBACK66.md,
/// Q-954; branch q954), but each still holds a descriptor, and `--watch`'s pages a thread.
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

/// A place among those for pages that wait (`Waiter`), or the 503 that tells the page to try later.
fn waiting_place(ip: &str) -> Result<Waiter, Answer> {
    Waiter::take(ip).ok_or_else(|| {
        crate::metrics::LIVE_REFUSED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut resp = plain(503, "too many pages are waiting for changes; try again later\n");
        resp.headers.push(("retry-after".into(), "5".into()));
        Answer::Respond(resp)
    })
}

/// Hands the connection to `f` on a thread of its own, holding one of the
/// waiting places; answers 503 when they are all taken, or the client at
/// `ip` holds its share of them.
fn waiter(name: &'static str, ip: &str, f: impl FnOnce(std::net::TcpStream) + Send + 'static) -> Answer {
    let place = match waiting_place(ip) {
        Ok(p) => p,
        Err(refused) => return refused,
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

/// With `takes`, the first message of a socket that takes new topics (see `page_said`), which says
/// so: a page sends its topics down a socket only when it was told, and one of a server that does not
/// take them (an older one, or one whose sockets are not watched) opens a socket for them as before
/// (FEEDBACK61.md, Q-900; branch feedback61).
fn live_json(shared: &Shared, seq: u64, changed: &[String], takes: bool) -> String {
    if takes {
        return serde_json::json!({ "seq": seq, "server": shared.id, "changed": changed, "takes": "topics" }).to_string();
    }
    serde_json::json!({ "seq": seq, "server": shared.id, "changed": changed }).to_string()
}

/// How often a waiting page is pinged (and what it sent looked at, in case a wake was lost), and
/// how long after a change the next may go. A socket is no longer looked at every 5 s where it
/// cannot be watched: the pages' runtime watches every one (FEEDBACK66.md, Q-954; branch q954).
const LIVE_PING: Duration = Duration::from_secs(20);
const LIVE_GATHER: Duration = Duration::from_millis(100);

/// Answers `/_quillo/live` on a thread of its own, so open pages do not hold
/// on to the server's threads. Over a WebSocket, each change to the topics
/// is sent as `{seq, server, changed}` for as long as the page stays; as a
/// plain request, the answer is the first change, or none after a while.
fn serve_live(shared: Arc<Shared>, req: HttpRequest, (tenants, tenant): (bool, Option<String>)) -> Answer {
    let qs = req.url.split_once('?').map_or("", |(_, q)| q);
    let (since, server, topics, later) = live_query(qs);
    // `tk=1`: the page asks whether its socket takes topics, and is told at once (see `live_json`).
    let tk = parse_urlencoded(qs).get("tk").is_some_and(|v| v.display() == "1");
    // Topics too many or too long for an address come after it: the first message of the socket,
    // or the body of a POST (FEEDBACK55.md, Q-801; branch feedback55).
    let upgrade = crate::ws::is_upgrade(&req);
    let topics = if later && !upgrade {
        match later_topics(&req.body) {
            Ok(t) => t,
            Err(r) => return Answer::Respond(r),
        }
    } else {
        topics
    };
    // The page's bell, which a change to what it follows rings: made here (see `Bell::new`). A
    // waiting page is a task on the pages' runtime, where it was a thread, 240 KB a page, most of
    // it kept after the page closed (FEEDBACK66.md, Q-954; branch q954); the handshake's answer
    // is made here, so that the task holds none of the request.
    let bell = crate::shared::Bell::new();
    let answer = upgrade.then(|| crate::ws::handshake(&req));
    let place = match waiting_place(&req.remote) {
        Ok(p) => p,
        Err(refused) => return refused,
    };
    Answer::Detach(Box::new(move |stream, mut before| {
        // With no runtime (none could be made), or a connection the runtime cannot take, the page
        // is hung up on, and asks again.
        let Some(rt) = crate::wt::pages() else { return };
        if stream.set_nonblocking(true).is_err() {
            return;
        }
        rt.spawn(async move {
            let _place = place;
            let _page = crate::metrics::LivePage::new();
            let Ok(mut stream) = tokio::net::TcpStream::from_std(stream) else { return };
            let own = |topics| own_topics(topics, tenants, tenant.as_deref());
            let Some(answer) = answer else {
                // A long poll: what changed within 25 s, after the answers before it on its
                // connection, which go at once, not held for the 25 s (found by its test).
                if !before.is_empty() {
                    if !matches!(tokio::time::timeout(Duration::from_secs(30), tokio::io::AsyncWriteExt::write_all(&mut stream, &before)).await, Ok(Ok(()))) {
                        return;
                    }
                    before.clear();
                }
                let following = shared.follow(own(topics), &bell);
                let (seq, changed) = following.wait_async(since, server.as_deref(), Duration::from_secs(25)).await;
                drop(following);
                let mut resp = json_response(200, serde_json::from_str(&live_json(&shared, seq, &changed, false)).unwrap_or_default());
                resp.headers.push(("cache-control".into(), "no-store".into()));
                crate::httpd::write_response(&mut before, &resp, false, true);
                let _ = tokio::time::timeout(Duration::from_secs(30), tokio::io::AsyncWriteExt::write_all(&mut stream, &before)).await;
                return;
            };
            // Earlier answers on this connection, then the handshake's, or its refusal.
            let upgraded = answer.is_ok();
            before.extend_from_slice(answer.as_ref().map_or_else(|r| r.as_bytes(), |a| a.as_bytes()));
            let wrote = tokio::time::timeout(Duration::from_secs(30), tokio::io::AsyncWriteExt::write_all(&mut stream, &before)).await;
            if !upgraded || !matches!(wrote, Ok(Ok(()))) {
                return;
            }
            drop(before);
            let mut socket = crate::ws::LiveSocket::new(stream);
            let topics = if later {
                let mut read = TopicReader::default();
                if !socket.first_text_parts(MAX_TOPICS_TEXT, Duration::from_secs(10), |part| read.take(part)).await {
                    return;
                }
                read.end()
            } else {
                topics
            };
            live_page(&shared, socket, &bell, own(topics), (since, server, tk), &own).await;
        });
    }))
}

/// A page with live data, on its WebSocket, as a task: told of each change to what it follows, and
/// of new topics it sends down the socket, at once. The socket is heard the moment the page sends
/// something: a close is answered at once, and topics sent down it are followed at once, where a
/// page that moved opened a socket for each page and closed the last, each close answered by
/// nothing until the next look, 5 s on (FEEDBACK61.md, Q-900; branch feedback61). Every socket is
/// watched now, as the runtime watches it, so the page is told at once that its socket takes topics.
async fn live_page(shared: &Arc<Shared>, mut socket: crate::ws::LiveSocket, bell: &Arc<crate::shared::Bell>, topics: Vec<String>, (mut since, mut server, tk): (u64, Option<String>, bool), own: &impl Fn(Vec<String>) -> Vec<String>) {
    let mut following = shared.follow(topics, bell);
    // The page is yet to be told that its socket takes topics: at once if it asks (`tk=1`), in its
    // first message otherwise.
    let mut takes = true;
    // The page sent topics, or asked whether it may, and is answered at once with where things stand.
    let mut asked = tk;
    // Looked at too at each ping, in case a wake was lost.
    let mut ping_at = Instant::now() + LIVE_PING;
    let mut next = Instant::now();
    // The page's one timer, till its next ping, set again as that moves (each change moves it). Set
    // only once it was done, its deadline passed before the runtime had fired it left the wait
    // nothing to wait on, and the task went round with no pause till the ping, holding every page's
    // thread (found in review); a wait that ends at the ping is now followed by a ping or a change
    // told, either of which moves it.
    let timer = tokio::time::sleep_until(ping_at.into());
    tokio::pin!(timer);
    loop {
        let (mut seq, mut changed) = if asked {
            following.now(since, server.as_deref())
        } else {
            if timer.deadline() != ping_at.into() {
                timer.as_mut().reset(ping_at.into());
            }
            let waited = tokio::select! {
                r = following.wait_until(since, server.as_deref(), timer.as_mut()) => Some(r),
                _ = socket.readable() => None,
            };
            match waited {
                Some(r) => r,
                None => {
                    bell.ring();
                    following.now(since, server.as_deref())
                }
            }
        };
        if bell.take() || Instant::now() >= ping_at {
            match page_said(&mut socket).await {
                PageSaid::Gone => return,
                PageSaid::Nothing => {}
                PageSaid::Topics(s, srv, topics) => {
                    following = shared.follow(own(topics), bell);
                    (since, server, asked) = (s, srv, true);
                    continue;
                }
            }
        }
        // A page that did not know the sequence number, or knew one
        // from another server or from before a restart, is told the
        // current one.
        if !asked && changed.is_empty() && since != 0 && since <= seq && server.is_none() {
            if Instant::now() >= ping_at {
                if !socket.ping().await {
                    return;
                }
                ping_at = Instant::now() + LIVE_PING;
            }
        } else {
            asked = false;
            // The first change goes at once; what follows it closely is
            // gathered into one frame, so a burst of writes is one
            // refresh (FEEDBACK24.md, Q-190; branch feedback24-fixes).
            let early = next.saturating_duration_since(Instant::now());
            if !early.is_zero() {
                tokio::time::sleep(early).await;
                (seq, changed) = following.now(since, server.as_deref());
            }
            if !socket.send(&live_json(shared, seq, &changed, std::mem::take(&mut takes))).await {
                return;
            }
            next = Instant::now() + LIVE_GATHER + Duration::from_millis(fastrand::u64(..50));
            ping_at = Instant::now() + LIVE_PING;
        }
        since = seq;
        server = None;
    }
}

/// A page's channels over a WebSocket (where WebTransport cannot be had), as tasks on the pages'
/// runtime for as long as the page stays (see `wt::pages`).
fn serve_channels(req: HttpRequest) -> Answer {
    let Some(ticket) = crate::wt::ticket_of(req.url.split_once('?').map_or("", |(_, q)| q)) else {
        return Answer::Respond(plain(403, "no ticket, or an old one\n"));
    };
    // A task on the pages' runtime, not a thread (FEEDBACK66.md, Q-954; branch q954): the
    // handshake's answer is made here, so that the task holds none of the request.
    let answer = crate::ws::handshake(&req);
    let place = match waiting_place(&req.remote) {
        Ok(p) => p,
        Err(refused) => return refused,
    };
    Answer::Detach(Box::new(move |stream, mut before| {
        // With no runtime (none could be made), or a connection the runtime cannot take, the page
        // is hung up on, and connects again.
        let Some(rt) = crate::wt::pages() else { return };
        if stream.set_nonblocking(true).is_err() {
            return;
        }
        rt.spawn(async move {
            let _place = place;
            let Ok(mut stream) = tokio::net::TcpStream::from_std(stream) else { return };
            // Earlier answers on this connection, then the handshake's, or its refusal.
            let upgraded = answer.is_ok();
            before.extend_from_slice(answer.as_ref().map_or_else(|r| r.as_bytes(), |a| a.as_bytes()));
            let wrote = tokio::time::timeout(Duration::from_secs(30), tokio::io::AsyncWriteExt::write_all(&mut stream, &before)).await;
            if !upgraded || !matches!(wrote, Ok(Ok(()))) {
                return;
            }
            drop(before);
            crate::wt::over_websocket(stream, ticket).await;
        });
    }))
}

/// The arguments a browser (or another service) calls a `server fn` with,
/// checked all the way in (list items, map values and record fields, since
/// the function trusts its types) and made into its parameters' types as
/// `Type(v)` makes them: a record keeps only its own fields, so what is
/// sent cannot add `is_admin` to one, and fields left out get their defaults.
/// A `dec` (a parameter or a record's field) takes a decimal, an int, a
/// number or numeric text ("12.50"), as a `dec` pattern takes it.
pub(crate) fn take_args(it: &mut Interp, def: &crate::ast::FnDef, args: Vec<Value>) -> Result<Vec<Value>, String> {
    make_args(it, def, args).unwrap_or_else(|(p, e)| match e {
        Ctrl::Error(e) => Err(format!("argument `{}` of `{}`: {}", p, def.name, e)),
        _ => Err(format!("argument `{}` of `{}` could not be made", p, def.name)),
    })
}

/// `take_args`, with a failure while an argument is made (a record's default that failed) kept
/// as the failure it is, with the parameter's name: the MCP door makes a tool's arguments as its
/// body runs, and such a failure is the body's (FEEDBACK56.md, Q-858, found in review; branch
/// feedback56).
pub(crate) fn make_args(it: &mut Interp, def: &crate::ast::FnDef, args: Vec<Value>) -> Result<Result<Vec<Value>, String>, (String, Ctrl)> {
    if args.len() > def.params.len() {
        return Ok(Err(format!("`{}` takes {} argument{}, got {}", def.name, def.params.len(), if def.params.len() == 1 { "" } else { "s" }, args.len())));
    }
    if let Some(p) = def.params.get(args.len()..).and_then(|rest| rest.iter().find(|p| p.default.is_none())) {
        return Ok(Err(format!("missing argument `{}` for `{}`", p.name, def.name)));
    }
    let mut out = Vec::with_capacity(args.len());
    for (v, p) in args.into_iter().zip(&def.params) {
        match make_arg(it, def, p, v)? {
            Ok(x) => out.push(x),
            Err(why) => return Ok(Err(why)),
        }
    }
    Ok(Ok(out))
}

/// One argument of `def` made to its parameter's type, as `make_args` makes each: the MCP door,
/// which takes them by name and may leave one out before another, makes each with it, so that
/// both doors say the same (FEEDBACK56.md, C.1; branch feedback56).
pub(crate) fn make_arg(it: &mut Interp, def: &crate::ast::FnDef, p: &crate::ast::Param, v: Value) -> Result<Result<Value, String>, (String, Ctrl)> {
    let Some(ty) = &p.ty else { return Ok(Ok(v)) };
    match crate::record::make(it, ty, &v, false) {
        Ok(Ok(x)) => Ok(Ok(x)),
        Ok(Err(why)) => Ok(Err(format!("argument `{}` of `{}` must be {}, got {}{}", p.name, def.name, ty.text(), v.type_name(), crate::interp::because(&why)))),
        Err(e) => Err((p.name.to_string(), e)),
    }
}

fn json_response(status: u16, j: serde_json::Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![("content-type".into(), "application/json".into())],
        body: j.to_string().into_bytes(),
    }
}

/// A 200 of JSON `j`, its text held to `--max-memory` as it is written: past it, a 500 that says
/// so, as a route's answer is ended (FEEDBACK71.md, Q-977; branch security).
/// A server fn's answer, `{"ok": v}` (or `err`), where the limit is set written straight into
/// its body held to `--max-memory`, with no tree made first: past it, a 500 that says so, as a
/// route's answer is ended (FEEDBACK71.md, Q-977, found in review: the tree of a list of 200
/// long texts took 408 MB first; branch security).
fn answer_held(key: &str, v: &Value) -> HttpResponse {
    if !crate::httpd::MEMORY_LIMITED.load(std::sync::atomic::Ordering::Relaxed) {
        return json_response(200, serde_json::json!({ key: crate::interp::island_json(v) }));
    }
    let mut body = format!("{{\"{}\":", key).into_bytes();
    if v.island_into_held(&mut body) && crate::value::bytes_room(&mut body, 1) {
        body.push(b'}');
        return HttpResponse { status: 200, headers: vec![("content-type".into(), "application/json".into())], body };
    }
    over_memory_answer()
}

/// The answer to a server fn call past `--max-memory`, and its line in the log.
fn over_memory_answer() -> HttpResponse {
    crate::httpd::error_line(&crate::httpd::over_memory());
    rpc_error(500, &crate::httpd::over_memory_said_to_pages())
}

fn rpc_error(status: u16, msg: &str) -> HttpResponse {
    json_response(status, serde_json::json!({ "error": msg }))
}

/// What a `server fn` that failed says to the page: under `--watch` the error, its place and the
/// calls it came through, as a route's page does, so the page's error value and the console say
/// what the log does; else only that it failed, the message being for the program's author
/// (FEEDBACK64.md, Q-933; branch feedback64: it had said "internal error in `down`" under `--watch`
/// too).
/// A `server fn` that failed (its error, or a run past `--max-compute`), answered so: its words
/// (see `rpc_failure`), with `x-quillo-answer: failed`, the server's own mark, which a page's `live`
/// does not try again. A program's `reply 500 { error }` or `respond(500, ...)` is the same answer
/// otherwise, and is the program's to mean: it had been taken for this, and not tried again
/// (FEEDBACK61.md, Q-899's second review; branch feedback61).
fn rpc_failed(name: &str, e: &crate::error::QuilloError) -> HttpResponse {
    let mut r = rpc_error(500, &rpc_failure(name, e));
    r.headers.push(("x-quillo-answer".into(), "failed".into()));
    r
}

pub(crate) fn rpc_failure(name: &str, e: &crate::error::QuilloError) -> String {
    if SHOW_ERRORS.load(std::sync::atomic::Ordering::Relaxed) {
        format!("{:#}", e)
    } else {
        format!("internal error in `{}`", name)
    }
}

/// A `Cache-Control` that a shared cache may keep, made one only the visitor's browser keeps:
/// `private` first, without `public`, `s-maxage` and `proxy-revalidate`, which are for shared
/// caches, and the rest as it was (`max-age`, `immutable`, `no-cache`). None for one that is
/// already the visitor's (`private`, `no-store`) or that no cache keeps without asking (FEEDBACK59.md,
/// Q-903; branch cache-and-migrate).
pub(crate) fn kept_by_visitor_alone(v: &str) -> Option<String> {
    // A route gives the same header each time: the last answer is kept, on each thread.
    thread_local! {
        static LAST: std::cell::RefCell<(String, Option<String>)> = const { std::cell::RefCell::new((String::new(), None)) };
    }
    if let Some(r) = LAST.with(|l| l.try_borrow().ok().filter(|l| l.0 == v).map(|l| l.1.clone())) {
        return r;
    }
    let r = work_out_private(v);
    LAST.with(|l| {
        if let Ok(mut l) = l.try_borrow_mut() {
            *l = (v.to_string(), r.clone());
        }
    });
    r
}

fn work_out_private(v: &str) -> Option<String> {
    let parts = directives_of(v);
    let is = |p: &str, n: &str| p.split('=').next().unwrap_or("").trim().eq_ignore_ascii_case(n);
    // `private` alone: one naming fields (`private="set-cookie"`) lets a shared cache keep the
    // rest (found in review).
    if parts.iter().any(|p| p.eq_ignore_ascii_case("private") || is(p, "no-store")) {
        return None;
    }
    // Any other: a shared cache may keep what says no time too, by an `Expires` or by its own
    // reckoning, and one that says `no-cache` (found in review).
    let kept: Vec<&str> = parts.into_iter().filter(|p| !(is(p, "public") || is(p, "s-maxage") || is(p, "proxy-revalidate") || is(p, "private"))).collect();
    Some(if kept.is_empty() { "private".to_string() } else { format!("private, {}", kept.join(", ")) })
}

/// A `Cache-Control`'s directives, split at the commas outside quotes: `private="a, b"` is one.
fn directives_of(v: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut quoted) = (0, false);
    for (i, c) in v.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                out.push(v[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(v[start..].trim());
    out.retain(|p| !p.is_empty());
    out
}

/// The paths `said_private` has said, the first 64.
static SAID_PRIVATE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();

thread_local! {
    /// The last path this thread found said, so that a route asked for again takes no lock.
    static SAID_HERE: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Whether there is nothing more to say for `path`: said already, or 64 said.
fn said_here(path: &str) -> bool {
    if SAID_HERE.with(|s| s.try_borrow().is_ok_and(|s| *s == path)) {
        return true;
    }
    let said = SAID_PRIVATE.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let done = said.len() >= 64 || said.contains(path);
    drop(said);
    if done {
        SAID_HERE.with(|s| {
            if let Ok(mut s) = s.try_borrow_mut() {
                s.clear();
                s.push_str(path);
            }
        });
    }
    done
}

/// Said once for each path, the first 64, where an answer's `public` was made `private`; what
/// changed is put in words only then.
fn said_private(path: &str, what: impl FnOnce() -> String) {
    let mut said = SAID_PRIVATE.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if said.len() < 64 && !said.contains(path) {
        said.insert(path.to_string());
        drop(said);
        // The path as a visitor wrote it, decoded: a line break in it would write a line of its
        // own in the log (found in review).
        let shown: String = path.chars().map(|c| if c.is_control() { c.escape_debug().to_string() } else { c.to_string() }).collect();
        eprintln!("quillo: {} read who the visitor is: {}, as a cache in front would give the answer to the next visitor (said once for each path)", shown, what());
    }
}

/// Whether a page has a `<script>` element whose
/// code starts with `code`: one put in already, as a mounted program's page
/// has it, not text that quotes it, which is escaped.
fn script_with(body: &[u8], code: &[u8]) -> bool {
    finder(code).find_iter(body).any(|i| body[..i].ends_with(b"<script>"))
}

/// What `page_tags` finds in a page.
#[derive(Default, Clone, Copy, PartialEq, Debug)]
struct PageTags {
    /// A link to prefetch, or a script that may make one: an `<a>` tag (`<a` and a space, a `>` or a
    /// `/`) or a `<script`, in any case (branch page-size). A `<` in the page's text is `&lt;`, so
    /// only markup is seen; one in a comment or an attribute only keeps the prefetch script.
    follow: bool,
    /// A stream's player, `<quillo-media` (as `has_media` finds it).
    media: bool,
}

/// `PageTags` of `body`, in one pass: a `<` and, after it, an `a`, `s` or `q` of either case, sixteen
/// places at a time where the processor compares sixteen at once, then each such place looked at.
/// Five searches had found these, one for each form (`<a`, `<A`, `<script`, `<SCRIPT`,
/// `<quillo-media`), 2.4% of the Fortunes page's time together (branch speed-fortunes).
fn page_tags(b: &[u8]) -> PageTags {
    let mut t = PageTags::default();
    let look = |p: usize, t: &mut PageTags| {
        let rest = &b[p + 1..];
        match rest.first().map(|c| c | 0x20) {
            Some(b'a') => t.follow |= matches!(rest.get(1), Some(b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'>' | b'/')),
            Some(b's') => t.follow |= rest.len() >= 6 && rest[..6].eq_ignore_ascii_case(b"script"),
            Some(b'q') => t.media |= rest.starts_with(b"quillo-media"),
            _ => {}
        }
    };
    let n = b.len();
    let mut i = 0;
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    while i + 17 <= n {
        // SAFETY: 17 bytes from `i` are inside `b`.
        let mut bits = unsafe { tag_starts16(b.as_ptr().add(i)) };
        while bits != 0 {
            look(i + (bits.trailing_zeros() / STRIDE) as usize, &mut t);
            bits &= !(((1u64 << STRIDE) - 1) << (bits.trailing_zeros() / STRIDE * STRIDE));
        }
        // A link found: only the player is still asked about, by one search to the end, not by a
        // look at each `<a` and `<s` after it (found in review).
        if t.follow {
            static MEDIA: std::sync::LazyLock<memchr::memmem::Finder<'static>> = std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"<quillo-media"));
            t.media |= MEDIA.find(&b[i + 16..]).is_some();
            return t;
        }
        i += 16;
    }
    while i + 1 < n {
        if b[i] == b'<' && matches!(b[i + 1] | 0x20, b'a' | b's' | b'q') {
            look(i, &mut t);
        }
        i += 1;
    }
    t
}

/// See `crate::interp`'s `STRIDE`: bits of a mask a byte.
#[cfg(target_arch = "aarch64")]
const STRIDE: u32 = 4;
#[cfg(target_arch = "x86_64")]
const STRIDE: u32 = 1;

/// `STRIDE` bits for each of the 16 places from `p` where a `<` is followed by `a`, `s` or `q` in
/// either case.
///
/// # Safety
/// `p` must be valid for reads of 17 bytes.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn tag_starts16(p: *const u8) -> u64 {
    use std::arch::aarch64::*;
    let (v0, v1) = (vld1q_u8(p), vorrq_u8(vld1q_u8(p.add(1)), vdupq_n_u8(0x20)));
    let next = vorrq_u8(vorrq_u8(vceqq_u8(v1, vdupq_n_u8(b'a')), vceqq_u8(v1, vdupq_n_u8(b's'))), vceqq_u8(v1, vdupq_n_u8(b'q')));
    let m = vandq_u8(vceqq_u8(v0, vdupq_n_u8(b'<')), next);
    vget_lane_u64::<0>(vreinterpret_u64_u8(vshrn_n_u16::<4>(vreinterpretq_u16_u8(m))))
}

/// See the aarch64 `tag_starts16`.
///
/// # Safety
/// `p` must be valid for reads of 17 bytes.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn tag_starts16(p: *const u8) -> u64 {
    use std::arch::x86_64::*;
    let c = |x: u8| _mm_set1_epi8(x as i8);
    let (v0, v1) = (_mm_loadu_si128(p.cast()), _mm_or_si128(_mm_loadu_si128(p.add(1).cast()), c(0x20)));
    let next = _mm_or_si128(_mm_or_si128(_mm_cmpeq_epi8(v1, c(b'a')), _mm_cmpeq_epi8(v1, c(b's'))), _mm_cmpeq_epi8(v1, c(b'q')));
    _mm_movemask_epi8(_mm_and_si128(_mm_cmpeq_epi8(v0, c(b'<')), next)) as u32 as u64
}

/// What `page_tags` gives `follow` for, as it was found before it (for the tests): four searches.
#[cfg(test)]
fn may_follow(body: &[u8]) -> bool {
    use memchr::memmem::Finder;
    let link = |i: usize| matches!(body.get(i + 2), Some(b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'>' | b'/'));
    Finder::new(b"<a").find_iter(body).any(link) || Finder::new(b"<A").find_iter(body).any(link) || Finder::new(b"<script").find(body).is_some() || Finder::new(b"<SCRIPT").find(body).is_some()
}

/// `bytes` put into `body` at `at`: a move and a copy. `Vec::splice` put them in a byte at a
/// time, the prefetch script's 1,300 on each page (2026-11-09, the server benchmark).
fn insert_at(body: &mut Vec<u8>, at: usize, bytes: &[u8]) {
    let len = body.len();
    body.resize(len + bytes.len(), 0);
    body.copy_within(at..len, at + bytes.len());
    body[at..at + bytes.len()].copy_from_slice(bytes);
}

/// The searcher for one of the needles the page's checks look for, made once each: the
/// prefetch script's start and the two scripts' paths, the most a page asks for (2026-11-09, the
/// server benchmark: each search had made its own). Any other is made for the call.
fn finder(needle: &[u8]) -> std::borrow::Cow<'static, memchr::memmem::Finder<'static>> {
    use std::sync::LazyLock;
    static PREFETCH: LazyLock<memchr::memmem::Finder<'static>> = LazyLock::new(|| memchr::memmem::Finder::new(&PREFETCH_JS.as_bytes()[..40]));
    static MEDIA: LazyLock<memchr::memmem::Finder<'static>> = LazyLock::new(|| memchr::memmem::Finder::new(b"/_quillo/media.js"));
    static BUNDLE: LazyLock<memchr::memmem::Finder<'static>> = LazyLock::new(|| memchr::memmem::Finder::new(b"/_quillo/quillo.js"));
    for f in [&*PREFETCH, &*MEDIA, &*BUNDLE] {
        if f.needle() == needle {
            return std::borrow::Cow::Borrowed(f);
        }
    }
    std::borrow::Cow::Owned(memchr::memmem::Finder::new(needle).into_owned())
}

/// Where the last `needle` is in `body`, for the page's own tags, whose searchers are made once:
/// `</body>` (2026-11-09, the server benchmark: each search had made its own).
fn rfind_tag(body: &[u8], needle: &[u8]) -> Option<usize> {
    static BODY: std::sync::LazyLock<memchr::memmem::FinderRev<'static>> = std::sync::LazyLock::new(|| memchr::memmem::FinderRev::new(b"</body>"));
    match needle {
        b"</body>" => BODY.rfind(body),
        _ => memchr::memmem::rfind(body, needle),
    }
}

/// Whether a page has a `<quillo-media` in it, searched for from the start: memchr's search
/// forwards has a SIMD prefilter and its search backwards none, so going back from the end through
/// a page with no player, as most are, was 3% of a page's CPU (branch speed-bun, 2026-11-09, the
/// Fortunes benchmark). Only whether there is one is asked, not where.
#[cfg(test)]
fn has_media(body: &[u8]) -> bool {
    static MEDIA: std::sync::LazyLock<memchr::memmem::Finder<'static>> = std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"<quillo-media"));
    MEDIA.find(body).is_some()
}

/// Where `needle` (lower case, starting with a byte that has no case, as `<` does) first is in
/// `body`, its letters in either case: `</HEAD>` is `</head>`. By the first byte's places, which
/// memchr finds, not byte by byte: the page's scans had been a third of a page's time
/// (2026-11-09, the server benchmark).
fn find_ascii_ci(body: &[u8], needle: &[u8]) -> Option<usize> {
    let (&first, _) = needle.split_first()?;
    memchr::memchr_iter(first, body).find(|&i| body.len() - i >= needle.len() && body[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

/// Whether a page has a `<script>` element whose `src` ends with `path`
/// (the program wrote it, or a mounted program's page already has it).
/// Only an element counts: text, which is escaped, cannot start one, nor
/// can `markdown` (whose `<img src=...>` could name the same path), so a
/// comment saying `/_quillo/quillo.js` does not stop the page loading it.
fn script_tag_for(body: &[u8], path: &[u8]) -> bool {
    for i in finder(path).find_iter(body) {
        let from = i + path.len();
        if !matches!(body.get(from), Some(b'"' | b'\'' | b'?' | b'#')) {
            continue;
        }
        let Some(lt) = memchr::memrchr(b'<', &body[..i]) else { continue };
        let tag = &body[lt..i];
        if tag.len() > 7 && tag[..7].eq_ignore_ascii_case(b"<script") && matches!(tag[7], b' ' | b'\t' | b'\n' | b'/') && !tag.contains(&b'>') {
            return true;
        }
    }
    false
}

/// Whether a value has an error in it that a visitor is not to read, at any depth.
fn holds_hidden_error(v: &Value) -> bool {
    holds_hidden_at(v, 0)
}

/// `holds_hidden_error` of `v`, `depth` levels down: past 256 levels on a new piece of stack when
/// this one runs low, as the JSON writers go (`value::deeper`), where a list 20,000 deep given to
/// `json.stringify` or `str` in a `parallel` call's function, on its stack of 512 KB, stopped the
/// server; a scalar in a list or map is looked at where it is, with no call (coverage of 0.8.7,
/// Q-1110; branch coverage-087).
fn holds_hidden_at(v: &Value, depth: usize) -> bool {
    match v {
        Value::Err(e, own, _) => !own || hidden_below(e, depth),
        Value::List(l) => {
            for x in l.iter() {
                if hidden_below(x, depth) {
                    return true;
                }
            }
            false
        }
        Value::Map(m) => {
            for x in m.values() {
                if hidden_below(x, depth) {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

/// `holds_hidden_at` of `x`, one level below `depth`, called only for what can hold an error.
#[inline(always)]
fn hidden_below(x: &Value, depth: usize) -> bool {
    matches!(x, Value::Err(..) | Value::List(_) | Value::Map(_)) && crate::value::deeper(depth + 1, || holds_hidden_at(x, depth + 1))
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
    without_hidden_at(v, 0)
}

/// `without_hidden_errors` of `v`, `depth` levels down, on a new piece of stack past 256 levels
/// as `holds_hidden_at` goes (coverage of 0.8.7, Q-1110; branch coverage-087).
fn without_hidden_at(v: &Value, depth: usize) -> Value {
    let inner = |x: &Value| crate::value::deeper(depth + 1, || without_hidden_at(x, depth + 1));
    match v {
        Value::Err(e, false, at) => {
            log_hidden(e, at);
            Value::err(Value::str("internal error"))
        }
        Value::Err(e, true, _) => Value::own_err(inner(e)),
        Value::List(l) => Value::list(l.iter().map(inner).collect()),
        Value::Map(m) => Value::map(m.iter().map(|(k, x)| (k.clone(), inner(x))).collect()),
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

/// `PREFETCH_JS` that fetches the links and writes no speculation rules: for a page whose policy
/// refuses inline scripts, where Chrome refuses the rules written for each link even with
/// `'inline-speculation-rules'`, as a hash beside it turns that off (FEEDBACK59.md, Q-904, found
/// in review; branch cache-and-migrate).
fn prefetch_by_fetch() -> &'static str {
    static MADE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    MADE.get_or_init(|| PREFETCH_JS.replace("const spec=HTMLScriptElement.supports&&HTMLScriptElement.supports(\"speculationrules\");", "const spec=false;"))
}

/// What the prefetch puts before a page's `</body>`, made once for a program's base and kept by
/// each thread: it had been made, 1,300 bytes, for each page (2026-11-09, the server benchmark).
fn prefetch_tags(base: &str) -> std::rc::Rc<[u8]> {
    thread_local! {
        static MADE: std::cell::RefCell<Option<(String, bool, std::rc::Rc<[u8]>)>> = const { std::cell::RefCell::new(None) };
    }
    let counted = crate::metrics::ENABLED.load(std::sync::atomic::Ordering::Relaxed);
    MADE.with(|m| {
        let mut m = m.borrow_mut();
        if let Some((_, _, tags)) = m.as_ref().filter(|(b, c, _)| b == base && *c == counted) {
            return tags.clone();
        }
        let tags: std::rc::Rc<[u8]> = format!("<script>{}</script>{}", PREFETCH_JS, used_script(base)).into_bytes().into();
        *m = Some((base.to_string(), counted, tags.clone()));
        tags
    })
}

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
/// A page Quillo's worker keeps as it installs (`x-quillo-prefetch: 1` from its fetch, as
/// `PREFETCH_JS`'s): a prefetch, whatever `--no-prefetch` says (FEEDBACK67.md, B.2, second review;
/// branch pwa).
#[cold]
fn precache(req: &HttpRequest) -> bool {
    req.method == "GET" && req.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("x-quillo-prefetch") && v == "1")
}

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
                // Held to `--max-memory` as it is written, as every text a request makes: the
                // answer's body is a value too (FEEDBACK71.md, Q-977, found in review: 200 long
                // texts made 180 MB of JSON under a 1 MB limit; branch security). Only where the
                // limit is set: a compare for each piece cost a JSON answer 100 to 270
                // instructions more.
                let mut out = Vec::new();
                let v = hidden(other);
                let v = v.as_ref().unwrap_or(other);
                let written = if crate::httpd::MEMORY_LIMITED.load(std::sync::atomic::Ordering::Relaxed) {
                    v.json_into_held(&mut out, crate::builtins::JSON_MOST, crate::builtins::JSON_DEEPEST)
                } else {
                    v.json_into_within(&mut out, crate::builtins::JSON_MOST, crate::builtins::JSON_DEEPEST).map_err(Some)
                };
                if let Err(why) = written {
                    let why = why.unwrap_or_else(crate::httpd::over_memory);
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
    /// Saves that did not load while a version that did is served: how many, and what the last
    /// one said (none once one loads), for the pages' `err:` messages (FEEDBACK54.md, Q-686;
    /// branch hmr).
    failed: Mutex<(u64, Option<String>)>,
    /// Under --watch, the version a save replaced: a page made with it asks whether the save
    /// changed it, and is answered by both versions' renders of what is there now, so that data
    /// changed since the page was made is no change of the program's (FEEDBACK54.md, Q-685;
    /// branch hmr).
    replaced: Mutex<Option<(u64, Arc<Loaded>)>>,
    /// The next reload carries nothing of the server's data: `r` and Enter in the terminal under
    /// --watch (FEEDBACK54.md, Part C's open question; the user, 2026-11-16: "proceed").
    fresh: std::sync::atomic::AtomicBool,
    /// The files the last load that failed read or tried, watched with the version served
    /// until one loads (FEEDBACK55.md, Q-836; branch feedback55).
    tried: Mutex<Vec<PathBuf>>,
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
            failed: Mutex::new((0, None)),
            replaced: Mutex::new(None),
            fresh: std::sync::atomic::AtomicBool::new(false),
            tried: Mutex::new(Vec::new()),
        }
    }

    /// The files a first load that failed read or tried (see `tried`).
    pub fn tried(&self, files: Vec<PathBuf>) {
        *self.tried.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = files;
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
    /// fails to load does not replace one that works. Whether it loaded.
    pub fn reload(&self) -> bool {
        self.reload_saying(true)
    }

    /// `reload`; `say` false for the other instances of `--instances`, which load what the first
    /// has just loaded and said (FEEDBACK55.md, Q-841; branch feedback55).
    fn reload_saying(&self, say: bool) -> bool {
        let mut problems = String::new();
        // The program's tools, as the checker read them, for its MCP door (FEEDBACK56.md, C.2;
        // branch feedback56).
        let mut tools = None;
        let (_, mut tried) = crate::error::noting_reads(|| {
            if self.static_check {
                let analysis = crate::check::check_file(&self.main_file);
                for d in &analysis.diagnostics {
                    eprintln!("{}", d);
                }
                problems = analysis.errors().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
                tools = Some(crate::mcp::tools(&analysis));
            }
        });
        // Said once: the checker's lines above are the errors (FEEDBACK55.md, Q-852; branch
        // feedback55).
        let checked_out = !problems.is_empty();
        // The version this one replaces, whose data a save under --watch carries (Part C).
        let before = if self.watch { Some(self.current().1) } else { None };
        let mut carried = String::new();
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
            let (res, read) = crate::error::noting_reads(|| it.load_file(&self.main_file));
            tried.extend(read);
            res
        } else {
            Err(crate::error::QuilloError::new(problems, 0))
        };
        let fresh = self.fresh.swap(false, std::sync::atomic::Ordering::Relaxed);
        if let (Ok(_), Some(before), false) = (&res, &before, fresh) {
            if let Loaded::Ready(old) = &**before {
                carried = carry_data(old, &it);
            }
        }
        if fresh {
            carried = "; its data started again (r)".to_string();
        }
        self.shared.gil.release();
        // What a failed load read or tried is watched until one loads (FEEDBACK55.md, Q-836;
        // branch feedback55).
        self.tried(if res.is_ok() { Vec::new() } else { tried });
        let mut slot = self.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let loaded = res.is_ok();
        match res {
            Ok(_) => {
                // "1 route", as when it starts (FEEDBACK55.md, Q-852; branch feedback55).
                let n = it.routes.iter().filter(|r| r.method != crate::ast::GUARD_METHOD).count();
                if say {
                    eprintln!("quillo: reloaded {} ({} route{}){}", self.main_file.display(), n, if n == 1 { "" } else { "s" }, carried);
                }
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
                if self.watch {
                    *self.replaced.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(slot.clone());
                }
                // What its app.toml says, as it is now (FEEDBACK67.md, B.1; branch pwa).
                if !self.is_service {
                    crate::pwa::attach(&mut it, &self.base_dir);
                }
                crate::mcp::open_door(&mut it, &self.main_file, tools);
                let loaded = Arc::new(Loaded::Ready(it));
                warm_client(&loaded);
                *slot = (slot.0 + 1, loaded);
                let mut f = self.failed.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if f.1.take().is_some() {
                    f.0 += 1;
                }
            }
            Err(e) => {
                if !checked_out {
                    eprintln!("quillo: {}", e);
                }
                if matches!(*slot.1, Loaded::Failed(_)) {
                    *slot = (slot.0 + 1, Arc::new(Loaded::Failed(e.to_string())));
                } else {
                    eprintln!("quillo: still serving the previous version");
                    // The open pages say so, and keep what they hold (FEEDBACK54.md, Q-686;
                    // branch hmr).
                    let mut f = self.failed.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    *f = (f.0 + 1, Some(e.to_string()));
                }
            }
        }
        self.changed.notify_all();
        loaded
    }

    /// Waits up to `timeout` for a version newer than `seen`, a stylesheet changed after
    /// `css_seen`, or a save that failed (or one that loaded after it) since `failed_seen`: the
    /// version, the stylesheets' paths when they changed, and the failure when it did (its
    /// count, and what it said; none once a save loaded).
    fn wait_for_change(&self, seen: u64, css_seen: u64, failed_seen: u64, timeout: Duration) -> (u64, Option<(u64, Vec<String>)>, Option<(u64, Option<String>)>) {
        let slot = self.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (slot, _) = self
            .changed
            .wait_timeout_while(slot, timeout, |s| {
                s.0 == seen && self.css.lock().map_or(true, |c| c.0 == css_seen) && self.failed.lock().map_or(true, |f| f.0 == failed_seen)
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let v = slot.0;
        drop(slot);
        let css = self.css_seq();
        let failed = self.failed.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        (v, (css.0 != css_seen).then_some(css), (failed.0 != failed_seen).then_some(failed))
    }

    fn watched_files(&self) -> Vec<PathBuf> {
        let mut files = vec![self.main_file.clone()];
        // The app.toml beside it, there or not (making one is a change), and the icon it names
        // (FEEDBACK67.md, B.1; branch pwa).
        let mut app = vec![self.base_dir.join(crate::pwa::FILE)];
        if let Loaded::Ready(it) = &*self.current().1 {
            files.extend(it.files());
            // Not one in a folder that is not there, which the system cannot watch: it fell back to
            // looking every 50 ms (found in review). `quillo check` says the icon is missing.
            let icon = it.site.pwa.as_ref().and_then(|w| w.icon_file()).filter(|f| f.parent().is_none_or(|d| d.as_os_str().is_empty() || d.is_dir()));
            app.extend(icon.map(Path::to_path_buf));
        }
        for f in app {
            if !files.contains(&f) {
                files.push(f);
            }
        }
        // Under --watch, the folders `db.migrate` read and their files: a migration a `git pull`
        // brings is applied by the reload, where the program ran on the old schema until a `.qwe`
        // file changed; and one that stopped the first load is tried again once fixed. Not under
        // --reload, where a deploy decides; nor a folder that is the program's own, where every
        // database write would reload it (FEEDBACK62.md, Q-919; branch cache-and-migrate).
        if self.watch {
            let own = |d: &Path| d.canonicalize().ok();
            let program = self.main_file.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
            for dir in crate::migrate::folders() {
                // One not there yet is watched too, as making it is a change (found in review).
                let same = match (own(&dir), own(program)) {
                    (Some(a), Some(b)) => a == b,
                    _ => dir == program,
                };
                if same || files.contains(&dir) {
                    continue;
                }
                let mut sql: Vec<PathBuf> = std::fs::read_dir(&dir)
                    .map(|es| es.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "sql") && p.is_file()).collect())
                    .unwrap_or_default();
                sql.sort();
                files.push(dir);
                files.extend(sql);
            }
        }
        // And what the last load that failed named: a used file fixed, or one it lacked made,
        // loads it (FEEDBACK55.md, Q-836; branch feedback55).
        for f in self.tried.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter() {
            if !files.contains(f) {
                files.push(f.clone());
            }
        }
        files
    }
}

/// Under --watch, what the version a save replaced held, carried into the new one (FEEDBACK54.md,
/// Part C and Q-687, the user's rules of 2026-11-04 and 2026-11-15; branch hmr): each top-level
/// `let` the program changes somewhere (its data: one it never changes is code, and follows the
/// edit), when the new version still has it, its value is plain data (no function, handle or
/// response in it), and the new one starts as the same kind of value (or as null). What was kept
/// and what started again is said on the reload's line, and so is a `:memory:` database, which
/// starts again empty (the user's choice). Mounted programs and services start again.
fn carry_data(old: &Interp, new: &Interp) -> String {
    fn plain(v: &Value) -> bool {
        match v {
            Value::Nil | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Dec(_) | Value::Str(_) | Value::Html(_) => true,
            Value::List(xs) => xs.iter().all(plain),
            Value::Map(m) => m.values().all(plain),
            Value::Err(v, ..) => plain(v),
            Value::Func(_) | Value::Builtin(..) | Value::Response(_) => false,
        }
    }
    let mut assigned = std::collections::HashSet::new();
    let mut lets = Vec::new();
    let mut memory = false;
    for stmts in new.program.iter() {
        crate::check::global_assigned(stmts, &mut assigned);
        for st in stmts.iter() {
            if let crate::ast::Stmt::Let(p, ..) = st {
                crate::check::pattern_names(p, &mut lets);
            }
        }
        crate::check::each_expr_of(stmts, &mut |e| memory |= matches!(e, crate::ast::Expr::Lit(crate::ast::Lit::Str(s)) if &**s == ":memory:"));
    }
    let (mut kept, mut reset) = (Vec::new(), Vec::new());
    lets.dedup();
    for name in lets.iter().filter(|n| assigned.contains(n.as_str())) {
        let (Some(was), Some(now)) = (crate::value::lookup(&old.globals, name.as_str()), crate::value::lookup(&new.globals, name.as_str())) else { continue };
        if !plain(&was) {
            reset.push(format!("{} (it holds {})", name, was.type_name()));
        } else if !matches!(now, Value::Nil) && now.type_name() != was.type_name() {
            reset.push(format!("{} (its type changed: {} to {})", name, was.type_name(), now.type_name()));
        } else {
            crate::value::assign(&new.globals, name.as_str(), was);
            kept.push(name.clone());
        }
    }
    let mut line = String::new();
    if !kept.is_empty() {
        line.push_str(&format!("; kept {}", kept.join(", ")));
    }
    if !reset.is_empty() {
        line.push_str(&format!("; started again: {}", reset.join(", ")));
    }
    if memory {
        line.push_str("; the :memory: database starts again empty (open a file to keep its rows across saves)");
    }
    line
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
/// Elsewhere, and when the system refuses (a folder missing, too many open),
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
                // A file not there yet (one a load that failed lacked): its folder, which tells
                // when the file is made (FEEDBACK55.md, Q-836; branch feedback55).
                let dir = f.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
                let f = if !f.exists() && dir.is_dir() { dir } else { f.as_path() };
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
    watcher_of(app, Vec::new())
}

/// `watcher`, reloading `others` too once `app` has loaded: the instances of `--instances`, which
/// serve one version (FEEDBACK55.md, Q-841; branch feedback55).
fn watcher_of(app: Arc<App>, others: Vec<Arc<App>>) -> impl FnOnce() + Send + 'static {
    let files = app.watched_files();
    let seen = mtimes(&files);
    move || watch(app, others, files, seen)
}

fn watch(app: Arc<App>, others: Vec<Arc<App>>, mut files: Vec<PathBuf>, mut seen: Vec<(Option<std::time::SystemTime>, Option<PathBuf>)>) {
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
            // The files as the reload is about to read them: what changes while it runs (a
            // migration that arrives as the one before it is applied) is a change still, where
            // times read after it took it for seen and it waited for another change
            // (FEEDBACK62.md, Q-919, found by CI; branch cache-and-migrate).
            let before: std::collections::HashMap<PathBuf, (Option<std::time::SystemTime>, Option<PathBuf>)> = files.iter().cloned().zip(mtimes(&files)).collect();
            if app.reload() {
                for o in &others {
                    o.reload_saying(false);
                }
            }
            files = app.watched_files();
            let after = mtimes(&files);
            seen = files.iter().zip(after).map(|(f, t)| before.get(f).cloned().unwrap_or(t)).collect();
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
/// feedback11-fixes). `err:N:"..."` is a save that did not load.
/// What a page's HTML is, the address of its bundle aside (which names its code, compared on
/// its own): a page told of a new version asks for itself again, and is not reloaded when this
/// and its code are as they were (FEEDBACK54.md, Q-685; branch hmr). Made with the version the
/// save replaced, the server renders that version too and gives its print (`x-quillo-was`),
/// which the page compares instead of its own: data changed since then changes both alike.
/// Live data's version too (`data-v="<seq>:<server>"`, which any change of data moves, a reload's
/// too): the page's live states follow it themselves (found by Quillo DevTools' tests). And what an
/// island holds, but its component and props: its code is compared on its own (the bundle's ids),
/// and a component swapped in place makes it again (FEEDBACK54.md, A.2).
fn page_print(body: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut rest = body;
    loop {
        let next = [rest.find("/_quillo/b/"), rest.find(" data-v=\""), rest.find("<quillo-island")];
        let Some((which, at)) = next.iter().enumerate().filter_map(|(k, i)| i.map(|i| (k, i))).min_by_key(|(_, i)| *i) else { break };
        rest[..at].hash(&mut h);
        rest = &rest[at..];
        match which {
            0 => rest = rest["/_quillo/b/".len()..].trim_start_matches(|c: char| c.is_ascii_hexdigit()),
            1 => {
                let after = &rest[" data-v=\"".len()..];
                rest = after.find('"').map_or("", |e| &after[e + 1..]);
            }
            _ => {
                let tag_end = rest.find('>').map_or(rest.len(), |e| e + 1);
                let tag = &rest[..tag_end];
                for a in [" data-c=\"", " data-p=\""] {
                    if let Some(i) = tag.find(a) {
                        let v = &tag[i + a.len()..];
                        v[..v.find('"').unwrap_or(v.len())].hash(&mut h);
                    }
                }
                // To its own end, past the islands inside it.
                let mut depth = 1usize;
                let mut at = tag_end;
                while depth > 0 {
                    let (open, close) = (rest[at..].find("<quillo-island"), rest[at..].find("</quillo-island>"));
                    match (open, close) {
                        (Some(o), Some(c)) if o < c => {
                            depth += 1;
                            at += o + "<quillo-island".len();
                        }
                        (_, Some(c)) => {
                            depth -= 1;
                            at += c + "</quillo-island>".len();
                        }
                        _ => {
                            at = rest.len();
                            break;
                        }
                    }
                }
                rest = &rest[at..];
            }
        }
    }
    rest.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// `--watch=reload`: every change reloads the open pages, as before HMR (FEEDBACK54.md, Q-685,
/// the user's choice 2026-11-15; branch hmr).
pub static PLAIN_RELOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn dev_script(version: u64, print: &str) -> String {
    // A save that did not load (`err:N:"..."`, `null` once one does) is said over the page, in
    // a shadow root the page's styles do not reach; the page is not reloaded, so what it holds
    // stays (FEEDBACK54.md, Q-686; branch hmr). What the page decided at a new version is told
    // to tools as a `quillo:hmr` event on the document: { what: "kept" | "reload", why: "html" |
    // "code" | "off" | "failed", version }, or { what: "swapped", why: the files } when the
    // changed components were swapped in place (A.2), or "morphed" when the page's own HTML was
    // put in place (A.3) (Quillo DevTools shows it). A page loaded after a
    // reload that kept what it held starts its code at once, to put it back (A.6).
    // A page a service worker answers from its cache reloaded for ever (--watch=reload), or asked
    // for itself each second and stayed old, saying nothing: a fetch of itself that carries the
    // version the page has, when the server told another, came from a cache, and reloads (why:
    // "cache"). The version a reload is from (the page's own, as it loaded) and the one it is for
    // are kept for the next load, which reads them once (sessionStorage, `quillo-reload:` and the
    // page's path and query); a page that comes back with the version it reloaded from, a worker
    // controlling it, reloads no more for that version and says why, over the page and in the
    // console ({ what: "cached" }) (FEEDBACK67.md, Q-957; branch pwa). By equality, never by
    // order: a restart counts from 1 again, below the version a worker kept, which reloaded for
    // ever (found in review).
    const SCRIPT: &str = r#"<script>/*quillo-dev*/(()=>{let v='@V@',h='@H@';const v0=v,HOT=@HOT@;let c=0,e=0,box=null;const K='quillo-reload:'+location.pathname+location.search;let stale='';try{const s=JSON.parse(sessionStorage.getItem(K)||'null');sessionStorage.removeItem(K);if(s&&s.from===v0&&navigator.serviceWorker.controller)stale=s.for}catch(_){}const css=j=>{const p=JSON.parse(j);for(const l of document.querySelectorAll('link[rel=stylesheet]')){const u=new URL(l.href,location.href);if(u.origin===location.origin&&p.includes(u.pathname)){u.searchParams.set('quillo',Date.now());const n=l.cloneNode();n.href=u.pathname+u.search;n.onload=()=>l.remove();l.after(n)}}};const said=m=>{if(m===null){if(box){box.remove();box=null}return}if(!box){box=document.createElement('quillo-error');const r=box.attachShadow({mode:'open'});r.innerHTML='<style>:host{all:initial;position:fixed;left:0;right:0;bottom:0;z-index:2147483647;font:13px/1.45 ui-monospace,Menlo,monospace}div{position:relative;margin:8px;padding:12px 44px 12px 14px;background:#fff1f0;color:#5c0000;border:2px solid #c62828;border-radius:6px;max-height:45vh;overflow:auto;box-shadow:0 4px 18px #0005}pre{margin:8px 0 0;white-space:pre-wrap;font:inherit}button{position:absolute;top:8px;right:10px;border:0;background:none;font-size:20px;line-height:1;cursor:pointer;color:inherit}</style><div role="alert"><b>This save did not load: the page shows the version before it.</b><pre></pre><button aria-label="Close">×</button></div>';r.querySelector('button').onclick=()=>{box.remove();box=null};document.documentElement.appendChild(box)}box.shadowRoot.querySelector('pre').textContent=m};const plain=()=>{const w=window[Symbol.for('quillo')]||{};return !HOT||w.hot===false||document.documentElement.hasAttribute('data-reload')};const ids=t=>{const m=/\.ids = (\{.*?\});/.exec(t);return m?JSON.parse(m[1]):null};const tell=(what,why,ver)=>{try{document.dispatchEvent(new CustomEvent('quillo:hmr',{detail:JSON.stringify({what,why,version:ver})}))}catch(_){}};const again=(why,ver)=>{ver=String(ver);if(stale&&ver===stale){v=ver;const m="This page came from a service worker's cache (`navigator.serviceWorker.controller`), so `--watch` cannot refresh it: unregister the worker (DevTools > Application > Service workers), or have it go to the server first for pages";console.warn(m);said(m);return tell('cached',why,ver)}tell('reload',why,ver);if(why!=='off')try{const w=window[Symbol.for('quillo')];if(w&&w.keep)w.keep()}catch(_){}try{sessionStorage.setItem(K,JSON.stringify({from:v0,for:ver}))}catch(_){}location.reload()};let busy=null;const hot=t=>busy||(busy=(async()=>{if(plain())return again('off',t);try{const r=await fetch(location.href,{cache:'no-store',credentials:'same-origin',headers:{'x-quillo-hot':v}});const p=await r.text();const was=r.headers.get('x-quillo-was');const m=/\/\*quillo-dev\*\/\(\(\)=>\{let v='(\d+)',h='([0-9a-f]+)'/.exec(p)||[],v2=m[1],h2=m[2];if(!r.ok||!v2)return again('failed',t);if(v2===v)return again('cache',t);let morphed=false;if(was?was!==h2:h2!==h){const w0=window[Symbol.for('quillo')]||{};let ok=false;try{ok=!!(w0.morph&&w0.morph(p))}catch(x){console.error(x)}if(!ok)return again('html',v2);morphed=true}h=h2;const mine=(window[Symbol.for('quillo')]||{}).ids;if(mine){const b=(/\/_quillo\/b\/[0-9a-f]+\.js/.exec(p)||[])[0];const lt=b?await (await fetch(b,{cache:'no-store'})).text():'';const theirs=b?ids(lt):null;if(!theirs)return again('failed',v2);const changed=Object.keys(theirs).filter(k=>mine[k]!==theirs[k]);if(changed.length){const w=window[Symbol.for('quillo')]||{};if(!w.swap||changed.includes('quillo.js')||changed.includes('boot.js')||Object.keys(mine).some(k=>!(k in theirs)))return again('code',v2);const at=(/const at = "([^"]*)"/.exec(lt)||[])[1],list=JSON.parse('['+((/for \(const f of \[([^\]]*)\]/.exec(lt)||[])[1]||'')+']');const urls=changed.map(k=>list.find(f=>f.slice(f.indexOf('/')+1)===k)).filter(Boolean).map(f=>at+f);if(urls.length!==changed.length)return again('code',v2);try{await w.swap(urls);w.ids=theirs}catch(x){console.error(x);return again('code',v2)}v=v2;said(null);return tell('swapped',changed.join(', ')+(morphed?'; the page morphed':''),v2)}}v=v2;said(null);tell(morphed?'morphed':'kept','',v2)}catch(x){again('failed',t)}})().finally(()=>{busy=null}));const seen=t=>{if(t.startsWith('css:')){const i=t.indexOf(':',4);c=+t.slice(4,i)||c;css(t.slice(i+1))}else if(t.startsWith('err:')){const i=t.indexOf(':',4);e=+t.slice(4,i)||e;said(JSON.parse(t.slice(i+1)))}else if(t!==v)return hot(t)};const u=()=>'/_quillo/events?v='+v+'&c='+c+'&e='+e,go=()=>{let o=0;const s=new WebSocket(location.origin.replace(/^http/,'ws')+u());s.onopen=()=>o=1;s.onmessage=e=>seen(e.data);s.onclose=()=>o?setTimeout(go,1000):fetch(u(),{cache:'no-store'}).then(r=>r.text()).then(async t=>{await seen(t);go()},()=>setTimeout(go,1000))};go();try{if(sessionStorage.getItem('quillo-keep:'+location.pathname+location.search)){const i=document.querySelector('quillo-island,[data-qi]');if(i)i.dispatchEvent(new Event('focusin',{bubbles:true}))}}catch(_){}})()</script>"#;
    let hot = !PLAIN_RELOAD.load(std::sync::atomic::Ordering::Relaxed);
    SCRIPT.replace("@V@", &version.to_string()).replace("@H@", print).replace("@HOT@", if hot { "1" } else { "0" })
}

/// Answers `/_quillo/events[?v=N]` on a thread of its own, so pages waiting
/// for a change do not hold on to the server's threads.
fn serve_events(app: Arc<App>, req: HttpRequest) -> Answer {
    let q = parse_urlencoded(req.url.split_once('?').map_or("", |(_, q)| q));
    let seen = q.get("v").and_then(|v| v.display().parse::<u64>().ok());
    // The stylesheet changes the page has taken (`css:N:[...]` messages);
    // none given: the ones so far.
    let css_seen = q.get("c").and_then(|v| v.display().parse::<u64>().ok()).filter(|c| *c > 0).unwrap_or_else(|| app.css_seq().0);
    // The failed saves the page was told of (`err:N:...`); 0, a page just loaded: told of the one
    // standing, if one does (FEEDBACK54.md, Q-686; branch hmr).
    let failed_seen = q.get("e").and_then(|v| v.display().parse::<u64>().ok()).unwrap_or(0);
    let failed_seen = {
        let f = app.failed.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if failed_seen == 0 && f.1.is_none() { f.0 } else { failed_seen }
    };
    let css_text = |(n, paths): (u64, Vec<String>)| format!("css:{}:{}", n, serde_json::json!(paths));
    let failed_text = |(n, said): (u64, Option<String>)| format!("err:{}:{}", n, serde_json::json!(said));
    waiter("quillo-events", &req.remote.clone(), move |stream| {
        if crate::ws::is_upgrade(&req) {
            let Some(mut socket) = crate::ws::Socket::accept(&req, stream) else { return };
            // A page that does not read is let go, not waited on for good (FEEDBACK49.md,
            // Q-594; found in review).
            socket.write_timeout(Duration::from_secs(30));
            let seen = seen.unwrap_or(0);
            let (mut css_seen, mut failed_seen) = (css_seen, failed_seen);
            loop {
                let (v, css, failed) = app.wait_for_change(seen, css_seen, failed_seen, Duration::from_secs(20));
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
                if let Some(f) = failed {
                    failed_seen = f.0;
                    if !socket.send(&failed_text(f)) {
                        return;
                    }
                    continue;
                }
                if !socket.ping() {
                    return;
                }
            }
        }
        let (v, css, failed) = match seen {
            Some(seen) => app.wait_for_change(seen, css_seen, failed_seen, Duration::from_secs(25)),
            None => (app.current().0, None, None),
        };
        let text = match (css, failed) {
            (Some(c), _) if Some(v) == seen => css_text(c),
            (_, Some(f)) if Some(v) == seen => failed_text(f),
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
pub(crate) fn as_written(resp: &HttpResponse) -> bool {
    // A header named `null` is none (FEEDBACK55.md, Q-710; branch feedback55).
    resp.status == 206 || resp.sent("etag").is_some() || resp.sent("content-range").is_some() || written_miss(resp)
}

/// Marks the page `files()` sends for a miss: a file, sent as its bytes are, which has lost its
/// `ETag` (that of another URL's answer, Q-715) and so looked like a page a route made, and was
/// given the prefetching script and `--watch`'s (FEEDBACK69.md, Q-964; branch q964-miss-page).
/// Only `files()` sets it (`respond`, `stream`, `events` and `site_headers` refuse it), a
/// `reply` with another status drops it, a program reading the answer's headers does not see
/// it, and `answer` takes it off the 404s it is on: it is never sent.
pub(crate) const AS_WRITTEN: &str = "x-quillo-as-written";

/// Whether the answer is a miss's page, sent as written. Only a 404 looks, so a page a route made
/// pays one comparison (FEEDBACK69.md, Q-964; branch q964-miss-page).
fn written_miss(resp: &HttpResponse) -> bool {
    resp.status == 404 && resp.header(AS_WRITTEN).is_some()
}

fn inject_dev_script(resp: &mut HttpResponse, version: u64) {
    let html = is_html(resp);
    if !html || as_written(resp) {
        return;
    }
    let body = String::from_utf8_lossy(&resp.body).into_owned();
    let script = dev_script(version, &page_print(&body));
    resp.body = match body.rfind("</body>") {
        Some(i) => format!("{}{}{}", &body[..i], script, &body[i..]),
        None => format!("{}{}", body, script),
    }
    .into_bytes();
    // Through the page's policy too, and the style of its box for a save that did not load
    // (FEEDBACK59.md, Q-904; branch cache-and-migrate).
    if resp.headers.iter().any(|(k, v)| is_policy(k) && v != DROPPED) {
        let style = script.split_once("<style>").and_then(|(_, r)| r.split_once("</style>")).map(|(t, _)| t.to_string());
        allow_own(resp, &script_texts(script.as_bytes()), &style.iter().map(String::as_str).collect::<Vec<_>>(), false);
    }
}

/// What an answer's content policy refuses of what Quillo writes into a page itself: inline
/// scripts, and a `style` attribute (a `<quillo-island>`'s `display:contents`). The guide's own
/// `default-src 'self'` refused both: the page looked right and no component ran, with no word
/// anywhere (FEEDBACK59.md, Q-904; branch cache-and-migrate).
#[derive(Default, Clone, Copy)]
pub(crate) struct Refused {
    pub scripts: bool,
    pub attr: bool,
}

/// The content policies an answer carries, the reporting ones too, which would report every one
/// of Quillo's scripts.
fn is_policy(name: &str) -> bool {
    name.eq_ignore_ascii_case("content-security-policy") || name.eq_ignore_ascii_case("content-security-policy-report-only")
}

/// A policy's directives, in order: the name in lower case and its sources.
fn directives(policy: &str) -> Vec<(String, Vec<&str>)> {
    policy
        .split(';')
        .filter_map(|d| {
            let mut words = d.split_ascii_whitespace();
            let name = words.next()?.to_ascii_lowercase();
            Some((name, words.collect()))
        })
        .collect()
}

/// The directive that decides for a kind of thing: the first of `names` the policy has.
fn deciding<'a>(ds: &'a [(String, Vec<&'a str>)], names: &[&str]) -> Option<&'a [&'a str]> {
    names.iter().find_map(|n| ds.iter().find(|(d, _)| d == n).map(|(_, v)| v.as_slice()))
}

/// Whether sources let an inline script or style run: `'unsafe-inline'`, which a nonce or a hash
/// beside it turns off, and for scripts `'strict-dynamic'` too (for styles it means nothing:
/// found in review, where a hash added beside it turned the program's own styles off).
fn lets_inline(sources: &[&str], scripts: bool) -> bool {
    let has = |p: &str| sources.iter().any(|s| s.to_ascii_lowercase().starts_with(p));
    has("'unsafe-inline'") && !has("'nonce-") && !has("'sha256-") && !has("'sha384-") && !has("'sha512-") && !(scripts && has("'strict-dynamic'"))
}

const SCRIPTS: &[&str] = &["script-src-elem", "script-src", "default-src"];
const STYLE_ELEMS: &[&str] = &["style-src-elem", "style-src", "default-src"];
const STYLE_ATTRS: &[&str] = &["style-src-attr", "style-src", "default-src"];

/// Each policy of the answer (a header may hold several, by commas) refuses or not.
pub(crate) fn policy_refuses(resp: &HttpResponse) -> Refused {
    let mut r = Refused::default();
    for (_, v) in resp.headers.iter().filter(|(k, v)| is_policy(k) && v != DROPPED) {
        for policy in v.split(',') {
            let ds = directives(policy);
            let refuses = |names: &[&str]| deciding(&ds, names).is_some_and(|s| !lets_inline(s, names == SCRIPTS));
            r.scripts |= refuses(SCRIPTS);
            r.attr |= refuses(STYLE_ATTRS);
        }
    }
    r
}

/// The text of each `<script>` element in markup Quillo wrote.
fn script_texts(tags: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(tags);
    let mut out = Vec::new();
    let mut rest = &*text;
    while let Some(i) = rest.find("<script>") {
        let after = &rest[i + 8..];
        let Some(end) = after.find("</script>") else { break };
        out.push(after[..end].to_string());
        rest = &after[end + 9..];
    }
    out
}

/// The rule a `<quillo-island>` takes from a `<style>` when the policy refuses its attribute.
const ISLAND_RULE: &str = "quillo-island{display:contents}";

/// Islands without their `style` attribute, for a page whose policy refuses it: whether there
/// was one.
pub(crate) fn plain_islands(body: &mut Vec<u8>) -> bool {
    const WITH: &[u8] = b"<quillo-island style=\"display:contents\"";
    if !body.windows(WITH.len()).any(|w| w == WITH) {
        return false;
    }
    *body = replace_bytes(body, WITH, b"<quillo-island");
    true
}

/// `body` with each `from` in it made `to`, as bytes: a page need not be UTF-8 (found in review).
fn replace_bytes(body: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if body[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(body[i]);
            i += 1;
        }
    }
    out
}

pub(crate) fn source_hash(text: &str) -> String {
    use sha2::Digest;
    // Most are the same on every page (the prefetch, the swap, the rule): the last few are kept,
    // on each thread.
    thread_local! {
        static KEPT: std::cell::RefCell<Vec<(String, String)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    if let Some(h) = KEPT.with(|k| k.borrow().iter().find(|(t, _)| t == text).map(|(_, h)| h.clone())) {
        return h;
    }
    let h = format!("'sha256-{}'", crate::ws::base64(&sha2::Sha256::digest(text.as_bytes())));
    KEPT.with(|k| {
        let mut k = k.borrow_mut();
        if k.len() >= 16 {
            k.remove(0);
        }
        k.push((text.to_string(), h.clone()));
    });
    h
}

/// A policy with `hashes` added where `names` decides, unless that already lets inline code
/// through (a hash would then turn its `'unsafe-inline'` off). A policy with only `default-src`
/// gets the last of `names` made from it, which keeps everything else as it was.
fn with_hashes(policy: &str, names: &[&str], hashes: &[String]) -> String {
    let ds = directives(policy);
    let Some(sources) = deciding(&ds, names) else { return policy.to_string() };
    if lets_inline(sources, names == SCRIPTS) || hashes.is_empty() {
        return policy.to_string();
    }
    let named = names.iter().position(|n| ds.iter().any(|(d, _)| d == n)).map(|i| names[i]).unwrap_or("default-src");
    let mut sources: Vec<String> = sources.iter().filter(|s| !s.eq_ignore_ascii_case("'none'")).map(|s| s.to_string()).collect();
    for h in hashes {
        if !sources.contains(h) {
            sources.push(h.clone());
        }
    }
    let line = |name: &str| format!("{} {}", name, sources.join(" "));
    if named == "default-src" {
        let made = names[names.len() - 2];
        let mut out = policy.trim_end().trim_end_matches(';').to_string();
        out.push_str("; ");
        out.push_str(&line(made));
        return out;
    }
    policy
        .split(';')
        .map(|d| if d.split_ascii_whitespace().next().is_some_and(|n| n.eq_ignore_ascii_case(named)) { format!(" {}", line(named)) } else { d.to_string() })
        .collect::<Vec<_>>()
        .join(";")
        .trim_start()
        .to_string()
}

/// Quillo's own scripts through each policy of the answer, by their hashes, which a kept page
/// keeps valid where a nonce would not; and, for a page whose policy refuses an island's
/// attribute, its rule in a `<style>` at the head, hashed where the policy wants that too.
pub(crate) fn allow_own(resp: &mut HttpResponse, scripts: &[String], styles: &[&str], island_style: bool) {
    let mut script_hashes: Vec<String> = Vec::new();
    for t in scripts {
        let h = source_hash(t);
        if !script_hashes.contains(&h) {
            script_hashes.push(h);
        }
    }
    let mut style_hashes: Vec<String> = styles.iter().map(|t| source_hash(t)).collect();
    if island_style {
        style_hashes.push(source_hash(ISLAND_RULE));
    }
    for (k, v) in resp.headers.iter_mut() {
        if !is_policy(k) || *v == DROPPED {
            continue;
        }
        *v = v
            .split(',')
            .map(|p| with_hashes(&with_hashes(p, SCRIPTS, &script_hashes), STYLE_ELEMS, &style_hashes))
            .collect::<Vec<_>>()
            .join(",");
    }
    if island_style {
        let tag = format!("<style>{}</style>", ISLAND_RULE);
        let find = |needle: &[u8]| find_ascii_ci(&resp.body, needle);
        let after = |at: usize| resp.body[at..].iter().position(|&c| c == b'>').map_or(0, |e| at + e + 1);
        // Never before a `<!doctype>`, which would put the page in quirks mode (found in review).
        let at = match (find(b"</head>"), find(b"<body"), find(b"<html"), find(b"<!doctype")) {
            (Some(h), _, _, _) => h,
            (None, Some(b), _, _) => after(b),
            (None, None, Some(h), _) => after(h),
            (None, None, None, Some(d)) => after(d),
            (None, None, None, None) => 0,
        };
        insert_at(&mut resp.body, at, tag.as_bytes());
    }
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

/// Largest body `/_quillo/markdown` takes, in bytes (FEEDBACK55.md, Q-706; branch feedback55).
pub const MARKDOWN_BODY: usize = 1 << 20;

/// The bytes that can begin or end an element of Markdown's: each may be a node of
/// pulldown-cmark's tree, which with the HTML made from it took up to about 200 bytes for each
/// (`[`, `<`, `>`; a heading's, link's, table's or list's marks and new lines about the same),
/// where plain words take a few (FEEDBACK75.md, Q-1069; branch feedback75).
const MARKDOWN_MARKS: [bool; 256] = {
    let mut t = [false; 256];
    let marks = b"[]<>*_!|&`\\~#-+.():\n";
    let mut i = 0;
    while i < marks.len() {
        t[marks[i] as usize] = true;
        i += 1;
    }
    t
};

/// What making `text` into HTML takes at most, worked out from it line by line (FEEDBACK75.md,
/// Q-1069 and its second review; branch feedback75):
/// - six bytes for each byte: the text, its HTML as it grows, the answer (plain words took four);
/// - `MARKDOWN_MARK_BYTES` for each byte that can begin or end an element;
/// - `MARKDOWN_CONTROL_BYTES` for each control character, which the answer writes `\u0001`;
/// - `MARKDOWN_CODE_BYTES` for each byte inside a fenced code block, which is highlighted: its
///   characters, its spans and the HTML made of them. A fence in a quote or a list item is taken
///   to run to the end, where pulldown-cmark may end it sooner;
/// - `MARKDOWN_CELL_BYTES` for each cell of each row of a table, those its header has and a row
///   leaves out (a row with no `|` at all) too.
pub fn markdown_room(text: &str) -> usize {
    let mut room = text.len().saturating_mul(6);
    // The fence being read: its character, its length, and whether a line can close it.
    let mut fence: Option<(u8, usize, bool)> = None;
    let (mut cols, mut pipes_before) = (0usize, false);
    for line in text.as_bytes().split_inclusive(|&c| c == b'\n') {
        if let Some((c, n, closes)) = fence {
            if closes && fence_closes(line, c, n) {
                fence = None;
            } else {
                room = room.saturating_add(line.len().saturating_mul(MARKDOWN_CODE_BYTES));
            }
            continue;
        }
        let (mut marks, mut controls) = (0usize, 0usize);
        for &b in line {
            marks += MARKDOWN_MARKS[b as usize] as usize;
            controls += (b < 0x20 && !matches!(b, b'\n' | b'\r' | b'\t')) as usize;
        }
        room = room.saturating_add(marks.saturating_mul(MARKDOWN_MARK_BYTES)).saturating_add(controls.saturating_mul(MARKDOWN_CONTROL_BYTES));
        if let Some(open) = fence_opens(line) {
            fence = Some(open);
            (cols, pipes_before) = (0, false);
            continue;
        }
        let blank = line.iter().all(u8::is_ascii_whitespace);
        if cols > 0 && !blank {
            room = room.saturating_add(cols.saturating_mul(MARKDOWN_CELL_BYTES));
        } else if blank {
            cols = 0;
        } else if let Some(n) = table_delimiter(line).filter(|_| pipes_before) {
            cols = n;
        }
        pipes_before = line.contains(&b'|');
    }
    room
}

/// Where `line` opens a fenced code block: its character, its length, and whether a later line
/// can close it as this reckons, which only one at the top (up to three spaces before it) can.
fn fence_opens(line: &[u8]) -> Option<(u8, usize, bool)> {
    let at = line.iter().position(|b| !matches!(b, b' ' | b'\t' | b'>' | b'-' | b'*' | b'+' | b'.' | b')' | b'0'..=b'9'))?;
    let c = *line.get(at).filter(|c| matches!(c, b'`' | b'~'))?;
    let n = line[at..].iter().take_while(|&&b| b == c).count();
    let top = at <= 3 && line[..at].iter().all(|&b| b == b' ');
    (n >= 3).then_some((c, n, top))
}

/// Whether `line` closes a fence of `n` of `c` opened at the top: up to three spaces, at least `n`
/// of `c`, and nothing after them but spaces.
fn fence_closes(line: &[u8], c: u8, n: usize) -> bool {
    let at = line.iter().take_while(|&&b| b == b' ').count();
    let run = line[at..].iter().take_while(|&&b| b == c).count();
    at <= 3 && run >= n && line[at + run..].iter().all(u8::is_ascii_whitespace)
}

/// The cells a table's delimiter row (`|---|:-:|`, in a quote too) says it has: the most of its
/// runs of `-` and its cells, or none for a line that is no delimiter row.
fn table_delimiter(line: &[u8]) -> Option<usize> {
    let body: &[u8] = {
        let at = line.iter().position(|b| !matches!(b, b' ' | b'\t' | b'>')).unwrap_or(line.len());
        &line[at..]
    };
    if !body.contains(&b'-') || !body.iter().all(|b| matches!(b, b'|' | b'-' | b':' | b' ' | b'\t' | b'\r' | b'\n')) {
        return None;
    }
    let runs = body.split(|&b| b != b'-').filter(|r| !r.is_empty()).count();
    Some(runs.max(body.iter().filter(|&&b| b == b'|').count() + 1))
}

/// More than the worst shapes measured took for each mark, besides the six a byte (`*a `, one
/// mark in three bytes, and nested `1. `: about 230 a mark; `>`: 175; `[` and `<`: 140), so that
/// what it lets through fits (tests/feedback75b.rs `the_markdown_doors_estimate_covers_what_is_made`).
const MARKDOWN_MARK_BYTES: usize = 300;

/// What a control character, a byte inside a fenced code block and a table's cell take at most,
/// besides the six a byte, as measured (tests/feedback75b.rs, the same test).
const MARKDOWN_CONTROL_BYTES: usize = 16;
const MARKDOWN_CODE_BYTES: usize = 80;
const MARKDOWN_CELL_BYTES: usize = 160;

/// Whether a header can be sent as it is: a name made of token characters,
/// and a value without line breaks or other control characters, which would
/// let text from a request add headers of its own.
/// The value of a header the program named `null` (`respond`, `stream`, `events`): it holds the
/// name's place, so that Quillo adds none of its own for it, and is not written (FEEDBACK55.md,
/// Q-710; branch feedback55).
pub const DROPPED: &str = "\0";

pub fn valid_header(name: &str, value: &str) -> bool {
    // By table, a byte a look: each byte of a name had been looked for in a list of 15 (2026-11-09,
    // the server benchmark).
    const fn tables() -> ([bool; 256], [bool; 256]) {
        let (mut token, mut text) = ([false; 256], [false; 256]);
        let mut c = 0;
        while c < 256 {
            let b = c as u8;
            token[c] = b.is_ascii_alphanumeric() || matches!(b, b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~');
            text[c] = b == b'\t' || (b >= 0x20 && b != 0x7f);
            c += 1;
        }
        (token, text)
    }
    static TABLES: ([bool; 256], [bool; 256]) = tables();
    !name.is_empty() && name.bytes().all(|c| TABLES.0[c as usize]) && value.bytes().all(|c| TABLES.1[c as usize])
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
        // Timed for the metrics and the log, when either is on (2026-11-09, the server benchmark).
        let start = (crate::metrics::ENABLED.load(std::sync::atomic::Ordering::Relaxed) || crate::metrics::LOG.load(std::sync::atomic::Ordering::Relaxed) != crate::metrics::OFF).then(Instant::now);
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
        crate::httpd::request_began(&req.method, &log_url(&req.url), &rid);
        let prefetch = is_prefetch(&req);
        // Its body's text, and what reading it as JSON or a form makes, held to `--max-memory`
        // as it is read, once, here: past it the request ends as a route's error does, a 500
        // (FEEDBACK71.md, Q-977; branch security). `--max-body` holds the bytes as they come. The
        // size first: a body under 1,260 bytes reads to less than 64 KB however it is read (a
        // form's field takes 80 bytes for at least two of the body's), and pays a compare.
        // Not a service's join, which never reads its body as a request's and refuses one larger
        // than a join before reading it, its signature checked over the bytes: read here, the
        // JSON of a join with no valid signature was made first (FEEDBACK75.md, Q-1052; branch
        // feedback75).
        let joins = || req.url.split('?').next() == Some("/_quillo/join");
        let body_over = if req.body.len() >= crate::httpd::LARGE / 52 && crate::httpd::MEMORY_LIMITED.load(std::sync::atomic::Ordering::Relaxed) && !joins() {
            read_body_held(&req).err()
        } else {
            None
        };
        let mut resp = if let Some(stop) = body_over {
            finish(Err(stop.at(0)))
        } else if app.watch && req.url == "/_quillo/dev" {
            plain(200, &self.generation.to_string())
        } else if prefetch && crate::httpd::others_waiting() {
            // A guess is work nobody has asked for yet, so it does not go before anyone. The
            // browser drops the 503, and a click is made as usual (FEEDBACK22.md, Q-179;
            // branch feedback22-fixes). Marked `busy`, which Quillo's worker asks again in a
            // moment as it installs: with nothing to tell it from a proxy's 503, a busy server's
            // pages were kept only on a navigation 10 s on, and under steady load not at all
            // (FEEDBACK67.md, B.2, found on CI; branch pwa).
            HttpResponse { status: 503, headers: vec![("cache-control".into(), "no-store".into()), ("x-quillo-prefetch".into(), "busy".into())], body: Vec::new() }
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
        // A page asking whether the save changed it (`x-quillo-hot: <its version>`): what the
        // version it was made with renders now, by the same request, as a print beside this one.
        if app.watch && is_html(&resp) {
            // A GET's only, run as a prefetch is, changing nothing: run for real, a route that writes
            // wrote twice at every save, once for each open page (found in review).
            let hot = (req.method == "GET").then(|| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("x-quillo-hot"))).flatten();
            if let Some(was) = hot.and_then(|(_, v)| v.parse::<u64>().ok()) {
                let old = app.replaced.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
                if let Some((v, loaded)) = old.filter(|(v, _)| *v == was) {
                    if let Loaded::Ready(it) = &*loaded {
                        let mut it = it.fork();
                        it.request_id = Some(rid.clone());
                        it.read_only = true;
                        // With page states of its own: its start replaced the request's, and the
                        // new version's deferred parts, rendered after, found none (found in
                        // review).
                        let mine = crate::value::page_take();
                        let old_render = run_program(&mut it, &req);
                        crate::value::page_restore(mine);
                        // Refused (it would change something): none, and the page compares its own.
                        if let Some(r) = old_render.filter(|r| r.status == 200) {
                            resp.headers.push(("x-quillo-was".into(), page_print(&String::from_utf8_lossy(&r.body))));
                        }
                    }
                    let _ = v;
                }
            }
        }
        if crate::httpd::MEMORY_LIMITED.load(std::sync::atomic::Ordering::Relaxed) {
            BODY_READ.with(|b| b.borrow_mut().take());
        }
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
                        // Streamed, its HTML is not known whole: one that never matches, so a
                        // change reloads it, as before (FEEDBACK54.md, Q-685; branch hmr).
                        let never = format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
                        let script = dev_script(self.generation, &never);
                        // Through the page's policy, as the head is not gone yet (FEEDBACK59.md,
                        // Q-904, found in review).
                        if resp.headers.iter().any(|(k, v)| is_policy(k) && v != DROPPED) {
                            let style = script.split_once("<style>").and_then(|(_, r)| r.split_once("</style>")).map(|(t, _)| t.to_string());
                            let mut head = HttpResponse { status: resp.status, headers: std::mem::take(&mut resp.headers), body: Vec::new() };
                            allow_own(&mut head, &script_texts(script.as_bytes()), &style.iter().map(String::as_str).collect::<Vec<_>>(), false);
                            resp.headers = head.headers;
                        }
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
        // Both scripts have been decided: the mark of a miss's page sent as written goes no
        // further. Only a 404 carries it (FEEDBACK69.md, Q-964; branch q964-miss-page).
        if resp.status == 404 {
            resp.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(AS_WRITTEN));
        }
        // Chrome asks every site for this while DevTools is open; it is not
        // the program's business, so it is not logged.
        // Nor are the batches of UDP packets handed to the program: they are
        // counted as packets (see `udp`), not requests.
        // Nor the scheduler's own call each second, asking that calls made
        // `later` be sent (only it has the token: others get a 404, logged)
        // (FEEDBACK4.md, Q-043; branch bank-fixes).
        let took = start.map_or(0.0, |s| s.elapsed().as_secs_f64());
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
    // Its event loop, the watch, accept and job threads, a wait thread for each worker, and the one
    // every page with live data or on a channel shares (FEEDBACK66.md, Q-954; branch q954), where
    // each such page had a thread of its own, not counted here.
    let needs = workers + 1 + 9;
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
    serve_listener(app, listener, addr, workers, listening)
}

/// `serve` on a port bound already: `--split` binds the app's before it starts its services, to
/// tell them the port (FEEDBACK55.md, Q-843; branch feedback55).
pub fn serve_bound(app: App, listener: std::net::TcpListener, workers: usize) -> Result<(), String> {
    take_descriptors();
    let addr = listener.local_addr().map_or_else(|_| "?".to_string(), |a| a.to_string());
    serve_listener(app, listener, &addr, workers, |_| {})
}

fn serve_listener(app: App, listener: std::net::TcpListener, addr: &str, workers: usize, listening: impl FnOnce(std::net::SocketAddr)) -> Result<(), String> {
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
        // `r` and Enter, typed where the server runs: the program loaded again with its data
        // started again, without a restart (which would sign no one out now either, Q-689). Only
        // when its input is a terminal: a pipe, a test's null or the debugger's protocol is not
        // read.
        let typed = std::io::IsTerminal::is_terminal(&std::io::stdin()) && !crate::debug::on();
        eprintln!("quillo: watching for changes{}", if typed { " (r and Enter starts the program's data again)" } else { "" });
        // Opened to the network (`--host 0.0.0.0`, to try it from a phone): allowed, and said,
        // as anyone there now reaches the routes, with errors shown in full (FEEDBACK62.md,
        // Q-918; branch cache-and-migrate).
        if let Some(a) = bound.filter(|a| !a.ip().is_loopback()) {
            eprintln!(
                "quillo: warning: --watch listens on {}, not this machine alone: anyone on its network can reach the program's routes and server fns, and sees its errors in full. Leave out --host to keep it to this machine",
                a.ip()
            );
        }
        if typed {
            let app = app.clone();
            let _ = std::thread::Builder::new().name("quillo-keys".into()).spawn(move || {
                let mut line = String::new();
                while std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line).unwrap_or(0) > 0 {
                    if line.trim() == "r" {
                        app.fresh.store(true, std::sync::atomic::Ordering::Relaxed);
                        app.reload();
                    }
                    line.clear();
                }
            });
        }
        // The MCP door's token, on this machine alone (FEEDBACK56.md, C.3; branch feedback56).
        // Where this server answers, also when the first load failed (found in review).
        if let Some(a) = bound.filter(|a| a.ip() == std::net::Ipv4Addr::LOCALHOST && a.port() != 0) {
            let current = app.current().1;
            crate::mcp::dev_door(format!("http://127.0.0.1:{}", a.port()), match &*current {
                Loaded::Ready(it) => Some(it),
                Loaded::Failed(_) => None,
            });
        }
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
    // `--reload`: each instance loads the program again, the first checking it and saying so
    // (FEEDBACK55.md, Q-841; branch feedback55).
    if apps[0].reload {
        std::thread::Builder::new()
            .name("quillo-watch".into())
            .stack_size(WORKER_STACK)
            .spawn(watcher_of(apps[0].clone(), apps[1..].to_vec()))
            .map_err(|e| format!("cannot start file watcher: {}", e))?;
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

    /// `is_html` by one comparison is the long way's answer, as the third review of branch pwa
    /// asked for it: every ASCII byte at each of the first ten places of three types, the cases
    /// by name, and one byte of a two-byte letter at the ninth (speed check of branch pwa).
    #[test]
    fn is_html_by_one_comparison_is_the_long_ways() {
        let long = |c: &str| c.trim_start_matches([' ', '\t']).get(..9).is_some_and(|t| t.eq_ignore_ascii_case("text/html"));
        let page = |c: &str| is_html(&HttpResponse { status: 200, headers: vec![("Content-Type".into(), c.into())], body: Vec::new() });
        for base in ["text/html; charset=utf-8", "text/html", "Text/HTML"] {
            for at in 0..10 {
                for b in 0u8..0x80 {
                    let mut v = base.as_bytes().to_vec();
                    if at < v.len() {
                        v[at] = b;
                    } else {
                        v.push(b);
                    }
                    let c = String::from_utf8(v).unwrap();
                    assert_eq!(page(&c), long(&c), "{:?}", c);
                }
            }
        }
        for (c, html) in [
            ("text/html", true),
            ("TEXT/HTML; charset=utf-8", true),
            ("text/HTML", true),
            ("tEXT/hTmL", true),
            (" text/html", true),
            ("\t Text/Html; charset=utf-8", true),
            ("text/htmlx", true),
            ("text/htm", false),
            ("", false),
            ("  ", false),
            ("text/plain; charset=utf-8", false),
            ("application/json", false),
            ("text\x0fhtml", false),
            ("text/htm\u{e9}", false),
            ("text/html\u{e9}", true),
            ("\u{a0}text/html", false),
            ("text/xhtml", false),
        ] {
            assert_eq!(page(c), html, "{:?}", c);
            assert_eq!(long(c), html, "{:?}", c);
        }
        // No content type: no page.
        assert!(!is_html(&HttpResponse { status: 200, headers: Vec::new(), body: Vec::new() }));
    }

    /// Header names and values by table, as by the list (2026-11-09): every byte, both ways.
    #[test]
    fn valid_header_by_table_is_as_by_list() {
        for c in 0u8..=255 {
            let token = c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c);
            let text = c == b'\t' || (c >= 0x20 && c != 0x7f);
            let one = [c];
            let as_str = std::str::from_utf8(&one).unwrap_or("\u{80}");
            if c < 0x80 {
                assert_eq!(valid_header(as_str, "v"), token, "name byte {:#x}", c);
                assert_eq!(valid_header("x", as_str), text, "value byte {:#x}", c);
            }
        }
        assert!(!valid_header("", "v"));
        assert!(valid_header("x", ""));
        assert!(valid_header("x", "caf\u{e9}"));
        assert!(!valid_header("caf\u{e9}", "v"));
        assert!(!valid_header("a b", "v"));
        assert!(!valid_header("x", "a\r\nb: c"));
        // The prefetch's tags, made once for a base, are what was made each time.
        assert_eq!(&*prefetch_tags("/b"), format!("<script>{}</script>{}", PREFETCH_JS, used_script("/b")).as_bytes());
        assert_eq!(&*prefetch_tags(""), format!("<script>{}</script>{}", PREFETCH_JS, used_script("")).as_bytes());
    }

    /// What is put into a page goes where splice put it (2026-11-09).
    #[test]
    fn insert_at_puts_bytes_where_splice_did() {
        for (body, at, bytes) in [(&b"<body></body>"[..], 6, &b"<script>x</script>"[..]), (b"abc", 0, b"xy"), (b"abc", 3, b"xy"), (b"", 0, b"xy"), (b"abc", 1, b""), (b"abc", 2, b"0123456789abcdef0123456789")] {
            let mut want = body.to_vec();
            want.splice(at..at, bytes.iter().copied());
            let mut got = body.to_vec();
            insert_at(&mut got, at, bytes);
            assert_eq!(got, want, "{:?} at {}", String::from_utf8_lossy(body), at);
        }
        // With room already made, and without.
        let mut room = Vec::with_capacity(64);
        room.extend_from_slice(b"<html></html>");
        insert_at(&mut room, 6, b"<head></head>");
        assert_eq!(room, b"<html><head></head></html>");
        // A needle of the page's own, and any other, are found alike.
        assert_eq!(finder(b"/_quillo/quillo.js").find(b"x/_quillo/quillo.js"), Some(1));
        assert_eq!(finder(b"/other.js").find(b"/other.js"), Some(0));
    }

    /// The page's scans by memchr find what the byte-by-byte ones did (2026-11-09, the server
    /// benchmark), at the edges too.
    #[test]
    fn the_pages_scans_find_what_they_did() {
        let ci = |b: &[u8], n: &[u8]| find_ascii_ci(b, n);
        assert_eq!(ci(b"<head></head><body>", b"</head>"), Some(6));
        assert_eq!(ci(b"<HEAD></HEAD>", b"</head>"), Some(6));
        assert_eq!(ci(b"</HeAd>", b"</head>"), Some(0));
        assert_eq!(ci(b"x</head", b"</head>"), None);
        assert_eq!(ci(b"</hea", b"</head>"), None);
        assert_eq!(ci(b"", b"</head>"), None);
        assert_eq!(ci(b"a < b </head>", b"</head>"), Some(6));
        assert_eq!(ci(b"<<body", b"<body"), Some(1));
        assert_eq!(ci(b"x", b""), None);
        // The first of two.
        assert_eq!(ci(b"</head></head>", b"</head>"), Some(0));
        // A script's own code, only in a <script> of its own.
        assert!(script_with(b"<body><script>abc</script>", b"abc"));
        assert!(!script_with(b"<body>abc<script>x</script>", b"abc"));
        assert!(script_with(b"abc <script>abc", b"abc"));
        assert!(!script_with(b"", b"abc"));
        assert!(!script_with(b"<script>ab", b"abc"));
        // A tag that loads the path, not text that names it.
        assert!(script_tag_for(b"<script src=\"/_quillo/quillo.js\" defer>", b"/_quillo/quillo.js"));
        assert!(script_tag_for(b"<p>x</p><SCRIPT\nsrc='/b/_quillo/quillo.js?v=1'>", b"/_quillo/quillo.js"));
        assert!(!script_tag_for(b"<p>/_quillo/quillo.js\"</p>", b"/_quillo/quillo.js"));
        assert!(!script_tag_for(b"<script>x</script> /_quillo/quillo.js\"", b"/_quillo/quillo.js"));
        assert!(!script_tag_for(b"<scripts src=\"/_quillo/quillo.js\">", b"/_quillo/quillo.js"));
        assert!(!script_tag_for(b"<script src=\"/_quillo/quillo.jsx\">", b"/_quillo/quillo.js"));
        assert!(!script_tag_for(b"/_quillo/quillo.js\"", b"/_quillo/quillo.js"));
        // A second mention after one that is text.
        assert!(script_tag_for(b"<p>/_quillo/quillo.js\"</p><script src=\"/_quillo/quillo.js\">", b"/_quillo/quillo.js"));
    }

    /// A player is asked for wherever the page has one, searched for forwards (branch speed-bun,
    /// 2026-11-09): what the search backwards found.
    /// `page_tags` against the rule a byte at a time, and never finding less than the five searches
    /// it took the place of (branch speed-fortunes): pages made of pieces that are tags, nearly
    /// tags and text, at every place against its sixteen.
    #[test]
    fn page_tags_finds_what_each_search_did() {
        fn by_rule(b: &[u8]) -> PageTags {
            let mut t = PageTags::default();
            for i in 0..b.len() {
                if b[i] != b'<' {
                    continue;
                }
                let r = &b[i + 1..];
                t.follow |= r.first().is_some_and(|c| c | 0x20 == b'a') && matches!(r.get(1), Some(b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'>' | b'/'));
                t.follow |= r.len() >= 6 && r[..6].eq_ignore_ascii_case(b"script");
                t.media |= r.starts_with(b"quillo-media");
            }
            t
        }
        let parts: [&[u8]; 24] = [
            b"<a href=x>", b"<A HREF=x>", b"<a>", b"<a/>", b"<a\n", b"<abbr>", b"<area href=y>", b"<script>", b"<SCRIPT src=s>", b"<Script>",
            b"<scrip", b"<quillo-media stream=c>", b"<QUILLO-MEDIA>", b"<quillo-medi", b"<q>", b"<s>", b"&lt;a href", b"<", b"<<", b"text ",
            "é".as_bytes(), "フレーム".as_bytes(), b"<td>1</td>", b"</a>",
        ];
        let mut seed = 11u64;
        for round in 0..20000 {
            let mut page = Vec::new();
            // Shifted by 0 to 16 bytes, so each piece falls at each place of a block of sixteen.
            page.extend(std::iter::repeat(b'.').take(round % 17));
            for _ in 0..(seed % 12) {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                page.extend_from_slice(parts[(seed >> 33) as usize % parts.len()]);
            }
            let got = page_tags(&page);
            assert_eq!(got, by_rule(&page), "{:?}", String::from_utf8_lossy(&page));
            assert_eq!(got.media, has_media(&page), "{:?}", String::from_utf8_lossy(&page));
            assert!(got.follow || !may_follow(&page), "{:?}", String::from_utf8_lossy(&page));
        }
        // At the very end, and nothing but it.
        for tail in [&b"<a"[..], b"<", b"<a ", b"<script", b"<scrip", b"<quillo-media", b"<quillo-medi", b""] {
            for pad in 0..40 {
                let mut page = vec![b'x'; pad];
                page.extend_from_slice(tail);
                assert_eq!(page_tags(&page), by_rule(&page), "{:?}", String::from_utf8_lossy(&page));
            }
        }
        // A tag in every place of a long page's blocks, found once it is there.
        for at in 0..64 {
            let mut page = vec![b'y'; 100];
            page[at..at + 3].copy_from_slice(b"<a>");
            assert!(page_tags(&page).follow, "{}", at);
        }
    }

    #[test]
    fn has_media_finds_what_the_search_backwards_did() {
        for body in [&b""[..], b"<quillo-media", b"<body><quillo-media stream=\"cam\"></body>", b"<p>x</p><quillo-media>", b"<quillo-medi", b"<QUILLO-MEDIA>", b"quillo-media", b"<quillo-media><quillo-media>", b"<<quillo-media"] {
            assert_eq!(has_media(body), memchr::memmem::rfind(body, b"<quillo-media").is_some(), "{:?}", String::from_utf8_lossy(body));
        }
        // Past the searcher's first block, at the very end of a long page.
        let mut long = vec![b'a'; 5000];
        long.extend_from_slice(b"<quillo-media");
        assert!(has_media(&long));
        assert!(!has_media(&long[..long.len() - 1]));
    }

    /// The text as it was, decoded byte by byte as before the shortcut (branch speed-bun).
    fn percent_decode_slowly(s: &str, plus_is_space: bool) -> String {
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

    /// Text with nothing to decode is given as it is, the rest decoded as it was (branch
    /// speed-bun, 2026-11-09).
    #[test]
    fn percent_decode_with_nothing_to_decode_gives_what_it_did() {
        let cases = ["", "a", "abc", "a+b", "+", "++", "%", "%4", "%41", "a%41b", "%zz", "%%41", "a%2Fb", "é", "%C3%A9", "%FF", "%c3", "a b", "x=1&y=2", "%2", "100%", "+%41+", "\u{1F600}", "%F0%9F%98%80"];
        for c in cases {
            for plus in [false, true] {
                assert_eq!(percent_decode(c, plus), percent_decode_slowly(c, plus), "{:?} plus {}", c, plus);
            }
        }
        // A `+` stays one where it is no space, and is one where it is.
        assert_eq!(percent_decode("a+b", false), "a+b");
        assert_eq!(percent_decode("a+b", true), "a b");
    }

    /// A path already in its form is given as it is; any other is made into it as it was (branch
    /// speed-bun, 2026-11-09).
    #[test]
    fn route_path_in_its_form_already_gives_what_it_did() {
        let slowly = |path: &str| {
            let pieces: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(|s| percent_decode_slowly(s, false).replace('/', "%2F")).collect();
            format!("/{}", pieces.join("/"))
        };
        let cases = ["", "/", "//", "///", "/a", "/a/", "/a//b", "a", "a/b", "/a/b/c", "/a%2Fb", "/a/%2F", "/%41", "/é", "/a+b", "/payments%2Freturn", "/a/b/", "//a", "/a/./b", "/%", "/a%", "/a/%zz"];
        for c in cases {
            assert_eq!(route_path(c), slowly(c), "{:?}", c);
        }
        assert_eq!(route_path("/a//b/"), "/a/b");
        assert_eq!(route_path("/x/y"), "/x/y");
    }

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
