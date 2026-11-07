//! `idempotent(f)`: a request that carries an `Idempotency-Key` header runs
//! once; the same request again with the same key gets the first answer,
//! without running again.
//!
//! ```text
//! route POST /payments { json: Payment } = idempotent(() => pay(request.json))
//! server fn transfer(t: Transfer) = idempotent({ isolation: "serializable", retries: 3 }, () => move(t))
//! ```
//!
//! It follows Stripe's idempotent requests
//! (https://docs.stripe.com/api/idempotent_requests) and the IETF draft
//! "The Idempotency-Key HTTP Header Field" (draft-ietf-httpapi-idempotency-key-header):
//!
//! - The client makes up the key (a UUID), and sends it again when it sends
//!   the request again: after a timeout, a dropped connection, a 409 or a
//!   5xx it did not see the end of.
//! - The answer kept is the status, the headers (not `Set-Cookie`) and the
//!   body. A request again gets it with `Idempotent-Replayed: true`.
//! - The same key with another request (method, path, query or body) is
//!   refused with 422, and while the first is running, a second gets 409:
//!   neither runs.
//! - For one key the work runs at most once, whoever asks. A key is one row, of a tenant and a
//!   mount point, claimed atomically on the key alone: who asks never chooses the row, it only
//!   decides who may see the answer. The row keeps who that is, as a set: what the first try
//!   said of itself as it began and, as the work may sign in, sign out or change the session,
//!   as it ended (see `says`). They are compared at the strongest level both sides have, `{ user }`,
//!   then the session, then credentials, then nobody, and only there (see `same_visitor`): the
//!   same replays, or waits while it runs. Different, or no level in common, gets 409 and nothing
//!   else, nothing run and nothing shown: a second client with the same key, or the same visitor
//!   after signing out or losing a cookie. Neither a second run nor another's receipt.
//! - `{ user }` is a namespace: two accounts may use one key, each a row of its own. A try under
//!   one also looks at the key's row with no `{ user }`, and that at those, so that `{ user:
//!   session().user }` going from null to signed in, or back, finds the first try.
//! - Answers are kept 24 hours (`{ max_age }`), as Stripe's are.
//!
//! It is a function rather than a word before `route`, for three reasons:
//! it runs where it is called, so the program's own checks (`auth.required`,
//! a pattern, a 400 for a bad form) come first and are not kept, as Stripe
//! keeps no answer for a request that failed validation; its options are
//! values computed for the request (the user a token names); and it takes
//! the transaction options `db.transaction` does, since it is one.
//!
//! **What is guaranteed.** The answers are kept in a table of Quillo's,
//! `_quillo_idempotency`, in the program's database (the tenant's, with a
//! database per tenant), and `f` runs in a transaction on that database.
//! The answer is written in that same transaction, so the work and its
//! answer commit together or not at all:
//!
//! - A key is first claimed, in a statement of its own, so that a second
//!   request sees it is running (409).
//! - `f` returns: its answer is written, the transaction commits, and the
//!   answer is sent. It fails, returns an error value or replies early:
//!   the transaction rolls back, the claim is given up, and the answer is
//!   sent but not kept. Nothing happened, so the client may try again, and
//!   it runs again. (To keep a refusal, return it: `respond(402, ...)`.)
//! - The server stops (or the database goes away) part way: the database
//!   rolls the transaction back, and the claim is left. Until its minute is
//!   up, every try gets 409. Then the next try by the same visitor takes it
//!   over and runs; anyone else gets 409, and the row stays until it expires.
//! - A claim is taken over only if it is still running; the answer is
//!   written only if the claim is still this request's. So if a request
//!   is slower than the minute, and another takes it over, at most one of
//!   them commits: the other rolls back (and gets the first's answer).
//!
//! Work outside that database (`http.post` to a payment provider, another
//! `db.connect`) is not part of the transaction. For such handlers,
//! `{ transaction: false }` runs `f` without one: every answer is kept,
//! failures too, as Stripe does, since part of the work may have happened.
//! A server that stops there leaves the key running: requests with it, from
//! anyone, get 409 until it expires, as it is not known whether the work
//! happened. It never runs again before that.
//!
//! Without the header, `f` simply runs (in its transaction), unless
//! `{ required: true }`, which answers 400. Outside a request (a script,
//! a job), there is no key: `f` runs.
//!
//! A plain form cannot send a header: `<form method="post" idempotent>` carries
//! a key in a hidden field instead (FEEDBACK24.md, Q-196; branch
//! feedback24-fixes). The browser shows the answer to the last of two clicks,
//! so that one waits for the first's answer rather than getting 409, and a
//! form edited and sent again is another request, not a 422.
//!
//! In a function a service answers with, the key is the call's own id, which
//! is the same for each try, so the value is kept in place of an answer
//! (FEEDBACK23.md, Q-188; branch feedback23-fixes). What would refuse a
//! request is a runtime error there: the call did not finish, and is sent
//! again.

use crate::interp::{fail, Ctrl, Interp};
use crate::server::HttpResponse;
use crate::value::*;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

type R = Result<Value, Ctrl>;

pub const BUILTINS: &[(&str, BuiltinFn)] = &[("idempotent", idempotent)];

/// The table the answers are kept in.
const TABLE: &str = "_quillo_idempotency";
/// How long a claim is the running request's; then, if its transaction
/// never committed, the next request with the key takes it over.
const LEASE_MS: i64 = 60_000;
/// How long answers are kept unless `{ max_age }` says: a day, as Stripe.
const KEEP_S: u64 = 86_400;
/// Keys are at most this long (Stripe's limit).
const MAX_KEY: usize = 255;
/// The field of a `<form idempotent>` its key comes in.
pub const FORM_FIELD: &str = "_idempotency_key";
/// How long a form sent again waits for the first to finish.
const FORM_WAIT_MS: i64 = 30_000;

struct Options {
    tx: crate::txn::Options,
    keep_ms: i64,
    user: Option<Value>,
    required: bool,
    transaction: bool,
}

fn options(v: Option<&Value>, line: u32) -> Result<Options, Ctrl> {
    let mut o = Options { tx: crate::txn::Options::default(), keep_ms: KEEP_S as i64 * 1000, user: None, required: false, transaction: true };
    let Some(v) = v else { return Ok(o) };
    let Value::Map(m) = v else {
        return fail(line, format!("`idempotent` expects options as a map, got {}", v.type_name()));
    };
    for k in m.keys() {
        if !matches!(&**k, "max_age" | "user" | "required" | "transaction" | "isolation" | "retries") {
            return fail(line, format!("`idempotent` has no option `{}` (max_age, user, required, transaction, isolation, retries)", k));
        }
    }
    o.tx = crate::txn::options(v, "idempotent", line)?;
    if let Some(a) = m.get("max_age") {
        match crate::builtins::duration_seconds(a) {
            Some(s) if s > 0 => o.keep_ms = (s.min(366 * 86_400) * 1000) as i64,
            _ => return fail(line, format!("`idempotent`'s max_age is seconds or a duration like \"24h\", got {}", a.repr())),
        }
    }
    o.user = m.get("user").cloned();
    // A namespace has one name: a record the work changes would be a new one, and the work would
    // run again (FEEDBACK55.md, Q-887; branch feedback55).
    if let Some(u) = o.user.as_ref().filter(|u| !matches!(u, Value::Nil | Value::Str(_) | Value::Int(_))) {
        return fail(line, user_not_a_name(u.type_name()));
    }
    o.required = matches!(m.get("required"), Some(Value::Bool(true)));
    match m.get("transaction") {
        None | Some(Value::Bool(true)) => {}
        Some(Value::Bool(false)) => {
            if o.tx.isolation.is_some() || o.tx.retries > 0 {
                return fail(line, "`idempotent` with { transaction: false } takes no isolation or retries");
            }
            o.transaction = false;
        }
        Some(other) => return fail(line, format!("`idempotent`'s transaction is true or false, got {}", other.repr())),
    }
    Ok(o)
}

/// What `{ user }` being anything but a name is told, here and by `quillo check`.
pub fn user_not_a_name(given: &str) -> String {
    format!("`idempotent`'s `user` says whose a key is, and is a str or an int, got {}: pass what does not change, its `.id`", given)
}

/// `idempotent(f)` or `idempotent(options, f)`.
fn idempotent(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (opts, f) = match args.as_slice() {
        [f @ (Value::Func(_) | Value::Builtin(..))] => (options(None, line)?, f.clone()),
        [o, f @ (Value::Func(_) | Value::Builtin(..))] => (options(Some(o), line)?, f.clone()),
        _ => return fail(line, "`idempotent` expects a function, like idempotent(() => pay(order)), with options before it if any"),
    };
    if it.tx_depth > 0 {
        return fail(line, "`idempotent` runs its function in a transaction of its own: call it outside `db.transaction`");
    }
    let run_plain = |it: &mut Interp| {
        if opts.transaction {
            crate::txn::run(it, None, &opts.tx, line, &mut |it| it.call(&f, Vec::new(), line))
        } else {
            it.call(&f, Vec::new(), line)
        }
    };
    let Some(Value::Map(req)) = it.request_value() else {
        return run_plain(it);
    };
    let text = |k: &str| match req.get(k) {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    };
    let header = |name: &str| match req.get("headers") {
        Some(Value::Map(h)) => h.get(name).map(|v| v.display()),
        _ => None,
    };
    let path = text("path");
    // A server fn called from a page answers as server fns do (see `server`).
    let rpc = path.strip_prefix("/_quillo/rpc/").map(str::to_string);
    let call = text("call");
    let step = !call.is_empty();
    let no = |status: u16, msg: &str| if step { fail(line, msg.to_string()) } else { refuse(status, msg) };
    let give = |a: HttpResponse, replayed: bool| if step { Ok(step_value(&a)) } else { answer(a, replayed) };
    let form_key = match req.get("form") {
        Some(Value::Map(f)) => f.get(FORM_FIELD).map(|v| v.display()),
        _ => None,
    };
    let mut by_form = false;
    let key = match (header("idempotency-key"), form_key) {
        (Some(k), _) => k,
        // By its line too: a function may have two.
        (None, _) if step => format!("{}:{}", call, line),
        (None, Some(k)) => {
            by_form = true;
            k
        }
        (None, None) if opts.required => return refuse(400, "this request needs an Idempotency-Key header"),
        (None, None) => return run_plain(it),
    };
    if key.is_empty() || key.len() > MAX_KEY || !key.bytes().all(|b| (0x21..0x7f).contains(&b)) {
        return refuse(400, &format!("an Idempotency-Key is 1 to {} visible ASCII characters", MAX_KEY));
    }
    let tenant = it.tenant();
    let print = fingerprint(it, &req, &header("content-type").unwrap_or_default());
    let form_print = if by_form { print.as_str() } else { "" };
    let named = opts.user.as_ref().filter(|u| !matches!(u, Value::Nil));
    let base = it.base.clone();
    let row_of = |owner: &str| sha256_hex(&[tenant.as_deref().unwrap_or("").as_bytes(), base.as_bytes(), owner.as_bytes(), key.as_bytes(), form_print.as_bytes()]);
    let bare = row_of("null");
    let id = named.map_or_else(|| bare.clone(), |u| row_of(&crate::interp::island_json(u).to_string()));
    // Rows of before the key's row was the key's: they went by `session().user`. A retry that
    // spans the upgrade finds its first try there.
    let old_user = crate::session::old_user(it);
    let old = old_user.as_ref().filter(|_| named.is_none()).map(|u| row_of(&crate::interp::island_json(u).to_string()));
    let mut owners = says(named, &header, crate::session::arrived_owner(it));
    if owners.is_empty() {
        owners.push(NOBODY.to_string());
    }
    let attempt = Attempt { id: &id, bare: &bare, old: old.as_deref(), path: &path, print: &print, keep_ms: opts.keep_ms, take_over: opts.transaction, owners: &owners };
    let mut claimed = take(it, &attempt);
    let until = now_ms() + FORM_WAIT_MS;
    while by_form && matches!(claimed, Ok(Claim::Busy)) && now_ms() < until {
        it.unlocked(|| std::thread::sleep(std::time::Duration::from_millis(50)));
        claimed = take(it, &attempt);
    }
    let (token, mut owners) = match claimed {
        Ok(Claim::Mine(t, known)) => (t, known),
        Ok(Claim::Answer(a)) => return give(a, true),
        Ok(Claim::Busy) => return no(409, RUNNING),
        Ok(Claim::Refused(status, msg)) => return no(status, &msg),
        Err(e) => return no(503, &format!("could not record the Idempotency-Key: {}", e)),
    };
    cleanup(it, tenant.as_deref());

    if !opts.transaction {
        let r = it.call(&f, Vec::new(), line);
        let a = match if step { render_step(r) } else { render(rpc.as_deref(), r) } {
            Ok(a) => a,
            Err(e) => {
                release(it, &id, &token);
                return Err(e);
            }
        };
        let after = says(named, &header, crate::session::current_owner(it));
        add_owners(&mut owners, after);
        match store(it, &id, &token, &a, &owners) {
            Ok(true) => {}
            Ok(false) => eprintln!("quillo: an idempotent request's claim was gone when its answer was kept"),
            Err(e) => eprintln!("quillo: could not keep an idempotent request's answer: {}", e),
        }
        return give(a, false);
    }

    // The work and its answer, in one transaction.
    let mut kept: Option<HttpResponse> = None;
    let mut lost = false;
    let ok = |r: &R| matches!(r, Ok(v) if !matches!(v, Value::Err(..)));
    let r = crate::txn::run(it, None, &opts.tx, line, &mut |it| {
        kept = None;
        lost = false;
        let v = match it.call(&f, Vec::new(), line) {
            Ok(v) if !matches!(v, Value::Err(..)) => v,
            other => return other,
        };
        let a = if step { render_step(Ok(v.clone()))? } else { render(rpc.as_deref(), Ok(v.clone()))? };
        let mut all = owners.clone();
        add_owners(&mut all, says(named, &header, crate::session::current_owner(it)));
        match store(it, &id, &token, &a, &all) {
            Ok(true) => {
                kept = Some(a);
                Ok(v)
            }
            // Taken over by another request: this one must not commit.
            Ok(false) => {
                lost = true;
                Ok(Value::err(Value::str("idempotent: the request was taken over")))
            }
            Err(e) => Ok(Value::err(Value::str(format!("db: {}", e)))),
        }
    });
    match kept {
        Some(a) if ok(&r) => give(a, false),
        _ => {
            // Nothing was committed: give the claim up, so the request may
            // run again.
            release(it, &id, &token);
            if lost {
                return match claim(it, &attempt) {
                    Ok(Claim::Answer(a)) => give(a, true),
                    Ok(Claim::Mine(t, _)) => {
                        release(it, &id, &t);
                        no(409, "a request with this Idempotency-Key was running at the same time; try again")
                    }
                    Ok(Claim::Busy) => no(409, RUNNING),
                    Ok(Claim::Refused(status, msg)) => no(status, &msg),
                    Err(e) => no(503, &format!("could not read the Idempotency-Key: {}", e)),
                };
            }
            r
        }
    }
}

/// Who a row is when nothing was said of who asked.
const NOBODY: &str = "nobody";

/// The headers that say who a client is. A plain cookie of the program's is not one of them:
/// `{ user }` says so.
const CREDENTIALS: &[&str] = &["authorization", "proxy-authorization", "x-api-key", "api-key", "x-auth-token", "x-access-token"];

/// Whether a credential header says nothing: empty, or what a page sent for a token it did not
/// have, as the whole value or after the scheme (`Bearer undefined`). Such a header is everyone's.
fn no_credential(v: &str) -> bool {
    let words: Vec<&str> = v.split_whitespace().collect();
    let scheme_alone = |w: &str| ["bearer", "basic", "token"].iter().any(|s| w.eq_ignore_ascii_case(s));
    match words.as_slice() {
        [] => true,
        [only] if scheme_alone(only) => true,
        [.., last] => ["undefined", "null"].iter().any(|n| last.eq_ignore_ascii_case(n)),
    }
}

/// What a try says of who it is, strongest first: the program's `{ user }`, the session
/// (`session::arrived_owner`, `current_owner`), each credential header apart, as one more on a
/// retry must still match. These only say who may see the answer, and none of them chooses the
/// row: a claim filed under one of several owners could not be found by a retry presenting
/// another, and each way of doing that double-charged (FEEDBACK55.md, Q-887; branch feedback55).
/// `nobody` is said by a try with none of these as it begins, and never at the end of the work:
/// a work that ends the session is no one's to replay to a stranger.
fn says(named: Option<&Value>, header: &dyn Fn(&str) -> Option<String>, session: Option<String>) -> Vec<String> {
    let mark = |level: &str, kind: &str, value: &str| format!("{}{}", level, sha256_hex(&[kind.as_bytes(), value.as_bytes()]));
    let mut out = Vec::new();
    if let Some(u) = named {
        out.push(mark(LEVELS[0], "user", &crate::interp::island_json(u).to_string()));
    }
    if let Some(s) = session {
        out.push(mark(LEVELS[1], "session", &s));
    }
    for h in CREDENTIALS {
        if let Some(v) = header(h).filter(|v| !no_credential(v)) {
            out.push(mark(LEVELS[2], h, &v));
        }
    }
    out
}

/// The levels an owner is at, strongest first.
const LEVELS: [&str; 4] = ["user:", "session:", "credential:", NOBODY];

/// Whether two sets of owners are one visitor: at the strongest level both have, and only there.
/// A public `X-Api-Key` two visitors share says nothing when each has a session. No level in
/// common is no one in common.
fn same_visitor(a: &[String], b: &[String]) -> bool {
    for level in LEVELS {
        let (x, y): (Vec<&String>, Vec<&String>) = (a.iter().filter(|o| o.starts_with(level)).collect(), b.iter().filter(|o| o.starts_with(level)).collect());
        if !x.is_empty() && !y.is_empty() {
            return x.iter().any(|o| y.contains(o));
        }
    }
    false
}

fn add_owners(owners: &mut Vec<String>, more: Vec<String>) {
    for o in more {
        if !owners.contains(&o) {
            owners.push(o);
        }
    }
}

pub fn sha256_hex(parts: &[&[u8]]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update((p.len() as u64).to_be_bytes());
        h.update(p);
    }
    crate::crypto::hex(&h.finalize())
}

/// What makes two requests the same one: method, path, query and body. A
/// multipart body is made with a boundary of its own each time it is sent,
/// so the boundary is left out.
fn fingerprint(it: &Interp, req: &Map, ctype: &str) -> String {
    let text = |k: &str| req.get(k).map_or(String::new(), |v| v.display());
    let mut query: Vec<(String, String)> = match req.get("query") {
        Some(Value::Map(q)) => q.iter().map(|(k, v)| (k.to_string(), v.display())).collect(),
        _ => Vec::new(),
    };
    query.sort();
    let query = query.into_iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<_>>().join("&");
    let body: Vec<u8> = match (crate::upload::boundary(ctype), &it.upload_body) {
        (Some(b), Some(bytes)) => without(bytes, b.as_bytes()),
        _ => text("body").into_bytes(),
    };
    sha256_hex(&[text("method").as_bytes(), text("path").as_bytes(), query.as_bytes(), &body])
}

/// `bytes` with every `cut` taken out.
fn without(bytes: &[u8], cut: &[u8]) -> Vec<u8> {
    if cut.is_empty() {
        return bytes.to_vec();
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(cut) {
            i += cut.len();
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// The HTTP answer for what `f` gave, as the route or server fn would
/// have sent it.
fn render(rpc: Option<&str>, r: R) -> Result<HttpResponse, Ctrl> {
    let streamed = |r: &Response| matches!(r.body, Body::Stream { .. });
    match &r {
        Ok(Value::Response(x)) | Err(Ctrl::Reply(x)) | Err(Ctrl::Return(Value::Response(x))) if streamed(x) => {
            return fail(0, "`idempotent` cannot keep a streamed answer");
        }
        _ => {}
    }
    let Some(name) = rpc else { return Ok(crate::server::finish(r)) };
    let json = |status: u16, j: serde_json::Value| HttpResponse {
        status,
        headers: vec![("content-type".into(), "application/json".into())],
        body: j.to_string().into_bytes(),
    };
    Ok(match r {
        Ok(v) | Err(Ctrl::Return(v)) => match crate::server::for_visitor(v) {
            v @ Value::Response(_) => crate::server::finish(Ok(v)),
            Value::Err(e, _, _) => json(200, serde_json::json!({ "err": crate::interp::island_json(&e) })),
            v => json(200, serde_json::json!({ "ok": crate::interp::island_json(&v) })),
        },
        Err(Ctrl::Reply(x)) => crate::server::finish(Err(Ctrl::Reply(x))),
        Err(Ctrl::Error(e)) => {
            eprintln!("quillo: error in server fn `{}`: {:#}", name, e);
            json(500, serde_json::json!({ "error": format!("internal error in `{}`", name) }))
        }
    })
}

/// What a service's function gave, kept as its value and not as a page.
fn render_step(r: R) -> Result<HttpResponse, Ctrl> {
    let v = match r {
        Ok(v) | Err(Ctrl::Return(v)) => v,
        Err(e) => return Err(e),
    };
    if matches!(v, Value::Response(_)) {
        return fail(0, "a service's function gives a value, not a page");
    }
    let body = crate::service::wire_answer(&v).to_string().into_bytes();
    Ok(HttpResponse { status: 200, headers: vec![("content-type".into(), "application/json".into())], body })
}

/// The value kept by `render_step`.
fn step_value(a: &HttpResponse) -> Value {
    // As deep as every reader takes (FEEDBACK55.md, Q-875's second review; branch feedback55).
    crate::service::from_answer(&crate::value::json_value(&a.body).unwrap_or_default())
}

/// Ends the request with an answer (kept, or given again).
fn answer(a: HttpResponse, replayed: bool) -> R {
    let mut headers = a.headers;
    if replayed {
        headers.push(("idempotent-replayed".into(), "true".into()));
    }
    Err(Ctrl::Reply(Arc::new(Response { status: a.status, headers, body: Body::Bytes(a.body) })))
}

fn refuse(status: u16, msg: &str) -> R {
    let body = serde_json::json!({ "error": msg }).to_string().into_bytes();
    Err(Ctrl::Reply(Arc::new(Response { status, headers: vec![("content-type".into(), "application/json".into())], body: Body::Bytes(body) })))
}

// ---- the table ----------------------------------------------------------------------------

enum Claim {
    /// This request runs: the claim's token, and who may see the answer so far.
    Mine(String, Vec<String>),
    /// The first request's answer.
    Answer(HttpResponse),
    /// The first request is still running, and this one may wait for it.
    Busy,
    Refused(u16, String),
}

const RUNNING: &str = "a request with this Idempotency-Key is still running; try again shortly";
/// The same for a row done or running: nothing is said of what is there.
const SOMEONE_ELSES: &str = "this Idempotency-Key is someone else's: nothing was run; use a key of your own, or say whose it is with `{ user }`";

/// A try of `idempotent`, as far as the table goes.
struct Attempt<'a> {
    /// Its row, and the key's row with no `{ user }`, which is the same one when it gave none.
    id: &'a str,
    bare: &'a str,
    /// A row of before there were owners, by `session().user`.
    old: Option<&'a str>,
    path: &'a str,
    print: &'a str,
    keep_ms: i64,
    /// A claim past its lease may be taken: the work is in a transaction of its own.
    take_over: bool,
    owners: &'a [String],
}

/// What a row means to a try.
enum Verdict {
    /// Past `expires_at`: there is no such key.
    Expired,
    Final(Claim),
    /// Past its lease, in a transaction: the work rolled back, or cannot commit once taken over.
    Stale,
}

/// The owners a row keeps. Only a row from before there were owners has none.
fn owners_of(row: &Map) -> Option<Vec<String>> {
    serde_json::from_str(&row_text(row, "owners")).ok()
}

fn judge(row: &Map, a: &Attempt) -> Verdict {
    let now = now_ms();
    if row_int(row, "expires_at") <= now {
        return Verdict::Expired;
    }
    let ours = owners_of(row).map_or(true, |o| same_visitor(&o, a.owners));
    let someone_elses = || Verdict::Final(Claim::Refused(409, SOMEONE_ELSES.into()));
    let same = row_text(row, "fingerprint") == a.print;
    let other = || Verdict::Final(Claim::Refused(422, "this Idempotency-Key was used with another request".into()));
    let done = row_int(row, "done") == 1;
    if done || row_int(row, "lease_until") > now {
        return match (ours, same) {
            (false, _) => someone_elses(),
            (true, false) => other(),
            _ if done => Verdict::Final(Claim::Answer(row_answer(row))),
            _ => Verdict::Final(Claim::Busy),
        };
    }
    // Past its lease it is no more absent for one than for another, and a stranger takes nothing
    // over: the row stays until it expires.
    if !ours {
        return someone_elses();
    }
    if !same {
        return other();
    }
    if !a.take_over {
        return Verdict::Final(Claim::Refused(409, "the first request with this Idempotency-Key stopped before it finished; whether it took effect is not known".into()));
    }
    Verdict::Stale
}

/// Takes the key for this try, or says why not. The claim comes first, and then the key's rows
/// under other names are looked for: a claim there cannot then slip in between. Of two at once
/// each sees the other and gives up, and the work runs once, on the next try. Reads come after
/// the claim's write, so that they are the primary's, never a replica's, which may not have a
/// first try yet (`Interp::wrote_req`; FEEDBACK55.md, Q-887; branch feedback55).
fn take(it: &mut Interp, a: &Attempt) -> Result<Claim, String> {
    let c = claim(it, a)?;
    if let Claim::Mine(token, _) = &c {
        if let Some(found) = elsewhere(it, a)? {
            release(it, a.id, token);
            return Ok(found);
        }
    }
    Ok(c)
}

const ROW: &str = "select id, fingerprint, token, done, lease_until, expires_at, status, headers, body, body_b64, owners from _quillo_idempotency";

/// The key's rows under other names: of a `{ user }`, the row with none; of none, each `{ user }`'s
/// (see the module's notes); and a row from before owners.
fn elsewhere(it: &mut Interp, a: &Attempt) -> Result<Option<Claim>, String> {
    let (text, p) = if a.id == a.bare {
        (format!("{} where bare = ? and id <> ?", ROW), vec![Value::str(a.bare), Value::str(a.id)])
    } else {
        (format!("{} where id = ?", ROW), vec![Value::str(a.bare)])
    };
    let mut found = Vec::new();
    if let Value::List(rows) = sql(it, true, &text, p)? {
        found.extend(rows.iter().cloned());
    }
    if let Some(old) = a.old {
        if let Value::List(rows) = sql(it, true, &format!("{} where id = ? and owners is null", ROW), vec![Value::str(old)])? {
            found.extend(rows.iter().cloned());
        }
    }
    for row in &found {
        let Value::Map(row) = row else { continue };
        match judge(row, a) {
            Verdict::Final(c) => return Ok(Some(c)),
            // Its work rolled back, or cannot commit without its row.
            Verdict::Expired | Verdict::Stale => {
                let p = vec![Value::str(row_text(row, "id")), Value::str(row_text(row, "token"))];
                sql(it, false, "delete from _quillo_idempotency where id = ? and token = ? and done = 0", p)?;
            }
        }
    }
    Ok(None)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn make_table(it: &mut Interp) -> Value {
    let sql = if crate::db::kind_on(it, None) {
        "create table if not exists _quillo_idempotency (id varchar(64) primary key, path varchar(1024) not null, \
         fingerprint varchar(64) not null, token varchar(32) not null, done int not null default 0, \
         lease_until bigint not null, expires_at bigint not null, status int, headers text, body longtext, body_b64 int, \
         index (expires_at))"
    } else {
        "create table if not exists _quillo_idempotency (id varchar(64) primary key, path text not null, \
         fingerprint varchar(64) not null, token varchar(32) not null, done int not null default 0, \
         lease_until bigint not null, expires_at bigint not null, status int, headers text, body text, body_b64 int);\n\
         create index if not exists _quillo_idempotency_expires on _quillo_idempotency (expires_at)"
    };
    let made = crate::db::exec_on(it, None, vec![Value::str(sql)]).unwrap_or_else(|_| Value::Nil);
    // Who may see the answer, and the key's row with no `{ user }`: on a table of an older Quillo
    // too. One there already says so, and that is all.
    for add in [
        "alter table _quillo_idempotency add column owners text",
        "alter table _quillo_idempotency add column bare varchar(64)",
        "create index _quillo_idempotency_bare on _quillo_idempotency (bare)",
    ] {
        let _ = crate::db::exec_on(it, None, vec![Value::str(add)]);
    }
    made
}

/// A statement on the program's database: its result, or the database's
/// message. (The table is made the first time it is missing.)
fn sql(it: &mut Interp, query: bool, sql: &str, params: Vec<Value>) -> Result<Value, String> {
    let mut args = vec![Value::str(sql)];
    args.extend(params);
    for first in [true, false] {
        let r = if query { crate::db::query_on(it, None, args.clone()) } else { crate::db::exec_on(it, None, args.clone()) };
        match r {
            Ok(Value::Err(e, _, _)) => {
                let msg = e.display();
                if first && (msg.contains(TABLE) || msg.contains("owners") || msg.contains("bare")) {
                    make_table(it);
                    continue;
                }
                return Err(msg);
            }
            Ok(v) => return Ok(v),
            Err(_) => return Err("the database failed".into()),
        }
    }
    Err("no table for idempotency keys".into())
}

fn changes(v: &Value) -> i64 {
    match v {
        Value::Map(m) => match m.get("changes") {
            Some(Value::Int(n)) => *n,
            _ => 0,
        },
        _ => 0,
    }
}

/// Claims the try's row, on the key alone: its owners go along, and decide nothing here but who
/// sees what is found (see `judge`).
fn claim(it: &mut Interp, a: &Attempt) -> Result<Claim, String> {
    let columns = "(id, path, fingerprint, token, lease_until, expires_at, owners, bare) values (?, ?, ?, ?, ?, ?, ?, ?)";
    let insert = if crate::db::kind_on(it, None) {
        format!("insert ignore into _quillo_idempotency {}", columns)
    } else {
        format!("insert into _quillo_idempotency {} on conflict (id) do nothing", columns)
    };
    let path: String = a.path.chars().take(1000).collect();
    let owners = serde_json::to_string(a.owners).unwrap_or_else(|_| "[]".into());
    for _ in 0..4 {
        let now = now_ms();
        let token = crate::crypto::hex(&crate::crypto::secret_bytes(16));
        let p = vec![Value::str(a.id), Value::str(&path), Value::str(a.print), Value::str(&token), Value::Int(now + LEASE_MS), Value::Int(now + a.keep_ms), Value::str(&owners), Value::str(a.bare)];
        if changes(&sql(it, false, &insert, p)?) == 1 {
            return Ok(Claim::Mine(token, a.owners.to_vec()));
        }
        // Someone has the key.
        let Value::List(rows) = sql(it, true, &format!("{} where id = ?", ROW), vec![Value::str(a.id)])? else { continue };
        let Some(Value::Map(row)) = rows.first() else { continue };
        let theirs = row_text(row, "token");
        match judge(row, a) {
            Verdict::Final(c) => return Ok(c),
            Verdict::Expired => {
                sql(it, false, "delete from _quillo_idempotency where id = ? and token = ?", vec![Value::str(a.id), Value::str(&theirs)])?;
            }
            // Its server stopped part way, and its transaction rolled back. A run that is in fact
            // still going finds its row taken and cannot commit, so at most one does.
            Verdict::Stale => {
                let mut known = owners_of(row).unwrap_or_default();
                add_owners(&mut known, a.owners.to_vec());
                let text = serde_json::to_string(&known).unwrap_or_else(|_| "[]".into());
                let p = vec![Value::str(&token), Value::Int(now + LEASE_MS), Value::str(text), Value::str(a.id), Value::str(&theirs)];
                if changes(&sql(it, false, "update _quillo_idempotency set token = ?, lease_until = ?, owners = ? where id = ? and token = ? and done = 0", p)?) == 1 {
                    return Ok(Claim::Mine(token, known));
                }
            }
        }
    }
    Ok(Claim::Busy)
}

fn row_int(row: &Map, k: &str) -> i64 {
    match row.get(k) {
        Some(Value::Int(n)) => *n,
        Some(Value::Bool(b)) => *b as i64,
        Some(Value::Str(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn row_text(row: &Map, k: &str) -> String {
    row.get(k).map_or(String::new(), |v| if matches!(v, Value::Nil) { String::new() } else { v.display() })
}

fn row_answer(row: &Map) -> HttpResponse {
    let headers: Vec<(String, String)> = serde_json::from_str(&row_text(row, "headers")).unwrap_or_default();
    let body = row_text(row, "body");
    let body = if row_int(row, "body_b64") == 1 { crate::crypto::unb64url(&body).unwrap_or_default() } else { body.into_bytes() };
    HttpResponse { status: row_int(row, "status") as u16, headers, body }
}

/// Keeps the answer, if the claim is still this request's: Ok(false) if not.
fn store(it: &mut Interp, id: &str, token: &str, a: &HttpResponse, owners: &[String]) -> Result<bool, String> {
    let headers: Vec<&(String, String)> = a.headers.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("set-cookie")).collect();
    let headers = serde_json::to_string(&headers).unwrap_or_else(|_| "[]".into());
    let (body, b64) = match std::str::from_utf8(&a.body) {
        Ok(s) => (s.to_string(), 0),
        Err(_) => (crate::crypto::b64url(&a.body), 1),
    };
    let owners = serde_json::to_string(owners).unwrap_or_else(|_| "[]".into());
    let p = vec![Value::Int(a.status as i64), Value::str(headers), Value::str(body), Value::Int(b64), Value::str(owners), Value::str(id), Value::str(token)];
    let r = sql(it, false, "update _quillo_idempotency set done = 1, status = ?, headers = ?, body = ?, body_b64 = ?, owners = ? where id = ? and token = ? and done = 0", p)?;
    Ok(changes(&r) == 1)
}

/// Gives a claim up (never a kept answer).
fn release(it: &mut Interp, id: &str, token: &str) {
    if let Err(e) = sql(it, false, "delete from _quillo_idempotency where id = ? and token = ? and done = 0", vec![Value::str(id), Value::str(token)]) {
        eprintln!("quillo: could not give up an Idempotency-Key's claim ({}); it is taken over in a minute", e);
    }
}

/// Deletes expired answers: from each tenant's table at most once a minute.
fn cleanup(it: &mut Interp, tenant: Option<&str>) {
    static LAST: Mutex<Option<std::collections::HashMap<String, i64>>> = Mutex::new(None);
    let now = now_ms();
    let k = format!("{}\0{}", it.base, tenant.unwrap_or(""));
    {
        let mut g = LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let last = g.get_or_insert_with(Default::default);
        if last.get(&k).map_or(false, |t| now - t < 60_000) {
            return;
        }
        last.insert(k, now);
    }
    let _ = sql(it, false, "delete from _quillo_idempotency where expires_at < ?", vec![Value::Int(now)]);
}

/// Whether a server fn uses `idempotent` (the name, not a text or a key that says it: found in
/// review of FEEDBACK49.md, Q-595), or a function it calls does
/// (FEEDBACK24.md, Q-196; branch feedback24-fixes): then the page sends a key
/// with each call, and sends it again with the call if the answer does not come
/// (see `rpcOnce` in the browser runtime).
pub fn used_by(globals: &Env, def: &crate::ast::FnDef) -> bool {
    reaches(globals, def, &mut std::collections::HashSet::new())
}

fn reaches(globals: &Env, def: &crate::ast::FnDef, seen: &mut std::collections::HashSet<String>) -> bool {
    let body = format!("{:?}", def.body);
    if body.contains("Ident(\"idempotent\"") {
        return true;
    }
    body.split("Ident(\"").skip(1).filter_map(|rest| rest.split_once('"')).any(|(name, _)| {
        if !seen.insert(name.to_string()) {
            return false;
        }
        matches!(lookup(globals, name), Some(Value::Func(c)) if reaches(globals, &c.def, seen))
    })
}

/// `used_by` over a file's own definitions, as a caller reads its copy of a service's file:
/// whether calling `def` twice is safe (FEEDBACK49.md, Q-595; branch main).
pub fn used_by_defs(defs: &std::collections::HashMap<String, std::sync::Arc<crate::ast::FnDef>>, def: &crate::ast::FnDef) -> bool {
    fn go(defs: &std::collections::HashMap<String, std::sync::Arc<crate::ast::FnDef>>, def: &crate::ast::FnDef, seen: &mut std::collections::HashSet<String>) -> bool {
        let body = format!("{:?}", def.body);
        if body.contains("Ident(\"idempotent\"") {
            return true;
        }
        body.split("Ident(\"").skip(1).filter_map(|rest| rest.split_once('"')).any(|(name, _)| seen.insert(name.to_string()) && defs.get(name).is_some_and(|d| go(defs, d, seen)))
    }
    go(defs, def, &mut std::collections::HashSet::new())
}

/// Whether an answer holds such a key: the page cannot be kept for anyone,
/// the same visitor later included, or two payments would be one.
pub fn holds_form_key(body: &[u8]) -> bool {
    let name = FORM_FIELD.as_bytes();
    body.windows(name.len()).any(|w| w == name)
}
