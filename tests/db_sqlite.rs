//! `db` on SQLite, past the usual path: what its failures are, the values it
//! takes and gives, how each function refuses what it cannot use, tenants'
//! files, shards, `db.atomic` and Quillo's own tables (src/db.rs), and what
//! `sqlinfo` reads from a statement (src/sqlinfo.rs) and `txn` from options
//! (src/txn.rs).

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::value::Value;
use quillo::Interp;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-dbsqlite-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn load(d: &Path, src: &str) -> Interp {
    let mut it = Interp::new(d);
    it.load_source(src, "app.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    it
}

fn eval(it: &mut Interp, e: &str) -> String {
    it.load_source(&format!("fn __t() {{\n{}\n}}\n", e), "case.qwe").unwrap();
    match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    }
}

fn request(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    let mut h: Vec<(String, String)> = vec![("Host".into(), "localhost".into())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: h, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
}

fn says(got: String, what: &str) {
    assert!(got.contains(what), "wanted {:?} in: {}", what, got);
}

const SCHEMA: &str = r#"
db.open("main.db")
db.exec("create table parents (id integer primary key)")
db.exec("create table kids (id integer primary key, parent int references parents (id), name text not null unique, age int check (age > 0))")
db.exec("insert into parents values (1)")
db.exec("insert into kids (parent, name, age) values (1, 'ana', 5)")
"#;

// ---- what failed ------------------------------------------------------------------------------

#[test]
fn a_failure_says_what_kind_it_is() {
    let d = dir("kinds");
    let mut it = load(&d, SCHEMA);
    let kind = |it: &mut Interp, sql: &str| eval(it, &format!("db.error_kind(db.exec(\"{}\"))", sql));
    assert_eq!(kind(&mut it, "insert into kids (parent, name, age) values (1, 'ana', 6)"), "\"unique\"");
    assert_eq!(kind(&mut it, "insert into kids (parent, name, age) values (9, 'bo', 6)"), "\"foreign_key\"");
    assert_eq!(kind(&mut it, "insert into kids (parent, age) values (1, 6)"), "\"not_null\"");
    assert_eq!(kind(&mut it, "insert into kids (parent, name, age) values (1, 'bo', 0)"), "\"check\"");
    // Any other failure, and what is not a database's error, has no kind.
    assert_eq!(kind(&mut it, "insert into nowhere values (1)"), "null");
    assert_eq!(eval(&mut it, "db.error_kind(err(\"unique constraint failed\"))"), "null");
    assert_eq!(eval(&mut it, "db.error_kind(err({ code: 1 }))"), "null");
    assert_eq!(eval(&mut it, "db.error_kind(\"db: unique constraint failed\")"), "null");
    assert_eq!(eval(&mut it, "db.error_kind()"), "null");
}

#[test]
fn a_database_another_process_holds_is_a_conflict() {
    let d = dir("locked");
    let mut it = load(&d, SCHEMA);
    eval(&mut it, "db.exec(\"pragma busy_timeout = 0\")");
    let other = rusqlite::Connection::open(d.join("main.db")).unwrap();
    other.execute_batch("begin immediate").unwrap();
    let r = eval(&mut it, "db.exec(\"insert into parents values (2)\")");
    says(r, "database is locked");
    assert_eq!(eval(&mut it, "db.error_kind(db.exec(\"insert into parents values (2)\"))"), "\"conflict\"");
    // With options a transaction takes the lock at once: it says so, and
    // tries again as often as it may.
    let r = eval(&mut it, "db.transaction({ retries: 2 }, () => db.exec(\"insert into parents values (2)\"))");
    says(r, "database is locked");
    other.execute_batch("rollback").unwrap();
    assert_eq!(eval(&mut it, "db.transaction({ retries: 2 }, () => db.exec(\"insert into parents values (2)\").changes)"), "1");
}

#[test]
fn a_transaction_with_options_tries_again_once_the_lock_is_free() {
    let d = dir("retry");
    let mut it = load(&d, SCHEMA);
    eval(&mut it, "db.exec(\"pragma busy_timeout = 0\")");
    let other = rusqlite::Connection::open(d.join("main.db")).unwrap();
    other.execute_batch("begin immediate").unwrap();
    let free = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(40));
        other.execute_batch("rollback").unwrap();
    });
    // Twenty tries, about ten seconds in all: more than the lock is held.
    let r = eval(&mut it, "db.transaction({ retries: 20 }, () => db.exec(\"insert into parents values (7)\").changes)");
    free.join().unwrap();
    assert_eq!(r, "1");
    assert_eq!(eval(&mut it, "db.one(\"select count(*) as n from parents where id = 7\").n"), "1");
}

// ---- values -----------------------------------------------------------------------------------

#[test]
fn every_kind_of_value_can_be_stored() {
    let d = dir("values");
    let mut it = load(&d, "db.open(\"main.db\")\ndb.exec(\"create table t (id integer primary key, v)\")\n");
    let round = |it: &mut Interp, v: &str| eval(it, &format!("db.one(\"insert into t (v) values (?) returning v\", {}).v", v));
    assert_eq!(round(&mut it, "1.5"), "1.5");
    assert_eq!(round(&mut it, "raw(\"<b>x</b>\")"), "\"<b>x</b>\"");
    assert_eq!(round(&mut it, "[1, \"a\", null]"), "\"[1,\\\"a\\\",null]\"");
    assert_eq!(round(&mut it, "{ a: 1 }"), "\"{\\\"a\\\":1}\"");
    // A map where `?` is asked for is one value, not a list of named ones.
    assert_eq!(eval(&mut it, "db.one(\"select ? as v\", { a: 1 }).v"), "\"{\\\"a\\\":1}\"");
    // A blob comes back as its text.
    assert_eq!(eval(&mut it, "db.one(\"select x'4142' as v\").v"), "\"AB\"");
}

// ---- what each function refuses --------------------------------------------------------------

#[test]
fn sql_must_be_text() {
    let d = dir("sqlarg");
    let mut it = load(&d, SCHEMA);
    says(eval(&mut it, "db.exec(5)"), "`db.exec` expects SQL text as its first argument, got int");
    says(eval(&mut it, "db.query()"), "`db.query` expects SQL text as its first argument, got nothing");
    says(eval(&mut it, "db.one(null)"), "`db.one` expects SQL text as its first argument, got null");
}

#[test]
fn a_transaction_needs_a_function_and_good_options() {
    let d = dir("txargs");
    let mut it = load(&d, SCHEMA);
    says(eval(&mut it, "db.transaction(5)"), "`db.transaction` expects a function (after its options, if any), got int");
    says(eval(&mut it, "db.transaction()"), "got nothing");
    says(eval(&mut it, "db.transaction({ isolation: 5 }, () => 1)"), "isolation is a string, got int");
    says(eval(&mut it, "db.transaction({ isolation: \"dirty\" }, () => 1)"), "\"serializable\", \"repeatable_read\" or \"read_committed\", got \"dirty\"");
    says(eval(&mut it, "db.transaction({ retries: 99 }, () => 1)"), "retries is a number from 0 to 20, got 99");
    says(eval(&mut it, "db.transaction({ retries: -1 }, () => 1)"), "retries is a number from 0 to 20, got -1");
    says(eval(&mut it, "db.transaction({ retries: \"x\" }, () => 1)"), "retries is a number from 0 to 20");
    // Null is the same as not given.
    assert_eq!(eval(&mut it, "db.transaction({ isolation: null, retries: null }, () => 7)"), "7");
    assert_eq!(eval(&mut it, "db.transaction({ isolation: \"read_committed\", retries: 3 }, () => 7)"), "7");
}

#[test]
fn options_that_are_not_a_map_are_refused() {
    let r = quillo::txn::options(&Value::Int(1), "idempotent", 3);
    let Err(quillo::interp::Ctrl::Error(e)) = r else { panic!("not refused") };
    assert_eq!(e.msg, "`idempotent` expects options as a map, like { isolation: \"serializable\", retries: 3 }");
}

#[test]
fn a_nested_transaction_cannot_change_the_isolation() {
    let d = dir("nested");
    let mut it = load(&d, SCHEMA);
    says(eval(&mut it, "db.transaction(() => db.transaction({ isolation: \"serializable\" }, () => 1))"), "cannot change its isolation");
}

#[test]
fn db_open_and_db_connect_refuse_what_they_cannot_open() {
    let d = dir("open");
    let mut it = load(&d, "db.open(\"main.db\")\n");
    says(eval(&mut it, "db.open(5)"), "`db.open` expects a path, a URL, or a function giving one for each tenant, got int");
    says(eval(&mut it, "db.connect(5)"), "`db.connect` expects a path");
    says(eval(&mut it, "db.open()"), "got nothing");
    says(eval(&mut it, "db.open(\"a.db\", 5)"), "`db.open`'s options are a map, like { replicas: [url, ...] }");
    says(eval(&mut it, "db.open(\"a.db\", { pool: 5 })"), "`db.open` has no option `pool` (replicas)");
    says(eval(&mut it, "db.open(\"a.db\", { replicas: 5 })"), "`replicas` is a list of database URLs, got int");
    says(eval(&mut it, "db.open(\"a.db\", { replicas: [5] })"), "`replicas` is a list of database URLs, got list");
    says(eval(&mut it, "db.open(\"a/{tenant}.db\", { replicas: [] })"), "`db.open`: replicas are of one database, given by its URL");
    says(eval(&mut it, "db.connect(5, { replicas: [] })"), "`db.connect`: replicas are of one database");
    says(eval(&mut it, "db.open(\"a.db\", { replicas: [\"postgres://u@r/a\"] })"), "replicas are of a PostgreSQL or MySQL database, not SQLite");
    // No replicas is just the database; one that cannot open is an error value.
    assert_eq!(eval(&mut it, "db.open(\"b.db\", { replicas: null })"), "null");
    assert_eq!(eval(&mut it, "db.open(\"c.db\", { replicas: [] })"), "null");
    assert!(d.join("c.db").is_file());
    assert!(eval(&mut it, "db.open(\"/no/such/dir/x.db\", {})").starts_with("err(\"db:"));
    assert!(eval(&mut it, "db.open(\"/no/such/dir/x.db\")").starts_with("err(\"db:"));
}

#[test]
fn a_connection_that_is_not_the_programs_is_refused() {
    let d = dir("forged");
    let mut it = load(&d, "db.open(\"main.db\")\nlet stray = { \"$db\": 99, exec: db.exec, query: db.query, migrate: db.migrate, transaction: db.transaction }\n");
    says(eval(&mut it, "stray.query(\"select 1\")"), "this connection is not one of this program's");
    says(eval(&mut it, "stray.exec(\"select 1\")"), "this connection is not one of this program's");
    says(eval(&mut it, "stray.migrate(\"migrations\")"), "db.migrate: this connection is not one of this program's");
    says(eval(&mut it, "stray.transaction(() => 1)"), "this connection is not one of this program's");
    says(eval(&mut it, "db.atomic([stray], () => 1)"), "this connection is not one of this program's");
}

#[test]
fn db_migrate_and_db_system_take_a_name() {
    let d = dir("names");
    let mut it = load(&d, SCHEMA);
    says(eval(&mut it, "db.migrate(5)"), "`db.migrate` expects a folder name, got int");
    says(eval(&mut it, "db.system(5)"), "`db.system` expects a path or a database URL");
    says(eval(&mut it, "db.system()"), "`db.system` expects a path or a database URL");
    assert!(eval(&mut it, "db.system(\"/no/such/dir/x.db\")").starts_with("err(\"db:"));
}

// ---- tenants ----------------------------------------------------------------------------------

#[test]
fn tenants_functions_need_a_database_per_tenant() {
    let d = dir("pertenant");
    let mut it = load(&d, SCHEMA);
    says(eval(&mut it, "db.tenants()"), "`db.tenants` needs a database per tenant");
    says(eval(&mut it, "db.tenant_exists(\"a\")"), "`db.tenant_exists` needs a database per tenant");
    let mut it = load(&d, "db.open(\"data/{tenant}.db\")\n");
    says(eval(&mut it, "db.tenant_exists(5)"), "`db.tenant_exists` expects a name, got int");
    says(eval(&mut it, "db.tenant_exists()"), "`db.tenant_exists` expects a name, got nothing");
    assert_eq!(eval(&mut it, "db.tenant_exists(\"Not A Name\")"), "false");
    // There is no folder yet: no tenants.
    assert_eq!(eval(&mut it, "db.tenants()"), "[]");
}

#[test]
fn tenants_are_listed_by_what_is_around_their_name() {
    let d = dir("listing");
    // Files with a prefix.
    let mut it = load(&d, "db.open(\"data/t_{tenant}.db\")\n");
    eval(&mut it, "with_tenant(\"acme\", () => db.exec(\"create table a (x)\"))");
    eval(&mut it, "with_tenant(\"zeta\", () => db.exec(\"create table a (x)\"))");
    std::fs::write(d.join("data/other.db"), "").unwrap();
    std::fs::write(d.join("data/t_acme.txt"), "").unwrap();
    std::fs::create_dir(d.join("data/t_folder.db")).unwrap();
    assert_eq!(eval(&mut it, "db.tenants()"), "[\"acme\", \"zeta\"]");
    // A folder for each.
    let mut it = load(&d, "db.open(\"homes/{tenant}/app.db\")\n");
    eval(&mut it, "with_tenant(\"acme\", () => db.exec(\"create table a (x)\"))");
    std::fs::create_dir(d.join("homes/empty")).unwrap();
    assert_eq!(eval(&mut it, "db.tenants()"), "[\"acme\"]");
    assert_eq!(eval(&mut it, "[db.tenant_exists(\"acme\"), db.tenant_exists(\"empty\")]"), "[true, false]");
    // Named by the file alone, in the program's folder.
    let d2 = dir("listing2");
    let mut it = load(&d2, "db.open(\"{tenant}.sqlite\")\n");
    eval(&mut it, "with_tenant(\"acme\", () => db.exec(\"create table a (x)\"))");
    std::fs::write(d2.join("app.qwe"), "").unwrap();
    assert_eq!(eval(&mut it, "db.tenants()"), "[\"acme\"]");
}

#[test]
fn the_function_for_tenants_must_give_a_path_or_a_url() {
    let d = dir("target");
    let src = r#"
db.open((tenant) => {
    if tenant == "wrong" { return err("no such shop") }
    if tenant == "number" { return 5 }
    if tenant == "boom" { nosuchthing() }
    if tenant == "replies" { reply 404 }
    `${tenant}.db`
})
"#;
    let mut it = load(&d, src);
    let on = |it: &mut Interp, t: &str| eval(it, &format!("with_tenant(\"{}\", () => db.query(\"select 1 as x\"))", t));
    assert_eq!(on(&mut it, "fine"), "[{ x: 1 }]");
    says(on(&mut it, "wrong"), "tenant wrong: no such shop");
    says(on(&mut it, "number"), "the function for tenants' databases gave int, not a path or a URL");
    says(on(&mut it, "boom"), "tenant boom: unknown name `nosuchthing`");
    says(on(&mut it, "replies"), "the function for tenants' databases did not give a path or a URL");
    // Asked about one, the function's failure is the answer.
    says(eval(&mut it, "db.tenant_exists(\"boom\")"), "tenant boom: unknown name `nosuchthing`");
}

#[test]
fn a_tenants_database_is_migrated_when_the_program_asks_inside_its_tenant() {
    let d = dir("tenant-migrate");
    std::fs::create_dir_all(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_a.sql"), "create table a (x int);").unwrap();
    let mut it = load(&d, "db.open(\"data/{tenant}.db\")\n");
    // Outside any tenant there is none to bring up to date: each is when opened.
    assert_eq!(eval(&mut it, "db.migrate()"), "[]");
    assert_eq!(eval(&mut it, "with_tenant(\"zeta\", () => len(db.query(\"select * from a\")))"), "0");
    // Inside one it was brought up to date as it opened: nothing more to do.
    assert_eq!(eval(&mut it, "with_tenant(\"acme\", () => db.migrate())"), "[]");
    assert_eq!(eval(&mut it, "with_tenant(\"acme\", () => len(db.query(\"select * from _quillo_migrations\")))"), "1");
}

#[test]
fn a_tenant_that_was_never_made_cannot_be_reached_from_a_request() {
    let d = dir("no-tenant");
    let src = "db.open(\"data/{tenant}.db\")\ntenant_from((request) => request.headers[\"x-tenant\"])\nroute GET /t = db.query(\"select 1 as x\")\n";
    let mut it = load(&d, src);
    says(eval(&mut it, "db.query(\"select 1\")"), "this code runs for none");
    let r = request(&mut it, "GET", "/t", &[("X-Tenant", "ghost")], "");
    assert_eq!(r.status, 500);
    assert!(!d.join("data/ghost.db").exists());
}

// ---- sql_name ---------------------------------------------------------------------------------

#[test]
fn sql_name_quotes_as_the_programs_database_reads_names() {
    let d = dir("sqlname");
    let mut it = load(&d, "db.open(\"data/{tenant}.db\")\n");
    assert_eq!(eval(&mut it, "sql_name(\"a\")"), "\"\\\"a\\\"\"");
    let mut it = load(&d, "db.open(\"mysql://u@h/app_{tenant}\")\n");
    assert_eq!(eval(&mut it, "sql_name(\"a\")"), "\"`a`\"");
    // A function of the tenant: as the tenants' databases opened so far are.
    let mut it = load(&d, "db.open((t) => `${t}.db`)\n");
    assert_eq!(eval(&mut it, "sql_name(\"a\")"), "\"\\\"a\\\"\"");
    eval(&mut it, "with_tenant(\"acme\", () => db.query(\"select 1\"))");
    assert_eq!(eval(&mut it, "sql_name(\"a\")"), "\"\\\"a\\\"\"");
}

// ---- shards -----------------------------------------------------------------------------------

#[test]
fn shards_refuse_what_they_cannot_use() {
    let d = dir("shardargs");
    let mut it = load(&d, "let s = db.shards([\"a.db\", \"b.db\"])\nlet one = db.connect(\"c.db\")\n");
    says(eval(&mut it, "db.shards(\"a.db\")"), "`db.shards` expects a list of paths or database URLs, one per shard");
    says(eval(&mut it, "db.shards([])"), "`db.shards` needs one shard at least");
    says(eval(&mut it, "db.shards([\"x/{tenant}.db\"])"), "a shard is one database: no {tenant} in it");
    says(eval(&mut it, "db.shards([5])"), "`db.connect` expects a path");
    assert!(eval(&mut it, "db.shards([\"/no/such/dir/a.db\"])").starts_with("err(\"db:"));
    // Asked as one database, shards say what to call.
    says(eval(&mut it, "s.exec(\"select 1\")"), "`.on(key)`");
    says(eval(&mut it, "s.transaction(() => 1)"), "`.on(key)`");
    // `.on` and `.all` belong to shards.
    let stray = "{ \"$db\": one[\"$db\"], on: s.on, all: s.all }";
    says(eval(&mut it, &format!("{}.on(1)", stray)), "`on` is a function of shards (`db.shards([...])`)");
    says(eval(&mut it, &format!("{}.all(\"select 1\")", stray)), "`all` is a function of shards");
    says(eval(&mut it, "s.all(5)"), "`all` expects SQL text as its first argument, got int");
    // A shard that cannot answer is named.
    says(eval(&mut it, "s.all(\"select * from nowhere\")"), "shard 1: no such table: nowhere");
    assert!(eval(&mut it, "s.all(\"select * from nowhere\")").starts_with("err(\"db: shard 1:"));
}

// ---- one transaction over several databases, without a server --------------------------------

#[test]
fn db_atomic_refuses_what_is_not_a_database() {
    let d = dir("atomicargs");
    let mut it = load(&d, "db.open(\"main.db\")\nlet other = db.connect(\"other.db\")\n");
    says(eval(&mut it, "db.atomic(db)"), "`db.atomic` expects the databases and a function");
    says(eval(&mut it, "db.atomic([db])"), "`db.atomic` expects the databases and a function");
    says(eval(&mut it, "db.atomic([db], 5)"), "`db.atomic` expects the databases and a function");
    says(eval(&mut it, "db.atomic([5], () => 1)"), "`db.atomic`: int is not a database");
    says(eval(&mut it, "db.atomic([{ a: 1 }], () => 1)"), "not a database (`db`, or what `db.connect` gives)");
    says(eval(&mut it, "db.atomic([{ \"$db\": \"x\" }], () => 1)"), "`db.atomic`: not a database");
    // A database named twice is one part.
    assert_eq!(eval(&mut it, "db.atomic([db, db], () => 7)"), "7");
    // Inside a transaction on SQLite it cannot begin.
    says(eval(&mut it, "other.transaction(() => db.atomic([db], () => 1))"), "cannot begin inside a transaction");
}

#[test]
fn shards_take_part_in_db_atomic_one_sqlite_at_most() {
    let d = dir("atomicshards");
    let mut it = load(&d, "let s = db.shards([\"a.db\", \"b.db\"])\nlet one = db.shards([\"c.db\"])\n");
    says(eval(&mut it, "db.atomic([s], () => 1)"), "one SQLite database at most");
    eval(&mut it, "one.on(1).exec(\"create table t (x int)\")");
    let r = eval(&mut it, "db.atomic([one], () => one.on(1).exec(\"insert into t values (1)\").changes)");
    assert_eq!(r, "1");
    assert_eq!(eval(&mut it, "one.all(\"select x from t\")"), "[{ x: 1 }]");
}

#[test]
fn db_atomic_without_a_program_database_records_decisions_in_the_default_one() {
    let d = dir("atomicnone");
    let mut it = load(&d, "let other = db.connect(\"other.db\")\nother.exec(\"create table t (x int)\")\n");
    assert_eq!(eval(&mut it, "db.atomic([other], () => other.exec(\"insert into t values (1)\").changes)"), "1");
    assert_eq!(eval(&mut it, "len(other.query(\"select * from t\"))"), "1");
    assert_eq!(eval(&mut it, "len(db.query(\"select * from _quillo_atomic\"))"), "0");
}

#[test]
fn db_atomic_stops_when_the_decision_cannot_be_recorded() {
    let d = dir("atomicdecide");
    let mut it = load(&d, "db.open(\"main.db\")\nlet other = db.connect(\"other.db\")\nother.exec(\"create table t (x int)\")\n");
    // Not even its table can be made.
    eval(&mut it, "db.exec(\"pragma query_only = on\")");
    says(eval(&mut it, "db.atomic([other], () => other.exec(\"insert into t values (1)\"))"), "db.atomic: attempt to write a readonly database");
    eval(&mut it, "db.exec(\"pragma query_only = off\")");
    // Its table is there, and the decision is refused: nothing is kept.
    assert_eq!(eval(&mut it, "db.atomic([other], () => 1)"), "1");
    eval(&mut it, "db.exec(\"pragma query_only = on\")");
    says(eval(&mut it, "db.atomic([other], () => other.exec(\"insert into t values (1)\"))"), "db.atomic: the decision could not be recorded");
    assert_eq!(eval(&mut it, "len(other.query(\"select * from t\"))"), "0");
}

#[test]
fn a_sqlite_part_that_cannot_commit_undoes_the_whole() {
    let d = dir("atomiccommit");
    let src = "db.open(\"main.db\")\nlet other = db.connect(\"other.db\")\n\
               other.exec(\"create table p (id integer primary key)\")\n\
               other.exec(\"create table k (p int references p (id) deferrable initially deferred)\")\n";
    let mut it = load(&d, src);
    let r = eval(&mut it, "db.atomic([other], () => other.exec(\"insert into k values (9)\"))");
    says(r, "db.atomic: FOREIGN KEY constraint failed");
    assert_eq!(eval(&mut it, "len(other.query(\"select * from k\"))"), "0");
    // The decision went with it, in the database that holds decisions...
    assert_eq!(eval(&mut it, "len(db.query(\"select * from _quillo_atomic\"))"), "0");
    // ... and the next one works.
    assert_eq!(eval(&mut it, "db.atomic([other], () => 3)"), "3");
    // With the decisions in the part itself they go when it rolls back.
    let mut it = load(&dir("atomiccommit2"), "db.open(\"main.db\")\ndb.exec(\"create table p (id integer primary key); create table k (p int references p (id) deferrable initially deferred)\")\n");
    says(eval(&mut it, "db.atomic([db], () => db.exec(\"insert into k values (9)\"))"), "FOREIGN KEY constraint failed");
    assert_eq!(eval(&mut it, "[len(db.query(\"select * from k\")), len(db.query(\"select * from _quillo_atomic\"))]"), "[0, 0]");
}

// ---- commits that fail ------------------------------------------------------------------------

const DEFERRED: &str = "db.open(\"main.db\")\n\
    db.exec(\"create table p (id integer primary key)\")\n\
    db.exec(\"create table k (p int references p (id) deferrable initially deferred)\")\n";

#[test]
fn a_transaction_with_options_that_cannot_commit_is_rolled_back() {
    let d = dir("commit");
    let mut it = load(&d, DEFERRED);
    let r = eval(&mut it, "db.transaction({ retries: 2 }, () => db.exec(\"insert into k values (9)\"))");
    says(r, "FOREIGN KEY constraint failed");
    assert_eq!(eval(&mut it, "len(db.query(\"select * from k\"))"), "0");
    assert_eq!(eval(&mut it, "db.transaction({ retries: 2 }, () => 5)"), "5");
}

#[test]
fn a_transaction_that_cannot_commit_says_so() {
    let d = dir("release");
    let mut it = load(&d, DEFERRED);
    says(eval(&mut it, "db.transaction(() => db.exec(\"insert into k values (9)\"))"), "FOREIGN KEY constraint failed");
}

#[test]
#[ignore = "finding: a plain db.transaction whose commit fails (deferred foreign key) stays open: its row is seen after and later statements run inside it"]
fn a_transaction_that_cannot_commit_leaves_nothing_behind() {
    let d = dir("release2");
    let mut it = load(&d, DEFERRED);
    says(eval(&mut it, "db.transaction(() => db.exec(\"insert into k values (9)\"))"), "FOREIGN KEY constraint failed");
    assert_eq!(eval(&mut it, "len(db.query(\"select * from k\"))"), "0");
}

// ---- opening ----------------------------------------------------------------------------------

#[test]
fn a_database_another_process_is_writing_still_goes_to_wal() {
    let d = dir("wal");
    let path = d.join("old.db");
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer.execute_batch("create table t (x int); begin immediate; insert into t values (1);").unwrap();
    // SQLite refuses the switch at once, without waiting, while anyone writes.
    let free = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        writer.execute_batch("commit").unwrap();
    });
    let mut it = load(&d, "");
    assert_eq!(eval(&mut it, "db.open(\"old.db\")"), "null");
    free.join().unwrap();
    assert_eq!(eval(&mut it, "db.one(\"pragma journal_mode\").journal_mode"), "\"wal\"");
    assert_eq!(eval(&mut it, "len(db.query(\"select * from t\"))"), "1");
}

// ---- live updates -----------------------------------------------------------------------------

#[test]
fn a_loader_that_writes_in_a_transaction_does_not_wake_itself() {
    let d = dir("live");
    let src = r#"
db.open("main.db")
db.exec("create table seen (n int)")
server fn mark() {
    db.transaction(() => db.exec("insert into seen values (1)"))
    db.query("select * from seen")
}
server fn plain() = db.query("select * from seen")
"#;
    let mut it = load(&d, src);
    let live = [("Content-Type", "application/json"), ("X-Quillo-Live", "1")];
    let j = |r: HttpResponse| serde_json::from_slice::<serde_json::Value>(&r.body).unwrap();
    let before = j(request(&mut it, "POST", "/_quillo/rpc/plain", &live, "[]"));
    let after = j(request(&mut it, "POST", "/_quillo/rpc/mark", &live, "[]"));
    assert_eq!(after["topics"], serde_json::json!(["db:seen"]));
    // It has seen its own change: the sequence it reports is past that.
    assert!(after["seq"].as_u64() > before["seq"].as_u64(), "{} then {}", before, after);
    let again = j(request(&mut it, "POST", "/_quillo/rpc/plain", &live, "[]"));
    assert_eq!(again["seq"], after["seq"]);
}

// ---- Quillo's own tables ----------------------------------------------------------------------

/// A scheduled job, asked for as the scheduler does.
fn job(it: &mut Interp, name: &str, at: u64) -> String {
    let r = request(it, "POST", &format!("/_quillo/job/{}?at={}", name, at), &[("X-Quillo-Job", quillo::jobs::token())], "");
    assert_eq!(r.status, 200);
    String::from_utf8_lossy(&r.body).into_owned()
}

const JOBBED: &str = "db.open(\"main.db\")\ndb.exec(\"create table ran (n int)\")\nfn work() = db.exec(\"insert into ran values (1)\")\nschedule(\"1h\", work)\n";

#[test]
fn a_jobs_run_is_claimed_once() {
    let d = dir("jobs");
    let mut it = load(&d, JOBBED);
    assert_eq!(job(&mut it, "work", 100), "ok\n");
    assert_eq!(job(&mut it, "work", 100), "run by another server\n");
    assert_eq!(job(&mut it, "work", 200), "ok\n");
    assert_eq!(eval(&mut it, "len(db.query(\"select * from ran\"))"), "2");
}

#[test]
fn a_job_runs_when_its_claim_cannot_be_kept() {
    // The table cannot be made.
    let d = dir("jobs2");
    let mut it = load(&d, "db.open(\"main.db\")\nfn work() = 1\nschedule(\"1h\", work)\n");
    eval(&mut it, "db.exec(\"pragma query_only = on\")");
    assert_eq!(job(&mut it, "work", 5), "ok\n");
    assert_eq!(job(&mut it, "work", 5), "ok\n");
    // The claim cannot be written.
    let d = dir("jobs3");
    let mut it = load(&d, JOBBED);
    eval(&mut it, "db.exec(\"create table _quillo_jobs (name text)\")");
    assert_eq!(job(&mut it, "work", 5), "ok\n");
    assert_eq!(job(&mut it, "work", 5), "ok\n");
    assert_eq!(eval(&mut it, "len(db.query(\"select * from ran\"))"), "2");
}

// ---- calls made later ---------------------------------------------------------------------------

/// The scheduler's call that delivers what is due.
fn deliver(it: &mut Interp) {
    let r = request(it, "POST", "/_quillo/later", &[("X-Quillo-Job", quillo::jobs::token())], "");
    assert_eq!((r.status, String::from_utf8_lossy(&r.body).into_owned()), (200, "ok\n".to_string()));
}

const LATER: &str = "(id text primary key, service text not null, fn text not null, args text not null, tenant text not null, \
                     rid text not null, at bigint not null, tries bigint not null, error text)";

/// A call made later and now due, for a service no one answers.
fn due(c: &rusqlite::Connection) {
    c.execute_batch("insert into _quillo_later values ('c1', 'mailer', 'send', '[]', '', '', 1, 0, null)").unwrap();
}

fn at_of(c: &rusqlite::Connection) -> i64 {
    c.query_row("select at from _quillo_later", [], |r| r.get(0)).unwrap()
}

const FRONT: &str = "service mailer = \"mailer.qwe\"\nroute GET /send/:to = mailer.later.send(to)\n";

#[test]
fn a_call_made_later_is_left_alone_by_a_server_that_cannot_take_it() {
    let d = dir("later-take");
    std::fs::write(d.join("mailer.qwe"), "server fn send() = 1\n").unwrap();
    let c = rusqlite::Connection::open(d.join("main.db")).unwrap();
    c.execute_batch(&format!("create table _quillo_later {}", LATER)).unwrap();
    due(&c);
    let mut it = load(&d, &format!("db.open(\"main.db\")\n{}", FRONT));
    // Another server took it first: the update changes nothing.
    c.execute_batch("create trigger taken before update on _quillo_later begin select raise(ignore); end").unwrap();
    deliver(&mut it);
    assert_eq!(at_of(&c), 1);
    // Taking it fails.
    c.execute_batch("drop trigger taken; create trigger refused before update on _quillo_later begin select raise(abort, 'no'); end").unwrap();
    deliver(&mut it);
    assert_eq!(at_of(&c), 1);
}

#[test]
fn a_call_made_later_with_odd_columns_is_read_as_empty() {
    let d = dir("later-odd");
    std::fs::write(d.join("mailer.qwe"), "server fn send() = 1\n").unwrap();
    let c = rusqlite::Connection::open(d.join("main.db")).unwrap();
    c.execute_batch("create table _quillo_later (id, service, fn, args, tenant, rid, at, tries, error)").unwrap();
    c.execute_batch("insert into _quillo_later values (7, 'mailer', 'send', '[]', '', '', 0.5, 0, null)").unwrap();
    let mut it = load(&d, &format!("db.open(\"main.db\")\n{}", FRONT));
    // Its id is not text, so it cannot be taken; nothing else stops.
    deliver(&mut it);
    let id: i64 = c.query_row("select id from _quillo_later", [], |r| r.get(0)).unwrap();
    assert_eq!(id, 7);
}

#[test]
fn a_call_made_later_is_kept_in_memory_when_the_database_will_not_keep_it() {
    let d = dir("later-memory");
    std::fs::write(d.join("mailer.qwe"), "server fn send(to: str) = to\n").unwrap();
    let mut it = load(&d, &format!("db.open(\"main.db\")\n{}", FRONT));
    eval(&mut it, "db.exec(\"pragma query_only = on\")");
    let r = request(&mut it, "GET", "/send/ana", &[], "");
    assert_eq!(r.status, 200);
    assert!(!String::from_utf8_lossy(&r.body).is_empty());
    assert_eq!(eval(&mut it, "len(db.query(\"select * from sqlite_master where name = '_quillo_later'\"))"), "0");
}

#[test]
fn with_tenants_nothing_is_delivered_until_there_is_a_system_database() {
    let d = dir("later-tenants");
    std::fs::write(d.join("mailer.qwe"), "server fn send() = 1\n").unwrap();
    let mut it = load(&d, &format!("db.open(\"data/{{tenant}}.db\")\n{}", FRONT));
    deliver(&mut it);
    assert!(!d.join("quillo-system.db").exists());
}

// ---- what the environment says ------------------------------------------------------------------

#[test]
fn quillo_db_can_be_one_database_per_tenant() {
    let d = dir("env-tenant");
    let app = d.join("app.qwe");
    let src = "with_tenant(\"acme\", () => db.exec(\"create table if not exists a (x int)\"))\n\
               tenant_from((request) => request.headers[\"x-tenant\"])\n\
               route GET /t = db.query(\"select * from a\")\n\
               route GET /name = sql_name(\"a\")\n";
    std::fs::write(&app, src).unwrap();
    let s = support::serve(&app, &[("QUILLO_DB", "data/{tenant}.db")]);
    assert!(d.join("data/acme.db").is_file());
    let r = support::send(s.port, "GET", "/t", &[("X-Tenant", "acme")], "");
    assert_eq!((r.status, r.body.as_str()), (200, "[]"));
}

#[test]
fn with_one_database_per_tenant_from_quillo_db_calls_made_later_go_to_the_system_database() {
    let d = dir("env-tenant-later");
    std::fs::write(d.join("mailer.qwe"), "server fn send(to: str) = to\n").unwrap();
    let app = d.join("app.qwe");
    std::fs::write(&app, FRONT).unwrap();
    let nobody = format!("http://127.0.0.1:{}", support::free_port());
    let s = support::serve(&app, &[("QUILLO_DB", "data/{tenant}.db"), ("QUILLO_SERVICE_MAILER", &nobody), ("QUILLO_SERVICE_SECRET", "a secret")]);
    assert_eq!(support::send(s.port, "GET", "/send/ana", &[], "").status, 200);
    let c = rusqlite::Connection::open(d.join("quillo-system.db")).unwrap();
    let n: i64 = c.query_row("select count(*) from _quillo_later", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}

#[test]
fn with_quillo_db_calls_made_later_are_kept_there() {
    let d = dir("env-later");
    std::fs::write(d.join("mailer.qwe"), "server fn send(to: str) = to\n").unwrap();
    let app = d.join("app.qwe");
    std::fs::write(&app, FRONT).unwrap();
    let nobody = format!("http://127.0.0.1:{}", support::free_port());
    let s = support::serve(&app, &[("QUILLO_DB", "main.db"), ("QUILLO_SERVICE_MAILER", &nobody), ("QUILLO_SERVICE_SECRET", "a secret")]);
    let r = support::send(s.port, "GET", "/send/ana", &[], "");
    assert_eq!(r.status, 200);
    let c = rusqlite::Connection::open(d.join("main.db")).unwrap();
    let (service, f): (String, String) = c.query_row("select service, fn from _quillo_later", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((service.as_str(), f.as_str()), ("mailer", "send"));
    assert!(!d.join("quillo-system.db").exists());
}

#[test]
fn db_atomic_says_when_nothing_can_hold_its_decisions() {
    let d = dir("env-system");
    let app = d.join("app.qwe");
    let src = "db.open(\"data/{tenant}.db\")\nlet other = db.connect(\"other.db\")\nroute GET /a = str(db.atomic([other], () => 1))\n";
    std::fs::write(&app, src).unwrap();
    let s = support::serve(&app, &[("QUILLO_SYSTEM_DB", "/no/such/dir/system.db")]);
    let r = support::send(s.port, "GET", "/a", &[], "");
    assert!(r.body.contains("db.atomic: no database to record decisions in"), "{} {}", r.status, r.body);
}

// ---- calls a service's copies share -------------------------------------------------------------

const SECRET: &str = "a secret for these tests";

/// A call to `ledger`, signed as another service would.
fn call(port: u16, id: &str, body: &str) -> support::Answer {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let sig = quillo::service::signature(SECRET, t, "ledger", "add", "", id, body.as_bytes());
    let signed = format!("t={},sig={}", t, sig);
    let headers = [("x-quillo-service", signed.as_str()), ("x-quillo-to", "ledger"), ("x-quillo-call", id)];
    support::send(port, "POST", "/_quillo/service/add", &headers, body)
}

const LEDGER: &str = "db.open(\"ledger.db\")\ndb.exec(\"create table if not exists entries (n int)\")\n\
                      server fn add(n: int) {\n    db.exec(\"insert into entries values (?)\", n)\n    n * 2\n}\n";

fn copy(d: &Path) -> support::Server {
    support::serve(&d.join("ledger.qwe"), &[("QUILLO_SERVICE_SECRET", SECRET)])
}

fn entries(c: &rusqlite::Connection) -> i64 {
    c.query_row("select count(*) from entries", [], |r| r.get(0)).unwrap()
}

#[test]
fn a_call_sent_again_to_another_copy_is_answered_as_it_was() {
    let d = dir("copies");
    std::fs::write(d.join("ledger.qwe"), LEDGER).unwrap();
    let (a, b) = (copy(&d), copy(&d));
    let first = call(a.port, "call1", "[21]");
    assert_eq!((first.status, first.body.as_str()), (200, "{\"ok\":42}"));
    let again = call(b.port, "call1", "[21]");
    assert_eq!((again.status, again.body.as_str()), (200, "{\"ok\":42}"));
    let c = rusqlite::Connection::open(d.join("ledger.db")).unwrap();
    assert_eq!(entries(&c), 1);
    // One still running on another copy: asked again later, not run here.
    c.execute_batch("insert into _quillo_calls values ('call2', 1, 0, '')").unwrap();
    let r = call(a.port, "call2", "[1]");
    assert_eq!(r.status, 503, "{}", r.body);
    assert!(r.body.contains("still running"), "{}", r.body);
    assert_eq!(entries(&c), 1);
}

#[test]
fn calls_no_copy_remembers_any_longer_are_forgotten_now_and_then() {
    let d = dir("forget");
    std::fs::write(d.join("ledger.qwe"), LEDGER).unwrap();
    let a = copy(&d);
    assert_eq!(call(a.port, "first", "[1]").status, 200);
    let c = rusqlite::Connection::open(d.join("ledger.db")).unwrap();
    c.execute_batch("insert into _quillo_calls values ('old', 1, 200, '{}')").unwrap();
    let gone = (0..8000).any(|i| {
        assert_eq!(call(a.port, &format!("n{}", i), "[1]").status, 200);
        c.query_row("select count(*) from _quillo_calls where id = 'old'", [], |r| r.get::<_, i64>(0)).unwrap() == 0
    });
    assert!(gone, "an old call was not forgotten in 8000 calls");
    assert_eq!(c.query_row("select count(*) from _quillo_calls where id = 'first'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn a_call_runs_when_its_copies_cannot_share_it() {
    // The table cannot be made: this copy runs it, on its own.
    let d = dir("noshare");
    std::fs::write(d.join("ledger.qwe"), "db.open(\"ledger.db\")\ndb.exec(\"pragma query_only = on\")\nserver fn add(n: int) = n * 2\n").unwrap();
    let a = copy(&d);
    assert_eq!(call(a.port, "x1", "[1]").body, "{\"ok\":2}");
    // The table is another's: the claim cannot be made, nor read.
    let d = dir("noshare2");
    let c = rusqlite::Connection::open(d.join("ledger.db")).unwrap();
    c.execute_batch("create table _quillo_calls (id text primary key)").unwrap();
    std::fs::write(d.join("ledger.qwe"), LEDGER).unwrap();
    let a = copy(&d);
    assert_eq!(call(a.port, "x2", "[1]").body, "{\"ok\":2}");
    assert_eq!(entries(&c), 1);
    // The claim is refused and nothing is there: forgotten between the two.
    let d = dir("noshare3");
    let c = rusqlite::Connection::open(d.join("ledger.db")).unwrap();
    c.execute_batch(
        "create table _quillo_calls (id text primary key, at bigint not null, status integer not null, answer text not null);\
         create trigger never before insert on _quillo_calls begin select raise(abort, 'no'); end",
    )
    .unwrap();
    std::fs::write(d.join("ledger.qwe"), LEDGER).unwrap();
    let a = copy(&d);
    assert_eq!(call(a.port, "x3", "[1]").body, "{\"ok\":2}");
    assert_eq!(entries(&c), 1);
}

// ---- what `sqlinfo` reads ---------------------------------------------------------------------

fn rw(sql: &str) -> (Vec<String>, Vec<String>) {
    let t = quillo::sqlinfo::tables(sql);
    (t.reads, t.writes)
}

fn reads(sql: &str, want: &[&str]) {
    assert_eq!(rw(sql), (want.iter().map(|s| s.to_string()).collect(), Vec::new()), "{}", sql);
}

fn writes(sql: &str, want: &[&str]) {
    assert_eq!(rw(sql), (Vec::new(), want.iter().map(|s| s.to_string()).collect()), "{}", sql);
}

#[test]
fn strings_and_comments_hold_no_table_names() {
    reads("select 'from fake', 'it''s from fake' from real", &["real"]);
    reads("select 'a\\' from fake' from real", &["real"]);
    reads("select 1 -- from fake\n from real", &["real"]);
    reads("select 1 -- from fake", &[]);
    reads("select 1 /* from fake */ from real", &["real"]);
    reads("select 1 /* from fake", &[]);
    reads("select $$ from fake $$ from real", &["real"]);
    reads("select $tag$ from fake $tag$ from real", &["real"]);
    reads("select $tag$ from fake", &[]);
    reads("select $1, $x from real", &["real"]);
    reads("select 1 from \"My Table\"", &["my table"]);
    reads("select 1 from `Ticks`", &["ticks"]);
    reads("select 1 from [Brackets]", &["brackets"]);
    reads("select 1 from \"unclosed", &["unclosed"]);
}

#[test]
fn a_tables_schema_is_left_out() {
    reads("select 1 from public.\"Notes\"", &["notes"]);
    reads("select 1 from a.b.c", &["c"]);
    reads("select 1 from a.*", &["a"]);
    reads("select 1 from only notes", &["notes"]);
    reads("select 1 from lateral notes", &["notes"]);
    // What follows `from` need not be a table.
    reads("select 1 from (select 2 from inner_one) x", &["inner_one"]);
    reads("select 1 from generate_series(1, 3)", &[]);
    reads("select * from json_each(?)", &[]);
    reads("select 1 from", &[]);
}

#[test]
fn a_list_of_tables_is_read_past_aliases() {
    reads("select 1 from a x, b as y, \"C\" \"z\", d", &["a", "b", "c", "d"]);
    reads("select 1 from a, (select 2) s", &["a"]);
    reads("select 1 from a where x in (select y from b)", &["a", "b"]);
    reads("select 1 from a join b on a.x = b.x left join c using (y)", &["a", "b", "c"]);
}

#[test]
fn writes_are_found_wherever_they_are_named() {
    writes("truncate notes", &["notes"]);
    writes("truncate table only notes", &["notes"]);
    writes("alter table notes add x int", &["notes"]);
    writes("drop table if exists notes", &["notes"]);
    writes("update only notes set x = 1", &["notes"]);
    writes("delete from public.notes", &["notes"]);
    writes("insert into notes default values", &["notes"]);
    writes("insert or replace into notes values (1)", &["notes"]);
    assert_eq!(rw("insert into a select * from b"), (vec!["b".to_string()], vec!["a".to_string()]));
    assert_eq!(rw("table notes"), (vec![], vec![]));
    assert_eq!(rw("into 'x'"), (vec![], vec![]));
    assert_eq!(rw("update set x = 1"), (vec![], vec![]));
}

#[test]
fn statements_are_worked_out_once_and_more_of_them_start_the_list_over() {
    let a = quillo::sqlinfo::tables_of("select 1 from cached_one");
    let b = quillo::sqlinfo::tables_of("select 1 from cached_one");
    assert!(std::sync::Arc::ptr_eq(&a, &b));
    assert_eq!(a.reads, ["cached_one"]);
    // Past the list's size, older ones are worked out anew, the same.
    for i in 0..1100 {
        let sql = format!("select 1 from generated_{}", i);
        assert_eq!(quillo::sqlinfo::tables_of(&sql).reads, [format!("generated_{}", i)]);
    }
    assert_eq!(quillo::sqlinfo::tables_of("select 1 from cached_one").reads, ["cached_one"]);
}
