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
//! 127.0.0.1.)

use crate::crypto::{b64url, hmac_sha256, unb64url};
use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::sync::OnceLock;

type R = Result<Value, Ctrl>;

pub const COOKIE: &str = "quillo_session";

/// Served over plain HTTP on purpose (`--watch`, desktop apps): cookies
/// are not `Secure`.
pub static PLAIN_HTTP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Browsers keep cookies up to about 4096 bytes, name and attributes included.
const MAX_COOKIE: usize = 3900;
const DEFAULT_AGE: u64 = 30 * 86400;

pub const BUILTINS: &[(&str, BuiltinFn)] = &[
    ("set_cookie", set_cookie),
    ("delete_cookie", delete_cookie),
    ("session", session),
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

/// The session in a cookie value, if it is signed for here and current.
fn open(ctx: &str, cookie: &str) -> Option<Value> {
    let (payload, sig) = cookie.split_once('.')?;
    if !crate::crypto::same(sign(ctx, payload).as_bytes(), sig.as_bytes()) {
        return None;
    }
    let j: serde_json::Value = serde_json::from_slice(&unb64url(payload)?).ok()?;
    if j.get("e")?.as_u64()? <= now() {
        return None;
    }
    Some(Value::from_json(j.get("d")?))
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

/// The `Set-Cookie` line, with the defaults and whatever `opts` changes.
fn cookie_line(it: &mut Interp, name: &str, value: &str, age: Option<u64>, opts: Option<&Map>, f: &str, line: u32) -> Result<String, Ctrl> {
    let opt = |k: &str| opts.and_then(|m| m.get(k)).filter(|v| !matches!(v, Value::Nil)).cloned();
    let path = match opt("path") {
        // Visible characters only: a line break would start another header.
        Some(Value::Str(p)) if p.starts_with('/') && p.bytes().all(|b| (b'!'..=b'~').contains(&b) && b != b';') => p.to_string(),
        Some(v) => return fail(line, format!("`{}`'s path starts with /, got {}", f, v.repr())),
        None if it.base.is_empty() => "/".into(),
        None => it.base.clone(),
    };
    let mut s = format!("{}={}; Path={}", name, encode(value), path);
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
    let secure = same_site == "None" || opt("secure").map_or(!plain, |v| v.truthy());
    if secure {
        s.push_str("; Secure");
    }
    s.push_str("; SameSite=");
    s.push_str(&same_site);
    Ok(s)
}

/// Queues the cookie for the response, replacing one queued under its name.
fn queue(it: &mut Interp, name: &str, line: String) {
    let prefix = format!("{}=", name);
    it.set_cookies.retain(|c| !c.starts_with(&prefix));
    it.set_cookies.push(line);
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
    queue(it, &name, l);
    Ok(Value::Nil)
}

/// `delete_cookie(name, { path, domain })`: the path and domain it was set with.
fn delete_cookie(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = name_arg(&args, "delete_cookie", line)?;
    let o = opts(&args, 1, "delete_cookie", line)?;
    let l = cookie_line(it, &name, "", Some(0), o.as_ref(), "delete_cookie", line)?;
    queue(it, &name, l);
    Ok(Value::Nil)
}

/// `session()`: the map `set_session` stored, or `{}`.
fn session(it: &mut Interp, _: Vec<Value>, _: u32) -> R {
    if let Some(s) = &it.session {
        return Ok(s.clone());
    }
    let ctx = context(it);
    // Every cookie of that name: a program mounted under a path gets its
    // own and its parent's.
    let raw = header(it, "cookie").unwrap_or_default();
    let found = raw
        .split(';')
        .filter_map(|c| c.trim().split_once('='))
        .filter(|(k, _)| *k == COOKIE)
        .find_map(|(_, v)| open(&ctx, v))
        .unwrap_or_else(|| Value::map(Map::new()));
    it.session = Some(found.clone());
    Ok(found)
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
    let mut json = Vec::new();
    serde_json::to_writer(&mut json, &serde_json::json!({ "e": now() + age, "d": data.to_json() })).ok();
    let payload = b64url(&json);
    let ctx = context(it);
    let value = format!("{}.{}", payload, sign(&ctx, &payload));
    if value.len() > MAX_COOKIE {
        return fail(
            line,
            format!("the session is {} bytes signed, more than a cookie holds ({}); keep an id in it and the rest in the database", value.len(), MAX_COOKIE),
        );
    }
    let l = cookie_line(it, COOKIE, &value, Some(age), None, "set_session", line)?;
    queue(it, COOKIE, l);
    it.session = Some(data);
    Ok(Value::Nil)
}

/// `end_session()`: forgets the session (logging out).
fn end_session(it: &mut Interp, _: Vec<Value>, line: u32) -> R {
    let l = cookie_line(it, COOKIE, "", Some(0), None, "end_session", line)?;
    queue(it, COOKIE, l);
    it.session = Some(Value::map(Map::new()));
    Ok(Value::Nil)
}
