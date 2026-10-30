//! A server's part in limer's pool: registering, keeping registered, telling
//! the others of changes and leaving, against a stand-in for limer's admin API.

use quillo::cluster::{announce_key, announced, announcement, join, Membership};
use quillo::shared::Shared;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SECRET: &str = "limer-admin-secret-for-the-tests";

struct Limer {
    url: String,
    /// What was asked: the start of the request and its body.
    asked: Arc<Mutex<Vec<(String, String)>>>,
    /// The status to answer each path starting with this with.
    status: Arc<Mutex<Vec<(String, u16)>>>,
}

impl Limer {
    fn start() -> Limer {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let asked: Arc<Mutex<Vec<(String, String)>>> = Default::default();
        let status: Arc<Mutex<Vec<(String, u16)>>> = Default::default();
        let (a, st) = (asked.clone(), status.clone());
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let (a, st) = (a.clone(), st.clone());
                std::thread::spawn(move || {
                    let mut s = s;
                    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    let mut got = Vec::new();
                    let mut b = [0u8; 4096];
                    let (head, body) = loop {
                        let text = String::from_utf8_lossy(&got).into_owned();
                        if let Some((h, b)) = text.split_once("\r\n\r\n") {
                            let len = h.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length: ").map(str::to_string)).and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
                            if b.len() >= len {
                                break (h.to_string(), b.to_string());
                            }
                        }
                        match s.read(&mut b) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => got.extend_from_slice(&b[..n]),
                        }
                    };
                    let line = head.lines().next().unwrap_or("").to_string();
                    assert!(head.to_ascii_lowercase().contains(&format!("authorization: bearer {}", SECRET.to_ascii_lowercase())), "{}", head);
                    let code = st.lock().unwrap().iter().find(|(p, _)| line.contains(p.as_str())).map_or(200, |(_, c)| *c);
                    a.lock().unwrap().push((line, body));
                    let _ = write!(s, "HTTP/1.1 {} X\r\nContent-Length: 7\r\nConnection: close\r\n\r\n refused", code);
                });
            }
        });
        Limer { url, asked, status }
    }

    fn lines(&self) -> Vec<(String, String)> {
        self.asked.lock().unwrap().clone()
    }

    fn wait_for(&self, what: &str, n: usize) -> Vec<(String, String)> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let found: Vec<_> = self.lines().into_iter().filter(|(l, _)| l.contains(what)).collect();
            if found.len() >= n {
                return found;
            }
            assert!(Instant::now() < deadline, "{} not asked {} times: {:?}", what, n, self.lines());
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// One test: leaving stops the process's heartbeats.
#[test]
fn a_member_registers_announces_and_leaves() {
    let good = Limer::start();
    let bad = Limer::start();
    bad.status.lock().unwrap().push(("/register".into(), 403));
    bad.status.lock().unwrap().push(("/broadcast".into(), 500));

    let member = |url: &str, service: &str| Membership { admin: format!("{}/", url), service: service.into(), port: 8081, secret: SECRET.into() };

    let shared = Arc::new(Shared::new());
    let refused = Arc::new(Shared::new());
    join(shared.clone(), member(&good.url, "my app"));
    join(refused.clone(), member(&bad.url, "other"));

    // The pool's name goes out encoded.
    let first = good.wait_for("/register", 1);
    assert!(first[0].0.contains("/register?service=my%20app&port=8081&ttl=15"), "{:?}", first);
    bad.wait_for("/register", 1);

    // Each change is signed and sent; what cannot be sent is passed over.
    shared.publish(&["db:b".to_string(), "db:a".to_string(), "var:only-here".to_string()]);
    let told = good.wait_for("/broadcast", 1);
    assert!(told[0].0.contains("/broadcast?service=my%20app"), "{:?}", told);
    let j: serde_json::Value = serde_json::from_str(&told[0].1).unwrap();
    let sent: Vec<&str> = j["topics"].as_array().unwrap().iter().map(|t| t.as_str().unwrap()).collect();
    assert_eq!(sent, ["db:a", "db:b"]);
    assert_eq!(j["from"].as_str(), Some(shared.id.as_str()));

    // A server takes it with the key, once.
    let other = Shared::new();
    let key = announce_key(SECRET);
    assert_eq!(announced(&other, &key, told[0].1.as_bytes()), 204);
    assert_eq!(announced(&other, &key, told[0].1.as_bytes()), 403);

    refused.publish(&["db:t".to_string()]);
    bad.wait_for("/broadcast", 1);
    refused.publish(&["db:u".to_string()]);
    bad.wait_for("/broadcast", 2);

    // Changes made while one is on its way go together.
    for i in 0..200 {
        shared.publish(&[format!("db:{}", i)]);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let topics = |l: &Limer| -> usize {
        l.lines().iter().filter(|(line, _)| line.contains("/broadcast")).map(|(_, b)| serde_json::from_str::<serde_json::Value>(b).unwrap()["topics"].as_array().unwrap().len()).sum()
    };
    while topics(&good) < 202 {
        assert!(Instant::now() < deadline, "{}", topics(&good));
        std::thread::sleep(Duration::from_millis(10));
    }

    // The heartbeat goes on, whichever way the last one went.
    good.wait_for("/register", 2);
    bad.wait_for("/register", 2);

    let m = member(&good.url, "my app");
    m.leave();
    good.wait_for("/deregister?service=my%20app&port=8081", 1);
    // One that cannot leave says so, and the heartbeat's end does the rest.
    bad.status.lock().unwrap().push(("/deregister".into(), 500));
    member(&bad.url, "other").leave();
    bad.wait_for("/deregister", 1);
    member("http://127.0.0.1:9", "gone").leave();
}

#[test]
fn announcements_are_refused_unless_signed_recent_and_new() {
    let s = Shared::new();
    let key = announce_key("the secret");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let topics = ["var:x".to_string()];
    let a = announcement(&key, "elsewhere", now, &topics);
    assert_eq!(announced(&s, &announce_key("another secret"), a.as_bytes()), 403);
    assert_eq!(announced(&s, &key, announcement(&key, "elsewhere", now - 300, &topics).as_bytes()), 403);
    assert_eq!(announced(&s, &key, b"not json"), 400);
    assert_eq!(announced(&s, &key, b"{}"), 400);
    let mixed = br#"{"from":"a","t":1,"nonce":"n","topics":[1],"sig":"s"}"#;
    assert_eq!(announced(&s, &key, mixed), 400);
    let many: Vec<String> = (0..257).map(|i| format!("t{}", i)).collect();
    assert_eq!(announced(&s, &key, announcement(&key, "elsewhere", now, &many).as_bytes()), 413);
    assert_eq!(announced(&s, &key, a.as_bytes()), 204);
    // Its own changes come back too.
    assert_eq!(announced(&s, &key, announcement(&key, &s.id, now, &topics).as_bytes()), 204);
}

#[test]
fn announcements_remembered_are_limited() {
    let s = Shared::new();
    let key = announce_key("flood secret");
    let at = |ahead: i64| {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        (now + ahead) as u64
    };
    for _ in 0..100_050 {
        let status = announced(&s, &key, announcement(&key, "elsewhere", at(-59), &[]).as_bytes());
        assert!(status == 204 || status == 503, "{}", status);
    }
    // Those remembered past their minute make room.
    std::thread::sleep(Duration::from_millis(2200));
    assert_eq!(announced(&s, &key, announcement(&key, "elsewhere", at(0), &[]).as_bytes()), 204);
    // Those still in their minute do not.
    let mut status = 204;
    let mut n = 0;
    while status == 204 {
        n += 1;
        assert!(n <= 100_200, "never refused");
        status = announced(&s, &key, announcement(&key, "elsewhere", at(59), &[]).as_bytes());
    }
    assert_eq!(status, 503);
}
