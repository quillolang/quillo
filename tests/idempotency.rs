//! `idempotent(f)`: requests with an `Idempotency-Key` run once, against a
//! `quillo run` server on SQLite. (PostgreSQL's turn is in isolation.rs.)

mod support;

use std::time::Duration;
use support::{send, serve, temp_dir};

const APP: &str = r#"
db.open("app.db")
db.exec("create table if not exists runs (what text)")
tenant_from((r) => r.headers["x-tenant"])

fn ran(what) = db.one("select count(*) as c from runs where what = ?", what).c

// Kept: what the function returns.
route POST /pay { json: { amount: int } } = idempotent(() => {
    db.exec("insert into runs values ('pay')")
    respond(201, { paid: amount, runs: ran("pay") }, { location: "/payments/1", "x-kept": "yes" })
})

// A failure is rolled back and not kept: a retry runs again.
let fails_left = 1
route POST /flaky = idempotent(() => {
    db.exec("insert into runs values ('flaky')")
    if fails_left > 0 {
        fails_left -= 1
        reply 503 { error: "try again" }
    }
    { runs: ran("flaky") }
})

route POST /needs-key = idempotent({ required: true }, () => "ran")

route POST /login { json: { user: str } } { set_session({ user }); "ok" }
route POST /mine = idempotent(() => { db.exec("insert into runs values ('mine')"); { user: session().user, runs: ran("mine") } })
route POST /api = idempotent({ user: request.headers["x-api-user"] }, () => {
    db.exec("insert into runs values ('api')")
    { tenant: tenant(), runs: ran("api") }
})

// Without a transaction, so a second request can come while it sleeps.
route POST /slow = idempotent({ transaction: false }, () => {
    db.exec("insert into runs values ('slow')")
    sleep(700)
    { runs: ran("slow") }
})
// In a transaction: on SQLite the second request waits for the first.
route POST /slow-tx = idempotent(() => {
    db.exec("insert into runs values ('slow-tx')")
    sleep(700)
    { runs: ran("slow-tx") }
})

route POST /short = idempotent({ max_age: 1 }, () => { db.exec("insert into runs values ('short')"); { runs: ran("short") } })

route POST /crash = idempotent(() => {
    db.exec("insert into runs values ('crash')")
    sleep(5000)
    { runs: ran("crash") }
})
route POST /crash-plain = idempotent({ transaction: false }, () => {
    db.exec("insert into runs values ('crash-plain')")
    sleep(5000)
    { runs: ran("crash-plain") }
})
// What a server that stopped leaves, a minute later.
route POST /leases-over = db.exec("update _quillo_idempotency set lease_until = 0").changes

server fn add(n: int) = idempotent(() => {
    db.exec("insert into runs values ('add')")
    { n: n + 1, runs: ran("add") }
})
component Adder() {
    state got = null
    <button @click={() => { got = add(1) }}>add</button>
}
route GET / = <html><body><Adder/></body></html>

route GET /runs/:what = ran(what)
route GET /keys = db.one("select count(*) as c from _quillo_idempotency").c
"#;

fn app(name: &str) -> (std::path::PathBuf, support::Server) {
    let dir = temp_dir(&format!("idem-{}", name));
    let file = dir.join("app.qwe");
    std::fs::write(&file, APP).unwrap();
    let s = serve(&file, &[]);
    (file, s)
}

fn runs(port: u16, what: &str) -> String {
    send(port, "GET", &format!("/runs/{}", what), &[], "").body
}

#[test]
fn a_retry_gets_the_first_answer_without_running_again() {
    let (_f, s) = app("retry");
    let first = send(s.port, "POST", "/pay", &[("Idempotency-Key", "k1")], r#"{"amount": 5}"#);
    assert_eq!(first.status, 201, "{}", first.body);
    assert_eq!(first.body, r#"{"paid":5,"runs":1}"#);
    assert_eq!(first.header("idempotent-replayed"), None);
    for _ in 0..3 {
        let again = send(s.port, "POST", "/pay", &[("Idempotency-Key", "k1")], r#"{"amount": 5}"#);
        assert_eq!((again.status, again.body.as_str()), (201, first.body.as_str()));
        assert_eq!(again.header("location"), Some("/payments/1"));
        assert_eq!(again.header("x-kept"), Some("yes"));
        assert_eq!(again.header("idempotent-replayed"), Some("true"));
    }
    assert_eq!(runs(s.port, "pay"), "1");
    // Another key is another request; no key runs every time.
    assert_eq!(send(s.port, "POST", "/pay", &[("Idempotency-Key", "k2")], r#"{"amount": 5}"#).body, r#"{"paid":5,"runs":2}"#);
    send(s.port, "POST", "/pay", &[], r#"{"amount": 5}"#);
    send(s.port, "POST", "/pay", &[], r#"{"amount": 5}"#);
    assert_eq!(runs(s.port, "pay"), "4");
    // A key that is not one.
    assert_eq!(send(s.port, "POST", "/pay", &[("Idempotency-Key", &"x".repeat(300))], r#"{"amount": 5}"#).status, 400);
    let missing = send(s.port, "POST", "/needs-key", &[], "");
    assert_eq!(missing.status, 400);
    assert!(missing.body.contains("Idempotency-Key"), "{}", missing.body);
    assert_eq!(send(s.port, "POST", "/needs-key", &[("Idempotency-Key", "n1")], "").body, "ran");
}

#[test]
fn the_same_key_with_another_request_is_refused() {
    let (_f, s) = app("other-body");
    assert_eq!(send(s.port, "POST", "/pay", &[("Idempotency-Key", "k")], r#"{"amount": 5}"#).status, 201);
    let other = send(s.port, "POST", "/pay", &[("Idempotency-Key", "k")], r#"{"amount": 500}"#);
    assert_eq!(other.status, 422, "{}", other.body);
    assert!(other.body.contains("another request"), "{}", other.body);
    assert_eq!(send(s.port, "POST", "/pay?to=x", &[("Idempotency-Key", "k")], r#"{"amount": 5}"#).status, 422);
    assert_eq!(send(s.port, "POST", "/flaky", &[("Idempotency-Key", "k")], r#"{"amount": 5}"#).status, 422);
    assert_eq!(runs(s.port, "pay"), "1");
    assert_eq!(runs(s.port, "flaky"), "0");
}

#[test]
fn a_failure_is_rolled_back_and_not_kept() {
    let (_f, s) = app("flaky");
    let first = send(s.port, "POST", "/flaky", &[("Idempotency-Key", "f")], "");
    assert_eq!(first.status, 503);
    assert_eq!(runs(s.port, "flaky"), "0", "the failed run's insert is rolled back");
    let second = send(s.port, "POST", "/flaky", &[("Idempotency-Key", "f")], "");
    assert_eq!((second.status, second.body.as_str()), (200, r#"{"runs":1}"#));
    let third = send(s.port, "POST", "/flaky", &[("Idempotency-Key", "f")], "");
    assert_eq!((third.status, third.body.as_str(), third.header("idempotent-replayed")), (200, r#"{"runs":1}"#, Some("true")));
    assert_eq!(runs(s.port, "flaky"), "1");
}

#[test]
fn concurrent_duplicates_run_once() {
    let (_f, s) = app("concurrent");
    let port = s.port;
    let go = move |path: &'static str| std::thread::spawn(move || send(port, "POST", path, &[("Idempotency-Key", path)], ""));
    // Without a transaction: the second sees the first running, and gets 409.
    let a = go("/slow");
    std::thread::sleep(Duration::from_millis(200));
    let b = go("/slow");
    let (a, b) = (a.join().unwrap(), b.join().unwrap());
    let mut statuses = [a.status, b.status];
    statuses.sort();
    assert_eq!(statuses, [200, 409], "{} / {}", a.body, b.body);
    let conflict = if a.status == 409 { &a } else { &b };
    assert!(conflict.body.contains("still running"), "{}", conflict.body);
    assert_eq!(runs(port, "slow"), "1");
    // Once it is done, the duplicate gets its answer.
    let later = send(port, "POST", "/slow", &[("Idempotency-Key", "/slow")], "");
    assert_eq!((later.status, later.body.as_str()), (200, r#"{"runs":1}"#));

    // In a transaction on SQLite, the second waits for the first (its
    // transaction holds the database), then gets its answer.
    let a = go("/slow-tx");
    std::thread::sleep(Duration::from_millis(200));
    let b = go("/slow-tx");
    let (a, b) = (a.join().unwrap(), b.join().unwrap());
    assert_eq!((a.status, b.status), (200, 200), "{} / {}", a.body, b.body);
    assert_eq!(a.body, b.body);
    assert_eq!(runs(port, "slow-tx"), "1");
}

#[test]
fn keys_belong_to_a_user_and_a_tenant() {
    let (_f, s) = app("scoped");
    let login = |user: &str| {
        let r = send(s.port, "POST", "/login", &[], &format!(r#"{{"user": "{}"}}"#, user));
        r.header("set-cookie").unwrap().split(';').next().unwrap().to_string()
    };
    let (ada, bob) = (login("ada"), login("bob"));
    let mine = |cookie: &str| send(s.port, "POST", "/mine", &[("Idempotency-Key", "shared"), ("Cookie", cookie)], "").body;
    assert_eq!(mine(&ada), r#"{"user":"ada","runs":1}"#);
    assert_eq!(mine(&bob), r#"{"user":"bob","runs":2}"#, "bob's request runs: he cannot read ada's answer");
    assert_eq!(mine(&ada), r#"{"user":"ada","runs":1}"#);
    assert_eq!(mine(&bob), r#"{"user":"bob","runs":2}"#);
    assert_eq!(runs(s.port, "mine"), "2");

    // The user an option names (a token's, say), and the tenant.
    let api = |user: &str, tenant: &str| {
        send(s.port, "POST", "/api", &[("Idempotency-Key", "shared"), ("x-api-user", user), ("x-tenant", tenant)], "").body
    };
    assert_eq!(api("u1", "acme"), r#"{"tenant":"acme","runs":1}"#);
    assert_eq!(api("u2", "acme"), r#"{"tenant":"acme","runs":2}"#);
    assert_eq!(api("u1", "beta"), r#"{"tenant":"beta","runs":3}"#);
    assert_eq!(api("u1", "acme"), r#"{"tenant":"acme","runs":1}"#);
    assert_eq!(runs(s.port, "api"), "3");
}

#[test]
fn answers_expire() {
    let (_f, s) = app("expiry");
    let first = send(s.port, "POST", "/short", &[("Idempotency-Key", "e")], "");
    assert_eq!(first.body, r#"{"runs":1}"#);
    assert_eq!(send(s.port, "POST", "/short", &[("Idempotency-Key", "e")], "").body, r#"{"runs":1}"#);
    std::thread::sleep(Duration::from_millis(1200));
    // Expired: the key is new again, and the old answer is gone.
    assert_eq!(send(s.port, "POST", "/short", &[("Idempotency-Key", "e")], "").body, r#"{"runs":2}"#);
    assert_eq!(send(s.port, "GET", "/keys", &[], "").body, "1");
}

#[test]
fn server_fns_called_from_pages_send_a_key_and_get_the_first_answer() {
    let (_f, s) = app("rpc");
    let bundle = send(s.port, "GET", "/_quillo/quillo.js", &[], "").body;
    assert!(bundle.contains(r#"W.server("add", true)"#), "the page sends keys for `add`");
    assert!(bundle.contains("idempotency-key"));

    let call = |key: &str| send(s.port, "POST", "/_quillo/rpc/add", &[("Idempotency-Key", key), ("Sec-Fetch-Site", "same-origin")], "[41]");
    let first = call("r1");
    assert_eq!((first.status, first.body.as_str()), (200, r#"{"ok":{"n":42,"runs":1}}"#));
    let again = call("r1");
    assert_eq!((again.status, again.body.as_str(), again.header("idempotent-replayed")), (200, first.body.as_str(), Some("true")));
    assert_eq!(call("r2").body, r#"{"ok":{"n":42,"runs":2}}"#);
    let other = send(s.port, "POST", "/_quillo/rpc/add", &[("Idempotency-Key", "r1")], "[1]");
    assert_eq!(other.status, 422);
    assert_eq!(runs(s.port, "add"), "2");
}

#[test]
fn a_server_that_stops_part_way_leaves_nothing_done() {
    let (file, mut s) = app("crash");
    let port = s.port;
    // One without a transaction (its sleep lets other requests run), then
    // one in a transaction; the server stops while both sleep.
    let u = std::thread::spawn(move || send(port, "POST", "/crash-plain", &[("Idempotency-Key", "p")], ""));
    std::thread::sleep(Duration::from_millis(300));
    let t = std::thread::spawn(move || send(port, "POST", "/crash", &[("Idempotency-Key", "c")], ""));
    std::thread::sleep(Duration::from_millis(300));
    s.kill();
    let _ = (t.join(), u.join());

    let s = serve(&file, &[]);
    let port = s.port;
    // The transaction rolled back; the claim is left, for a minute.
    assert_eq!(runs(port, "crash"), "0");
    let soon = send(port, "POST", "/crash", &[("Idempotency-Key", "c")], "");
    assert_eq!(soon.status, 409, "{}", soon.body);
    send(port, "POST", "/leases-over", &[], "");
    // Then the next request with the key takes it over, and runs it once.
    let next = send(port, "POST", "/crash", &[("Idempotency-Key", "c")], "");
    assert_eq!((next.status, next.body.as_str()), (200, r#"{"runs":1}"#));
    assert_eq!(send(port, "POST", "/crash", &[("Idempotency-Key", "c")], "").header("idempotent-replayed"), Some("true"));
    // Without a transaction it is not known whether the work happened (here
    // it did: the insert was in no transaction): 409, until the key expires.
    assert_eq!(runs(port, "crash-plain"), "1");
    let unknown = send(port, "POST", "/crash-plain", &[("Idempotency-Key", "p")], "");
    assert_eq!(unknown.status, 409);
    assert!(unknown.body.contains("not known"), "{}", unknown.body);
    assert_eq!(runs(port, "crash-plain"), "1");
}
