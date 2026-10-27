//! `db` against a real PostgreSQL server. The test starts a throwaway one
//! (initdb + pg_ctl, on its own port, in a temporary folder) and stops it
//! afterwards. Skipped when PostgreSQL's server programs are not installed;
//! set QUILLO_TEST_PG_BIN to the folder holding initdb and pg_ctl.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Pg {
    bin: PathBuf,
    dir: PathBuf,
    url: String,
}

impl Drop for Pg {
    fn drop(&mut self) {
        let _ = Command::new(self.bin.join("pg_ctl")).arg("-D").arg(&self.dir).args(["-m", "immediate", "stop"]).output();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn find_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_TEST_PG_BIN") {
        return Some(PathBuf::from(p));
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(path) = std::env::var("PATH") {
        candidates.extend(std::env::split_paths(&path));
    }
    for root in ["/usr/lib/postgresql", "/opt/homebrew/opt", "/usr/local/opt"] {
        if let Ok(entries) = std::fs::read_dir(root) {
            for e in entries.flatten() {
                candidates.push(e.path().join("bin"));
            }
        }
    }
    candidates.into_iter().find(|d| d.join("initdb").exists() && d.join("pg_ctl").exists())
}

/// Starts a throwaway server; it is stopped when the value is dropped.
fn server() -> Option<Pg> {
    let bin = find_bin()?;
    let dir = std::env::temp_dir().join(format!("quillo-pg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let out = Command::new(bin.join("initdb"))
        .arg("-D")
        .arg(&dir)
        .args(["-U", "quillo", "--auth=trust", "-E", "UTF8", "--no-locale"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    assert!(out.status.success(), "initdb failed: {}", String::from_utf8_lossy(&out.stderr));
    let port = 25000 + (std::process::id() % 2000) as u16;
    // Prepared transactions, for `db.atomic`.
    let opts = format!("-p {} -k {} -c listen_addresses=127.0.0.1 -c fsync=off -c max_prepared_transactions=10", port, dir.display());
    let pg = Pg { bin, dir, url: format!("postgres://quillo@127.0.0.1:{}/postgres", port) };
    let ok = Command::new(pg.bin.join("pg_ctl"))
        .arg("-D")
        .arg(&pg.dir)
        .args(["-o", &opts, "-l"])
        .arg(pg.dir.join("log"))
        .args(["-w", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    assert!(ok.success(), "pg_ctl start failed: {}", std::fs::read_to_string(pg.dir.join("log")).unwrap_or_default());
    Some(pg)
}

/// A fresh database for one test, and its URL.
fn database(pg: &Pg, name: &str) -> String {
    let out = Command::new(pg.bin.join("psql"))
        .args([&pg.url, "-q", "-c", &format!("drop database if exists {}", name), "-c", &format!("create database {}", name)])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    pg.url.replace("/postgres", &format!("/{}", name))
}

/// One server for every scenario; it is stopped even if one fails.
#[test]
fn postgres() {
    let Some(pg) = server() else {
        eprintln!("skipping: PostgreSQL's initdb and pg_ctl were not found");
        return;
    };
    values_round_trip_through_postgres_types(&database(&pg, "types"));
    placeholders_errors_and_returning(&database(&pg, "places"));
    transactions_commit_roll_back_and_nest(&database(&pg, "tx"));
    queries_wait_without_blocking_and_transactions_are_private(&database(&pg, "served"));
    migrations_apply_once_and_roll_back_on_failure(&database(&pg, "migrated"));
    each_tenant_gets_a_schema_of_its_own(&database(&pg, "tenants"));
    several_databases_of_several_kinds(&database(&pg, "main_db"), &database(&pg, "other_db"), &database(&pg, "beta_db"));
    replicas_take_reads_and_the_primary_the_rest(&database(&pg, "primary_db"), &database(&pg, "replica_db"));
    one_transaction_over_several_databases(&pg, &database(&pg, "bank_a"), &database(&pg, "bank_b"));
}

fn psql(pg: &Pg, url: &str, sql: &[&str]) {
    let mut cmd = Command::new(pg.bin.join("psql"));
    cmd.args([url, "-q", "-v", "ON_ERROR_STOP=1"]);
    for s in sql {
        cmd.args(["-c", s]);
    }
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// Reads go to a replica (here a database of its own, whose rows differ, as
/// a lagging replica's would), unless they lock, run in a transaction, or
/// come after this request wrote: those, and writes, go to the primary.
fn replicas_take_reads_and_the_primary_the_rest(primary: &str, replica: &str) {
    for (url, x) in [(primary, "primary"), (replica, "replica")] {
        run(url, &format!("db.exec(\"create table t (x text); insert into t values ('{}')\")", x));
    }
    let got = run(
        primary,
        &format!(
            r#"db.open("{}", {{ replicas: ["{}"] }})
let read = db.one("select x from t").x
let locked = db.one("select x from t for update").x
let in_tx = db.transaction(() => db.one("select x from t").x)
db.exec("insert into t values ('written')")
let after = db.one("select x from t order by x limit 1").x
[read, locked, in_tx, after, len(db.query("select * from t"))]"#,
            primary, replica
        ),
    );
    assert_eq!(got, r#"["replica", "primary", "primary", "primary", 2]"#);
    let e = quillo::Interp::new(".").load_source(r#"db.open("app.db", { replicas: ["x.db"] })"#, "r.qwe").err().unwrap();
    assert!(e.msg.contains("not SQLite"), "{}", e.msg);
}

/// `db.atomic`: two PostgreSQL databases and a SQLite one, all of it or none;
/// a part that cannot prepare undoes the others; transactions left prepared
/// by a server that stopped are finished as decided.
fn one_transaction_over_several_databases(pg: &Pg, a: &str, b: &str) {
    let d = std::env::temp_dir().join(format!("quillo-pg-atomic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let src = format!(
        r#"db.open("{a}")
let other = db.connect("{b}")
let local = db.connect("{local}")
db.exec("create table acct (id int primary key, n int)")
other.exec("create table acct (id int primary key, n int)")
other.exec("create table once (k int unique deferrable initially deferred)")
local.exec("create table moves (n int)")
db.exec("insert into acct values (1, 100)")
other.exec("insert into acct values (1, 0)")
fn move(n) = db.atomic([db, other, local], () => {{
    db.exec("update acct set n = n - ? where id = 1", n)
    other.exec("update acct set n = n + ? where id = 1", n)
    local.exec("insert into moves values (?)", n)
    if n > 50 {{ err("too much") }} else {{ n }}
}})
fn twice() = db.atomic([db, other], () => {{
    db.exec("update acct set n = 0 where id = 1")
    other.exec("insert into once values (1), (1)")
}})
let ok = move(30)
let refused = move(60)
let unprepared = twice()
[ok, refused, unprepared, db.one("select n from acct").n, other.one("select n from acct").n, len(local.query("select * from moves")), len(db.query("select * from pg_prepared_xacts"))]"#,
        a = a,
        b = b,
        local = d.join("local.db").display()
    );
    let mut it = quillo::Interp::new(&d);
    let got = it.load_source(&src, "atomic.qwe").unwrap_or_else(|e| panic!("{}", e)).repr();
    assert!(got.starts_with(r#"[30, err("too much"), err("db: db.atomic: a database could not prepare its part:"#), "{}", got);
    assert!(got.ends_with(", 70, 30, 1, 0]"), "{}", got);
    // A server stopped part way: one transaction decided (committed), one not (rolled back).
    let (decided, undecided) = ("quillo_1000_decided", "quillo_1000_undecided");
    psql(pg, b, &["begin", "insert into acct values (2, 2)", &format!("prepare transaction '{}_1'", decided)]);
    psql(pg, b, &["begin", "insert into acct values (3, 3)", &format!("prepare transaction '{}_1'", undecided)]);
    psql(pg, a, &[&format!("insert into _quillo_atomic (gid, decision, at) values ('{}', 'commit', 1000)", decided)]);
    quillo::db::recover(&mut it);
    let after = it.load_source("[other.query(\"select id from acct order by id\") | map((r) => r.id), len(other.query(\"select * from pg_prepared_xacts\")), db.one(\"select decision from _quillo_atomic where gid = 'quillo_1000_undecided'\").decision]", "check.qwe").unwrap().repr();
    assert_eq!(after, r#"[[1, 2], 0, "abort"]"#);
}

/// `db` on one Postgres database, a connection to another, and tenants
/// whose databases are SQLite files or Postgres, as a registry says.
fn several_databases_of_several_kinds(main: &str, other: &str, beta: &str) {
    let d = std::env::temp_dir().join(format!("quillo-pg-several-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let src = format!(
        r#"db.open("{main}")
db.exec("create table if not exists notes (id serial primary key, text text)")
let other = db.connect("{other}")
other.exec("create table if not exists notes (id serial primary key, text text)")
let registry = db.connect("registry.db")
registry.exec("create table if not exists tenants (name text primary key, url text)")
registry.exec("insert or replace into tenants values ('acme', 'acme.db'), ('beta', '{beta}')")
let tenants = db.connect((t) => registry.one("select url from tenants where name = ?", t)?.url)
fn both() = [len(db.query("select * from notes")), len(other.query("select * from notes"))]
fn inner_fails() = db.transaction(() => {{
    db.exec("insert into notes (text) values ('kept')")
    let r = other.transaction(() => {{
        other.exec("insert into notes (text) values ('lost')")
        err("no")
    }})
    is_err(r) ? "inner rolled back" : "?"
}})
fn outer_fails() = db.transaction(() => {{
    other.transaction(() => other.exec("insert into notes (text) values ('committed')"))
    db.exec("insert into notes (text) values ('lost')")
    err("no")
}})
fn put(t) = with_tenant(t, () => {{
    tenants.exec("create table if not exists items (n text)")
    tenants.exec("insert into items values (?)", t)
    tenants.query("select n from items") | map((r) => r.n)
}})
"#
    );
    let mut it = quillo::Interp::new(&d);
    it.load_source(&src, "app.qwe").unwrap_or_else(|e| panic!("{}", e));
    let call = |it: &mut quillo::Interp, f: &str, args: Vec<quillo::value::Value>| -> String {
        match it.call(&quillo::value::lookup(&it.globals, f).unwrap(), args, 0) {
            Ok(v) => v.repr(),
            Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
            Err(_) => "returned".into(),
        }
    };
    assert_eq!(call(&mut it, "both", vec![]), "[0, 0]");
    // Each connection's transaction is its own: the inner one rolls back alone.
    assert_eq!(call(&mut it, "inner_fails", vec![]), r#""inner rolled back""#);
    assert_eq!(call(&mut it, "both", vec![]), "[1, 0]");
    // The other one committed before the outer one rolled back.
    assert_eq!(call(&mut it, "outer_fails", vec![]), r#"err("no")"#);
    assert_eq!(call(&mut it, "both", vec![]), "[1, 1]");
    // A tenant on SQLite, another on Postgres, through one connection.
    assert_eq!(call(&mut it, "put", vec![quillo::value::Value::str("acme")]), r#"["acme"]"#);
    assert_eq!(call(&mut it, "put", vec![quillo::value::Value::str("beta")]), r#"["beta"]"#);
    assert!(d.join("acme.db").is_file());
    assert_eq!(run(beta, "db.query(\"select n from items\") | map((r) => r.n)"), r#"["beta"]"#);
}

fn each_tenant_gets_a_schema_of_its_own(url: &str) {
    let d = std::env::temp_dir().join(format!("quillo-pg-tenants-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_notes.sql"), "create table notes (id serial primary key, text text not null);").unwrap();
    let src = format!(
        r#"tenant_from((request) => request.headers["x-tenant"])
db.open("{url}?schema=t_{{tenant}}&pool_size=2")
db.migrate()
route POST /signup/:name = with_tenant(name, () => db.query("select 1 as x") | len)
route POST /notes {{ json: {{ text: str }} }} {{
    db.exec("insert into notes (text) values (?)", text)
    "ok"
}}
route GET /notes = db.query("select text from notes") | map((n) => n.text)
"#
    );
    let mut it = quillo::Interp::new(&d);
    it.load_source(&src, "app.qwe").unwrap_or_else(|e| panic!("{}", e));
    let send = |it: &mut quillo::Interp, method: &str, tenant: &str, body: &str| {
        let headers = vec![
            ("Host".to_string(), "x".to_string()),
            ("X-Tenant".to_string(), tenant.to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];
        let url = if method == "SIGNUP" { format!("/signup/{}", tenant) } else { "/notes".into() };
        let method = if method == "SIGNUP" { "POST" } else { method };
        let req = quillo::server::HttpRequest { method: method.into(), url, headers, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() };
        it.handle(&req).text()
    };
    // Only the program makes a tenant: a made-up one gets nothing.
    assert_eq!(send(&mut it, "GET", "ghost", ""), "internal error\n");
    assert_eq!(send(&mut it, "SIGNUP", "acme", ""), "1");
    assert_eq!(send(&mut it, "SIGNUP", "beta", ""), "1");
    assert_eq!(send(&mut it, "POST", "acme", r#"{"text":"acme's"}"#), "ok");
    assert_eq!(send(&mut it, "POST", "beta", r#"{"text":"beta's"}"#), "ok");
    assert_eq!(send(&mut it, "GET", "acme", ""), r#"["acme's"]"#);
    assert_eq!(send(&mut it, "GET", "beta", ""), r#"["beta's"]"#);
    assert_eq!(
        run(url, "db.query(\"select table_schema from information_schema.tables where table_name = 'notes' order by 1\") | map((r) => r.table_schema)"),
        "[\"t_acme\", \"t_beta\"]"
    );
    let _ = std::fs::remove_dir_all(&d);
}

fn migrations_apply_once_and_roll_back_on_failure(url: &str) {
    let d = std::env::temp_dir().join(format!("quillo-pg-migrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let m = d.join("migrations");
    std::fs::create_dir_all(&m).unwrap();
    std::fs::write(m.join("001_todos.sql"), "create table todos (id serial primary key, text text not null);").unwrap();
    std::fs::write(m.join("002_seed.sql"), "insert into todos (text) values ('first');").unwrap();
    let migrate = format!("db.migrate(\"{}\")", m.display());
    assert_eq!(run(url, &migrate), "[\"001_todos.sql\", \"002_seed.sql\"]");
    assert_eq!(run(url, &migrate), "[]");
    std::fs::write(m.join("003_bad.sql"), "create table tags (id int);\ninsert into nowhere values (1);").unwrap();
    let src = format!("db.open(\"{}\")\n{}", url, migrate);
    let e = quillo::Interp::new(".").load_source(&src, "pg.qwe").err().expect("a failing migration stops loading").to_string();
    assert!(e.contains("003_bad.sql") && e.contains("nowhere"), "{}", e);
    assert_eq!(
        run(url, "[db.query(\"select text from todos\"), db.one(\"select to_regclass('tags') as t\").t, len(db.query(\"select * from _quillo_migrations\"))]"),
        "[[{ text: \"first\" }], null, 2]"
    );
    let _ = std::fs::remove_dir_all(&d);
}

fn run(url: &str, src: &str) -> String {
    let src = format!("db.open(\"{}\")\n{}", url, src);
    match quillo::Interp::new(".").load_source(&src, "pg.qwe") {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

fn values_round_trip_through_postgres_types(url: &str) {
    let r = run(
        url,
        r#"
db.exec("create extension if not exists pgcrypto")
db.exec("create table t (
    id serial primary key, small int2, n int4, big int8, f float8, price numeric(10, 2),
    ok boolean, name text, code varchar(10), data jsonb, raw json, uid uuid,
    day date, at timestamptz, plain timestamp, tags text[], nums int4[]
)")
let a = db.exec("insert into t (small, n, big, f, price, ok, name, code, data, raw, uid, day, at, plain, tags, nums)
    values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    3, 42, 9000000000, 2.5, 19.99, true, "Ada", "x1", { k: [1, null] }, [1, 2],
    "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11", "2024-02-29", "2024-02-29 10:30:00+02", "1999-12-31 23:59:59.5",
    ["a", "b c", "q\"uote"], [1, 2, 3])
let row = db.one("select small, n, big, f, price, ok, name, code, data, raw, uid, day, at, plain, tags, nums from t")
[a, row, db.one("select null::int as nothing, 10.5::numeric as half, 7::numeric as whole")]
"#,
    );
    assert_eq!(
        r,
        "[{ changes: 1, last_id: null }, { small: 3, n: 42, big: 9000000000, f: 2.5, price: 19.99, ok: true, name: \"Ada\", \
         code: \"x1\", data: { k: [1, null] }, raw: [1, 2], uid: \"a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11\", day: \"2024-02-29\", \
         at: \"2024-02-29T08:30:00Z\", plain: \"1999-12-31T23:59:59.5\", tags: [\"a\", \"b c\", \"q\\\"uote\"], nums: [1, 2, 3] }, \
         { nothing: null, half: 10.5, whole: 7 }]"
    );
}

fn placeholders_errors_and_returning(url: &str) {
    let r = run(
        url,
        r#"
db.exec("create table users (id serial primary key, name text not null, age int);
         create table posts (id serial primary key, user_id int references users(id))")
let ada = db.one("insert into users (name, age) values (:name, :age) returning id", { name: "Ada", age: 36 })
db.exec("insert into users (name, age) values ($1, $2)", "Bob", 7)
let bad = db.query("select * from nope")
let count = db.query("select ? as a", 1, 2)
let fk = db.exec("insert into posts (user_id) values (?)", 99)
let strict = db.exec("insert into users (name, age) values (?, ?)", "C", "not a number")
let missing = db.one("select :a::int + :b::int as s", { a: 1 })
let extra = db.one("select :a::int as s", { a: 1, z: 2 })
let has = db.query("select k from (values ('{\"a\": 1}'::jsonb)) as v(k) where k ? 'a' and 1 = ?", 1)
[ada, db.query("select name from users where age > ? order by id", 1) | map(u => u.name),
 bad.error, count.error, is_err(fk), is_err(strict), db.one("select 1 where false"), missing.error, extra, has]
"#,
    );
    assert!(r.starts_with("[{ id: 1 }, [\"Ada\", \"Bob\"], \"db: relation \\\"nope\\\" does not exist\", \"db: the query has 1 parameter, got 2\", true, true, null, \"db: no value for :b\", { s: 1 }, [{ k: { a: 1 } }]]"), "{}", r);
}

fn transactions_commit_roll_back_and_nest(url: &str) {
    let r = run(
        url,
        r#"
db.exec("create table t (n int)")
db.transaction(() => db.exec("insert into t values (1)"))
db.transaction(() => {
    db.exec("insert into t values (2)")
    err("changed my mind")
})
db.transaction(() => {
    db.exec("insert into t values (3)")
    db.transaction(() => {
        db.exec("insert into t values (4)")
        err("inner only")
    })
    db.exec("insert into t values (5)")
})
db.query("select n from t order by n") | map(r => r.n)
"#,
    );
    assert_eq!(r, "[1, 3, 5]");
}

// ---- through the server ----------------------------------------------------

struct Running(std::process::Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(file: &Path) -> (Running, u16) {
    let port = 26000 + (std::process::id() % 2000) as u16;
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string(), "--workers", "4"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let child = Running(child);
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    (child, port)
}

fn get(port: u16, path: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "GET {} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n", path).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default()
}

fn queries_wait_without_blocking_and_transactions_are_private(url: &str) {
    let dir = std::env::temp_dir().join(format!("quillo-pg-app-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let app = dir.join("app.qwe");
    std::fs::write(
        &app,
        format!(
            r#"db.open("{url}")
db.exec("create table if not exists t (n int)")
route GET /slow = db.one("select true as slept from pg_sleep(0.4)")
route GET /write = db.transaction(() => {{
    db.exec("insert into t values (1)")
    db.query("select pg_sleep(0.6)")
    "written"
}})
route GET /count = db.one("select count(*) as c from t").c
"#
        ),
    )
    .unwrap();
    let (_server, port) = start(&app);

    // Four 0.4 s queries at once take about 0.4 s, not 1.6 s.
    let t0 = Instant::now();
    let hs: Vec<_> = (0..4).map(|_| std::thread::spawn(move || get(port, "/slow"))).collect();
    for h in hs {
        assert_eq!(h.join().unwrap(), r#"{"slept":true}"#);
    }
    let took = t0.elapsed();
    assert!(took < Duration::from_millis(1200), "took {:?}", took);

    // Requests share a few connections; while those wait on slow queries,
    // a quick one goes to another rather than queueing behind them.
    let hs: Vec<_> = (0..4).map(|_| std::thread::spawn(move || get(port, "/slow"))).collect();
    std::thread::sleep(Duration::from_millis(100));
    let t0 = Instant::now();
    assert_eq!(get(port, "/count"), "0");
    let took = t0.elapsed();
    assert!(took < Duration::from_millis(200), "took {:?}", took);
    for h in hs {
        assert_eq!(h.join().unwrap(), r#"{"slept":true}"#);
    }

    // While a transaction is open, other requests do not see its row.
    let writer = std::thread::spawn(move || get(port, "/write"));
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(get(port, "/count"), "0");
    assert_eq!(writer.join().unwrap(), "written");
    assert_eq!(get(port, "/count"), "1");

    let _ = std::fs::remove_dir_all(&dir);
}
