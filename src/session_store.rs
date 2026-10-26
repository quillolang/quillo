//! Sessions kept in the database, which can be ended.
//!
//! ```text
//! sessions({ store: "db", idle: "30m", max_age: "12h" })   // at the top of the program
//! set_session({ user: u.id })            // a new id when the user changes
//! renew_session()                        // a new id, the same session
//! end_session()                          // this one, on the server too
//! end_sessions(u.id)                     // all of a user's: a new password, an admin
//! end_sessions(u.id, { keep_current: true })
//! end_sessions(u.id, { id: s.id })       // one of them, from the list
//! user_sessions(u.id)                    // [{ id, created, seen, expires, ip, agent, current }]
//! ```
//!
//! The cookie holds only a random id (32 bytes from the operating system);
//! the session is a row of `_quillo_sessions`, kept with Quillo's own tables
//! (the program's database, or the system database when there is one per
//! tenant), under the id's SHA-256, so the table does not hold what the
//! cookies hold. Each row belongs to a tenant and a mount point, as signed
//! sessions do. Every server of the program reads the same rows, so a
//! session ended on one is ended on all.
//!
//! A session ends `max_age` after it began (12 hours unless set), or when it
//! has not been used for `idle` (30 minutes unless set; false for never).
//! When `set_session` changes who the session's `user` is (logging in, or
//! out to another), the session gets a new id, so an id someone planted or
//! saw before the login is worth nothing after it.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;

type R = Result<Value, Ctrl>;

pub const BUILTINS: &[(&str, BuiltinFn)] =
    &[("sessions", sessions), ("renew_session", renew_session), ("end_sessions", end_sessions), ("user_sessions", user_sessions)];

/// How sessions are kept when they are in the database.
#[derive(Clone, Debug)]
pub struct Config {
    /// Seconds a session lasts from its start.
    pub max_age: u64,
    /// Seconds a session lasts unused; None: no limit.
    pub idle: Option<u64>,
}

const MAX_AGE: u64 = 12 * 3600;
const IDLE: u64 = 30 * 60;

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn table(mysql: bool) -> String {
    if mysql {
        "create table if not exists _quillo_sessions (id varchar(64) primary key, scope varchar(255) not null, user_id varchar(255) not null, \
         data mediumtext not null, created bigint not null, seen bigint not null, expires bigint not null, ip varchar(64) not null, \
         agent varchar(255) not null, index (scope, user_id))"
            .into()
    } else {
        "create table if not exists _quillo_sessions (id text primary key, scope text not null, user_id text not null, \
         data text not null, created bigint not null, seen bigint not null, expires bigint not null, ip text not null, agent text not null)"
            .into()
    }
}

/// `sql` on the sessions' table, made the first time it is missing.
fn run(it: &mut Interp, sql: &str, params: Vec<Value>) -> Result<Vec<Value>, String> {
    match crate::db::own_sql(it, |_| sql.to_string(), params.clone()) {
        Ok(rows) => Ok(rows),
        Err(_) => {
            crate::db::own_sql(it, table, Vec::new())?;
            let _ = crate::db::own_sql(
                it,
                |mysql| if mysql { String::new() } else { "create index if not exists _quillo_sessions_user on _quillo_sessions (scope, user_id)".into() },
                Vec::new(),
            );
            crate::db::own_sql(it, |_| sql.to_string(), params)
        }
    }
}

/// The tenant and mount point a session belongs to.
fn scope(it: &mut Interp) -> String {
    let tenant = it.tenant();
    format!("{}|{}", tenant.as_deref().unwrap_or(""), it.base)
}

/// What is stored for a cookie's id.
fn key(token: &str) -> String {
    use sha2::Digest;
    crate::crypto::hex(&sha2::Sha256::digest(token.as_bytes()))
}

fn int(m: &Map, k: &str) -> i64 {
    match m.get(k) {
        Some(Value::Int(n)) => *n,
        Some(v) => v.display().parse().unwrap_or(0),
        None => 0,
    }
}

/// Who a session's data says is logged in, as text ("" for no one).
fn user_of(data: &Value) -> String {
    match data {
        Value::Map(m) => match m.get("user") {
            None | Some(Value::Nil) => String::new(),
            Some(v) => v.display(),
        },
        _ => String::new(),
    }
}

fn config(it: &Interp) -> Config {
    it.site.sessions.clone().unwrap_or(Config { max_age: MAX_AGE, idle: Some(IDLE) })
}

/// Whether a row is a session now (not past its end, nor idle too long).
fn alive(c: &Config, row: &Map, now: i64) -> bool {
    int(row, "expires") > now && c.idle.map_or(true, |i| int(row, "seen") + i as i64 > now)
}

/// The request's session from the database: its data, after finding its
/// row (once per request). Two cookies that are both sessions here are
/// none: one of them was planted.
pub fn read(it: &mut Interp) -> Value {
    if let Some(s) = &it.session {
        if it.access.session_read {
            return s.clone();
        }
    }
    it.access.session_read = true;
    it.access.session_id = None;
    let c = config(it);
    let here = scope(it);
    let t = now();
    let mut found = Vec::new();
    for token in crate::session::cookies(it) {
        // Ours are 43 characters of base64url; anything else is not one.
        if token.len() != 43 || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            continue;
        }
        let id = key(&token);
        let Ok(rows) = run(it, "select data, seen, expires from _quillo_sessions where id = ? and scope = ?", vec![Value::str(&id), Value::str(&here)]) else {
            continue;
        };
        if let Some(Value::Map(row)) = rows.first() {
            if alive(&c, row, t) {
                found.push((id, row.clone()));
            } else {
                let _ = run(it, "delete from _quillo_sessions where id = ?", vec![Value::str(&id)]);
            }
        }
    }
    let data = match found.len() {
        1 => {
            let (id, row) = found.remove(0);
            // Used now: written at most every tenth of the idle time (a minute at most).
            let every = c.idle.map_or(60, |i| (i / 10).clamp(1, 60)) as i64;
            if t - int(&row, "seen") >= every {
                let _ = run(it, "update _quillo_sessions set seen = ? where id = ?", vec![Value::Int(t), Value::str(&id)]);
            }
            it.access.session_id = Some(id);
            let text = row.get("data").map(|d| d.display()).unwrap_or_default();
            serde_json::from_str::<serde_json::Value>(&text).map(|j| Value::from_json(&j)).unwrap_or_else(|_| Value::map(Map::new()))
        }
        _ => Value::map(Map::new()),
    };
    sweep(it, t);
    it.session = Some(data.clone());
    data
}

/// Now and then, rows past their end go.
fn sweep(it: &mut Interp, t: i64) {
    use std::sync::atomic::{AtomicI64, Ordering::Relaxed};
    static LAST: AtomicI64 = AtomicI64::new(0);
    let last = LAST.load(Relaxed);
    if t - last >= 300 && LAST.compare_exchange(last, t, Relaxed, Relaxed).is_ok() {
        let _ = run(it, "delete from _quillo_sessions where expires <= ?", vec![Value::Int(t)]);
    }
}

fn header(it: &mut Interp, name: &str, most: usize) -> String {
    let Some(Value::Map(req)) = it.request_value() else { return String::new() };
    let v = match (name, req.get("headers")) {
        ("ip", _) => req.get("ip").map(|v| v.display()).unwrap_or_default(),
        (_, Some(Value::Map(h))) => h.get(name).map(|v| v.display()).unwrap_or_default(),
        _ => String::new(),
    };
    v.chars().take(most).collect()
}

/// A new row for `data`, and its cookie: the id changes.
fn start(it: &mut Interp, data: &Value, created: i64, expires: i64, line: u32) -> Result<(), Ctrl> {
    let token = crate::crypto::b64url(&crate::crypto::secret_bytes(32));
    let id = key(&token);
    let here = scope(it);
    let (ip, agent) = (header(it, "ip", 64), header(it, "user-agent", 255));
    let t = now();
    let sql = "insert into _quillo_sessions (id, scope, user_id, data, created, seen, expires, ip, agent) values (?, ?, ?, ?, ?, ?, ?, ?, ?)";
    let params = vec![
        Value::str(&id),
        Value::str(&here),
        Value::str(user_of(data)),
        Value::str(data.json_text()),
        Value::Int(created),
        Value::Int(t),
        Value::Int(expires),
        Value::str(ip),
        Value::str(agent),
    ];
    if let Err(e) = run(it, sql, params) {
        return fail(line, format!("the session could not be kept: {}", e));
    }
    it.access.session_id = Some(id);
    crate::session::send_cookie(it, &token, Some((expires - t).max(0) as u64), line)
}

/// `set_session(map, { max_age })` with sessions in the database: the
/// same id while the user is the same; a new one when it changes.
pub fn write(it: &mut Interp, data: Value, age: Option<u64>, line: u32) -> R {
    let before = read(it);
    let t = now();
    match it.access.session_id.clone() {
        Some(id) if user_of(&before) == user_of(&data) => {
            let mut sql = "update _quillo_sessions set data = ?, seen = ?".to_string();
            let mut params = vec![Value::str(data.json_text()), Value::Int(t)];
            if let Some(a) = age {
                sql.push_str(", expires = ?");
                params.push(Value::Int(t + a as i64));
            }
            sql.push_str(" where id = ?");
            params.push(Value::str(&id));
            if let Err(e) = run(it, &sql, params) {
                return fail(line, format!("the session could not be kept: {}", e));
            }
            if let Some(a) = age {
                // The cookie lasts as long as the session.
                let token = crate::session::cookies(it).into_iter().find(|tk| key(tk) == id).unwrap_or_default();
                crate::session::send_cookie(it, &token, Some(a), line)?;
            }
        }
        old => {
            if let Some(id) = old {
                let _ = run(it, "delete from _quillo_sessions where id = ?", vec![Value::str(&id)]);
            }
            let a = age.unwrap_or(config(it).max_age) as i64;
            start(it, &data, t, t + a, line)?;
        }
    }
    it.session = Some(data);
    Ok(Value::Nil)
}

/// `end_session()` with sessions in the database: the row goes too.
pub fn end(it: &mut Interp, line: u32) -> R {
    read(it);
    if let Some(id) = it.access.session_id.take() {
        let _ = run(it, "delete from _quillo_sessions where id = ?", vec![Value::str(&id)]);
    }
    crate::session::send_cookie(it, "", Some(0), line)?;
    it.session = Some(Value::map(Map::new()));
    Ok(Value::Nil)
}

/// `renew_session()`: the same session under a new id (after a change of
/// rights, say), ending when it would have. Signed sessions are signed
/// again.
fn renew_session(it: &mut Interp, _: Vec<Value>, line: u32) -> R {
    if it.site.sessions.is_none() {
        let data = crate::session::current(it);
        if matches!(&data, Value::Map(m) if m.is_empty()) {
            return Ok(Value::Nil);
        }
        return crate::session::sign_in(it, data, None, line);
    }
    let data = read(it);
    let Some(old) = it.access.session_id.clone() else { return Ok(Value::Nil) };
    let rows = run(it, "select created, expires from _quillo_sessions where id = ?", vec![Value::str(&old)]).unwrap_or_default();
    let Some(Value::Map(row)) = rows.first() else { return Ok(Value::Nil) };
    let (created, expires) = (int(row, "created"), int(row, "expires"));
    let _ = run(it, "delete from _quillo_sessions where id = ?", vec![Value::str(&old)]);
    start(it, &data, created, expires, line)?;
    Ok(Value::Nil)
}

fn duration(v: &Value, what: &str, line: u32) -> Result<Option<u64>, Ctrl> {
    match v {
        Value::Nil | Value::Bool(false) => Ok(None),
        v => match crate::builtins::duration_seconds(v) {
            Some(n) if n > 0 => Ok(Some(n)),
            _ => fail(line, format!("`sessions`' {} is a duration like \"30m\", or false, got {}", what, v.repr())),
        },
    }
}

/// `sessions({ store: "db", idle: "30m", max_age: "12h" })`, at the top of
/// a program: where sessions are kept. `store: "cookie"` (the default) signs
/// them into the cookie.
fn sessions(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let m = match args.first() {
        Some(Value::Map(m)) => m.clone(),
        other => return fail(line, format!("`sessions` expects a map such as {{ store: \"db\" }}, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let mut c = Config { max_age: MAX_AGE, idle: Some(IDLE) };
    let mut db = false;
    for (k, v) in m.iter() {
        match (&**k, v) {
            ("store", Value::Str(s)) if &**s == "db" => db = true,
            ("store", Value::Str(s)) if &**s == "cookie" => db = false,
            ("store", v) => return fail(line, format!("`sessions`' store is \"db\" or \"cookie\", got {}", v.repr())),
            ("idle", v) => c.idle = duration(v, "idle", line)?,
            ("max_age", v) => match duration(v, "max_age", line)? {
                Some(n) => c.max_age = n,
                None => return fail(line, "`sessions`' max_age is a duration like \"12h\""),
            },
            _ => return fail(line, format!("`sessions` takes store, idle and max_age, got {:?}", &**k)),
        }
    }
    std::sync::Arc::make_mut(&mut it.site).sessions = db.then_some(c);
    Ok(Value::Nil)
}

fn need_db(it: &Interp, f: &str, line: u32) -> Result<(), Ctrl> {
    if it.site.sessions.is_none() {
        return fail(line, format!("`{}` needs sessions kept in the database: sessions({{ store: \"db\" }}) at the top of the program", f));
    }
    Ok(())
}

fn user_arg(args: &[Value], f: &str, line: u32) -> Result<String, Ctrl> {
    match args.first() {
        None | Some(Value::Nil) => fail(line, format!("`{}` needs the user, as the sessions' `user` holds it", f)),
        Some(v) => Ok(v.display()),
    }
}

/// `end_sessions(user, { keep_current, id })`: ends the user's sessions
/// here (this tenant), on every server; how many it ended.
fn end_sessions(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    need_db(it, "end_sessions", line)?;
    let user = user_arg(&args, "end_sessions", line)?;
    let (keep, only) = match args.get(1) {
        None | Some(Value::Nil) => (false, None),
        Some(Value::Map(m)) => (m.get("keep_current").map_or(false, |v| v.truthy()), m.get("id").filter(|v| !matches!(v, Value::Nil)).map(|v| v.display())),
        Some(v) => return fail(line, format!("`end_sessions` takes options as a map, got {}", v.type_name())),
    };
    read(it);
    let current = it.access.session_id.clone();
    let here = scope(it);
    let rows = match run(it, "select id from _quillo_sessions where scope = ? and user_id = ?", vec![Value::str(&here), Value::str(&user)]) {
        Ok(r) => r,
        Err(e) => return fail(line, format!("`end_sessions`: {}", e)),
    };
    let mut ended = 0;
    let mut mine = false;
    for r in rows {
        let Value::Map(r) = r else { continue };
        let id = r.get("id").map(|v| v.display()).unwrap_or_default();
        if only.as_ref().map_or(false, |o| *o != id) || (keep && current.as_deref() == Some(id.as_str())) {
            continue;
        }
        if run(it, "delete from _quillo_sessions where id = ?", vec![Value::str(&id)]).is_ok() {
            ended += 1;
            mine |= current.as_deref() == Some(id.as_str());
        }
    }
    if mine {
        it.access.session_id = None;
        crate::session::send_cookie(it, "", Some(0), line)?;
        it.session = Some(Value::map(Map::new()));
    }
    Ok(Value::Int(ended))
}

/// `user_sessions(user)`: the user's sessions here, the latest used first,
/// for a page of the devices signed in.
fn user_sessions(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    need_db(it, "user_sessions", line)?;
    let user = user_arg(&args, "user_sessions", line)?;
    read(it);
    let current = it.access.session_id.clone();
    let c = config(it);
    let here = scope(it);
    let t = now();
    let rows = match run(
        it,
        "select id, created, seen, expires, ip, agent from _quillo_sessions where scope = ? and user_id = ? order by seen desc",
        vec![Value::str(&here), Value::str(&user)],
    ) {
        Ok(r) => r,
        Err(e) => return Ok(Value::err(Value::str(e))),
    };
    let list = rows
        .into_iter()
        .filter_map(|r| match r {
            Value::Map(r) if alive(&c, &r, t) => {
                let mut m = (*r).clone();
                let id = r.get("id").map(|v| v.display()).unwrap_or_default();
                m.insert("current".into(), Value::Bool(current.as_deref() == Some(id.as_str())));
                for k in ["created", "seen", "expires"] {
                    m.insert(k.into(), Value::Int(int(&r, k)));
                }
                Some(Value::map(m))
            }
            _ => None,
        })
        .collect();
    Ok(Value::list(list))
}
