//! FEEDBACK44.md: what one request's transaction lets another see (branch feedback40-43). A
//! SQLite transaction waits in place for another database, `begin` and `commit` by hand are
//! refused, and a PostgreSQL connection's session is reset before another request gets it.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::Path;
use std::time::{Duration, Instant};

fn app(dir: &Path, src: &str) -> Interp {
    let mut it = Interp::new(dir.to_str().unwrap());
    if let Err(e) = it.load_source(src, "main.qwe") {
        panic!("{}", e);
    }
    it
}

fn send(it: &mut Interp, method: &str, url: &str) -> HttpResponse {
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: vec![("Host".into(), "localhost".into())], body: Vec::new(), remote: "127.0.0.1".into() })
}

fn check(src: &str) -> Vec<String> {
    quillo::check::check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().map(|d| format!("{} {}", d.line, d.message)).collect()
}

// Q-579: inside a SQLite transaction a Postgres query paused the request, and others ran on
// SQLite's one connection, inside the transaction: they read its rows, and lost their own.
#[test]
fn a_sqlite_transaction_keeps_other_requests_out_while_it_asks_postgres() {
    let Some(pg) = support::postgres("f44-hold") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("hold");
    let d = support::temp_dir("f44-hold");
    std::fs::write(
        d.join("main.qwe"),
        format!(
            r#"db.open("app.db")
db.exec("create table if not exists t (id integer primary key, v text)")
let pg = db.connect("{}")

route POST /a = db.transaction(() => {{
    db.exec("insert into t (v) values ('A: not committed')")
    pg.query("select pg_sleep(1)")
    err("A gives up")
}})
route GET /b = json.stringify(db.query("select v from t"))
route POST /c = {{
    db.exec("insert into t (v) values ('C')")
    "C ok"
}}
"#,
            url
        ),
    )
    .unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let port = s.port;
    let a = std::thread::spawn(move || support::send(port, "POST", "/a", &[], ""));
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    let b = support::send(port, "GET", "/b", &[], "");
    // B waited for A, and never saw its row.
    assert_eq!(b.body, "[]", "B read inside A's transaction");
    assert!(t.elapsed() >= Duration::from_millis(400), "B did not wait: {:?}", t.elapsed());
    let c = support::send(port, "POST", "/c", &[], "");
    assert_eq!(c.body, "C ok");
    let a = a.join().unwrap();
    assert_eq!(a.status, 500);
    // C's row stays: it was written after A ended, not inside A.
    assert_eq!(support::send(port, "GET", "/b", &[], "").body, r#"[{"v":"C"}]"#);
}

// Q-580: a `begin` whose `commit` was never reached left the shared connection in it.
#[test]
fn a_transaction_by_hand_is_refused() {
    // A database's error is shown as under `--watch`, where the answer says it.
    quillo::server::SHOW_ERRORS.store(true, std::sync::atomic::Ordering::Relaxed);
    let d = support::temp_dir("f44-begin");
    let mut it = app(
        &d,
        r#"db.open("app.db")
db.exec("create table if not exists t (id integer primary key, v text)")
route POST /begin = db.exec("begin")
route POST /commit = db.exec("COMMIT;")
route POST /start = db.query("  -- a comment\n start transaction")
route POST /save = db.one("savepoint s")
route POST /script = db.exec("begin transaction; insert into t (v) values ('script'); commit;")
route POST /left = db.exec("begin; insert into t (v) values ('left')")
route POST /c = db.exec("insert into t (v) values ('C')")
route GET /rows = json.stringify(db.query("select v from t order by id"))
route GET /words = json.stringify(db.query("select 'begin' as w"))
"#,
    );
    for url in ["/begin", "/commit", "/start", "/save"] {
        let r = send(&mut it, "POST", url);
        assert_eq!(r.status, 500, "{}", url);
    }
    // A script that ends what it begins runs; one that leaves a transaction open is rolled
    // back, and later writes are their own.
    assert_eq!(send(&mut it, "POST", "/script").status, 200);
    let left = send(&mut it, "POST", "/left");
    assert!(left.text().contains("left a transaction open"), "{}", left.text());
    assert_eq!(send(&mut it, "POST", "/c").status, 200);
    assert_eq!(send(&mut it, "GET", "/rows").text(), r#"[{"v":"script"},{"v":"C"}]"#);
    // Its edge: the word inside a statement is no transaction.
    assert_eq!(send(&mut it, "GET", "/words").text(), r#"[{"w":"begin"}]"#);
    // `quillo check` says so for the literal text.
    assert_eq!(
        check("db.exec(\"begin\")\nprint(db.query(\"rollback\"))\nprint(db.exec(\"begin; select 1; commit\"))\n"),
        [
            "1 `db.exec(\"begin\")`: write `db.transaction(() => ...)`, which commits, rolls back when something fails, and keeps other requests out of it",
            "2 `db.query(\"rollback\")`: write `db.transaction(() => ...)`, which commits, rolls back when something fails, and keeps other requests out of it",
        ]
    );
}

// Q-581: `set`, `set_config(..., false)` and temporary tables outlived their request, and the
// next request on the connection read them.
#[test]
fn a_postgres_session_is_reset_before_another_request_gets_it() {
    let Some(pg) = support::postgres("f44-session") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("session");
    quillo::server::SHOW_ERRORS.store(true, std::sync::atomic::Ordering::Relaxed);
    let d = support::temp_dir("f44-session");
    let mut it = app(
        &d,
        &format!(
            r#"db.open("{}?pool_size=1")
route POST /alice = db.transaction(() => {{
    db.exec("set app.user_id = 'alice'")
    db.exec("create temp table if not exists cart (item text)")
    db.exec("insert into cart values ('ring')")
    "alice ok"
}})
route GET /bob = db.transaction(() => {{
    db.exec("create temp table if not exists cart (item text)")
    json.stringify({{ user: db.one("select current_setting('app.user_id', true) as u").u, cart: db.query("select item from cart") }})
}})
route POST /local = db.transaction(() => {{
    db.exec("set local app.user_id = 'carol'")
    db.one("select current_setting('app.user_id', true) as u").u
}})
route POST /outside = db.one("select set_config('app.user_id', 'mallory', false)")
route POST /outside-set = db.exec("set search_path = other")
route POST /outside-local = db.one("select set_config('app.user_id', 'x', true) as u").u
"#,
            url
        ),
    );
    assert_eq!(send(&mut it, "POST", "/alice").text(), "alice ok");
    let bob = send(&mut it, "GET", "/bob").text();
    assert!(!bob.contains("alice") && !bob.contains("ring"), "{}", bob);
    // `set local` stays inside its transaction, as it always did.
    assert_eq!(send(&mut it, "POST", "/local").text(), "carol");
    // Outside a transaction, a change to the session is refused.
    for url in ["/outside", "/outside-set"] {
        let r = send(&mut it, "POST", url);
        assert!(r.text().contains("outside `db.transaction` this connection is shared"), "{}: {}", url, r.text());
    }
    assert_eq!(send(&mut it, "POST", "/outside-local").text(), "x");
}

// The review of FEEDBACK44's first version.
#[test]
fn a_service_called_in_a_sqlite_transaction_keeps_others_out_too() {
    let d = support::temp_dir("f44-service");
    std::fs::write(d.join("slow.qwe"), "server fn wait(ms: int) -> int {\n    sleep(ms)\n    ms\n}\n").unwrap();
    std::fs::write(
        d.join("main.qwe"),
        r#"db.open("app.db")
db.exec("create table if not exists t (id integer primary key, v text)")
service slow = "slow.qwe"
route POST /a = db.transaction(() => {
    db.exec("insert into t (v) values ('A1')")
    slow.wait(800)?
    db.exec("insert into t (v) values ('A2')")
    err("A gives up")
})
route GET /b = json.stringify(db.query("select v from t"))
"#,
    )
    .unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let port = s.port;
    let a = std::thread::spawn(move || support::send(port, "POST", "/a", &[], ""));
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(support::send(port, "GET", "/b", &[], "").body, "[]");
    assert_eq!(a.join().unwrap().status, 500);
    // All of A rolled back, none of it half committed.
    assert_eq!(support::send(port, "GET", "/b", &[], "").body, "[]");
}

#[test]
fn postgres_in_a_sqlite_transaction_never_holds_the_server_and_sessions_reset_whole() {
    let Some(pg) = support::postgres("f44-review") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("review");
    let psql = |sql: &str| {
        let out = std::process::Command::new(pg.bin.join("psql")).args([&url, "-q", "-c", sql]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    };
    psql("create role lowpriv");
    psql("create table t (v text)");
    let d = support::temp_dir("f44-review");
    std::fs::write(
        d.join("main.qwe"),
        format!(
            r#"db.open("app.db")
db.exec("create table if not exists s (v text)")
let pg = db.connect("{url}?pool_size=1")
route POST /p = pg.transaction(() => {{
    pg.query("select pg_sleep(0.6)")
    "p"
}})
route POST /a = db.transaction(() => {{
    pg.query("select 1")
    "a"
}})
route GET /h = "h"
route POST /script = pg.exec("begin; insert into t (v) values ('script')")
route POST /as-low = pg.transaction(() => {{
    pg.exec("set session authorization lowpriv")
    pg.exec("declare c cursor with hold for select 1")
    "low"
}})
route GET /who = pg.transaction(() => pg.one("select current_user as u, (select count(*) from pg_cursors where name = 'c') as cursors")) | json.stringify
route POST /prep = pg.exec("prepare p1 as select 1")
route POST /tmp = pg.exec("create local temporary table cart (item text)")
route POST /into = pg.query("select 1 as x into temp t2")
route POST /lock = pg.one("select pg_try_advisory_lock(42) as l")
route POST /fine = pg.one("select set_config('app.user_id', 'x', true) as a, coalesce(null, false) as b") | json.stringify
"#
        ),
    )
    .unwrap();
    quillo::server::SHOW_ERRORS.store(true, std::sync::atomic::Ordering::Relaxed);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let port = s.port;
    // The pool's one connection is busy: inside a SQLite transaction that is an error at once,
    // not 30 s of every request waiting.
    let p = std::thread::spawn(move || support::send(port, "POST", "/p", &[], ""));
    std::thread::sleep(Duration::from_millis(150));
    let t = Instant::now();
    let a = support::send(port, "POST", "/a", &[], "");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    assert_eq!(a.status, 500);
    let t = Instant::now();
    assert_eq!(support::send(port, "GET", "/h", &[], "").body, "h");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    assert_eq!(p.join().unwrap().body, "p");
    // A script's `begin` on a shared connection is refused.
    assert_eq!(support::send(port, "POST", "/script", &[], "").status, 500);
    // The session authorization and a held cursor do not reach the next request.
    assert_eq!(support::send(port, "POST", "/as-low", &[], "").body, "low");
    assert_eq!(support::send(port, "GET", "/who", &[], "").body, r#"{"u":"quillo","cursors":0}"#);
    // Outside a transaction: a named statement, a temporary table, `into temp` and a session's
    // lock are refused; `set_config(..., true)` next to a `false` elsewhere is not.
    for url in ["/prep", "/tmp", "/into", "/lock"] {
        assert_eq!(support::send(port, "POST", url, &[], "").status, 500, "{}", url);
    }
    assert_eq!(support::send(port, "POST", "/fine", &[], "").body, r#"{"a":"x","b":false}"#);
}
