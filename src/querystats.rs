//! How long database queries take: the last 100 of them, and the last 100 of each
//! statement, for `/_quillo/health/queries` (branch query-stats).
//!
//! Off unless asked for: on under `--watch`, with `--query-stats` or
//! `QUILLO_QUERY_STATS=1`, or switched on while running by a `POST` to the address
//! (with the health token). Off, recording is one atomic load and nothing else.
//!
//!   GET  /_quillo/health/queries            { enabled, window, queries: { count, errors, min_ms, max_ms, avg_ms } }
//!                                           and, with the token, `statements`: each statement's figures, slowest first
//!   POST /_quillo/health/queries?on=1|0     on or off (with the token; under --watch, from this machine too)
//!
//! The token is `QUILLO_HEALTH_TOKEN`, sent as `Authorization: Bearer <token>`. Without
//! it, statements (which show the program's SQL) are not shown: a server behind a
//! proxy sees every visitor coming from this machine, so the address proves nothing.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};

/// How many of the last queries the figures are of.
pub const WINDOW: usize = 100;
/// Statements kept at most; past that, the one used longest ago makes room.
const MAX_STATEMENTS: usize = 500;

pub static ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Statement {
    last: VecDeque<f64>,
    total: u64,
    errors: u64,
    /// When it last ran, as a count of queries, to find the one used longest ago.
    seen: u64,
}

#[derive(Default)]
struct Stats {
    last: VecDeque<f64>,
    total: u64,
    errors: u64,
    by: HashMap<String, Statement>,
}

fn stats() -> &'static Mutex<Stats> {
    static S: OnceLock<Mutex<Stats>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn push(q: &mut VecDeque<f64>, ms: f64) {
    if q.len() == WINDOW {
        q.pop_front();
    }
    q.push_back(ms);
}

/// The statement as one line, its spaces squeezed, at most 300 characters.
fn statement(sql: &str) -> String {
    let one: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    one.chars().take(300).collect()
}

/// One query that took `ms` milliseconds; `ok` false when it failed. Quillo's own
/// tables (`_quillo_jobs` and the like) are not counted.
pub fn record(sql: &str, ms: f64, ok: bool) {
    if !ENABLED.load(Relaxed) || sql.contains("_quillo_") {
        return;
    }
    let key = statement(sql);
    let mut s = stats().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    push(&mut s.last, ms);
    s.total += 1;
    if !ok {
        s.errors += 1;
    }
    let now = s.total;
    if !s.by.contains_key(&key) && s.by.len() >= MAX_STATEMENTS {
        if let Some(oldest) = s.by.iter().min_by_key(|(_, st)| st.seen).map(|(k, _)| k.clone()) {
            s.by.remove(&oldest);
        }
    }
    let st = s.by.entry(key).or_default();
    push(&mut st.last, ms);
    st.total += 1;
    st.seen = now;
    if !ok {
        st.errors += 1;
    }
}

fn round(ms: f64) -> f64 {
    (ms * 1000.0).round() / 1000.0
}

fn figures(q: &VecDeque<f64>) -> serde_json::Value {
    if q.is_empty() {
        return serde_json::json!({ "count": 0, "min_ms": null, "max_ms": null, "avg_ms": null });
    }
    let min = q.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = q.iter().cloned().fold(0.0, f64::max);
    let avg = q.iter().sum::<f64>() / q.len() as f64;
    serde_json::json!({ "count": q.len(), "min_ms": round(min), "max_ms": round(max), "avg_ms": round(avg) })
}

/// What the address answers: the figures, and each statement's when `detail`.
pub fn report(detail: bool) -> serde_json::Value {
    if !ENABLED.load(Relaxed) {
        return serde_json::json!({ "enabled": false,
            "how": "off: run with --query-stats or QUILLO_QUERY_STATS=1, or POST /_quillo/health/queries?on=1 with the health token" });
    }
    let s = stats().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut queries = figures(&s.last);
    queries["total"] = s.total.into();
    queries["errors"] = s.errors.into();
    let mut out = serde_json::json!({ "enabled": true, "window": WINDOW, "queries": queries });
    if detail {
        let mut list: Vec<(&String, &Statement)> = s.by.iter().collect();
        let avg = |st: &Statement| st.last.iter().sum::<f64>() / st.last.len().max(1) as f64;
        list.sort_by(|a, b| avg(b.1).partial_cmp(&avg(a.1)).unwrap_or(std::cmp::Ordering::Equal));
        out["statements"] = list.iter().take(100).map(|(sql, st)| {
            let mut f = figures(&st.last);
            f["sql"] = (*sql).clone().into();
            f["total"] = st.total.into();
            f["errors"] = st.errors.into();
            f
        }).collect::<Vec<_>>().into();
    } else {
        out["statements"] = "hidden: send the health token (Authorization: Bearer ...) to see each statement".into();
    }
    out
}

/// On or off; what was there is forgotten either way.
pub fn set(on: bool) {
    ENABLED.store(on, Relaxed);
    *stats().lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Stats::default();
}

/// Whether a request carries the health token (`QUILLO_HEALTH_TOKEN`), compared in
/// constant time. None set: no request does.
pub fn has_token(authorization: Option<&str>) -> bool {
    let Ok(want) = std::env::var("QUILLO_HEALTH_TOKEN") else { return false };
    if want.len() < 16 {
        return false;
    }
    let Some(given) = authorization.and_then(|a| a.strip_prefix("Bearer ")) else { return false };
    let (a, b) = (given.trim().as_bytes(), want.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_hundred() {
        set(true);
        for i in 0..150 {
            record("select  *\n from users where id = ?", i as f64, true);
        }
        record("insert into users (name) values (?)", 1000.0, false);
        record("select * from _quillo_jobs", 5.0, true);
        let r = report(true);
        assert_eq!(r["queries"]["count"], 100);
        assert_eq!(r["queries"]["total"], 151);
        assert_eq!(r["queries"]["errors"], 1);
        assert_eq!(r["queries"]["max_ms"], 1000.0);
        assert_eq!(r["queries"]["min_ms"], 51.0);
        let st = r["statements"].as_array().unwrap();
        assert_eq!(st[0]["sql"], "insert into users (name) values (?)");
        assert_eq!(st[1]["sql"], "select * from users where id = ?");
        assert_eq!(st[1]["count"], 100);
        assert_eq!(st[1]["min_ms"], 50.0);
        assert_eq!(st[1]["avg_ms"], 99.5);
        assert!(report(false)["statements"].is_string());
        set(false);
        assert_eq!(report(false)["enabled"], false);
        record("select 1", 1.0, true);
        set(true);
        assert_eq!(report(false)["queries"]["count"], 0);
        set(false);
    }
}
