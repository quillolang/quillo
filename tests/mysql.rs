//! MySQL and MariaDB (src/my.rs), against a server of its own: a fresh
//! data folder in a temporary place, on its own port, stopped at the end.
//! Skipped when no MariaDB or MySQL server is found (set QUILLO_TEST_MYSQL_BIN
//! to the folder holding mariadbd or mysqld, and mariadb-install-db).
#![cfg(feature = "mysql")]

use quillo::Interp;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Server {
    child: Child,
    dir: PathBuf,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn bin() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = std::env::var("QUILLO_TEST_MYSQL_BIN")
        .map(|d| vec![PathBuf::from(d)])
        .unwrap_or_else(|_| vec!["/opt/homebrew/bin".into(), "/usr/local/bin".into(), "/usr/bin".into(), "/usr/sbin".into()]);
    candidates.into_iter().find(|d| d.join("mariadbd").exists() && d.join("mariadb-install-db").exists())
}

fn server() -> Option<Server> {
    let bin = bin()?;
    let port = std::net::TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok()?.port();
    let dir = std::env::temp_dir().join(format!("quillo-my-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    let ok = Command::new(bin.join("mariadb-install-db"))
        .arg(format!("--datadir={}", dir.join("data").display()))
        .args(["--auth-root-authentication-method=normal", "--skip-test-db"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    assert!(ok.success(), "mariadb-install-db failed");
    let child = Command::new(bin.join("mariadbd"))
        .arg("--no-defaults")
        .arg(format!("--datadir={}", dir.join("data").display()))
        .arg(format!("--port={}", port))
        .arg("--bind-address=127.0.0.1")
        .arg(format!("--socket={}", dir.join("s").display()))
        .arg(format!("--pid-file={}", dir.join("pid").display()))
        .arg(format!("--log-error={}", dir.join("err.log").display()))
        .arg("--skip-grant-tables")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let s = Server { child, dir, port };
    let deadline = Instant::now() + Duration::from_secs(30);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "mariadbd did not start: {}", std::fs::read_to_string(s.dir.join("err.log")).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(50));
    }
    Some(s)
}

fn eval(it: &mut Interp, e: &str) -> String {
    it.load_source(&format!("fn __t() {{\n{}\n}}\n", e), "case.qwe").unwrap_or_else(|e| panic!("{}", e));
    match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    }
}

fn program(dir: &std::path::Path, src: &str) -> Interp {
    let mut it = Interp::new(dir);
    it.load_source(src, "app.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    it
}

/// One server for every scenario.
#[test]
fn mysql() {
    let Some(s) = server() else {
        eprintln!("skipping: no MariaDB server (mariadbd, mariadb-install-db) was found");
        return;
    };
    let url = |db: &str| format!("mysql://root@127.0.0.1:{}/{}", s.port, db);
    let work = s.dir.join("work");
    std::fs::create_dir_all(work.join("migrations")).unwrap();

    // A database of its own, made when the program makes a tenant: here, by hand.
    let mut it = program(&work, &format!("db.open(\"{}\")\n", url("mysql")));
    assert_eq!(eval(&mut it, "db.exec(\"create database app\").changes"), "1");

    let mut it = program(&work, &format!("db.open(\"{}\")\n", url("app")));
    // Values and types, both ways.
    eval(&mut it, "db.exec(\"create table things (id int auto_increment primary key, name varchar(50), n bigint, price decimal(10,2), whole decimal(10,0), ok tinyint(1), score double, born date, at datetime, note text, data json)\")");
    assert_eq!(
        eval(&mut it, r#"db.exec("insert into things (name, n, price, whole, ok, score, born, at, note, data) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)", "Ada", 9007199254740993, 12.5, 7, true, 0.25, "1815-12-10", "2026-10-26 17:00:01", null, { tags: ["x"] })"#),
        "{ changes: 1, last_id: 1 }"
    );
    let row = eval(&mut it, "db.one(\"select name, n, price, whole, ok, score, born, at, note from things\")");
    assert_eq!(row, r#"{ name: "Ada", n: 9007199254740993, price: 12.50m, whole: 7, ok: true, score: 0.25, born: "1815-12-10", at: "2026-10-26 17:00:01", note: null }"#);
    // DECIMAL is exact: decimals both ways, the scale kept; an int when it
    // has none (`sum` of ints).
    eval(&mut it, "db.exec(\"create table money (id int auto_increment primary key, amount decimal(30,4), n int)\")");
    eval(&mut it, "db.exec(\"insert into money (amount, n) values (?, ?), (?, ?)\", 0.1m, 3, 12345678901234567890.1234m, -4)");
    assert_eq!(
        eval(&mut it, "[db.query(\"select amount from money order by id\") | map((r) => r.amount), db.one(\"select sum(amount) as s, sum(n) as n from money\"), db.one(\"select cast(? as decimal(10,2)) * 3 as t\", 0.10m).t]"),
        "[[0.1000m, 12345678901234567890.1234m], { s: 12345678901234567890.2234m, n: -1 }, 0.30m]"
    );
    // MariaDB keeps JSON as text; MySQL gives it back as a value.
    let data = eval(&mut it, "db.one(\"select data from things\").data");
    assert!(data == r#""{\"tags\":[\"x\"]}""# || data == r#"{ tags: ["x"] }"#, "{}", data);
    // Placeholders: `?`, and `:name` with a map.
    assert_eq!(eval(&mut it, r#"db.one("select name from things where id = :id and name = :name", { id: 1, name: "Ada" }).name"#), r#""Ada""#);
    assert_eq!(eval(&mut it, r#"db.one("select ':no' as s, ? + ? as sum", 2, 3)"#), r#"{ s: ":no", sum: 5 }"#);
    // Errors are values.
    assert!(eval(&mut it, "db.query(\"select * from nowhere\")").starts_with("err(\"db: "));
    // Several statements without parameters, as a schema has.
    eval(&mut it, "db.exec(\"create table a (x int); create table b (y int)\")");
    assert_eq!(eval(&mut it, "len(db.query(\"select * from a\")) + len(db.query(\"select * from b\"))"), "0");

    // Transactions: commit, roll back, nest as savepoints.
    eval(&mut it, "db.exec(\"create table t (v int)\")");
    assert_eq!(eval(&mut it, "db.transaction(() => db.exec(\"insert into t values (1)\").changes)"), "1");
    assert_eq!(eval(&mut it, "db.transaction(() => { db.exec(\"insert into t values (2)\")\nerr(\"no\") })"), r#"err("no")"#);
    assert_eq!(
        eval(&mut it, "db.transaction(() => {\nlet r = db.transaction(() => { db.exec(\"insert into t values (3)\")\nerr(\"inner\") })\ndb.exec(\"insert into t values (4)\")\nr\n})"),
        r#"err("inner")"#
    );
    assert_eq!(eval(&mut it, "db.transaction(() => {\ndb.transaction(() => db.exec(\"insert into t values (5)\"))\ndb.exec(\"insert into t values (6)\")\n})\ndb.query(\"select v from t order by v\") | map((r) => r.v)"), "[1, 5, 6]");

    // Migrations: once each, in order.
    std::fs::write(work.join("migrations/001_users.sql"), "create table users (id int primary key, email varchar(100))").unwrap();
    std::fs::write(work.join("migrations/002_more.sql"), "alter table users add column name varchar(50)").unwrap();
    assert_eq!(eval(&mut it, "db.migrate()"), r#"["001_users.sql", "002_more.sql"]"#);
    assert_eq!(eval(&mut it, "db.migrate()"), "[]");
    assert_eq!(eval(&mut it, "db.exec(\"insert into users values (1, 'a@x', 'A')\").changes"), "1");

    // A database per tenant, made when the program makes the tenant.
    let mut it = program(&work, &format!("db.open(\"{}\")\n", url("t_{tenant}")));
    assert_eq!(eval(&mut it, "with_tenant(\"acme\", () => db.exec(\"create table n (v int)\").changes)"), "0");
    assert_eq!(eval(&mut it, "with_tenant(\"beta\", () => db.exec(\"create table n (v int)\").changes)"), "0");
    assert_eq!(eval(&mut it, "db.tenants()"), r#"["acme", "beta"]"#);
    assert_eq!(eval(&mut it, "[db.tenant_exists(\"acme\"), db.tenant_exists(\"ghost\")]"), "[true, false]");

    // Beside SQLite in one program, each with its own transactions.
    let src = format!("db.open(\"local.db\")\ndb.exec(\"create table if not exists n (v int)\")\nlet my = db.connect(\"{}\")\nmy.exec(\"create table if not exists n (v int)\")\n", url("app"));
    let mut it = program(&work, &src);
    assert_eq!(
        eval(&mut it, "db.transaction(() => {\nlet r = my.transaction(() => { my.exec(\"insert into n values (1)\")\nerr(\"no\") })\ndb.exec(\"insert into n values (2)\")\nr\n})\n[len(db.query(\"select * from n\")), len(my.query(\"select * from n\"))]"),
        "[0, 0]"
    );
    assert_eq!(eval(&mut it, "my.exec(\"insert into n values (3)\")\n[len(db.query(\"select * from n\")), len(my.query(\"select * from n\"))]"), "[0, 1]");
    // One transaction over two MariaDB databases and SQLite (XA): all, or none.
    let mut it = program(&work, &format!("db.open(\"{}\")\n", url("mysql")));
    eval(&mut it, "db.exec(\"create database bank_a\")\ndb.exec(\"create database bank_b\")");
    let src = format!(
        "db.open(\"{}\")\nlet b = db.connect(\"{}\")\nlet local = db.connect(\"xa.db\")\n\
         db.exec(\"create table acct (id int primary key, n int)\")\nb.exec(\"create table acct (id int primary key, n int)\")\n\
         local.exec(\"create table if not exists moves (n int)\")\n\
         db.exec(\"insert into acct values (1, 100)\")\nb.exec(\"insert into acct values (1, 0)\")\n\
         fn move(n) = db.atomic([db, b, local], () => {{\n db.exec(\"update acct set n = n - ? where id = 1\", n)\n b.exec(\"update acct set n = n + ? where id = 1\", n)\n local.exec(\"insert into moves values (?)\", n)\n n > 50 ? err(\"too much\") : n\n}})\n",
        url("bank_a"),
        url("bank_b")
    );
    let mut it = program(&work, &src);
    assert_eq!(eval(&mut it, "[move(30), move(60)]"), r#"[30, err("too much")]"#);
    assert_eq!(eval(&mut it, "[db.one(\"select n from acct\").n, b.one(\"select n from acct\").n, len(local.query(\"select * from moves\")), len(db.query(\"xa recover\"))]"), "[70, 30, 1, 0]");
}
