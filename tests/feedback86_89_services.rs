//! FEEDBACK89.md, services and `idempotent` (branch feedback86-89):
//! - Q-1394: a call made inside an `idempotent` run gets an id derived from the run's key and its
//!   place in the run, so the same key again sends the same ids down every link (the user's design,
//!   "chained retries", 2026-11-24), and the chain's root crosses the wire signed (`x-quillo-root`);
//! - Q-1391: with `{ transaction: false }`, an early reply in place of a call that was refused
//!   before it was sent is not kept; one in place of a call that may have run still is.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SECRET: &str = "5f0c2a9d4e7b16a38c0d92f4b7e15a6c3d8f0b2e4a6c8e0f1a3b5c7d9e1f2a4b";

// ---- in one process --------------------------------------------------------------------------

/// The programs' own secret: chained retries derive ids only with one (the security review's q1),
/// which the programs these tests start inherit, and this process's in-process ones read.
const OWN_SECRET: &str = "own-secret-of-the-shop-0123456789abcdef0123456789abcdef";

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| std::env::set_var("QUILLO_SECRET", OWN_SECRET));
    let d = support::temp_dir(&format!("f86-89-services-{}", name));
    for (f, src) in files {
        std::fs::write(d.join(f), src).unwrap();
    }
    d
}

fn send(it: &mut Interp, method: &str, url: &str, key: Option<&str>) -> HttpResponse {
    let mut headers = vec![("Host".to_string(), "localhost".to_string())];
    if let Some(k) = key {
        headers.push(("Idempotency-Key".into(), k.into()));
    }
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers, body: Vec::new(), remote: "127.0.0.1".into() })
}

fn text(r: &HttpResponse) -> String {
    String::from_utf8_lossy(&r.body).into_owned()
}

fn hex_of(id: &str, n: usize) -> bool {
    id.len() == n && id.bytes().all(|b| b.is_ascii_hexdigit())
}

const WHO: &str = r#"let calls = []
server fn who(note: str) -> str {
    calls = calls + [request.call]
    request.call
}
server fn seen() -> list<str> = calls
"#;

const IDS_SHOP: &str = r#"service billing = "billing.qwe"
db.open("shop.db")
let failing = 0
route POST /failing/:n {
    failing = int(n)
    "ok"
}
route POST /three with http, db = idempotent(() => {
    let a = billing.who("a") ?? reply 503 "busy"
    let b = billing.who("ünï ☕") ?? reply 503 "busy"
    let c = billing.who("a") ?? reply 503 "busy"
    if failing > 0 {
        failing -= 1
        reply 503 "once more"
    }
    join([a, b, c], ",")
})
route POST /plain with http = billing.who("p") ?? "none"
route POST /seen with http = join(billing.seen() ?? [], ",")
"#;

fn seen(it: &mut Interp) -> Vec<String> {
    let r = send(it, "POST", "/seen", None);
    text(&r).split(',').filter(|s| !s.is_empty()).map(String::from).collect()
}

/// Q-1394: the same key again sends the same ids, each call its own (the n-th call of a function
/// apart from the first, with the same arguments too); another key sends others; a call outside a
/// keyed run gets a new id each time, as before. Before, each run's calls had new ids.
#[test]
fn q1394_the_same_key_sends_the_same_ids_and_another_key_others() {
    let d = dir("ids", &[("billing.qwe", WHO), ("main.qwe", IDS_SHOP)]);
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    assert_eq!(text(&send(&mut it, "POST", "/failing/1", None)), "ok");
    let first = send(&mut it, "POST", "/three", Some("k1"));
    assert_eq!((first.status, text(&first)), (503, "once more".to_string()));
    let again = send(&mut it, "POST", "/three", Some("k1"));
    assert_eq!(again.status, 200, "{}", text(&again));
    let ids = seen(&mut it);
    assert_eq!(ids.len(), 6, "{:?}", ids);
    assert_eq!(ids[..3], ids[3..], "the same key sent other ids: {:?}", ids);
    assert_eq!(text(&again), ids[..3].join(","));
    assert!(ids.iter().all(|i| hex_of(i, 16)), "{:?}", ids);
    // Three calls, three ids: the third is the second call of `who`, with the first's arguments.
    assert!(ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2], "{:?}", ids);
    // Another key, other ids.
    assert_eq!(send(&mut it, "POST", "/three", Some("k2")).status, 200);
    let more = seen(&mut it);
    assert!(more[6..].iter().all(|i| !ids.contains(i)), "{:?}", more);
    // The shortest and the longest key there is, and one past it.
    for key in ["a".to_string(), "k".repeat(255)] {
        let r = send(&mut it, "POST", "/three", Some(&key));
        assert_eq!(r.status, 200, "{}", text(&r));
        assert!(text(&r).split(',').all(|i| hex_of(i, 16)), "{}", text(&r));
    }
    assert_eq!(send(&mut it, "POST", "/three", Some(&"k".repeat(256))).status, 400);
    // No key: the run is not kept, and its calls are new ones, as before.
    let a = text(&send(&mut it, "POST", "/three", None));
    let b = text(&send(&mut it, "POST", "/three", None));
    assert!(a.split(',').all(|i| hex_of(i, 16)) && a != b, "{} / {}", a, b);
    // Outside `idempotent`: a new id for each call.
    let (p, q) = (text(&send(&mut it, "POST", "/plain", None)), text(&send(&mut it, "POST", "/plain", None)));
    assert!(hex_of(&p, 16) && hex_of(&q, 16) && p != q, "{} / {}", p, q);
}

const LEDGER: &str = r#"db.open("ledger.db")
db.exec("create table if not exists postings (id integer primary key, ref text not null, cents int not null)")
server fn post(ref: str, cents: int) -> map with db = idempotent(() => db.one("insert into postings (ref, cents) values (?, ?) returning id, ref", ref, cents))
server fn total() -> int with db = db.one("select count(*) as n from postings").n
route GET /n with db = str(db.one("select count(*) as n from postings").n)
"#;

/// Billing with no `idempotent`: it posts, then fails once after posting, when told to.
const PLAIN_BILLING: &str = r#"service ledger = "ledger.qwe"
db.open("billing.db")
db.exec("create table if not exists flags (n int)")
server fn fail_next() -> int with db = db.exec("insert into flags (n) values (1)").changes
server fn invoice(name: str, cents: int) -> map ! str with db, http {
    let posted = ledger.post(`for-${name}`, cents)?
    if db.one("select count(*) as n from flags").n > 0 {
        db.exec("delete from flags")
        return err("billing failed after posting")
    }
    { name, posting: posted.id }
}
"#;

const PLAIN_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0 }
service ledger = "ledger.qwe"
db.open("shop.db")
route POST /fail-next with http = str(billing.fail_next() ?? -1)
route POST /buy with http, db = idempotent(() => {
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "busy"
    `posting #${inv.posting}`
})
route GET /postings with http = str(ledger.total() ?? -1)
"#;

/// Q-1394: a link with no `idempotent` of its own passes the chain on: run again under the same id,
/// its own call gets the same id, and the ledger answers from its record. Before, the ledger posted
/// twice.
#[test]
fn q1394_a_link_that_is_not_idempotent_passes_the_chain_on_in_one_process() {
    let d = dir("plain-link", &[("ledger.qwe", LEDGER), ("billing.qwe", PLAIN_BILLING), ("main.qwe", PLAIN_SHOP)]);
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    assert_eq!(text(&send(&mut it, "POST", "/fail-next", None)), "1");
    let r = send(&mut it, "POST", "/buy", Some("k1"));
    assert_eq!((r.status, text(&r)), (503, "busy".to_string()));
    assert_eq!(text(&send(&mut it, "GET", "/postings", None)), "1");
    let r = send(&mut it, "POST", "/buy", Some("k1"));
    assert_eq!((r.status, text(&r)), (200, "posting #1".to_string()));
    assert_eq!(text(&send(&mut it, "GET", "/postings", None)), "1", "the ledger posted again");
    // Another key is another purchase.
    assert_eq!(text(&send(&mut it, "POST", "/buy", Some("k2"))), "posting #2");
}

// ---- over the network ------------------------------------------------------------------------

/// A `quillo run` this test started: stopped when dropped, and killed by its PID when asked.
struct Proc {
    child: Child,
    port: u16,
}

impl Proc {
    fn start(d: &Path, file: &str, port: u16, extra: &[&str], env: &[(&str, &str)]) -> Proc {
        Proc::start_within(d, file, port, extra, env, Duration::from_secs(60))
    }

    fn start_within(d: &Path, file: &str, port: u16, extra: &[&str], env: &[(&str, &str)], wait: Duration) -> Proc {
        let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .current_dir(d)
            .arg("run")
            .arg(d.join(file))
            .args(["--port", &port.to_string()])
            .args(extra)
            .env("QUILLO_SERVICE_SECRET", SECRET)
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(if std::env::var("QUILLO_TEST_LOG").is_ok() { Stdio::inherit() } else { Stdio::null() })
            .spawn()
            .expect("cannot start quillo");
        support::stop_with_tests(&child);
        let mut p = Proc { child, port };
        let deadline = Instant::now() + wait;
        while !support::listening(port) {
            assert!(p.child.try_wait().ok().flatten().is_none(), "{} stopped as it started", file);
            assert!(Instant::now() < deadline, "{} did not start", file);
            std::thread::sleep(Duration::from_millis(20));
        }
        p
    }

    /// `start`, waiting ten minutes: a program with a `.qwer` plugin compiles it first.
    fn start_slow(d: &Path, file: &str, port: u16, extra: &[&str], env: &[(&str, &str)]) -> Proc {
        Proc::start_within(d, file, port, extra, env, Duration::from_secs(600))
    }

    /// SIGKILL to its PID, as a crash.
    fn kill(&mut self) {
        let _ = Command::new("kill").args(["-KILL", &self.child.id().to_string()]).status();
        let _ = self.child.wait();
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        if let Ok(Some(_)) = self.child.try_wait() {
            return;
        }
        let _ = Command::new("kill").args(["-INT", &self.child.id().to_string()]).status();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A request: its status, its head (lower case) and its body. It waits two minutes at most.
fn http(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
    let mut req = format!("{} {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n", method, path, body.len());
    for (k, v) in headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out).into_owned();
    let status = out.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let (head, body) = out.split_once("\r\n\r\n").map(|(h, b)| (h.to_ascii_lowercase(), b.to_string())).unwrap_or_default();
    (status, head, body)
}

fn buy(port: u16, path: &str, key: &str) -> (u16, bool, String) {
    let (status, head, body) = http(port, "POST", path, &[("Idempotency-Key", key)], "");
    (status, head.contains("idempotent-replayed: true"), body)
}

fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// FEEDBACK89.md's chain, billing pausing after the ledger posted (`BILLING_PAUSE`), so that it is
/// killed between its links. A route sets the claims' minute over, as a minute passing would.
const CHAIN_BILLING: &str = r#"service ledger = "ledger.qwe"
db.open("billing.db")
db.exec("create table if not exists invoices (id integer primary key, name text not null, cents int not null)")
let pause = int(env("BILLING_PAUSE", "0"))
server fn invoice(name: str, cents: int) -> map ! str with db, http, time {
    idempotent(() => {
        let inv = db.one("insert into invoices (name, cents) values (?, ?) returning id, name, cents", name, cents)
        ledger.post(`inv-${inv.id}`, cents)?
        if pause > 0 { sleep(pause) }
        inv
    })
}
server fn total() -> int with db = db.one("select count(*) as n from invoices").n
route POST /leases-over with db = str(db.exec("update _quillo_idempotency set lease_until = 0 where done = 0").changes)
route POST /calls-over with db = str(db.exec("update _quillo_calls set at = at - 700000 where status = 0").changes)
"#;

const CHAIN_SHOP: &str = r#"service billing = "billing.qwe" { timeout: "120s", retries: 0 }
service ledger = "ledger.qwe"
db.open("shop.db")
route POST /buy with http, db = idempotent(() => {
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "busy"
    `Invoice #${inv.id}`
})
route GET /counts with http = `invoices ${billing.total() ?? -1}, postings ${ledger.total() ?? -1}`
route POST /leases-over with db = str(db.exec("update _quillo_idempotency set lease_until = 0 where done = 0").changes)
"#;

fn chain_dir(name: &str) -> PathBuf {
    dir(name, &[("ledger.qwe", LEDGER), ("billing.qwe", CHAIN_BILLING), ("main.qwe", CHAIN_SHOP)])
}

/// Q-1394, the report's crash test: shop -> billing -> ledger, each a process, each `idempotent`;
/// billing killed (SIGKILL, by its PID) after the ledger posted and before its own transaction
/// committed. The same key again: before its claims are over (the call's, of eleven minutes, as a
/// signature lasts, then its `idempotent`'s, of a minute), billing says the call is still
/// running (503, not an internal error); after it, billing runs again, under the
/// same id, and finds the ledger's posting done: 1 invoice, 1 posting. Before, the ledger was called under a new id
/// and posted again: 1 invoice, 2 postings.
#[test]
fn q1394_a_chain_killed_between_its_links_posts_once_when_the_same_key_comes_again() {
    let d = chain_dir("crash");
    let (sp, bp, lp) = (support::free_port(), support::free_port(), support::free_port());
    let ledger_url = format!("ledger=http://127.0.0.1:{}", lp);
    let billing_url = format!("billing=http://127.0.0.1:{}", bp);
    let _ledger = Proc::start(&d, "ledger.qwe", lp, &["--as-service"], &[]);
    let mut billing = Proc::start(&d, "billing.qwe", bp, &["--as-service", "--service-url", &ledger_url], &[("BILLING_PAUSE", "600000")]);
    let shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &billing_url, "--service-url", &ledger_url], &[]);
    let port = shop.port;
    let first = std::thread::spawn(move || buy(port, "/buy", "k1"));
    eventually(60, "the ledger did not post", || (http(lp, "GET", "/n", &[], "").2 == "1").then_some(()));
    billing.kill();
    assert_eq!(first.join().unwrap(), (503, false, "busy".to_string()));
    let billing = Proc::start(&d, "billing.qwe", bp, &["--as-service", "--service-url", &ledger_url], &[("BILLING_PAUSE", "0")]);
    assert_eq!(http(sp, "GET", "/counts", &[], "").2, "invoices 0, postings 1");
    // Its claims are not over: still running, said so, and not kept by the shop. The call's claim
    // first, then (that one's eleven minutes over) its `idempotent`'s, which was an internal error.
    assert_eq!(buy(sp, "/buy", "k1"), (503, false, "busy".to_string()));
    assert_eq!(http(billing.port, "POST", "/calls-over", &[], "").2, "1");
    assert_eq!(buy(sp, "/buy", "k1"), (503, false, "busy".to_string()));
    assert_eq!(http(sp, "GET", "/counts", &[], "").2, "invoices 0, postings 1");
    assert_eq!(http(billing.port, "POST", "/leases-over", &[], "").2, "1");
    assert_eq!(buy(sp, "/buy", "k1"), (200, false, "Invoice #1".to_string()));
    assert_eq!(http(sp, "GET", "/counts", &[], "").2, "invoices 1, postings 1", "the ledger posted again");
    assert_eq!(buy(sp, "/buy", "k1"), (200, true, "Invoice #1".to_string()));
    // Another key is another purchase.
    assert_eq!(buy(sp, "/buy", "k2"), (200, false, "Invoice #2".to_string()));
    assert_eq!(http(sp, "GET", "/counts", &[], "").2, "invoices 2, postings 2");
}

/// The chain in one process: billing and the ledger mounted, for their routes.
const ONE_SHOP: &str = r#"service /billing = "billing.qwe" { timeout: "120s", retries: 0 }
service /ledger = "ledger.qwe"
db.open("shop.db")
route POST /buy with http, db = idempotent(() => {
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "busy"
    `Invoice #${inv.id}`
})
route GET /counts with http = `invoices ${billing.total() ?? -1}, postings ${ledger.total() ?? -1}`
route POST /leases-over with db = str(db.exec("update _quillo_idempotency set lease_until = 0 where done = 0").changes)
"#;

/// The ledger's postings, read by a program of their own.
const COUNT: &str = "db.open(\"ledger.db\")\nprint(db.one(\"select count(*) as n from postings\").n)\n";

/// What a program prints, run with a deadline of half a minute.
fn script(d: &Path, file: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).current_dir(d).args(["run", file]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().ok().flatten().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut out = String::new();
    let _ = child.stdout.take().unwrap().read_to_string(&mut out);
    let _ = child.wait();
    out.trim().to_string()
}

/// Q-1394 in one process, killed (SIGKILL, by its PID) while billing pauses after the ledger posted:
/// started again, the shop's claim taken over, billing's claim not yet a minute old: billing says the
/// call is still running (a 503, not an internal error), and nothing is kept; once its minute is
/// over, billing runs again under the same id and finds the posting done. Before, billing ran under
/// a new id at once, and the ledger posted twice.
#[test]
fn q1394_one_process_killed_mid_chain_posts_once_when_started_again() {
    let d = dir("one-process", &[("ledger.qwe", LEDGER), ("billing.qwe", CHAIN_BILLING), ("main.qwe", ONE_SHOP), ("count.qwe", COUNT)]);
    let port = support::free_port();
    let mut app = Proc::start(&d, "main.qwe", port, &[], &[("BILLING_PAUSE", "600000")]);
    let first = std::thread::spawn(move || http(port, "POST", "/buy", &[("Idempotency-Key", "k1")], ""));
    // The app answers nothing else while billing pauses in its call: the ledger's file is read.
    eventually(60, "the ledger did not post", || (script(&d, "count.qwe") == "1").then_some(()));
    app.kill();
    let _ = first.join();
    let _app = Proc::start(&d, "main.qwe", port, &[], &[("BILLING_PAUSE", "0")]);
    assert_eq!(http(port, "GET", "/counts", &[], "").2, "invoices 0, postings 1");
    assert_eq!(http(port, "POST", "/leases-over", &[], "").2, "1");
    assert_eq!(buy(port, "/buy", "k1"), (503, false, "busy".to_string()));
    assert_eq!(http(port, "POST", "/billing/leases-over", &[], "").2, "1");
    assert_eq!(buy(port, "/buy", "k1"), (200, false, "Invoice #1".to_string()));
    assert_eq!(http(port, "GET", "/counts", &[], "").2, "invoices 1, postings 1", "the ledger posted again");
}

/// Q-1394, a restart at the top: the shop killed while billing runs its call, billing finishing it;
/// the shop started again, its claim's minute over: the same key sends the same id, and billing
/// answers from its record. Before, billing was called under a new id and billed twice.
#[test]
fn q1394_the_first_link_killed_mid_call_bills_once_when_started_again() {
    let d = chain_dir("top");
    let (sp, bp, lp) = (support::free_port(), support::free_port(), support::free_port());
    let ledger_url = format!("ledger=http://127.0.0.1:{}", lp);
    let billing_url = format!("billing=http://127.0.0.1:{}", bp);
    let _ledger = Proc::start(&d, "ledger.qwe", lp, &["--as-service"], &[]);
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service", "--service-url", &ledger_url], &[("BILLING_PAUSE", "1500")]);
    let mut shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &billing_url, "--service-url", &ledger_url], &[]);
    let first = std::thread::spawn(move || http(sp, "POST", "/buy", &[("Idempotency-Key", "k1")], ""));
    eventually(60, "the ledger did not post", || (http(lp, "GET", "/n", &[], "").2 == "1").then_some(()));
    shop.kill();
    let _ = first.join();
    let shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &billing_url, "--service-url", &ledger_url], &[]);
    // Billing's call ends after its pause, and is kept there.
    eventually(60, "billing did not finish", || (http(sp, "GET", "/counts", &[], "").2 == "invoices 1, postings 1").then_some(()));
    assert_eq!(http(shop.port, "POST", "/leases-over", &[], "").2, "1");
    assert_eq!(buy(sp, "/buy", "k1"), (200, false, "Invoice #1".to_string()));
    assert_eq!(http(sp, "GET", "/counts", &[], "").2, "invoices 1, postings 1", "billing billed again");
}

/// Q-1394 with two copies of a link that is not idempotent, sharing its database, as `--scale
/// billing=2` runs them: its error answer is not kept for the chain's id, and the retry, on either
/// copy, runs again and sends the ledger the same id. Before, the ledger posted twice.
#[test]
fn q1394_a_link_that_is_not_idempotent_passes_the_chain_on_across_its_copies() {
    let d = dir("plain-copies", &[("ledger.qwe", LEDGER), ("billing.qwe", PLAIN_BILLING), ("main.qwe", PLAIN_SHOP)]);
    let (sp, b1, b2, lp) = (support::free_port(), support::free_port(), support::free_port(), support::free_port());
    let ledger_url = format!("ledger=http://127.0.0.1:{}", lp);
    let billing_urls = format!("billing=http://127.0.0.1:{},http://127.0.0.1:{}", b1, b2);
    let _ledger = Proc::start(&d, "ledger.qwe", lp, &["--as-service"], &[]);
    let _one = Proc::start(&d, "billing.qwe", b1, &["--as-service", "--service-url", &ledger_url], &[]);
    let _two = Proc::start(&d, "billing.qwe", b2, &["--as-service", "--service-url", &ledger_url], &[]);
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &billing_urls, "--service-url", &ledger_url], &[]);
    for round in 0..2 {
        assert_eq!(http(sp, "POST", "/fail-next", &[], "").2, "1");
        let key = format!("k{}", round);
        assert_eq!(buy(sp, "/buy", &key), (503, false, "busy".to_string()));
        let want = format!("posting #{}", round + 1);
        assert_eq!(buy(sp, "/buy", &key), (200, false, want.clone()));
        assert_eq!(buy(sp, "/buy", &key), (200, true, want));
        assert_eq!(http(sp, "GET", "/postings", &[], "").2, (round + 1).to_string(), "the ledger posted again");
    }
}

const WHO_CALL: &str = "server fn who() -> str = request.call\n";

/// Billing, slow for `slow_who`, which says when it has begun.
const SLOW_WHO: &str = r#"db.open("billing.db")
db.exec("create table if not exists arrived (n int)")
server fn slow_who() -> str with db, time {
    db.exec("insert into arrived (n) values (1)")
    sleep(1500)
    request.call
}
server fn arrived() -> int with db = db.one("select count(*) as n from arrived").n
server fn who() -> str = request.call
"#;

const WAITING_SHOP: &str = r#"service billing = "billing.qwe" { timeout: "60s", retries: 0 }
db.open("shop.db")
route POST /a with http, db = idempotent({ transaction: false }, () => billing.slow_who() ?? reply 503 "busy")
route POST /arrived with http = str(billing.arrived() ?? -1)
route POST /b with http = billing.who() ?? "none"
"#;

/// Q-1394: a request in a chain that waits for its call (no transaction holds its thread) lends its
/// link to no other: the server answers others on its thread meanwhile, and their calls keep new
/// ids of their own. Kept per thread, the second request's call got an id of the first's chain.
#[test]
fn q1394_a_request_waiting_in_a_chain_lends_its_link_to_no_other() {
    let d = dir("waiting", &[("billing.qwe", SLOW_WHO), ("main.qwe", WAITING_SHOP)]);
    let (sp, bp) = (support::free_port(), support::free_port());
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    let (at, calls) = proxy(bp, false, 0);
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", at)], &[]);
    let first = std::thread::spawn(move || buy(sp, "/a", "k1"));
    eventually(60, "billing did not begin", || (http(sp, "POST", "/arrived", &[], "").2 == "1").then_some(()));
    let (status, _, other) = http(sp, "POST", "/b", &[], "");
    assert!(status == 200 && hex_of(&other, 16), "{} {}", status, other);
    let (status, replayed, id) = first.join().unwrap();
    assert!(status == 200 && !replayed && hex_of(&id, 16), "{} {}", status, id);
    assert_eq!(buy(sp, "/a", "k1"), (200, true, id));
    // Only the first request's call was made in its chain.
    let calls = calls.lock().unwrap().clone();
    assert!(calls.iter().any(|(path, root)| path.ends_with("/slow_who") && *root), "{:?}", calls);
    assert!(calls.iter().filter(|(path, _)| !path.ends_with("/slow_who")).all(|(_, root)| !root), "{:?}", calls);
}

/// A call as a Quillo service signs it with the shared secret at time `t`, with `root` in
/// `x-quillo-root` as `word` makes it.
fn signed_call(port: u16, t: u64, f: &str, call: &str, root: Option<String>) -> (u16, String) {
    let sig = quillo::svcauth::sign3(SECRET, t, "billing", f, "", call, "", "shop", "5000", "\n", b"[]");
    let service = format!("v=3,kid=,t={},sig={}", t, sig);
    let mut headers = vec![("x-quillo-service", service.as_str()), ("x-quillo-to", "billing"), ("x-quillo-call", call), ("x-quillo-caller", "shop"), ("x-quillo-deadline", "5000"), ("content-type", "application/json")];
    let root = root.unwrap_or_default();
    if !root.is_empty() {
        headers.push(("x-quillo-root", root.as_str()));
    }
    let (status, _, body) = http(port, "POST", &format!("/_quillo/service/{}", f), &headers, "[]");
    (status, body)
}

/// `x-quillo-root` as the guide's wire format says, for a call of billing's `f` in no tenant:
/// `<root>;sig=<hex of HMAC-SHA256(key, "quillo-root-1\n<to>\n<fn>\n<tenant>\n<call>\n<root>")>`.
fn word(key: &str, f: &str, call: &str, root: &str) -> String {
    let msg = format!("quillo-root-1\nbilling\n{}\n\n{}\n{}", f, call, root);
    format!("{};sig={}", root, quillo::crypto::hex(&quillo::crypto::hmac_sha256(key.as_bytes(), msg.as_bytes())))
}

/// Q-1394: `x-quillo-root` is believed only signed with the call's key and bound to the call: moved
/// to another call, changed, signed with another key or no word at all, the call is refused, as a
/// call changed on its way is. A call without one (from a Quillo before 0.8.12) is taken as before.
/// Before, the header was not read, and each of these was run.
#[test]
fn q1394_x_quillo_root_is_signed_and_bound_to_its_call() {
    let d = dir("root-word", &[("billing.qwe", RELAY), ("ledger.qwe", WHO_CALL)]);
    let port = support::free_port();
    let _billing = Proc::start(&d, "billing.qwe", port, &["--as-service"], &[]);
    let root = "0123456789abcdef0123456789abcdef";
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    // As 0.8.11 sends it, and with its root.
    assert_eq!(signed_call(port, t, "who", "c0", None), (200, r#"{"ok":"c0"}"#.to_string()));
    assert_eq!(signed_call(port, t, "who", "c1", Some(word(SECRET, "who", "c1", root))), (200, r#"{"ok":"c1","chain":1}"#.to_string()));
    // The longest root there is, 64 letters and digits.
    assert_eq!(signed_call(port, t, "who", "c2", Some(word(SECRET, "who", "c2", &"r".repeat(64)))).0, 200);
    // Moved to another call (a replay of its word), changed, another key's, another function's, too
    // long, not a word.
    let refused = [
        ("c3", word(SECRET, "who", "c1", root)),
        ("c4", word(SECRET, "who", "c4", root).replacen('0', "1", 1)),
        ("c5", word("another secret, not the services'", "who", "c5", root)),
        ("c6", word(SECRET, "relay", "c6", root)),
        ("c7", word(SECRET, "who", "c7", &"r".repeat(65))),
        ("c8", word(SECRET, "who", "c8", "ünï")),
        ("c9", root.to_string()),
        ("c10", ";sig=".to_string()),
    ];
    for (call, w) in refused {
        let (status, body) = signed_call(port, t, "who", call, Some(w.clone()));
        assert_eq!(status, 401, "{} with {}: {}", call, w, body);
        assert!(body.contains("x-quillo-root is not signed with this call"), "{}", body);
    }
    // In billing's own `idempotent` run, its call of the ledger gets an id derived from this call's;
    // in a tool's call with no asking (`u` first), the id says so too, as a new one does.
    // Its answers say `"chain": 1` (0.8.12 keeps chained records), which the id is read past.
    let id = |body: &str| serde_json::from_str::<serde_json::Value>(body).ok().and_then(|j| j["ok"].as_str().map(str::to_string)).unwrap_or_default();
    let (status, plain) = signed_call(port, t, "relay", "c20", None);
    assert!(status == 200 && hex_of(&id(&plain), 16), "{} {}", status, plain);
    assert_eq!(signed_call(port, t, "relay", "c20", None), (200, plain.clone()));
    let (status, unseen) = signed_call(port, t, "relay", "u0123456789abcde", None);
    let unseen = id(&unseen);
    assert!(status == 200 && unseen.starts_with('u') && hex_of(&unseen[1..], 15), "{} {}", status, unseen);
}

/// Billing for `q1394_x_quillo_root_is_signed_and_bound_to_its_call`: `who`, and `relay`, whose
/// `idempotent` run calls the ledger, in billing's process, for its call's id.
const RELAY: &str = r#"service ledger = "ledger.qwe"
db.open("billing.db")
server fn who() -> str = request.call
server fn relay() -> str with db, http = idempotent(() => ledger.who() ?? "none")
"#;

/// Q-1394: a service that does not read `x-quillo-root` (a Quillo before 0.8.12, or one in another
/// language) takes a chained call as before: the call's own signature does not cover it. Here a
/// proxy drops it on its way to billing, which answers the call, its id the chain's. Before, no
/// call carried a root, and a call's id was a new one.
#[test]
fn q1394_a_service_that_does_not_read_the_root_takes_the_call_as_before() {
    let shop = "service billing = \"billing.qwe\" { retries: 0 }\ndb.open(\"shop.db\")\nroute POST /who with http, db = idempotent(() => billing.who() ?? reply 503 \"busy\")\nroute POST /plain with http = billing.who() ?? \"none\"\n";
    let d = dir("old-service", &[("billing.qwe", WHO_CALL), ("main.qwe", shop)]);
    let (sp, bp) = (support::free_port(), support::free_port());
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    let (at, calls) = proxy(bp, true, 0);
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", at)], &[]);
    let (status, _, id) = http(sp, "POST", "/who", &[("Idempotency-Key", "k1")], "");
    assert_eq!(status, 200, "{}", id);
    assert!(hex_of(&id, 16), "{}", id);
    let roots = |c: &Arc<Mutex<Vec<(String, bool)>>>| c.lock().unwrap().iter().map(|(_, r)| *r).collect::<Vec<_>>();
    assert_eq!(roots(&calls), vec![true], "the call did not carry its root");
    // Outside a chain, as before: no root, a new id.
    let (status, _, id) = http(sp, "POST", "/plain", &[], "");
    assert!(status == 200 && hex_of(&id, 16), "{} {}", status, id);
    assert_eq!(roots(&calls), vec![true, false]);
}

/// A proxy to `port`, a thread a connection and one request each: it records each request's path
/// and whether it carried `x-quillo-root`, drops that header where `drop_root` says (as a service
/// that does not read it would ignore it), and gives its first `bad` answers as a 502 of its own, the
/// service's answer to a request it passed on thrown away. Its port, and what it saw.
fn proxy(port: u16, drop_root: bool, bad: usize) -> (u16, Arc<Mutex<Vec<(String, bool)>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let at = l.local_addr().unwrap().port();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let seen = calls.clone();
    let bad = Arc::new(std::sync::atomic::AtomicUsize::new(bad));
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let (seen, bad) = (seen.clone(), bad.clone());
            std::thread::spawn(move || pass_one(c, port, drop_root, &seen, &bad));
        }
    });
    (at, calls)
}

fn pass_one(mut c: TcpStream, port: u16, drop_root: bool, seen: &Mutex<Vec<(String, bool)>>, bad: &std::sync::atomic::AtomicUsize) {
    let _ = c.set_read_timeout(Some(Duration::from_secs(60)));
    let mut buf = Vec::new();
    let mut b = [0u8; 4096];
    let head_end = loop {
        let Ok(n) = c.read(&mut b) else { return };
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&b[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let length: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
    while buf.len() < head_end + length {
        let Ok(n) = c.read(&mut b) else { break };
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&b[..n]);
    }
    let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
    let mut had = false;
    let mut out = String::new();
    for line in head.split("\r\n").filter(|l| !l.is_empty()) {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("x-quillo-root:") {
            had = true;
            if drop_root {
                continue;
            }
        }
        if !lower.starts_with("connection:") {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }
    out.push_str("Connection: close\r\n\r\n");
    // The question a caller asks an address before its first chained call (`_contracts`): passed on
    // as it is, neither recorded nor answered badly, so that the calls below are what is tested.
    let probe = path.ends_with("/_contracts");
    if !probe {
        seen.lock().unwrap().push((path, had));
    }
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else { return };
    let _ = s.set_read_timeout(Some(Duration::from_secs(60)));
    let _ = s.write_all(out.as_bytes());
    let _ = s.write_all(&buf[head_end..]);
    let mut answer = Vec::new();
    let _ = s.read_to_end(&mut answer);
    let order = std::sync::atomic::Ordering::SeqCst;
    let mut left = bad.load(order);
    let taken = !probe && loop {
        if left == 0 {
            break false;
        }
        match bad.compare_exchange(left, left - 1, order, order) {
            Ok(_) => break true,
            Err(now) => left = now,
        }
    };
    if taken {
        let _ = c.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-type: text/plain\r\ncontent-length: 11\r\nconnection: close\r\n\r\nbad gateway");
    } else {
        let _ = c.write_all(&answer);
    }
}

// ---- Q-1391: `{ transaction: false }` and an early reply --------------------------------------

/// Billing, slow once when told (`slow_next`): past its caller's timeout.
const Q1391_BILLING: &str = r#"db.open("billing.db")
db.exec("create table if not exists invoices (id integer primary key, name text not null, cents int not null)")
db.exec("create table if not exists slow (n int)")
server fn slow_next() -> int with db = db.exec("insert into slow (n) values (1)").changes
server fn total() -> int with db = db.one("select count(*) as n from invoices").n
route POST /forget with db = str(db.exec("delete from _quillo_calls").changes)
server fn invoice(name: str, cents: int) -> map ! str with db, time {
    if cents <= 0 { return err("An invoice is for something") }
    if db.one("select count(*) as n from slow").n > 0 {
        db.exec("delete from slow")
        sleep(1500)
    }
    db.one("insert into invoices (name, cents) values (?, ?) returning id, name, cents", name, cents)
}
"#;

fn q1391_shop(opts: &str) -> String {
    format!(
        r#"service billing = "billing.qwe" {}
service mail = "mail.qwe" {{ retries: 0 }}
db.open("shop.db")
route POST / with http, db = idempotent({{ transaction: false }}, () => {{
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "Billing est occupé ☕"
    `Invoice #${{inv.id}}`
}})
route POST /free with http, db = idempotent({{ transaction: false }}, () => {{
    let sent = mail.note("hi") ?? "not sent"
    let inv = billing.invoice("Ada", 0) ?? reply 422 "refused"
    `Invoice #${{inv.id}} ${{sent}}`
}})
route POST /closed with http, db = idempotent({{ transaction: false }}, () => {{
    reply 503 "closed"
}})
let bank = env("BANK", "http://127.0.0.1:9")
route POST /bank with http, db = idempotent({{ transaction: false }}, () => {{
    let _paid = http.post(bank, {{ cents: 1200 }}) ?? reply 503 "the bank is busy"
    "paid"
}})
route POST /mail-then-bank with http, db = idempotent({{ transaction: false }}, () => {{
    let _sent = mail.note("hi") ?? "not sent"
    let _paid = http.post(bank, {{ cents: 1200 }}) ?? reply 503 "the bank is busy"
    "paid"
}})
route POST /slow-next with http = str(billing.slow_next() ?? -1)
route POST /count with http = str(billing.total() ?? -1)
"#,
        opts
    )
}

const MAIL: &str = "server fn note(text: str) -> str = text\n";

/// Q-1391: billing not running (connection refused): the early reply is not kept, and the same key,
/// once billing is up, gets the invoice (a success is kept). Before, the 503 was kept and replayed
/// until the key expired.
#[test]
fn q1391_an_early_reply_for_a_call_that_never_left_is_not_kept() {
    let d = dir("never-left", &[("billing.qwe", Q1391_BILLING), ("mail.qwe", MAIL), ("main.qwe", &q1391_shop("{ retries: 0 }"))]);
    let (sp, bp, mp) = (support::free_port(), support::free_port(), support::free_port());
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", bp), "--service-url", &format!("mail=http://127.0.0.1:{}", mp)], &[]);
    let busy = (503, false, "Billing est occupé ☕".to_string());
    assert_eq!(buy(sp, "/", "k1"), busy);
    assert_eq!(buy(sp, "/", "k1"), busy, "the reply was kept");
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    assert_eq!(buy(sp, "/", "k1"), (200, false, "Invoice #1".to_string()));
    assert_eq!(buy(sp, "/", "k1"), (200, true, "Invoice #1".to_string()));
    // Only the call it replaced counts: mail, never reached before it, does not let go of a
    // refusal billing gave, which ran.
    assert_eq!(buy(sp, "/free", "k2"), (422, false, "refused".to_string()));
    assert_eq!(buy(sp, "/free", "k2"), (422, true, "refused".to_string()));
    // A reply with no call before it is kept, as before.
    assert_eq!(buy(sp, "/closed", "k3"), (503, false, "closed".to_string()));
    assert_eq!(buy(sp, "/closed", "k3"), (503, true, "closed".to_string()));
}

/// Q-1391: an open circuit sends nothing: its early reply is not kept either. One failure opens it
/// (`failures: 1`), for an hour. Before, the second try got the first's 503, replayed.
#[test]
fn q1391_an_open_circuit_answered_early_is_not_kept() {
    let d = dir("circuit", &[("billing.qwe", Q1391_BILLING), ("mail.qwe", MAIL), ("main.qwe", &q1391_shop(r#"{ retries: 0, failures: 1, cooldown: "3600s" }"#))]);
    let (sp, bp, mp) = (support::free_port(), support::free_port(), support::free_port());
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", bp), "--service-url", &format!("mail=http://127.0.0.1:{}", mp)], &[]);
    let busy = (503, false, "Billing est occupé ☕".to_string());
    // Refused: the circuit opens.
    assert_eq!(buy(sp, "/", "k0"), busy);
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    // Up now, but left alone for an hour: nothing is sent, and nothing kept.
    assert_eq!(buy(sp, "/", "k1"), busy);
    assert_eq!(buy(sp, "/", "k1"), busy, "the reply was kept");
}

/// Q-1391, back to its first decision: a call of a Quillo service sent and not answered in its time
/// (billing slow once) may have run, and the early reply in its place is kept, as in 0.8.11: the
/// service's record of the call lasts eleven minutes, the key a day. Changed by the batch's last
/// decision (round 2 let it go).
#[test]
fn q1391_an_early_reply_after_a_timeout_is_kept() {
    let d = dir("no-answer", &[("billing.qwe", Q1391_BILLING), ("mail.qwe", MAIL), ("main.qwe", &q1391_shop(r#"{ timeout: "300ms", retries: 0 }"#))]);
    let (sp, bp, mp) = (support::free_port(), support::free_port(), support::free_port());
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", bp), "--service-url", &format!("mail=http://127.0.0.1:{}", mp)], &[]);
    assert_eq!(http(sp, "POST", "/slow-next", &[], "").2, "1");
    assert_eq!(buy(sp, "/", "k1"), (503, false, "Billing est occupé ☕".to_string()));
    assert_eq!(buy(sp, "/", "k1"), (503, true, "Billing est occupé ☕".to_string()));
    eventually(60, "billing did not finish", || (http(sp, "POST", "/count", &[], "").2 == "1").then_some(()));
    assert_eq!(buy(sp, "/", "k1"), (503, true, "Billing est occupé ☕".to_string()));
    assert_eq!(http(sp, "POST", "/count", &[], "").2, "1");
}

/// Q-1391, what must keep working: a refusal the service answered (a 401: billing has another
/// secret) is its final answer, and its early reply is kept (the batch's decision); so is one in
/// place of an `http.*` call to a third party that got no answer.
#[test]
fn q1391_an_early_reply_for_a_refusal_or_a_third_party_is_kept() {
    let d = dir("answered", &[("billing.qwe", Q1391_BILLING), ("mail.qwe", MAIL), ("main.qwe", &q1391_shop("{ retries: 0 }"))]);
    let (sp, bp, mp, bank) = (support::free_port(), support::free_port(), support::free_port(), support::free_port());
    let other = "0000000000000000000000000000000000000000000000000000000000000000";
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[("QUILLO_SERVICE_SECRET", other)]);
    let bank_url = format!("http://127.0.0.1:{}/pay", bank);
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", bp), "--service-url", &format!("mail=http://127.0.0.1:{}", mp)], &[("BANK", &bank_url)]);
    assert_eq!(buy(sp, "/", "k1"), (503, false, "Billing est occupé ☕".to_string()));
    assert_eq!(buy(sp, "/", "k1"), (503, true, "Billing est occupé ☕".to_string()));
    assert_eq!(buy(sp, "/bank", "k2"), (503, false, "the bank is busy".to_string()));
    assert_eq!(buy(sp, "/bank", "k2"), (503, true, "the bank is busy".to_string()));
    // The `http.post` is the run's last call, though a service call before it got no answer (mail is
    // not running): its reply is kept. Before, the service call's mark stayed, and the reply was not.
    assert_eq!(buy(sp, "/mail-then-bank", "k3"), (503, false, "the bank is busy".to_string()));
    assert_eq!(buy(sp, "/mail-then-bank", "k3"), (503, true, "the bank is busy".to_string()));
}

/// Q-1391, the security review's P3: a proxy's 502 after billing ran the call keeps the early reply,
/// so the same key never calls billing again, even once billing's record of the call is gone (swept
/// after eleven minutes, here at once, and billing started again). Let go, as in round 2, the retry
/// after the sweep billed a second time. Changed by the batch's last decision.
#[test]
fn q1391_p3_a_proxys_502_keeps_its_reply_past_the_services_record() {
    let d = dir("bad-gateway", &[("billing.qwe", Q1391_BILLING), ("mail.qwe", MAIL), ("main.qwe", &q1391_shop("{ retries: 0 }"))]);
    let (sp, bp, mp) = (support::free_port(), support::free_port(), support::free_port());
    let mut billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    let (at, calls) = proxy(bp, false, 1);
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", at), "--service-url", &format!("mail=http://127.0.0.1:{}", mp)], &[]);
    let busy = (503, false, "Billing est occupé ☕".to_string());
    assert_eq!(buy(sp, "/", "k1"), busy);
    assert_eq!(http(sp, "POST", "/count", &[], "").2, "1", "billing did not run the call");
    assert_ne!(http(bp, "POST", "/forget", &[], "").2, "0");
    billing.kill();
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    assert_eq!(buy(sp, "/", "k1"), (503, true, busy.2));
    assert_eq!(http(sp, "POST", "/count", &[], "").2, "1", "billing billed again");
    // Sent once to the proxy, not to another copy, and not again.
    assert_eq!(calls.lock().unwrap().iter().filter(|(p, _)| p.ends_with("/invoice")).count(), 1);
}

// ---- the reviews' probes (FEEDBACK89.md, Q-1391 and Q-1394, round 3) ---------------------------

const P1_BILLING: &str = r#"db.open("billing.db")
db.exec("create table if not exists charges (id integer primary key, cents int not null)")
server fn charge(cents: int) -> map with db = idempotent(() => db.one("insert into charges (cents) values (?) returning id, cents", cents))
server fn charged() -> int with db = db.one("select coalesce(sum(cents), 0) as n from charges").n
"#;

const P1_SHOP: &str = r#"service billing = "billing.qwe"
db.open("shop.db")
route POST /buy/:item with http, db = idempotent(() => {
    let cents = 100
    if item == "laptop" { cents = 150000 }
    let inv = billing.charge(cents) ?? reply 503 "busy"
    if item == "pen" { reply 409 "pens are out of stock" }
    `order ${item}: invoice #${inv.id}, paid ${inv.cents}`
})
route POST /order with http, db = idempotent(() => {
    let item = request.query.item
    let cents = 100
    if item == "laptop" { cents = 150000 }
    let inv = billing.charge(cents) ?? reply 503 "busy"
    if request.query.address == "bad" { reply 422 "no such address" }
    `order ${item}: invoice #${inv.id}, paid ${inv.cents}`
})
route GET /charged with http = str(billing.charged() ?? -1)
"#;

/// Q-1394, the security review's P1 (HIGH): a key first tried for a pen, refused by the shop after
/// billing charged it, then used for a laptop: another request, so other ids, and the laptop is
/// charged its own price. Before, it was answered the pen's charge: "invoice #1, paid 100". The
/// same with the query (`/order?...`).
#[test]
fn q1394_p1_another_request_with_the_key_gets_other_ids() {
    let d = dir("p1", &[("billing.qwe", P1_BILLING), ("main.qwe", P1_SHOP)]);
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    let r = send(&mut it, "POST", "/buy/pen", Some("K"));
    assert_eq!((r.status, text(&r)), (409, "pens are out of stock".to_string()));
    let r = send(&mut it, "POST", "/buy/laptop", Some("K"));
    assert_eq!((r.status, text(&r)), (200, "order laptop: invoice #2, paid 150000".to_string()));
    let r = send(&mut it, "POST", "/order?item=pen&address=bad", Some("Q"));
    assert_eq!(r.status, 422, "{}", text(&r));
    let r = send(&mut it, "POST", "/order?item=laptop&address=good", Some("Q"));
    assert_eq!((r.status, text(&r)), (200, "order laptop: invoice #4, paid 150000".to_string()));
    assert_eq!(text(&send(&mut it, "GET", "/charged", None)), "300200");
    // The same request again: its answer, kept.
    let r = send(&mut it, "POST", "/buy/laptop", Some("K"));
    assert_eq!((r.status, text(&r)), (200, "order laptop: invoice #2, paid 150000".to_string()));
}

/// Q-1394, P1b: two apps, both `main.qwe`, sharing one billing, the same key: the second app's
/// laptop is charged its own price. Before, it was answered the first app's pen.
#[test]
fn q1394_p1b_two_apps_of_one_name_sharing_a_service() {
    let d = dir("p1b", &[("billing.qwe", P1_BILLING)]);
    for app in ["a", "b"] {
        std::fs::create_dir_all(d.join(app)).unwrap();
        std::fs::write(d.join(app).join("main.qwe"), P1_SHOP).unwrap();
        std::fs::write(d.join(app).join("billing.qwe"), P1_BILLING).unwrap();
    }
    let (bp, ap, cp) = (support::free_port(), support::free_port(), support::free_port());
    let url = format!("billing=http://127.0.0.1:{}", bp);
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    let _a = Proc::start(&d.join("a"), "main.qwe", ap, &["--service-url", &url], &[]);
    let _b = Proc::start(&d.join("b"), "main.qwe", cp, &["--service-url", &url], &[]);
    assert_eq!(buy(ap, "/buy/pen", "K"), (409, false, "pens are out of stock".to_string()));
    assert_eq!(buy(cp, "/buy/laptop", "K"), (200, false, "order laptop: invoice #2, paid 150000".to_string()));
}

/// A third party that takes no key: it counts the requests it is sent, and answers each `{"ok":true}`.
fn bank() -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let at = l.local_addr().unwrap().port();
    let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let m = n.clone();
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let m = m.clone();
            std::thread::spawn(move || {
                let _ = c.set_read_timeout(Some(Duration::from_secs(10)));
                let mut buf = Vec::new();
                let mut b = [0u8; 4096];
                loop {
                    let Ok(k) = c.read(&mut b) else { return };
                    if k == 0 {
                        return;
                    }
                    buf.extend_from_slice(&b[..k]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                        let len: usize = head.lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                        if buf.len() >= i + 4 + len {
                            break;
                        }
                    }
                }
                m.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let body = r#"{"ok":true}"#;
                let _ = c.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", body.len(), body).as_bytes());
            });
        }
    });
    (at, n)
}

const PAY_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0 }
db.open("shop.db")
let bank = env("BANK", "http://127.0.0.1:9")
route POST /pay with http, db = idempotent({ transaction: false }, () => {
    let _paid = http.post(bank, { cents: 1200 }) ?? reply 502 "payment failed"
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "billing is busy"
    `Invoice #${inv.id}`
})
"#;

fn pay_shop(d: &Path, billing_at: u16, bank_at: u16) -> (u16, Proc) {
    let sp = support::free_port();
    let bank_url = format!("http://127.0.0.1:{}/charge", bank_at);
    let shop = Proc::start(d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", billing_at)], &[("BANK", &bank_url)]);
    (sp, shop)
}

/// Q-1391, the security review's P2: the bank paid, then a proxy's 502 for billing's call: the run
/// sent something outside, so its end is kept, and the same key does not pay the bank again. Before,
/// the 502 was no answer and the reply was let go: the bank paid twice.
#[test]
fn q1391_p2_a_run_that_paid_a_bank_keeps_its_end_after_a_502() {
    let d = dir("p2", &[("billing.qwe", Q1391_BILLING), ("main.qwe", PAY_SHOP)]);
    let bp = support::free_port();
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    let (at, _) = proxy(bp, false, 1);
    let (bank_at, paid) = bank();
    let (sp, _shop) = pay_shop(&d, at, bank_at);
    assert_eq!(buy(sp, "/pay", "k1"), (503, false, "billing is busy".to_string()));
    assert_eq!(buy(sp, "/pay", "k1"), (503, true, "billing is busy".to_string()));
    assert_eq!(paid.load(std::sync::atomic::Ordering::SeqCst), 1, "the bank was paid again");
}

/// Q-1391, the concurrency review's probe: the bank paid, then billing never reached (not running):
/// a call never sent, but the run sent something outside, so its end is kept. Before, the reply was
/// let go and each try paid the bank again.
#[test]
fn q1391_a_run_that_paid_a_third_party_keeps_its_end() {
    let d = dir("pay", &[("billing.qwe", Q1391_BILLING), ("main.qwe", PAY_SHOP)]);
    let (bank_at, paid) = bank();
    let (sp, _shop) = pay_shop(&d, support::free_port(), bank_at);
    assert_eq!(buy(sp, "/pay", "k1"), (503, false, "billing is busy".to_string()));
    assert_eq!(buy(sp, "/pay", "k1"), (503, true, "billing is busy".to_string()));
    assert_eq!(buy(sp, "/pay", "k1"), (503, true, "billing is busy".to_string()));
    assert_eq!(paid.load(std::sync::atomic::Ordering::SeqCst), 1, "the same key paid the bank again");
}

const QUESTION_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0 }
db.open("shop.db")
route POST /question with http, db = idempotent({ transaction: false }, () => {
    let inv = billing.invoice("Ada", 1200)?
    `Invoice #${inv.id}`
})
route POST /fails with http, db = idempotent({ transaction: false }, () => {
    let inv = billing.invoice("Ada", 1200)
    let n = inv.id + 1
    `Invoice #${n}`
})
"#;

/// Q-1391, the concurrency review's probe: a run that passes a never-sent call's error on with `?`,
/// or fails at run time after it, is not kept either: once billing is up, the same key gets the
/// invoice. Before, only an early `reply` was let go, and these failures were kept.
#[test]
fn q1391_a_question_mark_after_a_call_never_sent_is_not_kept() {
    let d = dir("question", &[("billing.qwe", Q1391_BILLING), ("main.qwe", QUESTION_SHOP)]);
    let (sp, bp) = (support::free_port(), support::free_port());
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", bp)], &[]);
    for path in ["/question", "/fails"] {
        let (status, replayed, _) = buy(sp, path, path);
        assert!(status >= 500 && !replayed, "{}: {}", path, status);
        let (status, replayed, _) = buy(sp, path, path);
        assert!(status >= 500 && !replayed, "{} kept: {}", path, status);
    }
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    assert_eq!(buy(sp, "/question", "/question"), (200, false, "Invoice #1".to_string()));
    assert_eq!(buy(sp, "/fails", "/fails"), (200, false, "Invoice #3".to_string()));
}

const ARGS_LEDGER: &str = r#"db.open("ledger.db")
db.exec("create table if not exists postings (id integer primary key, ref text not null, cents int not null)")
server fn post(ref: str, cents: int) -> map with db = idempotent(() => db.one("insert into postings (ref, cents) values (?, ?) returning id, ref, cents", ref, cents))
server fn refs() -> str with db = join(db.query("select ref from postings order by id").map((r) => r.ref), ",")
"#;

const ARGS_SHOP: &str = r#"service ledger = "ledger.qwe"
db.open("shop.db")
let n = 0
route POST /post with http, db = idempotent(() => {
    n += 1
    let p = ledger.post(`ref-${n}`, n * 100) ?? reply 503 "busy"
    if n == 1 { reply 503 "once more" }
    `asked ref-${n}, got ${p.ref} for ${p.cents}`
})
route GET /all with http = ledger.refs() ?? "none"
"#;

/// Q-1394, the concurrency review's probe: the same place in a run of the same request, with other
/// arguments (the run is not the same as the first time): the ledger refuses the call (422, as a key
/// used with another request is), and does not answer it from its record. Before, it answered the
/// first call's posting: "asked ref-2, got ref-1 for 100".
#[test]
fn q1394_the_same_place_with_other_arguments_is_refused() {
    let d = dir("args", &[("ledger.qwe", ARGS_LEDGER), ("main.qwe", ARGS_SHOP)]);
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    let r = send(&mut it, "POST", "/post", Some("k1"));
    assert_eq!((r.status, text(&r)), (503, "once more".to_string()));
    let r = send(&mut it, "POST", "/post", Some("k1"));
    assert_eq!((r.status, text(&r)), (503, "busy".to_string()));
    assert_eq!(text(&send(&mut it, "GET", "/all", None)), "ref-1");
}

const P4_BILLING: &str = r#"accept_calls { keys: { shop: env("SHOP_KEY"), intruder: env("INTRUDER_KEY") } }
service ledger = "ledger.qwe" { retries: 0 }
server fn charge(cents: int) -> map ! str with http = ledger.post("acct-1", cents)
"#;

const P4_LEDGER: &str = r#"db.open("ledger.db")
db.exec("create table if not exists postings (id integer primary key, ref text not null, cents int not null)")
server fn post(ref: str, cents: int) -> map with db = db.one("insert into postings (ref, cents) values (?, ?) returning id, ref, cents", ref, cents)
route GET /n with db = str(db.one("select count(*) as n from postings").n)
"#;

/// A call of billing's `f` signed with caller `kid`'s own key at time `t`, with a root when given.
#[allow(clippy::too_many_arguments)]
fn signed_as(port: u16, t: u64, f: &str, call: &str, kid: &str, key: &str, body: &str, root: Option<&str>) -> (u16, String) {
    let sig = quillo::svcauth::sign3(key, t, "billing", f, "", call, kid, kid, "5000", "\n", body.as_bytes());
    let service = format!("v=3,kid={},t={},sig={}", kid, t, sig);
    let mut headers = vec![("x-quillo-service", service.clone()), ("x-quillo-to", "billing".into()), ("x-quillo-call", call.into()), ("x-quillo-caller", kid.into()), ("x-quillo-deadline", "5000".into()), ("content-type", "application/json".into())];
    if let Some(r) = root {
        let msg = format!("quillo-root-1\nbilling\n{}\n\n{}\n{}", f, call, r);
        headers.push(("x-quillo-root", format!("{};sig={}", r, quillo::crypto::hex(&quillo::crypto::hmac_sha256(key.as_bytes(), msg.as_bytes())))));
    }
    let h: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (status, _, body) = http(port, "POST", &format!("/_quillo/service/{}", f), &h, body);
    (status, body)
}

fn posting_id(body: &str) -> i64 {
    serde_json::from_str::<serde_json::Value>(body).ok().and_then(|j| j["ok"]["id"].as_i64()).unwrap_or(-1)
}

/// Q-1394, the security review's P4: another caller with the shop's call id and a root is its own
/// call, its own posting, never the shop's answer; the shop's id with other arguments is refused
/// (422) and never answered from the record; the shop's call sent again is answered as it was.
/// Before, the intruder's call went to the ledger under the shop's ids and got the shop's posting,
/// and other arguments ran.
#[test]
fn q1394_p4_a_chained_call_is_known_by_its_caller_and_arguments() {
    let d = dir("p4", &[("billing.qwe", P4_BILLING), ("ledger.qwe", P4_LEDGER)]);
    let (lp, bp) = (support::free_port(), support::free_port());
    let sk = "shop-key-0123456789abcdef0123456789abcdef0123456789";
    let ik = "intruder-key-0123456789abcdef0123456789abcdef01234567";
    let _ledger = Proc::start(&d, "ledger.qwe", lp, &["--as-service"], &[]);
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service", "--service-url", &format!("ledger=http://127.0.0.1:{}", lp)], &[("SHOP_KEY", sk), ("INTRUDER_KEY", ik)]);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let root = "0123456789abcdef0123456789abcdef";
    let (status, first) = signed_as(bp, t, "charge", "0f1e2d3c4b5a6978", "shop", sk, "[1200]", Some(root));
    assert_eq!((status, posting_id(&first)), (200, 1), "{}", first);
    assert_eq!(signed_as(bp, t, "charge", "0f1e2d3c4b5a6978", "shop", sk, "[1200]", Some(root)), (200, first.clone()));
    let (status, other) = signed_as(bp, t, "charge", "0f1e2d3c4b5a6978", "shop", sk, "[1300]", Some(root));
    assert_eq!(status, 422, "{}", other);
    assert!(other.contains("other arguments"), "{}", other);
    let (status, theirs) = signed_as(bp, t, "charge", "0f1e2d3c4b5a6978", "intruder", ik, "[1200]", Some(root));
    assert_eq!((status, posting_id(&theirs)), (200, 2), "{}", theirs);
    assert_eq!(http(lp, "GET", "/n", &[], "").2, "2");
}

const P5_BILLING: &str = r#"db.open("billing.db")
db.exec("create table if not exists tries (n int)")
server fn declined() -> str ! str with db {
    db.exec("insert into tries (n) values (1)")
    return err("declined")
}
route GET /tries with db = str(db.one("select count(*) as n from tries").n)
"#;

/// Q-1394, the security review's P5: the same signed chained request sent again within its window,
/// its error answer not kept, is a replay: refused, not run again. Without a root (0.8.11's call),
/// as before: the error answer kept and given again. Before, the chained request ran each time.
#[test]
fn q1394_p5_a_chained_request_sent_again_as_it_was_is_refused() {
    let d = dir("p5", &[("billing.qwe", P5_BILLING)]);
    let bp = support::free_port();
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let plain: Vec<(u16, String)> = (0..3).map(|_| signed_call(bp, t, "declined", "aaaa0000bbbb1111", None)).collect();
    assert!(plain.iter().all(|(s, b)| *s == 200 && b.contains("declined")), "{:?}", plain);
    assert_eq!(http(bp, "GET", "/tries", &[], "").2, "1");
    let root = "0123456789abcdef0123456789abcdef";
    let w = || {
        let msg = format!("quillo-root-1\nbilling\ndeclined\n\ncccc2222dddd3333\n{}", root);
        format!("{};sig={}", root, quillo::crypto::hex(&quillo::crypto::hmac_sha256(SECRET.as_bytes(), msg.as_bytes())))
    };
    let chained: Vec<u16> = (0..3).map(|_| signed_call(bp, t, "declined", "cccc2222dddd3333", Some(w())).0).collect();
    assert_eq!(chained, vec![200, 409, 409]);
    assert_eq!(http(bp, "GET", "/tries", &[], "").2, "2");
}

/// What a run sends outside besides `http.*`: a packet, a push, a call of a site.
const OUTSIDE_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0 }
db.open("shop.db")
site partner = env("PARTNER", "http://127.0.0.1:9")
route POST /udp with http, db, net = idempotent({ transaction: false }, () => {
    let _sent = udp.send("127.0.0.1", 9, "billed") ?? null
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "busy"
    `Invoice #${inv.id}`
})
route POST /push with http, db = idempotent({ transaction: false }, () => {
    let _sent = notification.send("ada", { title: "Billed" }) ?? null
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "busy"
    `Invoice #${inv.id}`
})
route POST /site with http, db = idempotent({ transaction: false }, () => {
    let _notes = partner.notes(1) ?? null
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "busy"
    `Invoice #${inv.id}`
})
route POST /channel with http, db, net = idempotent({ transaction: false }, () => {
    let _took = channel.broadcast("orders", "billed")
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "busy"
    `Invoice #${inv.id}`
})
route POST /file with http, db, fs = idempotent({ transaction: false }, () => {
    write_file("billed.txt", "Ada")
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "busy"
    `Invoice #${inv.id}`
})
route POST /read with http, db, fs, time = idempotent({ transaction: false }, () => {
    let _t = now()
    let _f = read_file("main.qwe") ?? ""
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "busy"
    `Invoice #${inv.id}`
})
"#;

/// The run's outside call at `path`, then billing never reached, then an early reply: kept, as
/// after `http.*` (FEEDBACK89.md, Q-1391; branch feedback86-89).
fn outside_then_unsent_is_kept(name: &str, path: &str) {
    let d = dir(name, &[("billing.qwe", Q1391_BILLING), ("main.qwe", OUTSIDE_SHOP)]);
    let sp = support::free_port();
    let partner = format!("http://127.0.0.1:{}", support::free_port());
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", support::free_port())], &[("PARTNER", &partner)]);
    assert_eq!(buy(sp, path, "k1"), (503, false, "busy".to_string()));
    assert_eq!(buy(sp, path, "k1"), (503, true, "busy".to_string()), "{}: the end was not kept", path);
}

/// Q-1391: a `udp.send`, then a service call never sent, then a failure: the end is kept. Before,
/// a packet did not mark the run, and the reply was let go.
#[test]
fn q1391_a_run_that_sent_a_packet_keeps_its_end() {
    outside_then_unsent_is_kept("outside-udp", "/udp");
}

/// Q-1391: a `notification.send`, then a service call never sent, then a failure: the end is kept.
/// Before, a push did not mark the run.
#[test]
fn q1391_a_run_that_sent_a_push_keeps_its_end() {
    outside_then_unsent_is_kept("outside-push", "/push");
}

/// Q-1391: a call of a site (`site partner = ...`), then a service call never sent, then a failure:
/// the end is kept. Before, a site's call did not mark the run.
#[test]
fn q1391_a_run_that_called_a_site_keeps_its_end() {
    outside_then_unsent_is_kept("outside-site", "/site");
}

/// Q-1391, the concurrency review: a `channel.broadcast` to the pages, then a service call never
/// sent, then a failure: the end is kept. Before, a channel's send did not mark the run.
#[test]
fn q1391_a_run_that_sent_to_a_channel_keeps_its_end() {
    outside_then_unsent_is_kept("outside-channel", "/channel");
}

/// Q-1391: a file written, then a service call never sent, then a failure: the end is kept, and only
/// one file write is outside the database. Before, a write did not mark the run.
#[test]
fn q1391_a_run_that_wrote_a_file_keeps_its_end() {
    outside_then_unsent_is_kept("outside-file", "/file");
}

/// Q-1391: what reads only (the time, a file read) does not mark the run: the reply in place of the
/// call never sent is still let go (the boundary marks what goes out, not every effect).
#[test]
fn q1391_a_run_that_only_read_lets_its_end_go() {
    let d = dir("outside-read", &[("billing.qwe", Q1391_BILLING), ("main.qwe", OUTSIDE_SHOP)]);
    let sp = support::free_port();
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", support::free_port())], &[]);
    assert_eq!(buy(sp, "/read", "k1"), (503, false, "busy".to_string()));
    assert_eq!(buy(sp, "/read", "k1"), (503, false, "busy".to_string()), "the end was kept");
}

const SHIP: &str = "server fn go() -> str = \"shipped\"\n";

/// A shop whose run calls billing in a `parallel` task, or later, then shipping, which is down.
const SENT_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0 }
service ship = "ship.qwe" { retries: 0 }
db.open("shop.db")
route POST /parallel with http, db = idempotent({ transaction: false }, () => {
    let got = parallel(() => billing.invoice("Ada", 1200), () => billing.invoice("Bob", 100))
    let s = ship.go() ?? reply 503 "shipping is down"
    `Invoice ${got[0].id} ${s}`
})
route POST /later with http, db = idempotent({ transaction: false }, () => {
    let id = billing.later.invoice("Ada", 1200)
    let s = ship.go() ?? reply 503 "shipping is down"
    `queued ${id} ${s}`
})
route POST /plain with http, db = idempotent({ transaction: false }, () => {
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "billing is down"
    let s = ship.go() ?? reply 503 "shipping is down"
    `Invoice ${inv.id} ${s}`
})
route POST /count with http = str(billing.total() ?? -1)
"#;

/// Billing running apart, the shop sending to it and to a shipping service that is not running.
fn sent_shop(name: &str) -> (PathBuf, u16, u16, Proc, Proc) {
    let d = dir(name, &[("billing.qwe", Q1391_BILLING), ("ship.qwe", SHIP), ("main.qwe", SENT_SHOP)]);
    let (sp, bp) = (support::free_port(), support::free_port());
    let billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    let shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", bp), "--service-url", &format!("ship=http://127.0.0.1:{}", support::free_port())], &[]);
    (d, sp, bp, billing, shop)
}

/// Q-1391, the security review's n1 and the concurrency review's probe 9: billing called in a
/// `parallel` task (not chained: a new id each run), then shipping never reached: the calls were
/// sent, so the end is kept, and the same key makes no more invoices. Before, the reply was let go
/// and each try invoiced twice again.
#[test]
fn q1391_n1_service_calls_in_a_parallel_task_keep_the_end() {
    let (_d, sp, _bp, _billing, _shop) = sent_shop("n1");
    assert_eq!(buy(sp, "/parallel", "k1"), (503, false, "shipping is down".to_string()));
    assert_eq!(buy(sp, "/parallel", "k1"), (503, true, "shipping is down".to_string()));
    assert_eq!(buy(sp, "/parallel", "k1"), (503, true, "shipping is down".to_string()));
    assert_eq!(http(sp, "POST", "/count", &[], "").2, "2", "the same key invoiced again");
}

/// Q-1391, n2 and probe 10: a call made later (queued), then shipping never reached: the queued call
/// will be sent, so the end is kept, and one invoice is made. Before, each try queued another.
#[test]
fn q1391_n2_a_call_made_later_keeps_the_end() {
    let (_d, sp, _bp, _billing, _shop) = sent_shop("n2");
    assert_eq!(buy(sp, "/later", "k1"), (503, false, "shipping is down".to_string()));
    assert_eq!(buy(sp, "/later", "k1"), (503, true, "shipping is down".to_string()));
    assert_eq!(buy(sp, "/later", "k1"), (503, true, "shipping is down".to_string()));
    eventually(30, "the queued invoice", || (http(sp, "POST", "/count", &[], "").2 == "1").then_some(()));
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(http(sp, "POST", "/count", &[], "").2, "1", "the same key queued another invoice");
}

/// Q-1391, n4 and M3: billing (no `idempotent`) invoiced, then shipping never reached. The end is
/// kept, so the same key, once billing has forgotten the call (its records past their 11 minutes,
/// done here by deleting them and starting it again), invoices nothing more. Before, the reply was
/// let go, the retry sent billing's call again with the same id, and the forgotten id invoiced again.
#[test]
fn q1391_m3_a_sent_call_keeps_the_end_past_the_callees_record() {
    let (d, sp, bp, mut billing, _shop) = sent_shop("m3");
    assert_eq!(buy(sp, "/plain", "k1"), (503, false, "shipping is down".to_string()));
    assert_eq!(http(bp, "POST", "/forget", &[], "").0, 200);
    billing.kill();
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    assert_eq!(buy(sp, "/plain", "k1"), (503, true, "shipping is down".to_string()));
    assert_eq!(http(sp, "POST", "/count", &[], "").2, "1", "the same key invoiced again");
}

/// A service in this process that charges the bank with its own `http.post`.
const CHARGING_BILLING: &str = r#"let bank = env("BANK", "http://127.0.0.1:9")
server fn charge(cents: int) -> map ! str with http, env {
    let _r = http.post(bank, { cents })?
    return { ok: true }
}
"#;

const CHARGING_SHOP: &str = r#"service billing = "billing.qwe"
service ship = "ship.qwe" { retries: 0 }
db.open("shop.db")
route POST /buy with http, db = idempotent({ transaction: false }, () => {
    let _c = billing.charge(1200) ?? reply 502 "payment failed"
    let s = ship.go() ?? reply 503 "shipping is down"
    `charged ${s}`
})
"#;

/// Q-1391, n3 and probe 7: a service running in this process charges the bank, then shipping is
/// never reached: the call ran, so the end is kept, and the bank is charged once. Before, what the
/// service sent was its own, the shop's reply was let go, and each try charged again.
#[test]
fn q1391_n3_a_service_in_this_process_that_charged_keeps_the_end() {
    let d = dir("n3", &[("billing.qwe", CHARGING_BILLING), ("ship.qwe", SHIP), ("main.qwe", CHARGING_SHOP)]);
    let (bank_at, paid) = bank();
    let sp = support::free_port();
    let bank_url = format!("http://127.0.0.1:{}/charge", bank_at);
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("ship=http://127.0.0.1:{}", support::free_port())], &[("BANK", &bank_url)]);
    assert_eq!(buy(sp, "/buy", "k1"), (503, false, "shipping is down".to_string()));
    assert_eq!(buy(sp, "/buy", "k1"), (503, true, "shipping is down".to_string()));
    assert_eq!(buy(sp, "/buy", "k1"), (503, true, "shipping is down".to_string()));
    assert_eq!(paid.load(std::sync::atomic::Ordering::SeqCst), 1, "the same key charged the bank again");
}

const PAY_QWER: &str = r#"//! effects: http
use std::io::{Read, Write};

/// Charges at the bank.
pub fn pay_charge(addr: &str) -> Result<String, String> {
    let mut s = std::net::TcpStream::connect(addr).map_err(|e| e.to_string())?;
    let body = "{}";
    write!(s, "POST /charge HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).map_err(|e| e.to_string())?;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    Ok(out)
}
"#;

const NATIVE_SHOP: &str = r#"use "pay.qwer"
service billing = "billing.qwe" { retries: 0 }
db.open("shop.db")
let bank = env("BANKADDR", "127.0.0.1:9")
route POST /buy with http, db = idempotent({ transaction: false }, () => {
    let _paid = pay_charge(bank) ?? reply 502 "payment failed"
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "billing is busy"
    `Invoice #${inv.id}`
})
"#;

/// Q-1391, probe 6: the charge made by a plugin's Rust (`.qwer`, as the catalog's stripe, paypal,
/// smtp and sms are), then billing never reached: the plugin's function declares `http`, so the end
/// is kept and the bank charged once. Before, each try charged again.
#[test]
fn q1391_probe6_a_plugins_charge_keeps_the_end() {
    let d = dir("native", &[("billing.qwe", Q1391_BILLING), ("main.qwe", NATIVE_SHOP), ("pay.qwer", PAY_QWER)]);
    let (bank_at, paid) = bank();
    let cache = d.join("native-cache");
    std::fs::create_dir_all(&cache).unwrap();
    let sp = support::free_port();
    let addr = format!("127.0.0.1:{}", bank_at);
    let cache = cache.display().to_string();
    let _shop = Proc::start_slow(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", support::free_port())], &[("BANKADDR", &addr), ("QUILLO_NATIVE_CACHE", &cache)]);
    assert_eq!(buy(sp, "/buy", "k1"), (503, false, "billing is busy".to_string()));
    assert_eq!(buy(sp, "/buy", "k1"), (503, true, "billing is busy".to_string()));
    assert_eq!(buy(sp, "/buy", "k1"), (503, true, "billing is busy".to_string()));
    assert_eq!(paid.load(std::sync::atomic::Ordering::SeqCst), 1, "the same key charged the bank again");
}

/// A JSON Web Token, HS256, signed with `key`.
fn jwt(key: &str, claims: &str) -> String {
    fn b64(bytes: &[u8]) -> String {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for c in bytes.chunks(3) {
            let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
            for i in 0..=c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
        }
        out
    }
    let head = format!("{}.{}", b64(br#"{"alg":"HS256","typ":"JWT"}"#), b64(claims.as_bytes()));
    let sig = quillo::crypto::hmac_sha256(key.as_bytes(), head.as_bytes());
    format!("{}.{}", head, b64(&sig))
}

const TOKEN_BILLING: &str = r#"accept_calls { jwt: { secret: env("JWT_KEY"), audience: "billing" } }
db.open("billing.db")
db.exec("create table if not exists charges (id integer primary key, cents int not null)")
server fn charge(cents: int) -> map with db = idempotent(() => db.one("insert into charges (cents) values (?) returning id, cents", cents))
server fn charged() -> int with db = db.one("select coalesce(sum(cents), 0) as n from charges").n
"#;

const TOKEN_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0, auth: { bearer: env("TOKEN") } }
db.open("shop.db")
db.exec("create table if not exists cart (k int primary key, item text not null)")
route POST /cart/:item with db {
    db.exec("insert or replace into cart (k, item) values (1, ?)", item)
    "cart: " + item
}
route POST /checkout with http, db = idempotent(() => {
    let item = db.one("select item from cart where k = 1").item
    let cents = 100
    if item == "laptop" { cents = 150000 }
    let inv = billing.charge(cents) ?? reply 503 "busy"
    if item == "pen" { reply 409 "pens are out of stock" }
    `order ${item}: invoice #${inv.id}, paid ${inv.cents}`
})
route GET /charged with http = str(billing.charged() ?? -1)
"#;

/// Q-1394, the security review's n5b: billing reached with a bearer token, so no root goes with the
/// call (no key signs it). The cart, in the shop's database, changes between two checkouts with one
/// key: the laptop is charged as a laptop, under a new id, not answered the pen's charge. Before,
/// the id was derived all the same and billing's `idempotent` answered "paid 100" for the laptop.
#[test]
fn q1394_n5b_a_call_no_key_signs_gets_a_new_id() {
    let d = dir("n5b", &[("billing.qwe", TOKEN_BILLING), ("main.qwe", TOKEN_SHOP)]);
    let key = "jwt-key-0123456789abcdef0123456789abcdef";
    let token = jwt(key, r#"{"sub":"shop","aud":"billing","exp":4102444800}"#);
    let (sp, bp) = (support::free_port(), support::free_port());
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[("JWT_KEY", key)]);
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", bp)], &[("TOKEN", &token)]);
    assert_eq!(http(sp, "POST", "/cart/pen", &[], "").0, 200);
    assert_eq!(buy(sp, "/checkout", "K"), (409, false, "pens are out of stock".to_string()));
    assert_eq!(http(sp, "GET", "/charged", &[], "").2, "100");
    assert_eq!(http(sp, "POST", "/cart/laptop", &[], "").0, 200);
    let (status, _, body) = buy(sp, "/checkout", "K");
    assert_eq!((status, body.as_str()), (200, "order laptop: invoice #2, paid 150000"));
    assert_eq!(http(sp, "GET", "/charged", &[], "").2, "150100");
}

const DECLINING: &str = r#"db.open("billing.db")
db.exec("create table if not exists tries (n int)")
server fn declined() -> str ! str with db {
    db.exec("insert into tries (n) values (1)")
    return err("declined")
}
route GET /tries with db = str(db.one("select count(*) as n from tries").n)
"#;

const RETRYING_SHOP: &str = r#"service billing = "billing.qwe" { retries: 1 }
db.open("shop.db")
route POST /pay with http, db = idempotent({ transaction: false }, () => {
    let r = billing.declined() ?? reply 402 "declined"
    r
})
"#;

/// A relay to `port` that passes the first request on and closes its connection without the
/// answer (lost on its way back), and passes the others' answers.
fn cutter(port: u16) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let at = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let cut = Arc::new(std::sync::atomic::AtomicBool::new(false));
        for mut c in l.incoming().flatten() {
            let cut = cut.clone();
            std::thread::spawn(move || {
                let _ = c.set_read_timeout(Some(Duration::from_secs(60)));
                let mut buf = Vec::new();
                let mut b = [0u8; 4096];
                loop {
                    let Ok(k) = c.read(&mut b) else { return };
                    if k == 0 {
                        return;
                    }
                    buf.extend_from_slice(&b[..k]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                        let len: usize = head.lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                        if buf.len() >= i + 4 + len {
                            break;
                        }
                    }
                }
                // Closed after one request, so that its answer ends.
                let i = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(0);
                let head: String = String::from_utf8_lossy(&buf[..i]).lines().filter(|l| !l.to_ascii_lowercase().starts_with("connection:")).map(|l| format!("{}\r\n", l)).collect();
                let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else { return };
                let _ = s.set_read_timeout(Some(Duration::from_secs(60)));
                let _ = s.write_all(format!("{}Connection: close\r\n\r\n", head).as_bytes());
                let _ = s.write_all(&buf[i + 4..]);
                let mut answer = Vec::new();
                let _ = s.read_to_end(&mut answer);
                // The first call's answer is lost; the question before it (`_contracts`) is not a call.
                let probe = head.split_whitespace().nth(1).is_some_and(|p| p.ends_with("/_contracts"));
                if probe || cut.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    let _ = c.write_all(&answer);
                }
            });
        }
    });
    at
}

/// The review's L3: billing's error answer is lost on its way back (the connection cut), and the shop's own retry, within
/// the same second, signs other bytes: billing runs it again, not refusing it as a replay of the
/// first. Before, the retry signed the same bytes and billing answered 409, "sent before".
#[test]
fn q1394_l3_a_callers_retry_in_the_same_second_is_no_replay() {
    let d = dir("l3", &[("billing.qwe", DECLINING), ("main.qwe", RETRYING_SHOP)]);
    let bp = support::free_port();
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    let at = cutter(bp);
    let sp = support::free_port();
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", at)], &[]);
    // Early in a second, so that the retry (100 to 150 ms later) is in it too.
    while std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_millis() > 300 {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(buy(sp, "/pay", "k1"), (402, false, "declined".to_string()));
    assert_eq!(http(bp, "GET", "/tries", &[], "").2, "2", "the retry was refused as a replay");
}

/// A billing as a Quillo before 0.8.12 runs it, at its worst: it reads no root, and its
/// `idempotent` knows a call by its id alone, its arguments not in the fingerprint, so the same id
/// again is answered as it was. Its answers are signed with the shared secret, and say
/// `"chain": 1` (as 0.8.12 does) only while `mark` is set. It records each call's id and whether a
/// root came with it.
struct FakeBilling {
    port: u16,
    mark: Arc<std::sync::atomic::AtomicBool>,
    calls: Arc<Mutex<Vec<(String, bool)>>>,
    charged: Arc<Mutex<i64>>,
}

fn fake_billing(mark: bool) -> FakeBilling {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let fb = FakeBilling { port, mark: Arc::new(std::sync::atomic::AtomicBool::new(mark)), calls: Arc::new(Mutex::new(Vec::new())), charged: Arc::new(Mutex::new(0)) };
    let (mark, calls, charged) = (fb.mark.clone(), fb.calls.clone(), fb.charged.clone());
    let answers: Arc<Mutex<std::collections::HashMap<String, String>>> = Arc::default();
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let (mark, calls, charged, answers) = (mark.clone(), calls.clone(), charged.clone(), answers.clone());
            std::thread::spawn(move || {
                let _ = c.set_read_timeout(Some(Duration::from_secs(10)));
                let mut buf = Vec::new();
                let mut b = [0u8; 4096];
                let (head, body) = loop {
                    let Ok(k) = c.read(&mut b) else { return };
                    if k == 0 {
                        return;
                    }
                    buf.extend_from_slice(&b[..k]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_string();
                        let len: usize = head.to_ascii_lowercase().lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                        if buf.len() >= i + 4 + len {
                            break (head, String::from_utf8_lossy(&buf[i + 4..i + 4 + len]).to_string());
                        }
                    }
                };
                let header = |n: &str| head.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(n)).map(|(_, v)| v.trim().to_string()));
                let call = header("x-quillo-call").unwrap_or_default();
                let chain = if mark.load(std::sync::atomic::Ordering::SeqCst) { r#","chain":1"# } else { "" };
                let probe = head.starts_with("POST /_quillo/service/_contracts");
                calls.lock().unwrap().push((if probe { "probe".to_string() } else { call.clone() }, header("x-quillo-root").is_some()));
                let answer = if probe {
                    format!(r#"{{"ok":[]{}}}"#, chain)
                } else if head.starts_with("POST /_quillo/service/charged") {
                    format!(r#"{{"ok":{}{}}}"#, charged.lock().unwrap(), chain)
                } else {
                    let mut a = answers.lock().unwrap();
                    a.entry(call.clone())
                        .or_insert_with(|| {
                            let cents: i64 = body.trim_matches(|c| c == '[' || c == ']').parse().unwrap_or(0);
                            let mut t = charged.lock().unwrap();
                            *t += cents;
                            format!(r#"{{"ok":{{"id":{},"cents":{}}}{}}}"#, *t, cents, chain)
                        })
                        .clone()
                };
                let sig = quillo::svcauth::sign_answer(SECRET, &call, 200, answer.as_bytes());
                let _ = c.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nx-quillo-answer: sig={}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", sig, answer.len(), answer).as_bytes());
            });
        }
    });
    fb
}

const SECRET_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0 }
db.open("shop.db")
db.exec("create table if not exists cart (k int primary key, item text not null)")
route POST /cart/:item with db {
    db.exec("insert or replace into cart (k, item) values (1, ?)", item)
    "cart: " + item
}
route POST /checkout with http, db = idempotent(() => {
    let item = db.one("select item from cart where k = 1").item
    let cents = 100
    if item == "laptop" { cents = 150000 }
    let inv = billing.charge(cents) ?? reply 503 "busy"
    if item == "pen" { reply 409 "pens are out of stock" }
    `order ${item}: paid ${inv.cents}`
})
route GET /charged with http = str(billing.charged() ?? -1)
"#;

/// Billing's file, as the shop reads it for its functions (the fake answers in its place).
const BILLING_HEADERS: &str = r#"db.open("billing.db")
db.exec("create table if not exists charges (id integer primary key, cents int not null)")
server fn charge(cents: int) -> map with db = idempotent(() => db.one("insert into charges (cents) values (?) returning id, cents", cents))
server fn charged() -> int with db = db.one("select coalesce(sum(cents), 0) as n from charges").n
"#;

fn secret_shop(name: &str, billing_at: u16) -> (PathBuf, u16, Proc) {
    let d = dir(name, &[("billing.qwe", BILLING_HEADERS), ("main.qwe", SECRET_SHOP)]);
    let sp = support::free_port();
    let shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", billing_at)], &[]);
    (d, sp, shop)
}

/// Q-1394, the coordinator's follow-up to n5b: a billing of a Quillo before 0.8.12, reached with
/// the shared secret, never says it keeps chained records: every call gets a new id and no root,
/// as in 0.8.11, and the laptop is charged as a laptop. The shop asks it once, before its first
/// chained call, and not again. Before, the shop sent it derived ids, and its `idempotent` answered
/// the pen's charge for the laptop.
#[test]
fn q1394_an_older_callee_gets_new_ids_and_no_price_swap() {
    let fake = fake_billing(false);
    let (_d, sp, _shop) = secret_shop("old-callee", fake.port);
    assert_eq!(http(sp, "POST", "/cart/pen", &[], "").0, 200);
    assert_eq!(buy(sp, "/checkout", "K"), (409, false, "pens are out of stock".to_string()));
    assert_eq!(http(sp, "POST", "/cart/laptop", &[], "").0, 200);
    assert_eq!(buy(sp, "/checkout", "K"), (200, false, "order laptop: paid 150000".to_string()));
    assert_eq!(*fake.charged.lock().unwrap(), 150100);
    let calls = fake.calls.lock().unwrap().clone();
    assert!(calls.iter().all(|(_, root)| !root), "a root went to the older callee: {:?}", calls);
    assert_eq!(calls.iter().filter(|(c, _)| c == "probe").count(), 1, "{:?}", calls);
    assert_eq!(calls[0].0, "probe");
}

/// Q-1394: a billing that says it keeps chained records (`"chain": 1` in its signed answers) gets
/// derived ids and the root from the shop's first chained call on: the shop asks it once first (a
/// signed `_contracts`). Put back to a Quillo that does not say it, its next answer takes the shop
/// back to new ids and no root, without asking again.
#[test]
fn q1394_derived_ids_only_to_a_callee_that_says_it_keeps_chains() {
    let fake = fake_billing(true);
    let (_d, sp, _shop) = secret_shop("marked-callee", fake.port);
    assert_eq!(http(sp, "POST", "/cart/pen", &[], "").0, 200);
    assert_eq!(buy(sp, "/checkout", "K1"), (409, false, "pens are out of stock".to_string()));
    assert_eq!(buy(sp, "/checkout", "K1"), (409, false, "pens are out of stock".to_string()));
    let calls = fake.calls.lock().unwrap().clone();
    assert_eq!(calls[0], ("probe".to_string(), false), "{:?}", calls);
    assert!(calls[1].1 && calls[2].1, "no root after the mark: {:?}", calls);
    assert_eq!(calls[1].0, calls[2].0, "the same key sent other ids: {:?}", calls);
    // Put back to an older Quillo: its next answer (without the mark) is the last that gets a root.
    fake.mark.store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(buy(sp, "/checkout", "K3"), (409, false, "pens are out of stock".to_string()));
    assert_eq!(buy(sp, "/checkout", "K4"), (409, false, "pens are out of stock".to_string()));
    let calls = fake.calls.lock().unwrap().clone();
    assert!(calls[3].1 && !calls[4].1, "a root after the mark was gone: {:?}", calls);
    assert_eq!(calls.iter().filter(|(c, _)| c == "probe").count(), 1, "asked again: {:?}", calls);
}

const Q1_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0 }
db.open("shop.db")
route POST /buy/:item with http, db = idempotent(() => {
    let cents = 100
    if item == "laptop" { cents = 150000 }
    let inv = billing.charge(cents) ?? reply 503 "busy"
    `order ${item}: paid ${inv.cents}`
})
route GET /charged with http = str(billing.charged() ?? -1)
"#;

/// Two programs of one name, each with no secret of its own and both holding the services' secret,
/// buying with one key at one billing: `secrets` the programs' own (empty: none).
fn two_shops(name: &str, secrets: [&str; 2]) -> String {
    let d = dir(name, &[("billing.qwe", BILLING_HEADERS)]);
    let bp = support::free_port();
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    let mut shops = Vec::new();
    for (i, secret) in secrets.iter().enumerate() {
        let sd = d.join(format!("shop{}", i));
        std::fs::create_dir_all(&sd).unwrap();
        std::fs::write(sd.join("main.qwe"), Q1_SHOP).unwrap();
        std::fs::copy(d.join("billing.qwe"), sd.join("billing.qwe")).unwrap();
        let sp = support::free_port();
        shops.push(Proc::start(&sd, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", bp)], &[("QUILLO_SECRET", secret)]));
    }
    for s in &shops {
        assert_eq!(buy(s.port, "/buy/laptop", "K"), (200, false, "order laptop: paid 150000".to_string()));
    }
    http(shops[0].port, "GET", "/charged", &[], "").2
}

/// Q-1394, the security review's q1: two programs with no `QUILLO_SECRET` of their own, sharing the
/// services' secret, each buy a laptop with the key K: two charges. Ids are derived only with the
/// program's own secret; with their own (different) secrets, two charges too. Before, both derived
/// the same ids from the services' secret: one charge, and both delivered.
#[test]
fn q1394_q1_programs_sharing_only_the_services_secret_derive_nothing_alike() {
    assert_eq!(two_shops("q1-none", ["", ""]), "300000", "one payment for two programs' orders");
    assert_eq!(two_shops("q1-own", ["secret-of-shop-a-0123456789abcdef", "secret-of-shop-b-0123456789abcdef"]), "300000");
}

const WHO_NOTE: &str = r#"let calls = []
server fn who(note: str) -> str {
    calls = calls + [request.call]
    request.call
}
server fn seen() -> list<str> = calls
"#;

const ONE_CALL_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0 }
db.open("shop.db")
let failing = 0
route POST /failing/:n {
    failing = int(n)
    "ok"
}
route POST /one with http, db = idempotent(() => {
    let a = billing.who("a") ?? reply 503 "busy"
    if failing > 0 {
        failing -= 1
        reply 503 "once more"
    }
    a
})
route POST /seen with http = join(billing.seen() ?? [], ",")
"#;

/// Q-1394, the concurrency review's probe 11: billing down at the shop's first chained call (its
/// question about chains unanswered), then started: asked again a few seconds later, it gets
/// derived ids, the same key sending the same one. Before, the first silence was taken as "no" for
/// the process's life.
#[test]
fn q1394_probe11_a_callee_down_at_first_is_asked_again() {
    let d = dir("probe11", &[("billing.qwe", WHO_NOTE), ("main.qwe", ONE_CALL_SHOP)]);
    let (sp, bp) = (support::free_port(), support::free_port());
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", bp)], &[]);
    assert_eq!(buy(sp, "/one", "k0"), (503, false, "busy".to_string()));
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    std::thread::sleep(Duration::from_secs(6));
    assert_eq!(http(sp, "POST", "/failing/1", &[], "").2, "ok");
    assert_eq!(buy(sp, "/one", "k1").0, 503);
    let (status, _, again) = buy(sp, "/one", "k1");
    assert_eq!(status, 200);
    // The same id again is answered from billing's record: its function ran once.
    let ids: Vec<String> = http(sp, "POST", "/seen", &[], "").2.split(',').map(String::from).collect();
    assert_eq!(ids, vec![again], "the same key sent other ids");
}

const FLAKY_BILLING: &str = r#"let fail = 0
let count = 0
route POST /fail-next {
    fail = 1
    "1"
}
server fn charge() -> str ! str {
    count += 1
    if fail > 0 {
        fail = 0
        return err("try later")
    }
    `ran ${count}`
}
"#;

const TXN_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0 }
db.open("shop.db")
route POST /pay with http, db = idempotent(() => {
    let r = billing.charge() ?? reply 503 "busy"
    r
})
"#;

/// Q-1394, the concurrency review's probe 12: billing answers an error value, the shop replies early
/// (nothing kept), and the client tries again with the key at once, in the same second: the same
/// derived id, sent with other signed bytes (the deadline's count), runs again. Before, the same
/// bytes were refused as a replay (409, "busy" here).
#[test]
fn q1394_probe12_a_retry_in_the_same_second_runs_again() {
    let d = dir("probe12", &[("billing.qwe", FLAKY_BILLING), ("main.qwe", TXN_SHOP)]);
    let (sp, bp) = (support::free_port(), support::free_port());
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", bp)], &[]);
    for i in 0..3 {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
        std::thread::sleep(Duration::from_millis(1000 - u64::from(now.subsec_millis())) + Duration::from_millis(20));
        assert_eq!(http(bp, "POST", "/fail-next", &[], "").2, "1");
        let key = format!("k{}", i);
        assert_eq!(buy(sp, "/pay", &key).0, 503);
        let (status, _, body) = buy(sp, "/pay", &key);
        assert_eq!(status, 200, "try {}: the retry was refused: {}", i, body);
    }
}

const OTHER_DB_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0 }
db.open("shop.db")
let other = db.connect("other.db")
other.exec("create table if not exists ledger (id integer primary key, note text not null)")
route POST /pay with http, db = idempotent({ transaction: false }, () => {
    other.exec("insert into ledger (note) values (?)", "paid 1200 to the warehouse")
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "billing is down"
    `paid ${inv.id}`
})
route GET /n with db = str(other.one("select count(*) as n from ledger").n)
"#;

/// Q-1391, the security review's q2: a write to another database (`db.connect`), which the run's
/// `idempotent` record is not kept with, then billing never reached: the end is kept, and the
/// write made once. Before, a `db.connect` write was the program's own database, and each try
/// wrote again.
#[test]
fn q1391_q2_a_write_to_another_database_keeps_the_end() {
    let d = dir("q2", &[("billing.qwe", Q1391_BILLING), ("main.qwe", OTHER_DB_SHOP)]);
    let sp = support::free_port();
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", support::free_port())], &[]);
    assert_eq!(buy(sp, "/pay", "k1"), (503, false, "billing is down".to_string()));
    assert_eq!(buy(sp, "/pay", "k1"), (503, true, "billing is down".to_string()));
    assert_eq!(http(sp, "GET", "/n", &[], "").2, "1", "the same key wrote again");
}

const Q3_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0 }
db.open("shop.db")
let other = db.connect("other.db")
other.exec("create table if not exists ledger (id integer primary key, note text not null)")
let sh = db.shards(["s1.db", "s2.db"])
sh.migrate("m")
route POST /one with http, db = idempotent({ transaction: false }, () => {
    let row = other.one("insert into ledger (note) values (?) returning id", "paid 1200")
    let inv = billing.invoice("Ada", 1200) ?? reply 503 "billing is down"
    `paid ${inv.id}`
})
route POST /query with http, db = idempotent({ transaction: false }, () => {
    let rows = other.query("insert into ledger (note) values (?) returning id", "paid 1300")
    let inv = billing.invoice("Ada", 1300) ?? reply 503 "billing is down"
    `paid ${inv.id}`
})
route POST /shard with http, db = idempotent({ transaction: false }, () => {
    let row = sh.on("ada").one("insert into ledger (note) values (?) returning id", "paid 1400")
    let inv = billing.invoice("Ada", 1400) ?? reply 503 "billing is down"
    `paid ${inv.id}`
})
route GET /n with db = str(other.one("select count(*) as n from ledger").n)
route GET /ns with db = str(sh.all("select count(*) as n from ledger"))
"#;

/// Q-1391, the security review's q3: `one`, `query` and a shard's `one` on a database that is not
/// the program's own run any statement (an `insert ... returning` here), then billing never reached:
/// the end is kept, and each write made once. Before, only `exec` and the like marked the run.
#[test]
fn q1391_q3_one_and_query_on_another_database_keep_the_end() {
    let d = dir("q3", &[("billing.qwe", Q1391_BILLING), ("main.qwe", Q3_SHOP)]);
    std::fs::create_dir_all(d.join("m")).unwrap();
    std::fs::write(d.join("m/001.sql"), "create table ledger (id integer primary key, note text not null);\n").unwrap();
    let sp = support::free_port();
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", support::free_port())], &[]);
    for (path, key) in [("/one", "k1"), ("/query", "k2"), ("/shard", "k3")] {
        assert_eq!(buy(sp, path, key), (503, false, "billing is down".to_string()), "{}", path);
        assert_eq!(buy(sp, path, key), (503, true, "billing is down".to_string()), "{}: the end was not kept", path);
    }
    assert_eq!(http(sp, "GET", "/n", &[], "").2, "2", "the same keys wrote again");
}

const SHORT_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0, timeout: 30 }
db.open("shop.db")
route POST /pay with http, db = idempotent(() => {
    let r = billing.declined()
    if is_err(r) { return err(str(r)) }
    r
})
"#;

/// Q-1394, the review's j1: a 30 ms timeout, the same key sent again in the same second after two
/// other keys' calls: its own sends never sign the same bytes, so each runs. Before, a count shared
/// by the process's sends, over a span of 3, came round to the same deadline, and the third send of
/// the key was refused as a replay.
#[test]
fn q1394_j1_a_short_timeout_never_repeats_a_calls_bytes() {
    let d = dir("j1", &[("billing.qwe", P5_BILLING), ("main.qwe", SHORT_SHOP)]);
    let (sp, bp) = (support::free_port(), support::free_port());
    let _billing = Proc::start(&d, "billing.qwe", bp, &["--as-service"], &[]);
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", bp)], &[]);
    let _ = buy(sp, "/pay", "warm");
    let mut sent = 1;
    for round in 0..3 {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
        std::thread::sleep(Duration::from_millis(1000 - u64::from(now.subsec_millis())) + Duration::from_millis(10));
        for key in ["K", "X1", "X2", "K", "K"] {
            let _ = buy(sp, "/pay", &format!("{}{}", key, round));
            sent += 1;
        }
    }
    assert_eq!(http(bp, "GET", "/tries", &[], "").2, sent.to_string(), "a send of the key was refused as a replay");
}

/// Q-1394, the review's r1: a chained call whose error answer was not kept, captured and sent again
/// to each of two copies of billing sharing its database: run once, refused by both after. Before,
/// each copy remembered only what it had seen, and the other ran it again.
#[test]
fn q1394_r1_a_captured_errored_call_is_refused_by_every_copy() {
    let d = dir("r1", &[("billing.qwe", P5_BILLING)]);
    let (b1, b2) = (support::free_port(), support::free_port());
    let _one = Proc::start(&d, "billing.qwe", b1, &["--as-service"], &[]);
    let _two = Proc::start(&d, "billing.qwe", b2, &["--as-service"], &[]);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let root = "0123456789abcdef0123456789abcdef";
    let w = word(SECRET, "declined", "eeee4444ffff5555", root);
    let got: Vec<u16> = [b1, b2, b1, b2].iter().map(|p| signed_call(*p, t, "declined", "eeee4444ffff5555", Some(w.clone())).0).collect();
    assert_eq!(got, vec![200, 409, 409, 409]);
    assert_eq!(http(b1, "GET", "/tries", &[], "").2, "1", "a copy ran it again");
}

/// A listener that takes connections and never answers, noting when each request's first line came.
fn black_hole() -> (u16, Arc<Mutex<Vec<(Instant, String)>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let at = l.local_addr().unwrap().port();
    let got = Arc::new(Mutex::new(Vec::new()));
    let g = got.clone();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for mut c in l.incoming().flatten() {
            let g = g.clone();
            let _ = c.set_read_timeout(Some(Duration::from_secs(1)));
            let mut buf = [0u8; 4096];
            if let Ok(n) = c.read(&mut buf) {
                let line = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string();
                g.lock().unwrap().push((Instant::now(), line));
            }
            held.push(c);
        }
    });
    (at, got)
}

const HOLE_SHOP: &str = r#"service billing = "billing.qwe" { retries: 0, timeout: "6s", failures: 1000 }
db.open("shop.db")
route POST /pay with http, db = idempotent({ transaction: false }, () => {
    let inv = billing.invoice("Ada", 1) ?? reply 503 "billing is down"
    "ok"
})
"#;

/// Q-1394, the reviews' d1, R7-C and R7-E: billing takes connections and never answers (6 s timeout),
/// a keyed request each second. The question has a timeout of its own (1 s): the first request
/// waits at most that before its call, and the address is asked once in the first six and a half
/// seconds (again only `ASK_AGAIN` after the question gave up), never over a question still out. Before, the question waited the service's whole timeout
/// before the call (12 s for the first request), and a request 5 s in asked again over it.
#[test]
fn q1394_d1_a_callee_that_never_answers_is_asked_once_and_briefly() {
    let d = dir("d1", &[("billing.qwe", Q1391_BILLING), ("main.qwe", HOLE_SHOP)]);
    let (hole, got) = black_hole();
    let sp = support::free_port();
    let _shop = Proc::start(&d, "main.qwe", sp, &["--service-url", &format!("billing=http://127.0.0.1:{}", hole)], &[]);
    let start = Instant::now();
    let mut reqs = Vec::new();
    for i in 0..7 {
        reqs.push(std::thread::spawn(move || {
            let t = Instant::now();
            let r = buy(sp, "/pay", &format!("key{}", i));
            (r.0, t.elapsed())
        }));
        std::thread::sleep(Duration::from_millis(950));
    }
    let first = reqs.remove(0).join().unwrap();
    for r in reqs {
        let _ = r.join();
    }
    let asked: Vec<Duration> = got.lock().unwrap().iter().filter(|(_, l)| l.contains("_contracts")).map(|(t, _)| t.duration_since(start)).collect();
    assert_eq!(first.0, 503);
    assert!(first.1 < Duration::from_millis(8000), "the first request waited {:?}", first.1);
    assert_eq!(asked.iter().filter(|t| **t < Duration::from_millis(6500)).count(), 1, "asked: {:?}", asked);
}
