//! 0.8.12, slice W1: live topics signed for their page (FEEDBACK51.md, Q-666) and islands taken
//! up by their answer's nonce (FEEDBACK51.md, Q-667), the user's decisions of 2026-11-26 (branch
//! feedback86-89). Each test makes the attack of FEEDBACK51's probes, and the paths that must keep
//! working beside it.

mod support;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("f812-pages-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

/// The secret `support::serve` gives its servers.
const SECRET: &[u8] = b"test secret for the tests";

/// A live state's topics as a page sends them to a server with `SECRET`, signed for the program
/// mounted at `base` and `tenant`: `~` and the signature, then the topics.
fn signed(base: &str, tenant: Option<&str>, topics: &[&str]) -> Vec<String> {
    let mut out = vec![format!("~{}", sig(base, tenant, topics))];
    out.extend(topics.iter().map(|t| t.to_string()));
    out
}

/// The signature of a state's topics, as `set_sig` in src/server.rs makes it: over the mount, the
/// tenant (none where every topic is a global's) and the topics, sorted, each once and after its
/// length.
fn sig(base: &str, tenant: Option<&str>, topics: &[&str]) -> String {
    let key = quillo::crypto::hmac_sha256(SECRET, b"quillo page key 1");
    let bare = |t: &str| if base.is_empty() { t.to_string() } else { t.strip_prefix(base).and_then(|r| r.strip_prefix('|')).unwrap_or(t).to_string() };
    let tenant = if topics.iter().all(|t| bare(t).starts_with("var:")) { None } else { tenant };
    let mut set = topics.to_vec();
    set.sort_unstable();
    set.dedup();
    let mut m = format!("{}\0{}\0", base, tenant.unwrap_or(""));
    for t in set {
        m.push_str(&format!("{}:{}", t.len(), t));
    }
    quillo::crypto::b64url(&quillo::crypto::hmac_sha256(&key, m.as_bytes())[..12])
}

/// `topic` after a signature that is another's.
fn with_sig(sig: String, topic: &str) -> Vec<String> {
    vec![format!("~{}", sig), topic.to_string()]
}

/// The topics as `liveKey` writes them, then as `encodeURIComponent` does.
fn t_param(items: &[String]) -> String {
    let key = items.iter().map(|t| t.replace('%', "%25").replace(',', "%2C")).collect::<Vec<_>>().join(",");
    key.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

/// The server's sequence number now, as a page that has seen it.
fn seq(port: u16, prefix: &str, host: &str) -> u64 {
    let b = support::send(port, "GET", &format!("{}/_quillo/live?since=0&t=x", prefix), &[("Host", host)], "").body;
    serde_json::from_str::<serde_json::Value>(&b).unwrap_or_else(|e| panic!("{}: {}", e, b))["seq"].as_u64().unwrap()
}

/// A page waiting, as a long poll, for a change of `items` after `since`.
fn follow(port: u16, prefix: &str, host: &str, query: &str, items: &[String]) -> std::net::TcpStream {
    let mut c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(c, "GET {}/_quillo/live?{}&t={} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", prefix, query, t_param(items), host).unwrap();
    c
}

/// What came on a waiting page's connection within `ms`: "" for nothing.
fn heard(c: &mut std::net::TcpStream, ms: u64) -> String {
    c.set_read_timeout(Some(Duration::from_millis(ms))).unwrap();
    let mut out = String::new();
    let _ = c.read_to_string(&mut out);
    out
}

/// Whether an answer heard is the refusal of topics none of which are signed for the page: what a
/// page left open across the upgrade to 0.8.12 is answered, so that its runtime says so.
fn refused(answer: &str) -> bool {
    answer.starts_with("HTTP/1.1 403") && answer.contains("these live topics are not signed for this page")
}

/// The `changed` of an answer heard, as a list.
fn changed(answer: &str) -> Vec<String> {
    let body = &answer[answer.find("\r\n\r\n").map_or(0, |e| e + 4)..];
    let j: serde_json::Value = serde_json::from_str(body).unwrap_or_else(|e| panic!("{}: {}", e, answer));
    j["changed"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_string()).collect()
}

// Q-666, a topic the server never signed: an anonymous visitor named `dm:alice:bob` and heard when
// Alice wrote Bob. Now only a topic signed for its page is followed: not one sent bare, nor one
// with a signature made up, nor another topic's signature on it.
#[test]
fn a_topic_the_server_never_signed_is_not_followed() {
    let d = dir("made-up", "server fn load_dm(with: str) {\n    watch(`dm:alice:${with}`)\n    return []\n}\nroute POST /send/:to = { publish(`dm:alice:${to}`); return \"sent\" }\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let since = seq(s.port, "", "localhost");
    let q = format!("since={}", since);
    let topic = "dm:alice:bob".to_string();
    let mut bare = follow(s.port, "", "localhost", &q, std::slice::from_ref(&topic));
    let mut made_up = follow(s.port, "", "localhost", &q, &with_sig("A".repeat(16), &topic));
    let mut others = follow(s.port, "", "localhost", &q, &with_sig(sig("", None, &["dm:alice:carol"]), &topic));
    // A state's signature on more topics than it was given for, or fewer.
    let mut more = follow(s.port, "", "localhost", &q, &[format!("~{}", sig("", None, &["dm:alice:carol"])), "dm:alice:carol".into(), topic.clone()]);
    let mut fewer = follow(s.port, "", "localhost", &q, &with_sig(sig("", None, &[&topic, "dm:alice:carol"]), &topic));
    // A signature for another tenant, or another mount, is no signature here.
    let mut tenants = follow(s.port, "", "localhost", &q, &with_sig(sig("", Some("acme"), &[&topic]), &topic));
    let mut mounts = follow(s.port, "", "localhost", &q, &with_sig(sig("/app", None, &[&topic]), &topic));
    let mut own = follow(s.port, "", "localhost", &q, &signed("", None, &[&topic]));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(support::send(s.port, "POST", "/send/bob", &[], "").body, "sent");
    for (what, c) in [("bare", &mut bare), ("made up", &mut made_up), ("another topic's", &mut others), ("more than signed", &mut more), ("fewer than signed", &mut fewer), ("another tenant's", &mut tenants), ("another mount's", &mut mounts)] {
        let a = heard(c, 700);
        assert!(refused(&a), "{}: a visitor was not refused: {}", what, a);
    }
    assert_eq!(changed(&heard(&mut own, 10_000)), vec![topic.clone()], "the page's own signed topic");
}

// Q-666, another tenant's topic, where a mounted program has the tenants: 0.8.11 asked the top
// program, which has no tenant rule, and evil heard acme's orders. The mounted program's own rule
// says whose a page is.
#[test]
fn another_tenants_topic_is_not_followed_under_a_mount() {
    let d = dir("mount", "mount /app = \"app.qwe\"\nroute GET / = \"top\"\n");
    std::fs::write(d.join("app.qwe"), "tenant_from(\"example.com\")\nroute POST /change = { publish(\"orders\"); return \"ok\" }\n").unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let since = seq(s.port, "/app", "acme.example.com");
    let q = format!("since={}", since);
    let topic = "/app|acme@orders".to_string();
    let acme_signed = signed("/app", Some("acme"), &[&topic]);
    let mut evil_bare = follow(s.port, "/app", "evil.example.com", &q, std::slice::from_ref(&topic));
    let mut evil_signed = follow(s.port, "/app", "evil.example.com", &q, &acme_signed);
    let mut nobody = follow(s.port, "/app", "example.com", &q, &acme_signed);
    let mut acme = follow(s.port, "/app", "acme.example.com", &q, &acme_signed);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(support::send(s.port, "POST", "/app/change", &[("Host", "acme.example.com")], "").body, "ok");
    for (what, c) in [("named bare", &mut evil_bare), ("with acme's signature", &mut evil_signed), ("from no tenant", &mut nobody)] {
        let a = heard(c, 1_000);
        assert!(refused(&a), "evil, {}: {}", what, a);
    }
    assert_eq!(changed(&heard(&mut acme, 10_000)), vec![topic.clone()], "acme follows its own");
    // Evil's own, signed for evil, it follows.
    let since = seq(s.port, "/app", "evil.example.com");
    let mut evil = follow(s.port, "/app", "evil.example.com", &format!("since={}", since), &signed("/app", Some("evil"), &["/app|evil@orders"]));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(support::send(s.port, "POST", "/app/change", &[("Host", "evil.example.com")], "").body, "ok");
    assert_eq!(changed(&heard(&mut evil, 10_000)), vec!["/app|evil@orders".to_string()]);
}

// Q-666, the same tenant's topic the server did not sign for this page: within acme, a visitor
// followed `acme@dm:alice:bob`. Topics are signed over topic and tenant, not the session (the
// user's decision): another page of acme's given that topic follows it.
#[test]
fn the_same_tenants_topic_not_signed_for_the_page_is_not_followed() {
    let d = dir("tenant", "tenant_from(\"example.com\")\nserver fn load_dm(with: str) {\n    watch(`dm:alice:${with}`)\n    return []\n}\nroute POST /send/:to = { publish(`dm:alice:${to}`); return \"sent\" }\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let host = "acme.example.com";
    let since = seq(s.port, "", host);
    let q = format!("since={}", since);
    let topic = "acme@dm:alice:bob".to_string();
    let mut bare = follow(s.port, "", host, &q, std::slice::from_ref(&topic));
    let mut carols = follow(s.port, "", host, &q, &with_sig(sig("", Some("acme"), &["acme@dm:alice:carol"]), &topic));
    let mut untenanted = follow(s.port, "", host, &q, &with_sig(sig("", None, &[&topic]), &topic));
    let mut given = follow(s.port, "", host, &q, &signed("", Some("acme"), &[&topic]));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(support::send(s.port, "POST", "/send/bob", &[("Host", host)], "").body, "sent");
    for (what, c) in [("bare", &mut bare), ("another topic's signature", &mut carols), ("a signature for no tenant", &mut untenanted)] {
        let a = heard(c, 1_000);
        assert!(refused(&a), "{}: {}", what, a);
    }
    assert_eq!(changed(&heard(&mut given, 10_000)), vec![topic.clone()], "a page given the topic");
}

// What the server gives a page: the topics its live state read, with one signature for them all for
// the page's place, in the page (`data-l`'s `k`) and in a live call's answer (`key`); a state of
// global variables' topics only is every tenant's, signed for none.
#[test]
fn the_server_signs_each_topic_it_gives_a_page() {
    let src = r#"tenant_from("example.com")
let board = { n: 1 }
server fn load(with: str) {
    watch(`dm:${with}`)
    watch("dm:élan:✓,%2C|x")
    return board.n
}
route POST /bump = { board = { n: board.n + 1 }; return "ok" }
route POST /odd = { publish("dm:élan:✓,%2C|x"); return "ok" }
component Box() {
    state v = live(load, "bob")
    <p>{v}</p>
}
route GET / = <html><head><title>t</title></head><body><Box/></body></html>
"#;
    let d = dir("given", src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let want = |t: &str| {
        let mut topics = vec![format!("{}@dm:bob", t), format!("{}@dm:élan:✓,%2C|x", t), "var:board.n".to_string()];
        topics.sort();
        let set: Vec<&str> = topics.iter().map(String::as_str).collect();
        let key = sig("", Some(t), &set);
        (topics, key)
    };
    for t in ["acme", "evil"] {
        let host = format!("{}.example.com", t);
        let r = support::send(s.port, "POST", "/_quillo/rpc/load", &[("Host", &host), ("Content-Type", "application/json"), ("X-Quillo-Live", "1"), ("Sec-Fetch-Site", "same-origin")], "[\"bob\"]");
        let j: serde_json::Value = serde_json::from_str(&r.body).unwrap_or_else(|e| panic!("{}: {}", e, r.body));
        let (topics, key) = want(t);
        assert_eq!(j["topics"], serde_json::json!(topics), "{}", r.body);
        assert_eq!(j["key"], serde_json::json!(key), "{}", r.body);
        // In the page, the same.
        let page = support::send(s.port, "GET", "/", &[("Host", &host)], "").body;
        let l = page.split(" data-l=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or_else(|| panic!("no data-l: {}", page)).replace("&quot;", "\"").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">");
        let l: serde_json::Value = serde_json::from_str(&l).unwrap_or_else(|e| panic!("{}: {}", e, l));
        assert_eq!((&l["v"]["t"], &l["v"]["k"]), (&serde_json::json!(topics), &serde_json::json!(key)), "{}", page);
        // And those it gives, it follows: a unicode topic with `,`, `%` and `|` in it among them.
        let since = seq(s.port, "", &host);
        let mut items = vec![format!("~{}", key)];
        items.extend(topics.iter().cloned());
        let mut c = follow(s.port, "", &host, &format!("since={}", since), &items);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(support::send(s.port, "POST", "/odd", &[("Host", &host)], "").body, "ok");
        assert_eq!(changed(&heard(&mut c, 10_000)), vec![format!("{}@dm:élan:✓,%2C|x", t)]);
        let since = seq(s.port, "", &host);
        let mut c = follow(s.port, "", &host, &format!("since={}", since), &items);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(support::send(s.port, "POST", "/bump", &[("Host", &host)], "").body, "ok");
        assert_eq!(changed(&heard(&mut c, 10_000)), vec!["var:board.n".to_string()]);
    }
}

// A page from before a restart, or from another server, whose topics' signatures this server does
// not take (a key made at start, with no QUILLO_SECRET): told at once that they changed, as it is
// told of all its topics then, so it loads them again and gets them signed anew. A page from this
// server is told nothing of them, and none of this says anything a page could not make up.
#[test]
fn a_page_from_elsewhere_is_told_its_unsigned_topics_changed() {
    let d = dir("elsewhere", "route POST /send = { publish(\"news\"); return \"sent\" }\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let since = seq(s.port, "", "localhost");
    let mut items = vec!["plain".to_string(), format!("~{}", "X".repeat(16)), "news".to_string()];
    items.extend(signed("", None, &["signed"]));
    // From another server: at once, all of them, by the name the page knows them by.
    let mut c = follow(s.port, "", "localhost", &format!("since={}&s=another", since), &items);
    let mut got = changed(&heard(&mut c, 5_000));
    got.sort();
    assert_eq!(got, vec!["news".to_string(), "plain".to_string(), "signed".to_string()]);
    // From this one: nothing at once, and the change of an unsigned one is not told.
    let me = support::send(s.port, "GET", "/_quillo/live?since=0&t=x", &[], "").body;
    let me: serde_json::Value = serde_json::from_str(&me).unwrap();
    let mut c = follow(s.port, "", "localhost", &format!("since={}&s={}", since, me["server"].as_str().unwrap()), &items);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(support::send(s.port, "POST", "/send", &[], "").body, "sent");
    assert_eq!(heard(&mut c, 1_500), "", "a page of this server's was told of an unsigned topic");
    // The longest topic a page follows (448 bytes), and one more: only the first is told of.
    let long = "a".repeat(448);
    let mut c = follow(s.port, "", "localhost", &format!("since={}&s=another", since), &[long.clone(), "b".repeat(449)]);
    assert_eq!(changed(&heard(&mut c, 5_000)), vec![long]);
    // None, and an empty one.
    let mut c = follow(s.port, "", "localhost", &format!("since={}&s=another", since), &[]);
    assert_eq!(changed(&heard(&mut c, 5_000)), Vec::<String>::new());
    // A signature with nothing after it, two in a row, a `~` too short or too long to be one, and a
    // topic that starts with `~`, written `%7E`.
    let t = format!(".,~{},~{},~{},~{},%257Ex", "A".repeat(15), "A".repeat(17), "A".repeat(16), "B".repeat(16));
    let a = support::send(s.port, "GET", &format!("/_quillo/live?since={}&s=another&t={}", since, t), &[], "");
    let mut got = changed(&format!("\r\n\r\n{}", a.body));
    got.sort();
    assert_eq!(got, vec![".".to_string(), format!("~{}", "A".repeat(15)), format!("~{}", "A".repeat(17)), "~x".to_string()]);
}

// The same on the program's own path, as a mounted program answers it without waiting, with the
// keys a live call gave: a server with no QUILLO_SECRET signs with a key of its own.
#[test]
fn the_programs_own_path_follows_only_what_it_signed() {
    let src = "tenant_from(\"example.com\")\nserver fn load() {\n    watch(\"orders\")\n    return 1\n}\nroute POST /change = { publish(\"orders\"); return \"ok\" }\n";
    let d = dir("own-path", src);
    let mut it = quillo::Interp::new(&d);
    it.load_source(src, "main.qwe").unwrap();
    let mut ask = |method: &str, url: &str, t: &str, live: bool| {
        let mut headers = vec![("host".to_string(), format!("{}.example.com", t))];
        if live {
            headers.push(("x-quillo-live".into(), "1".into()));
            headers.push(("content-type".into(), "application/json".into()));
        }
        let r = it.handle(&quillo::server::HttpRequest { method: method.into(), url: url.into(), headers, body: if live { b"[]".to_vec() } else { Vec::new() }, remote: "127.0.0.1".into() });
        r.text()
    };
    let j: serde_json::Value = serde_json::from_str(&ask("POST", "/_quillo/rpc/load", "acme", true)).unwrap();
    assert_eq!(j["topics"], serde_json::json!(["acme@orders"]));
    let acme = vec![format!("~{}", j["key"].as_str().unwrap()), "acme@orders".to_string()];
    let since: serde_json::Value = serde_json::from_str(&ask("GET", "/_quillo/live?since=0&t=x", "acme", false)).unwrap();
    ask("POST", "/change", "acme", false);
    let url = |items: &[String]| format!("/_quillo/live?since={}&t={}", since["seq"], t_param(items));
    assert!(ask("GET", &url(&acme), "acme", false).contains("\"changed\":[\"acme@orders\"]"));
    for (items, t) in [(vec!["acme@orders".to_string()], "acme"), (acme.clone(), "evil"), (with_sig(sig("", Some("acme"), &["acme@orders"]), "acme@orders"), "acme")] {
        let b = ask("GET", &url(&items), t, false);
        assert_eq!(b, "these live topics are not signed for this page: load the page again\n", "{:?} from {}", items, t);
    }
    // From before a restart: told at once.
    let b = ask("GET", &format!("/_quillo/live?since=1&s=another&t={}", t_param(&["acme@orders".to_string()])), "acme", false);
    assert!(b.contains("\"changed\":[\"acme@orders\"]"), "{}", b);
}

fn chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium", "/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// `url` in headless Chrome, with `script` run in the page: what it stored in `data-result`.
fn in_chrome(chrome: &Path, url: &str, script: &str) -> serde_json::Value {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let d = support::temp_dir("f812-pages-chrome");
    std::fs::write(d.join("t.js"), script).unwrap();
    let out = Command::new("node").arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(d.join("profile")).arg("30000").arg(d.join("t.js")).output().expect("node");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

const BOOTED: &str = "const tick = (ms) => new Promise((r) => setTimeout(r, ms));\n  for (let i = 0; i < 300 && !(window[Symbol.for(\"quillo\")] || {}).booted; i++) await tick(30);\n  const QW = window[Symbol.for(\"quillo\")];";

const COUNTER: &str = r#"db.open("count.db")
db.exec("create table if not exists c (id integer primary key, n int not null)")
db.exec("insert or ignore into c (id, n) values (1, 0)")
server fn load_n() {
    return db.query("select n from c where id = 1")[0].n
}
component Count() {
    state n = live(load_n)
    state clicks = 0
    <div><p #n>{n}</p><button #b @click={() => { clicks += 1 }}>{clicks}</button></div>
}
route GET / = <html><head><title>t</title></head><body><Count/></body></html>
route POST /add = { db.exec("update c set n = n + 1 where id = 1"); return "ok" }
"#;

/// The script that adds one on the server and waits for the page to show it; what the page sent
/// the live endpoint, and whether each topic went signed.
const ADDS: &str = r#"(async () => {
  const sent = [];
  { const Real = WebSocket; window.WebSocket = class extends Real { constructor(u, ...a) { sent.push(String(u)); super(u, ...a); } send(m) { sent.push(String(m)); return super.send(m); } }; }
  BOOTED
  for (let i = 0; i < 100 && !sent.length; i++) await tick(50);
  const before = document.getElementById("n").textContent;
  await fetch("/add", { method: "POST" });
  for (let i = 0; i < 200 && document.getElementById("n").textContent === before; i++) await tick(50);
  document.getElementById("b").click();
  await tick(200);
  const ts = sent.map((u) => { const m = /[?&]t=([^&]*)/.exec(u); return m ? decodeURIComponent(m[1]) : ""; }).filter(Boolean);
  document.body.dataset.result = JSON.stringify({ before, after: document.getElementById("n").textContent, clicks: document.getElementById("b").textContent, ts, nonce: QW.qn || null, errors: window.__quilloErrors || null });
})();"#;

// The normal path, in Chrome: a live state follows its table, the page sending each topic with the
// signature the server gave it, and its island, carrying the answer's nonce, is taken up.
#[test]
fn live_updates_and_islands_keep_working_in_chrome() {
    let d = dir("chrome-live", COUNTER);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let page = support::send(s.port, "GET", "/", &[], "").body;
    let nonce = page.split("<script data-qn=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or_else(|| panic!("no nonce in the loader: {}", page)).to_string();
    assert!(page.contains(&format!(" data-qn=\"{}\"", nonce)), "{}", page);
    assert_eq!(nonce.len(), 12, "{}", nonce);
    // Each answer its own.
    let again = support::send(s.port, "GET", "/", &[], "").body;
    assert!(!again.contains(&nonce), "the same nonce twice");
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), &ADDS.replace("BOOTED", BOOTED));
    assert_eq!((r["before"].as_str(), r["after"].as_str(), r["clicks"].as_str()), (Some("0"), Some("1"), Some("1")), "{}", r);
    assert_eq!(r["ts"], serde_json::json!([signed("", None, &["db:c"]).join(",")]), "{}", r);
}

// Q-666, old pages after a restart with no QUILLO_SECRET (a key made at start): the page's
// signatures are another key's, and the server it came from is gone; it is told all its topics
// changed, loads them again, signed anew, and follows them on the new server.
#[test]
fn a_page_follows_on_after_a_restart_without_a_secret() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let d = dir("restart", COUNTER);
    let port = support::free_port();
    let start = || {
        let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
        c.arg("run").arg(d.join("main.qwe")).args(["--port", &port.to_string()]).env_remove("QUILLO_SECRET").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let g = support::Guard::new(c.spawn().unwrap());
        let until = std::time::Instant::now() + Duration::from_secs(20);
        while !support::listening(port) {
            assert!(std::time::Instant::now() < until, "the server did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
        g
    };
    let first = start();
    let script = r#"(async () => {
  BOOTED
  const n = () => document.getElementById("n").textContent;
  // The test restarts the server, then adds one on the new one.
  for (let i = 0; i < 400 && n() === "0"; i++) await tick(50);
  document.body.dataset.result = JSON.stringify({ n: n() });
})();"#
    .replace("BOOTED", BOOTED);
    let url = format!("http://127.0.0.1:{}/", port);
    let browser = std::thread::spawn({
        let chrome = chrome.clone();
        move || in_chrome(&chrome, &url, &script)
    });
    std::thread::sleep(Duration::from_secs(4));
    drop(first);
    let _second = start();
    // The page's socket comes back to the new server, is told its topics changed, and loads them.
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(support::send(port, "POST", "/add", &[], "").body, "ok");
    let r = browser.join().unwrap();
    assert_eq!(r["n"].as_str(), Some("1"), "{}", r);
}

const CARD: &str = r#"component Card(children) {
    state open = false
    <div class="card"><button #t @click={() => { open = !open }}>{open ? "open" : "shut"}</button>{children}</div>
}
route GET / = <html><head><title>t</title></head><body><Card><b #kid>kid</b></Card><div #bio>{raw(request.query.bio ?? "")}</div></body></html>
"#;

// Q-667: visitor HTML that a sanitizer keeping `data-*` let through, with a `data-qi`, was taken
// up as a component of the visitor's choosing, with the props and state it said, its string
// `children` put in as HTML. Now only islands carrying the answer's nonce are taken up, and inside
// one only those; the server's own island, its children as HTML, works as before.
#[test]
fn an_injected_island_is_not_taken_up() {
    let d = dir("islands", CARD);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    let script = r#"(async () => {
  BOOTED
  const v = document.getElementById("pwn");
  if (v) { v.dispatchEvent(new PointerEvent("pointerover", { bubbles: true })); v.click(); }
  await tick(400);
  document.getElementById("t").click();
  await tick(200);
  document.body.dataset.result = JSON.stringify({ pwn: window.__pwn ?? null, imgs: document.querySelectorAll("img").length, own: document.getElementById("t").textContent, kid: !!document.getElementById("kid"), text: v ? v.textContent : null });
})();"#
    .replace("BOOTED", BOOTED);
    let attacks = [
        // The report's repro.
        r#"<div id="pwn" data-qi="" data-c="Card" data-p='{"children":"<img src=x onerror=window.__pwn=1>"}' data-s='{"open":true}'>about me</div>"#,
        // A nonce guessed, an empty one, and `data-qh` said too.
        r#"<div id="pwn" data-qi="" data-qn="" data-qh data-c="Card" data-p='{"children":"<img src=x onerror=window.__pwn=2>"}' data-s='{"open":true}'>about me</div>"#,
        r#"<quillo-island id="pwn" data-qn="AAAAAAAAAAAAAAAA" data-qh data-c="Card" data-p='{"children":"<img src=x onerror=window.__pwn=3>"}' data-s='{"open":true}' data-e>about me</quillo-island>"#,
        // Marked by comments, as the server marks one where no element can stand.
        r#"<!--q data-c="Card" data-p='{"children":"<img src=x onerror=window.__pwn=4>"}' data-s="{}" data-e--><i id="pwn">about me</i><!--/q-->"#,
    ];
    for a in attacks {
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/?bio={}", s.port, enc(a)), &script);
        assert_eq!((&r["pwn"], r["imgs"].as_u64()), (&serde_json::Value::Null, Some(0)), "{}: {}", a, r);
        assert_eq!((r["own"].as_str(), r["kid"].as_bool(), r["text"].as_str()), (Some("open"), Some(true), Some("about me")), "{}: {}", a, r);
    }
    // The server's island carries the nonce and says its children are its HTML.
    let page = support::send(s.port, "GET", "/", &[], "").body;
    assert!(page.contains(" data-qh data-qn=\""), "{}", page);
}

// Q-667: a string `children` is put in as HTML only where the server marked it (`data-qh`): an
// island of this answer's, its nonce copied, that does not say so shows its text.
#[test]
fn a_string_children_prop_is_html_only_where_the_server_marked_it() {
    let d = dir("children", CARD);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let script = r#"(async () => {
  BOOTED
  // As a script of the page's would make it, with the answer's nonce: no `data-qh`.
  const made = (marked, children) => {
    const el = document.createElement("div");
    el.setAttribute("data-qi", "");
    el.setAttribute("data-qn", String(QW.qn));
    if (marked) el.setAttribute("data-qh", "");
    el.dataset.c = "Card";
    el.dataset.p = JSON.stringify({ children });
    el.dataset.s = JSON.stringify({ open: false });
    el.innerHTML = '<div class="card"><button>shut</button></div>';
    document.getElementById("bio").appendChild(el);
    el.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    return el;
  };
  made(false, "<i class=x>i</i><img src=x onerror=window.__pwn=1>");
  await tick(300);
  // Said so, as the server says it of the HTML it made of an element's children: HTML.
  made(true, "<i class=y>y</i>");
  await tick(300);
  // Made again, in a <quillo-island> of its own.
  const bio = document.getElementById("bio");
  document.body.dataset.result = JSON.stringify({ pwn: window.__pwn ?? null, text: bio.textContent, html: bio.querySelectorAll("i.x").length, marked: bio.querySelectorAll("i.y").length, remade: QW.remade ?? 0 });
})();"#
    .replace("BOOTED", BOOTED);
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), &script);
    assert_eq!((&r["pwn"], r["html"].as_u64(), r["marked"].as_u64(), r["remade"].as_u64()), (&serde_json::Value::Null, Some(0), Some(1), Some(2)), "{}", r);
    assert!(r["text"].as_str().is_some_and(|t| t.contains("<i class=x>i</i>")), "{}", r);
}

// Under --watch: a save that changes the page's own HTML is put in place (HMR's morph), its islands
// not yet taken up given the new answer's nonce, and taken up after it as before.
#[test]
fn islands_are_taken_up_after_a_watch_morph() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let src = "component A() {\n    state n = 0\n    <button #a @click={() => { n += 1 }}>{n}</button>\n}\ncomponent B() {\n    state n = 0\n    <button #b @click={() => { n += 1 }}>{n}</button>\n}\nroute GET / = <html><head><title>t</title></head><body><A/><div><p #txt>one</p><div><B/></div></div></body></html>\nlet seen = false\nroute POST /ready = { seen = true; return \"ok\" }\nroute GET /seen = seen ? \"yes\" : \"no\"\n";
    let d = dir("watch", src);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let script = r#"(async () => {
  BOOTED
  const was = QW.qn;
  const hmr = [];
  document.addEventListener("quillo:hmr", (e) => hmr.push(JSON.parse(e.detail).what));
  await fetch("/ready", { method: "POST" });
  for (let i = 0; i < 400 && document.getElementById("txt").textContent !== "two"; i++) await tick(50);
  await tick(300);
  const b = document.getElementById("b");
  b.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  b.click();
  await tick(300);
  document.body.dataset.result = JSON.stringify({ txt: document.getElementById("txt").textContent, b: b.textContent, hmr, changed: QW.qn !== was && !!QW.qn });
})();"#
    .replace("BOOTED", BOOTED);
    let url = format!("http://127.0.0.1:{}/", s.port);
    let browser = std::thread::spawn({
        let chrome = chrome.clone();
        move || in_chrome(&chrome, &url, &script)
    });
    // Once the page has booted and listens (it says so), the save.
    let until = std::time::Instant::now() + Duration::from_secs(60);
    while support::send(s.port, "GET", "/seen", &[], "").body != "yes" {
        assert!(std::time::Instant::now() < until, "the page never said it was ready");
        std::thread::sleep(Duration::from_millis(100));
    }
    std::fs::write(d.join("main.qwe"), src.replace(">one<", ">two<")).unwrap();
    let r = browser.join().unwrap();
    assert_eq!((r["txt"].as_str(), r["b"].as_str(), r["changed"].as_bool()), (Some("two"), Some("1"), Some(true)), "{}", r);
    assert_eq!(r["hmr"], serde_json::json!(["morphed"]), "{}", r);
}

// A page left open across the upgrade to 0.8.12 sends its topics bare: refused (403), so that its
// runtime says in the console that live updates are not reaching it, where it would have waited for
// good, told nothing (the coordinator's decision of 2026-11-27). Only where none is signed, from a
// page that has seen a change of this server's: `since=0`, a page from elsewhere, one signed topic
// among bare ones, are answered as before.
#[test]
fn bare_topics_are_refused_so_an_old_page_says_so() {
    let d = dir("bare", COUNTER);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let since = seq(s.port, "", "localhost");
    let ask = |q: &str| support::send(s.port, "GET", &format!("/_quillo/live?{}", q), &[], "");
    let a = ask(&format!("since={}&t=db%3Ac", since));
    assert_eq!((a.status, a.body.as_str()), (403, "these live topics are not signed for this page: load the page again\n"));
    // As a socket, refused at its handshake; topics after the address, as a POST, the same.
    let mut c = std::net::TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    write!(c, "GET /_quillo/live?since={}&t=db%3Ac HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", since).unwrap();
    assert!(heard(&mut c, 5_000).starts_with("HTTP/1.1 403"));
    let a = support::send(s.port, "POST", &format!("/_quillo/live?since={}&tm=1", since), &[("Content-Type", "text/plain")], "db:c");
    assert_eq!(a.status, 403);
    // Not refused: the sequence number asked for, a page from elsewhere, none at all, a signed one.
    assert_eq!(ask("since=0&t=db%3Ac").status, 200);
    let a = ask(&format!("since={}&s=another&t=db%3Ac", since));
    assert_eq!((a.status, changed(&a.body)), (200, vec!["db:c".to_string()]));
    let mut c = follow(s.port, "", "localhost", &format!("since={}", since), &{ let mut v = vec!["db:c".to_string()]; v.extend(signed("", None, &["db:c"])); v });
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(support::send(s.port, "POST", "/add", &[], "").body, "ok");
    assert_eq!(changed(&heard(&mut c, 10_000)), vec!["db:c".to_string()]);
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    // The page as a runtime before 0.8.12 makes its requests: its topics without their signatures.
    let script = r#"(async () => {
  const bare = (u) => String(u).replace(/([?&]t=)([^&]*)/, (_, a, t) => a + encodeURIComponent(decodeURIComponent(t).split(",").map((x) => x).filter((x) => !/^~[A-Za-z0-9_-]{16}$/.test(x)).join(",")));
  { const Real = WebSocket; window.WebSocket = class extends Real { constructor(u, ...a) { super(bare(u), ...a); } }; }
  { const real = window.fetch.bind(window); window.fetch = (u, init) => real(String(u).includes("/_quillo/live") ? bare(u) : u, init); }
  BOOTED
  for (let i = 0; i < 200 && !(window.__quilloErrors || []).length; i++) await tick(50);
  document.body.dataset.result = JSON.stringify({ errors: window.__quilloErrors || null });
})();"#
    .replace("BOOTED", BOOTED);
    // A page whose live state has seen a change (`since` past 0), as one open across the upgrade.
    assert_eq!(support::send(s.port, "POST", "/add", &[], "").body, "ok");
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), &script);
    assert!(r["errors"].to_string().contains("live updates are not reaching this page"), "{}", r);
}

// The review of slice W1 (Q-667): an element named for a property of `document` is what `document`
// gives by that name. `<img name="currentScript" data-qn="…">` gave the loader the visitor's nonce,
// and an island marked with it was taken up, its `children` put in as HTML; one such `<img>` gave no
// nonce, so no island was taken up; two made a collection, and the loader stopped. The loader and
// the runtime's start read the document's own, through `Document.prototype`, whatever the page names,
// on a page with the loader and on one that loads the whole bundle itself.
#[test]
fn elements_named_for_the_documents_properties_change_nothing() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let src = format!("{}route GET /whole = <html><head><title>t</title><script src=\"/_quillo/quillo.js\" defer></script></head><body><Card><b #kid>kid</b></Card><div #bio>{{raw(request.query.bio ?? \"\")}}</div></body></html>\n", CARD);
    let d = dir("clobber", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let script = r#"(async () => {
  BOOTED
  const byId = (id) => Document.prototype.getElementById.call(document, id);
  const v = byId("pwn");
  if (v) { v.dispatchEvent(new PointerEvent("pointerover", { bubbles: true })); v.click(); }
  await tick(400);
  const t = byId("t");
  t.click();
  await tick(200);
  Object.getOwnPropertyDescriptor(Document.prototype, "body").get.call(document).dataset.result = JSON.stringify({ pwn: window.__pwn ?? null, booted: !!(QW && QW.booted), own: t.textContent, qn: QW && QW.qn || null });
})();"#
    .replace("BOOTED", "const tick = (ms) => new Promise((r) => setTimeout(r, ms));\n  for (let i = 0; i < 200 && !(window[Symbol.for(\"quillo\")] || {}).booted; i++) await tick(30);\n  const QW = window[Symbol.for(\"quillo\")];");
    let evil = r#"<img name="currentScript" data-qn="EVILNONCEEVI"><div id="pwn" data-qi="" data-qn="EVILNONCEEVI" data-qh data-e data-c="Card" data-p='{"children":"<img src=x onerror=window.__pwn=1>"}' data-s='{"open":true}'>about me</div>"#;
    let names = ["createElement", "head", "readyState", "addEventListener", "querySelector", "querySelectorAll", "activeElement", "body", "createTreeWalker", "documentElement", "prerendering", "currentScript"];
    let many: String = names.iter().map(|n| format!("<img name=\"{}\">", n)).collect();
    for path in ["/", "/whole"] {
        for (what, bio) in [("an injected nonce", evil.to_string()), ("one <img>", "<img name=\"currentScript\">".to_string()), ("two", "<img name=\"currentScript\"><img name=\"currentScript\">".to_string()), ("every name", many.clone())] {
            let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}{}?bio={}", s.port, path, enc(&bio)), &script);
            assert_eq!((&r["pwn"], r["booted"].as_bool(), r["own"].as_str()), (&serde_json::Value::Null, Some(true), Some("open")), "{} {}: {}", path, what, r);
            assert_ne!(r["qn"].as_str(), Some("EVILNONCEEVI"), "{} {}: {}", path, what, r);
        }
    }
}

// The review of slice W1 (Q-666): a live state that read more than 256 topics, or one longer than
// the endpoint can be told of (448 bytes, as UTF-8, where the runtime counted UTF-16), was followed
// for none of them; as in 0.8.11 it is followed for the first 256, and for those not too long: the
// server signs the set the endpoint takes, and the page sends that set.
#[test]
fn a_state_that_read_more_than_the_endpoint_takes_is_followed_for_what_it_takes() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let many = "let n = 0\nserver fn load() {\n    for i in 0..300 {\n        watch(`x${i + 1000}`)\n    }\n    return n\n}\ncomponent Box() {\n    state v = live(load)\n    <p #v>{v}</p>\n}\nroute GET / = <html><head><title>t</title></head><body><Box/></body></html>\nroute POST /bump = { n = n + 1; return \"ok\" }\n";
    let long = format!("mount /{} = \"app.qwe\"\nroute GET / = \"top\"\n", "m".repeat(200));
    let app = format!("let n = 0\nserver fn load() {{\n    watch(\"{}\")\n    return n\n}}\ncomponent Box() {{\n    state v = live(load)\n    <p #v>{{v}}</p>\n}}\nroute GET / = <html><head><title>t</title></head><body><Box/></body></html>\nroute POST /bump = {{ n = n + 1; return \"ok\" }}\n", "é".repeat(128));
    let d = dir("over", many);
    let d2 = dir("over-long", &long);
    std::fs::write(d2.join("app.qwe"), &app).unwrap();
    let script = r##"(async () => {
  BOOTED
  for (let i = 0; i < 60 && !document.querySelector("#v"); i++) await tick(50);
  await tick(800);
  const before = document.getElementById("v").textContent;
  await fetch(location.pathname.replace(/\/$/, "") + "/bump", { method: "POST" });
  for (let i = 0; i < 200 && document.getElementById("v").textContent === before; i++) await tick(50);
  document.body.dataset.result = JSON.stringify({ before, after: document.getElementById("v").textContent, errors: window.__quilloErrors || null });
})();"##
    .replace("BOOTED", BOOTED);
    for (d, path) in [(&d, "/".to_string()), (&d2, format!("/{}/", "m".repeat(200)))] {
        let s = support::serve(&d.join("main.qwe"), &[]);
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}{}", s.port, path), &script);
        assert_eq!((r["before"].as_str(), r["after"].as_str()), (Some("0"), Some("1")), "{}: {}", path, r);
    }
}

// The review of slice W1 (Q-667): where the program loads the whole bundle itself, the nonce was said
// by a script whose text changed with each answer; a 304 for the page's `etag` comes with the policy
// of a new answer and the body the browser kept, whose script the policy then refused. That script
// is the same in every answer, and reads the nonce from its tag. A page the program sends as written
// (its own `etag` in `respond`) had no script at all, and none of its components was taken up.
#[test]
fn a_page_kept_by_its_etag_keeps_its_scripts_under_the_policy() {
    let src = format!("{}site_headers(\"/p\", {{ \"content-security-policy\": \"default-src 'self'\" }})\nsite_headers(\"/q\", {{ \"content-security-policy\": \"default-src 'self'\", etag: \"\\\"v1\\\"\" }})\nroute GET /p = respond(200, <html><head><title>p</title><script src=\"/_quillo/quillo.js\" defer></script></head><body><Card><b #kid>kid</b></Card></body></html>, {{ etag: \"\\\"v1\\\"\" }})\nroute GET /q = <html><head><title>q</title><script src=\"/_quillo/quillo.js\" defer></script></head><body><Card><b #kid>kid</b></Card></body></html>\n", CARD);
    let d = dir("etag", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    for path in ["/p", "/q"] {
        let first = support::send(s.port, "GET", path, &[], "");
        assert_eq!(first.status, 200, "{}: {}", path, first.body);
        let again = support::send(s.port, "GET", path, &[("If-None-Match", "\"v1\"")], "");
        assert_eq!(again.status, 304, "{}: {}", path, again.body);
        let policy = again.header("content-security-policy").unwrap_or_else(|| panic!("{}: no policy on the 304", path)).to_string();
        // Each inline script of the page kept has its hash in the 304's policy.
        let mut inline = 0;
        for part in first.body.split("<script").skip(1) {
            let (tag, rest) = part.split_once('>').unwrap();
            if tag.contains("src=") {
                continue;
            }
            let text = &rest[..rest.find("</script>").unwrap()];
            use sha2::Digest;
            let h = sha2::Sha256::digest(text.as_bytes());
            // Base64 with `+`, `/` and padding, as a policy writes a hash.
            let hash = format!("'sha256-{}'", quillo::crypto::b64url(&h).replace('-', "+").replace('_', "/") + "=");
            assert!(policy.contains(&hash), "{}: {} not in {} for {}", path, hash, policy, text);
            inline += 1;
        }
        assert!(inline >= 1, "{}: {}", path, first.body);
    }
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    let script = r#"(async () => {
  BOOTED
  document.getElementById("t").click();
  await tick(200);
  document.body.dataset.result = JSON.stringify({ booted: !!(QW && QW.booted), own: document.getElementById("t").textContent });
})();"#
        .replace("BOOTED", BOOTED);
    for path in ["/p", "/q"] {
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}{}", s.port, path), &script);
        assert_eq!((r["booted"].as_bool(), r["own"].as_str()), (Some(true), Some("open")), "{}: {}", path, r);
    }
}

// The second review of slice W1 (Q-667): a page moved to in place took its nonce from the first
// script whose text was the loader's, and a parser that runs no scripts makes elements of a
// `<noscript>`'s: the visitor's copy of the loader there, with its own nonce, gave the nonce of an
// island it wrote, whose `children` went in as HTML. The nonce of a page the runtime asks for comes
// in the answer's header, and only scripts carrying it are Quillo's own.
#[test]
fn a_page_moved_to_in_place_takes_its_nonce_from_its_answer() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let src = format!("moves_in_place()\n{}route GET /a = <html><head><title>a</title></head><body><a #go href={{request.query.to ?? \"/b\"}}>b</a><Card><b>a</b></Card></body></html>\nroute GET /b = <html><head><title>b</title></head><body><div #bio>{{raw(request.query.bio ?? \"\")}}</div><Card><b #kid>kid</b></Card></body></html>\n", CARD.replace("route GET / =", "route GET /c ="));
    let d = dir("in-place", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    // The loader's text, as a visitor reads it from any page.
    // The nonce in the header, to the runtime's own requests only (a move, a page asked for ahead).
    assert!(support::send(s.port, "GET", "/a", &[], "").header("x-quillo-nonce").is_none());
    let page = support::send(s.port, "GET", "/a", &[("x-quillo-move", "1")], "");
    let nonce = page.header("x-quillo-nonce").unwrap_or_else(|| panic!("no nonce header: {:?}", page.headers)).to_string();
    let at = page.body.find("(()=>{const d=document,W=window").unwrap();
    let loader = &page.body[at..at + page.body[at..].find("</script>").unwrap()];
    assert!(page.body.contains(&format!("data-quillo=\"{}\"", nonce)), "{}", page.body);
    let bio = format!(
        "<noscript><script data-quillo data-qn=\"EVILNONCEEVI\">{}</script></noscript><div id=\"pwn\" data-qi=\"\" data-qn=\"EVILNONCEEVI\" data-qh data-e data-c=\"Card\" data-p='{{\"children\":\"<img src=x onerror=window.__pwn=1>\"}}' data-s='{{\"open\":true}}'>about me</div>",
        loader
    );
    // In a frame, which a whole load moves on, the move from /a to /b: a clean one in place, the
    // marker set before it still there; and the attack's, which either loads /b whole (a script of the
    // visitor's in it) or moves with the answer's nonce, never the visitor's.
    let script = r##"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const move = async (to) => {
    const f = document.createElement("iframe");
    f.src = "/a?to=" + encodeURIComponent(to);
    document.body.appendChild(f);
    const q = () => f.contentWindow && f.contentWindow[Symbol.for("quillo")];
    for (let i = 0; i < 200 && !(q() && q().booted); i++) await tick(30);
    f.contentWindow.__marker = 1;
    f.contentDocument.getElementById("go").click();
    for (let i = 0; i < 200 && f.contentWindow.location.pathname !== "/b"; i++) await tick(30);
    for (let i = 0; i < 200 && !(q() && q().booted && f.contentDocument.getElementById("kid")); i++) await tick(30);
    await tick(600);
    const w = f.contentWindow, d = f.contentDocument;
    const v = d.getElementById("pwn");
    if (v) { v.dispatchEvent(new w.PointerEvent("pointerover", { bubbles: true })); v.click(); }
    await tick(300);
    const t = [...d.querySelectorAll("#t")].pop();
    t.click();
    await tick(300);
    return { pwn: w.__pwn ?? null, at: w.location.pathname, inPlace: w.__marker === 1, own: t.textContent };
  };
  const clean = await move("/b");
  const attack = await move(ATTACK);
  document.body.dataset.result = JSON.stringify({ clean, attack });
})();"##
        .replace("ATTACK", &serde_json::to_string(&format!("/b?bio={}", enc(&bio))).unwrap());
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/c", s.port), &script);
    assert_eq!((&r["clean"]["pwn"], r["clean"]["at"].as_str(), r["clean"]["inPlace"].as_bool(), r["clean"]["own"].as_str()), (&serde_json::Value::Null, Some("/b"), Some(true), Some("open")), "{}", r);
    assert_eq!((&r["attack"]["pwn"], r["attack"]["at"].as_str(), r["attack"]["own"].as_str()), (&serde_json::Value::Null, Some("/b"), Some("open")), "{}", r);
}

// The second review of slice W1 (Q-666): `with_tenant` around a page, with no tenant rule, signed its
// live state for the tenant it named, which the live endpoint cannot know, and its own topics were
// refused. Signing and checking take the tenant one way: the program's rule's.
#[test]
fn a_page_under_with_tenant_follows_its_own_topics() {
    let src = "let n = 0\nserver fn load() {\n    watch(\"orders\")\n    return n\n}\ncomponent Box() {\n    state v = live(load)\n    <p>{v}</p>\n}\nroute GET / = with_tenant(\"acme\", () => <html><head><title>t</title></head><body><Box/></body></html>)\nroute POST /change = with_tenant(\"acme\", () => { publish(\"orders\"); return \"ok\" })\n";
    let d = dir("with-tenant", src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let page = support::send(s.port, "GET", "/", &[], "").body;
    let l = page.split(" data-l=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or_else(|| panic!("no data-l: {}", page)).replace("&quot;", "\"").replace("&amp;", "&");
    let l: serde_json::Value = serde_json::from_str(&l).unwrap();
    let (t, k) = (l["v"]["t"].clone(), l["v"]["k"].as_str().unwrap().to_string());
    let topics: Vec<String> = t.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
    assert!(topics.contains(&"acme@orders".to_string()), "{:?}", topics);
    let since = seq(s.port, "", "localhost");
    let mut items = vec![format!("~{}", k)];
    items.extend(topics);
    let mut c = follow(s.port, "", "localhost", &format!("since={}", since), &items);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(support::send(s.port, "POST", "/change", &[], "").body, "ok");
    let a = heard(&mut c, 10_000);
    assert!(a.starts_with("HTTP/1.1 200") && a.contains("acme@orders"), "{}", a);
}

// The second review of slice W1 (Q-667): where a browser has no `requestIdleCallback` (Safari), an
// element named so stopped the loader; a part waiting for its state stayed waiting where the page
// named an element `querySelector`; and the prefetch of links read `createElement`, `head` and
// `addEventListener` off the document.
#[test]
fn the_pages_other_scripts_read_the_document_past_what_it_names() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let src = format!(
        "{}server fn slow() {{\n    sleep(300)\n    return \"done\"\n}}\ncomponent Late() {{\n    state x = defer(slow())\n    <p #late>{{x ?? \"wait\"}}</p>\n}}\nlet SAFARI = \"delete Window.prototype.requestIdleCallback; delete window.requestIdleCallback;\"\nroute GET /safari = <html><head><title>s</title><script>{{raw(SAFARI)}}</script></head><body><img id=\"requestIdleCallback\"><Card><b #kid>kid</b></Card></body></html>\nroute GET /late = <html><head><title>l</title></head><body><form name=\"querySelector\"></form><img name=\"currentScript\"><Late/><a href=\"/safari\">s</a></body></html>\n",
        CARD
    );
    let d = dir("other-scripts", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 100 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(50);
  const QW = window[Symbol.for("quillo")] || {};
  const byId = (id) => Document.prototype.getElementById.call(document, id);
  for (let i = 0; i < 60 && byId("late") && byId("late").textContent === "wait"; i++) await tick(50);
  const late = byId("late");
  Object.getOwnPropertyDescriptor(Document.prototype, "body").get.call(document).dataset.result = JSON.stringify({ booted: !!QW.booted, late: late ? late.textContent : null, errors: window.__quilloErrors || null });
})();"#;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/safari", s.port), script);
    assert_eq!(r["booted"].as_bool(), Some(true), "/safari: {}", r);
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/late", s.port), script);
    assert_eq!(r["late"].as_str(), Some("done"), "/late: {}", r);
}

// The second review of slice W1: a HEAD of a page the program sends as written came without the
// nonce's script and its hash in the policy, its length and policy another answer's than its GET's.
#[test]
fn a_head_has_its_gets_headers() {
    let src = format!("{}site_headers(\"/p\", {{ \"content-security-policy\": \"default-src 'self'\" }})\nroute GET /p = respond(200, <html><head><title>p</title><script src=\"/_quillo/quillo.js\" defer></script></head><body><Card><b #kid>kid</b></Card></body></html>, {{ etag: \"\\\"v1\\\"\" }})\n", CARD);
    let d = dir("head", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = support::send(s.port, "GET", "/p", &[], "");
    let head = support::send(s.port, "HEAD", "/p", &[], "");
    assert_eq!((get.status, head.status), (200, 200));
    for h in ["content-length", "content-security-policy"] {
        let (g, hd) = (get.header(h).map(|v| v.split("sha256-").count()), head.header(h).map(|v| v.split("sha256-").count()));
        assert_eq!(g, hd, "{}: GET {:?}, HEAD {:?}", h, get.header(h), head.header(h));
    }
    assert_eq!(get.header("content-length"), head.header("content-length"));
}

const CARD_N: &str = "component Card(n) {\n    state open = false\n    <button #c{n} @click={() => { open = !open }}>{n} {open ? \"open\" : \"shut\"}</button>\n}\n";

/// The ids of the islands' nonces in a page, each once.
fn nonces(page: &str) -> Vec<String> {
    let mut v: Vec<String> = page.split(" data-qn=\"").skip(1).map(|r| r[..r.find('"').unwrap()].to_string()).collect();
    v.sort();
    v.dedup();
    v
}

// The third review of slice W1 (Q-667): islands rendered by `parallel`'s functions each made a nonce
// of their own, and those rendered at load (`let nav = <Card/>`) the load's: none of them was taken
// up, where 0.8.11 took them up. The answer's nonce is made before `parallel` forks; one rendered
// at load is given the answer's where the page shows that very markup, and a visitor's copy of it is
// still nothing.
#[test]
fn islands_rendered_by_parallel_and_at_load_are_taken_up() {
    let src = format!(
        "{}let nav = <Card n={{9}}/>\nroute GET /par {{\n    let parts = parallel([() => <Card n={{1}}/>, () => <Card n={{2}}/>])\n    return <html><body><Card n={{0}}/>{{parts[0]}}{{parts[1]}}</body></html>\n}}\nroute GET /top = <html><body><Card n={{0}}/>{{nav}}<div>{{raw(request.query.bio ?? \"\")}}</div></body></html>\n",
        CARD_N
    );
    let d = dir("par-top", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    // One nonce on each page, the answer's, a new one each time; the load's in none.
    let a = support::send(s.port, "GET", "/par", &[], "").body;
    assert_eq!(nonces(&a).len(), 1, "{}", a);
    let t1 = support::send(s.port, "GET", "/top", &[], "").body;
    let t2 = support::send(s.port, "GET", "/top", &[], "").body;
    assert_eq!((nonces(&t1).len(), nonces(&t2).len()), (1, 1), "{}\n{}", t1, t2);
    assert_ne!(nonces(&t1), nonces(&t2));
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    // The island of `nav` as an earlier answer wrote it, copied into the page by a visitor.
    let at = t1.find("<quillo-island").unwrap();
    let nav_at = t1[at + 1..].find("<quillo-island").map(|i| i + at + 1).unwrap();
    let copy = t1[nav_at..nav_at + t1[nav_at..].find("</quillo-island>").unwrap() + "</quillo-island>".len()].replace("id=\"c9\"", "id=\"copy\"");
    let script = r#"(async () => {
  BOOTED
  const out = {};
  for (const id of ["c0", "c1", "c2", "c9", "copy"]) {
    const b = document.getElementById(id);
    if (!b) continue;
    b.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    b.click();
    await tick(150);
    out[id] = b.textContent;
  }
  document.body.dataset.result = JSON.stringify(out);
})();"#
        .replace("BOOTED", BOOTED);
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/par", s.port), &script);
    assert_eq!(r, serde_json::json!({ "c0": "0 open", "c1": "1 open", "c2": "2 open" }), "{}", r);
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/top?bio={}", s.port, enc(&copy)), &script);
    assert_eq!(r, serde_json::json!({ "c0": "0 open", "c9": "9 open", "copy": "9 shut" }), "{}", r);
}

// The third review of slice W1 (Q-667), --watch: a save made the page again (HMR's morph) and an
// island taken up was made again from the new answer's props, without looking at its nonce: an
// island a visitor's markup put where the component was ran its `children` as HTML. Only one the new
// answer wrote is made again; else the page loads whole.
#[test]
fn a_watch_morph_makes_again_only_the_new_answers_islands() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let v1 = format!("{}route GET / = <html><head><title>a</title></head><body><Card><b>kid</b></Card></body></html>\nroute GET /frame = <html><body><iframe id=\"f\" src=\"/\"></iframe></body></html>\nlet seen = false\nroute POST /ready = {{ seen = true; return \"ok\" }}\nroute GET /seen = seen ? \"yes\" : \"no\"\n", CARD.replace("route GET / = <html><head><title>t</title></head><body><Card><b #kid>kid</b></Card><div #bio>{raw(request.query.bio ?? \"\")}</div></body></html>\n", ""));
    let forged = "<quillo-island style=\\\"display:contents\\\" data-c=\\\"Card\\\" data-qn=\\\"FORGEDFORGED\\\" data-qh data-p='{\\\"children\\\":\\\"<img src=x onerror=window.parent.__pwn=1>\\\"}' data-s='{\\\"open\\\":false}'><div class=\\\"card\\\"><button>shut</button></div></quillo-island>";
    let v2 = v1.replace("<body><Card><b>kid</b></Card></body>", "<body>{raw(BIO)}</body>").replace("route GET / =", &format!("let BIO = \"{}\"\nroute GET / =", forged));
    let d = dir("hot-forged", &v1);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const f = document.getElementById("f");
  const q = () => f.contentWindow && f.contentWindow[Symbol.for("quillo")];
  for (let i = 0; i < 200 && !(q() && q().booted); i++) await tick(30);
  const t = f.contentDocument.getElementById("t");
  t.dispatchEvent(new f.contentWindow.PointerEvent("pointerover", { bubbles: true }));
  await tick(300);
  document.body.dataset.ready = "1";
  await fetch("/ready", { method: "POST" });
  for (let i = 0; i < 120 && window.__pwn !== 1; i++) await tick(50);
  document.body.dataset.result = JSON.stringify({ pwn: window.__pwn ?? null });
})();"#;
    let url = format!("http://127.0.0.1:{}/frame", s.port);
    let browser = std::thread::spawn({
        let chrome = chrome.clone();
        let script = script.to_string();
        move || in_chrome(&chrome, &url, &script)
    });
    // Once the frame's island is taken up (the page says so), the save.
    let until = std::time::Instant::now() + Duration::from_secs(60);
    while support::send(s.port, "GET", "/seen", &[], "").body != "yes" {
        assert!(std::time::Instant::now() < until, "the page never said it was ready");
        std::thread::sleep(Duration::from_millis(100));
    }
    std::fs::write(d.join("main.qwe"), &v2).unwrap();
    let r = browser.join().unwrap();
    assert_eq!(r["pwn"], serde_json::Value::Null, "{}", r);
}

/// Every island nonce in a page, each once, however it is written: an attribute, or inside a
/// waiting part's script (`qn=\"…"`, `qn=&quot;…`).
fn all_nonces(page: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for (at, _) in page.match_indices("qn=") {
        let rest = &page[at + 3..];
        let rest = rest.strip_prefix("\\\"").or_else(|| rest.strip_prefix('"')).or_else(|| rest.strip_prefix("&quot;"));
        if let Some(n) = rest.and_then(|r| r.get(..12)).filter(|n| n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')) {
            v.push(n.to_string());
        }
    }
    v.sort();
    v.dedup();
    v
}

/// `a` and `b` asked for at once, `a` first: their bodies.
fn at_once(port: u16, a: &str, b: &str) -> (String, String) {
    let a = a.to_string();
    let first = std::thread::spawn(move || support::send(port, "GET", &a, &[], "").body);
    std::thread::sleep(Duration::from_millis(80));
    let second = support::send(port, "GET", b, &[], "").body;
    (first.join().unwrap(), second)
}

// The fourth review of slice W1 (Q-667): the answer's nonce for markup made at load was the thread's,
// and requests are coroutines sharing a thread: a page that waited (`sleep`, a waiting part) wrote
// the nonce of the answer made meanwhile, another visitor's, into its island of `nav`, which was then
// dead. Each answer writes its own: its interpreter's, which goes with it.
#[test]
fn a_page_that_waits_writes_only_its_own_nonce() {
    let src = format!(
        "{}let nav = <Card n={{9}}/>\nroute GET /slow {{\n    sleep(300)\n    return <html><body><Card n={{0}}/>{{nav}}</body></html>\n}}\nroute GET /fast = <html><body><Card n={{1}}/></body></html>\nserver fn slow() {{\n    sleep(300)\n    return 1\n}}\ncomponent Later() {{\n    state r = defer(slow())\n    <div>{{r == null ? \"wait\" : nav}}</div>\n}}\nroute GET /def = <html><body><Card n={{0}}/><Later/></body></html>\n",
        CARD_N
    );
    let d = dir("waits", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    for slow in ["/slow", "/def"] {
        for _ in 0..3 {
            let (a, b) = at_once(s.port, slow, "/fast");
            let (na, nb) = (all_nonces(&a), all_nonces(&b));
            assert!(a.contains("9 shut") && b.contains("1 shut"), "{}\n{}", a, b);
            assert_eq!((na.len(), nb.len()), (1, 1), "{}: {:?} {:?}\n{}", slow, na, nb, a);
            assert_ne!(na, nb, "{}", slow);
        }
    }
}

// The fourth review of slice W1 (Q-667): markup made at load shown by a view (`view Shell = …{nav}…`)
// was kept by the view cache with the first answer's nonce, given to every later visitor, whose island
// was dead. Such a render is not kept. And the markup made at load anywhere: in a map however deep,
// in a function's scope, by `parallel` at load, a route's whole answer; and a page showing only it
// has its loader. JSON and `str()` of it keep the load's nonce, which no page takes up.
#[test]
fn markup_made_at_load_has_each_answers_nonce_wherever_it_is() {
    let src = format!(
        "{}let nav = <Card n={{9}}/>\nlet parts = parallel([() => <Card n={{1}}/>, () => <Card n={{2}}/>])\nlet deep = {{ a: {{ b: {{ c: {{ d: {{ e: {{ f: [[{{ nav: <Card n={{3}}/> }}]] }} }} }} }} }} }}\nfn make() {{\n    let x = <Card n={{4}}/>\n    return () => x\n}}\nlet getx = make()\nview Shell(title: str) = <header><h1>{{title}}</h1>{{nav}}</header>\nroute GET /view = <html><body><Card n={{0}}/><Shell title=\"a\"/></body></html>\nroute GET /only = <html><body>{{nav}}</body></html>\nroute GET /par = <html><body><Card n={{0}}/>{{parts}}</body></html>\nroute GET /deep = <html><body><Card n={{0}}/>{{deep.a.b.c.d.e.f[0][0].nav}}</body></html>\nroute GET /clo = <html><body><Card n={{0}}/>{{getx()}}</body></html>\nroute GET /bare = nav\nroute GET /json = {{ nav: nav }}\nroute GET /text = str(nav)\n",
        CARD_N
    );
    let d = dir("load-kept", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| support::send(s.port, "GET", p, &[], "").body;
    let load = all_nonces(&get("/json"));
    assert_eq!(load.len(), 1);
    assert_eq!(all_nonces(&get("/text")), load);
    for p in ["/view", "/only", "/par", "/deep", "/clo", "/bare"] {
        let (a, b) = (get(p), get(p));
        let (na, nb) = (all_nonces(&a), all_nonces(&b));
        assert_eq!((na.len(), nb.len()), (1, 1), "{}: {:?} {:?}\n{}\n{}", p, na, nb, a, b);
        assert_ne!(na, nb, "{}\n{}\n{}", p, a, b);
        assert_ne!(na, load, "{}\n{}", p, a);
    }
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    let script = r#"(async () => {
  BOOTED
  const out = {};
  for (const id of ["c0", "c1", "c2", "c3", "c4", "c9"]) {
    const b = document.getElementById(id);
    if (!b) continue;
    b.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    b.click();
    await tick(150);
    out[id] = b.textContent;
  }
  document.body.dataset.result = JSON.stringify(out);
})();"#
        .replace("BOOTED", BOOTED);
    // The view's page twice: the second from the view cache, were it kept.
    for _ in 0..2 {
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/view", s.port), &script);
        assert_eq!(r, serde_json::json!({ "c0": "0 open", "c9": "9 open" }), "{}", r);
    }
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/only", s.port), &script);
    assert_eq!(r, serde_json::json!({ "c9": "9 open" }), "{}", r);
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/deep", s.port), &script);
    assert_eq!(r, serde_json::json!({ "c0": "0 open", "c3": "3 open" }), "{}", r);
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/clo", s.port), &script);
    assert_eq!(r, serde_json::json!({ "c0": "0 open", "c4": "4 open" }), "{}", r);
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/par", s.port), &script);
    assert_eq!(r, serde_json::json!({ "c0": "0 open", "c1": "1 open", "c2": "2 open" }), "{}", r);
}

// The fourth review of slice W1: a `Vary` on the nonce's requests kept a page fetched ahead
// (FEEDBACK22's own fetch, `x-quillo-prefetch`) from serving the click, and the page was made twice.
// The answer fetched ahead serves the plain request after it.
#[test]
fn a_page_fetched_ahead_serves_the_click() {
    let src = format!("{}route GET /a = <html><body><a href=\"/b\">b</a><Card n={{0}}/></body></html>\nroute GET /b = <html><body><Card n={{1}}/></body></html>\n", CARD_N);
    let d = dir("ahead", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let ahead = support::send(s.port, "GET", "/b", &[("x-quillo-prefetch", "1")], "");
    assert!(ahead.header("x-quillo-nonce").is_some(), "{:?}", ahead.headers);
    assert!(!ahead.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("vary") && v.contains("x-quillo")), "{:?}", ahead.headers);
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    // Asked ahead again while the server says it is busy (a 503 the page's own loading can cause,
    // not stored); then the plain request, which the browser serves from what it fetched.
    let script = r#"(async () => {
  let status = 0;
  for (let i = 0; i < 40 && status !== 200; i++) {
    if (i) await new Promise((r) => setTimeout(r, 100));
    const a = await fetch("/b", { credentials: "same-origin", headers: { "x-quillo-prefetch": "1" } });
    await a.text();
    status = a.status;
  }
  const b = await fetch("/b", { credentials: "same-origin" });
  await b.text();
  const sizes = performance.getEntriesByName(new URL("/b", location.href).href).map((x) => x.transferSize);
  document.body.dataset.result = JSON.stringify({ status, sizes });
})();"#;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/a", s.port), script);
    let sizes: Vec<u64> = r["sizes"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
    assert_eq!(r["status"], 200, "{}", r);
    assert!(sizes.len() >= 2 && sizes[sizes.len() - 2] > 0 && sizes[sizes.len() - 1] == 0, "the click went to the server: {}", r);
}

// Kept-markup hardening: `respond(200, nav)` and `reply 200 nav` stamp the load-time markup with
// each answer's nonce too (just like returning `nav` directly), and nested lists of kept markup
// grow stack safely.
#[test]
fn respond_and_reply_of_load_markup_keep_per_answer_nonces() {
    let src = format!(
        "{}let nav = <Card n={{9}}/>\nroute GET /resp = respond(200, nav)\nroute GET /rep = {{ reply 200 nav }}\n",
        CARD_N
    );
    let d = dir("load-resp", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| support::send(s.port, "GET", p, &[], "").body;
    for p in ["/resp", "/rep"] {
        let (a, b) = (get(p), get(p));
        let (na, nb) = (all_nonces(&a), all_nonces(&b));
        assert_eq!((na.len(), nb.len()), (1, 1), "{}: {:?} {:?}\n{}\n{}", p, na, nb, a, b);
        assert_ne!(na, nb, "{}\n{}\n{}", p, a, b);
    }
}
