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
//!   session, cookies, headers, the locale), wrote anything, made something
//!   of its own (`viewcache::Kind::Acts`), set a cookie or answered with a
//!   response of its own is not kept, for anyone: what told viewers apart
//!   was a list of what identifies nobody, and `Referer` was on it.
//! - Patches. Each live answer carries a hash of the value. The page sends
//!   back the hash of the value it has (`x-quillo-had`), and when the server
//!   still knows that value, the answer is what changed instead of the whole
//!   value. Lists of rows are matched by `id`, so a row moved, added or
//!   changed costs that row, not the list. A viewer's value is kept for that
//!   viewer, a tenant's for that tenant, and the hashes have a key nobody
//!   outside can work out: nobody else can make one, nor be answered against it.
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
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long a run is shared: the pages told of one change ask within it.
pub const SHARED_FOR: Duration = Duration::from_secs(2);

/// Values kept to make patches against, and patches, in bytes.
const KEPT_BYTES: usize = 64 << 20;
/// What keeping a value costs besides its text, as near as it can be said: its place in the map
/// and its allocation. Counted by their text alone, a million values of `1` were nothing, and the
/// process grew without end (FEEDBACK55.md, Q-884, found in review; branch feedback55).
const KEPT_ENTRY: usize = 160;
/// How many values are looked at to choose the one to let go.
const CANDIDATES: usize = 4;

/// Whose a kept value is, when it read nobody, in a program with no tenants or mounts (see
/// `everyone`): a viewer's is under their `Interp::who`.
pub const EVERYONE: u64 = 0;

/// Whose a run for everyone is, in `scope`: everyone there. Another tenant's are not theirs.
pub fn everyone(scope: &str) -> u64 {
    if scope.is_empty() {
        EVERYONE
    } else {
        hash(scope)
    }
}

#[derive(Default)]
pub struct LiveCache {
    runs: HashMap<RunKey, Run>,
    /// By the value's hash and whose it is: one store for all viewers answered a page that said
    /// it had any value, and the patch told it what that viewer's held, so that Bob found Alice's
    /// PIN with hashes he worked out at home (FEEDBACK55.md, Q-884; branch feedback55).
    values: HashMap<(u64, u64), Kept>,
    bytes: usize,
    patches: HashMap<((u64, u64), u64), Arc<str>>,
    patch_bytes: usize,
    swept: Option<Instant>,
    /// Runs under way, and the runs known to read the request or to act, which cannot be
    /// given to anyone, so that nothing waits for them.
    flights: HashMap<RunKey, Arc<Flight>>,
    /// By the hash of the key, as a key holds its arguments: 4,096 of a megabyte each were 4 GB
    /// (FEEDBACK55.md, Q-881, found in review; branch feedback55).
    personal: HashSet<u64>,
}

/// A run under way. On a database that lets go of the lock while it works, the
/// next page's call would find no answer and start the same query, up to a
/// server's worth of them: those wait for this one and take its answer
/// (FEEDBACK24.md, Q-191; branch feedback24-fixes).
#[derive(Default)]
pub struct Flight {
    done: Mutex<bool>,
    ended: Condvar,
}

impl Flight {
    pub fn wait(&self, longest: Duration) {
        let done = self.done.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = self.ended.wait_timeout_while(done, longest, |d| !*d);
    }

    fn end(&self) {
        *self.done.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.ended.notify_all();
    }
}

/// A run a page may have: the answer, its topics and its sequence number.
pub type Shared = (Arc<Answer>, Vec<String>, u64);

/// What a call with no answer to share does.
pub enum Begun {
    /// It runs the loader.
    Run,
    /// It waits for the run under way.
    Wait(Arc<Flight>),
    /// It runs, beside the others: its run is nobody else's.
    Alone,
}

/// Whose it is (`Interp::run_scope`: the program and the tenant in force, however it was named),
/// the function, its arguments as the page sent them, and the host.
pub type RunKey = (String, String, Vec<u8>, String);

struct Run {
    seq: u64,
    topics: Vec<String>,
    at: Instant,
    answer: Arc<Answer>,
}

/// What a loader gave: its value as JSON, the text of it, and its hash.
pub struct Answer {
    pub value: Arc<J>,
    pub text: Arc<str>,
    pub h: u64,
    /// An error value: sent as `err`, and never patched.
    pub err: bool,
    /// The answer as it goes out gzipped, and the `seq` in it: a thousand pages
    /// sharing a run are not a thousand gzips (FEEDBACK24.md, Q-193; branch
    /// feedback24-fixes).
    pub gz: Mutex<Option<(u64, Arc<Vec<u8>>)>>,
}

impl Answer {
    pub fn new(value: J, err: bool) -> Self {
        let text: Arc<str> = value.to_string().into();
        Answer { h: hash(&text), value: Arc::new(value), text, err, gz: Mutex::new(None) }
    }
}

/// As text, not parsed: what the budget counts is then what is held, several
/// times less than a parsed value (FEEDBACK24.md, Q-193; branch feedback24-fixes).
struct Kept {
    text: Arc<str>,
    uses: u64,
}

impl LiveCache {
    /// A run another page made that this one may have: the answer, its
    /// topics and the sequence number it was made at.
    pub fn shared(&mut self, key: &RunKey, unchanged: impl Fn(&[String], u64) -> bool) -> Option<Shared> {
        let r = self.runs.get(key)?;
        if r.at.elapsed() >= SHARED_FOR {
            self.runs.remove(key);
            return None;
        }
        unchanged(&r.topics, r.seq).then(|| (r.answer.clone(), r.topics.clone(), r.seq))
    }

    /// A call with no answer to share, for `key`.
    pub fn begin(&mut self, key: &RunKey) -> Begun {
        if self.personal.contains(&key_hash(key)) {
            return Begun::Alone;
        }
        if let Some(f) = self.flights.get(key) {
            return Begun::Wait(f.clone());
        }
        self.flights.insert(key.clone(), Default::default());
        Begun::Run
    }

    /// The run `begin` said to make has ended, whatever became of it; `personal`
    /// if it can be nobody else's.
    pub fn land(&mut self, key: &RunKey, personal: bool) {
        if let Some(f) = self.flights.remove(key) {
            f.end();
        }
        if personal {
            if self.personal.len() >= 4096 {
                self.personal.clear();
            }
            self.personal.insert(key_hash(key));
        } else {
            self.personal.remove(&key_hash(key));
        }
    }

    pub fn share(&mut self, key: RunKey, seq: u64, topics: Vec<String>, answer: Arc<Answer>) {
        if self.runs.len() > 256 && self.swept.map_or(true, |t| t.elapsed() >= Duration::from_secs(1)) {
            self.swept = Some(Instant::now());
            self.runs.retain(|_, r| r.at.elapsed() < SHARED_FOR);
        }
        self.runs.insert(key, Run { seq, topics, at: Instant::now(), answer });
    }

    /// Drops the shared runs that read a topic under one of `wildcards` (`db:*`).
    pub fn forget_under(&mut self, wildcards: &[&str]) {
        self.runs.retain(|_, r| !r.topics.iter().any(|t| wildcards.iter().any(|w| crate::shared::under(w, t))));
    }

    /// Keeps a value a page now has, by its hash, for patches against it: `owner` is whose it is,
    /// `everyone(scope)` or the viewer's `who`.
    pub fn keep(&mut self, h: u64, text: &Arc<str>, owner: u64) {
        let at = (h, owner);
        if let Some(k) = self.values.get_mut(&at) {
            k.uses += 1;
            return;
        }
        if text.len() > KEPT_BYTES / 4 {
            return;
        }
        self.bytes += text.len() + KEPT_ENTRY;
        self.values.insert(at, Kept { text: text.clone(), uses: 1 });
        while self.bytes + self.patch_bytes > KEPT_BYTES {
            if self.patch_bytes > 0 {
                self.patches.clear();
                self.patch_bytes = 0;
                continue;
            }
            let Some(old) = self.least_used(at) else { break };
            if let Some(k) = self.values.remove(&old) {
                self.bytes -= k.text.len() + KEPT_ENTRY;
            }
        }
    }

    /// The value to let go. Pages refresh in turn, so the oldest is the next to
    /// be asked for: dropping it, past the budget, would leave no patches at all
    /// where some random ones cost a part. Of a few, the one least used goes, as
    /// a value many pages have is worth more than one for a single viewer.
    fn least_used(&self, not: (u64, u64)) -> Option<(u64, u64)> {
        let from = fastrand::usize(..self.values.len().min(1024));
        self.values.iter().skip(from).chain(self.values.iter()).filter(|(h, _)| **h != not).take(CANDIDATES).min_by_key(|(_, k)| k.uses).map(|(h, _)| *h)
    }

    /// The patch from the value hashed `from` to `to` (which is `new`), as text,
    /// when `from` is still known and is one of `owners`' (the asker's, and everyone's where they
    /// are); made once for all the pages that had it. Another viewer's or another tenant's is not
    /// there for them, whatever hash they bring: the answer is the same, and takes the same
    /// lookups, as for a value nobody has (FEEDBACK55.md, Q-884; branch feedback55).
    pub fn patch(&mut self, from: u64, to: u64, new: &J, owners: [u64; 2]) -> Option<Arc<str>> {
        if from == to {
            return Some(Arc::from("{\"$=\":1}"));
        }
        let at = owners.into_iter().map(|o| (from, o)).find(|at| self.values.contains_key(at))?;
        if let Some(p) = self.patches.get(&(at, to)) {
            return Some(p.clone());
        }
        let kept = self.values.get_mut(&at)?;
        kept.uses += 1;
        let old: J = serde_json::from_str(&kept.text).ok()?;
        let p: Arc<str> = diff(&old, new).to_string().into();
        if self.patches.len() > 256 {
            self.patches.clear();
            self.patch_bytes = 0;
        }
        self.patch_bytes += p.len();
        self.patches.insert((at, to), p.clone());
        Some(p)
    }
}

/// The key of what is hashed here, from the site's secret: servers behind a balancer sharing
/// `QUILLO_SECRET` agree on a value's hash, so a page that moves to another still gets a patch.
/// Without one, a random key for this process. `DefaultHasher::new()` has no key, and anyone
/// could work out the hash of a guess (FEEDBACK55.md, Q-884; branch feedback55).
fn key_of(secret: &[u8]) -> (u64, u64) {
    let k = crate::crypto::hmac_sha256(secret, b"quillo live values");
    let half = |at: usize| k[at..at + 8].iter().fold(0u64, |n, b| n << 8 | *b as u64);
    (half(0), half(8))
}

/// A hasher with that key.
#[allow(deprecated)]
pub fn hasher() -> std::hash::SipHasher {
    static KEY: std::sync::OnceLock<(u64, u64)> = std::sync::OnceLock::new();
    let (a, b) = *KEY.get_or_init(|| match std::env::var("QUILLO_SECRET") {
        Ok(s) if !s.is_empty() => key_of(s.as_bytes()),
        _ => key_of(&crate::crypto::secret_bytes(32)),
    });
    std::hash::SipHasher::new_with_keys(a, b)
}

fn key_hash(key: &RunKey) -> u64 {
    let mut h = hasher();
    key.hash(&mut h);
    h.finish()
}

/// The hash a value's JSON text is known by.
pub fn hash(text: &str) -> u64 {
    let mut h = hasher();
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
    if same(old, new) {
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
                    Some(o) if same(o, v) => {}
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

/// `==`, but a map with its keys in another order differs: `serde_json` ignores
/// the order, and a page that loops over a map shows it (FEEDBACK16.md, Q-121;
/// branch feedback20-fixes).
fn same(a: &J, b: &J) -> bool {
    match (a, b) {
        (J::Object(x), J::Object(y)) => x.len() == y.len() && x.iter().zip(y).all(|((k, v), (l, w))| k == l && same(v, w)),
        (J::Array(x), J::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(v, w)| same(v, w)),
        _ => a == b,
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
        let k = if id.is_none() && a.get(j).is_some_and(|y| same(y, x)) {
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
            Some(k) if same(&a[k], x) => match &mut run {
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

    /// Pages refreshing in turn, more of them than there is room for: taking the
    /// oldest would leave none (FEEDBACK24.md, Q-193; branch feedback24-fixes).
    #[test]
    fn a_full_store_still_has_most_of_what_fits() {
        let text = |i: u64| -> Arc<str> { format!("[{}{}]", i, " ".repeat(1 << 20)).into() };
        let mut c = LiveCache::default();
        let (mut found, mut asked) = (0, 0);
        for round in 0..6 {
            for i in 0..100u64 {
                asked += (round > 2) as u32;
                found += (round > 2 && c.values.contains_key(&(i, EVERYONE))) as u32;
                c.keep(i, &text(i), EVERYONE);
            }
        }
        assert!(c.bytes <= KEPT_BYTES);
        assert!(found * 100 / asked >= 35, "{} of {}", found, asked);
    }

    #[test]
    fn a_value_many_pages_have_is_the_last_to_go() {
        let text = |i: u64| -> Arc<str> { format!("[{}{}]", i, " ".repeat(1 << 20)).into() };
        let mut c = LiveCache::default();
        for _ in 0..50 {
            c.keep(1_000_000, &text(1_000_000), EVERYONE);
        }
        for i in 0..300 {
            c.keep(i, &text(i), EVERYONE);
        }
        assert!(c.values.contains_key(&(1_000_000, EVERYONE)));
    }

    #[test]
    fn patches_are_counted_and_let_go_first() {
        let mut c = LiveCache::default();
        let a: Arc<str> = "[1]".into();
        c.keep(1, &a, EVERYONE);
        let p = c.patch(1, 2, &json!([1, 2]), [7, EVERYONE]).unwrap();
        assert_eq!(c.patch_bytes, p.len());
        assert_eq!(c.patch(1, 1, &json!([1]), [7, EVERYONE]).unwrap().as_ref(), "{\"$=\":1}");
        let big: Arc<str> = " ".repeat(KEPT_BYTES / 4).into();
        for i in 10..15 {
            c.keep(i, &big, EVERYONE);
        }
        assert_eq!(c.patch_bytes, 0);
    }

    /// A viewer's value is for them, and a patch made for them is not another's (FEEDBACK55.md,
    /// Q-884; branch feedback55).
    #[test]
    fn a_viewers_value_is_no_one_elses() {
        let mut c = LiveCache::default();
        let a: Arc<str> = "[1]".into();
        c.keep(1, &a, 7);
        assert!(c.patch(1, 2, &json!([1, 2]), [8, EVERYONE]).is_none());
        assert!(c.patch(1, 2, &json!([1, 2]), [EVERYONE, EVERYONE]).is_none());
        assert!(c.patch(1, 2, &json!([1, 2]), [7, EVERYONE]).is_some());
        // Made for 7 and kept: still not 8's.
        assert!(c.patch(1, 2, &json!([1, 2]), [8, EVERYONE]).is_none());
        // The same value kept for everyone as well is everyone's, and the same for each.
        c.keep(1, &a, EVERYONE);
        assert!(c.patch(1, 2, &json!([1, 2]), [8, EVERYONE]).is_some());
    }

    /// One tenant's value for everyone there is not the next tenant's.
    #[test]
    fn a_tenants_value_is_no_other_tenants() {
        let mut c = LiveCache::default();
        let a: Arc<str> = "[1]".into();
        c.keep(1, &a, everyone("\0acme"));
        assert!(c.patch(1, 2, &json!([1, 2]), [everyone("\0globex"); 2]).is_none());
        assert!(c.patch(1, 2, &json!([1, 2]), [EVERYONE; 2]).is_none());
        assert!(c.patch(1, 2, &json!([1, 2]), [everyone("\0acme"); 2]).is_some());
        assert_eq!(everyone(""), EVERYONE);
    }

    /// Each value costs what it holds, however small: the store cannot be filled by the million
    /// with values it counted as nothing (FEEDBACK55.md, Q-884, found in review; branch feedback55).
    #[test]
    fn small_values_are_counted_too() {
        let mut c = LiveCache::default();
        let one: Arc<str> = "1".into();
        for i in 0..(KEPT_BYTES / KEPT_ENTRY) as u64 + 5000 {
            c.keep(1, &one, i + 1);
        }
        assert!(c.values.len() <= KEPT_BYTES / KEPT_ENTRY, "{}", c.values.len());
        assert!(c.bytes <= KEPT_BYTES);
        // And all of it is given back as values go.
        assert_eq!(c.bytes, c.values.len() * (KEPT_ENTRY + 1));
    }

    /// A key known to be nobody else's is remembered, not kept: its arguments may be megabytes.
    #[test]
    fn what_is_known_of_a_key_holds_no_argument() {
        let mut c = LiveCache::default();
        let key = |n: u8| (String::new(), "f".to_string(), vec![n; 1 << 20], String::new());
        assert!(matches!(c.begin(&key(1)), Begun::Run));
        c.land(&key(1), true);
        assert!(matches!(c.begin(&key(1)), Begun::Alone));
        assert!(matches!(c.begin(&key(2)), Begun::Run));
        c.land(&key(2), false);
        assert!(matches!(c.begin(&key(2)), Begun::Run));
        c.land(&key(2), false);
        assert_eq!(std::mem::size_of_val(c.personal.iter().next().unwrap()), 8);
    }

    /// Two servers with one secret agree; with another, or none, they do not.
    #[test]
    fn hashes_are_keyed_by_the_sites_secret() {
        assert_eq!(key_of(b"one"), key_of(b"one"));
        assert_ne!(key_of(b"one"), key_of(b"two"));
        let mut plain = std::collections::hash_map::DefaultHasher::new();
        "[1]".hash(&mut plain);
        assert_ne!(hash("[1]"), plain.finish());
        assert_eq!(hash("[1]"), hash("[1]"));
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
