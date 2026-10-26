//! `audit`: an append-only log of what people did, kept with the program's
//! data.
//!
//! ```text
//! audit("contact.delete", { id })                  // who, what, when, from where
//! audit.query({ actor: "42", action: "contact.*", since: "2026-10-01", limit: 50 })
//! audit.verify()                                   // { ok, rows, last }, or where it breaks
//! audit.actor(() => current_user().email)         // who "the user" is (default: session().user)
//! ```
//!
//! Each row goes to `_quillo_audit` in the program's database (the
//! tenant's, with a database per tenant), so inside `db.transaction` it is
//! written with the change: a change rolled back leaves no row, and one
//! committed always has its row. Outside a transaction it is written in
//! one of its own.
//!
//! Rows are chained, one chain per tenant: each holds the SHA-256 of the
//! row before it and of its own contents, so changing, removing or
//! inserting a row breaks the chain from there on, and `audit.verify()`
//! says where. Appends take turns per tenant (a lock on a row of
//! `_quillo_audit_lock` on PostgreSQL and MySQL; SQLite has one writer at a
//! time), so the chain has one order whatever the number of servers, and
//! `unique (tenant, seq)` makes sure of it.
//!
//! Quillo's own `db` functions refuse SQL that writes to the table: only
//! `audit(...)` adds to it. That stops mistakes, not someone with the
//! database's password: revoke `update` and `delete` on it for that (see
//! the user guide).

use crate::db::{Backend, Pinned};
use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

type R = Result<Value, Ctrl>;

/// The key under which a map that can be called keeps its function
/// (`audit(...)` is `audit`'s; `audit.query` and the rest are its fields).
pub const CALL_KEY: &str = "$call";

pub fn install(m: &mut Map) {
    m.insert(CALL_KEY.into(), Value::Builtin("audit", log));
    let fns: &[(&'static str, BuiltinFn)] = &[("audit.query", query), ("audit.verify", verify), ("audit.actor", actor)];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("audit.").into(), Value::Builtin(name, *f));
    }
}

/// The hash a tenant's first row follows.
const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
/// The columns of a row, in the order its hash covers them.
const COLUMNS: &str = "id, tenant, seq, at, actor, action, details, rid, ip, agent, prev, hash";
const MAX_DETAILS: usize = 64 << 10;

/// Why SQL a program runs through `db` may not run: it writes to the audit
/// log, which only `audit(...)` adds to.
pub(crate) fn refused(sql: &str) -> Option<String> {
    // Most SQL does not name it at all (and this runs for every query).
    if !sql.as_bytes().windows(13).any(|w| w.eq_ignore_ascii_case(b"_quillo_audit")) {
        return None;
    }
    let t = crate::sqlinfo::tables_of(sql);
    let audit_table = |w: &String| {
        let w = w.to_ascii_lowercase();
        let name = w.rsplit('.').next().unwrap_or("");
        name == "_quillo_audit" || name == "_quillo_audit_lock"
    };
    t.writes.iter().any(audit_table).then(|| "_quillo_audit is append-only: audit(...) adds to it, and nothing changes or removes its rows".to_string())
}

/// A row about to be written.
struct Row {
    tenant: String,
    at: String,
    actor: Option<String>,
    action: String,
    details: Option<String>,
    rid: Option<String>,
    ip: Option<String>,
    agent: Option<String>,
}

/// What a row's hash covers: the previous row's hash, then its contents
/// as a JSON array.
pub fn row_hash(prev: &str, tenant: &str, seq: i64, at: &str, actor: Option<&str>, action: &str, details: Option<&str>, rid: Option<&str>, ip: Option<&str>, agent: Option<&str>) -> String {
    let content = serde_json::json!([tenant, seq, at, actor, action, details, rid, ip, agent]).to_string();
    let mut h = Sha256::new();
    h.update(prev.as_bytes());
    h.update(b"\n");
    h.update(content.as_bytes());
    crate::crypto::hex(&h.finalize())
}

/// Databases (by their `Backend`'s address) known to have something, such
/// as the table. Each is kept as a `Weak` too: while it is in the set, its
/// address cannot be given to a database opened after it was closed (one
/// that may not have the table yet), as a bare address could be.
#[derive(Default)]
struct Known(std::collections::HashMap<usize, std::sync::Weak<Backend>>);

impl Known {
    fn has(&self, b: &Arc<Backend>) -> bool {
        self.0.contains_key(&(Arc::as_ptr(b) as usize))
    }

    fn add(&mut self, b: &Arc<Backend>) {
        // Those closed since are let go (and their addresses with them).
        if self.0.len() >= 64 {
            self.0.retain(|_, w| w.strong_count() > 0);
        }
        self.0.insert(Arc::as_ptr(b) as usize, Arc::downgrade(b));
    }
}

fn made() -> &'static Mutex<Known> {
    static M: std::sync::OnceLock<Mutex<Known>> = std::sync::OnceLock::new();
    M.get_or_init(Default::default)
}

fn table_sql(b: &Backend) -> &'static str {
    match b {
        Backend::Sqlite(_) => {
            "create table if not exists _quillo_audit (id integer primary key autoincrement, tenant text not null, seq integer not null, \
             at text not null, actor text, action text not null, details text, rid text, ip text, agent text, prev text not null, \
             hash text not null, unique (tenant, seq)); \
             create index if not exists _quillo_audit_at on _quillo_audit (tenant, at)"
        }
        Backend::Postgres(_) => {
            "create table if not exists _quillo_audit (id bigserial primary key, tenant text not null, seq bigint not null, \
             at text not null, actor text, action text not null, details text, rid text, ip text, agent text, prev text not null, \
             hash text not null, unique (tenant, seq)); \
             create index if not exists _quillo_audit_at on _quillo_audit (tenant, at); \
             create table if not exists _quillo_audit_lock (tenant text primary key)"
        }
        #[cfg(feature = "mysql")]
        Backend::Mysql(_) => {
            "create table if not exists _quillo_audit (id bigint auto_increment primary key, tenant varchar(63) not null, \
             seq bigint not null, at varchar(32) not null, actor varchar(255), action varchar(255) not null, details mediumtext, \
             rid varchar(64), ip varchar(64), agent varchar(512), prev char(64) not null, hash char(64) not null, \
             unique key (tenant, seq), key (tenant, at)); \
             create table if not exists _quillo_audit_lock (tenant varchar(63) primary key)"
        }
    }
}

/// Makes the tables if this database may not have them yet. On a server
/// database it is done on a connection of its own, outside any
/// transaction: MySQL would commit the request's transaction on `create`.
fn ensure(it: &mut Interp, b: &Arc<Backend>) -> Result<(), String> {
    if made().lock().unwrap_or_else(|e| e.into_inner()).has(b) {
        return Ok(());
    }
    let sql = table_sql(b);
    match &**b {
        Backend::Sqlite(m) => m.lock().unwrap_or_else(|e| e.into_inner()).execute_batch(sql).map_err(|e| e.to_string())?,
        _ => {
            let pool = b.clone();
            it.unlocked(move || {
                let mut c = pool.checkout()?;
                // Two servers making it at once: one of them may fail; the
                // second try finds it made.
                let mut r = c.batch(sql);
                if r.is_err() {
                    r = c.batch(sql);
                }
                // A user who may not create tables (as it should be, once
                // they are made): there it is.
                if r.is_err() && c.query("select seq from _quillo_audit where seq < 0", &[], None).is_ok() {
                    r = Ok(());
                }
                pool.give_back(c);
                r
            })?
        }
    }
    made().lock().unwrap_or_else(|e| e.into_inner()).add(b);
    Ok(())
}

/// Makes a tenant's row in `_quillo_audit_lock` (PostgreSQL's way, then
/// MySQL's).
const LOCK_ROW: [&str; 2] =
    ["insert into _quillo_audit_lock (tenant) values (?) on conflict do nothing", "insert ignore into _quillo_audit_lock (tenant) values (?)"];

/// Makes sure the tenant's lock row is there, on a connection of its own
/// that commits at once. Made inside the appends' transactions instead,
/// two first appends at once could each wait for the other on MySQL.
fn lock_row(it: &mut Interp, b: &Arc<Backend>, tenant: &str) -> Result<(), String> {
    // Held as a `Weak`, as in `Known`: the address stays this database's.
    static DONE: Mutex<Vec<(std::sync::Weak<Backend>, String)>> = Mutex::new(Vec::new());
    let here = |(w, t): &(std::sync::Weak<Backend>, String)| std::ptr::eq(w.as_ptr(), Arc::as_ptr(b)) && t == tenant;
    if matches!(&**b, Backend::Sqlite(_)) || DONE.lock().unwrap_or_else(|e| e.into_inner()).iter().any(here) {
        return Ok(());
    }
    let (pool, t, sql) = (b.clone(), tenant.to_string(), LOCK_ROW[b.is_mysql() as usize]);
    it.unlocked(move || {
        let mut c = pool.checkout()?;
        let r = c.exec(sql, &[Value::str(t)]).map(|_| ());
        pool.give_back(c);
        r
    })?;
    let mut done = DONE.lock().unwrap_or_else(|e| e.into_inner());
    if done.len() > 10_000 {
        done.clear();
    }
    done.push((Arc::downgrade(b), tenant.to_string()));
    Ok(())
}

/// The statements of an append, on any database.
trait Conn {
    fn rows(&mut self, sql: &str, params: &[Value]) -> Result<Vec<Value>, String>;
    fn run(&mut self, sql: &str, params: &[Value]) -> Result<(), String>;
}

impl Conn for rusqlite::Connection {
    fn rows(&mut self, sql: &str, params: &[Value]) -> Result<Vec<Value>, String> {
        crate::db::rows(self, sql, params, None)
    }
    fn run(&mut self, sql: &str, params: &[Value]) -> Result<(), String> {
        crate::db::rows(self, sql, params, None).map(|_| ())
    }
}

impl Conn for Pinned {
    fn rows(&mut self, sql: &str, params: &[Value]) -> Result<Vec<Value>, String> {
        self.query(sql, params, None)
    }
    fn run(&mut self, sql: &str, params: &[Value]) -> Result<(), String> {
        self.exec(sql, params).map(|_| ())
    }
}

fn text(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Nil) => None,
        Some(v) => Some(v.display()),
    }
}

fn opt_val(s: &Option<String>) -> Value {
    s.as_deref().map_or(Value::Nil, Value::str)
}

/// Appends `row` after the tenant's last one: (seq, hash). `kind` is
/// "sqlite", "pg" or "mysql".
fn append(c: &mut dyn Conn, kind: &str, row: &Row) -> Result<(i64, String), String> {
    let tenant = Value::str(&row.tenant);
    // Appends for a tenant take turns: a lock on its row in
    // `_quillo_audit_lock` (made beforehand: see `lock_row`), held until the
    // transaction ends.
    if kind != "sqlite" {
        let lock = "select tenant from _quillo_audit_lock where tenant = ? for update";
        if c.rows(lock, std::slice::from_ref(&tenant))?.is_empty() {
            c.run(LOCK_ROW[(kind == "mysql") as usize], std::slice::from_ref(&tenant))?;
            c.rows(lock, std::slice::from_ref(&tenant))?;
        }
    }
    // MySQL reads a transaction's first snapshot again unless the read
    // locks; a shared lock reads the latest row (and needs no more than
    // `select` on the table).
    let last_sql = if kind == "mysql" {
        "select seq, hash from _quillo_audit where tenant = ? order by seq desc limit 1 lock in share mode"
    } else {
        "select seq, hash from _quillo_audit where tenant = ? order by seq desc limit 1"
    };
    let last = c.rows(last_sql, std::slice::from_ref(&tenant))?;
    let (seq, prev) = match last.first() {
        Some(Value::Map(m)) => (
            match m.get("seq") {
                Some(Value::Int(n)) => n + 1,
                _ => return Err("the audit log's last row has no number".into()),
            },
            m.get("hash").map(|h| h.display()).unwrap_or_default(),
        ),
        _ => (1, GENESIS.to_string()),
    };
    let hash = row_hash(&prev, &row.tenant, seq, &row.at, row.actor.as_deref(), &row.action, row.details.as_deref(), row.rid.as_deref(), row.ip.as_deref(), row.agent.as_deref());
    c.run(
        "insert into _quillo_audit (tenant, seq, at, actor, action, details, rid, ip, agent, prev, hash) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        &[
            tenant,
            Value::Int(seq),
            Value::str(&row.at),
            opt_val(&row.actor),
            Value::str(&row.action),
            opt_val(&row.details),
            opt_val(&row.rid),
            opt_val(&row.ip),
            opt_val(&row.agent),
            Value::str(&prev),
            Value::str(&hash),
        ],
    )?;
    Ok((seq, hash))
}

/// Runs `f` on the database `b`: in the request's transaction if one is
/// open on it, else in a transaction of its own.
fn in_tx<T: Send>(it: &mut Interp, b: &Arc<Backend>, f: impl FnOnce(&mut dyn Conn, &str) -> Result<T, String> + Send) -> Result<T, String> {
    match &**b {
        Backend::Sqlite(m) => {
            // Under the interpreter lock and the connection's: no other
            // request of this process runs between the read and the write.
            let mut c = m.lock().unwrap_or_else(|e| e.into_inner());
            if !c.is_autocommit() {
                return f(&mut *c, "sqlite");
            }
            // Other processes may share the file: `immediate` takes the
            // write lock before the last row is read.
            c.execute_batch("begin immediate").map_err(|e| e.to_string())?;
            let r = f(&mut *c, "sqlite");
            let end = if r.is_ok() { "commit" } else { "rollback" };
            let ended = c.execute_batch(end).map_err(|e| e.to_string());
            if ended.is_err() && !c.is_autocommit() {
                let _ = c.execute_batch("rollback");
            }
            ended.and(r)
        }
        _ => {
            let kind = if b.is_mysql() { "mysql" } else { "pg" };
            let own = !crate::db::in_transaction(it, b);
            crate::db::pooled_run(it, b, move |c| {
                if own {
                    c.batch("begin")?;
                }
                let r = f(c, kind);
                if own {
                    let ended = c.batch(if r.is_ok() { "commit" } else { "rollback" });
                    if ended.is_err() {
                        let _ = c.batch("rollback");
                    }
                    return ended.and(r);
                }
                r
            })
        }
    }
}

/// Who the user is: what the function given to `audit.actor` returns, or
/// else the session's `user`.
fn actor_now(it: &mut Interp) -> Result<Option<String>, Ctrl> {
    if let Some(f) = it.site.audit_actor.clone() {
        return match it.call(&f, Vec::new(), 0) {
            Ok(v) | Err(Ctrl::Return(v)) => Ok(match v {
                Value::Err(_) => None,
                v => text(Some(&v)),
            }),
            Err(e) => Err(e),
        };
    }
    Ok(session_user(it))
}

/// The session's `user`, if the request read or set the session (for the
/// request log), or else as the cookie has it.
pub fn session_user(it: &mut Interp) -> Option<String> {
    let s = match &it.session {
        Some(s) => s.clone(),
        None => crate::session::current(it),
    };
    match &s {
        Value::Map(m) => text(m.get("user")),
        _ => None,
    }
}

/// The session's `user` if this request read or set its session: what the
/// request log shows (it reads no cookie of its own).
pub fn known_user(it: &Interp) -> Option<String> {
    match &it.session {
        Some(Value::Map(m)) => text(m.get("user")),
        _ => None,
    }
}

fn cut(s: String, most: usize) -> String {
    if s.len() <= most {
        return s;
    }
    let mut end = most;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// The program's database for the audit log, made ready.
fn audit_db(it: &mut Interp) -> Result<Arc<Backend>, String> {
    let b = crate::db::backend_on(it, None)?;
    ensure(it, &b)?;
    Ok(b)
}

fn db_err(e: impl std::fmt::Display) -> Value {
    Value::err(Value::str(format!("audit: {}", e)))
}

/// `audit(action, details = null)`: `{ seq, hash, at }`, or an error value
/// when the database could not take it.
fn log(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let action = match args.first() {
        Some(Value::Str(s)) if !s.trim().is_empty() && s.len() <= 255 => s.to_string(),
        Some(v) => return fail(line, format!("`audit` expects what was done, such as \"contact.delete\" (up to 255 characters), got {}", crate::interp::short(v))),
        None => return fail(line, "`audit` expects what was done, such as audit(\"contact.delete\", { id })"),
    };
    let details = match args.get(1) {
        None | Some(Value::Nil) => None,
        // Values under keys marked sensitive (see `redact`) are not kept.
        Some(v) => Some(crate::redact::value(v).json_text()),
    };
    if details.as_ref().map_or(false, |d| d.len() > MAX_DETAILS) {
        return fail(line, "`audit`'s details are over 64 KB: keep ids, not documents");
    }
    let actor = actor_now(it)?.map(|a| cut(a, 255));
    let tenant = it.tenant().map(|t| t.to_string()).unwrap_or_default();
    let (rid, ip, agent) = match it.request_value() {
        Some(Value::Map(req)) => {
            let agent = match req.get("headers") {
                Some(Value::Map(h)) => text(h.get("user-agent")).map(|a| cut(a, 512)),
                _ => None,
            };
            (it.request_id.as_ref().map(|r| r.to_string()), text(req.get("ip")).map(|a| cut(a, 64)), agent)
        }
        _ => (None, None, None),
    };
    let row = Row { tenant, at: crate::httpd::iso_now(), actor, action, details, rid: rid.map(|r| cut(r, 64)), ip, agent };
    let b = match audit_db(it) {
        Ok(b) => b,
        Err(e) => return Ok(db_err(e)),
    };
    if let Err(e) = lock_row(it, &b, &row.tenant) {
        return Ok(db_err(e));
    }
    it.wrote_req = Some(crate::httpd::request_number());
    let at = row.at.clone();
    match in_tx(it, &b, move |c, kind| append(c, kind, &row)) {
        Ok((seq, hash)) => {
            crate::db::announce(it, "insert into _quillo_audit", None);
            let mut m = Map::new();
            m.insert("seq".into(), Value::Int(seq));
            m.insert("hash".into(), Value::str(hash));
            m.insert("at".into(), Value::str(at));
            Ok(Value::map(m))
        }
        Err(e) => Ok(db_err(e)),
    }
}

/// `audit.actor(fn)`: how `audit` finds who the user is. The function takes
/// no arguments (it may call `session()`, or the database); null is nobody.
fn actor(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match args.first() {
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => {
            Arc::make_mut(&mut it.site).audit_actor = Some(f.clone());
            Ok(Value::Nil)
        }
        other => fail(line, format!("`audit.actor` expects a function that gives who the user is, got {}", other.map_or("nothing", |v| v.type_name()))),
    }
}

/// A time to compare with rows' `at`: RFC 3339 text in UTC (or a date), or
/// milliseconds since 1970 as `now()` gives.
fn when(v: &Value, what: &str, line: u32) -> Result<String, Ctrl> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        Value::Int(ms) => {
            let secs = ms.div_euclid(1000);
            let (y, mo, d) = crate::httpd::civil(secs.div_euclid(86400));
            let s = secs.rem_euclid(86400);
            Ok(format!("{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", y, mo, d, s / 3600, s % 3600 / 60, s % 60, ms.rem_euclid(1000)))
        }
        v => fail(line, format!("`audit.query`'s {} is a date, an RFC 3339 time or milliseconds, got {}", what, v.type_name())),
    }
}

/// A row as programs see it: `details` parsed back.
fn row_out(v: Value) -> Value {
    let Value::Map(mut m) = v else { return v };
    let r = Arc::make_mut(&mut m);
    if let Some(Value::Str(d)) = r.get("details") {
        let parsed = serde_json::from_str::<serde_json::Value>(d).map(|j| Value::from_json(&j)).unwrap_or(Value::Str(d.clone()));
        r.insert("details".into(), parsed);
    }
    Value::Map(m)
}

/// Reads rows with `sql`, from the request's transaction's connection if
/// it has one.
fn read(it: &mut Interp, b: &Arc<Backend>, sql: String, params: Vec<Value>) -> Result<Vec<Value>, String> {
    match &**b {
        Backend::Sqlite(m) => crate::db::rows(&m.lock().unwrap_or_else(|e| e.into_inner()), &sql, &params, None),
        _ => crate::db::pooled_run(it, b, move |c| c.query(&sql, &params, None)),
    }
}

/// `audit.query({ actor, action, since, until, before, limit })`: the
/// tenant's rows, newest first. `action` ending in `*` matches the start
/// (`"contact.*"`); `since` is inclusive and `until` is not; `before` is a
/// row's `seq`, for the next page; `limit` is 100 unless given (10,000 at
/// most).
fn query(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let o = match args.first() {
        None | Some(Value::Nil) => Map::new(),
        Some(Value::Map(m)) => (**m).clone(),
        Some(v) => return fail(line, format!("`audit.query` expects options as a map, got {}", v.type_name())),
    };
    let mut sql = format!("select {} from _quillo_audit where tenant = ?", COLUMNS);
    let mut params = vec![Value::str(it.tenant().as_deref().unwrap_or(""))];
    for (k, v) in o.iter() {
        if matches!(v, Value::Nil) {
            continue;
        }
        match k.as_str() {
            "actor" => {
                sql.push_str(" and actor = ?");
                params.push(Value::str(v.display()));
            }
            "action" => {
                let a = v.display();
                match a.strip_suffix('*') {
                    Some(prefix) => {
                        // Rows whose action starts with `prefix`, as text
                        // (`like` would take `_` and `%` in it as patterns).
                        sql.push_str(&format!(" and substr(action, 1, {}) = ?", prefix.chars().count()));
                        params.push(Value::str(prefix));
                    }
                    None => {
                        sql.push_str(" and action = ?");
                        params.push(Value::str(a));
                    }
                }
            }
            "since" => {
                sql.push_str(" and at >= ?");
                params.push(Value::str(when(v, "since", line)?));
            }
            "until" => {
                sql.push_str(" and at < ?");
                params.push(Value::str(when(v, "until", line)?));
            }
            "before" => match v {
                Value::Int(n) => {
                    sql.push_str(" and seq < ?");
                    params.push(Value::Int(*n));
                }
                v => return fail(line, format!("`audit.query`'s before is a row's seq (an int), got {}", v.type_name())),
            },
            "limit" => {}
            other => return fail(line, format!("`audit.query` takes actor, action, since, until, before and limit, not `{}`", other)),
        }
    }
    let limit = match o.get("limit") {
        None | Some(Value::Nil) => 100,
        Some(Value::Int(n)) if (1..=10_000).contains(n) => *n,
        Some(v) => return fail(line, format!("`audit.query`'s limit is from 1 to 10000, got {}", crate::interp::short(v))),
    };
    sql.push_str(&format!(" order by seq desc limit {}", limit));
    let b = match audit_db(it) {
        Ok(b) => b,
        Err(e) => return Ok(db_err(e)),
    };
    match read(it, &b, sql, params) {
        Ok(rows) => {
            crate::db::announce(it, "select * from _quillo_audit", None);
            Ok(Value::list(rows.into_iter().map(row_out).collect()))
        }
        Err(e) => Ok(db_err(e)),
    }
}

/// `audit.verify()`: walks the tenant's chain from its first row. `{ ok:
/// true, rows, last }` (`last` is the newest row's hash: kept somewhere
/// else, it shows later that no rows were taken off the end), or `{ ok:
/// false, rows, broken: { seq, id, why } }` naming the first row that does
/// not follow from the one before.
fn verify(it: &mut Interp, _: Vec<Value>, _: u32) -> R {
    let tenant = it.tenant().map(|t| t.to_string()).unwrap_or_default();
    let b = match audit_db(it) {
        Ok(b) => b,
        Err(e) => return Ok(db_err(e)),
    };
    let mut prev = GENESIS.to_string();
    let mut expect = 1i64;
    let mut count = 0i64;
    let broken = |seq: i64, id: Option<&Value>, why: String, count: i64| {
        let mut b = Map::new();
        b.insert("seq".into(), Value::Int(seq));
        b.insert("id".into(), id.cloned().unwrap_or(Value::Nil));
        b.insert("why".into(), Value::str(why));
        let mut m = Map::new();
        m.insert("ok".into(), Value::Bool(false));
        m.insert("rows".into(), Value::Int(count));
        m.insert("broken".into(), Value::map(b));
        Ok(Value::map(m))
    };
    loop {
        let sql = format!("select {} from _quillo_audit where tenant = ? and seq >= ? order by seq limit 1000", COLUMNS);
        let rows = match read(it, &b, sql, vec![Value::str(&tenant), Value::Int(expect)]) {
            Ok(r) => r,
            Err(e) => return Ok(db_err(e)),
        };
        let n = rows.len();
        for r in rows {
            let Value::Map(m) = r else { continue };
            let t = |k: &str| text(m.get(k));
            let seq = match m.get("seq") {
                Some(Value::Int(n)) => *n,
                _ => return broken(expect, m.get("id"), "its seq is not a number".into(), count),
            };
            if seq != expect {
                return broken(expect, None, format!("row {} is missing (the next is {})", expect, seq), count);
            }
            if t("prev").as_deref() != Some(prev.as_str()) {
                return broken(seq, m.get("id"), "it does not follow from the row before (its prev differs)".into(), count);
            }
            let tenant_of = t("tenant").unwrap_or_default();
            let hash = row_hash(
                &prev,
                &tenant_of,
                seq,
                &t("at").unwrap_or_default(),
                t("actor").as_deref(),
                &t("action").unwrap_or_default(),
                t("details").as_deref(),
                t("rid").as_deref(),
                t("ip").as_deref(),
                t("agent").as_deref(),
            );
            if t("hash").as_deref() != Some(hash.as_str()) {
                return broken(seq, m.get("id"), "its contents changed since it was written (its hash differs)".into(), count);
            }
            prev = hash;
            expect += 1;
            count += 1;
        }
        if n < 1000 {
            break;
        }
    }
    let mut m = Map::new();
    m.insert("ok".into(), Value::Bool(true));
    m.insert("rows".into(), Value::Int(count));
    m.insert("last".into(), if count == 0 { Value::Nil } else { Value::str(prev) });
    Ok(Value::map(m))
}

#[cfg(test)]
mod tests {
    #[test]
    fn writes_to_the_log_are_refused() {
        assert!(super::refused("delete from _quillo_audit where id = 1").is_some());
        assert!(super::refused("UPDATE \"_quillo_audit\" set action = 'x'").is_some());
        assert!(super::refused("drop table public._quillo_audit").is_some());
        assert!(super::refused("insert into _quillo_audit (action) values ('x')").is_some());
        assert!(super::refused("select * from _quillo_audit").is_none());
        assert!(super::refused("delete from contacts").is_none());
    }
}
