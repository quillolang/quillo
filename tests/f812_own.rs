// Quillo 0.8.12, slice E (branch feedback86-89): Quillo's own tables (`_quillo_*`) held by a
// database role the program's SQL cannot use, on PostgreSQL and MySQL/MariaDB (the user's decision
// of 2026-11-26). Quillo's statements run as that role through its definer function, each call
// signed with a key from QUILLO_SECRET; the program's role may call the functions and nothing else.

mod support;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SECRET: &str = "test secret for the tests";

/// A folder for a test, removed when the value goes.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn dir(name: &str, files: &[(&str, &str)]) -> Dir {
    let d = support::temp_dir(&format!("f812own-{}", name));
    for (f, text) in files {
        let p = d.join(f);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, text).unwrap();
    }
    Dir(d)
}

/// `quillo <args>` in `d` with `vars` (QUILLO_SECRET the tests' unless given, or removed when given
/// empty), within 120 s: whether it succeeded, and what it printed and logged.
fn quillo(d: &Path, args: &[&str], vars: &[(&str, &str)]) -> (bool, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(args).current_dir(d).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    c.env("QUILLO_SECRET", SECRET);
    for (k, v) in vars {
        if v.is_empty() {
            c.env_remove(k);
        } else {
            c.env(k, v);
        }
    }
    let mut child = c.spawn().unwrap();
    let read = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut s = Vec::new();
            let _ = r.read_to_end(&mut s);
            String::from_utf8_lossy(&s).into_owned()
        })
    };
    let (out, err) = (read(Box::new(child.stdout.take().unwrap())), read(Box::new(child.stderr.take().unwrap())));
    let end = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > end {
            let _ = child.kill();
            let _ = child.wait();
            panic!("quillo {:?} did not end within 120 s", args);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    (status.success(), out.join().unwrap() + &err.join().unwrap())
}

fn checked(d: &Path, file: &str) {
    let (ok, said) = quillo(d, &["check", file], &[]);
    assert!(ok, "{}", said);
}

/// psql on `url`: whether it succeeded, and what it said.
fn psql(pg: &support::Pg, url: &str, sql: &str) -> (bool, String) {
    let out = Command::new(pg.bin.join("psql")).args([url, "-qAt", "-v", "ON_ERROR_STOP=1", "-c", sql]).output().unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr))
}

fn psql_ok(pg: &support::Pg, url: &str, sql: &str) -> String {
    let (ok, said) = psql(pg, url, sql);
    assert!(ok, "{}: {}", sql, said);
    said.trim().to_string()
}

/// A database whose program's role `app` owns it, and Quillo's role `qrole`, a role of its own.
struct Db {
    pg: support::Pg,
    name: String,
    root: String,
    app: String,
    q: String,
}

fn database(name: &str) -> Option<Db> {
    let pg = support::postgres(&format!("f812own-{}", name))?;
    let db = format!("f812own_{}", name.replace('-', "_"));
    let root = pg.database(&db);
    psql_ok(&pg, &root, &format!("create role app login; create role qrole login; alter database {} owner to app; alter role app set log_statement = 'all'", db));
    let (app, q) = (root.replace("postgres://quillo@", "postgres://app@"), root.replace("postgres://quillo@", "postgres://qrole@"));
    Some(Db { pg, name: db, root, app, q })
}

/// The key Quillo signs its calls on database `db` with (as `own_tables::key_for` makes it).
fn key(db: &str) -> [u8; 32] {
    let base = quillo::crypto::hmac_sha256(SECRET.as_bytes(), b"quillo session");
    quillo::crypto::hmac_sha256(&base, format!("quillo own tables\0{}", db).as_bytes())
}

fn mac(k: &[u8; 32], parts: &[&str]) -> String {
    quillo::crypto::hex(&quillo::crypto::hmac_sha256(k, parts.join("\u{1}").as_bytes()))
}

const INNER: &str = "let seen = []\nserver fn hit(x: str) {\n    seen = [...seen, x]\n    return \"ok\"\n}\nserver fn hits() {\n    return seen | join(\",\")\n}\n";

/// The program: idempotent payments, a failing one, a job, a later call, an atomic step on a SQLite
/// connection, migrations, the two documented uses of Quillo's tables, and a read of one.
const APP: &str = r#"db.open("URL")
db.exec("create table if not exists paid (n int)")
db.exec("create table if not exists ticks (n int)")
service inner = "inner.qwe"
let lite = db.connect("lite.db")
lite.exec("create table if not exists t (x int)")
fn tick() {
    return db.exec("insert into ticks (n) values (1)")
}
schedule("1s", tick)
route POST /pay = idempotent(() => {
    db.exec("insert into paid (n) values (1)")
    return { paid: db.one("select count(*) as n from paid").n }
})
route POST /fail = idempotent(() => {
    db.exec("insert into paid (n) values (2)")
    return err("declined")
})
route GET /paid = db.one("select count(*) as n from paid").n
route GET /ticks = db.one("select count(*) as n from ticks").n
route GET /mine {
    let r = db.query("select * from quillo._quillo_idempotency")
    return is_err(r) ? r.error : "read"
}
route POST /migrate = len(db.migrate("migrations"))
route GET /migrations = db.query("select name from _quillo_migrations order by name")
route POST /retry = db.exec("update _quillo_later set at = 0, tries = 0 where tries < 0").changes
route POST /atomic = db.atomic([lite], () => lite.exec("insert into t (x) values (1)").changes)
route GET /go = inner.later.hit("x")
route GET /seen = inner.hits()
"#;

fn pay(port: u16, path: &str, key: &str) -> support::Answer {
    support::send(port, "POST", path, &[("Idempotency-Key", key)], "")
}

fn eventually(secs: u64, what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < end, "{}", what);
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// 0.8.11's tables, written by the program's role, move into Quillo's schema at the setup, which
/// says so; Quillo's role owns them, and nothing of Quillo's is left in the program's schema. Then:
/// a request sent again with a key of before gets its first answer; idempotent payments run once,
/// their record committed with the program's writes, and a failing one keeps nothing; a migration
/// is recorded; the documented `select name from _quillo_migrations` and `update _quillo_later
/// ... where tries < 0` work as written; a later call is delivered, a job runs, an atomic step
/// commits, each through Quillo's role; and the program's own SQL cannot read Quillo's records
/// (the user's decision of 2026-11-26; branch feedback86-89).
#[test]
fn own_setup_moves_0811s_tables_and_quillos_work_runs_as_its_role() {
    let Some(db) = database("flow") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let d = dir(
        "flow",
        &[
            ("main.qwe", &APP.replace("URL", &db.app)),
            ("inner.qwe", INNER),
            ("migrations/001_things.sql", "create table things (x int);\n"),
            ("setup.qwe", &format!("db.open(\"{}\")\nprint(db.own_setup(\"{}\"))\n", db.app, db.q)),
        ],
    );
    checked(&d, "main.qwe");
    checked(&d, "setup.qwe");
    // On 0.8.11's way: the program's role writes Quillo's tables in its own schema.
    let first;
    {
        let s = support::serve(&d.join("main.qwe"), &[]);
        let a = pay(s.port, "/pay", "old-key");
        assert_eq!(a.status, 200, "{}", a.body);
        first = a.body;
        assert_eq!(support::send(s.port, "POST", "/migrate", &[], "").body, "1");
    }
    assert_eq!(psql_ok(&db.pg, &db.root, "select tableowner from pg_tables where schemaname = 'public' and tablename = '_quillo_idempotency'"), "app");
    // The setup, as the operator runs it.
    let (ok, said) = quillo(&d, &["run", "setup.qwe"], &[]);
    assert!(ok, "{}", said);
    for line in ["moved public._quillo_idempotency (1 rows) to quillo._quillo_idempotency", "moved public._quillo_migrations (1 rows) to quillo._quillo_migrations", "made the views public._quillo_migrations (name) and public._quillo_later (at, tries)"] {
        assert!(said.contains(line), "{}\n{}", line, said);
    }
    assert_eq!(psql_ok(&db.pg, &db.root, "select tableowner from pg_tables where schemaname = 'quillo' and tablename = '_quillo_idempotency'"), "qrole");
    assert_eq!(psql_ok(&db.pg, &db.root, "select count(*) from pg_tables where schemaname = 'public' and tablename like '\\_quillo\\_%'"), "0");
    let s = support::serve(&d.join("main.qwe"), &[]);
    // The answer kept before the move, given again.
    let again = pay(s.port, "/pay", "old-key");
    assert_eq!((again.status, again.body.as_str()), (200, first.as_str()));
    assert_eq!(again.header("idempotent-replayed"), Some("true"));
    // Exactly once, with the program's writes.
    let a = pay(s.port, "/pay", "k2");
    assert_eq!(a.body, r#"{"paid":2}"#);
    let b = pay(s.port, "/pay", "k2");
    assert_eq!((b.body.as_str(), b.header("idempotent-replayed")), (r#"{"paid":2}"#, Some("true")));
    assert_eq!(support::send(s.port, "GET", "/paid", &[], "").body, "2");
    // A failure keeps nothing: neither its write nor its answer.
    let f1 = pay(s.port, "/fail", "f1");
    let f2 = pay(s.port, "/fail", "f1");
    assert_eq!(f2.header("idempotent-replayed"), None, "{} / {}", f1.body, f2.body);
    assert_eq!(support::send(s.port, "GET", "/paid", &[], "").body, "2");
    // A migration, recorded as Quillo's role, and the documented uses.
    std::fs::write(d.join("migrations/002_more.sql"), "create table more_things (x int);\n").unwrap();
    assert_eq!(support::send(s.port, "POST", "/migrate", &[], "").body, "1");
    assert_eq!(support::send(s.port, "GET", "/migrations", &[], "").body, r#"[{"name":"001_things.sql"},{"name":"002_more.sql"}]"#);
    assert_eq!(support::send(s.port, "POST", "/retry", &[], "").body, "0");
    // A later call, a job, an atomic step.
    assert_eq!(support::send(s.port, "GET", "/go", &[], "").status, 200);
    eventually(30, "the later call was not delivered", || support::send(s.port, "GET", "/seen", &[], "").body == "x");
    eventually(30, "the job did not run", || support::send(s.port, "GET", "/ticks", &[], "").body != "0");
    assert_eq!(support::send(s.port, "POST", "/atomic", &[], "").body, "1");
    // The program's own SQL reads none of Quillo's records.
    assert!(support::send(s.port, "GET", "/mine", &[], "").body.contains("permission denied"));
    // Each of those went through Quillo's role's function.
    let log = db.pg.log();
    for t in ["_quillo_idempotency", "_quillo_later", "_quillo_jobs", "_quillo_atomic", "_quillo_migrations"] {
        assert!(log.lines().any(|l| l.to_ascii_lowercase().contains("parameters:") && l.contains("'quillo'") && l.contains(t)), "{} was not written through quillo._quillo_run", t);
    }
}

/// The program's role reads and writes none of Quillo's tables or its key's pads; a call with a
/// made-up MAC is refused, and so is a MAC of Quillo's given another statement or other values; a
/// call signed as Quillo signs runs. The views give only what the documented uses need: the
/// migrations' names, and a later call's `at` and `tries`, updated but not deleted, inserted or
/// read whole (the user's decision of 2026-11-26; branch feedback86-89).
#[test]
fn the_programs_role_reads_writes_and_forges_nothing() {
    let Some(db) = database("forge") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let d = dir("forge", &[("setup.qwe", &format!("db.open(\"{}\")\nprint(db.own_setup(\"{}\"))\n", db.app, db.q))]);
    checked(&d, "setup.qwe");
    let (ok, said) = quillo(&d, &["run", "setup.qwe"], &[]);
    assert!(ok, "{}", said);
    psql_ok(&db.pg, &db.q, "create table if not exists quillo._quillo_idempotency (id text primary key, token text); insert into quillo._quillo_idempotency values ('k1', 'secret-token'), ('k2', 'other')");
    for sql in [
        "select * from quillo._quillo_idempotency",
        "insert into quillo._quillo_idempotency values ('x', 'y')",
        "update quillo._quillo_idempotency set token = 'x'",
        "select * from quillo._quillo_secret",
        "select quillo._quillo_check('x', repeat('0', 64))",
        "delete from _quillo_later",
        "insert into _quillo_later (at, tries) values (1, 1)",
        "select at from _quillo_later",
        "select sha256 from _quillo_migrations",
    ] {
        let (ok, said) = psql(&db.pg, &db.app, sql);
        assert!(!ok && (said.contains("permission denied") || said.contains("does not exist")), "{}: {}", sql, said);
    }
    assert_eq!(psql_ok(&db.pg, &db.app, "update _quillo_later set at = 0, tries = 0 where tries < 0; select count(*) from _quillo_migrations"), "0");
    let k = key(&db.name);
    let stmt = "select token from _quillo_idempotency where id = %1$L";
    let run = |stmt: &str, args: &str, m: &str| psql(&db.pg, &db.app, &format!("select r from quillo._quillo_run('quillo', '{}', '{}', 'r', '{}') r", stmt.replace('\'', "''"), args, m));
    let good = mac(&k, &["quillo", stmt, r#"["k1"]"#, "r"]);
    let (ok, said) = run(stmt, r#"["k1"]"#, &good);
    assert!(ok && said.contains("secret-token"), "{}", said);
    for (s, a, m) in [
        (stmt, r#"["k1"]"#, "0".repeat(64)),
        (stmt, r#"["k2"]"#, good.clone()),
        ("select token from _quillo_idempotency", r#"["k1"]"#, good.clone()),
        (stmt, r#"["k1"]"#, mac(&[7u8; 32], &["quillo", stmt, r#"["k1"]"#, "r"])),
        (stmt, r#"["k1"]"#, String::new()),
    ] {
        let (ok, said) = run(s, a, &m);
        assert!(!ok && said.contains("not Quillo's"), "{} {}: {}", s, a, said);
    }
}

/// Under a quillo.confirmed a taken file's SQL reaches none of Quillo's records, however written,
/// and neither does the main file's: the database refuses by privilege (the user's decision of
/// 2026-11-26; branch feedback86-89).
#[test]
fn a_taken_files_sql_reaches_no_record() {
    let Some(db) = database("taken") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let kit = "fn steal() {\n    let said = (r) => is_err(r) ? r.error : \"ran\"\n    return [said(db.query(\"select count(*) as n from quillo._quillo_idempotency\")), said(db.query(\"select table_to_xml('quillo._quillo_idempotency', true, false, '') as x\")), said(db.query(\"select * from quillo._quillo_secret\"))] | join(\"\\n\")\n}\n";
    let main = "import kit/k.qwe use steal\nprint(steal())\nlet r = db.query(\"select count(*) as n from quillo._quillo_idempotency\")\nprint(\"main: \" + (is_err(r) ? r.error : \"read\"))\n";
    let d = dir(
        "taken",
        &[("kit/k.qwe", kit), ("main.qwe", main), ("setup.qwe", &format!("db.open(\"{}\")\nprint(db.own_setup(\"{}\"))\n", db.app, db.q))],
    );
    checked(&d, "setup.qwe");
    let (ok, said) = quillo(&d, &["run", "setup.qwe"], &[]);
    assert!(ok, "{}", said);
    psql_ok(&db.pg, &db.q, "create table if not exists quillo._quillo_idempotency (id text primary key)");
    std::fs::write(d.join("quillo.confirmed"), "[\"main.qwe\"]\n").unwrap();
    checked(&d, "main.qwe");
    let (ok, out) = quillo(&d, &["run", "main.qwe"], &[("QUILLO_DB", &db.app)]);
    assert!(ok, "{}", out);
    assert_eq!(out.matches("are refused for kit/k.qwe's SQL").count() + out.matches("is refused for kit/k.qwe's SQL").count(), 3, "{}", out);
    assert!(!out.contains("\nran"), "{}", out);
    assert!(out.contains("main: db: permission denied"), "{}", out);
    assert!(!out.contains("held by reading each taken file's SQL"), "{}", out);
}

/// Without Quillo's role, 0.8.11's way holds: the program's role makes Quillo's tables in its own
/// schema, with one warning where a quillo.confirmed holds the run; one, saying so, without
/// QUILLO_SECRET too; none where nothing is held (the user's decision of 2026-11-26; branch
/// feedback86-89).
#[test]
fn without_quillos_role_0811s_way_holds_with_one_warning() {
    let Some(db) = database("none") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let main = "db.migrate(\"migrations\")\nprint(db.query(\"select name from _quillo_migrations\"))\n";
    let d = dir("none", &[("main.qwe", main), ("migrations/001_a.sql", "create table a (x int);\n")]);
    checked(&d, "main.qwe");
    let (ok, out) = quillo(&d, &["run", "main.qwe"], &[("QUILLO_DB", &db.app)]);
    assert!(ok, "{}", out);
    assert!(!out.contains("held by reading"), "{}", out);
    assert!(out.contains("001_a.sql"), "{}", out);
    assert_eq!(psql_ok(&db.pg, &db.root, "select tableowner from pg_tables where tablename = '_quillo_migrations'"), "app");
    std::fs::write(d.join("quillo.confirmed"), "[\"main.qwe\"]\nfiles = [\"migrations/\"]\n").unwrap();
    let (ok, out) = quillo(&d, &["run", "main.qwe"], &[("QUILLO_DB", &db.app)]);
    assert!(ok, "{}", out);
    assert_eq!(out.matches("held by reading each taken file's SQL, as before: no role holds them here").count(), 1, "{}", out);
    let (ok, out) = quillo(&d, &["run", "main.qwe"], &[("QUILLO_DB", &db.app), ("QUILLO_SECRET", "")]);
    assert!(ok, "{}", out);
    assert_eq!(out.matches("held by reading each taken file's SQL, as before: QUILLO_SECRET is not set").count(), 1, "{}", out);
}

/// A database set up with one QUILLO_SECRET and opened with another: `db.open` stops, saying to run
/// the setup again; with the first, it opens (the user's decision of 2026-11-26; branch
/// feedback86-89).
#[test]
fn a_changed_secret_stops_db_open() {
    let Some(db) = database("secret") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let d = dir(
        "secret",
        &[("main.qwe", &format!("db.open(\"{}\")\nprint(\"it ran on\")\n", db.app)), ("setup.qwe", &format!("db.open(\"{}\")\nprint(db.own_setup(\"{}\"))\n", db.app, db.q))],
    );
    checked(&d, "setup.qwe");
    let (ok, said) = quillo(&d, &["run", "setup.qwe"], &[]);
    assert!(ok, "{}", said);
    let (ok, out) = quillo(&d, &["run", "main.qwe"], &[]);
    assert!(ok && out.contains("it ran on"), "{}", out);
    let (ok, out) = quillo(&d, &["run", "main.qwe"], &[("QUILLO_SECRET", "another secret, not the one it was set up with")]);
    assert!(!ok && out.contains("run setup again") && !out.contains("it ran on"), "{}", out);
}

/// A tenant's own tables, on a database with a schema per tenant: Quillo's role makes the
/// tenant's schema of its own (`quillo_<tenant>`) the first time, and its records go there, owned
/// by that role (the user's decision of 2026-11-26; branch feedback86-89).
#[test]
fn a_tenants_own_tables_are_in_a_schema_of_quillos() {
    let Some(db) = database("tenant") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = format!("{}?schema={{tenant}}", db.app);
    let d = dir(
        "tenant",
        &[
            ("setup.qwe", &format!("db.open(\"{}\")\nwith_tenant(\"t0\", () => print(db.own_setup(\"{}\")))\n", url, db.q)),
            ("main.qwe", &format!("db.open(\"{}\")\nprint(with_tenant(\"t1\", () => db.migrate(\"migrations\")))\n", url)),
            ("migrations/001_a.sql", "create table a (x int);\n"),
        ],
    );
    checked(&d, "setup.qwe");
    checked(&d, "main.qwe");
    let (ok, said) = quillo(&d, &["run", "setup.qwe"], &[]);
    assert!(ok, "{}", said);
    let (ok, out) = quillo(&d, &["run", "main.qwe"], &[]);
    assert!(ok && out.contains("001_a.sql"), "{}", out);
    assert_eq!(psql_ok(&db.pg, &db.root, "select nspowner::regrole::text from pg_namespace where nspname = 'quillo_t1'"), "qrole");
    assert_eq!(psql_ok(&db.pg, &db.root, "select count(*) from quillo_t1._quillo_migrations"), "1");
    assert_eq!(psql_ok(&db.pg, &db.root, "select count(*) from pg_tables where schemaname = 't1' and tablename like '\\_quillo\\_%'"), "0");
}

/// The same on MariaDB: Quillo's tables in the database `<db>_quillo`, its procedures definer's;
/// idempotent payments run once and answer again, and the program's user reads none of them
/// (the user's decision of 2026-11-26; branch feedback86-89).
#[test]
fn on_mariadb_too() {
    let Some(m) = support::mariadb("f812own") else {
        eprintln!("skipping: no MariaDB");
        return;
    };
    let root = m.url("shop");
    let sql = |q: &str| {
        let out = Command::new("mariadb").args(["-h", "127.0.0.1", "-P", &m.port.to_string(), "-u", "root", "-e", q]).output().unwrap();
        (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr))
    };
    let (ok, said) = sql(
        "create user 'app'@'%' identified by 'app'; grant all on shop.* to 'app'@'%'; \
         create user 'q'@'%' identified by 'q'; create database shop_quillo; grant all on shop_quillo.* to 'q'@'%' with grant option; \
         grant create view, select on shop.* to 'q'@'%'",
    );
    // The tests' MariaDB runs with `--skip-grant-tables`, where no privilege holds anything.
    if !ok && said.contains("skip-grant-tables") {
        eprintln!("skipping: this MariaDB holds no privileges (--skip-grant-tables)");
        return;
    }
    assert!(ok, "{}", said);
    let (app, q) = (root.replace("root@", "app:app@"), root.replace("root@", "q:q@"));
    let main = format!(
        "db.open(\"{}\")\ndb.exec(\"create table if not exists paid (n int)\")\nroute POST /pay = idempotent(() => {{\n    db.exec(\"insert into paid (n) values (1)\")\n    return {{ paid: db.one(\"select count(*) as n from paid\").n }}\n}})\nroute GET /mine {{\n    let r = db.query(\"select * from shop_quillo._quillo_idempotency\")\n    return is_err(r) ? r.error : \"read\"\n}}\nroute GET /leak = db.query(\"select @quillo_sql as s\")\nroute GET /migrations = db.query(\"select name from _quillo_migrations\")\n",
        app
    );
    let d = dir("maria", &[("main.qwe", &main), ("setup.qwe", &format!("db.open(\"{}\")\nprint(db.own_setup(\"{}\"))\n", app, q))]);
    checked(&d, "main.qwe");
    checked(&d, "setup.qwe");
    let (ok, said) = quillo(&d, &["run", "setup.qwe"], &[]);
    assert!(ok && said.contains("made the database shop_quillo"), "{}", said);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let a = pay(s.port, "/pay", "m1");
    assert_eq!(a.body, r#"{"paid":1}"#);
    let b = pay(s.port, "/pay", "m1");
    assert_eq!((b.body.as_str(), b.header("idempotent-replayed")), (r#"{"paid":1}"#, Some("true")));
    // Verify `q_run` cleared `@quillo_sql` on the pooled connection so subsequent SQL cannot read it.
    let leak = support::send(s.port, "GET", "/leak", &[], "").body;
    assert_eq!(leak, r#"[{"s":null}]"#, "pooled @quillo_sql leaked: {}", leak);
    assert_eq!(support::send(s.port, "GET", "/migrations", &[], "").body, "[]");
    let mine = support::send(s.port, "GET", "/mine", &[], "").body;
    assert!(mine.contains("denied"), "{}", mine);
    let (ok, said) = sql("select count(*) from shop_quillo._quillo_idempotency");
    assert!(ok && said.contains('1'), "{}", said);
}
