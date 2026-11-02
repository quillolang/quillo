//! `db.exec_many(sql, rows)`: a statement for each row, all of them or none (branch
//! feedback40-43). PostgreSQL and MariaDB run the same program in tests/postgres.rs and
//! tests/mysql.rs, where an insert goes as many rows a statement.

mod support;

use quillo::check::check_source;
use quillo::Interp;
use std::path::Path;

fn run(src: &str) -> String {
    Interp::new(".").load_source(&format!("db.open(\":memory:\")\n{}", src), "many.qwe").map(|v| v.repr()).unwrap_or_else(|e| panic!("{}", e))
}

#[test]
fn rows_go_in_together_or_not_at_all() {
    assert_eq!(run(support::EXEC_MANY), support::EXEC_MANY_WANT);
}

#[test]
fn on_a_connection_and_after_a_failure_the_database_is_as_it_was() {
    let r = run(r#"
let other = db.connect(":memory:")
other.exec("create table t (n int unique)")
let a = other.exec_many("insert into t values (?)", [[1], [2], [3]])
let b = other.exec_many("insert into t values (?)", [[4], [2]])
[a, b.error, other.one("select count(*) as n from t").n]
"#);
    assert_eq!(r, r#"[{ changes: 3 }, "db: UNIQUE constraint failed: t.n", 3]"#);
}

#[test]
fn what_it_is_given_is_checked() {
    let it = |src: &str| Interp::new(".").load_source(&format!("db.open(\":memory:\")\n{}", src), "many.qwe").map(|v| v.repr()).map_err(|e| e.to_string());
    assert!(it("db.exec_many(\"select 1\", [1, 2])").unwrap_err().contains("row 1 is int, not a list of the statement's values"));
    assert!(it("db.exec_many(\"select 1\")").unwrap_err().contains("takes the SQL and a list of rows"));
    assert!(it("db.exec_many(\"select 1\", [[1]], [[2]])").unwrap_err().contains("takes the SQL and a list of rows"));
    // The checker knows it, and that it needs the db effect.
    let diags = |src: &str| check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>();
    assert!(diags("let n = db.exec_many(\"insert into t values (?)\", [[1]])\n").is_empty());
    assert!(diags("fn f() with time = db.exec_many(\"insert into t values (?)\", [[1]])\n")[0].contains("`db.exec_many` needs the db effect"));
    assert!(diags("let n = db.exec_many(\"insert into t values (?)\", 5)\n")[0].contains("db.exec_many"));
}
