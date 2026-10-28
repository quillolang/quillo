//! Live data without a whole run and a whole answer for each page
//! (FEEDBACK2.md, Q-028; branch feedback6-fixes).
//!
//! When something a `live` loader read changes, every page showing it asks
//! again. Two things keep that cheap:
//!
//! - Shared runs. A loader's answer is kept, with the topics it read, for a
//!   moment (`SHARED_FOR`): the other pages asking for the same function
//!   with the same arguments while none of those topics changed get it
//!   without running the loader again. A run that read the request (the
//!   session, cookies, headers, the locale) is shared only with the same
//!   viewer's pages; one that wrote, set a cookie or answered with a
//!   response of its own is not kept.
//! - Patches. Each live answer carries a hash of the value. The page sends
//!   back the hash of the value it has (`x-quillo-had`), and when the server
//!   still knows that value, the answer is what changed instead of the whole
//!   value. Lists of rows are matched by `id`, so a row moved, added or
//!   changed costs that row, not the list.
//!
//! The patch format, which src/client/runtime.js `livePatched` applies:
//!
//! - `{"$=": 1}`: the same.
//! - `{"$v": value}`: this value instead.
//! - `{"$m": {key: patch, ...}, "$x": [key, ...]}`: a map with those keys
//!   changed or added, and the `$x` ones gone; the others as they were.
//! - `{"$l": [op, ...]}`: a list, made of: `k` (the old list's item k),
//!   `[k, n]` (its n items from k), `{"$p": k, "d": patch}` (item k changed),
//!   `{"$v": value}` (a new item).
//!
//! Tagged values (`{"$dec": "1.50"}` and the like) are replaced whole.

use serde_json::{json, Map, Value as J};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a run is shared: the pages told of one change ask within it.
pub const SHARED_FOR: Duration = Duration::from_secs(2);

/// Values kept to make patches against, in bytes of JSON.
const KEPT_BYTES: usize = 64 << 20;

#[derive(Default)]
pub struct LiveCache {
    runs: HashMap<RunKey, Vec<Run>>,
    values: HashMap<u64, Kept>,
    bytes: usize,
    tick: u64,
    patches: HashMap<(u64, u64), Arc<J>>,
}

/// The function, its arguments as the page sent them, and the host (each
/// tenant has its own data).
pub type RunKey = (String, Vec<u8>, String);

struct Run {
    /// The viewer, when the run read the request.
    viewer: Option<u64>,
    seq: u64,
    topics: Vec<String>,
    at: Instant,
    answer: Arc<Answer>,
}

/// What a loader gave: its value as JSON, the text of it, and its hash.
pub struct Answer {
    pub value: Arc<J>,
    pub text: String,
    pub h: u64,
    /// An error value: sent as `err`, and never patched.
    pub err: bool,
}

impl Answer {
    pub fn new(value: J, err: bool) -> Self {
        let text = value.to_string();
        Answer { h: hash(&text), value: Arc::new(value), text, err }
    }
}

struct Kept {
    value: Arc<J>,
    size: usize,
    used: u64,
}

impl LiveCache {
    /// A run another page made that this one may have: the answer, its
    /// topics and the sequence number it was made at.
    pub fn shared(&mut self, key: &RunKey, viewer: u64, unchanged: impl Fn(&[String], u64) -> bool) -> Option<(Arc<Answer>, Vec<String>, u64)> {
        let runs = self.runs.get_mut(key)?;
        runs.retain(|r| r.at.elapsed() < SHARED_FOR);
        let r = runs.iter().rev().find(|r| r.viewer.map_or(true, |v| v == viewer) && unchanged(&r.topics, r.seq))?;
        Some((r.answer.clone(), r.topics.clone(), r.seq))
    }

    pub fn share(&mut self, key: RunKey, viewer: Option<u64>, seq: u64, topics: Vec<String>, answer: Arc<Answer>) {
        if self.runs.len() > 256 {
            self.runs.retain(|_, rs| {
                rs.retain(|r| r.at.elapsed() < SHARED_FOR);
                !rs.is_empty()
            });
        }
        let runs = self.runs.entry(key).or_default();
        runs.retain(|r| r.at.elapsed() < SHARED_FOR && r.viewer != viewer);
        runs.push(Run { viewer, seq, topics, at: Instant::now(), answer });
    }

    /// Keeps a value a page now has, by its hash, for patches against it.
    pub fn keep(&mut self, h: u64, value: Arc<J>, size: usize) {
        self.tick += 1;
        if let Some(k) = self.values.get_mut(&h) {
            k.used = self.tick;
            return;
        }
        if size > KEPT_BYTES / 4 {
            return;
        }
        self.bytes += size;
        self.values.insert(h, Kept { value, size, used: self.tick });
        while self.bytes > KEPT_BYTES {
            let Some((&old, _)) = self.values.iter().min_by_key(|(_, k)| k.used) else { break };
            if let Some(k) = self.values.remove(&old) {
                self.bytes -= k.size;
            }
            self.patches.retain(|(a, b), _| *a != old && *b != old);
        }
    }

    /// The patch from the value hashed `from` to `to` (which is `new`), when
    /// `from` is still known; made once for all the pages that had it.
    pub fn patch(&mut self, from: u64, to: u64, new: &J) -> Option<Arc<J>> {
        if from == to {
            return Some(Arc::new(json!({ "$=": 1 })));
        }
        if let Some(p) = self.patches.get(&(from, to)) {
            return Some(p.clone());
        }
        let old = self.values.get(&from)?.value.clone();
        let p = Arc::new(diff(&old, new));
        if self.patches.len() > 256 {
            self.patches.clear();
        }
        self.patches.insert((from, to), p.clone());
        Some(p)
    }
}

/// The hash a value's JSON text is known by.
pub fn hash(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

pub fn hash_text(h: u64) -> String {
    format!("{:016x}", h)
}

pub fn parse_hash(s: &str) -> Option<u64> {
    u64::from_str_radix(s.trim(), 16).ok()
}

/// What is replaced whole: scalars, and tagged values (`{"$dec": ...}`).
fn atom(v: &J) -> bool {
    match v {
        J::Object(o) => o.len() == 1 && o.keys().next().map_or(false, |k| k.starts_with('$')),
        J::Array(_) => false,
        _ => true,
    }
}

/// The patch that makes `new` out of `old`.
pub fn diff(old: &J, new: &J) -> J {
    if old == new {
        return json!({ "$=": 1 });
    }
    if atom(old) || atom(new) {
        return json!({ "$v": new });
    }
    match (old, new) {
        (J::Object(a), J::Object(b)) => {
            // The keys keep their order in the browser: the old ones that
            // stay, then the new ones. Otherwise the map goes whole.
            let order = a.keys().filter(|k| b.contains_key(*k)).chain(b.keys().filter(|k| !a.contains_key(*k)));
            if !order.eq(b.keys()) {
                return json!({ "$v": new });
            }
            let mut m = Map::new();
            for (k, v) in b {
                match a.get(k) {
                    Some(o) if o == v => {}
                    Some(o) => {
                        m.insert(k.clone(), diff(o, v));
                    }
                    None => {
                        m.insert(k.clone(), json!({ "$v": v }));
                    }
                }
            }
            let gone: Vec<&String> = a.keys().filter(|k| !b.contains_key(*k)).collect();
            let mut p = Map::new();
            p.insert("$m".into(), J::Object(m));
            if !gone.is_empty() {
                p.insert("$x".into(), json!(gone));
            }
            J::Object(p)
        }
        (J::Array(a), J::Array(b)) => list_diff(a, b),
        _ => json!({ "$v": new }),
    }
}

/// A row's `id`, as text, when it has one.
fn id_of(v: &J) -> Option<String> {
    match v {
        J::Object(o) if !atom(v) => o.get("id").filter(|i| i.is_number() || i.is_string()).map(|i| i.to_string()),
        _ => None,
    }
}

fn list_diff(a: &[J], b: &[J]) -> J {
    let mut by_id: HashMap<String, usize> = HashMap::new();
    for (k, x) in a.iter().enumerate() {
        if let Some(id) = id_of(x) {
            by_id.entry(id).or_insert(k);
        }
    }
    // Items without an id are found by their text, worked out only if
    // one is not where it was.
    let mut by_text: Option<HashMap<String, usize>> = None;
    let mut ops: Vec<J> = Vec::new();
    let mut run: Option<(usize, usize)> = None;
    let flush = |run: &mut Option<(usize, usize)>, ops: &mut Vec<J>| match run.take() {
        Some((k, 1)) => ops.push(json!(k)),
        Some((k, n)) => ops.push(json!([k, n])),
        None => {}
    };
    for (j, x) in b.iter().enumerate() {
        let id = id_of(x);
        let k = if id.is_none() && a.get(j) == Some(x) {
            Some(j)
        } else if let Some(id) = &id {
            by_id.get(id).copied()
        } else {
            let t = by_text.get_or_insert_with(|| {
                let mut m = HashMap::new();
                for (k, y) in a.iter().enumerate() {
                    if id_of(y).is_none() {
                        m.entry(y.to_string()).or_insert(k);
                    }
                }
                m
            });
            t.get(&x.to_string()).copied()
        };
        match k {
            Some(k) if a[k] == *x => match &mut run {
                Some((s, n)) if *s + *n == k => *n += 1,
                _ => {
                    flush(&mut run, &mut ops);
                    run = Some((k, 1));
                }
            },
            Some(k) => {
                flush(&mut run, &mut ops);
                ops.push(json!({ "$p": k, "d": diff(&a[k], x) }));
            }
            None => {
                flush(&mut run, &mut ops);
                ops.push(json!({ "$v": x }));
            }
        }
    }
    flush(&mut run, &mut ops);
    json!({ "$l": ops })
}

/// Applies a patch, as the browser does (for tests).
pub fn apply(old: &J, p: &J) -> J {
    if p.get("$=").is_some() {
        return old.clone();
    }
    if let Some(v) = p.get("$v") {
        return v.clone();
    }
    if let (Some(J::Object(m)), J::Object(o)) = (p.get("$m"), old) {
        let gone: Vec<&str> = p.get("$x").and_then(|x| x.as_array()).map_or(vec![], |x| x.iter().filter_map(|k| k.as_str()).collect());
        let mut out = Map::new();
        for (k, v) in o {
            if !gone.contains(&k.as_str()) {
                out.insert(k.clone(), m.get(k).map_or_else(|| v.clone(), |d| apply(v, d)));
            }
        }
        for (k, d) in m {
            if !o.contains_key(k) {
                out.insert(k.clone(), apply(&J::Null, d));
            }
        }
        return J::Object(out);
    }
    if let (Some(J::Array(ops)), J::Array(a)) = (p.get("$l"), old) {
        let mut out = Vec::new();
        for op in ops {
            match op {
                J::Number(k) => out.push(a[k.as_u64().unwrap_or(0) as usize].clone()),
                J::Array(r) => {
                    let (k, n) = (r[0].as_u64().unwrap_or(0) as usize, r[1].as_u64().unwrap_or(0) as usize);
                    out.extend(a[k..k + n].iter().cloned());
                }
                _ => match (op.get("$p").and_then(|k| k.as_u64()), op.get("d"), op.get("$v")) {
                    (Some(k), Some(d), _) => out.push(apply(&a[k as usize], d)),
                    (_, _, Some(v)) => out.push(v.clone()),
                    _ => {}
                },
            }
        }
        return J::Array(out);
    }
    old.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(a: J, b: J) -> J {
        let p = diff(&a, &b);
        assert_eq!(apply(&a, &p), b, "patch {}", p);
        p
    }

    #[test]
    fn patches_make_the_new_value() {
        assert_eq!(round(json!(1), json!(1)), json!({ "$=": 1 }));
        round(json!({ "a": 1, "b": [1, 2] }), json!({ "a": 1, "b": [1, 2, 3], "c": { "$dec": "1.50" } }));
        round(json!({ "a": 1, "b": 2 }), json!({ "b": 2, "a": 1 }));
        round(json!({ "a": 1, "b": 2 }), json!({ "b": 3 }));
        round(json!({ "$dec": "1.50" }), json!({ "$dec": "2.50" }));
        round(json!([1, 2, 3, 4]), json!([4, 1, 2, 3, 5]));
        round(json!([{ "x": 1 }, { "x": 2 }]), json!([{ "x": 2 }, { "x": 3 }]));
        round(json!(null), json!([1]));
    }

    #[test]
    fn a_moved_row_costs_that_row() {
        let rows: Vec<J> = (0..3000).map(|i| json!({ "id": i, "title": format!("Card {}", i), "column": i % 5 })).collect();
        let mut moved = rows.clone();
        let r = moved.remove(10);
        moved.insert(2500, json!({ "id": 10, "title": "Card 10", "column": 4 }));
        let _ = r;
        let p = round(J::Array(rows.clone()), J::Array(moved));
        assert!(p.to_string().len() < 200, "{}", p);
        let mut changed = rows.clone();
        changed[7]["title"] = json!("Renamed");
        let p = round(J::Array(rows), J::Array(changed));
        assert_eq!(p, json!({ "$l": [[0, 7], { "$p": 7, "d": { "$m": { "title": { "$v": "Renamed" } } } }, [8, 2992]] }));
    }
}
