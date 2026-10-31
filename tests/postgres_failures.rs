//! PostgreSQL failing where the usual path doesn't: key files that are no
//! good, no certificates on the system, a statement that fails as it runs on
//! the connections requests share, a value that won't go or won't come back,
//! a server stopped between two requests. Skipped when PostgreSQL's server
//! programs are not installed.

mod support;

use quillo::Interp;
use std::path::Path;
use std::process::{Command, Stdio};

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

const CERT: &str = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";

#[test]
fn a_key_file_that_is_not_there_is_named() {
    let d = support::temp_dir("pgfail-no-key");
    std::fs::write(d.join("c.pem"), CERT).unwrap();
    let mut it = load(&d, "");
    let url = format!("postgres://u@127.0.0.1:1/x?sslcert={}&sslkey=/no/such.key", d.join("c.pem").display());
    says(eval(&mut it, &format!("db.open(\"{}\")", url)), "sslkey /no/such.key");
}

#[test]
fn a_key_that_is_no_key_is_refused_with_its_certificate() {
    let d = support::temp_dir("pgfail-bad-key");
    std::fs::write(d.join("c.pem"), CERT).unwrap();
    std::fs::write(d.join("k.pem"), "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n").unwrap();
    let mut it = load(&d, "");
    let url = format!("postgres://u@127.0.0.1:1/x?sslcert={}&sslkey={}", d.join("c.pem").display(), d.join("k.pem").display());
    says(eval(&mut it, &format!("db.open(\"{}\")", url)), "sslcert and sslkey: ");
}

#[test]
fn certificates_the_system_can_not_read_say_why_there_are_none() {
    let d = support::temp_dir("pgfail-no-cert-file");
    std::fs::write(d.join("app.qwe"), "print(db.open(\"postgres://u@127.0.0.1:1/x?sslrootcert=system\"))\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["run", "app.qwe"]).current_dir(&d).env("SSL_CERT_FILE", "/no/such/certs.pem").env("SSL_CERT_DIR", "/no/such/certs").stdin(Stdio::null()).output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    says(said.to_string(), "sslrootcert=system: no CA certificates found on this system (");
    says(said.to_string(), "/no/such/certs.pem");
}

#[test]
fn outside_a_request_failures_of_several_statements_and_of_values_give_error_values() {
    let Some(pg) = support::postgres("pgfail-values") else { return };
    let d = support::temp_dir("pgfail-values");
    let mut it = load(&d, &format!("db.open(\"{}\")\n", pg.url));
    says(eval(&mut it, "db.exec(\"select 1; select 1 / 0\")"), "division by zero");
    // A number PostgreSQL's json keeps as written, and JSON readers can't hold.
    says(eval(&mut it, "db.query(\"select '1e999'::json as j\")"), "error deserializing column 0: number out of range");
    says(eval(&mut it, "db.one(\"select ?::bigint as n\", 100000000000000000000000000m)"), "cannot send dec 100000000000000000000000000m as int8");
    assert_eq!(eval(&mut it, "db.one(\"select ?::bigint as n\", 7m).n"), "7", "one that fits goes");
}

#[test]
fn a_transaction_over_several_databases_can_not_begin_in_a_transaction() {
    let Some(pg) = support::postgres("pgfail-atomic") else { return };
    let d = support::temp_dir("pgfail-atomic");
    let mut it = load(&d, &format!("db.open(\"{}\")\n", pg.url));
    says(eval(&mut it, "db.transaction(() => db.atomic([db], () => 1))"), "`db.atomic` cannot begin inside a transaction already open on one of its databases");
}

#[test]
fn in_a_request_a_statement_that_fails_as_it_runs_gives_an_error_value() {
    let Some(pg) = support::postgres("pgfail-lanes") else { return };
    let url = pg.database("lanes");
    let d = support::temp_dir("pgfail-lanes");
    let src = format!(
        "db.open(\"{}\")\ndb.exec(\"create table t (a int primary key)\")\ndb.exec(\"insert into t values (1)\")\n\
         route GET /divide = str(db.query(\"select 1 / (a - 1) from t\"))\n\
         route GET /twice = str(db.exec(\"insert into t values (1)\"))\n\
         route GET /rows = str(len(db.query(\"select * from t\")))\n",
        url
    );
    std::fs::write(d.join("app.qwe"), src).unwrap();
    let s = support::serve(&d.join("app.qwe"), &[]);
    let body = |path: &str| support::send(s.port, "GET", path, &[], "").body;
    says(body("/divide"), "division by zero");
    says(body("/twice"), "duplicate key value violates unique constraint");
    assert_eq!(body("/rows"), "1", "the shared connection goes on working");
}

#[test]
fn a_server_gone_before_the_first_request_gives_an_error_value() {
    let Some(pg) = support::postgres("pgfail-gone") else { return };
    let url = pg.database("gone");
    let d = support::temp_dir("pgfail-gone");
    std::fs::write(d.join("app.qwe"), format!("db.open(\"{}\")\nroute GET /rows = str(db.query(\"select 1 as x\"))\nroute GET /up = \"up\"\n", url)).unwrap();
    let s = support::serve(&d.join("app.qwe"), &[]);
    drop(pg);
    says(support::send(s.port, "GET", "/rows", &[], "").body, "Connection refused");
    assert_eq!(support::send(s.port, "GET", "/up", &[], "").body, "up");
}
