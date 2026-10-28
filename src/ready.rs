//! Readiness: whether what a program needs is there (FEEDBACK10.md, Q-076;
//! branch feedback10-fixes).
//!
//! `/_quillo/health` says the process is up. `/_quillo/ready` says whether
//! the program can do its work: its database answers `select 1`, each
//! service it declares is ready (one in this process asked as this one is,
//! down its own chain; one elsewhere asked at each of its addresses, briefly,
//! and the answer kept a second or two so a probe does not walk the chain
//! each time), and the checks the program adds with `ready(name, f)` hold.
//! 200 when all are, 503 otherwise, with each one's state as JSON. Only
//! states are said, not error texts: like `/_quillo/health`, anyone may ask.

use crate::interp::{Ctrl, Interp};
use crate::value::Value;
use serde_json::{json, Map, Value as J};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long an address's answer is kept.
const KEPT: Duration = Duration::from_secs(2);
/// How long an address is given to answer.
const WAIT: Duration = Duration::from_millis(1500);
/// How many programs deep in-process services are asked.
const DEPTH: u32 = 8;

/// Whether the program is ready, and each thing it needs, by name.
pub fn report(it: &mut Interp) -> (bool, J) {
    report_at(it, 0)
}

fn report_at(it: &mut Interp, depth: u32) -> (bool, J) {
    let mut checks = Map::new();
    let mut all = true;
    let mut put = |name: String, ok: bool, detail: J| {
        all &= ok;
        checks.insert(name, detail);
    };
    // The database, when the program opened one.
    if it.db.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        let ok = crate::parser::Parser::new("db.query(\"select 1\")")
            .parse_expr()
            .ok()
            .map(|e| {
                let g = it.globals.clone();
                matches!(it.eval(&e, &g), Ok(v) if !matches!(v, Value::Err(_)))
            })
            .unwrap_or(false);
        put("database".into(), ok, json!(if ok { "ready" } else { "down" }));
    }
    // Its services.
    let names: Vec<String> = it.declared_services.to_vec();
    for name in names {
        // Given addresses; or the service here, with the copies that joined
        // it (FEEDBACK10.md, Q-074).
        let configured = crate::service::configured_urls(&it.shared, &name);
        let urls = if configured.is_empty() { Vec::new() } else { crate::service::urls_of(&it.shared, &name) };
        if !urls.is_empty() {
            let at: Vec<(String, bool)> = it.unlocked(|| urls.iter().map(|u| (u.clone(), remote_ready(u))).collect());
            let ok = at.iter().any(|(_, r)| *r);
            let detail = at.into_iter().map(|(u, r)| (u, json!(if r { "ready" } else { "down" }))).collect::<Map<_, _>>();
            put(format!("service {}", name), ok, J::Object(detail));
            continue;
        }
        let local = it.services.loaded.lock().unwrap_or_else(|e| e.into_inner()).get(&name).map(|(_, s)| s.fork());
        let joined = crate::service::joined_urls(&name);
        match local {
            Some(mut svc) if depth < DEPTH => {
                let (ok, mut detail) = report_at(&mut svc, depth + 1);
                if !joined.is_empty() {
                    let at: Vec<(String, bool)> = it.unlocked(|| joined.iter().map(|u| (u.clone(), remote_ready(u))).collect());
                    detail["copies"] = J::Object(at.into_iter().map(|(u, r)| (u, json!(if r { "ready" } else { "down" }))).collect());
                }
                put(format!("service {}", name), ok, detail);
            }
            Some(_) => put(format!("service {}", name), true, json!("ready")),
            None => put(format!("service {}", name), false, json!("not loaded")),
        }
    }
    // The program's own checks.
    let own: Vec<(String, Value)> = it.ready_checks.to_vec();
    for (name, f) in own {
        let ok = matches!(it.call(&f, vec![], 0), Ok(Value::Bool(true)));
        put(name, ok, json!(if ok { "ready" } else { "not ready" }));
    }
    (all, json!({ "ready": all, "checks": checks }))
}

/// A service elsewhere, asked at one of its addresses, for `/<prefix>/_quillo/ready`.
pub fn service(it: &mut Interp, name: &str, url: &str) -> (bool, J) {
    let url = url.to_string();
    let ok = it.unlocked(move || remote_ready(&url));
    (ok, json!({ "ready": ok, "service": name }))
}

/// Whether the service at `url` says it is ready, asked at most every
/// `KEPT`.
fn remote_ready(url: &str) -> bool {
    static SEEN: OnceLock<Mutex<HashMap<String, (Instant, bool)>>> = OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);
    if let Some((at, ok)) = seen.lock().unwrap_or_else(|e| e.into_inner()).get(url) {
        if at.elapsed() < KEPT {
            return *ok;
        }
    }
    let agent = ureq::AgentBuilder::new().timeout(WAIT).redirects(0).build();
    let ok = matches!(agent.get(&format!("{}/_quillo/ready", url)).call(), Ok(r) if r.status() == 200);
    seen.lock().unwrap_or_else(|e| e.into_inner()).insert(url.to_string(), (Instant::now(), ok));
    ok
}

/// `ready(name, f)`: a check of the program's own, which holds when `f()`
/// gives true. Made where the program starts, at the top level.
pub fn add_check(it: &mut Interp, args: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
    match (args.first(), args.get(1)) {
        (Some(Value::Str(name)), Some(f @ (Value::Func(_) | Value::Builtin(..)))) => {
            let checks = std::sync::Arc::make_mut(&mut it.ready_checks);
            checks.retain(|(n, _)| **n != **name);
            checks.push((name.to_string(), f.clone()));
            Ok(Value::Nil)
        }
        _ => crate::interp::fail(line, "`ready` takes a name and a function that says whether it holds: ready(\"ledger\", () => ...)"),
    }
}
