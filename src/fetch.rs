//! `http`: calling other servers.
//!
//! ```text
//! let r = http.get("https://api.example.com/users/1")
//! r.status  r.ok  r.headers  r.body  r.json
//! http.post(url, { name: "Ada" })          // maps and lists are sent as JSON
//! http.request("PUT", url, body, { authorization: "Bearer ..." })
//! http.stream(url, (piece) => ..., { method, body, headers })
//! http.post(url, body, {}, { redirects: 0, public_only: true })
//! http.get(url, {}, { cache: 60 })
//! ```
//!
//! Options, after the headers (in `http.stream`'s map): `redirects`, how
//! many redirects to follow (5 unless given); `public_only`, connect only to
//! addresses on the public internet. That is checked on the addresses the
//! host name gives when the connection is made (and again for a redirect),
//! so a name cannot be looked up as public and then connect inside.
//! `cache`, for a GET: seconds a successful answer is kept and given again
//! to the same call (same URL, headers and options), by any request.
//! `timeout`: seconds the whole call may take (up to 300), after which it
//! gives an error value; unless given, 10 s to connect and 60 s in all
//! (`http.stream`: 300 s of silence). Any other option is an error, not
//! ignored (FEEDBACK5.md, Q-049; branch stays-fixes).
//!
//! In a component, these run in the browser (src/client/runtime.js). While
//! the server renders one, a call from its code is a mistake: it would be
//! made from the server's network, with what the page gave it.
//!
//! `http.stream` hands over the answer as it arrives: each event of an event
//! stream (its data, parsed as JSON when it is, and its name), each line of
//! newline-delimited JSON, or else each piece of text. The function may stop
//! it by returning `false`; given `send` (in a `stream` or `events`), what
//! arrives goes straight on to the client.
//!
//! Any HTTP status is a response; a failure to connect is an error value.
//! The interpreter lock is released while waiting, so other requests run.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::collections::HashMap;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

type R = Result<Value, Ctrl>;

const MAX_BODY: u64 = 64 << 20;

pub fn install(m: &mut Map) {
    let fns: &[(&'static str, BuiltinFn)] = &[
        ("http.get", get),
        ("http.delete", delete),
        ("http.post", post),
        ("http.put", put),
        ("http.patch", patch),
        ("http.request", request),
        ("http.stream", stream),
    ];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("http.").into(), Value::Builtin(name, *f));
    }
}

/// A call's options (see the top of this file).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Opts {
    redirects: u32,
    public_only: bool,
    /// Seconds to keep a GET's successful answer (0: not kept).
    cache: u32,
    /// Milliseconds the whole call may take (0: the agent's own limits).
    timeout_ms: u32,
    /// The certificates a call trusts and presents (`ca`, `cert`, `key`), by their number in
    /// `TLS` (0: the usual public CAs, and none presented).
    tls: u32,
}

const DEFAULT_OPTS: Opts = Opts { redirects: 5, public_only: false, cache: 0, timeout_ms: 0, tls: 0 };

/// The options there are; `http.stream`'s map also has its request's.
const OPTION_NAMES: &[&str] = &["redirects", "public_only", "cache", "timeout", "ca", "cert", "key"];
/// What `ca`, `cert` and `key` named, each set once, so that `Opts` stays a plain value: a CA
/// file (or "system") to trust instead of the bundled ones, and a certificate and its key to
/// present, as some banks' and governments' APIs ask (FEEDBACK36.md, Q-442; branch feedback36-39).
static TLS: Mutex<Vec<(Option<String>, Option<String>, Option<String>)>> = Mutex::new(Vec::new());

fn tls_number(ca: Option<String>, cert: Option<String>, key: Option<String>) -> u32 {
    if ca.is_none() && cert.is_none() && key.is_none() {
        return 0;
    }
    let mut t = TLS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = (ca, cert, key);
    match t.iter().position(|e| *e == entry) {
        Some(i) => i as u32 + 1,
        None => {
            t.push(entry);
            t.len() as u32
        }
    }
}

/// A client's TLS settings: the CAs in `ca` (a PEM file, or "system" for the machine's), or the
/// usual public ones; and `cert` with its `key` presented to the server.
pub(crate) fn client_tls(ca: Option<&str>, cert: Option<&str>, key: Option<&str>) -> Result<std::sync::Arc<rustls::ClientConfig>, String> {
    use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
    let mut roots = rustls::RootCertStore::empty();
    match ca {
        None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
        Some("system") => {
            let found = rustls_native_certs::load_native_certs();
            roots.add_parsable_certificates(found.certs);
            if roots.is_empty() {
                return Err("ca \"system\": no CA certificates found on this system".into());
            }
        }
        Some(path) => {
            let certs = CertificateDer::pem_file_iter(path).and_then(|c| c.collect::<Result<Vec<_>, _>>()).map_err(|e| format!("ca {}: {}", path, e))?;
            if certs.is_empty() {
                return Err(format!("ca {}: no certificate in it (a PEM file is wanted)", path));
            }
            for c in certs {
                roots.add(c).map_err(|e| format!("ca {}: {}", path, e))?;
            }
        }
    }
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots);
    let config = match (cert, key) {
        (None, None) => builder.with_no_client_auth(),
        (Some(cert), Some(key)) => {
            let chain = CertificateDer::pem_file_iter(cert).and_then(|c| c.collect::<Result<Vec<_>, _>>()).map_err(|e| format!("cert {}: {}", cert, e))?;
            if chain.is_empty() {
                return Err(format!("cert {}: no certificate in it (a PEM file is wanted)", cert));
            }
            let key = PrivateKeyDer::from_pem_file(key).map_err(|e| format!("key {}: {}", key, e))?;
            builder.with_client_auth_cert(chain, key).map_err(|e| format!("cert and key: {}", e))?
        }
        _ => return Err("cert and key go together: a certificate, and its key".into()),
    };
    Ok(std::sync::Arc::new(config))
}

/// The proxy for `url` from `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` (either case), unless
/// `NO_PROXY` names its host (a name, `.domain` or `domain` for it and below, an address, `*`);
/// the machine itself never goes through one, as Go's default has it (FEEDBACK36.md, Q-442;
/// branch feedback36-39).
fn proxy_for(url: &str) -> Option<String> {
    let var = |names: &[&str]| names.iter().find_map(|n| std::env::var(n).ok().filter(|v| !v.trim().is_empty()));
    let https = url.starts_with("https://");
    let proxy = if https { var(&["HTTPS_PROXY", "https_proxy"]) } else { var(&["HTTP_PROXY", "http_proxy"]) }.or_else(|| var(&["ALL_PROXY", "all_proxy"]))?;
    let host = host_of(url).to_ascii_lowercase();
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if bare == "localhost" || bare.ends_with(".localhost") || bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
        return None;
    }
    let no = var(&["NO_PROXY", "no_proxy"]).unwrap_or_default();
    for entry in no.split(',').map(|e| e.trim().to_ascii_lowercase()).filter(|e| !e.is_empty()) {
        if entry == "*" {
            return None;
        }
        let name = entry.trim_start_matches('.');
        let name = name.rsplit_once(':').filter(|(_, p)| p.bytes().all(|b| b.is_ascii_digit())).map_or(name, |(h, _)| h);
        if bare == name || bare.ends_with(&format!(".{}", name)) {
            return None;
        }
    }
    Some(proxy)
}

/// The host of an http(s) URL, without its port or user.
fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    match authority.strip_prefix('[') {
        Some(v6) => &authority[..v6.find(']').map_or(authority.len(), |i| i + 2)],
        None => authority.rsplit_once(':').map_or(authority, |(h, _)| h),
    }
}
const STREAM_NAMES: &[&str] = &["method", "body", "headers"];

/// The longest `timeout`, in seconds.
const MAX_TIMEOUT_SECS: f64 = 300.0;

/// The longest `cache`: a day.
const MAX_CACHE_SECS: i64 = 86_400;

fn opts_of(v: Option<&Value>, fname: &str, line: u32, base: &std::path::Path) -> Result<Opts, Ctrl> {
    let m = match v {
        None | Some(Value::Nil) => return Ok(DEFAULT_OPTS),
        Some(Value::Map(m)) => m,
        Some(other) => return fail(line, format!("`{}` takes options as a map, got {}", fname, other.type_name())),
    };
    let redirects = match m.get("redirects") {
        None | Some(Value::Nil) => DEFAULT_OPTS.redirects,
        Some(Value::Int(n)) if (0..=20).contains(n) => *n as u32,
        Some(other) => return fail(line, format!("`{}`: `redirects` is a whole number from 0 to 20, got {}", fname, other.display())),
    };
    let public_only = match m.get("public_only") {
        None | Some(Value::Nil) => false,
        Some(Value::Bool(b)) => *b,
        Some(other) => return fail(line, format!("`{}`: `public_only` is true or false, got {}", fname, other.type_name())),
    };
    let cache = match m.get("cache") {
        None | Some(Value::Nil) => 0,
        Some(Value::Int(n)) if (0..=MAX_CACHE_SECS).contains(n) => *n as u32,
        Some(other) => return fail(line, format!("`{}`: `cache` is seconds, a whole number from 0 to {}, got {}", fname, MAX_CACHE_SECS, other.display())),
    };
    let secs = match m.get("timeout") {
        None | Some(Value::Nil) => None,
        Some(Value::Int(n)) => Some(*n as f64),
        Some(Value::Float(x)) => Some(*x),
        Some(other) => return fail(line, format!("`{}`: `timeout` is seconds, more than 0 and up to {}, got {}", fname, MAX_TIMEOUT_SECS, other.display())),
    };
    let timeout_ms = match secs {
        None => 0,
        Some(s) if s > 0.0 && s <= MAX_TIMEOUT_SECS => ((s * 1000.0).round() as u32).max(1),
        Some(s) => return fail(line, format!("`{}`: `timeout` is seconds, more than 0 and up to {}, got {}", fname, MAX_TIMEOUT_SECS, s)),
    };
    let stream = fname == "http.stream";
    if let Some(k) = m.keys().map(|k| k.as_ref()).find(|k| !OPTION_NAMES.contains(k) && !(stream && STREAM_NAMES.contains(k))) {
        let known = if stream { [STREAM_NAMES, OPTION_NAMES].concat() } else { OPTION_NAMES.to_vec() };
        return fail(line, format!("`{}`: unknown option `{}` (the options are {})", fname, k, known.join(", ")));
    }
    // Files are named from the program's folder, as `read_file` names them.
    let file = |name: &str| -> Result<Option<String>, Ctrl> {
        match m.get(name) {
            None | Some(Value::Nil) => Ok(None),
            Some(Value::Str(s)) if name == "ca" && &**s == "system" => Ok(Some("system".into())),
            Some(Value::Str(s)) if !s.is_empty() => Ok(Some(base.join(&**s).to_string_lossy().into_owned())),
            Some(other) => fail(line, format!("`{}`: `{}` is the path of a PEM file{}, got {}", fname, name, if name == "ca" { " or \"system\"" } else { "" }, other.repr())),
        }
    };
    let (ca, cert, key) = (file("ca")?, file("cert")?, file("key")?);
    if cert.is_some() != key.is_some() {
        return fail(line, format!("`{}`: `cert` and `key` go together: a certificate, and its key", fname));
    }
    Ok(Opts { redirects, public_only, cache, timeout_ms, tls: tls_number(ca, cert, key) })
}

/// The server's own network is not where a component's calls go (see the
/// top of this file).
fn not_in_component(it: &Interp, fname: &str, line: u32) -> Result<(), Ctrl> {
    if it.in_component {
        return fail(
            line,
            format!(
                "`{}` in a component runs in the browser, once the page has arrived; while the server renders the \
                 component it is not called: call it from an event handler or @mount, or from a `server fn`",
                fname
            ),
        );
    }
    Ok(())
}

/// Whether `ip` is on the public internet: not this machine, a private or
/// link-local network (where a cloud's metadata service is), or an address
/// kept for other uses.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => public_v4(a),
        IpAddr::V6(a) => {
            // An IPv4 address written as IPv6 is that address.
            if let Some(v4) = a.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let s = a.segments();
            // NAT64 (64:ff9b::/96) and 6to4 (2002::/16) lead to IPv4 addresses.
            if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
                return public_v4(Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8));
            }
            if s[0] == 0x2002 {
                return public_v4(Ipv4Addr::new((s[1] >> 8) as u8, s[1] as u8, (s[2] >> 8) as u8, s[2] as u8));
            }
            !(a.is_unspecified()
                || a.is_loopback()
                || a.is_multicast()
                || s[0..6] == [0, 0, 0, 0, 0, 0] // IPv4-compatible, and the rest of ::/96
                || (s[0] & 0xfe00) == 0xfc00 // unique local, fc00::/7
                || (s[0] & 0xffc0) == 0xfe80 // link-local, fe80::/10
                || (s[0] & 0xffc0) == 0xfec0 // site-local, fec0::/10
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || (s[0] == 0x2001 && s[1] < 0x0200) // Teredo and other IETF uses, 2001::/23
                || s[0] == 0x0100 && s[1..4] == [0, 0, 0]) // discard-only, 100::/64
        }
    }
}

fn public_v4(a: Ipv4Addr) -> bool {
    let [x, y, ..] = a.octets();
    !(a.is_unspecified()
        || x == 0 // "this network"
        || a.is_loopback()
        || a.is_private()
        || a.is_link_local() // 169.254.0.0/16, metadata services
        || (x == 100 && (64..128).contains(&y)) // shared address space (carrier NAT)
        || (x == 192 && y == 0 && a.octets()[2] == 0) // IETF protocol assignments
        || a.is_documentation()
        || (x == 198 && (y == 18 || y == 19)) // benchmarking
        || a.is_multicast()
        || x >= 240) // reserved, and broadcast
}

/// Looks `netloc` up and keeps only public addresses; none is an error.
fn resolve_public(netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
    let all: Vec<SocketAddr> = netloc.to_socket_addrs()?.collect();
    let public: Vec<SocketAddr> = all.into_iter().filter(|a| public_ip(a.ip())).collect();
    if public.is_empty() {
        let host = netloc.rsplit_once(':').map_or(netloc, |(h, _)| h);
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("{} is not an address on the public internet", host)));
    }
    Ok(public)
}

/// The agent for calls with these options to `url` (`stream`: for `http.stream`, which has no
/// limit on the whole answer, only on a silence in it).
fn agent(o: Opts, stream: bool, url: &str) -> Result<ureq::Agent, String> {
    type Key = (u32, bool, bool, u32, Option<String>);
    static AGENTS: OnceLock<Mutex<HashMap<Key, ureq::Agent>>> = OnceLock::new();
    let proxy = proxy_for(url);
    let mut agents = AGENTS.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = (o.redirects, o.public_only, stream, o.tls, proxy.clone());
    if let Some(a) = agents.get(&key) {
        return Ok(a.clone());
    }
    let b = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        // Under a policy that limits where it goes, each redirect is followed here, its address
        // judged (see `follow`), not by the client, which would go anywhere.
        .redirects(if crate::policy::get().limits_http() { 0 } else { o.redirects })
        // `Authorization` goes on to the same host, as `fetch` keeps it for the same origin: an
        // API that adds a `/` by a redirect answered 401 (FEEDBACK36.md, Q-437; branch
        // feedback36-39).
        .redirect_auth_headers(ureq::RedirectAuthHeaders::SameHost)
        .user_agent(concat!("quillo/", env!("CARGO_PKG_VERSION")));
    let b = if stream { b.timeout_read(Duration::from_secs(300)) } else { b.timeout(Duration::from_secs(60)) };
    // Through a proxy, the proxy looks the name up: `public_only` checks it before the call.
    // Under a policy with `private = false` too: the name is judged as it is looked up to
    // connect, not only before, which a name that changes its answer would get past
    // (FEEDBACK41.md, Q-478; branch feedback40-43).
    let b = if (o.public_only || !crate::policy::get().private) && proxy.is_none() { b.resolver(resolve_public) } else { b };
    let b = match &proxy {
        Some(p) => b.proxy(ureq::Proxy::new(p).map_err(|e| format!("the proxy in the environment, {}: {}", p, e))?),
        None => b,
    };
    let b = if o.tls > 0 {
        let entry = TLS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)[o.tls as usize - 1].clone();
        b.tls_config(client_tls(entry.0.as_deref(), entry.1.as_deref(), entry.2.as_deref())?)
    } else {
        b
    };
    let a = b.build();
    agents.insert(key, a.clone());
    Ok(a)
}

#[derive(Clone)]
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    /// Every `Set-Cookie`, which cannot be joined into one header as others are.
    cookies: Vec<String>,
    body: String,
}

/// One call: its answer, the head read, the body not yet.
fn call(o: Opts, stream: bool, method: &str, url: &str, headers: &[(String, String)], body: Option<&(String, Vec<u8>)>) -> Result<ureq::Response, String> {
    if o.public_only && proxy_for(url).is_some() {
        let host = host_of(url).trim_start_matches('[').trim_end_matches(']');
        let port = if url.starts_with("https://") { 443 } else { 80 };
        resolve_public(&format!("{}:{}", host, port)).map_err(|e| timed_out(o, url, format!("{}: {}", url, e)))?;
    }
    let mut req = agent(o, stream, url)?.request(method, url);
    if o.timeout_ms > 0 {
        req = req.timeout(Duration::from_millis(o.timeout_ms as u64));
    }
    for (k, v) in headers {
        req = req.set(k, v);
    }
    let res = match body {
        Some((ctype, bytes)) => {
            if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
                req = req.set("content-type", ctype);
            }
            req.send_bytes(bytes)
        }
        None => req.call(),
    };
    match res {
        Ok(r) | Err(ureq::Error::Status(_, r)) => Ok(r),
        Err(e) => Err(timed_out(o, url, e.to_string())),
    }
}

/// `call`, and the redirects the client does not follow: a `307` or `308` after a write is
/// followed with the same method and body, as RFC 9110 §15.4.8 and `fetch` do, where the
/// redirect was the answer (FEEDBACK36.md, Q-437; branch feedback36-39). To another origin,
/// `Authorization` and `Cookie` are not sent on.
fn follow(o: Opts, stream: bool, method: &str, url: &str, headers: &[(String, String)], body: Option<(String, Vec<u8>)>) -> Result<(String, ureq::Response), String> {
    let mut url = url.to_string();
    let mut headers = headers.to_vec();
    let mut hops = 0;
    let (mut method, mut body) = (method.to_string(), body);
    let policed = crate::policy::get().limits_http();
    loop {
        // Each address, the first and each one it is sent on to (FEEDBACK41.md, Q-478; branch
        // feedback40-43).
        crate::policy::get().http(&url)?;
        let resp = call(o, stream, &method, &url, &headers, body.as_ref())?;
        let status = resp.status();
        let again = o.redirects > 0
            && (matches!(status, 307 | 308) && !matches!(method.as_str(), "GET" | "HEAD") || policed && matches!(status, 301 | 302 | 303 | 307 | 308));
        // As the client does when it follows them: a 301, 302 or 303 after anything but a GET
        // or a HEAD is a GET with no body.
        if again && matches!(status, 301 | 302 | 303) && !matches!(method.as_str(), "GET" | "HEAD") {
            method = "GET".into();
            body = None;
            headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type") && !k.eq_ignore_ascii_case("content-length"));
        }
        let next = if again { resp.header("location").and_then(|l| join_url(&url, l)) } else { None };
        let Some(next) = next else { return Ok((url, resp)) };
        hops += 1;
        if hops > o.redirects {
            return Err(format!("{}: more than {} redirects", url, o.redirects));
        }
        if origin_of(&next) != origin_of(&url) {
            headers.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization") && !k.eq_ignore_ascii_case("cookie"));
        }
        url = next;
    }
}

/// A `Location` read against the URL it came from; only http(s).
fn join_url(base: &str, location: &str) -> Option<String> {
    let u = url::Url::parse(base).ok()?.join(location).ok()?;
    matches!(u.scheme(), "http" | "https").then(|| u.to_string())
}

/// `scheme://host:port`, the port written out.
fn origin_of(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(u) => format!("{}://{}:{}", u.scheme(), u.host_str().unwrap_or(""), u.port_or_known_default().unwrap_or(0)),
        Err(_) => url.to_string(),
    }
}

fn send(o: Opts, method: &str, url: &str, headers: &[(String, String)], body: Option<(String, Vec<u8>)>) -> Result<Reply, String> {
    let (url, resp) = follow(o, false, method, url, headers, body)?;
    let (status, headers, cookies, reader) = head_of(o, &url, method, resp)?;
    let mut bytes = Vec::new();
    reader.take(MAX_BODY).read_to_end(&mut bytes).map_err(|e| timed_out(o, &url, e.to_string()))?;
    Ok(Reply { status, headers, cookies, body: String::from_utf8_lossy(&bytes).into_owned() })
}

/// An answer's status, headers, `Set-Cookie`s and body.
type Head = (u16, Vec<(String, String)>, Vec<String>, Body);

/// The answer's status, headers (a repeated one joined with `, `, as RFC 9110 §5.3 allows; the
/// `Set-Cookie`s, which cannot be, with a line break), its `Set-Cookie`s, and its body decoded
/// (FEEDBACK36.md, Q-435 and Q-438; branch feedback36-39). Interim answers are passed over to
/// the final one: a `103 Early Hints` was taken for the answer, and the call waited out its
/// time limit for its body (Q-436).
fn head_of(o: Opts, url: &str, method: &str, resp: ureq::Response) -> Result<Head, String> {
    let status = resp.status();
    if (100..200).contains(&status) && status != 101 {
        // ureq reads an interim answer as one whose body lasts until the connection closes:
        // that body is the rest of what the server sent, the final answer among it.
        return after_interim(o, url, method, resp.into_reader());
    }
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut cookies = Vec::new();
    for n in resp.headers_names() {
        let values = resp.all(&n);
        let n = n.to_ascii_lowercase();
        if headers.iter().any(|(k, _)| *k == n) {
            continue;
        }
        if n == "set-cookie" {
            cookies.extend(values.iter().map(|v| v.to_string()));
        }
        headers.push((n.clone(), values.join(if n == "set-cookie" { "\n" } else { ", " })));
    }
    let coding = headers.iter().find(|(k, _)| k == "content-encoding").map(|(_, v)| v.trim().to_ascii_lowercase());
    let reader = decoded(coding.as_deref(), resp.into_reader())?;
    if coding.is_some() {
        headers.retain(|(k, _)| k != "content-encoding" && k != "content-length");
    }
    Ok((status, headers, cookies, reader))
}

/// A body in the coding its `Content-Encoding` names, decoded: ureq decodes gzip and brotli,
/// and leaves `deflate`; another coding is an error that names it, where the program got the
/// encoded bytes as text (FEEDBACK36.md, Q-438; branch feedback36-39).
fn decoded(coding: Option<&str>, reader: Body) -> Result<Body, String> {
    Ok(match coding {
        None | Some("" | "identity") => reader,
        Some("gzip" | "x-gzip") => Box::new(flate2::read::MultiGzDecoder::new(reader)),
        Some("br") => Box::new(brotli_decompressor::Decompressor::new(reader, 8 << 10)),
        Some("deflate") => {
            // `deflate` is zlib's format, though some servers send the raw one: the first two
            // bytes tell them apart.
            let mut r = std::io::BufReader::new(reader);
            let head = std::io::BufRead::fill_buf(&mut r).map_err(|e| e.to_string())?;
            let zlib = head.len() >= 2 && head[0] & 0x0f == 8 && (u16::from(head[0]) << 8 | u16::from(head[1])) % 31 == 0;
            if zlib {
                Box::new(flate2::read::ZlibDecoder::new(r))
            } else {
                Box::new(flate2::read::DeflateDecoder::new(r))
            }
        }
        Some(other) => return Err(format!("the answer is in a coding Quillo cannot read: content-encoding {}", other)),
    })
}

/// The final answer after interim ones, read from what followed them.
fn after_interim(o: Opts, url: &str, method: &str, mut rest: Body) -> Result<Head, String> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let mut hs = [httparse::EMPTY_HEADER; 100];
        let mut r = httparse::Response::new(&mut hs);
        match r.parse(&buf) {
            Ok(httparse::Status::Complete(n)) => {
                let status = r.code.unwrap_or(0);
                if (100..200).contains(&status) && status != 101 {
                    buf.drain(..n);
                    continue;
                }
                let mut headers: Vec<(String, String)> = Vec::new();
                let mut cookies = Vec::new();
                for h in r.headers.iter() {
                    let (k, v) = (h.name.to_ascii_lowercase(), String::from_utf8_lossy(h.value).trim().to_string());
                    if k == "set-cookie" {
                        cookies.push(v.clone());
                    }
                    match headers.iter_mut().find(|(n, _)| *n == k) {
                        Some((_, old)) => {
                            old.push_str(if k == "set-cookie" { "\n" } else { ", " });
                            old.push_str(&v);
                        }
                        None => headers.push((k, v)),
                    }
                }
                let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
                let after: Body = Box::new(std::io::Cursor::new(buf[n..].to_vec()).chain(rest));
                let framed: Body = if method == "HEAD" || matches!(status, 204 | 304) {
                    Box::new(std::io::empty())
                } else if header("transfer-encoding").is_some_and(|t| t.to_ascii_lowercase().contains("chunked")) {
                    Box::new(Dechunk { inner: after, left: 0, done: false })
                } else if let Some(len) = header("content-length").and_then(|l| l.parse::<u64>().ok()) {
                    Box::new(after.take(len))
                } else {
                    after
                };
                let coding = header("content-encoding").map(|c| c.trim().to_ascii_lowercase());
                let reader = decoded(coding.as_deref(), framed)?;
                headers.retain(|(k, _)| k != "content-encoding" && k != "transfer-encoding" && (coding.is_none() || k != "content-length"));
                return Ok((status, headers, cookies, reader));
            }
            Ok(httparse::Status::Partial) if buf.len() < 100 << 10 => {
                let n = rest.read(&mut chunk).map_err(|e| timed_out(o, url, e.to_string()))?;
                if n == 0 {
                    return Err(format!("{}: the connection closed after an interim (1xx) answer, with no final one", url));
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            _ => return Err(format!("{}: a bad answer after an interim (1xx) one", url)),
        }
    }
}

/// A chunked body, decoded as it arrives.
struct Dechunk {
    inner: Body,
    /// Bytes left in this chunk.
    left: u64,
    done: bool,
}

impl Dechunk {
    fn line(&mut self) -> std::io::Result<String> {
        let mut l = Vec::new();
        let mut b = [0u8; 1];
        while self.inner.read(&mut b)? == 1 && b[0] != b'\n' {
            l.push(b[0]);
            if l.len() > 4096 {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "a chunk's size line is too long"));
            }
        }
        Ok(String::from_utf8_lossy(&l).trim().to_string())
    }
}

impl Read for Dechunk {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.done || out.is_empty() {
            return Ok(0);
        }
        if self.left == 0 {
            let size = self.line()?;
            let size = size.split(';').next().unwrap_or("").trim();
            self.left = u64::from_str_radix(size, 16).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "a bad chunk size"))?;
            if self.left == 0 {
                self.done = true;
                return Ok(0);
            }
        }
        let want = out.len().min(self.left as usize);
        let n = self.inner.read(&mut out[..want])?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        self.left -= n as u64;
        if self.left == 0 {
            self.line()?;
        }
        Ok(n)
    }
}

/// A call cut short by its `timeout` says so, in those words.
fn timed_out(o: Opts, url: &str, e: String) -> String {
    if o.timeout_ms > 0 && e.contains("timed out") {
        return format!("{}: took more than {} s (its timeout)", url, o.timeout_ms as f64 / 1000.0);
    }
    // `public_only` refusing an address happens as the name is resolved, so
    // the client calls it a failed lookup: say what refused it (FEEDBACK5.md,
    // Q-052; branch feedback6-fixes).
    if let Some(at) = e.find(" is not an address on the public internet") {
        let host = e[..at].rsplit(": ").next().unwrap_or("").trim();
        return format!("{}: refused by public_only: {} is not on the public internet", url, host);
    }
    e
}

/// Answers kept by `cache`: by method, URL, headers and options, until
/// when, and their size. At most `CACHE_BYTES` of bodies are kept; past
/// that, those that end soonest go first.
struct Cached {
    until: Instant,
    reply: Reply,
    size: usize,
}

const CACHE_BYTES: usize = 64 << 20;

fn cache() -> std::sync::MutexGuard<'static, (HashMap<String, Cached>, usize)> {
    static CACHE: OnceLock<Mutex<(HashMap<String, Cached>, usize)>> = OnceLock::new();
    CACHE.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn cache_key(o: Opts, method: &str, url: &str, headers: &[(String, String)]) -> String {
    let mut hs: Vec<String> = headers.iter().map(|(k, v)| format!("{}:{}", k.to_ascii_lowercase(), v)).collect();
    hs.sort();
    format!("{} {} {} {}\n{}", method, url, o.redirects, o.public_only, hs.join("\n"))
}

fn cached(key: &str) -> Option<Reply> {
    let mut c = cache();
    let fresh = c.0.get(key).map(|e| e.until > Instant::now());
    match fresh {
        Some(true) => c.0.get(key).map(|e| e.reply.clone()),
        Some(false) => {
            if let Some(e) = c.0.remove(key) {
                c.1 -= e.size;
            }
            None
        }
        None => None,
    }
}

fn keep(key: String, secs: u32, reply: &Reply) {
    let size = key.len() + reply.body.len() + reply.headers.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>();
    if size > CACHE_BYTES / 4 {
        return;
    }
    let mut c = cache();
    let now = Instant::now();
    let (map, total) = &mut *c;
    if let Some(old) = map.remove(&key) {
        *total -= old.size;
    }
    if *total + size > CACHE_BYTES {
        map.retain(|_, e| {
            let keep = e.until > now;
            if !keep {
                *total -= e.size;
            }
            keep
        });
        let mut ends: Vec<(Instant, String)> = map.iter().map(|(k, e)| (e.until, k.clone())).collect();
        ends.sort();
        for (_, k) in ends {
            if *total + size <= CACHE_BYTES {
                break;
            }
            if let Some(e) = map.remove(&k) {
                *total -= e.size;
            }
        }
    }
    *total += size;
    map.insert(key, Cached { until: now + Duration::from_secs(secs as u64), reply: reply.clone(), size });
}

fn do_request(it: &mut Interp, fname: &str, method: &str, url: &Value, body: &Value, headers: &Value, opts: Option<&Value>, line: u32) -> R {
    not_in_component(it, fname, line)?;
    let o = opts_of(opts, fname, line, &it.base_dir.clone())?;
    if o.cache > 0 && method != "GET" {
        return fail(line, format!("`{}`: `cache` keeps the answers of GET requests, not {}", fname, method));
    }
    let Value::Str(url) = url else {
        return fail(line, format!("`{}` expects a URL string, got {}", fname, url.type_name()));
    };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Ok(Value::err(Value::str(format!("http: not an http(s) URL: {}", url))));
    }
    let hdrs: Vec<(String, String)> = match headers {
        Value::Map(m) => m.iter().map(|(k, v)| (k.to_string(), v.display())).collect(),
        Value::Nil => Vec::new(),
        other => return fail(line, format!("`{}` expects headers as a map, got {}", fname, other.type_name())),
    };
    let payload: Option<(String, Vec<u8>)> = match body {
        Value::Nil => None,
        Value::Str(s) => Some(("text/plain; charset=utf-8".into(), s.as_bytes().to_vec())),
        Value::Html(h) => Some(("text/html; charset=utf-8".into(), h.as_bytes().to_vec())),
        other => Some(("application/json".into(), other.json_text().into_bytes())),
    };
    let key = (o.cache > 0).then(|| cache_key(o, method, url, &hdrs));
    let reply = match key.as_deref().and_then(cached) {
        Some(r) => r,
        None => {
            let (method, url) = (method.to_string(), url.to_string());
            let res = it.unlocked(move || send(o, &method, &url, &hdrs, payload));
            let reply = match res {
                Ok(r) => r,
                Err(e) => return Ok(Value::err(Value::str(format!("http: {}", e)))),
            };
            if let Some(key) = key {
                if (200..300).contains(&reply.status) {
                    keep(key, o.cache, &reply);
                }
            }
            reply
        }
    };
    let json = serde_json::from_str::<serde_json::Value>(&reply.body).map(|j| Value::from_json(&j)).unwrap_or(Value::Nil);
    let mut m = Map::new();
    m.insert("status".into(), Value::Int(reply.status as i64));
    m.insert("ok".into(), Value::Bool((200..300).contains(&reply.status)));
    m.insert("headers".into(), Value::map(reply.headers.into_iter().map(|(k, v)| (k.into(), Value::str(v))).collect()));
    // Each `Set-Cookie` as it came (FEEDBACK36.md, Q-435; branch feedback36-39).
    m.insert("cookies".into(), Value::list(reply.cookies.into_iter().map(Value::str).collect()));
    m.insert("body".into(), Value::str(reply.body));
    m.insert("json".into(), json);
    Ok(Value::map(m))
}

fn a(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Nil)
}

/// `http.get(url, headers = {}, options = {})`
fn get(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.get", "GET", &a(&args, 0), &Value::Nil, &a(&args, 1), args.get(2), line)
}

/// `http.delete(url, headers = {}, options = {})`
fn delete(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.delete", "DELETE", &a(&args, 0), &Value::Nil, &a(&args, 1), args.get(2), line)
}

/// `http.post(url, body = null, headers = {}, options = {})`
fn post(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.post", "POST", &a(&args, 0), &a(&args, 1), &a(&args, 2), args.get(3), line)
}

fn put(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.put", "PUT", &a(&args, 0), &a(&args, 1), &a(&args, 2), args.get(3), line)
}

fn patch(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.patch", "PATCH", &a(&args, 0), &a(&args, 1), &a(&args, 2), args.get(3), line)
}

/// `http.request(method, url, body = null, headers = {}, options = {})`
fn request(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let method = match a(&args, 0) {
        Value::Str(m) => m.to_ascii_uppercase(),
        other => return fail(line, format!("`http.request` expects a method string first, got {}", other.type_name())),
    };
    do_request(it, "http.request", &method, &a(&args, 1), &a(&args, 2), &a(&args, 3), args.get(4), line)
}

type Body = Box<dyn Read + Send + Sync>;

fn open(o: Opts, method: &str, url: &str, headers: &[(String, String)], body: Option<(String, Vec<u8>)>) -> Result<Head, String> {
    let (url, resp) = follow(o, true, method, url, headers, body)?;
    head_of(o, &url, method, resp)
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Events,
    Lines,
    Text,
}

fn parsed(text: &str) -> Value {
    serde_json::from_str::<serde_json::Value>(text).map(|j| Value::from_json(&j)).unwrap_or_else(|_| Value::str(text))
}

/// Longest line or event `http.stream` waits for the end of: a server that
/// never ends one cannot fill the program's memory.
const MAX_PIECE: usize = 16 << 20;

/// What has arrived of a stream and is not handed over yet. `scanned`:
/// how much of `buf` is known to hold no end of a line or event, so each
/// byte is looked at about once however the stream is cut up.
#[derive(Default)]
struct Pending {
    buf: Vec<u8>,
    scanned: usize,
    /// The last event id the stream gave.
    id: Option<String>,
}

/// Where the first event in `b` ends at a blank line (`\n\n`, `\r\n\r\n`
/// or a mix), looking from `from`: its end, and where the next one starts.
fn event_end(b: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while let Some(j) = b[i..].iter().position(|x| *x == b'\n').map(|j| j + i) {
        match (b.get(j + 1), b.get(j + 2)) {
            (Some(b'\n'), _) => return Some((j, j + 2)),
            (Some(b'\r'), Some(b'\n')) => return Some((j, j + 3)),
            _ => i = j + 1,
        }
    }
    None
}

/// The whole pieces at the front of what arrived (all of it at the end), as
/// the arguments the function gets; an error when one is longer than
/// `MAX_PIECE`.
fn pieces(p: &mut Pending, kind: Kind, end: bool) -> Result<Vec<Vec<Value>>, String> {
    let mut out = Vec::new();
    let buf = &mut p.buf;
    match kind {
        Kind::Text => {
            // Whole characters only: the rest of one comes with the next piece.
            let n = match std::str::from_utf8(buf) {
                Ok(_) => buf.len(),
                Err(e) if e.error_len().is_none() && !end => e.valid_up_to(),
                Err(_) => buf.len(),
            };
            if n > 0 {
                let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                buf.drain(..n);
                out.push(vec![Value::str(text)]);
            }
        }
        Kind::Lines => {
            let mut start = 0;
            let mut at = p.scanned;
            loop {
                let i = match buf[at..].iter().position(|b| *b == b'\n') {
                    Some(i) => at + i,
                    None if end && start < buf.len() => buf.len(),
                    None => break,
                };
                let line = String::from_utf8_lossy(&buf[start..i]);
                let line = line.trim_end_matches('\r');
                if !line.trim().is_empty() {
                    out.push(vec![parsed(line)]);
                }
                start = (i + 1).min(buf.len());
                at = start;
            }
            buf.drain(..start);
            p.scanned = buf.len();
        }
        Kind::Events => {
            let mut start = 0;
            // A blank line may have begun in what was looked at before.
            let mut from = p.scanned.saturating_sub(2);
            loop {
                let (stop, next) = match event_end(buf, from.max(start)) {
                    Some(e) => e,
                    None if end && !String::from_utf8_lossy(&buf[start..]).trim().is_empty() => (buf.len(), buf.len()),
                    None => break,
                };
                let block = String::from_utf8_lossy(&buf[start..stop]).replace("\r\n", "\n");
                let (mut data, mut name): (Vec<&str>, Option<&str>) = (Vec::new(), None);
                let mut retry = None;
                for l in block.lines() {
                    let (field, value) = l.split_once(':').map_or((l, ""), |(f, v)| (f, v.strip_prefix(' ').unwrap_or(v)));
                    match field {
                        "data" => data.push(value),
                        "event" => name = Some(value),
                        // The last id is kept for the events after it, as a browser's
                        // `EventSource` keeps it (FEEDBACK36.md, Q-444; branch feedback36-39).
                        "id" if !value.contains('\0') => p.id = Some(value.to_string()).filter(|v| !v.is_empty()),
                        "retry" => retry = value.parse::<i64>().ok(),
                        _ => {}
                    }
                }
                if !data.is_empty() {
                    out.push(vec![
                        parsed(&data.join("\n")),
                        name.map_or(Value::Nil, Value::str),
                        p.id.as_deref().map_or(Value::Nil, Value::str),
                        retry.map_or(Value::Nil, Value::Int),
                    ]);
                }
                start = next;
                from = next;
                if start >= buf.len() {
                    break;
                }
            }
            buf.drain(..start.min(buf.len()));
            p.scanned = buf.len();
        }
    }
    if buf.len() > MAX_PIECE {
        return Err(format!("http: a {} longer than {} bytes", if matches!(kind, Kind::Events) { "event" } else { "line" }, MAX_PIECE));
    }
    Ok(out)
}

/// `http.stream(url, f, { method = "GET", body, headers })`: `{ status, ok,
/// headers }` once the answer has ended (with `body`, unread by `f`, when it
/// is not a success), or an error value when it could not be had.
fn stream(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    not_in_component(it, "http.stream", line)?;
    let Value::Str(url) = a(&args, 0) else {
        return fail(line, format!("`http.stream` expects a URL string, got {}", a(&args, 0).type_name()));
    };
    let f = match a(&args, 1) {
        f @ (Value::Func(_) | Value::Builtin(..)) => f,
        other => return fail(line, format!("`http.stream` takes a function for what arrives, got {}", other.type_name())),
    };
    let opts = match a(&args, 2) {
        Value::Map(m) => m,
        Value::Nil => std::sync::Arc::new(Map::new()),
        other => return fail(line, format!("`http.stream` takes options as a map, got {}", other.type_name())),
    };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Ok(Value::err(Value::str(format!("http: not an http(s) URL: {}", url))));
    }
    let o = opts_of(Some(&Value::Map(opts.clone())), "http.stream", line, &it.base_dir.clone())?;
    if o.cache > 0 {
        return fail(line, "`http.stream`: `cache` keeps whole answers, of `http.get`");
    }
    let body = opts.get("body").cloned().unwrap_or(Value::Nil);
    let method = match opts.get("method") {
        Some(Value::Str(m)) => m.to_ascii_uppercase(),
        _ if !matches!(body, Value::Nil) => "POST".into(),
        _ => "GET".into(),
    };
    let hdrs: Vec<(String, String)> = match opts.get("headers") {
        Some(Value::Map(m)) => m.iter().map(|(k, v)| (k.to_string(), v.display())).collect(),
        _ => Vec::new(),
    };
    let payload: Option<(String, Vec<u8>)> = match &body {
        Value::Nil => None,
        Value::Str(s) => Some(("text/plain; charset=utf-8".into(), s.as_bytes().to_vec())),
        other => Some(("application/json".into(), other.json_text().into_bytes())),
    };
    let u = url.to_string();
    let (status, headers, cookies, mut reader) = match it.unlocked(move || open(o, &method, &u, &hdrs, payload)) {
        Ok(r) => r,
        Err(e) => return Ok(Value::err(Value::str(format!("http: {}", e)))),
    };
    let ok = (200..300).contains(&status);
    let mut m = Map::new();
    m.insert("status".into(), Value::Int(status as i64));
    m.insert("ok".into(), Value::Bool(ok));
    m.insert("headers".into(), Value::map(headers.iter().map(|(k, v)| (k.into(), Value::str(v))).collect()));
    m.insert("cookies".into(), Value::list(cookies.into_iter().map(Value::str).collect()));
    if !ok {
        let bytes = it.unlocked(move || {
            let mut b = Vec::new();
            let _ = reader.take(MAX_BODY).read_to_end(&mut b);
            b
        });
        m.insert("body".into(), Value::str(String::from_utf8_lossy(&bytes)));
        return Ok(Value::map(m));
    }
    let ctype = headers.iter().find(|(k, _)| k == "content-type").map_or("", |(_, v)| v.as_str()).to_ascii_lowercase();
    let kind = if ctype.starts_with("text/event-stream") {
        Kind::Events
    } else if ctype.contains("ndjson") || ctype.contains("jsonl") || ctype.contains("json-seq") {
        Kind::Lines
    } else {
        Kind::Text
    };
    let params = match &f {
        Value::Func(c) => c.def.params.len(),
        _ => usize::MAX,
    };
    let mut pending = Pending::default();
    loop {
        // Read while other requests run; hand over what came, here.
        let (r, got) = it.unlocked(move || {
            let mut chunk = vec![0u8; 16 << 10];
            let n = reader.read(&mut chunk).map(|n| {
                chunk.truncate(n);
                chunk
            });
            (reader, n)
        });
        reader = r;
        let end = match got {
            Ok(c) if c.is_empty() => true,
            Ok(c) => {
                pending.buf.extend_from_slice(&c);
                false
            }
            Err(e) => return Ok(Value::err(Value::str(format!("http: {}", e)))),
        };
        let got = match pieces(&mut pending, kind, end) {
            Ok(g) => g,
            Err(e) => return Ok(Value::err(Value::str(e))),
        };
        for mut piece in got {
            piece.truncate(params);
            if matches!(it.call(&f, piece, line)?, Value::Bool(false)) {
                return Ok(Value::map(m));
            }
        }
        if end {
            return Ok(Value::map(m));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(kind: Kind, input: &[u8], step: usize) -> Result<Vec<String>, String> {
        let mut p = Pending::default();
        let mut got = Vec::new();
        for c in input.chunks(step) {
            p.buf.extend_from_slice(c);
            got.extend(pieces(&mut p, kind, false)?);
        }
        got.extend(pieces(&mut p, kind, true)?);
        Ok(got.iter().map(|a| a.iter().map(|v| v.repr()).collect::<Vec<_>>().join(" ")).collect())
    }

    #[test]
    fn stream_pieces_however_they_arrive() {
        let events = b"data: 1\n\nevent: tick\ndata: {\"a\": 2}\r\n\r\n: comment\ndata: x\ndata: y\n\r\ndata: last";
        let want = vec!["1 null null null", "{ a: 2 } \"tick\" null null", "\"x\\ny\" null null null", "\"last\" null null null"];
        let lines = b"{\"n\": 1}\n\n2\r\nthree\n4";
        for step in [1, 2, 3, 7, 1000] {
            assert_eq!(feed(Kind::Events, events, step).unwrap(), want, "{}", step);
            assert_eq!(feed(Kind::Lines, lines, step).unwrap(), vec!["{ n: 1 }", "2", "\"three\"", "4"], "{}", step);
        }
    }

    #[test]
    fn a_line_or_event_without_end_is_refused_at_a_limit() {
        // What the review saw: a server that never ends a line fills memory,
        // and each piece that came was decoded again from the start.
        let long = vec![b'x'; MAX_PIECE + 1];
        for kind in [Kind::Lines, Kind::Events] {
            let e = feed(kind, &long, 64 << 10).unwrap_err();
            assert!(e.contains("longer than"), "{}", e);
        }
        // Many events in one go, or one in many pieces: looked at once.
        let mut many = Vec::new();
        for i in 0..20_000 {
            many.extend_from_slice(format!("data: {}\n\n", i).as_bytes());
        }
        let t = std::time::Instant::now();
        assert_eq!(feed(Kind::Events, &many, many.len()).unwrap().len(), 20_000);
        let mut one = b"data: ".to_vec();
        one.extend(vec![b'y'; 4 << 20]);
        one.extend_from_slice(b"\n\n");
        assert_eq!(feed(Kind::Events, &one, 1024).unwrap().len(), 1);
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
    }
}
