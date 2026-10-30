//! SQLite refusing work where a program can't see it coming: a statement that
//! fails only as it runs, a value too long to bind, words in a decimal column;
//! and the database Quillo keeps its own records in, named more than once.

mod support;

use quillo::Interp;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn dir(name: &str) -> PathBuf {
    support::temp_dir(&format!("sqlite-failures-{}", name))
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

#[test]
fn a_statement_that_fails_as_it_runs_gives_an_error_value() {
    let d = dir("step");
    let mut it = load(&d, "db.open(\"app.db\")\n");
    assert_eq!(eval(&mut it, "db.query(\"select abs(-9223372036854775807 - 1) as n\")"), r#"err("db: integer overflow")"#);
    assert_eq!(eval(&mut it, "db.one(\"select json('{') as j\")"), r#"err("db: malformed JSON")"#);
}

#[test]
fn a_failure_past_the_first_row_gives_the_error_not_the_first_rows() {
    let d = dir("step-later");
    let mut it = load(&d, "db.open(\"app.db\")\ndb.exec(\"create table t (n int)\")\ndb.exec(\"insert into t values (1), (-9223372036854775808)\")\n");
    assert_eq!(eval(&mut it, "db.query(\"select abs(n) as n from t order by n desc\")"), r#"err("db: integer overflow")"#);
}

#[test]
fn words_in_a_decimal_column_come_back_as_words() {
    let d = dir("decimal");
    let mut it = load(&d, "db.open(\"app.db\")\ndb.exec(\"create table prices (amount decimal(12,2))\")\ndb.exec(\"insert into prices values ('about ten'), ('12.50')\")\n");
    assert_eq!(eval(&mut it, "db.query(\"select amount from prices order by rowid\").map(p => p.amount)"), r#"["about ten", 12.5m]"#);
}

#[test]
fn a_string_past_what_sqlite_binds_is_refused() {
    let d = dir("too-big");
    let mut it = load(&d, "db.open(\"app.db\")\nlet part = repeat(\"x\", 100000000)\nlet big = join([part, part, part, part, part, part, part, part, part, part, \"x\"], \"\")\n");
    assert_eq!(eval(&mut it, "db.query(\"select length(?) as n\", big)"), r#"err("db: string or blob too big")"#);
    assert_eq!(eval(&mut it, "db.query(\"select length(:text) as n\", { text: big })"), r#"err("db: string or blob too big")"#);
    assert_eq!(eval(&mut it, "db.one(\"select length(?) as n\", part).n"), "100000000", "one that fits still goes");
}

#[test]
fn the_last_system_database_named_is_the_one_used() {
    let d = dir("system");
    let mut it = load(&d, "db.open(\"main.db\")\ndb.exec(\"create table t (a int)\")\ndb.system(\"first.db\")\ndb.system(\"second.db\")\n");
    assert!(eval(&mut it, "db.atomic([db], () => db.exec(\"insert into t values (1)\"))").contains("changes"));
    let tables = |f: &str| -> i64 {
        rusqlite::Connection::open(d.join(f)).unwrap().query_row("select count(*) from sqlite_master where type = 'table'", [], |r| r.get(0)).unwrap()
    };
    assert_eq!(tables("first.db"), 0);
    assert!(tables("second.db") > 0);
}

fn printed(d: &Path, src: &str, database: &str) -> String {
    std::fs::write(d.join("app.qwe"), src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg("app.qwe").current_dir(d).env("QUILLO_DB", database).stdin(Stdio::null()).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn a_name_is_quoted_for_the_database_quillo_db_will_open() {
    let d = dir("quillo-db");
    assert_eq!(printed(&d, "print(sql_name(\"orders\"))\n", "mysql://root@127.0.0.1:1/shop"), "`orders`");
    assert_eq!(printed(&d, "print(sql_name(\"orders\"))\n", "shop.db"), "\"orders\"");
}
