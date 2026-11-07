//! Cookies and sessions.
//!
//! ```text
//! set_cookie("theme", "dark", { max_age: "1y" })   // HttpOnly, SameSite=Lax, Path=/
//! delete_cookie("theme")
//! set_session({ user: user.id })                   // signed; 30 days unless { max_age }
//! session().user                                   // {} when there is none
//! end_session()
//! ```
//!
//! A session is a map kept in a cookie, signed (HMAC-SHA256) with the
//! `QUILLO_SECRET` environment variable, so the browser can read it but not
//! change it. Keep secrets out of it, and keep it small (cookies hold about
//! 4 KB): an id, with the rest in the database. The signature covers the
//! expiry, the tenant and the mount point, so a session from one tenant or
//! program is no session in another, and an old cookie is no session after
//! its time.
//!
//! Cookies are `Secure`, except in development (`--watch`) and in a desktop
//! app's window, which are served over plain HTTP on this machine. (The
//! request's host cannot tell: behind a proxy, every request is to
//! 127.0.0.1.) A `Secure` session cookie for the whole site is named
//! `__Host-quillo_session`, which browsers only take from this very host,
//! over HTTPS: a sibling subdomain (another tenant's) cannot plant one. Two
//! session cookies that are both valid here are no session.

use crate::crypto::{b64url, hmac_sha256, unb64url};
use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::sync::{Arc, OnceLock};

type R = Result<Value, Ctrl>;

pub const COOKIE: &str = "quillo_session";
/// The session cookie's name when it is `Secure` with `Path=/`.
pub const HOST_COOKIE: &str = "__Host-quillo_session";

/// Served over plain HTTP on purpose (`--watch`, desktop apps): cookies
/// are not `Secure`.
pub static PLAIN_HTTP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// `--plain-http`: the site is served over plain HTTP on purpose, on a
/// network it trusts (a factory's kiosks at http://10.0.0.5:8080), so its
/// cookies are not `Secure`, which browsers would drop there (FEEDBACK13.md,
/// Q-089; branch feedback13-fixes).
pub static PLAIN_HTTP_ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether the warning that browsers drop a `Secure` cookie sent over plain
/// HTTP has been given (once a process).
static WARNED_PLAIN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Browsers keep cookies up to about 4096 bytes, name and attributes included.
const MAX_COOKIE: usize = 3900;
const DEFAULT_AGE: u64 = 30 * 86400;

pub const BUILTINS: &[(&str, BuiltinFn)] = &[
    ("set_cookie", set_cookie),
    ("delete_cookie", delete_cookie),
    ("session", session),
    ("cookie", cookie),
    ("set_session", set_session),
    ("end_session", end_session),
];

/// The signing key, from `QUILLO_SECRET`; without it a random one, so
/// sessions end when the server restarts (and are not shared by servers).
fn key() -> &'static [u8; 32] {
    static KEY: OnceLock<[u8; 32]> = OnceLock::new();
    KEY.get_or_init(|| {
        let secret = match std::env::var("QUILLO_SECRET") {
            Ok(s) if !s.is_empty() => s.into_bytes(),
            _ => {
                eprintln!("quillo: QUILLO_SECRET is not set; sessions use a random key and end when the server stops");
                crate::crypto::secret_bytes(32)
            }
        };
        hmac_sha256(&secret, b"quillo session")
    })
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// What a signature binds a session to besides its contents.
fn context(it: &mut Interp) -> String {
    let tenant = it.tenant();
    format!("{}\0{}", it.base, tenant.as_deref().unwrap_or(""))
}

fn sign(ctx: &str, payload: &str) -> String {
    let mut msg = Vec::with_capacity(ctx.len() + payload.len() + 1);
    msg.extend_from_slice(ctx.as_bytes());
    msg.push(0);
    msg.extend_from_slice(payload.as_bytes());
    b64url(&hmac_sha256(key(), &msg))
}

/// The session in a cookie value, and its hidden id, if it is signed for here and current.
fn open(ctx: &str, cookie: &str) -> Option<(Value, Option<String>)> {
    let (payload, sig) = cookie.split_once('.')?;
    if !crate::crypto::same(sign(ctx, payload).as_bytes(), sig.as_bytes()) {
        return None;
    }
    // At most as deep as every reader takes (FEEDBACK55.md, Q-875's second review; branch feedback55).
    let j: serde_json::Value = crate::value::json_value(&unb64url(payload)?).ok()?;
    if j.get("e")?.as_u64()? <= now() {
        return None;
    }
    // Decimals and ints past 2^53 come back as they went, as an island's do, not as text
    // that `+` would join (FEEDBACK30.md, Q-374; branch feedback27-35-fixes).
    Some((crate::interp::from_island_json(j.get("d")?), j.get("i").and_then(|i| i.as_str()).map(str::to_string)))
}

/// The session the request came with, and its hidden id. One valid among others is the session;
/// two are none, since one of them was planted (by a sibling subdomain, say).
fn arrived(it: &mut Interp) -> Option<(Value, Option<String>)> {
    let ctx = context(it);
    let mut valid: Vec<(Value, Option<String>)> = cookies(it).iter().filter_map(|v| open(&ctx, v)).collect();
    if valid.len() == 1 {
        valid.pop()
    } else {
        None
    }
}

/// What a session says of who the visitor is, for an `Idempotency-Key`'s owners: its hidden id,
/// which stays from the first `set_session` to `end_session`, whatever else it comes to hold. One
/// with no id (from before there were ids, or too full for one) says `user`, when that is a name,
/// and else the session itself: a false refusal is fine where `nobody` would be anyone.
fn owner_in(id: Option<&str>, data: &Value) -> Option<String> {
    if let Some(id) = id {
        return Some(format!("id {}", id));
    }
    let Value::Map(m) = data else { return None };
    match m.get("user") {
        _ if m.is_empty() => None,
        Some(u @ (Value::Str(_) | Value::Int(_))) => Some(format!("user {}", u.display())),
        _ => Some(format!("session {}", crate::interp::island_json(data))),
    }
}

/// That, of the session the request came with.
pub(crate) fn arrived_owner(it: &mut Interp) -> Option<String> {
    arrived(it).and_then(|(data, id)| owner_in(id.as_deref(), &data))
}

/// That, of the session as it is now, which the answer's cookie will carry.
pub(crate) fn current_owner(it: &mut Interp) -> Option<String> {
    match (&it.session_id, &it.session) {
        (None, _) => arrived_owner(it),
        (Some(id), Some(data)) => owner_in(id.as_deref(), data),
        (Some(id), None) => id.as_deref().map(|id| format!("id {}", id)),
    }
}

/// `session().user` of the session the request came with, for rows from before owners.
pub(crate) fn old_user(it: &mut Interp) -> Option<Value> {
    match arrived(it) {
        Some((Value::Map(m), _)) => m.get("user").filter(|u| !matches!(u, Value::Nil)).cloned(),
        _ => None,
    }
}

/// The id `set_session` writes: the session's own, kept, or a new one for a session new in this
/// request. `session()` never shows it. It is not made anew at sign-in, from no `user` to one, as
/// a session's id usually should be: this one is no credential, the cookie's signature is, and
/// the owners kept at the end of the work already cover a work that signs in. From one `user` to
/// another it is (see `set_session`): Bob on Alice's browser is not Alice (FEEDBACK55.md, Q-887;
/// branch feedback55).
fn session_id(it: &mut Interp) -> Arc<str> {
    if let Some(Some(id)) = &it.session_id {
        return id.clone();
    }
    let id = if it.session_id.is_none() { arrived(it).and_then(|(_, id)| id) } else { None };
    let id: Arc<str> = id.unwrap_or_else(|| crate::service::new_id(32)).into();
    it.session_id = Some(Some(id.clone()));
    id
}

/// `payload`, signed with the session key for `purpose`: something the
/// server gave out and takes back, with no one able to change it between
/// (a page's ticket to join channels; see `wt`). Servers sharing
/// `QUILLO_SECRET` take each other's.
pub fn seal(purpose: &str, payload: &str) -> String {
    let body = b64url(payload.as_bytes());
    format!("{}.{}", body, sign(purpose, &body))
}

/// What `seal` signed, if `token` is unchanged.
pub fn unseal(purpose: &str, token: &str) -> Option<String> {
    let (body, sig) = token.split_once('.')?;
    if !crate::crypto::same(sign(purpose, body).as_bytes(), sig.as_bytes()) {
        return None;
    }
    String::from_utf8(unb64url(body)?).ok()
}

/// The request's session, as `session()` gives it.
pub fn current(it: &mut Interp) -> Value {
    session(it, Vec::new(), 0).unwrap_or_else(|_| Value::map(Map::new()))
}

fn header(it: &mut Interp, name: &str) -> Option<String> {
    let Some(Value::Map(req)) = it.request_value() else { return None };
    let Some(Value::Map(h)) = req.get("headers") else { return None };
    h.get(name).map(|v| v.display())
}

fn token_ok(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Cookie values are percent-encoded where needed (and decoded when read).
fn encode(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        match b {
            b'!' | b'#'..=b'+' | b'-'..=b':' | b'<'..=b'[' | b']'..=b'~' if b != b'%' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// The options `set_cookie` takes; `delete_cookie` takes them but `max_age`, a deletion's being 0
/// (FEEDBACK55.md, Q-712; branch feedback55).
const OPTIONS: &[&str] = &["max_age", "path", "domain", "http_only", "secure", "same_site"];

/// What is said of an option `f` does not take: the name to write for Express's and the
/// browser's spellings (`sameSite`, `httpOnly`, `Max-Age`), else the options it takes, where it
/// was let go, and `sameSite: "Strict"` left the cookie `Lax` (FEEDBACK55.md, Q-712; branch
/// feedback55).
fn unknown_option(f: &str, k: &str) -> String {
    let bare = |s: &str| s.chars().filter(|c| *c != '_' && *c != '-').collect::<String>().to_ascii_lowercase();
    if let Some(known) = OPTIONS.iter().find(|o| bare(o) == bare(k)) {
        if *known == "max_age" && f == "delete_cookie" {
            return format!("`delete_cookie` has no option `{}` but 0: a deleted cookie's age is 0", k);
        }
        return format!("`{}` has no option `{}`: write it `{}`", f, k, known);
    }
    let taken = if f == "delete_cookie" { "`path`, `domain`, `http_only`, `secure` and `same_site`" } else { "`max_age`, `path`, `domain`, `http_only`, `secure` and `same_site`" };
    let hint = if k.eq_ignore_ascii_case("expires") && f != "delete_cookie" { " (`max_age` is how long it lasts, as seconds or a duration like \"30d\")" } else { "" };
    format!("`{}` has no option `{}`: it takes {}{}", f, k, taken, hint)
}

/// What `f` (`set_cookie`, `delete_cookie`) says of option `k`, if it does not take it: when it
/// runs, and in `quillo check` for one written out (FEEDBACK55.md, Q-712; branch feedback55).
/// `zero`: its value is 0 or null, which a deletion's `max_age` may be, as it is that anyway.
pub(crate) fn option_refused(f: &str, k: &str, zero: bool) -> Option<String> {
    (!OPTIONS.contains(&k) || (f == "delete_cookie" && k == "max_age" && !zero)).then(|| unknown_option(f, k))
}

/// The prefix a cookie's name has, which browsers hold it to (any case, as they read it): None,
/// `__Secure-` or `__Host-` (FEEDBACK55.md, Q-713; branch feedback55).
fn prefix(name: &str) -> Option<&'static str> {
    ["__Host-", "__Secure-"].into_iter().find(|p| name.get(..p.len()).is_some_and(|n| n.eq_ignore_ascii_case(p)))
}

/// The `Set-Cookie` line, with the defaults and whatever `opts` changes.
fn cookie_line(it: &mut Interp, name: &str, value: &str, age: Option<u64>, opts: Option<&Map>, f: &str, line: u32) -> Result<String, Ctrl> {
    if let Some(why) = opts.and_then(|m| m.iter().find_map(|(k, v)| option_refused(f, k, matches!(v, Value::Int(0) | Value::Nil)))) {
        return fail(line, why);
    }
    let opt = |k: &str| opts.and_then(|m| m.get(k)).filter(|v| !matches!(v, Value::Nil)).cloned();
    // A prefixed cookie that breaks its prefix's rules is dropped by the browser, and was sent so:
    // a `__Host-` one with a path but `/`, or a domain, and either without `Secure` (FEEDBACK55.md,
    // Q-713; branch feedback55).
    let prefixed = prefix(name);
    if let Some(p) = prefixed {
        if opt("secure").is_some_and(|v| !v.truthy()) {
            return fail(line, format!("`{}`: a `{}` cookie is `Secure`, as browsers take it no other way; leave `secure` out", f, p));
        }
        if p == "__Host-" && opt("domain").is_some() {
            return fail(line, format!("`{}`: a `__Host-` cookie has no domain, as browsers take it only from the host that sets it; leave `domain` out", f));
        }
        if p == "__Host-" && opt("path").is_some_and(|v| !matches!(v, Value::Str(p) if &*p == "/")) {
            return fail(line, format!("`{}`: a `__Host-` cookie's path is /, as browsers take it no other way, got {}", f, opt("path").map_or(String::new(), |v| v.repr())));
        }
    }
    // What is wrong with a path, said: "starts with /" was said of `/a b` (FEEDBACK55.md, Q-726;
    // branch feedback55).
    let path = match opt("path") {
        Some(Value::Str(p)) if !p.starts_with('/') => return fail(line, format!("`{}`'s path starts with /, got {}", f, Value::Str(p).repr())),
        // Visible characters only: a line break would start another header.
        Some(Value::Str(p)) if p.bytes().all(|b| (b'!'..=b'~').contains(&b) && b != b';') => p.to_string(),
        Some(v @ Value::Str(_)) => {
            return fail(line, format!("`{}`'s path cannot hold a space, a `;`, a control character or a letter past ASCII (percent-encode them), got {}", f, v.repr()))
        }
        Some(v) => return fail(line, format!("`{}`'s path is a str that starts with /, got {}", f, v.type_name())),
        None if it.base.is_empty() || prefixed == Some("__Host-") => "/".into(),
        None => it.base.clone(),
    };
    let pair = format!("{}={}", name, encode(value));
    // As `set_session`'s: a browser drops a cookie larger than about 4 KB with its attributes, and
    // nothing said so (FEEDBACK55.md, Q-726; branch feedback55).
    if f == "set_cookie" && pair.len() > MAX_COOKIE {
        return fail(
            line,
            format!("`{}`: the cookie `{}` is {} bytes as sent (its value percent-encoded), more than a browser keeps with its attributes ({}); keep an id in it and the rest in the database", f, name, pair.len(), MAX_COOKIE),
        );
    }
    let mut s = format!("{}; Path={}", pair, path);
    if let Some(d) = opt("domain") {
        match d {
            Value::Str(d) if !d.is_empty() && d.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') => {
                s.push_str("; Domain=");
                s.push_str(&d);
            }
            v => return fail(line, format!("`{}`'s domain is a host name, got {}", f, v.repr())),
        }
    }
    let age = match opt("max_age") {
        Some(v) => match crate::builtins::duration_seconds(&v) {
            Some(n) => Some(n),
            None => return fail(line, format!("`{}`'s max_age is seconds or a duration like \"30d\", got {}", f, v.repr())),
        },
        None => age,
    };
    if let Some(a) = age {
        s.push_str(&format!("; Max-Age={}", a));
    }
    if opt("http_only").map_or(true, |v| v.truthy()) {
        s.push_str("; HttpOnly");
    }
    let same_site = match opt("same_site") {
        None => "Lax".to_string(),
        Some(Value::Str(v)) => match v.to_ascii_lowercase().as_str() {
            "lax" => "Lax".into(),
            "strict" => "Strict".into(),
            "none" => "None".into(),
            _ => return fail(line, format!("`{}`'s same_site is \"Lax\", \"Strict\" or \"None\", got {:?}", f, &*v)),
        },
        Some(v) => return fail(line, format!("`{}`'s same_site is a str, got {}", f, v.type_name())),
    };
    // SameSite=None is only kept by browsers on Secure cookies.
    let plain = PLAIN_HTTP.load(std::sync::atomic::Ordering::Relaxed);
    // A deletion carries nothing to keep from a plain connection, and one marked `Secure` is
    // dropped by a browser there: a cookie set with `secure: false` could never be taken away.
    // It is `Secure` where the request came over HTTPS, or the name needs it (FEEDBACK34.md,
    // Q-425; branch feedback27-35-fixes).
    let default_secure = if f == "delete_cookie" { came_over_https(it) } else { !plain };
    // A prefixed name is `Secure` under `--watch` too, where it went without (FEEDBACK55.md,
    // Q-713; branch feedback55).
    let secure = prefixed.is_some() || same_site == "None" || opt("secure").map_or(default_secure, |v| v.truthy());
    if secure {
        s.push_str("; Secure");
        warn_if_dropped(it, name);
    }
    s.push_str("; SameSite=");
    s.push_str(&same_site);
    Ok(s)
}

/// A `Secure` cookie on a request that came straight from another machine
/// over plain HTTP (no proxy here, no `x-forwarded-proto: https`, a host that
/// is not this machine's) is dropped by the browser: signing in there never
/// sticks, and nothing else would say so. Said once (FEEDBACK13.md, Q-089;
/// branch feedback13-fixes).
/// Whether the request came over HTTPS, as far as this server can tell: a proxy says so.
fn came_over_https(it: &mut Interp) -> bool {
    header(it, "x-forwarded-proto").is_some_and(|p| p.eq_ignore_ascii_case("https"))
}

fn warn_if_dropped(it: &mut Interp, cookie: &str) {
    if WARNED_PLAIN.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Some(Value::Map(req)) = it.request_value() else { return };
    let ip = req.get("ip").map(|v| v.display()).unwrap_or_default();
    let host = header(it, "host").unwrap_or_default();
    let proto = header(it, "x-forwarded-proto").unwrap_or_default();
    let name = host.rsplit_once(':').filter(|(_, p)| p.bytes().all(|b| b.is_ascii_digit())).map_or(host.as_str(), |(h, _)| h);
    let local = |h: &str| h == "localhost" || h.ends_with(".localhost") || h.starts_with("127.") || h == "::1" || h == "[::1]";
    if ip.is_empty() || local(&ip) || local(name) || proto.eq_ignore_ascii_case("https") {
        return;
    }
    if !WARNED_PLAIN.swap(true, std::sync::atomic::Ordering::Relaxed) {
        eprintln!(
            "quillo: warning: the Secure cookie `{}` was sent to http://{} over plain HTTP, which browsers drop{}. \
             Serve the site over HTTPS (a proxy with TLS in front, sending x-forwarded-proto: https), or run with --plain-http on a network you trust",
            cookie,
            host,
            if cookie == COOKIE || cookie == HOST_COOKIE { ": sessions will not stick there" } else { "" }
        );
    }
}

/// Queues the cookie for the response, replacing one queued under its name, path and domain,
/// which a browser keeps it by: `/old`'s deletion and `/`'s cookie were one, and the first not
/// sent (FEEDBACK55.md, Q-714; branch feedback55).
fn queue(it: &mut Interp, line: String) {
    let key = cookie_key(&line);
    let key = (key.0.to_string(), key.1.to_string(), key.2);
    it.set_cookies.retain(|c| {
        let (n, p, d) = cookie_key(c);
        (n, p, d.as_str()) != (key.0.as_str(), key.1.as_str(), key.2.as_str())
    });
    it.set_cookies.push(line);
}

/// What a `parallel` function queued, taken by the request it ran for (the functions in their
/// order): its cookies, each replacing one of its name, path and domain as `queue` does, and the
/// session, when it set one or the request has not read it yet (FEEDBACK55.md, Q-796; branch
/// feedback55).
pub(crate) fn take_back(it: &mut Interp, cookies: Vec<String>, session: Option<Value>) {
    let name = cookie_name(it);
    let mut set = false;
    for c in cookies {
        set |= cookie_key(&c).0 == name;
        queue(it, c);
    }
    if session.is_some() && (set || it.session.is_none()) {
        it.session = session;
    }
}

/// A `Set-Cookie` line's name, path and domain (lower case, without a leading `.`), which a
/// browser keeps it by (FEEDBACK55.md, Q-714; branch feedback55).
fn cookie_key(line: &str) -> (&str, &str, String) {
    let mut parts = line.split("; ");
    let name = parts.next().and_then(|p| p.split_once('=')).map_or("", |(n, _)| n);
    let (mut path, mut domain) = ("", String::new());
    for p in parts {
        if let Some(v) = p.strip_prefix("Path=") {
            path = v;
        } else if let Some(v) = p.strip_prefix("Domain=") {
            domain = v.trim_start_matches('.').to_ascii_lowercase();
        }
    }
    (name, path, domain)
}

fn opts(args: &[Value], i: usize, f: &str, line: u32) -> Result<Option<Map>, Ctrl> {
    match args.get(i) {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Map(m)) => Ok(Some((**m).clone())),
        Some(v) => fail(line, format!("`{}` expects options as a map, got {}", f, v.type_name())),
    }
}

fn name_arg(args: &[Value], f: &str, line: u32) -> Result<String, Ctrl> {
    match args.first() {
        Some(Value::Str(n)) if token_ok(n) => Ok(n.to_string()),
        Some(v) => fail(line, format!("`{}` needs a cookie name of letters, digits and -_.!#$%&'*+^`|~, got {}", f, v.repr())),
        None => fail(line, format!("`{}` needs a cookie name", f)),
    }
}

/// `set_cookie(name, value, { max_age, path, domain, http_only, secure,
/// same_site })`: sent with the response, whatever the route returns.
fn set_cookie(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = name_arg(&args, "set_cookie", line)?;
    let value = match args.get(1) {
        Some(Value::Str(s)) => s.to_string(),
        Some(Value::Nil) | None => String::new(),
        Some(v) => v.display(),
    };
    let o = opts(&args, 2, "set_cookie", line)?;
    let l = cookie_line(it, &name, &value, None, o.as_ref(), "set_cookie", line)?;
    queue(it, l);
    Ok(Value::Nil)
}

/// `delete_cookie(name, { path, domain })`: the path and domain it was set with.
fn delete_cookie(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = name_arg(&args, "delete_cookie", line)?;
    let o = opts(&args, 1, "delete_cookie", line)?;
    let l = cookie_line(it, &name, "", Some(0), o.as_ref(), "delete_cookie", line)?;
    queue(it, l);
    Ok(Value::Nil)
}

/// The session cookie's name here: `__Host-` when the cookie is `Secure`
/// and for the whole site, as browsers require of that prefix.
fn cookie_name(it: &Interp) -> &'static str {
    if it.base.is_empty() && !PLAIN_HTTP.load(std::sync::atomic::Ordering::Relaxed) {
        HOST_COOKIE
    } else {
        COOKIE
    }
}

/// Every value the request has for the session cookie: a program mounted
/// under a path gets its own and its parent's.
pub(crate) fn cookies(it: &mut Interp) -> Vec<String> {
    let name = cookie_name(it);
    let raw = header(it, "cookie").unwrap_or_default();
    // Each, read as `request.cookies` reads one (FEEDBACK55.md, Q-718; branch feedback55).
    raw.split(';').filter_map(crate::server::cookie_pair).filter(|(k, _)| *k == name).map(|(_, v)| v.to_string()).collect()
}

/// Queues the session cookie (empty with age 0: removes it).
pub(crate) fn send_cookie(it: &mut Interp, value: &str, age: Option<u64>, line: u32) -> Result<(), Ctrl> {
    let name = cookie_name(it);
    let l = cookie_line(it, name, value, age, None, "set_session", line)?;
    queue(it, l);
    Ok(())
}

/// `session()`: the map `set_session` stored, or `{}`.
fn session(it: &mut Interp, _: Vec<Value>, _: u32) -> R {
    it.visitor_read = true;
    // Marked here, not only on the way to the cookie: a second `live` loader of a page, or one
    // after a route that read the session, takes the kept copy, and its run went to every
    // viewer as nobody's (FEEDBACK55.md, Q-876; branch feedback55).
    it.read_request = true;
    if let Some(s) = &it.session {
        return Ok(s.clone());
    }
    let found = arrived(it).map_or_else(|| Value::map(Map::new()), |(d, _)| d);
    it.session = Some(found.clone());
    Ok(found)
}

/// `cookie(name)`: the request's cookie of that name, as `request.cookies`
/// has it, or null (also outside a request). It works in any function the
/// request runs, as `session()` does, so a helper need not be handed
/// `request` by each caller.
fn cookie(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = match args.first() {
        Some(Value::Str(s)) => s.clone(),
        other => return fail(line, format!("`cookie` expects a cookie's name, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    it.visitor_read = true;
    let Some(Value::Map(req)) = it.request_value() else { return Ok(Value::Nil) };
    Ok(match req.get("cookies") {
        Some(Value::Map(c)) => c.get(&*name).cloned().unwrap_or(Value::Nil),
        _ => Value::Nil,
    })
}

/// `set_session(map, { max_age })`: replaces the session.
fn set_session(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let data = match args.first() {
        Some(v @ Value::Map(_)) => v.clone(),
        Some(Value::Nil) => return end_session(it, Vec::new(), line),
        other => return fail(line, format!("`set_session` expects a map, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let o = opts(&args, 1, "set_session", line)?;
    let age = match o.as_ref().and_then(|m| m.get("max_age")) {
        None | Some(Value::Nil) => DEFAULT_AGE,
        Some(v) => match crate::builtins::duration_seconds(v) {
            Some(n) if n > 0 => n,
            _ => return fail(line, format!("`set_session`'s max_age is seconds or a duration like \"30d\", got {}", v.repr())),
        },
    };
    let mut stored = serde_json::json!({ "e": now() + age, "d": crate::interp::island_json(&data) });
    // Read back as JSON at most `JSON_NESTING` deep: one deeper was stored, and the next request
    // had no session (FEEDBACK55.md, Q-875's second review; branch feedback55).
    if crate::value::json_value_depth(&stored) > crate::value::JSON_NESTING {
        return fail(line, format!("`set_session`'s map nests more than {} deep as it is stored, which no request could read back: keep it flatter, or the rest in the database", crate::value::JSON_NESTING));
    }
    let ctx = context(it);
    let seal = |stored: &serde_json::Value| {
        let mut json = Vec::new();
        serde_json::to_writer(&mut json, stored).ok();
        let payload = b64url(&json);
        let signed = sign(&ctx, &payload);
        format!("{}.{}", payload, signed)
    };
    let bare = seal(&stored);
    let user_of = |v: &Value| match v {
        Value::Map(m) => m.get("user").filter(|u| !matches!(u, Value::Nil)).cloned(),
        _ => None,
    };
    let was = it.session.clone().or_else(|| arrived(it).map(|(d, _)| d));
    if let (Some(a), Some(b)) = (was.as_ref().and_then(user_of), user_of(&data)) {
        if !crate::value::equal(&a, &b) {
            it.session_id = Some(None);
        }
    }
    stored["i"] = serde_json::json!(&*session_id(it));
    let mut value = seal(&stored);
    // What a program can keep in a session is what it always was: an id that does not fit is left
    // out, and an `Idempotency-Key` goes by what else says who this is (FEEDBACK55.md, Q-887; branch
    // feedback55).
    if value.len() > MAX_COOKIE {
        value = bare;
        it.session_id = Some(None);
    }
    if value.len() > MAX_COOKIE {
        return fail(
            line,
            format!("the session is {} bytes signed, more than a cookie holds ({}); keep an id in it and the rest in the database", value.len(), MAX_COOKIE),
        );
    }
    send_cookie(it, &value, Some(age), line)?;
    it.session = Some(data);
    Ok(Value::Nil)
}

/// `end_session()`: forgets the session (logging out).
fn end_session(it: &mut Interp, _: Vec<Value>, line: u32) -> R {
    send_cookie(it, "", Some(0), line)?;
    it.session = Some(Value::map(Map::new()));
    it.session_id = Some(None);
    Ok(Value::Nil)
}
