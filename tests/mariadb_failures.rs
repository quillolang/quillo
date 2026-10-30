//! MariaDB failing where the usual path doesn't: a database it won't make, a
//! statement with no rows asked for rows, a server stopped while the program
//! runs. Skipped when MariaDB's server programs are not installed.

mod support;

use quillo::Interp;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

/// One server starting at a time: many at once fail now and then, for the load.
fn in_turn() -> MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock().unwrap_or_else(|e| e.into_inner())
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

#[track_caller]
fn says(got: String, what: &str) {
    assert!(got.contains(what), "wanted {:?} in: {}", what, got);
}

#[test]
fn a_database_the_server_will_not_make_says_why() {
    let _turn = in_turn();
    let Some(m) = support::mariadb("myfail-name") else { return };
    let d = support::temp_dir("myfail-name");
    let mut it = load(&d, "");
    let name = "n".repeat(70);
    says(eval(&mut it, &format!("db.open(\"{}\")", m.url(&name))), "Incorrect database name");
}

#[test]
fn a_statement_with_no_rows_gives_no_rows_when_asked_for_them() {
    let _turn = in_turn();
    let Some(m) = support::mariadb("myfail-no-rows") else { return };
    let d = support::temp_dir("myfail-no-rows");
    let mut it = load(&d, &format!("db.open(\"{}\")\ndb.exec(\"create table t (a int)\")\ndb.exec(\"insert into t values (1)\")\n", m.url("rows")));
    assert_eq!(eval(&mut it, "db.query(\"update t set a = 2\")"), "[]");
    assert_eq!(eval(&mut it, "db.one(\"select a from t\").a"), "2");
}

#[test]
fn a_server_stopped_while_the_program_runs_gives_an_error_value() {
    let _turn = in_turn();
    let Some(m) = support::mariadb("myfail-gone") else { return };
    let d = support::temp_dir("myfail-gone");
    let mut it = load(&d, &format!("db.open(\"{}\")\n", m.url("gone")));
    assert_eq!(eval(&mut it, "db.one(\"select 1 as x\").x"), "1");
    drop(m);
    let got = eval(&mut it, "db.query(\"select 1 as x\")");
    assert!(got.starts_with("err(\"db: "), "{}", got);
    says(got, "Connection refused");
}
