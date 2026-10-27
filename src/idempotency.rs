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
//! - Keys belong to a tenant, a mount point and a user (the session's
//!   `user`, or `{ user }`), so one user cannot read another's answer.
//!   They are kept 24 hours (`{ max_age }`), as Stripe's are.
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
//!   rolls the transaction back, and the claim is left. After a minute it
//!   is taken over by the next request with the key, which runs.
//! - A claim is taken over only if it is still running; the answer is
//!   written only if the claim is still this request's. So if a request
//!   is slower than the minute, and another takes it over, at most one of
//!   them commits: the other rolls back (and gets the first's answer).
//!
//! Work outside that database (`http.post` to a payment provider, another
//! `db.connect`) is not part of the transaction. For such handlers,
//! `{ transaction: false }` runs `f` without one: every answer is kept,
//! failures too, as Stripe does, since part of the work may have happened.
//! A server that stops there leaves the key running: requests with it get
//! 409 until it expires, as it is not known whether the work happened.
//!
//! Without the header, `f` simply runs (in its transaction), unless
//! `{ required: true }`, which answers 400. Outside a request (a script,
//! a job), there is no key: `f` runs.

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
    let key = match header("idempotency-key") {
        Some(k) => k,
        None if opts.required => return refuse(400, "this request needs an Idempotency-Key header"),
        None => return run_plain(it),
    };
    if key.is_empty() || key.len() > MAX_KEY || !key.bytes().all(|b| (0x21..0x7f).contains(&b)) {
        return refuse(400, &format!("an Idempotency-Key is 1 to {} visible ASCII characters", MAX_KEY));
    }
    let user = match &opts.user {
        Some(u) => u.clone(),
        None => session_user(it, line)?,
    };
    let tenant = it.tenant();
    let id = sha256_hex(&[tenant.as_deref().unwrap_or("").as_bytes(), it.base.as_bytes(), crate::interp::island_json(&user).to_string().as_bytes(), key.as_bytes()]);
    let print = fingerprint(it, &req, &header("content-type").unwrap_or_default());
    let token = match claim(it, &id, &path, &print, opts.keep_ms, opts.transaction) {
        Ok(Claim::Mine(t)) => t,
        Ok(Claim::Answer(a)) => return answer(a, true),
        Ok(Claim::Refused(status, msg)) => return refuse(status, &msg),
        Err(e) => return refuse(503, &format!("could not record the Idempotency-Key: {}", e)),
    };
    cleanup(it, tenant.as_deref());

    if !opts.transaction {
        let r = it.call(&f, Vec::new(), line);
        let a = match render(rpc.as_deref(), r) {
            Ok(a) => a,
            Err(e) => {
                release(it, &id, &token);
                return Err(e);
            }
        };
        match store(it, &id, &token, &a) {
            Ok(true) => {}
            Ok(false) => eprintln!("quillo: an idempotent request's claim was gone when its answer was kept"),
            Err(e) => eprintln!("quillo: could not keep an idempotent request's answer: {}", e),
        }
        return answer(a, false);
    }

    // The work and its answer, in one transaction.
    let mut kept: Option<HttpResponse> = None;
    let mut lost = false;
    let ok = |r: &R| matches!(r, Ok(v) if !matches!(v, Value::Err(_)));
    let r = crate::txn::run(it, None, &opts.tx, line, &mut |it| {
        kept = None;
        lost = false;
        let v = match it.call(&f, Vec::new(), line) {
            Ok(v) if !matches!(v, Value::Err(_)) => v,
            other => return other,
        };
        let a = render(rpc.as_deref(), Ok(v.clone()))?;
        match store(it, &id, &token, &a) {
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
        Some(a) if ok(&r) => answer(a, false),
        _ => {
            // Nothing was committed: give the claim up, so the request may
            // run again.
            release(it, &id, &token);
            if lost {
                return match claim(it, &id, &path, &print, opts.keep_ms, true) {
                    Ok(Claim::Answer(a)) => answer(a, true),
                    Ok(Claim::Mine(t)) => {
                        release(it, &id, &t);
                        refuse(409, "a request with this Idempotency-Key was running at the same time; try again")
                    }
                    Ok(Claim::Refused(status, msg)) => refuse(status, &msg),
                    Err(e) => refuse(503, &format!("could not read the Idempotency-Key: {}", e)),
                };
            }
            r
        }
    }
}

/// `session().user`, through the `session` builtin.
fn session_user(it: &mut Interp, line: u32) -> R {
    let Some((_, f)) = crate::session::BUILTINS.iter().find(|(n, _)| *n == "session") else { return Ok(Value::Nil) };
    match f(it, Vec::new(), line)? {
        Value::Map(m) => Ok(m.get("user").cloned().unwrap_or(Value::Nil)),
        _ => Ok(Value::Nil),
    }
}

fn sha256_hex(parts: &[&[u8]]) -> String {
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
        Ok(v) | Err(Ctrl::Return(v)) => match v {
            Value::Response(_) => crate::server::finish(Ok(v)),
            Value::Err(e) => json(200, serde_json::json!({ "err": crate::interp::island_json(&e) })),
            v => json(200, serde_json::json!({ "ok": crate::interp::island_json(&v) })),
        },
        Err(Ctrl::Reply(x)) => crate::server::finish(Err(Ctrl::Reply(x))),
        Err(Ctrl::Error(e)) => {
            eprintln!("quillo: error in server fn `{}`: {}", name, e);
            json(500, serde_json::json!({ "error": format!("internal error in `{}`", name) }))
        }
    })
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
    /// This request runs; the claim's token.
    Mine(String),
    /// The first request's answer.
    Answer(HttpResponse),
    Refused(u16, String),
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
    crate::db::exec_on(it, None, vec![Value::str(sql)]).unwrap_or_else(|_| Value::Nil)
}

/// A statement on the program's database: its result, or the database's
/// message. (The table is made the first time it is missing.)
fn sql(it: &mut Interp, query: bool, sql: &str, params: Vec<Value>) -> Result<Value, String> {
    let mut args = vec![Value::str(sql)];
    args.extend(params);
    for first in [true, false] {
        let r = if query { crate::db::query_on(it, None, args.clone()) } else { crate::db::exec_on(it, None, args.clone()) };
        match r {
            Ok(Value::Err(e)) => {
                let msg = e.display();
                if first && msg.contains(TABLE) {
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

/// Claims key `id` for this request, or says why not.
fn claim(it: &mut Interp, id: &str, path: &str, print: &str, keep_ms: i64, take_over: bool) -> Result<Claim, String> {
    let insert = if crate::db::kind_on(it, None) {
        "insert ignore into _quillo_idempotency (id, path, fingerprint, token, lease_until, expires_at) values (?, ?, ?, ?, ?, ?)"
    } else {
        "insert into _quillo_idempotency (id, path, fingerprint, token, lease_until, expires_at) values (?, ?, ?, ?, ?, ?) on conflict (id) do nothing"
    };
    let path: String = path.chars().take(1000).collect();
    for _ in 0..4 {
        let now = now_ms();
        let token = crate::crypto::hex(&crate::crypto::secret_bytes(16));
        let p = vec![Value::str(id), Value::str(&path), Value::str(print), Value::str(&token), Value::Int(now + LEASE_MS), Value::Int(now + keep_ms)];
        if changes(&sql(it, false, insert, p)?) == 1 {
            return Ok(Claim::Mine(token));
        }
        // Someone has the key.
        let rows = sql(
            it,
            true,
            "select fingerprint, token, done, lease_until, expires_at, status, headers, body, body_b64 from _quillo_idempotency where id = ?",
            vec![Value::str(id)],
        )?;
        let Value::List(rows) = rows else { continue };
        let Some(Value::Map(row)) = rows.first() else { continue };
        let int = |k: &str| match row.get(k) {
            Some(Value::Int(n)) => *n,
            Some(Value::Bool(b)) => *b as i64,
            Some(Value::Str(s)) => s.parse().unwrap_or(0),
            _ => 0,
        };
        let text = |k: &str| row.get(k).map_or(String::new(), |v| if matches!(v, Value::Nil) { String::new() } else { v.display() });
        let theirs = text("token");
        if int("expires_at") <= now {
            sql(it, false, "delete from _quillo_idempotency where id = ? and token = ?", vec![Value::str(id), Value::str(&theirs)])?;
            continue;
        }
        if text("fingerprint") != print {
            return Ok(Claim::Refused(422, "this Idempotency-Key was used with another request".into()));
        }
        if int("done") == 1 {
            let headers: Vec<(String, String)> = serde_json::from_str(&text("headers")).unwrap_or_default();
            let body = text("body");
            let body = if int("body_b64") == 1 { crate::crypto::unb64url(&body).unwrap_or_default() } else { body.into_bytes() };
            return Ok(Claim::Answer(HttpResponse { status: int("status") as u16, headers, body }));
        }
        if int("lease_until") > now {
            return Ok(Claim::Refused(409, "a request with this Idempotency-Key is still running; try again shortly".into()));
        }
        if !take_over {
            return Ok(Claim::Refused(
                409,
                "the first request with this Idempotency-Key stopped before it finished; whether it took effect is not known".into(),
            ));
        }
        // Its server stopped part way, and its transaction rolled back.
        let p = vec![Value::str(&token), Value::Int(now + LEASE_MS), Value::str(id), Value::str(&theirs)];
        if changes(&sql(it, false, "update _quillo_idempotency set token = ?, lease_until = ? where id = ? and token = ? and done = 0", p)?) == 1 {
            return Ok(Claim::Mine(token));
        }
    }
    Ok(Claim::Refused(409, "a request with this Idempotency-Key is still running; try again shortly".into()))
}

/// Keeps the answer, if the claim is still this request's: Ok(false) if not.
fn store(it: &mut Interp, id: &str, token: &str, a: &HttpResponse) -> Result<bool, String> {
    let headers: Vec<&(String, String)> = a.headers.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("set-cookie")).collect();
    let headers = serde_json::to_string(&headers).unwrap_or_else(|_| "[]".into());
    let (body, b64) = match std::str::from_utf8(&a.body) {
        Ok(s) => (s.to_string(), 0),
        Err(_) => (crate::crypto::b64url(&a.body), 1),
    };
    let p = vec![Value::Int(a.status as i64), Value::str(headers), Value::str(body), Value::Int(b64), Value::str(id), Value::str(token)];
    let r = sql(it, false, "update _quillo_idempotency set done = 1, status = ?, headers = ?, body = ?, body_b64 = ? where id = ? and token = ? and done = 0", p)?;
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
        let mut g = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let last = g.get_or_insert_with(Default::default);
        if last.get(&k).map_or(false, |t| now - t < 60_000) {
            return;
        }
        last.insert(k, now);
    }
    let _ = sql(it, false, "delete from _quillo_idempotency where expires_at < ?", vec![Value::Int(now)]);
}

/// Whether a server fn uses `idempotent`: then the page sends a key with
/// each call, and sends it again with the call if the answer does not come
/// (see `rpcRaw` in the browser runtime).
pub fn used_by(def: &crate::ast::FnDef) -> bool {
    format!("{:?}", def.body).contains("\"idempotent\"")
}
