//! `db.migrate` when it can't do what it was asked: a folder or a file it
//! can't read, a database it can't write or that is held by someone else, a
//! file that fails only at its commit, and a tenant's database that fails.

mod support;

use quillo::server::HttpRequest;
use quillo::Interp;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn dir(name: &str) -> PathBuf {
    support::temp_dir(&format!("migrate-failures-{}", name))
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

fn says(got: String, what: &str) {
    assert!(got.contains(what), "wanted {:?} in: {}", what, got);
}

/// A path with these permissions until dropped, whatever the test did.
struct Mode(PathBuf);

impl Mode {
    fn set(path: &Path, mode: u32) -> Mode {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        Mode(path.to_path_buf())
    }
}

impl Drop for Mode {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

#[test]
fn a_folder_that_is_not_there_is_named() {
    let d = dir("missing");
    let mut it = load(&d, "db.open(\"app.db\")\n");
    says(eval(&mut it, "db.migrate(\"nowhere\")"), &format!("db.migrate: cannot read {}: No such file or directory", d.join("nowhere").display()));
}

#[test]
fn a_file_where_the_folder_should_be_is_named() {
    let d = dir("a-file");
    std::fs::write(d.join("migrations"), "not a folder").unwrap();
    let mut it = load(&d, "db.open(\"app.db\")\n");
    says(eval(&mut it, "db.migrate()"), "Not a directory");
}

#[test]
fn a_folder_it_may_not_read_is_named() {
    let d = dir("no-read");
    let folder = d.join("migrations");
    std::fs::create_dir(&folder).unwrap();
    let mut it = load(&d, "db.open(\"app.db\")\n");
    let _shut = Mode::set(&folder, 0o000);
    says(eval(&mut it, "db.migrate()"), "Permission denied");
}

#[test]
fn a_migration_it_may_not_read_is_named_and_nothing_runs() {
    let d = dir("no-read-file");
    std::fs::create_dir(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_ok.sql"), "create table one (a int);").unwrap();
    std::fs::write(d.join("migrations/002_shut.sql"), "create table two (a int);").unwrap();
    let mut it = load(&d, "db.open(\"app.db\")\n");
    let shut = Mode::set(&d.join("migrations/002_shut.sql"), 0o000);
    let got = eval(&mut it, "db.migrate()");
    says(got.clone(), "cannot read");
    says(got, "002_shut.sql: Permission denied");
    assert_eq!(eval(&mut it, "len(db.query(\"select name from sqlite_master where name = 'one'\"))"), "0", "no file ran");
    drop(shut);
    assert_eq!(eval(&mut it, "db.migrate()"), r#"["001_ok.sql", "002_shut.sql"]"#);
}

#[test]
fn a_migration_that_is_not_text_is_named() {
    let d = dir("not-text");
    std::fs::create_dir(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_bytes.sql"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    let mut it = load(&d, "db.open(\"app.db\")\n");
    says(eval(&mut it, "db.migrate()"), "001_bytes.sql: stream did not contain valid UTF-8");
}

#[test]
fn a_database_that_can_not_be_written_says_so() {
    let d = dir("read-only");
    std::fs::create_dir(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_t.sql"), "create table t (a int);").unwrap();
    // Already in WAL mode: opening it then needs no write, and the first write is the migration's.
    let made = rusqlite::Connection::open(d.join("app.db")).unwrap();
    made.query_row("pragma journal_mode = wal", [], |_| Ok(())).unwrap();
    made.execute_batch("create table other (a int)").unwrap();
    drop(made);
    let _file = Mode::set(&d.join("app.db"), 0o444);
    let mut it = load(&d, "db.open(\"app.db\")\n");
    says(eval(&mut it, "db.migrate()"), "db.migrate: attempt to write a readonly database");
}

#[test]
fn a_database_another_holds_says_it_is_locked_and_works_once_let_go() {
    let d = dir("locked");
    std::fs::create_dir(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_t.sql"), "create table t (a int);").unwrap();
    let mut it = load(&d, "db.open(\"app.db\")\ndb.exec(\"create table _quillo_migrations (name text primary key, sha256 text not null, applied_at text not null)\")\n");
    let other = rusqlite::Connection::open(d.join("app.db")).unwrap();
    other.execute_batch("begin immediate").unwrap();
    says(eval(&mut it, "db.migrate()"), "db.migrate: database is locked");
    other.execute_batch("rollback").unwrap();
    assert_eq!(eval(&mut it, "db.migrate()"), r#"["001_t.sql"]"#);
}

const FAILS_AT_COMMIT: &str = "create table parents (id integer primary key);\n\
    create table kids (parent int references parents (id) deferrable initially deferred);\n\
    insert into kids values (99);\n";

#[test]
fn a_file_that_fails_at_its_commit_is_reported() {
    let d = dir("commit");
    std::fs::create_dir(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_kids.sql"), FAILS_AT_COMMIT).unwrap();
    let mut it = load(&d, "db.open(\"app.db\")\n");
    says(eval(&mut it, "db.migrate()"), "db.migrate: FOREIGN KEY constraint failed");
}

#[test]
#[ignore = "finding: a migration that fails at its commit stays open: the file isn't rolled back, and the next db.migrate says it can't start a transaction"]
fn a_file_that_fails_at_its_commit_is_rolled_back() {
    let d = dir("commit-rollback");
    std::fs::create_dir(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_kids.sql"), FAILS_AT_COMMIT).unwrap();
    let mut it = load(&d, "db.open(\"app.db\")\n");
    says(eval(&mut it, "db.migrate()"), "FOREIGN KEY constraint failed");
    assert_eq!(eval(&mut it, "len(db.query(\"select name from sqlite_master where name = 'kids'\"))"), "0");
    std::fs::write(d.join("migrations/001_kids.sql"), "create table kids (a int);\n").unwrap();
    assert_eq!(eval(&mut it, "db.migrate()"), r#"["001_kids.sql"]"#);
}

#[test]
fn a_tenants_database_that_fails_to_migrate_names_the_tenant() {
    let d = dir("tenant");
    std::fs::create_dir(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_bad.sql"), "create table broken (;\n").unwrap();
    let src = "db.open(\"data/{tenant}.db\")\ndb.migrate()\nroute POST /signup/:name = str(with_tenant(name, () => db.query(\"select 1 as x\")))\n";
    let mut it = load(&d, src);
    let r = it.handle(&HttpRequest { method: "POST".into(), url: "/signup/acme".into(), headers: vec![("Host".into(), "localhost".into())], body: Vec::new(), remote: "127.0.0.1".into() });
    let text = r.text();
    assert!(text.contains("tenant acme: migration 001_bad.sql failed"), "{} {}", r.status, text);
}
