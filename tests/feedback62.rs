// FEEDBACK62.md: migrations and development, apart from the tool the report studies: a
// table rebuilt in a SQLite migration (Q-915), `--watch` open to the network (Q-918), a new
// migration under `--watch` (Q-919), `platform().dev` (Q-920) and what `quillo check` says a
// failed `db.migrate` gives (Q-923) (branch cache-and-migrate).

mod support;

use quillo::Interp;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    let d = support::temp_dir(&format!("f62-{}", name));
    std::fs::create_dir_all(d.join("migrations")).unwrap();
    d
}

fn load(d: &Path, src: &str) -> Result<String, String> {
    let mut it = Interp::new(d);
    it.load_source(src, "app.qwe").map(|v| v.repr()).map_err(|e| e.to_string())
}

/// `quillo run` of a script: its exit, and what it printed and said.
fn run(d: &Path, src: &str) -> (bool, String, String) {
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).current_dir(d).output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}

fn check(d: &Path, src: &str) -> String {
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

const AUTHORS: &str = "create table authors (id integer primary key, name text);
create table books (id integer primary key, author_id integer references authors(id) on delete cascade, title text);
create table notes (id integer primary key, author_id integer references authors(id) on delete set null, text text);
insert into authors (id, name) values (1, 'Ana'), (2, 'Bo');
insert into books (id, author_id, title) values (1, 1, 'A'), (2, 1, 'B'), (3, 2, 'C');
insert into notes (id, author_id, text) values (1, 2, 'n');
";

/// SQLite's own steps for a table rebuilt, as its page writes them.
const REBUILD: &str = "pragma foreign_keys = off;
create table authors_new (id integer primary key, name varchar(80) not null default '');
insert into authors_new (id, name) select id, coalesce(name, '') from authors;
drop table authors;
alter table authors_new rename to authors;
pragma foreign_keys = on;
";

const ROWS: &str = "db.open(\"app.db\")\ndb.migrate()\n[db.query(\"select id, author_id from books order by id\") | map((r) => r.author_id), db.query(\"select author_id from notes\") | map((r) => r.author_id), db.one(\"pragma foreign_keys\").foreign_keys]";

// Q-915: a file that turns foreign keys off has them off for its run: the rebuild keeps every
// child row (cascade and set null alike) and the database has them on again after.
#[test]
fn a_rebuild_that_turns_foreign_keys_off_keeps_the_rows_pointing_at_it() {
    let d = dir("rebuild");
    std::fs::write(d.join("migrations/001.sql"), AUTHORS).unwrap();
    std::fs::write(d.join("migrations/002.sql"), REBUILD).unwrap();
    assert_eq!(load(&d, ROWS).unwrap(), "[[1, 1, 2], [2], 1]");
    // The rebuilt table is the new one, and its key still holds: a book of no author is refused.
    let after = "db.open(\"app.db\")\n[db.query(\"select sql from sqlite_master where name = 'authors'\")[0].sql, db.exec(\"insert into books (author_id, title) values (9, 'x')\")]";
    let v = load(&d, after).unwrap();
    assert!(v.contains("varchar(80) not null default ''") && v.contains("err(\"db: FOREIGN KEY constraint failed\")"), "{}", v);
    let _ = std::fs::remove_dir_all(&d);
}

// Q-915, edges: how the pragma is written (case, `0`, `false`, `no`, quoted, the `(off)` form, a
// schema named, no spaces), and a file whose pragma is only in a comment or a string, which is
// not taken: its drop cascades as before, with the warning.
#[test]
fn the_pragma_is_read_as_sqlite_reads_it_and_not_in_a_comment_or_a_string() {
    for (i, pragma) in ["PRAGMA FOREIGN_KEYS = OFF;", "pragma foreign_keys=0;", "pragma foreign_keys = false;", "pragma foreign_keys = no;", "pragma foreign_keys = 'off';", "pragma foreign_keys(off);", "pragma main.foreign_keys = off;", "pragma \"foreign_keys\" = off;", "\n\n-- first\npragma   foreign_keys\n=\noff ;"].iter().enumerate() {
        let d = dir(&format!("forms{}", i));
        std::fs::write(d.join("migrations/001.sql"), AUTHORS).unwrap();
        std::fs::write(d.join("migrations/002.sql"), REBUILD.replace("pragma foreign_keys = off;", pragma)).unwrap();
        assert_eq!(load(&d, ROWS).unwrap(), "[[1, 1, 2], [2], 1]", "{}", pragma);
        let _ = std::fs::remove_dir_all(&d);
    }
    for (i, not) in ["-- pragma foreign_keys = off;", "/* pragma foreign_keys = off; */", "select 'pragma foreign_keys = off;';", "pragma foreign_keys = on;", "pragma foreign_keys;"].iter().enumerate() {
        let d = dir(&format!("not{}", i));
        std::fs::write(d.join("migrations/001.sql"), AUTHORS).unwrap();
        std::fs::write(d.join("migrations/002.sql"), REBUILD.replace("pragma foreign_keys = off;", not)).unwrap();
        let (ok, out, err) = run(&d, ROWS.replace("db.migrate()\n[", "db.migrate()\nprint([").replace("foreign_keys]", "foreign_keys])").as_str());
        assert!(ok, "{}", err);
        assert_eq!(out.trim(), "[[], [null], 1]", "{}", not);
        assert!(err.contains("warning: migration 002.sql drops table authors, which books points at with on delete cascade") && err.contains("(and 1 more)") && err.contains("write `pragma foreign_keys = off`"), "{}", err);
        let _ = std::fs::remove_dir_all(&d);
    }
}

// Q-915: a rebuild that leaves a row pointing at nothing fails, rolled back whole, naming the
// table, the row and the parent; the database keeps its keys on, and the next start runs it
// again once it is fixed.
#[test]
fn a_rebuild_that_breaks_a_key_is_rolled_back_and_says_where() {
    let d = dir("broken");
    std::fs::write(d.join("migrations/001.sql"), AUTHORS).unwrap();
    let broken = REBUILD.replace("select id, coalesce(name, '') from authors", "select id, coalesce(name, '') from authors where id = 1");
    std::fs::write(d.join("migrations/002.sql"), &broken).unwrap();
    let e = load(&d, ROWS).unwrap_err();
    assert!(e.contains("migration 002.sql failed: it turns foreign keys off, and leaves books row 3 pointing at no row of authors") && e.contains("rolled back"), "{}", e);
    // `notes`' row too: one named, the rest counted.
    assert!(e.contains("(and 1 more)"), "{}", e);
    let state = "db.open(\"app.db\")\n[db.query(\"select name from _quillo_migrations\") | map((r) => r.name), len(db.query(\"select * from authors\")), db.query(\"select name from sqlite_master where name = 'authors_new'\"), db.one(\"pragma foreign_keys\").foreign_keys]";
    assert_eq!(load(&d, state).unwrap(), "[[\"001.sql\"], 2, [], 1]");
    std::fs::write(d.join("migrations/002.sql"), REBUILD).unwrap();
    assert_eq!(load(&d, ROWS).unwrap(), "[[1, 1, 2], [2], 1]");
    let _ = std::fs::remove_dir_all(&d);
}

// Q-915, the form that must keep working: a file that drops a table nothing points at, or one
// pointed at with no action, says nothing more than before; with no action and rows pointing at
// it the drop still stops the file, foreign keys being on; and a file already applied is not run
// again, its pragma or not.
#[test]
fn files_without_the_pragma_behave_as_before() {
    let d = dir("before");
    std::fs::write(d.join("migrations/001.sql"), "create table a (id integer primary key);\ncreate table b (id integer primary key, a_id integer references a(id));\ninsert into a values (1);\ninsert into b values (1, 1);\ncreate table lone (id integer);\n").unwrap();
    std::fs::write(d.join("migrations/002.sql"), "drop table lone;\n").unwrap();
    let (ok, out, err) = run(&d, "db.open(\"app.db\")\nprint(db.migrate())");
    assert!(ok && out.contains("[\"001.sql\", \"002.sql\"]"), "{}{}", out, err);
    assert!(!err.contains("warning"), "{}", err);
    std::fs::write(d.join("migrations/003.sql"), "drop table a;\n").unwrap();
    let (ok, _, err) = run(&d, "db.open(\"app.db\")\nprint(db.migrate())");
    assert!(!ok && err.contains("003.sql failed") && err.contains("FOREIGN KEY constraint failed") && !err.contains("warning"), "{}", err);
    std::fs::remove_file(d.join("migrations/003.sql")).unwrap();
    std::fs::write(d.join("migrations/002.sql"), "pragma foreign_keys = off;\ndrop table lone;\n").unwrap();
    let (ok, out, err) = run(&d, "db.open(\"app.db\")\nprint(db.migrate())\nprint(db.one(\"pragma foreign_keys\"))");
    assert!(ok && out.contains("[]") && out.contains("{ foreign_keys: 1 }") && err.contains("002.sql changed after it was applied"), "{}{}", out, err);
    let _ = std::fs::remove_dir_all(&d);
}

// Q-915 on PostgreSQL and MySQL: no change; the pragma is SQLite's (their own files keep what
// they did). Checked where a server is at hand.
#[test]
fn other_databases_run_their_files_as_before() {
    let Some(pg) = support::postgres("f62") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let d = dir("pg");
    std::fs::write(d.join("migrations/001.sql"), "create table a (id integer primary key);\ninsert into a values (1);\n").unwrap();
    let src = format!("db.open(\"{}\")\ndb.migrate()", pg.database("f62"));
    assert_eq!(load(&d, &src).unwrap(), "[\"001.sql\"]");
    let _ = std::fs::remove_dir_all(&d);
}

/// A `quillo run` server whose lines go to a file: (child, port, log).
/// The system picks the port and the log says which: a port picked here could be taken by another
/// process first (a shown bug in the test; branch security). A port `args` name (for a first load
/// that fails under --watch, which says no port) is kept, and checked to be this server's.
fn serve_logged(d: &Path, args: &[&str]) -> (support::Guard, u16, PathBuf) {
    let log = d.join("server.log");
    let named = support::port_named(args);
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.arg("run").arg(d.join("main.qwe"));
    if named.is_none() {
        c.args(["--port", "0"]);
    }
    let child = c
        .args(args)
        .env("QUILLO_SECRET", "a test secret for the tests, 32 bytes or more")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    if let Some(port) = named {
        assert!(until(20, || support::listening(port)), "server did not start: {}", std::fs::read_to_string(&log).unwrap_or_default());
        assert!(support::listener_is(port, g.0.id()), "another process listens on {}", port);
        return (g, port, log);
    }
    let port = support::port_in_log(&mut g.0, &log, Duration::from_secs(20));
    (g, port, log)
}

fn until(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    f()
}

const TABLES: &str = "db.open(\"app.db\")\ndb.migrate()\nroute GET /tables = db.query(\"select name from sqlite_master where type = 'table' and name <> '_quillo_migrations' order by name\") | map((r) => r.name) | join(\",\")\n";

// Q-919: under --watch, a migration that arrives (a `git pull`) is applied, with no `.qwe`
// file changed; and a second one after it, the folder watched as it is now.
#[test]
fn a_new_migration_reaches_a_watched_program() {
    let d = dir("watch");
    std::fs::write(d.join("migrations/001_a.sql"), "create table a (id integer);").unwrap();
    std::fs::write(d.join("main.qwe"), TABLES).unwrap();
    let (_g, port, log) = serve_logged(&d, &["--watch"]);
    let tables = || support::send(port, "GET", "/tables", &[], "").body;
    assert_eq!(tables(), "a");
    std::fs::write(d.join("migrations/002_b.sql"), "create table b (id integer);").unwrap();
    assert!(until(20, || tables() == "a,b"), "{}: {}", tables(), std::fs::read_to_string(&log).unwrap_or_default());
    std::fs::write(d.join("migrations/003_c.sql"), "create table c (id integer);").unwrap();
    assert!(until(20, || tables() == "a,b,c"), "{}: {}", tables(), std::fs::read_to_string(&log).unwrap_or_default());
    let said = std::fs::read_to_string(&log).unwrap();
    assert!(said.contains("applied migration 002_b.sql") && said.contains("applied migration 003_c.sql"), "{}", said);
    let _ = std::fs::remove_dir_all(&d);
}

// Q-919, edges: an empty folder, whose first file arrives; a folder that is the program's own
// (`db.migrate(".")`), not watched, where every write to its database would reload it; and
// --reload, where a deploy decides when a migration runs.
#[test]
fn the_folder_is_watched_under_watch_alone_and_never_the_programs_own() {
    let d = dir("empty");
    std::fs::write(d.join("migrations/.gitkeep"), "").unwrap();
    std::fs::write(d.join("main.qwe"), TABLES).unwrap();
    {
        let (_g, port, log) = serve_logged(&d, &["--watch"]);
        let tables = || support::send(port, "GET", "/tables", &[], "").body;
        assert_eq!(tables(), "");
        std::fs::write(d.join("migrations/001_a.sql"), "create table a (id integer);").unwrap();
        assert!(until(20, || tables() == "a"), "{}: {}", tables(), std::fs::read_to_string(&log).unwrap_or_default());
    }
    let r = dir("reload");
    std::fs::write(r.join("migrations/001_a.sql"), "create table a (id integer);").unwrap();
    std::fs::write(r.join("main.qwe"), TABLES).unwrap();
    {
        let (_g, port, _log) = serve_logged(&r, &["--reload"]);
        std::fs::write(r.join("migrations/002_b.sql"), "create table b (id integer);").unwrap();
        std::thread::sleep(Duration::from_millis(2500));
        assert_eq!(support::send(port, "GET", "/tables", &[], "").body, "a");
    }
    let o = dir("own");
    std::fs::write(o.join("001_a.sql"), "create table a (id integer);").unwrap();
    std::fs::write(o.join("main.qwe"), TABLES.replace("db.migrate()", "db.migrate(\".\")") + "route POST /w { db.exec(\"insert into a values (1)\"); \"ok\" }\n").unwrap();
    {
        let (_g, port, log) = serve_logged(&o, &["--watch"]);
        for _ in 0..5 {
            assert_eq!(support::send(port, "POST", "/w", &[("sec-fetch-site", "same-origin")], "").body, "ok");
        }
        std::thread::sleep(Duration::from_millis(1500));
        let said = std::fs::read_to_string(&log).unwrap();
        assert!(!said.contains("reloaded"), "{}", said);
    }
    for x in [d, r, o] {
        let _ = std::fs::remove_dir_all(&x);
    }
}

// Q-918: --watch on an address beyond this machine says so at start, and still serves; on
// loopback (the default) nothing more is said.
#[test]
fn watch_open_to_the_network_says_so() {
    let d = dir("host");
    std::fs::write(d.join("main.qwe"), "route GET / = \"hi\"\n").unwrap();
    {
        let (_g, port, log) = serve_logged(&d, &["--watch", "--host", "0.0.0.0"]);
        assert_eq!(support::send(port, "GET", "/", &[], "").body, "hi");
        let said = std::fs::read_to_string(&log).unwrap();
        assert!(said.contains("warning: --watch listens on 0.0.0.0, not this machine alone") && said.contains("Leave out --host"), "{}", said);
    }
    for args in [&["--watch"][..], &["--watch", "--host", "127.0.0.1"], &["--host", "0.0.0.0"]] {
        let (_g, port, log) = serve_logged(&d, args);
        assert_eq!(support::send(port, "GET", "/", &[], "").body, "hi");
        let said = std::fs::read_to_string(&log).unwrap();
        assert!(!said.contains("--watch listens on"), "{:?}: {}", args, said);
    }
    let _ = std::fs::remove_dir_all(&d);
}

// Q-920: `platform().dev` is true under --watch alone, at the top level too, and false in a
// script and a server without it.
#[test]
fn platform_says_whether_it_runs_in_development() {
    let d = dir("dev");
    let (ok, out, err) = run(&d, "print(platform().dev)");
    assert!(ok && out.trim() == "false", "{}{}", out, err);
    std::fs::write(d.join("main.qwe"), "let at_start = platform().dev\nroute GET / = `${at_start} ${platform().dev}`\n").unwrap();
    for (args, want) in [(&["--watch"][..], "true true"), (&["--watch=reload"], "true true"), (&[], "false false"), (&["--reload"], "false false")] {
        let (_g, port, log) = serve_logged(&d, args);
        assert_eq!(support::send(port, "GET", "/", &[], "").body, want, "{:?}: {}", args, std::fs::read_to_string(&log).unwrap_or_default());
    }
    let _ = std::fs::remove_dir_all(&d);
}

// Q-923: `quillo check` takes `db.migrate` as stopping on failure, as it does: no error value,
// so `.error` on its answer is the error it is for a list; and the request it runs in stops.
#[test]
fn check_says_a_migration_gives_a_list() {
    let d = dir("check");
    let out = check(&d, "db.open(\"app.db\")\nlet m = db.migrate(\"migrations\")\nprint(m.error)\n");
    assert!(out.contains("error"), "{}", out);
    let out = check(&d, "db.open(\"app.db\")\nlet m = db.migrate(\"migrations\")\nprint(len(m))\n");
    assert!(out.contains("ok"), "{}", out);
    std::fs::create_dir_all(d.join("bad")).unwrap();
    std::fs::write(d.join("bad/001.sql"), "insert into nowhere values (1);").unwrap();
    std::fs::write(d.join("main.qwe"), "db.open(\"app.db\")\nroute GET /apply {\n    let m = db.migrate(\"bad\")\n    `ok ${len(m)}`\n}\n").unwrap();
    let (_g, port, _log) = serve_logged(&d, &[]);
    let a = support::send(port, "GET", "/apply", &[], "");
    assert_eq!(a.status, 500, "{}", a.body);
    let _ = std::fs::remove_dir_all(&d);
}

// Q-915, found in review: rows that pointed at nothing before the file ran (made with foreign
// keys off, by the `sqlite3` shell say) are not the file's doing: it applies, and a warning names
// them; a row it leaves pointing at nothing still fails it.
#[test]
fn rows_broken_before_the_file_are_left_and_said() {
    let d = dir("orphans");
    std::fs::write(d.join("migrations/001.sql"), AUTHORS).unwrap();
    assert_eq!(load(&d, "db.open(\"app.db\")\ndb.migrate()").unwrap(), "[\"001.sql\"]");
    let orphan = "db.open(\"app.db\")\ndb.exec(\"pragma foreign_keys = off\")\ndb.exec(\"insert into books (id, author_id, title) values (9, 99, 'lost')\")\ndb.exec(\"pragma foreign_keys = on\")";
    load(&d, orphan).unwrap();
    std::fs::write(d.join("migrations/002.sql"), REBUILD).unwrap();
    let (ok, out, err) = run(&d, "db.open(\"app.db\")\nprint(db.migrate())\nprint(len(db.query(\"select * from books\")))");
    assert!(ok, "{}", err);
    assert_eq!(out, "[\"002.sql\"]\n4\n");
    assert!(err.contains("warning: before migration 002.sql ran, books row 9 already pointed at no row of authors: left as they are"), "{}", err);
    // A third file that breaks a key of its own: refused, naming its row and not the old one.
    std::fs::write(d.join("migrations/003.sql"), "pragma foreign_keys = off;\ndelete from authors where id = 2;\n").unwrap();
    let (ok, _, err) = run(&d, "db.open(\"app.db\")\nprint(db.migrate())");
    assert!(!ok && err.contains("leaves books row 3 pointing at no row of authors") && !err.contains("books row 9 pointing"), "{}", err);
    let _ = std::fs::remove_dir_all(&d);
}

// Q-919, found in review: a migration that stops the first load under --watch is tried again
// once fixed; and the folder of another database's `migrate` is watched too.
#[test]
fn a_broken_first_migration_and_another_databases_folder_are_watched() {
    let d = dir("broken-first");
    std::fs::write(d.join("migrations/001_a.sql"), "create table a (id integer);\ninsert into nowhere values (1);").unwrap();
    std::fs::write(d.join("main.qwe"), TABLES).unwrap();
    {
        // Its first load fails, and then it says no port: one is given (branch security).
        let given = support::free_port().to_string();
        let (_g, port, log) = serve_logged(&d, &["--watch", "--port", &given]);
        let tables = || support::send(port, "GET", "/tables", &[], "").body;
        assert!(tables().contains("The program did not load"), "{}", tables());
        std::fs::write(d.join("migrations/001_a.sql"), "create table a (id integer);").unwrap();
        assert!(until(20, || tables() == "a"), "{}: {}", tables(), std::fs::read_to_string(&log).unwrap_or_default());
    }
    let o = dir("other-db");
    std::fs::create_dir_all(o.join("more")).unwrap();
    std::fs::write(o.join("more/001_x.sql"), "create table x (id integer);").unwrap();
    std::fs::write(o.join("main.qwe"), "db.open(\"app.db\")\nlet other = db.connect(\"other.db\")\nother.migrate(\"more\")\nroute GET /tables = other.query(\"select name from sqlite_master where type = 'table' and name <> '_quillo_migrations' order by name\") | map((r) => r.name) | join(\",\")\n").unwrap();
    {
        let (_g, port, log) = serve_logged(&o, &["--watch"]);
        let tables = || support::send(port, "GET", "/tables", &[], "").body;
        assert_eq!(tables(), "x");
        std::fs::write(o.join("more/002_y.sql"), "create table y (id integer);").unwrap();
        assert!(until(20, || tables() == "x,y"), "{}: {}", tables(), std::fs::read_to_string(&log).unwrap_or_default());
    }
    for x in [d, o] {
        let _ = std::fs::remove_dir_all(&x);
    }
}

// Q-919, found in the second review: under --watch, an editor's new file, empty at first, waits
// until it holds SQL, which then runs; and a folder made after the start is watched too.
#[test]
fn an_empty_new_file_waits_and_a_folder_made_later_is_watched() {
    let d = dir("empty-file");
    std::fs::write(d.join("migrations/001_a.sql"), "create table a (id integer);").unwrap();
    std::fs::write(d.join("main.qwe"), TABLES).unwrap();
    {
        let (_g, port, log) = serve_logged(&d, &["--watch"]);
        let tables = || support::send(port, "GET", "/tables", &[], "").body;
        assert_eq!(tables(), "a");
        std::fs::write(d.join("migrations/002_b.sql"), "").unwrap();
        assert!(until(20, || std::fs::read_to_string(&log).unwrap_or_default().contains("002_b.sql has no statement yet")), "{}", std::fs::read_to_string(&log).unwrap_or_default());
        std::fs::write(d.join("migrations/002_b.sql"), "-- only a comment\n").unwrap();
        std::thread::sleep(Duration::from_millis(500));
        std::fs::write(d.join("migrations/002_b.sql"), "create table b (id integer);").unwrap();
        assert!(until(20, || tables() == "a,b"), "{}: {}", tables(), std::fs::read_to_string(&log).unwrap_or_default());
        assert!(!std::fs::read_to_string(&log).unwrap().contains("changed after it was applied"));
    }
    // No folder at the start: the program does not load; making it, with a file, loads it.
    let n = support::temp_dir("f62-no-folder");
    std::fs::write(n.join("main.qwe"), TABLES).unwrap();
    {
        // Its first load fails, and then it says no port: one is given (branch security).
        let given = support::free_port().to_string();
        let (_g, port, log) = serve_logged(&n, &["--watch", "--port", &given]);
        let tables = || support::send(port, "GET", "/tables", &[], "").body;
        assert!(tables().contains("The program did not load"), "{}", tables());
        std::fs::create_dir_all(n.join("migrations")).unwrap();
        std::fs::write(n.join("migrations/001_a.sql"), "create table a (id integer);").unwrap();
        assert!(until(20, || tables() == "a"), "{}: {}", tables(), std::fs::read_to_string(&log).unwrap_or_default());
    }
    for x in [d, n] {
        let _ = std::fs::remove_dir_all(&x);
    }
}

// Q-915, found in the second review: a rebuild of a table with no integer key numbers its rows
// again, and an orphan there before is still the one there before; in a `without rowid` table, a
// new one is still found.
#[test]
fn old_orphans_are_counted_not_matched_by_row() {
    let d = dir("renumber");
    std::fs::write(d.join("migrations/001.sql"), "create table people (id integer primary key, name text);\ncreate table pets (name text, owner integer references people(id));\ninsert into people values (1, 'a');\ninsert into pets values ('x', 1), ('y', 1), ('z', 1);\n").unwrap();
    assert_eq!(load(&d, "db.open(\"app.db\")\ndb.migrate()").unwrap(), "[\"001.sql\"]");
    load(&d, "db.open(\"app.db\")\ndb.exec(\"pragma foreign_keys = off\")\ndb.exec(\"delete from pets where name = 'y'\")\ndb.exec(\"update pets set owner = 9 where name = 'z'\")\ndb.exec(\"pragma foreign_keys = on\")").unwrap();
    std::fs::write(d.join("migrations/002.sql"), "pragma foreign_keys = off;\ncreate table pets_new (name text not null default '', owner integer references people(id));\ninsert into pets_new select coalesce(name, ''), owner from pets;\ndrop table pets;\nalter table pets_new rename to pets;\n").unwrap();
    let (ok, out, err) = run(&d, "db.open(\"app.db\")\nprint(db.migrate())");
    assert!(ok && out.contains("002.sql"), "{}{}", out, err);
    assert!(err.contains("already pointed at no row of people"), "{}", err);
    let w = dir("without-rowid");
    std::fs::write(w.join("migrations/001.sql"), "create table people (id integer primary key);\ncreate table tags (k text primary key, owner integer references people(id)) without rowid;\ninsert into people values (1), (2);\ninsert into tags values ('a', 1), ('b', 2);\n").unwrap();
    assert_eq!(load(&w, "db.open(\"app.db\")\ndb.migrate()").unwrap(), "[\"001.sql\"]");
    load(&w, "db.open(\"app.db\")\ndb.exec(\"pragma foreign_keys = off\")\ndb.exec(\"update tags set owner = 9 where k = 'a'\")\ndb.exec(\"pragma foreign_keys = on\")").unwrap();
    std::fs::write(w.join("migrations/002.sql"), "pragma foreign_keys = off;\ndelete from people where id = 2;\n").unwrap();
    let (ok, _, err) = run(&w, "db.open(\"app.db\")\nprint(db.migrate())");
    assert!(!ok && err.contains("002.sql failed") && err.contains("pointing at no row of people"), "{}", err);
    for x in [d, w] {
        let _ = std::fs::remove_dir_all(&x);
    }
}

// Q-919, found by CI: a migration that arrives while a reload runs (the one before it being
// applied) is applied after it, where the times read after the reload took it for seen. Made
// sure here by a program that waits after migrating, the next file written in that wait; and a
// database kept in the watched folder, which its own writes and reloads change, does not reload
// the program again and again.
#[test]
fn a_migration_that_arrives_during_a_reload_is_applied_after_it() {
    let d = dir("during");
    std::fs::write(d.join("migrations/001_a.sql"), "create table a (id integer);").unwrap();
    std::fs::write(d.join("main.qwe"), TABLES.replace("db.migrate()\n", "db.migrate()\nsleep(1500)\n")).unwrap();
    let (_g, port, log) = serve_logged(&d, &["--watch"]);
    let tables = || support::send(port, "GET", "/tables", &[], "").body;
    assert_eq!(tables(), "a");
    std::fs::write(d.join("migrations/002_b.sql"), "create table b (id integer);").unwrap();
    // The reload has read the folder and waits: the next file comes now.
    assert!(until(20, || std::fs::read_to_string(&log).unwrap_or_default().contains("applied migration 002_b.sql")), "{}", std::fs::read_to_string(&log).unwrap_or_default());
    std::fs::write(d.join("migrations/003_c.sql"), "create table c (id integer);").unwrap();
    assert!(until(30, || tables() == "a,b,c"), "{}: {}", tables(), std::fs::read_to_string(&log).unwrap_or_default());
    let o = dir("db-inside");
    std::fs::write(o.join("migrations/001_a.sql"), "create table a (id integer);").unwrap();
    std::fs::write(o.join("main.qwe"), TABLES.replace("app.db", "migrations/app.db") + "route POST /w { db.exec(\"insert into a values (1)\"); \"ok\" }\n").unwrap();
    {
        let (_g, port, log) = serve_logged(&o, &["--watch"]);
        for _ in 0..5 {
            assert_eq!(support::send(port, "POST", "/w", &[("sec-fetch-site", "same-origin")], "").body, "ok");
        }
        std::fs::write(o.join("main.qwe"), TABLES.replace("app.db", "migrations/app.db") + "route POST /w { db.exec(\"insert into a values (2)\"); \"ok\" }\n").unwrap();
        assert!(until(20, || std::fs::read_to_string(&log).unwrap_or_default().contains("reloaded")), "{}", std::fs::read_to_string(&log).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(3000));
        let reloads = std::fs::read_to_string(&log).unwrap().matches("reloaded").count();
        assert!(reloads <= 2, "{} reloads: {}", reloads, std::fs::read_to_string(&log).unwrap());
    }
    for x in [d, o] {
        let _ = std::fs::remove_dir_all(&x);
    }
}

// Lines 0.8.6's coverage run did not reach: a pragma of another name is not taken for the foreign
// keys one, and a drop written `if exists` or with its schema is still found and warned of.
#[test]
fn another_pragma_and_drops_written_with_if_exists_or_a_schema() {
    for (i, drop) in ["drop table if exists authors;", "drop table main.authors;", "DROP TABLE IF EXISTS main.\"authors\";"].iter().enumerate() {
        let d = dir(&format!("drops{}", i));
        std::fs::write(d.join("migrations/001.sql"), AUTHORS).unwrap();
        std::fs::write(d.join("migrations/002.sql"), format!("pragma defer_foreign_keys = on;\ncreate table authors_new (id integer primary key, name text);\ninsert into authors_new select id, name from authors;\n{}\nalter table authors_new rename to authors;\n", drop)).unwrap();
        let (ok, out, err) = run(&d, "db.open(\"app.db\")\nprint(db.migrate())\nprint(len(db.query(\"select * from books\")))");
        assert!(ok, "{}", err);
        // Foreign keys stayed on (the pragma was another), so the cascade ran, as warned.
        assert!(out.contains("0"), "{}: {}", drop, out);
        assert!(err.contains("drops table authors, which books points at with on delete cascade"), "{}: {}", drop, err);
        let _ = std::fs::remove_dir_all(&d);
    }
}
