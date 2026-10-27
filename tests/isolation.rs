//! `db.transaction({ isolation, retries }, f)` against real PostgreSQL and
//! MariaDB servers: two connections make a real conflict, and trying again
//! makes both succeed with the right result. Also `idempotent` on
//! PostgreSQL, where a duplicate that comes while the first runs gets 409.
//! Skipped for a database whose server is not installed.

mod support;

use quillo::Interp;
use std::sync::{Arc, Barrier};

fn program(dir: &std::path::Path, src: &str) -> Interp {
    let mut it = Interp::new(dir);
    it.load_source(src, "app.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    it
}

fn call(it: &mut Interp, name: &str) -> String {
    let f = quillo::value::lookup(&it.globals, name).unwrap();
    match it.call(&f, vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    }
}

/// Runs `go` in two programs at once (each with its own connection), and
/// gives back what each gave.
fn both(dir: &std::path::Path, src: &str, go: [&'static str; 2]) -> Vec<String> {
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = go
        .into_iter()
        .map(|name| {
            let (dir, src, barrier) = (dir.to_path_buf(), src.to_string(), barrier.clone());
            std::thread::spawn(move || {
                let mut it = program(&dir, &src);
                barrier.wait();
                call(&mut it, name)
            })
        })
        .collect();
    threads.into_iter().map(|t| t.join().unwrap()).collect()
}

fn eval(dir: &std::path::Path, src: &str, name: &str) -> String {
    call(&mut program(dir, src), name)
}

#[test]
fn postgres() {
    let Some(pg) = support::postgres("isolation") else {
        eprintln!("skipping: PostgreSQL's initdb and pg_ctl were not found");
        return;
    };
    let dir = support::temp_dir("isolation-pg");
    let url = pg.database("conflicts");
    let setup = format!("db.open(\"{}\")\ndb.exec(\"create table if not exists counter (id int primary key, n int not null)\")\n", url);
    let reset = || eval(&dir, &format!("{}fn reset() = db.exec(\"delete from counter; insert into counter values (1, 0)\")", setup), "reset");

    // Read, wait for the other to read too, write what was read plus one.
    let bump = |opts: &str, check: &str| {
        format!(
            r#"{setup}
let tries = 0
fn bump() {{
    let r = db.transaction({opts}() => {{
        tries += 1
        let n = db.one("select n from counter where id = 1").n
        sleep(300)
        db.exec("update counter set n = ? where id = 1", n + 1){check}
        n + 1
    }})
    is_err(r) ? `failed: ${{r.error}}` : `${{r}} after ${{tries}}`
}}
fn total() = db.one("select n from counter where id = 1").n
"#
        )
    };

    // Read committed (Postgres's default): one update is lost.
    reset();
    let src = bump("", "");
    let got = both(&dir, &src, ["bump", "bump"]);
    assert_eq!(got, ["\"1 after 1\"", "\"1 after 1\""]);
    assert_eq!(eval(&dir, &src, "total"), "1");

    // Serializable without retries: the second to write fails.
    reset();
    let src = bump("{ isolation: \"serializable\" }, ", "?");
    let mut got = both(&dir, &src, ["bump", "bump"]);
    got.sort();
    assert_eq!(got[0], "\"1 after 1\"");
    assert!(got[1].contains("failed: db: could not serialize access"), "{:?}", got);
    assert_eq!(eval(&dir, &src, "total"), "1");

    // With retries, it runs again, sees the other's write, and both count.
    for check in ["?", ""] {
        // (Without `?` the function goes on after the failed update; it is
        // still not committed, and is tried again.)
        reset();
        let src = bump("{ isolation: \"serializable\", retries: 3 }, ", check);
        let mut got = both(&dir, &src, ["bump", "bump"]);
        got.sort();
        assert_eq!(got, ["\"1 after 1\"", "\"2 after 2\""], "with {:?}", check);
        assert_eq!(eval(&dir, &src, "total"), "2");
    }

    // The level is the transaction's, and only the outermost one's.
    let src = format!(
        r#"{setup}
fn level() = db.one("show transaction_isolation").transaction_isolation
fn levels() = [
    db.transaction({{ isolation: "serializable" }}, () => level()),
    db.transaction({{ isolation: "repeatable_read" }}, () => level()),
    db.transaction(() => level()),
    level(),
]
fn nested() = db.transaction(() => db.transaction({{ isolation: "serializable" }}, () => 1))
fn nested_plain() = db.transaction({{ isolation: "serializable", retries: 2 }}, () => db.transaction(() => level()))
fn bad() = db.transaction({{ isolation: "snapshot" }}, () => 1)
"#
    );
    assert_eq!(eval(&dir, &src, "levels"), r#"["serializable", "repeatable read", "read committed", "read committed"]"#);
    assert!(eval(&dir, &src, "nested").contains("cannot change its isolation"));
    assert_eq!(eval(&dir, &src, "nested_plain"), "\"serializable\"");
    assert!(eval(&dir, &src, "bad").contains("\"serializable\", \"repeatable_read\" or \"read_committed\""));

    // Other failures are not tried again.
    let src = format!(
        r#"{setup}
let tries = 0
fn dup() {{
    let r = db.transaction({{ retries: 5 }}, () => {{ tries += 1; db.exec("insert into counter values (1, 0)")? }})
    [is_err(r), tries]
}}
"#
    );
    reset();
    assert_eq!(eval(&dir, &src, "dup"), "[true, 1]");

    idempotent_on_postgres(&pg.database("idem"));
}

/// A duplicate that comes while the first is running gets 409; then the
/// first's answer. Payments with retries on a serializable transaction.
fn idempotent_on_postgres(url: &str) {
    let dir = support::temp_dir("isolation-pg-idem");
    let file = dir.join("app.qwe");
    std::fs::write(
        &file,
        format!(
            r#"db.open("{url}")
db.exec("create table if not exists balance (id int primary key, n int not null); insert into balance values (1, 0) on conflict do nothing")
route POST /add {{ json: {{ n: int }} }} = idempotent({{ isolation: "serializable", retries: 5 }}, () => {{
    let b = db.one("select n from balance where id = 1").n
    sleep(300)
    db.exec("update balance set n = ? where id = 1", b + n)?
    {{ balance: b + n }}
}})
route GET /balance = db.one("select n from balance where id = 1").n
"#
        ),
    )
    .unwrap();
    let s = support::serve(&file, &[]);
    let port = s.port;
    let add = move |key: &'static str, n: i32| {
        std::thread::spawn(move || support::send(port, "POST", "/add", &[("Idempotency-Key", key)], &format!(r#"{{"n": {}}}"#, n)))
    };
    // Two at once with the same key: one runs, one is told to wait.
    let (a, b) = (add("k1", 5), add("k1", 5));
    let (a, b) = (a.join().unwrap(), b.join().unwrap());
    let mut statuses = [a.status, b.status];
    statuses.sort();
    assert_eq!(statuses, [200, 409], "{} / {}", a.body, b.body);
    let again = add("k1", 5).join().unwrap();
    assert_eq!((again.status, again.body.as_str()), (200, r#"{"balance":5}"#));
    // Different keys at once: a serialization conflict, tried again, and
    // both are counted once.
    let (c, d) = (add("k2", 10), add("k3", 100));
    let (c, d) = (c.join().unwrap(), d.join().unwrap());
    assert_eq!((c.status, d.status), (200, 200), "{} / {}", c.body, d.body);
    assert_eq!(support::send(port, "GET", "/balance", &[], "").body, "115");
    // Their answers are kept too.
    assert_eq!(add("k2", 10).join().unwrap().body, c.body);
    assert_eq!(support::send(port, "GET", "/balance", &[], "").body, "115");
}

#[test]
#[cfg(feature = "mysql")]
fn mariadb() {
    let Some(my) = support::mariadb("isolation") else {
        eprintln!("skipping: no MariaDB server (mariadbd, mariadb-install-db) was found");
        return;
    };
    let dir = support::temp_dir("isolation-my");
    assert_eq!(eval(&dir, &format!("db.open(\"{}\")\nfn make() = db.exec(\"create database app\").changes", my.url("mysql")), "make"), "1");
    let setup = format!(
        "db.open(\"{}\")\ndb.exec(\"create table if not exists acct (id int primary key, n int not null) engine = innodb\")\n",
        my.url("app")
    );
    let reset = || eval(&dir, &format!("{}fn reset() = db.exec(\"delete from acct; insert into acct values (1, 0), (2, 0)\")", setup), "reset");

    // A deadlock: each locks one row, then waits for the other's.
    let cross = |retries: u32| {
        format!(
            r#"{setup}
let tries = 0
fn go(a, b) {{
    let r = db.transaction({{ retries: {retries} }}, () => {{
        tries += 1
        db.exec("update acct set n = n + 1 where id = ?", a)?
        sleep(300)
        db.exec("update acct set n = n + 1 where id = ?", b)?
        "done"
    }})
    is_err(r) ? `failed: ${{r.error}}` : `${{r}} after ${{tries}}`
}}
fn one_two() = go(1, 2)
fn two_one() = go(2, 1)
fn totals() = db.query("select n from acct order by id") | map((r) => r.n)
"#
        )
    };
    reset();
    let src = cross(0);
    let mut got = both(&dir, &src, ["one_two", "two_one"]);
    got.sort();
    assert_eq!(got[0], "\"done after 1\"");
    assert!(got[1].contains("failed: db: ") && got[1].contains("Deadlock"), "{:?}", got);
    assert_eq!(eval(&dir, &src, "totals"), "[1, 1]");

    reset();
    let src = cross(3);
    let mut got = both(&dir, &src, ["one_two", "two_one"]);
    got.sort();
    assert_eq!(got, ["\"done after 1\"", "\"done after 2\""]);
    assert_eq!(eval(&dir, &src, "totals"), "[2, 2]");

    // Read, then write what was read plus one: in MySQL's serializable
    // level the reads share a lock, and the writes deadlock; tried again,
    // nothing is lost. (Repeatable read, the default, loses one.)
    let bump = |opts: &str| {
        format!(
            r#"{setup}
let tries = 0
fn bump() {{
    let r = db.transaction({opts}() => {{
        tries += 1
        let n = db.one("select n from acct where id = 1").n
        sleep(300)
        db.exec("update acct set n = ? where id = 1", n + 1)?
        n + 1
    }})
    is_err(r) ? `failed: ${{r.error}}` : `${{r}} after ${{tries}}`
}}
fn total() = db.one("select n from acct where id = 1").n
"#
        )
    };
    reset();
    let src = bump("");
    both(&dir, &src, ["bump", "bump"]);
    assert_eq!(eval(&dir, &src, "total"), "1");
    reset();
    let src = bump("{ isolation: \"serializable\", retries: 3 }, ");
    let mut got = both(&dir, &src, ["bump", "bump"]);
    got.sort();
    assert_eq!(got, ["\"1 after 1\"", "\"2 after 2\""]);
    assert_eq!(eval(&dir, &src, "total"), "2");

    // `idempotent` keeps its answers in MySQL's dialect too.
    let file = dir.join("idem.qwe");
    std::fs::write(
        &file,
        format!(
            r#"{setup}
route POST /add {{ json: {{ n: int }} }} = idempotent({{ isolation: "serializable", retries: 5 }}, () => {{
    db.exec("update acct set n = n + ? where id = 1", n)?
    {{ total: db.one("select n from acct where id = 1").n }}
}})
"#
        ),
    )
    .unwrap();
    reset();
    let s = support::serve(&file, &[]);
    let add = |key: &str, n: i32| support::send(s.port, "POST", "/add", &[("Idempotency-Key", key)], &format!(r#"{{"n": {}}}"#, n));
    let first = add("a", 5);
    assert_eq!((first.status, first.body.as_str()), (200, r#"{"total":5}"#));
    let again = add("a", 5);
    assert_eq!((again.body.as_str(), again.header("idempotent-replayed")), (r#"{"total":5}"#, Some("true")));
    assert_eq!(add("a", 6).status, 422);
    assert_eq!(add("b", 6).body, r#"{"total":11}"#);
}

#[test]
fn sqlite_takes_the_options() {
    let dir = support::temp_dir("isolation-sqlite");
    let src = r#"db.open("app.db")
db.exec("create table if not exists t (n int)")
fn go() = db.transaction({ isolation: "serializable", retries: 2 }, () => {
    db.exec("insert into t values (1)")
    db.transaction(() => db.exec("insert into t values (2)"))
    db.one("select count(*) as c from t").c
})
fn nested() = db.transaction(() => db.transaction({ isolation: "read_committed" }, () => 1))
fn failed() = db.transaction({ retries: 2 }, () => { db.exec("insert into t values (3)"); err("no") })
fn count() = db.one("select count(*) as c from t").c
"#;
    let mut it = program(&dir, src);
    assert_eq!(call(&mut it, "go"), "2");
    assert!(call(&mut it, "nested").contains("cannot change its isolation"));
    assert_eq!(call(&mut it, "failed"), "err(\"no\")");
    assert_eq!(call(&mut it, "count"), "2");
}

// ---- audit rows in transactions tried again ----------------------------------

/// Programs (each with its own connection) that each bump a counter a few
/// times at once, in transactions that read, write and add an audit row,
/// tried again when they conflict. A try rolled back takes its audit row
/// with it: each bump that counted has one row, with the value it read, and
/// the chain is whole. `setup` opens the database and makes `counter`;
/// `read` is how the counter is read.
fn audited_at_once(dir: &std::path::Path, setup: &str, opts: &str, read: &str) {
    let src = format!(
        r#"{setup}
fn bump() {{
    let r = db.transaction({opts}, () => {{
        let n = db.one("{read}")?.n
        audit("counter.bump", {{ from: n }})?
        sleep(5)
        db.exec("update counter set n = ? where id = 1", n + 1)?
        n + 1
    }})
    is_err(r) ? `failed: ${{r.error}}` : "ok"
}}
fn total() = db.one("select n from counter where id = 1").n
fn chain() = audit.verify()
fn read() = audit.query({{ action: "counter.bump", limit: 1000 }}) | map((r) => r.details.from) | sort
"#
    );
    const PROGRAMS: usize = 4;
    const EACH: usize = 5;
    let barrier = Arc::new(Barrier::new(PROGRAMS));
    let threads: Vec<_> = (0..PROGRAMS)
        .map(|_| {
            let (dir, src, barrier) = (dir.to_path_buf(), src.clone(), barrier.clone());
            std::thread::spawn(move || {
                let mut it = program(&dir, &src);
                barrier.wait();
                (0..EACH).map(|_| call(&mut it, "bump")).collect::<Vec<_>>()
            })
        })
        .collect();
    let got: Vec<String> = threads.into_iter().flat_map(|t| t.join().unwrap()).collect();
    assert!(got.iter().all(|g| g == "\"ok\""), "{}: {:?}", opts, got);
    let n = PROGRAMS * EACH;
    assert_eq!(eval(dir, &src, "total"), n.to_string(), "{}", opts);
    let chain = eval(dir, &src, "chain");
    assert!(chain.starts_with(&format!("{{ ok: true, rows: {},", n)), "{}: {}", opts, chain);
    let read: Vec<String> = (0..n).map(|i| i.to_string()).collect();
    assert_eq!(eval(dir, &src, "read"), format!("[{}]", read.join(", ")), "{}", opts);
}

#[test]
fn audit_rows_of_a_try_rolled_back_go_with_it() {
    let dir = support::temp_dir("isolation-audit-sqlite");
    let setup = "db.open(\"audited.db\")\ndb.exec(\"create table if not exists counter (id int primary key, n int not null)\")\ndb.exec(\"insert into counter values (1, 0) on conflict do nothing\")\n";
    let read = "select n from counter where id = 1";
    audited_at_once(&dir, setup, "{ isolation: \"serializable\", retries: 20 }", read);
    let Some(pg) = support::postgres("isolation-audit") else {
        eprintln!("skipping PostgreSQL: its initdb and pg_ctl were not found");
        return;
    };
    for (name, iso) in [("audit_serializable", "serializable"), ("audit_repeatable", "repeatable_read"), ("audit_committed", "read_committed")] {
        let url = pg.database(name);
        let setup = format!(
            "db.open(\"{}\")\ndb.exec(\"create table if not exists counter (id int primary key, n int not null)\")\ndb.exec(\"insert into counter values (1, 0) on conflict do nothing\")\n",
            url
        );
        // Read committed would lose updates (each reads before the other
        // writes): there the counter is read with a lock on its row.
        let read = if iso == "read_committed" { "select n from counter where id = 1 for update" } else { read };
        audited_at_once(&dir, &setup, &format!("{{ isolation: \"{}\", retries: 20 }}", iso), read);
    }
}

#[test]
#[cfg(feature = "mysql")]
fn audit_rows_of_a_try_rolled_back_go_with_it_on_mariadb() {
    let Some(my) = support::mariadb("isolation-audit") else {
        eprintln!("skipping: no MariaDB server (mariadbd, mariadb-install-db) was found");
        return;
    };
    let dir = support::temp_dir("isolation-audit-my");
    assert_eq!(eval(&dir, &format!("db.open(\"{}\")\nfn make() = db.exec(\"create database audited\").changes", my.url("mysql")), "make"), "1");
    let setup = format!(
        "db.open(\"{}\")\ndb.exec(\"create table if not exists counter (id int primary key, n int not null) engine = innodb\")\ndb.exec(\"insert ignore into counter values (1, 0)\")\n",
        my.url("audited")
    );
    audited_at_once(&dir, &setup, "{ isolation: \"serializable\", retries: 20 }", "select n from counter where id = 1");
}
