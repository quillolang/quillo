//! Several Quillo servers running one app behind a load balancer
//! ([limer](https://limer.sh)).
//!
//! `quillo run app.qwe --port 8081 --register http://lb:2019` makes this
//! server a member of limer's `discover` pool for the app: it registers, and
//! keeps registering as a heartbeat. Pages connected to one server must also
//! hear about changes made through another, so each server sends the topics
//! it changes to limer's `POST /broadcast`, which hands them to every member
//! at `/_quillo/announce`:
//!
//! ```text
//! app.example.com {
//!     reverse_proxy {
//!         discover app {
//!             allow 10.0.0.0/8
//!             broadcast /_quillo/announce
//!         }
//!     }
//! }
//! ```
//!
//! Only shared data is announced: database tables and `publish` topics.
//! Variables belong to each server. Each message is signed (HMAC-SHA256) with
//! a key derived from limer's admin secret, with the time and a random
//! nonce: only servers that know the secret can make pages reload, a message
//! is taken once, and only for a minute. A server takes only the topics it
//! already knows, so a message cannot make it remember new ones.

use crate::shared::Shared;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Where to register, and as what.
#[derive(Clone)]
pub struct Membership {
    /// limer's admin API, such as `http://127.0.0.1:2019`.
    pub admin: String,
    /// The `discover` pool's name.
    pub service: String,
    /// The port this server listens on (limer takes the address from the
    /// connection).
    pub port: u16,
    /// limer's admin secret (`LIMER_ADMIN_SECRET`): required.
    pub secret: String,
}

/// How long limer keeps a member that stops heartbeating, in seconds.
const TTL: u64 = 15;

/// How old an announcement may be (seconds), either way: clocks differ a little.
const WINDOW: u64 = 60;

/// Most topics in one announcement; more go in several.
pub const MAX_TOPICS: usize = 256;

impl Membership {
    /// The key announcements are signed with, derived from the secret, so
    /// the secret itself is never sent to every member.
    pub fn key(&self) -> String {
        announce_key(&self.secret)
    }

    fn post(&self, path_and_query: &str, body: Option<&str>) -> Result<(), String> {
        let rq = ureq::post(&format!("{}{}", self.admin.trim_end_matches('/'), path_and_query))
            .timeout(Duration::from_secs(5))
            .set("authorization", &format!("Bearer {}", self.secret));
        let r = match body {
            Some(b) => rq.set("content-type", "application/json").send_string(b),
            None => rq.call(),
        };
        match r {
            Ok(_) => Ok(()),
            Err(ureq::Error::Status(code, resp)) => {
                Err(format!("{} {}", code, resp.into_string().unwrap_or_default().trim()))
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Set once the server leaves the pool: the heartbeat stops.
static LEAVING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Membership {
    /// Leaves the pool, so limer sends no more requests here (when the
    /// server stops).
    pub fn leave(&self) {
        LEAVING.store(true, std::sync::atomic::Ordering::SeqCst);
        let path = format!("/deregister?service={}&port={}", encode(&self.service), self.port);
        match self.post(&path, None) {
            Ok(()) => eprintln!("quillo: left `{}` at {}", self.service, self.admin),
            Err(e) => eprintln!("quillo: cannot leave `{}` at {}: {} (it drops this server when the heartbeat stops)", self.service, self.admin, e),
        }
    }
}

/// Joins the pool and starts telling the other members about changes.
pub fn join(shared: Arc<Shared>, m: Membership) {
    let heartbeat = m.clone();
    let _ = std::thread::Builder::new().name("quillo-register".into()).spawn(move || {
        let path = format!("/register?service={}&port={}&ttl={}", encode(&heartbeat.service), heartbeat.port, TTL);
        let mut ok = None;
        while !LEAVING.load(std::sync::atomic::Ordering::SeqCst) {
            match heartbeat.post(&path, None) {
                Ok(()) if ok != Some(true) => {
                    eprintln!("quillo: registered with {} as a member of `{}`", heartbeat.admin, heartbeat.service);
                    ok = Some(true);
                }
                Err(e) if ok != Some(false) => {
                    eprintln!("quillo: cannot register with {}: {} (trying again)", heartbeat.admin, e);
                    ok = Some(false);
                }
                _ => {}
            }
            std::thread::sleep(Duration::from_secs(TTL / 3));
        }
    });

    let (tx, rx) = std::sync::mpsc::channel::<Vec<String>>();
    shared.set_outbox(tx);
    let from = shared.id.clone();
    let _ = std::thread::Builder::new().name("quillo-broadcast".into()).spawn(move || {
        let path = format!("/broadcast?service={}", encode(&m.service));
        let key = m.key();
        let mut failing = false;
        while let Ok(first) = rx.recv() {
            // Changes made while the last message was on its way go together.
            let mut topics = first;
            while let Ok(more) = rx.try_recv() {
                topics.extend(more);
            }
            topics.sort();
            topics.dedup();
            for part in topics.chunks(MAX_TOPICS) {
                let body = announcement(&key, &from, now_secs(), part);
                match m.post(&path, Some(&body)) {
                    Ok(()) => failing = false,
                    Err(e) if !failing => {
                        eprintln!("quillo: cannot announce changes through {}: {}", m.admin, e);
                        failing = true;
                    }
                    Err(_) => {}
                }
            }
        }
    });
}

/// The key announcements are signed with, for limer's admin secret.
pub fn announce_key(secret: &str) -> String {
    crate::crypto::hex(&crate::crypto::hmac_sha256(secret.as_bytes(), b"quillo-announce"))
}

/// What is signed: the sender, the time, the nonce and the topics.
fn signed_text(from: &str, t: u64, nonce: &str, topics: &[String]) -> String {
    serde_json::json!([from, t, nonce, topics]).to_string()
}

fn sign(key: &str, text: &str) -> String {
    crate::crypto::hex(&crate::crypto::hmac_sha256(key.as_bytes(), text.as_bytes()))
}

/// An announcement of `topics`, made at `t` (seconds since 1970), signed
/// with `key` (see `announce_key`).
pub fn announcement(key: &str, from: &str, t: u64, topics: &[String]) -> String {
    let nonce = crate::crypto::hex(&crate::crypto::secret_bytes(16));
    let sig = sign(key, &signed_text(from, t, &nonce, topics));
    serde_json::json!({ "from": from, "t": t, "nonce": nonce, "topics": topics, "sig": sig }).to_string()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Signatures of the announcements taken lately, and when they were made:
/// one sent again is refused.
fn seen() -> &'static Mutex<HashMap<String, u64>> {
    static S: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Most announcements remembered (those of the last `WINDOW` seconds).
const SEEN_MAX: usize = 100_000;

/// `POST /_quillo/announce` from limer: another member changed these topics.
/// Returns the status to answer with.
pub fn announced(shared: &Shared, key: &str, body: &[u8]) -> u16 {
    let Ok(j) = serde_json::from_slice::<serde_json::Value>(body) else { return 400 };
    let (Some(from), Some(t), Some(nonce), Some(list), Some(sig)) =
        (j["from"].as_str(), j["t"].as_u64(), j["nonce"].as_str(), j["topics"].as_array(), j["sig"].as_str())
    else {
        return 400;
    };
    if list.len() > MAX_TOPICS {
        return 413;
    }
    let Some(topics) = list.iter().map(|t| t.as_str().map(String::from)).collect::<Option<Vec<String>>>() else { return 400 };
    let now = now_secs();
    let expected = sign(key, &signed_text(from, t, nonce, &topics));
    if now.abs_diff(t) > WINDOW || !crate::crypto::same(expected.as_bytes(), sig.as_bytes()) {
        return 403;
    }
    {
        let mut seen = seen().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if seen.contains_key(sig) {
            return 403;
        }
        if seen.len() >= SEEN_MAX {
            seen.retain(|_, at| now.abs_diff(*at) <= WINDOW);
            if seen.len() >= SEEN_MAX {
                return 503;
            }
        }
        seen.insert(sig.to_string(), t);
    }
    // Our own changes come back too; they are already known here.
    if from != shared.id {
        shared.publish_known(&topics);
    }
    204
}

/// Percent-encodes a query parameter.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}
