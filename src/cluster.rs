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
//! Variables belong to each server. Messages carry a key derived from limer's
//! admin secret, so only servers that know it can make pages reload.

use crate::shared::Shared;
use std::sync::Arc;
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
    /// limer's admin secret (`LIMER_ADMIN_SECRET`), if it has one.
    pub secret: Option<String>,
}

/// How long limer keeps a member that stops heartbeating, in seconds.
const TTL: u64 = 15;

impl Membership {
    /// Proves an announcement came from a server that knows the secret,
    /// without sending the secret itself to every member.
    pub fn key(&self) -> String {
        let secret = self.secret.as_deref().unwrap_or("");
        crate::ws::hex(&crate::ws::sha1(format!("quillo-announce:{}", secret).as_bytes()))
    }

    /// The addresses of limer's admin host, which broadcasts come from.
    pub fn admin_addresses(&self) -> Vec<std::net::IpAddr> {
        use std::net::ToSocketAddrs;
        let rest = self.admin.split_once("://").map_or(self.admin.as_str(), |(_, r)| r);
        let authority = rest.split('/').next().unwrap_or("");
        let with_port = if authority.rsplit_once(':').map_or(false, |(_, p)| p.parse::<u16>().is_ok()) {
            authority.to_string()
        } else {
            format!("{}:80", authority)
        };
        with_port.to_socket_addrs().map(|a| a.map(|s| s.ip()).collect()).unwrap_or_default()
    }

    fn post(&self, path_and_query: &str, body: Option<&str>) -> Result<(), String> {
        let mut rq = ureq::post(&format!("{}{}", self.admin.trim_end_matches('/'), path_and_query))
            .timeout(Duration::from_secs(5));
        if let Some(s) = &self.secret {
            rq = rq.set("authorization", &format!("Bearer {}", s));
        }
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

/// Joins the pool and starts telling the other members about changes.
pub fn join(shared: Arc<Shared>, m: Membership) {
    let heartbeat = m.clone();
    let _ = std::thread::Builder::new().name("quillo-register".into()).spawn(move || {
        let path = format!("/register?service={}&port={}&ttl={}", encode(&heartbeat.service), heartbeat.port, TTL);
        let mut ok = None;
        loop {
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
            let body = serde_json::json!({ "from": from, "key": key, "topics": topics }).to_string();
            match m.post(&path, Some(&body)) {
                Ok(()) => failing = false,
                Err(e) if !failing => {
                    eprintln!("quillo: cannot announce changes through {}: {}", m.admin, e);
                    failing = true;
                }
                Err(_) => {}
            }
        }
    });
}

/// `POST /_quillo/announce` from limer: another member changed these topics.
/// Returns the status to answer with.
pub fn announced(shared: &Shared, key: &str, body: &[u8]) -> u16 {
    let Ok(j) = serde_json::from_slice::<serde_json::Value>(body) else { return 400 };
    if !same(j["key"].as_str().unwrap_or("").as_bytes(), key.as_bytes()) {
        return 403;
    }
    // Our own changes come back too; they are already known here.
    if j["from"].as_str() != Some(shared.id.as_str()) {
        let topics: Vec<String> =
            j["topics"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect()).unwrap_or_default();
        shared.publish_here(&topics);
    }
    204
}

/// Compares in time that does not depend on where the texts differ, so the
/// key cannot be guessed a character at a time.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
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
