//! `db.migrate` on each database: a file changed after it ran gets a warning
//! and no second run, and a file that fails stops the rest.

mod support;

use quillo::server::HttpRequest;
use quillo::Interp;
use std::path::{Path, PathBuf};

fn program(name: &str, open: &str) -> (Interp, PathBuf) {
    let d = support::temp_dir(&format!("migrations-{}", name));
    std::fs::create_dir_all(d.join("migrations")).unwrap();
    let src = format!("db.open(\"{}\")\nroute GET /migrate = str(db.migrate(\"migrations\"))\nroute GET /rows = str(db.one(\"select count(*) as c from t\").c)\n", open);
    std::fs::write(d.join("app.qwe"), src).unwrap();
    let mut it = Interp::new(d.to_str().unwrap());
    it.load_file(&d.join("app.qwe")).unwrap_or_else(|e| panic!("{}", e));
    (it, d)
}

fn get(it: &mut Interp, url: &str) -> (u16, String) {
    let r = it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers: Vec::new(), body: Vec::new(), remote: "127.0.0.1".into() });
    (r.status, r.text())
}

fn write(d: &Path, file: &str, sql: &str) {
    std::fs::write(d.join("migrations").join(file), sql).unwrap();
}

fn a_changed_file_is_not_run_again(it: &mut Interp, d: &Path) {
    write(d, "001_t.sql", "create table t (a int);\ninsert into t values (1);\n");
    assert_eq!(get(it, "/migrate"), (200, "[\"001_t.sql\"]".to_string()));
    assert_eq!(get(it, "/migrate"), (200, "[]".to_string()));
    write(d, "001_t.sql", "create table t (a int);\ninsert into t values (1);\ninsert into t values (2);\n");
    assert_eq!(get(it, "/migrate"), (200, "[]".to_string()));
    assert_eq!(get(it, "/rows").1, "1");
}

#[test]
fn sqlite_warns_of_a_changed_migration_and_leaves_it() {
    let (mut it, d) = program("sqlite", "app.db");
    a_changed_file_is_not_run_again(&mut it, &d);
}

#[test]
fn postgres_warns_of_a_changed_migration_and_leaves_it() {
    let Some(pg) = support::postgres("migrations-changed") else { return };
    let url = pg.database("changed");
    let (mut it, d) = program("pg", &url);
    a_changed_file_is_not_run_again(&mut it, &d);
}

#[test]
fn mysql_warns_of_a_changed_migration_and_stops_at_a_failure() {
    let Some(my) = support::mariadb("migrations-changed") else { return };
    let boot = format!("db.open(\"{}\")\nlet made = db.exec(\"create database if not exists changed\")\nroute GET / = \"up\"\n", my.url("mysql"));
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    it.load_source(&boot, "boot.qwe").unwrap_or_else(|e| panic!("{}", e));
    let (mut it, d) = program("my", &my.url("changed"));
    a_changed_file_is_not_run_again(&mut it, &d);
    write(&d, "002_bad.sql", "this is not sql");
    write(&d, "003_after.sql", "insert into t values (9);");
    let (status, text) = get(&mut it, "/migrate");
    assert!(text.contains("002_bad.sql") || status == 500, "{}: {}", status, text);
    assert_eq!(get(&mut it, "/rows").1, "1");
}
