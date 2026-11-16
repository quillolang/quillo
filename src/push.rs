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

use crate::ast::{AttrVal, Expr, FnDef, HtmlChild, HtmlEl, Pattern, RouteDef, Seg, Stmt, TPart};
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

/// `notification.user(x)`: `x`, sealed for `ask`'s `user:` option, for this place and for the
/// session this request has now (one it has just set too), for 30 days: `/_quillo/push` takes it
/// only from that session. A page that gives `ask` a plain value could name anyone, and be sent
/// their notifications; a ticket of no session would be anyone's who saw it, so none is made
/// (FEEDBACK67.md, B.3, review; branch pwa).
fn user(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let who = match args.first() {
        None | Some(Value::Nil) => return fail(line, "`notification.user` is given who the device is for: an id, as `notification.send` is given it"),
        Some(v) => who_of(v),
    };
    if it.in_component {
        return fail(line, "`notification.user` seals on the server: make it in a route or a `server fn`, and give it to the page");
    }
    let Some(owner) = crate::session::current_owner(it) else {
        return Ok(Value::err(Value::str("notification.user: this request has no session to seal it for, and a ticket of no session could be used by anyone who saw it: set_session first (a cart, a sign-in), then make it")));
    };
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
/// with QUILLO_PUSH_LOCAL=1 alone, for tests that play the push service. Elsewhere any visitor
/// could have the server post to its own ports, --watch too, which may listen beyond this
/// machine (FEEDBACK67.md, B.3, review; branch pwa).
fn local_taken() -> bool {
    std::env::var("QUILLO_PUSH_LOCAL").is_ok_and(|v| v == "1")
}

/// A name of this machine's, written with a final dot or in any case too.
fn local_name(d: &str) -> bool {
    let d = d.trim_end_matches('.').to_ascii_lowercase();
    d == "localhost" || d.ends_with(".localhost")
}

fn loopback(h: &url::Host<&str>) -> bool {
    match h {
        url::Host::Domain(d) => local_name(d),
        url::Host::Ipv4(a) => a.is_loopback(),
        url::Host::Ipv6(a) => a.is_loopback(),
    }
}

/// An endpoint Quillo sends to: https on port 443 (where push services are) to an address on the
/// public internet, as `http.*`'s `public_only` has it (checked again on the addresses the name
/// gives as it connects), or plain http to this machine where that is taken (`local_taken`).
/// Nothing else: no other scheme or port, no user in it, no private address.
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
        url::Host::Domain(d) => !local_name(d),
        url::Host::Ipv4(a) => crate::fetch::public_ip((*a).into()),
        url::Host::Ipv6(a) => crate::fetch::public_ip((*a).into()),
    };
    match u.scheme() {
        "https" if public && u.port_or_known_default() == Some(443) => Ok(u),
        "https" if public => Err("the endpoint is https on another port than 443, where push services are".into()),
        "http" if loopback(&host) && local_taken() => Ok(u),
        "http" if loopback(&host) => Err("the endpoint is plain http to this machine, which is taken only with QUILLO_PUSH_LOCAL=1 (a test's push service)".into()),
        "https" | "http" => Err("the endpoint is not https to an address on the public internet".into()),
        other => Err(format!("the endpoint is {}:, not https", other)),
    }
}

/// The origin the page was on, from the request itself: its Host, over https where the request
/// came so (`session::came_over_https`). Never from what the page posted, which a client without
/// an Origin header set to another site, and which became `navigate` and VAPID's `sub`
/// (FEEDBACK67.md, B.3, review; branch pwa).
fn origin_of(it: &mut Interp, req: &HttpRequest) -> Option<String> {
    let host = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("host")).map(|(_, v)| v.trim().to_string())?;
    let scheme = if crate::session::came_over_https(it) { "https" } else { "http" };
    let u = url::Url::parse(&format!("{}://{}", scheme, host)).ok()?;
    if u.host().is_none() || !matches!(u.path(), "" | "/") || u.query().is_some() || !u.username().is_empty() {
        return None;
    }
    Some(u.origin().ascii_serialization())
}

// ---- the table --------------------------------------------------------------------------------

/// The table, with an index for a person's devices (each send) and one for a session's (each
/// sign-out): at 20,000 rows each scanned the table (FEEDBACK67.md, B.3, review; branch pwa).
/// MySQL's go in the statement, of a prefix of the text columns.
fn make_table(mysql: bool) -> String {
    format!(
        "create table if not exists _quillo_push (endpoint varchar({}) {}primary key, p256dh text not null, auth text not null, who text not null, \
         session text not null, origin text not null, place text not null, created bigint not null, succeeded bigint not null{}){}",
        MAX_ENDPOINT,
        if mysql { "character set ascii collate ascii_bin " } else { "" },
        if mysql { ", index _quillo_push_who (who(191), place(191)), index _quillo_push_session (session(191))" } else { "" },
        if mysql { " character set utf8mb4 collate utf8mb4_bin" } else { "" }
    )
}

/// The indexes on SQLite and PostgreSQL, each made if it is not there: after the table is made,
/// and once a process for a table an earlier build made.
const INDEXES: &[&str] = &[
    "create index if not exists _quillo_push_who on _quillo_push (who, place)",
    "create index if not exists _quillo_push_session on _quillo_push (session)",
];

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
        if make {
            indexed(it, on, false);
        }
        return Ok(Some(on));
    }
    if !make {
        return Ok(None);
    }
    let mysql = crate::db::kind_on(it, on);
    crate::db::own_table_make(it, on, TABLE, &make_table(mysql))
        .map_err(|e| format!("its table, {}, could not be made ({}): the program's database user needs the right to create a table there, once", TABLE, e))?;
    indexed(it, on, true);
    Ok(Some(on))
}

/// The indexes, where the database takes `if not exists` (MySQL's are made with the table): with
/// `made`, now; else once a process.
fn indexed(it: &mut Interp, on: Option<usize>, made: bool) {
    static ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if (!made && ONCE.swap(true, std::sync::atomic::Ordering::Relaxed)) || crate::db::kind_on(it, on) {
        return;
    }
    ONCE.store(true, std::sync::atomic::Ordering::Relaxed);
    for ix in INDEXES {
        if let Err(e) = sql(it, on, false, vec![Value::str(*ix)]) {
            eprintln!("quillo: {}: an index could not be made ({}): sends and sign-outs read the whole table", TABLE, e);
        }
    }
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

/// A database's error, as the visitor gets it: "internal error" where the server hides a
/// builtin's errors, in full under --watch; in full in the log (FEEDBACK67.md, B.3, review; branch
/// pwa).
fn failed_here(e: &str) -> HttpResponse {
    eprintln!("quillo: /_quillo/push: {}", e);
    refused(500, if crate::value::hiding() { "internal error" } else { e })
}

/// What a page does here in a minute, at most, by session: binds and unbinds. The program's own
/// middleware runs before routes, never here (FEEDBACK67.md, B.3, review; branch pwa).
const A_MINUTE: usize = 30;

/// Whether `owner`'s session has done what it may here in the last minute: each try counted, in a
/// window that moves with it.
fn too_often(owner: &str) -> bool {
    static SEEN: Mutex<Option<std::collections::HashMap<String, std::collections::VecDeque<std::time::Instant>>>> = Mutex::new(None);
    let now = std::time::Instant::now();
    let minute = std::time::Duration::from_secs(60);
    let mut seen = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let seen = seen.get_or_insert_with(Default::default);
    if seen.len() > 10_000 {
        seen.retain(|_, ts| ts.back().is_some_and(|t| now.duration_since(*t) < minute));
    }
    let ts = seen.entry(owner.to_string()).or_default();
    while ts.front().is_some_and(|t| now.duration_since(*t) >= minute) {
        ts.pop_front();
    }
    if ts.len() >= A_MINUTE {
        return true;
    }
    ts.push_back(now);
    false
}

/// Who the request's session says the visitor is, as a row keeps it: `session().user`, when set.
fn person(it: &mut Interp) -> Option<String> {
    match crate::session::current(it) {
        Value::Map(m) => m.get("user").filter(|u| !matches!(u, Value::Nil)).map(who_of),
        _ => None,
    }
}

/// `<base>/_quillo/push`, for a program that uses `notification` (FEEDBACK67.md, B.3; branch pwa):
/// GET, the key a page subscribes with (none in the phone shells and `quillo app`, where there is
/// no push); POST, a subscription bound to who the visitor is, or with `check`, whether the
/// browser's subscription is still this visitor's; DELETE, one unbound. From the program's own
/// pages only, as a server fn is called. Read with the request as `it`'s current one, for its
/// session.
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
    let owner = crate::session::arrived_owner(it);
    // A page's look at the subscription its browser holds, as it starts: many a minute, and it
    // only ever takes a binding away.
    if req.method == "POST" && j["check"] == true {
        return match check(it, endpoint, owner.as_deref()) {
            Ok(on) => answer(200, serde_json::json!({ "state": if on { "granted" } else { "default" } })),
            Err(e) => failed_here(&e),
        };
    }
    // Binding and unbinding go by the session; with none there is nothing either can do.
    let Some(owner) = owner else {
        return refused(403, "no one to send to: the visitor is not signed in (session().user), and ask was given no user: notification.user(x), made on the server, says who");
    };
    if too_often(&owner) {
        return refused(429, "more than 30 subscriptions changed in a minute by this session");
    }
    if req.method == "DELETE" {
        let who = person(it);
        return match unbind_own(it, endpoint, &owner, who.as_deref()) {
            Ok(()) => answer(200, serde_json::json!({ "ok": true })),
            Err(e) => failed_here(&e),
        };
    }
    match bind(it, req, endpoint, &j, &owner) {
        Ok(()) => answer(200, serde_json::json!({ "ok": true })),
        Err((500, e)) => failed_here(&e),
        Err((status, e)) => refused(status, &e),
    }
}

/// Whether the subscription at `endpoint` is the visitor's: bound to the person the request's
/// session names, or made in this very session (a `notification.user` ticket's). One bound to
/// anyone else, or with no one signed in now, is unbound here: the cookie before it ended, or
/// another person signed in on the device, and it still brought the one before their
/// notifications (FEEDBACK67.md, B.3, review; branch pwa).
fn check(it: &mut Interp, endpoint: &str, owner: Option<&str>) -> Result<bool, String> {
    let Some(on) = table_on(it, false)? else { return Ok(false) };
    let place = crate::session::place(it);
    let rows = sql(it, on, true, vec![Value::str("select who, session, place from _quillo_push where endpoint = ?"), Value::str(endpoint)])?;
    let Value::List(l) = rows else { return Ok(false) };
    let Some(Value::Map(row)) = l.iter().find(|r| matches!(r, Value::Map(m) if text(m, "place") == place)) else { return Ok(false) };
    let who = person(it);
    let mine = who.as_deref() == Some(text(row, "who").as_str()) || owner.is_some_and(|o| !o.is_empty() && text(row, "session") == o);
    if !mine {
        unbind(it, endpoint)?;
    }
    Ok(mine)
}

/// Unbinds the subscription at `endpoint`, in this place.
fn unbind(it: &mut Interp, endpoint: &str) -> Result<(), String> {
    let Some(on) = table_on(it, false)? else { return Ok(()) };
    let place = crate::session::place(it);
    sql(it, on, false, vec![Value::str("delete from _quillo_push where endpoint = ? and place = ?"), Value::str(endpoint), Value::str(&place)]).map(|_| ())
}

/// `stop`'s DELETE: the subscription unbound only where it is the visitor's, bound to the person
/// the session names or made in this session; anyone else's endpoint is left as it is
/// (FEEDBACK67.md, B.3, review; branch pwa).
fn unbind_own(it: &mut Interp, endpoint: &str, owner: &str, who: Option<&str>) -> Result<(), String> {
    let Some(on) = table_on(it, false)? else { return Ok(()) };
    let place = crate::session::place(it);
    let args = vec![
        Value::str("delete from _quillo_push where endpoint = ? and place = ? and (session = ? or who = ?)"),
        Value::str(endpoint),
        Value::str(&place),
        Value::str(owner),
        Value::str(who.unwrap_or("")),
    ];
    sql(it, on, false, args).map(|_| ())
}

/// The most devices one person has here: a new one past it replaces the oldest (FEEDBACK67.md,
/// B.3, review; branch pwa).
const DEVICES: usize = 20;

/// The subscription the page posted, bound and kept: checked first, so that a send never meets
/// what it cannot use.
fn bind(it: &mut Interp, req: &HttpRequest, endpoint: &str, j: &serde_json::Value, owner: &str) -> Result<(), (u16, String)> {
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
    let origin = origin_of(it, req).ok_or_else(|| bad("the request names no host to send its pages from".into()))?;
    let place = crate::session::place(it);
    let who = match &j["user"] {
        serde_json::Value::Null => person(it).ok_or_else(|| (403, "no one to send to: the visitor is not signed in (session().user), and ask was given no user: notification.user(x), made on the server, says who".to_string()))?,
        serde_json::Value::String(t) => sealed_user(t, &place, owner).ok_or_else(|| (403, "the user given to notification.ask is not one notification.user made here, for this session, in the last 30 days: make it on the server with notification.user(x) and give it to the page".to_string()))?,
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
            Value::str(owner),
            Value::str(&origin),
            Value::str(&place),
            Value::Int(now_ms()),
        ]
    };
    // The endpoint is the key: the device is whoever subscribed it last. Twice, for a page that
    // posted the same one at once.
    let mut last = None;
    for _ in 0..2 {
        let gone = sql(it, on, false, vec![Value::str("delete from _quillo_push where endpoint = ?"), Value::str(endpoint)]);
        match gone.and_then(|_| sql(it, on, false, row(endpoint))) {
            Ok(_) => {
                last = None;
                break;
            }
            Err(e) => last = Some(e),
        }
    }
    if let Some(e) = last {
        return Err((500, e));
    }
    keep_newest(it, on, &who, &place).map_err(|e| (500, e))
}

/// A person's devices here past `DEVICES`, the oldest first, unbound.
fn keep_newest(it: &mut Interp, on: Option<usize>, who: &str, place: &str) -> Result<(), String> {
    let rows = sql(it, on, true, vec![Value::str("select endpoint, who, place, created from _quillo_push where who = ? and place = ?"), Value::str(who), Value::str(place)])?;
    let Value::List(l) = rows else { return Ok(()) };
    let mut mine: Vec<(i64, String)> = l
        .iter()
        .filter_map(|r| match r {
            Value::Map(m) if text(m, "who") == who && text(m, "place") == place => Some((text(m, "created").parse().unwrap_or(0), text(m, "endpoint"))),
            _ => None,
        })
        .collect();
    if mine.len() <= DEVICES {
        return Ok(());
    }
    mine.sort();
    for (_, endpoint) in &mine[..mine.len() - DEVICES] {
        sql(it, on, false, vec![Value::str("delete from _quillo_push where endpoint = ?"), Value::str(endpoint)])?;
    }
    Ok(())
}

/// Milliseconds since 1970, rising with each call: a person's devices are told apart by when
/// each was bound, binds a millisecond apart included.
fn now_ms() -> i64 {
    static LAST: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
    let prev = LAST.fetch_max(t, std::sync::atomic::Ordering::Relaxed);
    if prev >= t {
        LAST.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
    } else {
        t
    }
}

/// Who `notification.user` sealed in `token`, if it was made here, in the session this request has
/// (a ticket always names one), and is still good.
fn sealed_user(token: &str, place: &str, owner: &str) -> Option<String> {
    let j: serde_json::Value = serde_json::from_str(&crate::session::unseal(USER_PURPOSE, token)?).ok()?;
    if j["p"].as_str()? != place || j["e"].as_u64()? <= now() || j["o"].as_str()? != owner {
        return None;
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

/// What `send` in a transaction is told, as it runs and by `quillo check` where it sees one.
pub const IN_A_TRANSACTION: &str = "`notification.send` waits on push services, which inside a transaction holds up every other request (a SQLite transaction keeps the interpreter's lock), and sends even when the transaction is rolled back: call it after the transaction";

/// `notification.send(user, { title, body, url, tag, badge, silent, ttl, urgency })`: to each of
/// `user`'s devices in this place. `{ sent, gone }`: how many push services took it, and how many
/// said the subscription had ended (dropped). Waits as `http.post` does, off the interpreter's
/// lock, sending to the devices at once; a message longer than one record takes is an error
/// value, and so is a send inside a transaction. Not in a component, which runs in the browser
/// too.
fn send(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    if it.in_component {
        return fail(line, "`notification.send` sends from the server; a component runs in the browser too: call it from a `server fn`");
    }
    // Inside one, `unlocked` would wait holding the lock, and the push services for every request
    // (interp.rs, `unlocked`; FEEDBACK67.md, B.3, review; branch pwa).
    if it.tx_depth > 0 || it.sqlite_tx > 0 || !it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_empty() {
        return Ok(Value::err(Value::str(IN_A_TRANSACTION)));
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
    let outcomes: Vec<(String, Outcome)> = it.unlocked(move || {
        let deadline = std::time::Instant::now() + ALL_SENDS;
        let done = in_parallel(&jobs, WORKERS, deadline, |(s, p), left| deliver(s, p, ttl, urgency, left), |(s, _)| Outcome::Failed(format!("{}: not sent: the {} s a send may take were up", host_of(&s.endpoint), ALL_SENDS.as_secs())));
        jobs.iter().map(|(s, _)| s.endpoint.clone()).zip(done).collect()
    });
    let (mut sent, mut gone) = (0i64, 0i64);
    let stamp = now_ms();
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

/// A person's devices are sent to at once, so many at a time, each given so long, and all of them
/// within so long: past it the rest are not sent, and said in the log (FEEDBACK67.md, B.3, review;
/// branch pwa).
const WORKERS: usize = 8;
const EACH_SEND: std::time::Duration = std::time::Duration::from_secs(10);
const ALL_SENDS: std::time::Duration = std::time::Duration::from_secs(30);

/// `run` of each job, `WORKERS` at a time, each given what is left until `deadline` (at most
/// `EACH_SEND`); `late` of each not started by then. The results in the jobs' order.
fn in_parallel<J: Sync, O: Send>(jobs: &[J], workers: usize, deadline: std::time::Instant, run: impl Fn(&J, std::time::Duration) -> O + Sync, late: impl Fn(&J) -> O + Sync) -> Vec<O> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out: Mutex<Vec<Option<O>>> = Mutex::new((0..jobs.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..workers.min(jobs.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(job) = jobs.get(i) else { break };
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                let r = if left < std::time::Duration::from_millis(100) { late(job) } else { run(job, left.min(EACH_SEND)) };
                out.lock().unwrap_or_else(std::sync::PoisonError::into_inner)[i] = Some(r);
            });
        }
    });
    out.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner).into_iter().map(|r| r.expect("each job ran or was late")).collect()
}

/// An endpoint as the log names it: its host alone, as its path is the subscription's secret.
fn host_of(endpoint: &str) -> String {
    url::Url::parse(endpoint).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| "the push service".into())
}

/// A push service's words for the log: control characters written as escapes (a line break or a
/// terminal's colours could make a line of its own), at most 200 characters.
fn for_the_log(text: &str) -> String {
    let cut: String = text.trim().chars().take(200).collect();
    let mut out = String::with_capacity(cut.len());
    for c in cut.chars() {
        if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    if text.trim().chars().count() > 200 {
        out.push('…');
    }
    out
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

/// One message to one subscription within `left`: encrypted, signed, posted, its answer read to the
/// end so that the connection is kept for the next (Q-960). What fails names the push service's
/// host, never the endpoint, whose path is the subscription's secret.
fn deliver(s: &Sub, payload: &str, ttl: u64, urgency: &str, left: std::time::Duration) -> Outcome {
    let host = host_of(&s.endpoint);
    let shown = url::Url::parse(&s.endpoint).map(|u| u.to_string()).unwrap_or_default();
    let failed = |e: String| {
        let said = e.replace(&s.endpoint, &host).replace(&shown, &host);
        let said = said.strip_prefix(&format!("{}: ", host)).unwrap_or(&said);
        Outcome::Failed(format!("{}: {}", host, for_the_log(said)))
    };
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
    let o = crate::fetch::Opts { public_only: https, timeout_ms: left.as_millis().clamp(1, 30_000) as u32, ..crate::fetch::DEFAULT_OPTS };
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
        _ => failed(format!("the push service answered {}{}", status, if rest.trim().is_empty() { String::new() } else { format!(": {}", rest) })),
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

/// Whether the program calls the module: `notification.ask`, `.state`, `.stop`, `.send` or `.user`
/// where `notification` is not a name of the program's own (a top-level one hides the module
/// everywhere, as Q-069 has it; a parameter, a `let`, a loop's or a match arm's where they are
/// bound). A loop over `notifications` naming each `notification` turned it all on: the endpoint,
/// the worker, the warning and the table (FEEDBACK67.md, B.3, review; branch pwa). Read once, at
/// load.
pub fn used_by(it: &Interp) -> bool {
    let module = matches!(crate::value::lookup(&it.globals, "notification"), Some(Value::Map(m)) if matches!(m.get("send"), Some(Value::Builtin(n, _)) if *n == "notification.send"));
    module && it.program.iter().any(|stmts| Calls::default().block(stmts))
}

/// The walk `used_by` makes, with the names bound around where it is: whether one of them is
/// `notification`.
#[derive(Default)]
struct Calls {
    scopes: Vec<bool>,
}

const NAME: &str = "notification";

impl Calls {
    fn hidden(&self) -> bool {
        self.scopes.iter().any(|b| *b)
    }

    fn within<T>(&mut self, binds: bool, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(binds);
        let r = f(self);
        self.scopes.pop();
        r
    }

    fn binds(p: &Pattern) -> bool {
        let mut names = Vec::new();
        crate::check::pattern_names(p, &mut names);
        names.iter().any(|n| n == NAME)
    }

    /// A block's statements, in a scope of their own: a `let` or a `state` binds for what follows.
    fn block(&mut self, stmts: &[Stmt]) -> bool {
        self.within(false, |w| {
            for st in stmts {
                let found = match st {
                    Stmt::Let(p, e, _) => w.pattern(p) || w.expr(e),
                    Stmt::State(_, e, _) | Stmt::Assign(_, _, e, _) | Stmt::Expr(e) | Stmt::Return(Some(e), _) => w.expr(e),
                    Stmt::SetField(o, _, _, e, _) => w.expr(o) || w.expr(e),
                    Stmt::Fn(d) => w.func(d),
                    Stmt::Route(r) => w.route(r),
                    Stmt::Service(_, _, _, Some(e), _, _) => w.expr(e),
                    Stmt::Type(td) => td.defaults.iter().any(|(_, e)| w.expr(e)),
                    Stmt::Use(..) | Stmt::Mount(..) | Stmt::Service(_, _, _, None, _, _) | Stmt::Return(None, _) => false,
                };
                if found {
                    return true;
                }
                let bound = match st {
                    Stmt::Let(p, ..) => Self::binds(p),
                    Stmt::State(n, ..) => n == NAME,
                    _ => false,
                };
                if bound {
                    *w.scopes.last_mut().expect("the block's own") = true;
                }
            }
            false
        })
    }

    fn func(&mut self, d: &FnDef) -> bool {
        let binds = d.params.iter().any(|p| p.name == NAME);
        self.within(binds, |w| d.params.iter().filter_map(|p| p.default.as_ref()).any(|e| w.expr(e)) || w.expr(&d.body))
    }

    fn route(&mut self, r: &RouteDef) -> bool {
        let pats: Vec<&Pattern> = r.query.iter().chain(r.body_pat.iter()).chain(r.guards.iter()).collect();
        let binds = r.path.iter().any(|s| matches!(s, Seg::Param(n, ..) | Seg::Rest(n) if n == NAME)) || pats.iter().any(|p| Self::binds(p));
        self.within(binds, |w| pats.iter().any(|p| w.pattern(p)) || w.expr(&r.body))
    }

    /// A pattern's defaults.
    fn pattern(&mut self, p: &Pattern) -> bool {
        match p {
            Pattern::Record(fields) => fields.iter().any(|f| f.default.as_ref().is_some_and(|d| self.expr(d)) || self.pattern(&f.pat)),
            Pattern::List(items, _) | Pattern::Alt(items) => items.iter().any(|q| self.pattern(q)),
            Pattern::As(q, _) => self.pattern(q),
            Pattern::Wild | Pattern::Bind(_) | Pattern::Lit(_) | Pattern::Type(_) => false,
        }
    }

    fn html(&mut self, el: &HtmlEl) -> bool {
        el.id.as_ref().is_some_and(|e| self.expr(e))
            || el.classes.iter().any(|e| self.expr(e))
            || el.attrs.iter().any(|(_, v)| match v {
                AttrVal::Expr(e) | AttrVal::Spread(e) => self.expr(e),
                AttrVal::Bool => false,
            })
            || el.children.iter().any(|c| match c {
                HtmlChild::El(e) => self.html(e),
                HtmlChild::Expr(e) => self.expr(e),
            })
    }

    fn expr(&mut self, e: &Expr) -> bool {
        match e {
            Expr::Call(callee, args, _) => {
                if let Expr::Field(obj, f, _) = &**callee {
                    if matches!(&**obj, Expr::Ident(n, _) if *n == NAME) && matches!(f.as_str(), "ask" | "state" | "stop" | "send" | "user") && !self.hidden() {
                        return true;
                    }
                }
                self.expr(callee) || args.iter().any(|a| self.expr(a))
            }
            Expr::Lit(_) | Expr::Ident(..) => false,
            Expr::Template(parts) => parts.iter().any(|p| matches!(p, TPart::Expr(x) if self.expr(x))),
            Expr::List(xs) => xs.iter().any(|x| self.expr(x)),
            Expr::Map(es) => es.iter().any(|(_, x)| self.expr(x)),
            Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => self.expr(x),
            Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) | Expr::While(a, b, _) => self.expr(a) || self.expr(b),
            Expr::If(c, a, b) => self.expr(c) || self.expr(a) || b.as_ref().is_some_and(|b| self.expr(b)),
            Expr::Match(s, arms, _) => {
                self.expr(s) || arms.iter().any(|a| self.within(Self::binds(&a.pat), |w| w.pattern(&a.pat) || a.guard.as_ref().is_some_and(|g| w.expr(g)) || w.expr(&a.body)))
            }
            Expr::For(p, i, b, _) => self.expr(i) || self.within(Self::binds(p), |w| w.pattern(p) || w.expr(b)),
            Expr::Block(stmts) => self.block(stmts),
            Expr::Lambda(d) => self.func(d),
            Expr::Reply(a, b, _) => self.expr(a) || b.as_ref().is_some_and(|b| self.expr(b)),
            Expr::Html(el, _) => self.html(el),
        }
    }
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


#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    // A person's devices at once, at most so many, each given what is left: those not started by
    // the deadline are late, in their place (FEEDBACK67.md, B.3, review; branch pwa).
    #[test]
    fn devices_are_sent_to_at_once_until_the_deadline() {
        let jobs: Vec<u64> = (0..10).collect();
        let t0 = Instant::now();
        let r = in_parallel(&jobs, 4, t0 + Duration::from_millis(450), |j, left| {
            assert!(left <= EACH_SEND && left > Duration::from_millis(100));
            std::thread::sleep(Duration::from_millis(200));
            format!("sent {}", j)
        }, |j| format!("late {}", j));
        let sent = r.iter().filter(|x| x.starts_with("sent")).count();
        assert_eq!(sent, 8, "{:?}", r);
        assert!(r[..8].iter().all(|x| x.starts_with("sent")) && r[8..] == ["late 8", "late 9"], "{:?}", r);
        assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
        assert!(in_parallel(&Vec::<u64>::new(), 8, Instant::now(), |_, _| 1, |_| 0).is_empty());
    }

    // What a push service says, as the log writes it: one line, its control characters escaped,
    // at most 200 characters.
    #[test]
    fn a_push_services_words_are_one_line_in_the_log() {
        assert_eq!(for_the_log("line1\n\u{1b}[31mFAKE quillo: injected\u{1b}[0m"), "line1\\n\\u{1b}[31mFAKE quillo: injected\\u{1b}[0m");
        let long = "é".repeat(300);
        assert_eq!(for_the_log(&long), format!("{}…", "é".repeat(200)));
        assert_eq!(host_of("https://fcm.googleapis.com/fcm/send/secret"), "fcm.googleapis.com");
        assert_eq!(host_of("nonsense"), "the push service");
    }

    // The endpoints taken: https on 443 to the public internet; plain http to this machine with
    // QUILLO_PUSH_LOCAL=1 alone; names of this machine with a final dot too.
    #[test]
    fn where_a_subscription_may_lead() {
        let ok = |e: &str| endpoint_ok(e).is_ok();
        assert!(ok("https://fcm.googleapis.com/fcm/send/x") && ok("https://web.push.apple.com:443/x"));
        assert!(!ok("https://example.com:22/x") && !ok("https://example.com:8443/x"));
        for e in ["https://localhost./x", "https://LOCALHOST/x", "https://a.localhost./x", "https://[::ffff:127.0.0.1]/x", "https://2130706433/x", "https://0x7f.1/x", "https://[fd00::1]/x", "https://[64:ff9b:1::a00:1]/x", "https://169.254.169.254/x", "https://[::1]/x"] {
            assert!(!ok(e), "{}", e);
        }
        // The tests run without QUILLO_PUSH_LOCAL.
        assert!(!ok("http://127.0.0.1:9/x") && !ok("http://localhost:9/x"));
    }
}
