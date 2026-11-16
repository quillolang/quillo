//! `notification`: a message to a person's devices, the page open or not, over Web Push
//! (FEEDBACK67.md, B.3; branch pwa).
//!
//! ```text
//! component Alerts() {
//!     state on = notification.state()          // "granted", "denied", "default" or "unsupported"
//!     <button @click={() => { on = notification.ask() }}>Tell me when it ships</button>
//! }
//! server fn ship(order: int) {
//!     notification.send(user_id, { title: "Shipped", body: `Order ${order}`, url: `/orders/${order}` })
//! }                                            // { sent, gone }
//! ```
//!
//! In the browser (src/client/runtime.js), `ask` registers Quillo's worker (/_quillo/sw.js), asks
//! the person, subscribes with the server's key and posts the subscription to `/_quillo/push`,
//! which binds it to `session().user`, or to the `user:` given to `ask`, which the server made
//! with `notification.user(x)` (sealed, so that no page can name someone else). They are kept in
//! `_quillo_push`, where Quillo keeps its own tables (quillo-system.db), and `end_session()`
//! unbinds the session's. `send` encrypts for each (RFC 8291, aes128gcm, one record of 4096),
//! signs for its push service (RFC 8292, VAPID, ES256), posts off the interpreter's lock on one
//! kept connection to each push service (Q-960), and drops what the push service says has ended
//! (404, 410). The VAPID key is derived from the session key, so every instance and every server
//! that shares `QUILLO_SECRET` signs alike (Q-956). A program that never names `notification`
//! gets none of this: no endpoint, no worker beyond the offline one, no table.

use crate::ast::Expr;
use crate::interp::{fail, Ctrl, Interp};
use crate::server::{HttpRequest, HttpResponse};
use crate::value::*;
use std::sync::{Arc, Mutex, OnceLock};

type R = Result<Value, Ctrl>;

pub fn install(m: &mut Map) {
    let fns: &[(&'static str, BuiltinFn)] = &[
        ("notification.ask", in_browser),
        ("notification.state", state),
        ("notification.stop", in_browser),
        ("notification.send", send),
        ("notification.user", user),
    ];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("notification.").into(), Value::Builtin(name, *f));
    }
}

/// The table, beside `_quillo_mcp_tokens`.
const TABLE: &str = "_quillo_push";
/// What a message holds at most: one record of 4,096 bytes, as push services take.
pub const MAX_PAYLOAD: usize = 3800;
/// The longest endpoint taken: push services' are 150 to 500 characters.
const MAX_ENDPOINT: usize = 1000;
/// The most a `/_quillo/push` request may send.
const MAX_BODY: usize = 8 << 10;
/// How long a `notification.user(x)` stays good: a session's 30 days.
const USER_AGE: u64 = 30 * 86_400;
const USER_PURPOSE: &str = "quillo push user";
/// The label the VAPID key is derived under from the session key; never to change, as every
/// subscription is made with the key it gives.
const VAPID_LABEL: &[u8] = b"quillo vapid p-256 key";

/// `notification.ask` and `notification.stop` speak to the browser; on the server they fail, as
/// `ask` does, saying where they go.
fn in_browser(_: &mut Interp, _: Vec<Value>, line: u32) -> R {
    fail(line, BROWSER_ONLY)
}

/// What `notification.ask`, `.state` and `.stop` on the server are told, here and by `quillo check`.
pub const BROWSER_ONLY: &str = "`notification.ask`, `notification.state` and `notification.stop` ask and read the browser: call them from an event handler of a component (`@click`); the server sends with `notification.send(user, { title, body, url })`";

/// `notification.state()`: the browser's. While the server renders a component, null (as `js`
/// gives), and the state it sets is read again in the browser; elsewhere on the server, a mistake.
fn state(it: &mut Interp, _: Vec<Value>, line: u32) -> R {
    if it.in_component {
        return Ok(Value::Nil);
    }
    fail(line, BROWSER_ONLY)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Who a subscription is bound to, as it is kept: the user as JSON, so 7 and "7" are two.
fn who_of(v: &Value) -> String {
    crate::interp::island_json(v).to_string()
}

/// `notification.user(x)`: `x`, sealed for `ask`'s `user:` option, for the session it was made in
/// (when there is one) and this place, for 30 days. A page that gives `ask` a plain value could
/// name anyone, and be sent their notifications.
fn user(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let who = match args.first() {
        None | Some(Value::Nil) => return fail(line, "`notification.user` is given who the device is for: an id, as `notification.send` is given it"),
        Some(v) => who_of(v),
    };
    if it.in_component {
        return fail(line, "`notification.user` seals on the server: make it in a route or a `server fn`, and give it to the page");
    }
    let owner = crate::session::arrived_owner(it);
    let place = crate::session::place(it);
    let sealed = crate::session::seal(USER_PURPOSE, &serde_json::json!({ "u": who, "p": place, "o": owner, "e": now() + USER_AGE }).to_string());
    Ok(Value::str(sealed))
}

// ---- the key ----------------------------------------------------------------------------------

struct Len(usize);

impl ring::hkdf::KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

fn hkdf(salt: &[u8], ikm: &[u8], info: &[&[u8]], out: &mut [u8]) {
    let prk = ring::hkdf::Salt::new(ring::hkdf::HKDF_SHA256, salt).extract(ikm);
    prk.expand(info, Len(out.len())).and_then(|okm| okm.fill(out)).expect("HKDF-SHA256 gives up to 8,160 bytes");
}

struct Vapid {
    pair: ring::signature::EcdsaKeyPair,
    /// The public key, base64url: `applicationServerKey`, and VAPID's `k`.
    key: String,
}

/// The VAPID key: a P-256 scalar from the session key by HKDF under a fixed label (another
/// counter byte for the rare value that is no scalar), its public key by p256 (ring cannot
/// make one from a scalar), and ring to sign. One per process: `--instances` share it, and servers
/// that share `QUILLO_SECRET` derive the same (FEEDBACK67.md, Q-956; branch pwa).
fn vapid() -> &'static Vapid {
    static V: OnceLock<Vapid> = OnceLock::new();
    V.get_or_init(|| {
        use p256::elliptic_curve::sec1::ToSec1Point;
        let ikm = crate::session::signing_key();
        for n in 0u8..=255 {
            let mut d = [0u8; 32];
            hkdf(&[], ikm, &[VAPID_LABEL, &[n]], &mut d);
            let Ok(secret) = p256::SecretKey::from_slice(&d) else { continue };
            let public = secret.public_key().to_sec1_point(false);
            let rng = ring::rand::SystemRandom::new();
            let pair = ring::signature::EcdsaKeyPair::from_private_key_and_public_key(&ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING, &d, public.as_bytes(), &rng)
                .expect("ring takes the key p256 made");
            return Vapid { pair, key: crate::crypto::b64url(public.as_bytes()) };
        }
        unreachable!("256 tries, each a 1 in 2^32 chance of no scalar")
    })
}

/// The server's public key, base64url, as a page subscribes with it.
pub fn public_key() -> &'static str {
    &vapid().key
}

// ---- where subscriptions may lead -------------------------------------------------------------

/// Whether a push service on this machine is taken (plain http to 127.0.0.1, ::1 or localhost):
/// under --watch, or with QUILLO_PUSH_LOCAL=1, for tests that play the push service. Elsewhere
/// any visitor could have the server post to its own ports.
fn local_taken() -> bool {
    crate::platform::DEV.load(std::sync::atomic::Ordering::Relaxed) || std::env::var("QUILLO_PUSH_LOCAL").is_ok_and(|v| v == "1")
}

fn loopback(h: &url::Host<&str>) -> bool {
    match h {
        url::Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(a) => a.is_loopback(),
        url::Host::Ipv6(a) => a.is_loopback(),
    }
}

/// An endpoint Quillo sends to: https to an address on the public internet, as `http.*`'s
/// `public_only` has it (checked again on the addresses the name gives as it connects), or plain
/// http to this machine where that is taken (`local_taken`). Nothing else: no other scheme, no
/// user in it, no private address.
fn endpoint_ok(e: &str) -> Result<url::Url, String> {
    if e.len() > MAX_ENDPOINT || !e.is_ascii() {
        return Err(format!("the endpoint is longer than {} characters, or not ASCII", MAX_ENDPOINT));
    }
    let u = url::Url::parse(e).map_err(|_| "the endpoint is not a URL".to_string())?;
    if !u.username().is_empty() || u.password().is_some() || u.fragment().is_some() {
        return Err("the endpoint holds a user or a fragment".into());
    }
    let host = u.host().ok_or("the endpoint has no host")?;
    let public = match &host {
        url::Host::Domain(d) => !(d.eq_ignore_ascii_case("localhost") || d.to_ascii_lowercase().ends_with(".localhost")),
        url::Host::Ipv4(a) => crate::fetch::public_ip((*a).into()),
        url::Host::Ipv6(a) => crate::fetch::public_ip((*a).into()),
    };
    match u.scheme() {
        "https" if public => Ok(u),
        "http" if loopback(&host) && local_taken() => Ok(u),
        "http" if loopback(&host) => Err("the endpoint is plain http to this machine, which is taken only under --watch or with QUILLO_PUSH_LOCAL=1".into()),
        "https" | "http" => Err("the endpoint is not https to an address on the public internet".into()),
        other => Err(format!("the endpoint is {}:, not https", other)),
    }
}

/// An origin as a page has it: `https://shop.example`, a port when it has one, nothing after.
fn origin_ok(o: &str) -> Option<String> {
    let u = url::Url::parse(o).ok()?;
    if !matches!(u.scheme(), "http" | "https") || u.host().is_none() || !matches!(u.path(), "" | "/") || u.query().is_some() || !u.username().is_empty() {
        return None;
    }
    Some(u.origin().ascii_serialization())
}

// ---- the table --------------------------------------------------------------------------------

fn make_table(mysql: bool) -> String {
    format!(
        "create table if not exists _quillo_push (endpoint varchar({}) {}primary key, p256dh text not null, auth text not null, who text not null, \
         session text not null, origin text not null, place text not null, created bigint not null, succeeded bigint not null){}",
        MAX_ENDPOINT,
        if mysql { "character set ascii collate ascii_bin " } else { "" },
        if mysql { " character set utf8mb4 collate utf8mb4_bin" } else { "" }
    )
}

/// A statement on Quillo's own tables there: its value, or the database's error.
fn sql(it: &mut Interp, on: Option<usize>, query: bool, args: Vec<Value>) -> Result<Value, String> {
    let r = if query { crate::db::query_on(it, on, args) } else { crate::db::exec_on(it, on, args) };
    match r {
        Ok(Value::Err(e, _, _)) => Err(e.display()),
        Ok(v) => Ok(v),
        Err(Ctrl::Error(e)) => Err(e.msg.clone()),
        Err(_) => Err("the statement was stopped".into()),
    }
}

/// Where the table is: None where there is none yet (and, with `make`, none could be made).
fn table_on(it: &mut Interp, make: bool) -> Result<Option<Option<usize>>, String> {
    let Some(on) = crate::db::system_on_for(it, make, true) else {
        return if make { Err("the system database (quillo-system.db) could not be opened".into()) } else { Ok(None) };
    };
    if crate::db::own_table_there(it, on, TABLE)? {
        return Ok(Some(on));
    }
    if !make {
        return Ok(None);
    }
    let mysql = crate::db::kind_on(it, on);
    crate::db::own_table_make(it, on, TABLE, &make_table(mysql))
        .map_err(|e| format!("its table, {}, could not be made ({}): the program's database user needs the right to create a table there, once", TABLE, e))?;
    Ok(Some(on))
}

/// A subscription as it is kept.
#[derive(Clone)]
struct Sub {
    endpoint: String,
    p256dh: Vec<u8>,
    auth: Vec<u8>,
    origin: String,
}

fn text(m: &Map, k: &str) -> String {
    m.get(k).map(|v| v.display()).unwrap_or_default()
}

/// `who`'s subscriptions in `place`, compared exactly here, whatever a database's collation takes
/// for the same.
fn subscriptions(it: &mut Interp, who: &str, place: &str) -> Result<Vec<Sub>, String> {
    let Some(on) = table_on(it, false)? else { return Ok(Vec::new()) };
    let rows = sql(it, on, true, vec![Value::str("select endpoint, p256dh, auth, who, origin, place from _quillo_push where who = ? and place = ?"), Value::str(who), Value::str(place)])?;
    let Value::List(l) = rows else { return Ok(Vec::new()) };
    Ok(l.iter()
        .filter_map(|r| {
            let Value::Map(m) = r else { return None };
            if text(m, "who") != who || text(m, "place") != place {
                return None;
            }
            Some(Sub { endpoint: text(m, "endpoint"), p256dh: crate::crypto::unb64url(&text(m, "p256dh"))?, auth: crate::crypto::unb64url(&text(m, "auth"))?, origin: text(m, "origin") })
        })
        .collect())
}

/// `end_session()`'s part, and a change of person in `set_session`: the subscriptions made in each
/// of these sessions (by their owners, as `session::current_owner` names them) are unbound here,
/// so that the next person on the device gets nothing meant for the one before.
pub(crate) fn unbind_sessions(it: &mut Interp, owners: &[String]) -> Result<(), String> {
    if owners.is_empty() {
        return Ok(());
    }
    let Some(on) = table_on(it, false)? else { return Ok(()) };
    let place = crate::session::place(it);
    for o in owners.iter().filter(|o| !o.is_empty()) {
        sql(it, on, false, vec![Value::str("delete from _quillo_push where session = ? and place = ?"), Value::str(o), Value::str(&place)])?;
    }
    Ok(())
}

// ---- /_quillo/push ----------------------------------------------------------------------------

fn answer(status: u16, j: serde_json::Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![("content-type".into(), "application/json".into()), ("cache-control".into(), "no-store".into())],
        body: j.to_string().into_bytes(),
    }
}

fn refused(status: u16, msg: &str) -> HttpResponse {
    answer(status, serde_json::json!({ "error": msg }))
}

/// `<base>/_quillo/push`, for a program that uses `notification` (FEEDBACK67.md, B.3; branch pwa):
/// GET, the key a page subscribes with (none in the phone shells and `quillo app`, where there is
/// no push); POST, a subscription bound to who the visitor is; DELETE, one unbound. From the
/// program's own pages only, as a server fn is called. Read with the request as `it`'s current
/// one, for its session.
#[cold]
#[inline(never)]
pub fn endpoint(it: &mut Interp, req: &HttpRequest, path: &str) -> HttpResponse {
    it.current_request = Some(Value::map(crate::server::build_request(req, path, &Value::map(Map::new()))));
    let r = answer_push(it, req);
    it.current_request = None;
    r
}

fn answer_push(it: &mut Interp, req: &HttpRequest) -> HttpResponse {
    if req.method == "GET" || req.method == "HEAD" {
        let key = (crate::platform::runtime() == "server").then(public_key);
        return answer(200, serde_json::json!({ "key": key }));
    }
    if !matches!(req.method.as_str(), "POST" | "DELETE") {
        return refused(405, "GET, POST or DELETE");
    }
    if crate::server::cross_site(req) {
        return refused(403, "only the program's own pages subscribe here");
    }
    let ctype = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map_or("", |(_, v)| v.as_str());
    if crate::server::essence(ctype) != "application/json" {
        return refused(415, "send JSON (content-type: application/json)");
    }
    if req.body.len() > MAX_BODY {
        return refused(413, "a subscription is a few hundred bytes");
    }
    let Ok(j) = serde_json::from_slice::<serde_json::Value>(&req.body) else { return refused(400, "the body is not JSON") };
    let Some(endpoint) = j["endpoint"].as_str() else { return refused(400, "no endpoint") };
    if req.method == "DELETE" {
        return match unbind(it, endpoint) {
            Ok(()) => answer(200, serde_json::json!({ "ok": true })),
            Err(e) => refused(500, &e),
        };
    }
    match bind(it, req, endpoint, &j) {
        Ok(()) => answer(200, serde_json::json!({ "ok": true })),
        Err((status, e)) => refused(status, &e),
    }
}

fn unbind(it: &mut Interp, endpoint: &str) -> Result<(), String> {
    let Some(on) = table_on(it, false)? else { return Ok(()) };
    let place = crate::session::place(it);
    sql(it, on, false, vec![Value::str("delete from _quillo_push where endpoint = ? and place = ?"), Value::str(endpoint), Value::str(&place)]).map(|_| ())
}

/// The subscription the page posted, bound and kept: checked first, so that a send never meets
/// what it cannot use.
fn bind(it: &mut Interp, req: &HttpRequest, endpoint: &str, j: &serde_json::Value) -> Result<(), (u16, String)> {
    let bad = |e: String| (400, e);
    endpoint_ok(endpoint).map_err(bad)?;
    let key = |k: &str, len: usize| -> Result<Vec<u8>, (u16, String)> {
        let s = j["keys"][k].as_str().ok_or_else(|| bad(format!("no keys.{}", k)))?;
        crate::crypto::unb64url(s.trim_end_matches('=')).filter(|b| b.len() == len).ok_or_else(|| bad(format!("keys.{} is not {} bytes of base64url", k, len)))
    };
    let (p256dh, auth) = (key("p256dh", 65)?, key("auth", 16)?);
    if p256::PublicKey::from_sec1_bytes(&p256dh).is_err() {
        return Err(bad("keys.p256dh is not a P-256 public key".into()));
    }
    // The origin the page was on: navigate's, and VAPID's `sub`.
    let said = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("origin")).map(|(_, v)| v.as_str()).or_else(|| j["origin"].as_str());
    let origin = said.and_then(origin_ok).ok_or_else(|| bad("no origin: the page's own (location.origin) is sent with it".into()))?;
    let place = crate::session::place(it);
    let owner = crate::session::arrived_owner(it);
    let who = match &j["user"] {
        serde_json::Value::Null => {
            let s = crate::session::current(it);
            match &s {
                Value::Map(m) => m.get("user").filter(|u| !matches!(u, Value::Nil)).map(who_of),
                _ => None,
            }
            .ok_or_else(|| (403, "no one to send to: the visitor is not signed in (session().user), and ask was given no user: notification.user(x), made on the server, says who".to_string()))?
        }
        serde_json::Value::String(t) => sealed_user(t, &place, owner.as_deref()).ok_or_else(|| (403, "the user given to notification.ask is not one notification.user made here, for this session, in the last 30 days: make it on the server with notification.user(x) and give it to the page".to_string()))?,
        _ => return Err((403, "the user given to notification.ask is made on the server with notification.user(x), so that a page cannot name someone else".into())),
    };
    let on = table_on(it, true).map_err(|e| (500, e))?.ok_or((500, "no table".to_string()))?;
    let row = |endpoint: &str| {
        vec![
            Value::str("insert into _quillo_push (endpoint, p256dh, auth, who, session, origin, place, created, succeeded) values (?, ?, ?, ?, ?, ?, ?, ?, 0)"),
            Value::str(endpoint),
            Value::str(crate::crypto::b64url(&p256dh)),
            Value::str(crate::crypto::b64url(&auth)),
            Value::str(&who),
            Value::str(owner.as_deref().unwrap_or("")),
            Value::str(&origin),
            Value::str(&place),
            Value::Int(now() as i64),
        ]
    };
    // The endpoint is the key: the device is whoever subscribed it last. Twice, for a page that
    // posted the same one at once.
    let mut last = String::new();
    for _ in 0..2 {
        let gone = sql(it, on, false, vec![Value::str("delete from _quillo_push where endpoint = ?"), Value::str(endpoint)]);
        match gone.and_then(|_| sql(it, on, false, row(endpoint))) {
            Ok(_) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err((500, last))
}

/// Who `notification.user` sealed in `token`, if it was made here, for this session (when it was
/// made in one), and is still good.
fn sealed_user(token: &str, place: &str, owner: Option<&str>) -> Option<String> {
    let j: serde_json::Value = serde_json::from_str(&crate::session::unseal(USER_PURPOSE, token)?).ok()?;
    if j["p"].as_str()? != place || j["e"].as_u64()? <= now() {
        return None;
    }
    if let Some(o) = j["o"].as_str() {
        if owner != Some(o) {
            return None;
        }
    }
    j["u"].as_str().map(str::to_string)
}

// ---- sending ----------------------------------------------------------------------------------

/// What `send` was told: the message, and how the push service is to keep it.
struct Message {
    title: String,
    body: Option<String>,
    path: String,
    tag: Option<String>,
    badge: Option<i64>,
    silent: bool,
    ttl: u64,
    urgency: &'static str,
}

/// What `send` takes; `quillo check` refuses any other written in the call, as it runs does.
pub const OPTIONS: &[&str] = &["title", "body", "url", "tag", "badge", "silent", "ttl", "urgency"];
/// The longest a push service keeps a message for a device that is away: four weeks.
const MAX_TTL: u64 = 2_419_200;

fn message(it: &Interp, o: &Map, line: u32) -> Result<Message, Ctrl> {
    if let Some(k) = o.keys().find(|k| !OPTIONS.contains(&k.as_ref())) {
        return fail(line, format!("`notification.send`: unknown option `{}` (the options are {})", k.as_ref(), OPTIONS.join(", ")));
    }
    let text = |k: &str| -> Result<Option<String>, Ctrl> {
        match o.get(k) {
            None | Some(Value::Nil) => Ok(None),
            Some(Value::Str(s)) => Ok(Some(s.to_string())),
            Some(v) => fail(line, format!("`notification.send`'s `{}` is text, got {}", k, v.type_name())),
        }
    };
    let title = text("title")?.filter(|t| !t.is_empty());
    let Some(title) = title else { return fail(line, "`notification.send` needs a `title`: the line the device shows first") };
    let url = text("url")?.unwrap_or_else(|| "/".into());
    if !url.starts_with('/') || url.starts_with("//") || url.contains(['\\', '\r', '\n', '\t']) {
        return fail(line, format!("`notification.send`'s `url` is a path on this site, like \"/orders/7\", got {:?}", url));
    }
    let badge = match o.get("badge") {
        None | Some(Value::Nil) => None,
        Some(Value::Int(n)) if *n >= 0 => Some(*n),
        Some(v) => return fail(line, format!("`notification.send`'s `badge` is a whole number, 0 or more (the app's badge), got {}", v.repr())),
    };
    let silent = match o.get("silent") {
        None | Some(Value::Nil) => false,
        Some(Value::Bool(b)) => *b,
        Some(v) => return fail(line, format!("`notification.send`'s `silent` is true or false, got {}", v.type_name())),
    };
    let ttl = match o.get("ttl") {
        None | Some(Value::Nil) => 86_400,
        Some(v) => match crate::builtins::duration_seconds(v) {
            Some(n) if n <= MAX_TTL => n,
            _ => return fail(line, format!("`notification.send`'s `ttl` is how long a push service keeps it for a device that is away: seconds or a duration like \"1h\", at most 4 weeks, got {}", v.repr())),
        },
    };
    let urgency = match text("urgency")?.as_deref() {
        None | Some("normal") => "normal",
        Some("very-low") => "very-low",
        Some("low") => "low",
        Some("high") => "high",
        Some(u) => return fail(line, format!("`notification.send`'s `urgency` is \"very-low\", \"low\", \"normal\" or \"high\", got {:?}", u)),
    };
    Ok(Message { title, body: text("body")?, path: it.with_base(&url), tag: text("tag")?, badge, silent, ttl, urgency })
}

impl Message {
    /// Declarative Web Push's JSON (`"web_push": 8030`): what Safari shows without the worker, and
    /// Quillo's worker shows elsewhere. `navigate` from the origin the subscription was made on.
    fn payload(&self, origin: &str) -> String {
        let mut n = serde_json::Map::new();
        n.insert("title".into(), self.title.clone().into());
        if let Some(b) = &self.body {
            n.insert("body".into(), b.clone().into());
        }
        n.insert("navigate".into(), format!("{}{}", origin, self.path).into());
        if let Some(t) = &self.tag {
            n.insert("tag".into(), t.clone().into());
        }
        if self.silent {
            n.insert("silent".into(), true.into());
        }
        if let Some(b) = self.badge {
            n.insert("app_badge".into(), b.into());
        }
        serde_json::json!({ "web_push": 8030, "notification": n }).to_string()
    }
}

/// `notification.send(user, { title, body, url, tag, badge, silent, ttl, urgency })`: to each of
/// `user`'s devices in this place. `{ sent, gone }`: how many push services took it, and how many
/// said the subscription had ended (dropped). Waits as `http.post` does, off the interpreter's
/// lock; a message longer than one record takes is an error value. Not in a component, which
/// runs in the browser too.
fn send(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    if it.in_component {
        return fail(line, "`notification.send` sends from the server; a component runs in the browser too: call it from a `server fn`");
    }
    let who = match args.first() {
        None | Some(Value::Nil) => return fail(line, "`notification.send` is given who to send to first: the user, as `session().user` names them"),
        Some(v) => who_of(v),
    };
    let o = match args.get(1) {
        Some(Value::Map(m)) => m.clone(),
        other => return fail(line, format!("`notification.send` takes the message as a map ({{ title, body, url }}), got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let msg = message(it, &o, line)?;
    let place = crate::session::place(it);
    let subs = match subscriptions(it, &who, &place) {
        Ok(s) => s,
        Err(e) => return Ok(Value::err(Value::str(format!("notification.send: {}", e)))),
    };
    let longest = subs.iter().map(|s| s.origin.as_str()).max_by_key(|o| o.len()).unwrap_or("").to_string();
    let size = msg.payload(&longest).len();
    if size > MAX_PAYLOAD {
        return Ok(Value::err(Value::str(format!("notification.send: the message is {} bytes as sent, more than the {} one push message holds: shorten its body", size, MAX_PAYLOAD))));
    }
    let jobs: Vec<(Sub, String)> = subs.into_iter().map(|s| {
        let p = msg.payload(&s.origin);
        (s, p)
    }).collect();
    let (ttl, urgency) = (msg.ttl, msg.urgency);
    let outcomes: Vec<(String, Outcome)> = it.unlocked(move || jobs.into_iter().map(|(s, p)| (s.endpoint.clone(), deliver(&s, &p, ttl, urgency))).collect());
    let (mut sent, mut gone) = (0i64, 0i64);
    let stamp = now() as i64;
    for (endpoint, outcome) in outcomes {
        let r = match outcome {
            Outcome::Sent => {
                sent += 1;
                touch(it, &endpoint, stamp)
            }
            Outcome::Gone => {
                gone += 1;
                unbind(it, &endpoint)
            }
            Outcome::Failed(why) => {
                eprintln!("quillo: notification.send: {}", why);
                Ok(())
            }
        };
        if let Err(e) = r {
            eprintln!("quillo: notification.send: {}", e);
        }
    }
    let mut out = Map::new();
    out.insert("sent".into(), Value::Int(sent));
    out.insert("gone".into(), Value::Int(gone));
    Ok(Value::map(out))
}

fn touch(it: &mut Interp, endpoint: &str, at: i64) -> Result<(), String> {
    let Some(on) = table_on(it, false)? else { return Ok(()) };
    sql(it, on, false, vec![Value::str("update _quillo_push set succeeded = ? where endpoint = ?"), Value::Int(at), Value::str(endpoint)]).map(|_| ())
}

enum Outcome {
    Sent,
    Gone,
    Failed(String),
}

/// One message to one subscription: encrypted, signed, posted, its answer read to the end so that
/// the connection is kept for the next (Q-960).
fn deliver(s: &Sub, payload: &str, ttl: u64, urgency: &str) -> Outcome {
    let host = url::Url::parse(&s.endpoint).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let failed = |e: String| Outcome::Failed(format!("{}: {}", host, e));
    // Checked again as it is sent: the rules may have changed since it was kept (no more
    // QUILLO_PUSH_LOCAL), and the host's policy may refuse it.
    let https = match endpoint_ok(&s.endpoint) {
        Ok(u) => u.scheme() == "https",
        Err(e) => return failed(e),
    };
    if let Err(e) = crate::policy::get().http(&s.endpoint) {
        return failed(e);
    }
    let body = match encrypt(&s.p256dh, &s.auth, payload.as_bytes()) {
        Ok(b) => b,
        Err(e) => return failed(e),
    };
    let headers = vec![
        ("content-encoding".to_string(), "aes128gcm".to_string()),
        ("ttl".to_string(), ttl.to_string()),
        ("urgency".to_string(), urgency.to_string()),
        ("authorization".to_string(), vapid_header(&s.endpoint, &s.origin)),
    ];
    let o = crate::fetch::Opts { public_only: https, timeout_ms: 30_000, ..crate::fetch::DEFAULT_OPTS };
    let resp = match crate::fetch::call(o, false, "POST", &s.endpoint, &headers, Some(&("application/octet-stream".to_string(), body))) {
        Ok(r) => r,
        Err(e) => return failed(e),
    };
    let status = resp.status();
    let mut rest = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::Read::take(resp.into_reader(), 64 << 10), &mut rest);
    match status {
        200..=299 => Outcome::Sent,
        404 | 410 => Outcome::Gone,
        _ => failed(format!("the push service answered {}{}", status, if rest.trim().is_empty() { String::new() } else { format!(": {}", rest.trim().chars().take(200).collect::<String>()) })),
    }
}

/// RFC 8291 (aes128gcm, RFC 8188): the message for one subscription, as the body to post. One
/// record of 4096 whose last byte, after the message, is the delimiter 2.
fn encrypt(p256dh: &[u8], auth: &[u8], payload: &[u8]) -> Result<Vec<u8>, String> {
    use ring::agreement;
    use ring::rand::SecureRandom;
    let rng = ring::rand::SystemRandom::new();
    let mine = agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &rng).map_err(|_| "no key could be made".to_string())?;
    let as_public = mine.compute_public_key().map_err(|_| "no key could be made".to_string())?;
    let shared = agreement::agree_ephemeral(mine, &agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, p256dh), |k| k.to_vec())
        .map_err(|_| "the subscription's p256dh is not a P-256 public key".to_string())?;
    let mut ikm = [0u8; 32];
    hkdf(auth, &shared, &[b"WebPush: info\0", p256dh, as_public.as_ref()], &mut ikm);
    let mut salt = [0u8; 16];
    rng.fill(&mut salt).map_err(|_| "no random bytes".to_string())?;
    let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
    hkdf(&salt, &ikm, &[b"Content-Encoding: aes128gcm\0"], &mut cek);
    hkdf(&salt, &ikm, &[b"Content-Encoding: nonce\0"], &mut nonce);
    let key = ring::aead::LessSafeKey::new(ring::aead::UnboundKey::new(&ring::aead::AES_128_GCM, &cek).map_err(|_| "no key".to_string())?);
    let mut sealed = payload.to_vec();
    sealed.push(2);
    key.seal_in_place_append_tag(ring::aead::Nonce::assume_unique_for_key(nonce), ring::aead::Aad::empty(), &mut sealed).map_err(|_| "could not encrypt".to_string())?;
    let mut body = Vec::with_capacity(86 + sealed.len());
    body.extend_from_slice(&salt);
    body.extend_from_slice(&4096u32.to_be_bytes());
    body.push(as_public.as_ref().len() as u8);
    body.extend_from_slice(as_public.as_ref());
    body.extend_from_slice(&sealed);
    Ok(body)
}

/// RFC 8292's `Authorization: vapid t=<JWT>, k=<key>`: ES256, `aud` the endpoint's origin, `sub`
/// the origin the subscription was made on, valid 12 hours. Signed once for each push service and
/// site, and again once less than 11 hours are left of it.
fn vapid_header(endpoint: &str, sub: &str) -> String {
    static SIGNED: OnceLock<Mutex<std::collections::HashMap<(String, String), (u64, Arc<str>)>>> = OnceLock::new();
    let aud = url::Url::parse(endpoint).map(|u| u.origin().ascii_serialization()).unwrap_or_default();
    let now = now();
    let mut kept = SIGNED.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((exp, h)) = kept.get(&(aud.clone(), sub.to_string())) {
        if *exp > now + 11 * 3600 {
            return h.to_string();
        }
    }
    let v = vapid();
    let exp = now + 12 * 3600;
    let head = crate::crypto::b64url(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = crate::crypto::b64url(serde_json::json!({ "aud": aud, "exp": exp, "sub": sub }).to_string().as_bytes());
    let input = format!("{}.{}", head, claims);
    let sig = v.pair.sign(&ring::rand::SystemRandom::new(), input.as_bytes()).map(|s| crate::crypto::b64url(s.as_ref())).unwrap_or_default();
    let h: Arc<str> = format!("vapid t={}.{}, k={}", input, sig, v.key).into();
    if kept.len() > 1000 {
        kept.clear();
    }
    kept.insert((aud, sub.to_string()), (exp, h.clone()));
    h.to_string()
}

// ---- at load ----------------------------------------------------------------------------------

/// Whether the program names the module anywhere (`notification.x`): a program that does not gets
/// none of this. Read once, at load.
pub fn used_by(it: &Interp) -> bool {
    let mut found = false;
    for stmts in it.program.iter() {
        crate::check::each_expr_of(stmts, &mut |e| {
            if let Expr::Field(obj, _, _) = e {
                found |= matches!(&**obj, Expr::Ident(n, _) if n == "notification");
            }
        });
        if found {
            return true;
        }
    }
    false
}

/// At start, for a program that uses `notification` with no `QUILLO_SECRET` and not under
/// --watch: the key is made then, so a restart ends every subscription, with nothing said to
/// anyone. Once a process.
pub fn warn_without_secret() {
    static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if crate::platform::DEV.load(std::sync::atomic::Ordering::Relaxed) || std::env::var("QUILLO_SECRET").is_ok_and(|s| !s.is_empty()) {
        return;
    }
    if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
        eprintln!("quillo: this program uses `notification` without QUILLO_SECRET: its push key is made at start, so a restart ends every device's subscription; set QUILLO_SECRET, the same on every server");
    }
}

