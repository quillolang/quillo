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
//! `save`: a path, held as `write_file`'s is, that a 2xx answer's body is
//! written to as it arrives, through a file beside it that takes its place
//! once the whole has come (so a failed call leaves what was there), at most
//! `max_bytes` (64 MiB unless given); the call gives `{ status, ok, headers,
//! file, bytes }`, and an answer that is not 2xx is not written, its text in
//! `body`. It needs the fs effect as well (FEEDBACK85.md, Q-1342; branch
//! feedback82-85).
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
    /// A `site`'s call (src/sites.rs), no option of the program's: a redirect to another origin,
    /// or to what is not http(s), is not followed but is the answer.
    same_origin: bool,
    /// The hosts the calling file was confirmed for, by its block (`confirm::held_id`; 0: none
    /// held), each address and redirect held to them (FEEDBACK83.md, Q-1300, D.4; branch
    /// feedback82-85).
    held: u32,
}

const DEFAULT_OPTS: Opts = Opts { redirects: 5, public_only: false, cache: 0, timeout_ms: 0, tls: 0, same_origin: false, held: 0 };

/// The options there are; `http.stream`'s map also has its request's. `save`, a file to write the
/// answer's body to, with `max_bytes`, the most it writes: the others' alone, as `http.stream`
/// hands its answer over as it arrives (FEEDBACK85.md, Q-1342; branch feedback82-85).
pub(crate) const OPTION_NAMES: &[&str] = &["redirects", "public_only", "cache", "timeout", "ca", "cert", "key", "save", "max_bytes"];

/// Where a call's options are among its arguments, by its name.
fn options_at(name: &str) -> Option<usize> {
    match name {
        "http.get" | "http.delete" => Some(2),
        "http.post" | "http.put" | "http.patch" => Some(3),
        "http.request" => Some(4),
        _ => None,
    }
}

/// Whether a call written as `args` (`piped` of its arguments given by a pipe, not written) says
/// `save`, for `quillo check`: it writes a file, and needs the fs effect as well as http
/// (FEEDBACK85.md, Q-1342; branch feedback82-85).
pub(crate) fn save_written(name: &str, args: &[crate::ast::Expr], piped: usize) -> bool {
    use crate::ast::{Expr, Lit};
    let at = options_at(name).and_then(|i| i.checked_sub(piped)).and_then(|i| args.get(i));
    matches!(at, Some(Expr::Map(fields)) if fields.iter().any(|(k, v)| k == "save" && !matches!(v, Expr::Lit(Lit::Nil))))
}

/// Whether a call says `save`, as it runs: the fs effect too (FEEDBACK85.md, Q-1342; branch
/// feedback82-85).
pub(crate) fn saves(name: &str, args: &[Value]) -> bool {
    matches!(options_at(name).and_then(|i| args.get(i)), Some(Value::Map(m)) if m.get("save").is_some_and(|v| !matches!(v, Value::Nil)))
}
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
/// `notification.send` goes through the same proxy, and judges the address the same way
/// (FEEDBACK67.md, B.3, second review; branch pwa).
pub(crate) fn proxy_for(url: &str) -> Option<String> {
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
/// The host of `url` as the client reads it (WHATWG), an IPv6 address in `[...]`; empty if it
/// does not parse. Split by hand, `http://169.254.169.254\x@example.com` was example.com here
/// and the metadata address to the client (found in review of FEEDBACK51.md, Q-650).
pub(crate) fn host_of(url: &str) -> String {
    match url::Url::parse(url).ok().and_then(|u| u.host().map(|h| h.to_owned())) {
        Some(url::Host::Ipv6(a)) => format!("[{}]", a),
        Some(h) => h.to_string(),
        None => String::new(),
    }
}
const STREAM_NAMES: &[&str] = &["method", "body", "headers"];

/// The longest `timeout`, in seconds.
const MAX_TIMEOUT_SECS: f64 = 300.0;

/// The longest `cache`: a day.
pub(crate) const MAX_CACHE_SECS: i64 = 86_400;

/// `saving`: set when the options say `save` or `max_bytes`, found by their names as the keys are
/// looked over, so that a call without them looks nothing up for them (FEEDBACK85.md, Q-1342;
/// branch feedback82-85).
fn opts_of(v: Option<&Value>, fname: &str, line: u32, base: &std::path::Path, saving: &mut bool) -> Result<Opts, Ctrl> {
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
    *saving = m.keys().any(|k| matches!(k.as_ref(), "save" | "max_bytes"));
    if stream && *saving {
        // A null is no option, as `http.get` reads `save: null` (FEEDBACK85.md, Q-1342, found by
        // the coverage tests; branch feedback82-85): `http.stream(url, f, { save: null })` was
        // stopped where `quillo check` said ok.
        if let Some(k) = ["save", "max_bytes"].into_iter().find(|k| m.get(*k).is_some_and(|v| !matches!(v, Value::Nil))) {
            return fail(line, format!("`http.stream`: `{}` is for the calls that read an answer whole (http.get and the others): http.stream hands it over as it arrives", k));
        }
    }
    if let Some(k) = m.keys().map(|k| k.as_ref()).find(|k| !OPTION_NAMES.contains(k) && !(stream && STREAM_NAMES.contains(k))) {
        // `save` and `max_bytes` are not `http.stream`'s (FEEDBACK85.md, Q-1342; branch feedback82-85).
        let known = if stream { [STREAM_NAMES, OPTION_NAMES].concat().into_iter().filter(|k| !matches!(*k, "save" | "max_bytes")).collect() } else { OPTION_NAMES.to_vec() };
        return fail(line, format!("`{}`: unknown option `{}` (the options are {})", fname, k, known.join(", ")));
    }
    // Files are named from the program's folder, as `read_file` names them.
    let file = |name: &str| -> Result<Option<String>, Ctrl> {
        match m.get(name) {
            None | Some(Value::Nil) => Ok(None),
            Some(Value::Str(s) | Value::Css(s)) if name == "ca" && &**s == "system" => Ok(Some("system".into())),
            Some(Value::Str(s) | Value::Css(s)) if !s.is_empty() => Ok(Some(base.join(&**s).to_string_lossy().into_owned())),
            Some(other) => fail(line, format!("`{}`: `{}` is the path of a PEM file{}, got {}", fname, name, if name == "ca" { " or \"system\"" } else { "" }, other.repr())),
        }
    };
    let (ca, cert, key) = (file("ca")?, file("cert")?, file("key")?);
    if cert.is_some() != key.is_some() {
        return fail(line, format!("`{}`: `cert` and `key` go together: a certificate, and its key", fname));
    }
    Ok(Opts { redirects, public_only, cache, timeout_ms, tls: tls_number(ca, cert, key), same_origin: false, held: 0 })
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
            // RFC 8215's local-use NAT64, 64:ff9b:1::/48: a network's own translator, to its own
            // addresses (FEEDBACK67.md, B.3, review; branch pwa).
            if s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1 {
                return false;
            }
            if s[0] == 0x2002 {
                return public_v4(Ipv4Addr::new((s[1] >> 8) as u8, s[1] as u8, (s[2] >> 8) as u8, s[2] as u8));
            }
            let embedded = || Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
            // IPv4-translated, ::ffff:0:a.b.c.d (RFC 2765, 6145): a stateless translator (SIIT)
            // takes it to the IPv4 address, loopback and the metadata service among them, which
            // was taken for public (FEEDBACK75.md, Q-1093; branch feedback75).
            if s[0..6] == [0, 0, 0, 0, 0xffff, 0] {
                return public_v4(embedded());
            }
            // ISATAP, an interface of ::5efe:a.b.c.d (or ::200:5efe:, RFC 5214) under any prefix:
            // sent inside IPv4 to the address it ends in (Q-1093).
            if s[4] & 0xfdff == 0 && s[5] == 0x5efe && !public_v4(embedded()) {
                return false;
            }
            !(a.is_unspecified()
                || a.is_loopback()
                || a.is_multicast()
                // IPv4-compatible, and the rest of 0::/8, which IANA keeps (none of it is anyone's
                // address on the internet): ::/96 was refused, and ::ffff:0:0/96 beside it was not
                // (Q-1093).
                || s[0] < 0x0100
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
pub(crate) fn resolve_public(netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
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
    let key = (0, o.public_only, stream, o.tls, proxy.clone());
    if let Some(a) = agents.get(&key) {
        return Ok(a.clone());
    }
    let b = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        // Each redirect is followed here (see `follow`), its address judged, never by the
        // client: it sent the program's headers to another port, through a proxy it went past
        // `public_only`, and a `file:` one panicked (FEEDBACK51.md, Q-649 and Q-650; branch
        // feedback51-fixes).
        .redirects(0)
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

/// One call: its answer, the head read, the body not yet. `left`: what is left of the whole
/// call's `timeout`.
fn call(o: Opts, stream: bool, method: &str, url: &str, headers: &[(String, String)], body: Option<&(String, Vec<u8>)>, left: Option<Duration>) -> Result<ureq::Response, String> {
    if o.public_only && proxy_for(url).is_some() {
        // The host and port the client will ask the proxy for.
        let u = url::Url::parse(url).map_err(|e| format!("{}: {}", url, e))?;
        let host = host_of(url);
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let port = u.port_or_known_default().unwrap_or(80);
        let at = if host.contains(':') { format!("[{}]:{}", host, port) } else { format!("{}:{}", host, port) };
        resolve_public(&at).map_err(|e| timed_out(o, url, format!("{}: {}", url, e)))?;
    }
    let mut req = agent(o, stream, url)?.request(method, url);
    if let Some(left) = left {
        req = req.timeout(left);
    }
    // Those that say how the request is framed, or are the connection's own, are the client's to
    // write, from the body it sends: a program's `content-length: 3` before a longer body made the
    // upstream read the rest as a second request, and a route that passed on a visitor's headers
    // let them send any request from the server's address. They are left out, as `fetch` leaves
    // them out in a browser, where these calls run in a component (FEEDBACK75.md, Q-1089; branch
    // feedback75).
    for (k, v) in headers.iter().filter(|(k, _)| !framing_header(k)) {
        req = req.set(k, v);
    }
    if let Some((ctype, _)) = body {
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
            req = req.set("content-type", ctype);
        }
    }
    // What the client cannot take is an error value, not a crash of the route (Q-649).
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || match body {
        Some((_, bytes)) => req.send_bytes(bytes),
        None => req.call(),
    }))
    .map_err(|_| format!("{}: the HTTP client could not make the call", url))?;
    match res {
        Ok(r) | Err(ureq::Error::Status(_, r)) => Ok(r),
        Err(e) => Err(timed_out(o, url, e.to_string())),
    }
}

/// Whether a request header is one the client writes itself, never the program (see `call`): the
/// body's framing and the connection's own headers. A program's `Host` is still sent, as before:
/// programs reach a virtual host by its address with it, and it frames nothing (Quillo Coder's
/// call for FEEDBACK75.md, Q-1089, 2026-11-19, on the list of decisions for the user; branch
/// feedback75).
fn framing_header(name: &str) -> bool {
    let name = name.trim();
    ["content-length", "transfer-encoding", "connection", "keep-alive", "te", "trailer", "upgrade"].iter().any(|h| name.eq_ignore_ascii_case(h))
}

/// `call`, and its redirects, each followed here: a `307` or `308` with the same method and
/// body, as RFC 9110 §15.4.8 and `fetch` do (FEEDBACK36.md, Q-437; branch feedback36-39); a
/// `301`, `302` or `303` after a write as a GET. To another origin (scheme, host and port), the
/// program's headers stay behind but for those that say what it takes (`accept`, `user-agent`,
/// ...): `x-api-key` went on to another port (FEEDBACK51.md, Q-649; branch feedback51-fixes).
/// A site's call (`same_origin`) puts the `Set-Cookie`s of each answer it went past in `passed`.
fn follow(o: Opts, stream: bool, method: &str, url: &str, headers: &[(String, String)], body: Option<(String, Vec<u8>)>, passed: &mut Vec<String>) -> Result<(String, ureq::Response), String> {
    let mut url = url.to_string();
    let mut headers = headers.to_vec();
    let mut hops = 0;
    let (mut method, mut body) = (method.to_string(), body);
    // `timeout` is the whole call's, its redirects in it: each had all of it, so a site's 30 s
    // were 180 over 5 redirects, where the client it had kept one (FEEDBACK71.md, Q-973; branch
    // security).
    let until = (o.timeout_ms > 0).then(|| Instant::now() + Duration::from_millis(o.timeout_ms as u64));
    loop {
        // Each address, the first and each one it is sent on to (FEEDBACK41.md, Q-478; branch
        // feedback40-43). A refusal is said once for each host, where it was an error value that
        // nothing in the log showed (FEEDBACK83.md, Q-1302; branch feedback82-85).
        crate::policy::get().http(&url).inspect_err(|e| refused_once(&url, e))?;
        crate::confirm::http_ok(o.held, &url)?;
        let left = match until.map(|u| u.saturating_duration_since(Instant::now())) {
            Some(d) if d.is_zero() => return Err(timed_out(o, &url, "timed out".into())),
            left => left,
        };
        let resp = call(o, stream, &method, &url, &headers, body.as_ref(), left)?;
        let status = resp.status();
        let again = o.redirects > 0 && matches!(status, 301 | 302 | 303 | 307 | 308);
        // As browsers do: a 301, 302 or 303 after anything but a GET or a HEAD is a GET with no
        // body.
        if again && matches!(status, 301 | 302 | 303) && !matches!(method.as_str(), "GET" | "HEAD") {
            method = "GET".into();
            body = None;
            headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type") && !k.eq_ignore_ascii_case("content-length"));
        }
        let next = if again { resp.header("location").and_then(|l| join_url(&url, l)) } else { None };
        // A site's call goes on only within the site: its server sent Quillo anywhere, inside the
        // network too, and the start of what was there came back in the error (FEEDBACK71.md,
        // Q-973; branch security).
        let next = next.filter(|n| !o.same_origin || origin_of(n) == origin_of(&url));
        let Some(next) = next else { return Ok((url, resp)) };
        // A site's own cookies, set on the way: every answer here is from its origin (Q-973).
        if o.same_origin {
            passed.extend(resp.all("set-cookie").into_iter().map(str::to_string));
        }
        hops += 1;
        if hops > o.redirects {
            return Err(format!("{}: more than {} redirects", url, o.redirects));
        }
        if origin_of(&next) != origin_of(&url) {
            headers.retain(|(k, _)| ["accept", "accept-language", "content-type", "user-agent"].iter().any(|h| k.eq_ignore_ascii_case(h)));
        }
        url = next;
    }
}

/// Says in the log, once for each host, that the policy refused a call to it.
fn refused_once(url: &str, why: &str) {
    static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let host = host_of(url);
    let mut said = SAID.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if said.len() < 256 && !said.contains(&host) {
        eprintln!("quillo: a call to {} was refused: {}", host, why);
        said.push(host);
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

/// The longest answer read whole: `MAX_BODY`, or `--max-memory` where that is less, as the
/// answer's text is a value (FEEDBACK71.md, Q-977; branch security). The limit is looked at once a
/// call, before its round trip, as no size is known before.
fn body_room() -> u64 {
    crate::httpd::value_most().min(MAX_BODY)
}

/// What an answer longer than `MAX_BODY` is told.
fn too_long(url: &str) -> String {
    format!("{}: the answer is longer than {} MiB, the most that is read whole: read it with http.stream", url, MAX_BODY >> 20)
}

/// Why `send` gave no answer: what went wrong, said, or `--max-memory`'s refusal of a body that
/// long, told by its kind: a server's own words in an error (its `Content-Encoding`) were taken
/// for the stop (FEEDBACK71.md, Q-977, found in review; branch security).
enum Unsent {
    Said(String),
    OverMemory,
}

impl From<String> for Unsent {
    fn from(s: String) -> Unsent {
        Unsent::Said(s)
    }
}

/// What the answers being read whole take, every call's at once, past the first `READ_FREE` of
/// each: at most `READING_MOST`, and a call whose answer would take more is an error value. Calls
/// run off the interpreter lock, up to `--workers` at once, and each answer could take its limit
/// (64 MiB): 64 answers of 200 KB of gzip asked for about 13 GB (FEEDBACK75.md, Q-1072; branch
/// feedback75).
static READING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const READING_MOST: u64 = 256 << 20;
const READ_FREE: u64 = 1 << 20;
/// The first piece an answer is read into; each next one as large as all before it.
const READ_PIECE: u64 = 16 << 10;

/// Why `read_whole` gave no body.
enum Unread {
    Io(std::io::Error),
    /// The answer is longer than the most read.
    Longer,
    /// All the answers being read at once would take more than `READING_MOST`.
    Busy,
}

/// What this call has taken from `READING`, given back when it is dropped.
struct Reading(u64);

impl Reading {
    /// The call's answer takes `total` bytes: what that is past `READ_FREE` taken from what all
    /// may take at once, or false when it does not fit.
    fn hold(&mut self, total: u64) -> bool {
        let want = total.saturating_sub(READ_FREE);
        if want <= self.0 {
            return true;
        }
        let more = want - self.0;
        let mut now = READING.load(std::sync::atomic::Ordering::SeqCst);
        loop {
            let Some(next) = now.checked_add(more).filter(|t| *t <= READING_MOST) else { return false };
            match READING.compare_exchange_weak(now, next, std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst) {
                Ok(_) => break,
                Err(seen) => now = seen,
            }
        }
        self.0 = want;
        true
    }
}

impl Drop for Reading {
    fn drop(&mut self) {
        READING.fetch_sub(self.0, std::sync::atomic::Ordering::SeqCst);
    }
}

/// An answer's body, read to its end, or `Unread::Longer` past `most` bytes, never holding more
/// than `most`: `read_to_end` of `most + 1` doubled its buffer, so reaching 64 MiB + 1 made one of
/// 128 MiB and copied the old one into it, about 230 MB a call for 200 KB of gzip (FEEDBACK75.md,
/// Q-1072; branch feedback75). Read 16 KiB at a time into pieces, each with room for as much as all
/// before it (the first 16 KiB, the last to end at `most`), none ever grown or copied, and joined
/// once the answer has ended: one past the limit is dropped holding at most the limit, and an
/// ordinary one is one piece. A piece's room is only asked for, not written, until bytes come.
/// What has arrived is held to `READING_MOST` with every other call's, and the join while it is
/// made, the whole and the largest piece: room asked for ahead (an answer that stalled after
/// 32 MiB held 64) tied up the budget, and every other call over 1 MiB was refused (found in
/// review).
fn read_whole(mut reader: impl Read, most: u64) -> Result<Vec<u8>, Unread> {
    let most = usize::try_from(most).unwrap_or(usize::MAX);
    let mut pieces: Vec<Vec<u8>> = Vec::new();
    let mut total = 0usize;
    let mut held = Reading(0);
    let mut chunk = [0u8; READ_PIECE as usize];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Unread::Io(e)),
        };
        if n > most - total {
            return Err(Unread::Longer);
        }
        if !held.hold((total + n) as u64) {
            return Err(Unread::Busy);
        }
        let mut rest = &chunk[..n];
        while !rest.is_empty() {
            let room = pieces.last().map_or(0, |p| p.capacity() - p.len());
            if room == 0 {
                let want = total.max(READ_PIECE as usize).min(most - total).max(rest.len());
                let mut piece = Vec::new();
                piece.try_reserve_exact(want).map_err(|_| Unread::Busy)?;
                pieces.push(piece);
                continue;
            }
            let k = room.min(rest.len());
            if let Some(piece) = pieces.last_mut() {
                piece.extend_from_slice(&rest[..k]);
            }
            rest = &rest[k..];
            total += k;
        }
    }
    if pieces.len() <= 1 {
        return Ok(pieces.pop().unwrap_or_default());
    }
    let largest = pieces.iter().map(Vec::len).max().unwrap_or(0);
    if !held.hold((total + largest) as u64) {
        return Err(Unread::Busy);
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(total).map_err(|_| Unread::Busy)?;
    for piece in pieces {
        bytes.extend_from_slice(&piece);
    }
    Ok(bytes)
}

/// What a call is told when the answers being read at once are too many (`READING_MOST`).
fn too_busy(url: &str) -> String {
    format!("{}: the answers being read at once would take more than the {} MiB read at once; try again shortly, or read it with http.stream", url, READING_MOST >> 20)
}

/// `most`: the longest answer read, `MAX_BODY` or less where `--max-memory` leaves less (see
/// `body_room`); past a limit of its, `Unsent::OverMemory`.
fn send(o: Opts, method: &str, url: &str, headers: &[(String, String)], body: Option<(String, Vec<u8>)>, most: u64) -> Result<Reply, Unsent> {
    let mut passed = Vec::new();
    let (url, resp) = follow(o, false, method, url, headers, body, &mut passed)?;
    let (status, headers, last, reader) = head_of(o, &url, method, resp)?;
    // Those set on the way first, so that the last answer's have the last word.
    passed.extend(last);
    let cookies = passed;
    let body = read_text(o, &url, reader, most)?;
    Ok(Reply { status, headers, cookies, body })
}

/// An answer's body read whole, as its text, held to `most` (`send`'s, and `save`'s for an answer
/// it does not write: FEEDBACK85.md, Q-1342; branch feedback82-85).
fn read_text(o: Opts, url: &str, reader: Body, most: u64) -> Result<String, Unsent> {
    let url = url.to_string();
    // A site's call reads the same way, as it is made here (FEEDBACK75.md, Q-1072; branch
    // feedback75).
    let bytes = match read_whole(reader, most) {
        Ok(b) => b,
        Err(Unread::Io(e)) => return Err(Unsent::Said(timed_out(o, &url, e.to_string()))),
        Err(Unread::Busy) => return Err(Unsent::Said(too_busy(&url))),
        // Read no further than one value may be (FEEDBACK71.md, Q-977; branch security).
        Err(Unread::Longer) if most < MAX_BODY => return Err(Unsent::OverMemory),
        // A longer one is an error, where it was cut at the limit and said nothing: cut JSON was
        // `json: null` (FEEDBACK49.md, Q-599; branch main). A site's call has no `http.stream` to
        // read it with (FEEDBACK71.md, Q-973; branch security).
        Err(Unread::Longer) => {
            return Err(Unsent::Said(if o.same_origin { format!("{}: the answer is longer than {} MiB, the most a site's call reads", url, MAX_BODY >> 20) } else { too_long(&url) }));
        }
    };
    // Made text held too: what is not UTF-8 takes three bytes a byte as U+FFFD, and 1 MB of
    // 0xFF read to a 3 MB text under a 1 MB limit (FEEDBACK71.md, Q-977, found in review; branch
    // security).
    // Text that is UTF-8 is the buffer itself, not a copy of it (FEEDBACK75.md, Q-1072; branch
    // feedback75).
    Ok(if most < MAX_BODY {
        crate::value::lossy_held(bytes).map_err(|_| Unsent::OverMemory)?
    } else {
        String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
    })
}

/// Where `save` writes a call's answer: the file, as the program named it, and the most it writes.
struct SaveTo {
    full: std::path::PathBuf,
    shown: String,
    most: u64,
}

/// `save` and `max_bytes` of a call's options: the file judged as `write_file` judges one (the
/// program's folder, or one `--allow-path` named; none of the program's own files), or the error
/// value that says why not (FEEDBACK85.md, Q-1342; branch feedback82-85).
fn save_to(it: &Interp, fname: &str, opts: Option<&Value>, o: Opts, line: u32) -> Result<Result<Option<SaveTo>, Value>, Ctrl> {
    let Some(Value::Map(m)) = opts else { return Ok(Ok(None)) };
    let most = match m.get("max_bytes") {
        None | Some(Value::Nil) => None,
        Some(Value::Int(n)) if *n > 0 => Some(*n as u64),
        Some(other) => return fail(line, format!("`{}`: `max_bytes` is the most bytes `save` writes, a whole number more than 0, got {}", fname, other.display())),
    };
    let p = match m.get("save") {
        None | Some(Value::Nil) if most.is_some() => {
            return fail(line, format!("`{}`: `max_bytes` goes with `save`; an answer read whole is held to {} MiB", fname, MAX_BODY >> 20));
        }
        None | Some(Value::Nil) => return Ok(Ok(None)),
        Some(Value::Str(s) | Value::Css(s)) if !s.is_empty() => s.to_string(),
        Some(other) => return fail(line, format!("`{}`: `save` is the path of the file to write the answer to, got {}", fname, other.repr())),
    };
    if o.cache > 0 {
        return fail(line, format!("`{}`: `cache` keeps answers in memory and `save` writes one to its file: one or the other", fname));
    }
    let said = |why: String| Ok(Err(Value::err(Value::str(format!("{}: save: {}", fname, why)))));
    let Some(full) = crate::builtins::local_path(it, &p) else {
        return said(format!("path must stay inside the program's folder: {}{}", p, crate::builtins::way_out(&p, "write")));
    };
    // The paths its file was confirmed for (FEEDBACK83.md, Q-1300; branch feedback82-85).
    if let Err(why) = crate::confirm::path_ok(it.cur_file.as_deref(), &it.base_dir, &p, true) {
        return said(why);
    }
    if let Some(why) = crate::builtins::own_file(it, &full, false) {
        return said(format!("will not write {}: {}", p, why));
    }
    if full.is_dir() {
        return said(format!("{} is a folder", p));
    }
    Ok(Ok(Some(SaveTo { full, shown: p, most: most.unwrap_or(MAX_BODY) })))
}

/// A call whose answer is saved: an answer of 2xx written to its file, through a file beside it
/// that takes its place when the whole has arrived (the bytes it wrote), so the file is never half
/// one answer; any other answer read as its text, as without `save`, and no file written
/// (FEEDBACK85.md, Q-1342; branch feedback82-85).
fn send_saved(o: Opts, method: &str, url: &str, headers: &[(String, String)], body: Option<(String, Vec<u8>)>, most: u64, to: &SaveTo) -> Result<(Reply, Option<u64>), Unsent> {
    let mut passed = Vec::new();
    let (url, resp) = follow(o, false, method, url, headers, body, &mut passed)?;
    let (status, headers, last, reader) = head_of(o, &url, method, resp)?;
    passed.extend(last);
    if !(200..300).contains(&status) {
        let body = read_text(o, &url, reader, most)?;
        return Ok((Reply { status, headers, cookies: passed, body }, None));
    }
    let n = write_answer(o, &url, reader, to).map_err(Unsent::Said)?;
    Ok((Reply { status, headers, cookies: passed, body: String::new() }, Some(n)))
}

/// The files a save of this name left beside it when its process ended as it wrote (a crash, a
/// SIGKILL), each up to its `max_bytes`: `stem` and 16 hex digits and `.part`, older than the
/// longest a call may take, removed (found in review of FEEDBACK85.md, Q-1342; branch
/// feedback82-85).
fn stale_parts(dir: &std::path::Path, stem: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let ceiling = Duration::from_secs(MAX_TIMEOUT_SECS as u64 * 2);
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(stem)).and_then(|r| r.strip_suffix(".part")) else { continue };
        if rest.len() != 16 || !rest.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let old = e.metadata().ok().filter(|m| m.is_file()).and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|age| age > ceiling);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// The answer's bytes written to `to.full`, `to.most` at most, 64 KiB held at a time: into a new
/// file beside it (its name begun with a dot, which `files` does not serve), made by this call
/// alone, then moved into its place. One that fails, or goes past the most, is removed, and a
/// file that was there stays as it was.
fn write_answer(o: Opts, url: &str, mut reader: Body, to: &SaveTo) -> Result<u64, String> {
    use std::io::Write;
    let said = |e: std::io::Error| format!("save: {}: {}", to.shown, e);
    let dir = to.full.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    std::fs::create_dir_all(&dir).map_err(said)?;
    let name = to.full.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // Short enough for a name, whatever the file's is.
    let mut cut = name.len().min(180);
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    let stem = format!(".{}.", &name[..cut]);
    stale_parts(&dir, &stem);
    let (part, mut f) = loop {
        let part = dir.join(format!("{}{:016x}.part", stem, fastrand::u64(..)));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&part) {
            Ok(f) => break (part, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(said(e)),
        }
    };
    // The file it replaces keeps its mode: a 0600 file was made 0644, and one run lost its `x`
    // (found in review).
    // Only its read, write and run bits: never setuid, setgid or sticky, which would make the
    // downloaded bytes run as the file's owner where the process may keep them (as root in a
    // container) (FEEDBACK85.md, Q-1342, found in the second review; branch feedback82-85).
    if let Ok(m) = std::fs::metadata(&to.full) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = f.set_permissions(std::fs::Permissions::from_mode(m.permissions().mode() & 0o777));
        }
        #[cfg(not(unix))]
        {
            let _ = f.set_permissions(m.permissions());
        }
    }
    let written = (|| -> Result<u64, String> {
        let mut buf = vec![0u8; 64 << 10];
        let mut total = 0u64;
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(timed_out(o, url, e.to_string())),
            };
            if total + n as u64 > to.most {
                return Err(format!("{}: the answer is longer than {} bytes, the most `save` writes (say more with max_bytes)", url, to.most));
            }
            f.write_all(&buf[..n]).map_err(said)?;
            total += n as u64;
        }
        f.sync_all().map_err(said)?;
        Ok(total)
    })();
    drop(f);
    let done = written.and_then(|n| std::fs::rename(&part, &to.full).map(|()| n).map_err(said));
    if done.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    done
}

/// A `site`'s call (src/sites.rs), `POST url` with `body` as JSON, made as every call here is:
/// the host's policy judges each address, the client follows no redirect itself and a `file:`
/// one is no crash, through the environment's proxy, saying `User-Agent: quillo/<version>`, and
/// an answer over 64 MiB is an error. It has 10 s to connect and 30 in all, redirects too, and
/// follows up to 5 redirects on the site's own origin; one elsewhere is the answer. Its status,
/// headers, body, and the `Set-Cookie`s of each answer on the way and of the last, all from the
/// site's origin. A site's call built a client of its own, which none of that held for
/// (FEEDBACK71.md, Q-973; branch security). Err(None): refused at `--max-memory`, told by its
/// kind, which the caller makes the request's stop.
pub(crate) fn site_post(url: &str, headers: &[(String, String)], body: Vec<u8>) -> Result<(u16, Vec<(String, String)>, Vec<String>, String), Option<String>> {
    let o = Opts { timeout_ms: 30_000, same_origin: true, ..DEFAULT_OPTS };
    // Read no further than one value may be, and its text held to it, as `http`'s calls are
    // (`body_room`): given `MAX_BODY`, an answer of 600 KB not UTF-8 was made a text of 1.8 MB
    // under a 1 MB limit, and one of 64 MiB was read whole; and the stop told by its kind, not
    // turned into an error value with its words, which `??` took (coverage of 0.8.7, Q-1105;
    // branch coverage-087).
    let r = send(o, "POST", url, headers, Some(("application/json".into(), body)), body_room()).map_err(|e| match e {
        Unsent::Said(s) => Some(s),
        Unsent::OverMemory => None,
    })?;
    Ok((r.status, r.headers, r.cookies, r.body))
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

/// The same call: its method, URL, headers, body and every option, each part apart, hashed. The
/// certificates left out, one client certificate's answer was given to calls with another, or
/// none, which could not even connect; a GET's body left out, one query's answer was given to
/// another; and parts joined by spaces and line breaks, a URL with a line break in it (which
/// the client drops) was the same call as one with headers that held the rest (FEEDBACK71.md,
/// Q-974; branch security). Each option is named, so that one added later cannot be left out.
fn cache_key(o: Opts, method: &str, url: &str, headers: &[(String, String)], body: Option<&(String, Vec<u8>)>) -> String {
    let Opts { redirects, public_only, cache, timeout_ms, tls, same_origin, held } = o;
    let opts = format!("{} {} {} {} {} {} {}", redirects, public_only, cache, timeout_ms, tls, same_origin, held);
    let mut hs: Vec<(String, &str)> = headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.as_str())).collect();
    hs.sort();
    let (ctype, bytes) = body.map_or(("", &[][..]), |(t, b)| (t.as_str(), b.as_slice()));
    let mut parts: Vec<&[u8]> = vec![method.as_bytes(), url.as_bytes(), opts.as_bytes(), if body.is_some() { b"body" } else { b"none" }, ctype.as_bytes(), bytes];
    for (k, v) in &hs {
        parts.push(k.as_bytes());
        parts.push(v.as_bytes());
    }
    crate::idempotency::sha256_hex(&parts)
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
    let mut saving = false;
    let mut o = opts_of(opts, fname, line, &it.base_dir.clone(), &mut saving)?;
    o.held = crate::confirm::held_id(it.cur_file.as_deref());
    if o.cache > 0 && method != "GET" {
        return fail(line, format!("`{}`: `cache` keeps the answers of GET requests, not {}", fname, method));
    }
    let (Value::Str(url) | Value::Css(url)) = url else {
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
        // Trusted CSS is the text it is (FEEDBACK79.md, Q-1250; branch feedback72-81).
        Value::Str(s) | Value::Css(s) => Some(("text/plain; charset=utf-8".into(), s.as_bytes().to_vec())),
        Value::Html(h) => Some(("text/html; charset=utf-8".into(), h.as_bytes().to_vec())),
        other => match json_body(other, line)? {
            Ok(j) => Some(("application/json".into(), j)),
            Err(e) => return Ok(e),
        },
    };
    // Written to a file (FEEDBACK85.md, Q-1342; branch feedback82-85): looked at only when the
    // options name it.
    if saving {
        let to = match save_to(it, fname, opts, o, line)? {
            Ok(to) => to,
            Err(e) => return Ok(e),
        };
        if let Some(to) = to {
            let (method, url) = (method.to_string(), url.to_string());
            let most = body_room();
            let res = it.unlocked(move || send_saved(o, &method, &url, &hdrs, payload, most, &to).map(|r| (r, to.shown)));
            let ((reply, bytes), shown) = match res {
                Ok(r) => r,
                Err(Unsent::OverMemory) => return Err(crate::httpd::OverMemory.at(line)),
                Err(Unsent::Said(e)) => return Ok(Value::err(Value::str(format!("http: {}", e)))),
            };
            let mut m = Map::new();
            m.insert("status".into(), Value::Int(reply.status as i64));
            m.insert("ok".into(), Value::Bool((200..300).contains(&reply.status)));
            m.insert("headers".into(), Value::map(reply.headers.into_iter().map(|(k, v)| (k.into(), Value::str(v))).collect()));
            m.insert("file".into(), if bytes.is_some() { Value::str(shown) } else { Value::Nil });
            m.insert("bytes".into(), Value::Int(bytes.unwrap_or(0) as i64));
            // An answer not saved says why in its own words.
            if bytes.is_none() {
                m.insert("body".into(), Value::str(reply.body));
            }
            return Ok(Value::map(m));
        }
    }
    let key = (o.cache > 0).then(|| cache_key(o, method, url, &hdrs, payload.as_ref()));
    let reply = match key.as_deref().and_then(cached) {
        Some(r) => r,
        None => {
            let (method, url) = (method.to_string(), url.to_string());
            let most = body_room();
            let res = it.unlocked(move || send(o, &method, &url, &hdrs, payload, most));
            let reply = match res {
                Ok(r) => r,
                // Past `--max-memory`: the request's stop, not an error value (FEEDBACK71.md, Q-977).
                Err(Unsent::OverMemory) => return Err(crate::httpd::OverMemory.at(line)),
                Err(Unsent::Said(e)) => return Ok(Value::err(Value::str(format!("http: {}", e)))),
            };
            if let Some(key) = key {
                if (200..300).contains(&reply.status) {
                    keep(key, o.cache, &reply);
                }
            }
            reply
        }
    };
    // Read straight into values, with a budget of them: an answer of 8 MB of `0,` took 633 MB
    // (FEEDBACK51.md, Q-660; branch feedback51-fixes). Each list, map and text held to
    // `--max-memory` as it is made (FEEDBACK71.md, Q-977; branch security).
    let json = Value::parse_json_held(&reply.body).map_err(|s| s.at(line))?.unwrap_or(Value::Nil);
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

/// A request's body as JSON, written as `json.stringify` writes it: a function in it, at any
/// depth, is an error value that says where it is, and nothing is sent, where it went out as its
/// text (`["<builtin len>",1]`); held to `--max-memory` as it is written, and past it the
/// request's stop, as `json.stringify`'s is (coverage of 0.8.7, Q-1107; branch coverage-087).
fn json_body(v: &Value, line: u32) -> Result<Result<Vec<u8>, Value>, Ctrl> {
    let mut out = Vec::new();
    match v.json_into_held(&mut out, crate::builtins::JSON_MOST, crate::builtins::JSON_DEEPEST) {
        Ok(()) => Ok(Ok(out)),
        Err(None) => Err(crate::httpd::OverMemory.at(line)),
        Err(Some(why)) => Ok(Err(Value::err(Value::str(format!("http: the body: {}", why))))),
    }
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
    // Judged as written, and said so: upper case changes no character's kind.
    let method = match a(&args, 0) {
        Value::Str(m) | Value::Css(m) => match not_a_method(&m) {
            Some(e) => return Ok(e),
            None => m.to_ascii_uppercase(),
        },
        other => return fail(line, format!("`http.request` expects a method string first, got {}", other.type_name())),
    };
    do_request(it, "http.request", &method, &a(&args, 1), &a(&args, 2), &a(&args, 3), args.get(4), line)
}

/// The error value for a method that is not an HTTP token (RFC 9110 §9.1 and §5.6.2: letters,
/// digits and ``!#$%&'*+-.^_`|~``, at least one), before anything connects. The client writes
/// the method into the request line as it is given, so one with a line break in it wrote
/// headers, and a whole second request, of whoever chose it (FEEDBACK71.md, Q-972; branch
/// security).
fn not_a_method(m: &str) -> Option<Value> {
    let token = !m.is_empty() && m.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
    if token {
        return None;
    }
    let shown: String = m.chars().take(40).collect();
    let more = if m.chars().count() > 40 { "..." } else { "" };
    Some(Value::err(Value::str(format!("http: not an HTTP method: {:?}{} (a method is letters, digits and !#$%&'*+-.^_`|~)", shown, more))))
}

type Body = Box<dyn Read + Send + Sync>;

fn open(o: Opts, method: &str, url: &str, headers: &[(String, String)], body: Option<(String, Vec<u8>)>) -> Result<Head, String> {
    let (url, resp) = follow(o, true, method, url, headers, body, &mut Vec::new())?;
    head_of(o, &url, method, resp)
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Events,
    Lines,
    Text,
}

fn parsed(text: &str) -> Value {
    Value::parse_json(text).unwrap_or_else(|_| Value::str(text))
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
    let (Value::Str(url) | Value::Css(url)) = a(&args, 0) else {
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
    let mut o = opts_of(Some(&Value::Map(opts.clone())), "http.stream", line, &it.base_dir.clone(), &mut false)?;
    o.held = crate::confirm::held_id(it.cur_file.as_deref());
    if o.cache > 0 {
        return fail(line, "`http.stream`: `cache` keeps whole answers, of `http.get`");
    }
    let body = opts.get("body").cloned().unwrap_or(Value::Nil);
    let method = match opts.get("method") {
        Some(Value::Str(m) | Value::Css(m)) => match not_a_method(m) {
            Some(e) => return Ok(e),
            None => m.to_ascii_uppercase(),
        },
        _ if !matches!(body, Value::Nil) => "POST".into(),
        _ => "GET".into(),
    };
    let hdrs: Vec<(String, String)> = match opts.get("headers") {
        Some(Value::Map(m)) => m.iter().map(|(k, v)| (k.to_string(), v.display())).collect(),
        _ => Vec::new(),
    };
    let payload: Option<(String, Vec<u8>)> = match &body {
        Value::Nil => None,
        Value::Str(s) | Value::Css(s) => Some(("text/plain; charset=utf-8".into(), s.as_bytes().to_vec())),
        other => match json_body(other, line)? {
            Ok(j) => Some(("application/json".into(), j)),
            Err(e) => return Ok(e),
        },
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
        // No further than one value may be, and made text held to it (FEEDBACK71.md, Q-977;
        // branch security).
        let most = body_room();
        // Into a buffer that never passes the limit, held with the others being read
        // (FEEDBACK75.md, Q-1072; branch feedback75).
        let read = it.unlocked(move || read_whole(reader, most));
        // Its error said, not dropped, and one too long refused (Q-599).
        let bytes = match read {
            Ok(b) => b,
            Err(Unread::Longer) if most < MAX_BODY => return Err(crate::httpd::OverMemory.at(line)),
            Err(Unread::Longer) => return Ok(Value::err(Value::str(format!("http: {}", too_long(&url))))),
            Err(Unread::Io(e)) => return Ok(Value::err(Value::str(format!("http: {}: {}", url, e)))),
            Err(Unread::Busy) => return Ok(Value::err(Value::str(format!("http: {}", too_busy(&url))))),
        };
        m.insert("body".into(), Value::str(crate::value::lossy_held(bytes).map_err(|o| o.at(line))?));
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

    // RFC 8215's local-use NAT64 prefix leads to the network's own addresses: not public, as the
    // well-known 64:ff9b::/96 is public only where its IPv4 address is (FEEDBACK67.md, B.3,
    // review; branch pwa).
    #[test]
    fn local_use_nat64_is_not_public() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        assert!(!public_ip(ip("64:ff9b:1::a00:1")) && !public_ip(ip("64:ff9b:1:ffff::808:808")));
        assert!(public_ip(ip("64:ff9b::808:808")) && !public_ip(ip("64:ff9b::a00:1")));
        // In 0::/8, which IANA keeps: none of it is anyone's address on the internet (changed with
        // FEEDBACK75.md, Q-1093; branch feedback75).
        assert!(!public_ip(ip("64:ff9c::1")) && !public_ip(ip("64:ff9b:2::1")));
    }

    // Every option is in a cached answer's key: another certificate, none, another CA, another
    // timeout or another `cache` is another call (FEEDBACK71.md, Q-974; branch security).
    #[test]
    fn a_cached_answer_is_kept_apart_by_every_option() {
        let tls = |ca: Option<&str>, cert: Option<&str>, key: Option<&str>| tls_number(ca.map(String::from), cert.map(String::from), key.map(String::from));
        let alice = Opts { tls: tls(Some("ca.pem"), Some("alice.pem"), Some("alice.key")), ..DEFAULT_OPTS };
        let bob = Opts { tls: tls(Some("ca.pem"), Some("bob.pem"), Some("bob.key")), ..DEFAULT_OPTS };
        let ca_only = Opts { tls: tls(Some("ca.pem"), None, None), ..DEFAULT_OPTS };
        let system = Opts { tls: tls(Some("system"), None, None), ..DEFAULT_OPTS };
        let all = [
            alice,
            bob,
            ca_only,
            system,
            DEFAULT_OPTS,
            Opts { timeout_ms: 5000, ..DEFAULT_OPTS },
            Opts { timeout_ms: 5001, ..DEFAULT_OPTS },
            Opts { cache: 10, ..DEFAULT_OPTS },
            Opts { cache: 60, ..DEFAULT_OPTS },
            Opts { redirects: 0, ..DEFAULT_OPTS },
            Opts { public_only: true, ..DEFAULT_OPTS },
            Opts { same_origin: true, ..DEFAULT_OPTS },
        ];
        let url = "https://127.0.0.1:4863/balance";
        let keys: std::collections::HashSet<String> = all.iter().map(|o| cache_key(*o, "GET", url, &[], None)).collect();
        assert_eq!(keys.len(), all.len(), "each option apart");
        // The same settings again are the same call: the same entry of `TLS`, and the same key.
        let again = Opts { tls: tls(Some("ca.pem"), Some("alice.pem"), Some("alice.key")), ..DEFAULT_OPTS };
        assert_eq!(again.tls, alice.tls);
        assert_eq!(cache_key(again, "GET", url, &[], None), cache_key(alice, "GET", url, &[], None));
        assert_eq!(tls(None, None, None), 0, "no TLS settings: the usual CAs");
        // Headers in any order and any case of their names are the same call; their values not.
        let h = |v: &[(&str, &str)]| v.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>();
        assert_eq!(cache_key(alice, "GET", url, &h(&[("A", "1"), ("b", "2")]), None), cache_key(alice, "GET", url, &h(&[("b", "2"), ("a", "1")]), None));
        assert_ne!(cache_key(alice, "GET", url, &h(&[("a", "1")]), None), cache_key(alice, "GET", url, &h(&[("a", "2")]), None));
        // A GET's body, and its type, are the call too; none is not an empty one.
        let body = |t: &str, b: &str| Some((t.to_string(), b.as_bytes().to_vec()));
        let with = |b: &Option<(String, Vec<u8>)>| cache_key(alice, "GET", url, &[], b.as_ref());
        let bodies = [None, body("text/plain; charset=utf-8", ""), body("text/plain; charset=utf-8", "query-for-alice"), body("text/plain; charset=utf-8", "query-for-bob"), body("application/json", "query-for-alice")];
        let keys: std::collections::HashSet<String> = bodies.iter().map(with).collect();
        assert_eq!(keys.len(), bodies.len());
        assert_eq!(with(&body("application/json", "1")), with(&body("application/json", "1")));
        // Parts kept apart: what one part ends with is never the next one's start.
        let u1 = format!("{} 5 false 60 0 0 false\nk:v", url);
        assert_ne!(cache_key(DEFAULT_OPTS, "GET", &u1, &h(&[("z", "1")]), None), cache_key(DEFAULT_OPTS, "GET", url, &h(&[("k", "v 5 false 60 0 0 false"), ("z", "1")]), None));
        assert_ne!(cache_key(DEFAULT_OPTS, "GET", url, &h(&[("a:b", "c")]), None), cache_key(DEFAULT_OPTS, "GET", url, &h(&[("a", "b:c")]), None));
        assert_ne!(cache_key(DEFAULT_OPTS, "GET", url, &h(&[("a", "1\nb:2")]), None), cache_key(DEFAULT_OPTS, "GET", url, &h(&[("a", "1"), ("b", "2")]), None));
        assert_eq!(cache_key(DEFAULT_OPTS, "GET", url, &[], None).len(), 64, "a hash: the key's size does not grow with the call");
    }

    // A method is an HTTP token, or an error value before anything connects (FEEDBACK71.md,
    // Q-972; branch security).
    #[test]
    fn a_method_is_an_http_token() {
        let said = |v: Option<Value>| match v {
            Some(Value::Err(e, ..)) => e.display(),
            other => format!("not an error: {:?}", other.map(|v| v.repr())),
        };
        for m in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE", "CONNECT", "PROPFIND", "M-SEARCH", "X", "9", "!#$%&'*+-.^_`|~", &"A".repeat(10_000)] {
            assert!(not_a_method(m).is_none(), "{}", m);
        }
        for m in ["", " ", "GET ", " GET", "GE T", "GET\r\nX-Injected: yes", "GET\n", "GET\r", "G\tET", "GÉT", "ПОСТ", "GET\0", "(GET)", "GET/", "GET:", "\"GET\"", "{GET}", "G@T", "G,T", "G;T", "G=T", "G?T", "G[T]", "G\\T", "\u{7f}"] {
            let e = said(not_a_method(m));
            assert!(e.starts_with("http: not an HTTP method: "), "{:?}: {}", m, e);
        }
        // Shown escaped, and cut short.
        let e = said(not_a_method("GET /admin HTTP/1.1\r\nHost: internal\r\nX-Injected: yes"));
        assert!(e.contains(r#""GET /admin HTTP/1.1\r\nHost: internal\r\nX-I"..."#), "{}", e);
    }

    // As the client reads a URL (found in review of FEEDBACK51.md, Q-650).
    #[test]
    fn a_host_is_the_clients() {
        assert_eq!(host_of("http://169.254.169.254\\latest\\x@example.com/"), "169.254.169.254");
        assert_eq!(host_of("http://[::1]:8080/a"), "[::1]");
        assert_eq!(host_of("https://Example.COM./"), "example.com.");
        assert_eq!(host_of("not a url"), "");
    }

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
