//! Rate limits.
//!
//! ```text
//! rate_limit("login:" + request.ip, 5, "1m")              // true while under 5 a minute
//! rate_limit("api:" + key, 100, "1m", { reply: true })    // else 429, with Retry-After
//! rate_limit("fail:" + email, 5, "15m", { cost: 0 })      // is there room? (counts nothing)
//! rate_limit_reset("fail:" + email)
//! ```
//!
//! A limit counts calls in windows of `per`, and weighs the window before
//! by how much of it still falls within `per` of now: a sliding window,
//! without the burst at the turn of a fixed one. Calls refused count too,
//! so a client that keeps trying stays refused. Keys are the tenant's (and
//! the mount point's): one tenant's limits never touch another's.
//!
//! Counts are kept in this process, unless sessions are kept in the
//! database (`sessions({ store: "db" })`): then they are kept there too,
//! in `_quillo_limits` with Quillo's own tables, and every server of the
//! program counts the same calls. `{ store: "memory" }` or `{ store: "db" }`
//! chooses for one limit.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::collections::HashMap;
use std::sync::Mutex;

type R = Result<Value, Ctrl>;

pub const BUILTINS: &[(&str, BuiltinFn)] = &[("rate_limit", rate_limit), ("rate_limit_reset", rate_limit_reset)];

/// The counts of a key's last two windows, kept in this process.
struct Counts {
    window: i64,
    now: u64,
    before: u64,
    /// When it can be forgotten.
    until: i64,
}

static KEPT: Mutex<Option<HashMap<String, Counts>>> = Mutex::new(None);

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// A key for the tenant and mount point, short enough for any database.
fn full_key(it: &mut Interp, key: &str) -> String {
    use sha2::Digest;
    let tenant = it.tenant();
    let whole = format!("{}|{}|{}", tenant.as_deref().unwrap_or(""), it.base, key);
    crate::crypto::hex(&sha2::Sha256::digest(whole.as_bytes()))
}

/// The weighted count: this window's calls and the share of the last's
/// still within `per` (in thousandths).
fn weighed(now: u64, before: u64, elapsed: i64, per: i64) -> f64 {
    now as f64 + before as f64 * (1.0 - elapsed as f64 / per as f64)
}

/// Seconds until a call would be allowed again.
fn retry_after(n: u64, now: u64, before: u64, elapsed: i64, per: i64) -> i64 {
    let (n, per_f, el) = (n as f64, per as f64, elapsed as f64);
    let ms = if now as f64 >= n {
        // Into the next window, until this one's share is small enough.
        (per_f - el) + per_f * (1.0 - n / now as f64).max(0.0)
    } else {
        // Until the last window's share is small enough.
        per_f * (1.0 - (n - now as f64) / before.max(1) as f64) - el
    };
    ((ms / 1000.0).ceil() as i64).max(1)
}

fn in_memory(key: &str, cost: u64, per: i64, t: i64) -> (u64, u64, i64) {
    let window = t.div_euclid(per);
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    let map = kept.get_or_insert_with(HashMap::new);
    // Now and then, what has run out goes.
    if map.len() > 10_000 {
        map.retain(|_, c| c.until > t);
    }
    let c = map.entry(key.to_string()).or_insert(Counts { window, now: 0, before: 0, until: 0 });
    if c.window != window {
        c.before = if c.window == window - 1 { c.now } else { 0 };
        c.now = 0;
        c.window = window;
    }
    c.now += cost;
    c.until = (window + 2) * per;
    (c.now, c.before, t - window * per)
}

fn table(mysql: bool) -> String {
    if mysql {
        "create table if not exists _quillo_limits (k varchar(64) not null, w bigint not null, n bigint not null, until bigint not null, primary key (k, w))".into()
    } else {
        "create table if not exists _quillo_limits (k text not null, w bigint not null, n bigint not null, until bigint not null, primary key (k, w))".into()
    }
}

fn in_db(it: &mut Interp, key: &str, cost: u64, per: i64, t: i64) -> Result<(u64, u64, i64), String> {
    let window = t.div_euclid(per);
    // Inside a transaction on SQLite the count is made after it (see
    // `db::own_sql`): this call's own cost is not in the rows read.
    let held = if cost > 0 && crate::db::own_waits(it) { cost } else { 0 };
    let mut counted = false;
    let mut run = |it: &mut Interp| -> Result<Vec<Value>, String> {
        // Held back, the count is made once, even when the table is made
        // and the rows read again.
        if cost > 0 && !(held > 0 && counted) {
            counted = true;
            crate::db::own_sql_made(
                it,
                |mysql| {
                    if mysql {
                        "insert into _quillo_limits (k, w, n, until) values (?, ?, ?, ?) on duplicate key update n = n + values(n)".into()
                    } else {
                        "insert into _quillo_limits (k, w, n, until) values (?, ?, ?, ?) on conflict (k, w) do update set n = _quillo_limits.n + excluded.n".into()
                    }
                },
                vec![Value::str(key), Value::Int(window), Value::Int(cost as i64), Value::Int((window + 2) * per)],
                Some(table),
            )?;
        }
        crate::db::own_sql(it, |_| "select w, n from _quillo_limits where k = ? and w >= ?".into(), vec![Value::str(key), Value::Int(window - 1)])
    };
    let rows = match run(it) {
        Ok(r) => r,
        Err(_) => {
            crate::db::own_sql(it, table, Vec::new())?;
            run(it)?
        }
    };
    let (mut now, mut before) = (held, 0);
    for r in rows {
        if let Value::Map(m) = r {
            let (w, n) = match (m.get("w"), m.get("n")) {
                (Some(Value::Int(w)), Some(Value::Int(n))) => (*w, *n as u64),
                _ => continue,
            };
            if w == window {
                now += n;
            } else if w == window - 1 {
                before = n;
            }
        }
    }
    // Now and then, windows that have run out go.
    use std::sync::atomic::{AtomicI64, Ordering::Relaxed};
    static SWEPT: AtomicI64 = AtomicI64::new(0);
    let last = SWEPT.load(Relaxed);
    if t - last >= 60_000 && SWEPT.compare_exchange(last, t, Relaxed, Relaxed).is_ok() {
        let _ = crate::db::own_sql(it, |_| "delete from _quillo_limits where until < ?".into(), vec![Value::Int(t)]);
    }
    Ok((now, before, t - window * per))
}

fn use_db(it: &Interp, opts: Option<&Map>, line: u32) -> Result<bool, Ctrl> {
    match opts.and_then(|m| m.get("store")) {
        None | Some(Value::Nil) => Ok(it.site.sessions.is_some()),
        Some(Value::Str(s)) if &**s == "db" => Ok(true),
        Some(Value::Str(s)) if &**s == "memory" => Ok(false),
        Some(v) => fail(line, format!("`rate_limit`'s store is \"memory\" or \"db\", got {}", v.repr())),
    }
}

/// `rate_limit(key, n, per, { cost: 1, reply: false, store })`: whether
/// this call is within `n` calls in `per` (a duration like "1m"). With
/// `reply`, a call over it answers 429 Too Many Requests, with Retry-After.
fn rate_limit(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let key = match args.first() {
        Some(Value::Str(s)) => s.to_string(),
        other => return fail(line, format!("`rate_limit` needs a key (a str such as \"login:\" + request.ip), got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let n = match args.get(1) {
        Some(Value::Int(n)) if *n > 0 => *n as u64,
        other => return fail(line, format!("`rate_limit` needs how many calls are allowed (an int above 0), got {}", other.map_or("nothing".into(), |v| v.repr()))),
    };
    let per = match args.get(2).and_then(crate::builtins::duration_seconds) {
        Some(s) if s > 0 => s as i64 * 1000,
        _ => return fail(line, "`rate_limit` needs a duration like \"1m\" or \"15m\" as its third argument"),
    };
    let opts = match args.get(3) {
        None | Some(Value::Nil) => None,
        Some(Value::Map(m)) => Some(m.clone()),
        Some(v) => return fail(line, format!("`rate_limit` takes options as a map, got {}", v.type_name())),
    };
    let cost = match opts.as_ref().and_then(|m| m.get("cost")) {
        None | Some(Value::Nil) => 1,
        Some(Value::Int(c)) if *c >= 0 => *c as u64,
        Some(v) => return fail(line, format!("`rate_limit`'s cost is an int, 0 or more, got {}", v.repr())),
    };
    let reply = opts.as_ref().and_then(|m| m.get("reply")).map_or(false, |v| v.truthy());
    let db = use_db(it, opts.as_deref(), line)?;
    let k = full_key(it, &key);
    let t = now_ms();
    let (now, before, elapsed) = if db {
        match in_db(it, &k, cost, per, t) {
            Ok(c) => c,
            Err(e) => return fail(line, format!("`rate_limit`: {}", e)),
        }
    } else {
        in_memory(&k, cost, per, t)
    };
    // With cost 0: whether one more call would be allowed.
    let counted = if cost == 0 { now + 1 } else { now };
    let allowed = weighed(counted, before, elapsed, per) <= n as f64;
    if !allowed && reply {
        let wait = retry_after(n, counted, before, elapsed, per);
        return Err(Ctrl::Reply(std::sync::Arc::new(Response {
            status: 429,
            headers: vec![("retry-after".into(), wait.to_string())],
            body: Body::Value(Value::str("Too many requests; try again later\n")),
        })));
    }
    Ok(Value::Bool(allowed))
}

/// `rate_limit_reset(key, { store })`: forgets the key's calls (after a
/// login that went well, say).
fn rate_limit_reset(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let key = match args.first() {
        Some(Value::Str(s)) => s.to_string(),
        other => return fail(line, format!("`rate_limit_reset` needs a key, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let opts = match args.get(1) {
        Some(Value::Map(m)) => Some(m.clone()),
        _ => None,
    };
    let db = use_db(it, opts.as_deref(), line)?;
    let k = full_key(it, &key);
    if let Some(m) = KEPT.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        m.remove(&k);
    }
    if db {
        let _ = crate::db::own_sql(it, |_| "delete from _quillo_limits where k = ?".into(), vec![Value::str(&k)]);
    }
    Ok(Value::Nil)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_slide() {
        // Half way through a window: half the last one still counts.
        assert_eq!(weighed(2, 4, 30_000, 60_000), 4.0);
        // 5 a minute, 5 used at the start of this window: the next is in a
        // minute and a little; 4 last window and 4 now, half way: in 15 s.
        assert_eq!(retry_after(5, 6, 0, 0, 60_000), 70);
        assert_eq!(retry_after(5, 4, 4, 30_000, 60_000), 15);
        let (a, b, _) = in_memory("test-key", 1, 60_000, 0);
        let (c, _, _) = in_memory("test-key", 1, 60_000, 1000);
        let (d, e, _) = in_memory("test-key", 1, 60_000, 61_000);
        assert_eq!((a, b, c, d, e), (1, 0, 2, 1, 2));
    }
}
