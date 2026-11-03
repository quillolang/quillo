//! `db` past the usual path (src/db.rs, src/pg.rs): `new_tenant` where the function for tenants'
//! databases gives nothing or fails, a tenant in memory or behind a file, `db.exec_many` outside
//! any tenant, names quoted for shards, calls made later in a tenant (kept, tried, looked at
//! again or let go) with the system database on SQLite and on MariaDB, and PostgreSQL tenants
//! whose database or schema cannot be had. Server databases are throwaway ones; their tests are
//! skipped when the server is not installed.

mod support;

use quillo::value::Value;
use quillo::Interp;
use std::path::{Path, PathBuf};
use std::process::Command;

fn dir(name: &str) -> PathBuf {
    support::temp_dir(&format!("cover-db-{}", name))
}

fn load(d: &Path, src: &str) -> Interp {
    let mut it = Interp::new(d);
    it.load_source(src, "app.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    it
}

fn eval(it: &mut Interp, e: &str) -> String {
    it.load_source(&format!("fn __t() {{\n{}\n}}\n", e), "case.qwe").unwrap_or_else(|x| panic!("{}\n{}", x, e));
    match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    }
}

fn says(got: String, what: &str) {
    assert!(got.contains(what), "wanted {:?} in: {}", what, got);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

// ---- new_tenant ---------------------------------------------------------------------------

const TENANTS_BY_FN: &str = r#"
db.open((tenant) => {
    if tenant == "none" { return null }
    if tenant == "refused" { return err("not a shop we serve") }
    if tenant == "mem" { return ":memory:" }
    if tenant == "blocked" { return "a-file/blocked.db" }
    `data/${tenant}.db`
})
"#;

#[test]
fn new_tenant_says_when_the_function_for_tenants_gives_nothing_or_fails() {
    let d = dir("new-tenant-fn");
    let mut it = load(&d, TENANTS_BY_FN);
    says(eval(&mut it, "new_tenant(\"none\", () => 1)"), "`new_tenant`: the function for tenants' databases gave null for \"none\"");
    says(eval(&mut it, "new_tenant(\"refused\", () => 1)"), "`new_tenant`: tenant refused: not a shop we serve");
    assert_eq!(eval(&mut it, "new_tenant(\"shop\", () => \"made\")"), "\"made\"");
    assert!(d.join("data/shop.db").is_file());
}

#[test]
fn a_tenant_in_memory_is_always_new_and_one_behind_a_file_cannot_be_made() {
    let d = dir("new-tenant-where");
    std::fs::write(d.join("a-file"), "a file, not a folder").unwrap();
    let mut it = load(&d, TENANTS_BY_FN);
    // In memory there is nothing to find: each is new, and works.
    assert_eq!(eval(&mut it, "new_tenant(\"mem\", () => db.query(\"select 2 as two\"))"), "[{ two: 2 }]");
    assert_eq!(eval(&mut it, "new_tenant(\"mem\", () => \"again\")"), "\"again\"");
    let blocked = eval(&mut it, "new_tenant(\"blocked\", () => 1)");
    says(blocked.clone(), "`new_tenant`: ");
    says(blocked, "a-file/blocked.db");
}

#[test]
fn exec_many_outside_any_tenant_is_an_error_value() {
    let d = dir("exec-many-none");
    let mut it = load(&d, "db.open(\"data/{tenant}.db\")\n");
    let got = eval(&mut it, "let r = db.exec_many(\"insert into t values (?)\", [[1], [2]])\n[is_err(r), error(r)]");
    assert!(got.starts_with("[true, ") && got.contains("this code runs for none"), "{}", got);
    // Inside one it goes.
    assert_eq!(eval(&mut it, "with_tenant(\"acme\", () => { db.exec(\"create table t (x)\")\n db.exec_many(\"insert into t values (?)\", [[1], [2]]).changes })"), "2");
}

#[test]
fn a_name_is_quoted_as_sqlite_and_postgres_quote_it_for_shards() {
    let d = dir("shards-name");
    let mut it = load(&d, "let s = db.shards([\"a.db\", \"b.db\"])\nlet named = { \"$db\": s[\"$db\"], sql_name: db.sql_name }\n");
    assert_eq!(eval(&mut it, "named.sql_name(\"order\")"), "\"\\\"order\\\"\"");
    assert_eq!(eval(&mut it, "s.count"), "2");
}

// ---- calls made later, in tenants ------------------------------------------------------------

/// A program with a database per tenant and service `mail` at an address no one answers at.
fn later_program(name: &str, extra: &str) -> (PathBuf, Interp) {
    let d = dir(name);
    std::fs::write(d.join("mail.qwe"), "server fn send(x) = x\n").unwrap();
    let mut it = Interp::new(&d);
    it.shared.service_urls.lock().unwrap().insert("mail".to_string(), format!("http://127.0.0.1:{}", support::free_port()));
    let src = format!("db.open(\"data/{{tenant}}.db\")\n{}\nservice mail = \"mail.qwe\" {{ retries: 0 }}\n", extra);
    it.load_source(&src, "app.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    (d, it)
}

fn system_rows(d: &Path) -> Vec<(String, i64, i64)> {
    let c = rusqlite::Connection::open(d.join("quillo-system.db")).unwrap();
    let mut s = c.prepare("select tenant, made, at from _quillo_later_tenants order by tenant").unwrap();
    s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(|r| r.unwrap()).collect()
}

#[test]
fn a_tenants_call_not_made_yet_is_looked_at_again_when_it_is_next_due() {
    let (d, mut it) = later_program("later-tenant-due", "");
    let id = eval(&mut it, "with_tenant(\"acme\", () => mail.later.send(1))");
    assert!(id.starts_with('"') && id.len() == 26, "{}", id);
    let before = system_rows(&d);
    assert_eq!(before.len(), 1, "{:?}", before);
    assert_eq!(before[0].0, "acme");
    let began = now_ms();
    quillo::service::deliver(&mut it);
    // Tried once and not made: it waits, and the tenant is looked at again when it is next due.
    assert_eq!(eval(&mut it, "with_tenant(\"acme\", () => db.query(\"select tries from _quillo_later\"))"), "[{ tries: 1 }]");
    let due = eval(&mut it, "with_tenant(\"acme\", () => db.one(\"select at from _quillo_later\").at)").parse::<i64>().unwrap();
    let after = system_rows(&d);
    assert_eq!(after[0].2, due, "{:?}", after);
    assert!(due >= began + 1000, "a second later at least: {} {}", due, began);
    // Its state, while it waits; and one asked for that no home has.
    let st = eval(&mut it, &format!("with_tenant(\"acme\", () => later.status({}))", id));
    assert!(st.contains("state: \"waiting\"") && st.contains("tries: 1"), "{}", st);
    assert_eq!(eval(&mut it, "with_tenant(\"acme\", () => later.status(\"no-such-call\"))"), "null");
}

#[test]
fn a_tenant_with_nothing_waiting_is_let_go_after_an_hour_and_one_that_cannot_say_is_asked_in_a_minute() {
    let (d, mut it) = later_program("later-tenant-seen", "");
    // The system database made by a first call, then notes of tenants that have nothing waiting.
    eval(&mut it, "with_tenant(\"acme\", () => mail.later.send(1))");
    std::fs::create_dir_all(d.join("data")).unwrap();
    std::fs::write(d.join("data/broken.db"), "this is not a database, only words").unwrap();
    let old = now_ms() - 2 * 3_600_000;
    {
        let c = rusqlite::Connection::open(d.join("quillo-system.db")).unwrap();
        c.execute("insert into _quillo_later_tenants (tenant, made, at) values ('ghost', ?1, 0)", [old]).unwrap();
        c.execute("insert into _quillo_later_tenants (tenant, made, at) values ('broken', ?1, 0)", [now_ms()]).unwrap();
    }
    let began = now_ms();
    quillo::service::deliver(&mut it);
    let rows = system_rows(&d);
    let names: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(names, ["acme", "broken"], "ghost, with nothing for two hours, is let go: {:?}", rows);
    let broken = rows.iter().find(|r| r.0 == "broken").unwrap();
    assert!(broken.2 >= began + 59_000 && broken.2 <= now_ms() + 61_000, "asked again in a minute: {:?}", broken);
}

#[test]
fn a_call_whose_tenant_cannot_be_noted_waits_in_memory() {
    let d = dir("later-tenant-unnoted");
    std::fs::write(d.join("mail.qwe"), "server fn send(x) = x\n").unwrap();
    // A system database whose table of tenants is not Quillo's: nothing can be noted in it.
    {
        let c = rusqlite::Connection::open(d.join("quillo-system.db")).unwrap();
        c.execute_batch("create table _quillo_later_tenants (x)").unwrap();
    }
    let mut it = Interp::new(&d);
    it.shared.service_urls.lock().unwrap().insert("mail".to_string(), format!("http://127.0.0.1:{}", support::free_port()));
    it.load_source("db.open(\"data/{tenant}.db\")\nservice mail = \"mail.qwe\" { retries: 0 }\n", "app.qwe").unwrap();
    let id = eval(&mut it, "with_tenant(\"acme\", () => mail.later.send(1))");
    let st = eval(&mut it, &format!("later.status({})", id));
    assert_eq!(st, "{ state: \"waiting\", tries: 0, error: null }");
    // Nothing went into the tenant's database.
    assert_eq!(eval(&mut it, "with_tenant(\"acme\", () => len(db.query(\"select name from sqlite_master where name = '_quillo_later'\")))"), "0");
}

#[test]
fn a_tenants_call_is_noted_in_a_system_database_on_mariadb() {
    let Some(m) = support::mariadb("cover-db-later") else { return };
    let (_d, mut it) = later_program("later-tenant-maria", &format!("db.system({0:?})\nlet sys = db.connect({0:?})", m.url("cover_sys")));
    eval(&mut it, "with_tenant(\"acme\", () => mail.later.send(1))");
    eval(&mut it, "with_tenant(\"acme\", () => mail.later.send(2))");
    assert_eq!(eval(&mut it, "sys.query(\"select tenant from _quillo_later_tenants\")"), "[{ tenant: \"acme\" }]", "noted once, made again");
    assert_eq!(eval(&mut it, "with_tenant(\"acme\", () => len(db.query(\"select id from _quillo_later\")))"), "2");
}

// ---- PostgreSQL tenants ------------------------------------------------------------------------

fn psql(pg: &support::Pg, sql: &str) {
    let out = Command::new(pg.bin.join("psql")).args([pg.url.as_str(), "-q", "-c", sql]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn a_postgres_tenant_needs_its_database_made_beforehand_and_a_schema_it_may_make() {
    let Some(pg) = support::postgres("cover-db-tenants") else { return };
    let d = dir("pg-tenants");
    // A database of its own for each tenant: made beforehand, never by new_tenant.
    let per_db = pg.url.replace("/postgres", "/cover_t_{tenant}");
    let mut it = load(&d, &format!("db.open({:?})\n", per_db));
    says(eval(&mut it, "new_tenant(\"acme\", () => 1)"), "`new_tenant`: its database is made beforehand: ");
    psql(&pg, "create database cover_t_made");
    let made = eval(&mut it, "new_tenant(\"made\", () => 1)");
    assert!(made.contains("there is already a tenant \\\"made\\\""), "{}", made);
    // A schema for each, by a role that may not make one.
    psql(&pg, "create role cover_weak login");
    let weak = pg.url.replace("quillo@", "cover_weak@") + "?schema=t_{tenant}";
    let mut it = load(&d, &format!("db.open({:?})\n", weak));
    let refused = eval(&mut it, "new_tenant(\"acme\", () => 1)");
    assert!(refused.contains("`new_tenant`: ") && refused.to_ascii_lowercase().contains("permission denied"), "{}", refused);
}

#[allow(dead_code)]
fn unused(_: Value) {}
