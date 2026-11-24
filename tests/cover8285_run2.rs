//! Coverage of the lines of src/db.rs, interp.rs, head.rs, server.rs, builtins.rs, native.rs,
//! mcp.rs and i18n.rs that the full run at 0038a97 left unrun, added by the four fix rounds after
//! the first coverage round: the hold of a run to its `quillo.confirmed` (FEEDBACK83.md, Q-1300 and
//! Q-1301) where SQLite's authorizer, a builtin taken as a value, a page's state and a taken file's
//! calls for the whole site meet it, and `page_head`'s reading of a head's noscript (FEEDBACK84.md,
//! Q-1320) (coverage of feedback82-85; branch feedback82-85). Each test asserts what a developer
//! sees: the answer, the page, the message, the log line, the database's rows. The tests marked
//! `#[ignore = "bug: ..."]` are repros of what these tests found, asserting what should happen.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::{port_in_log, send, temp_dir, Guard};

// ---- helpers, as tests/cover8285_confirm.rs has them --------------------------------------------

/// A test's folder, removed with the record of what its machine built when the test ends.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        let _ = std::fs::remove_dir_all(record_dir(&self.0));
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn dir(name: &str) -> Dir {
    Dir(temp_dir(&format!("cov8285r2-{}", name)))
}

/// Where a test's program keeps the record of what its machine built: beside its folder.
fn record_dir(dir: &Path) -> PathBuf {
    dir.with_file_name(format!("{}-built", dir.file_name().unwrap().to_string_lossy()))
}

const SECRET: &str = "test secret for the tests, 32 bytes or more";

/// `quillo` with `args` in `dir`, `vars` set, ended after `secs`: its exit code and what it wrote.
fn quillo_env(dir: &Path, args: &[&str], secs: u64, vars: &[(&str, &str)]) -> (Option<i32>, String) {
    let log = dir.join("cmd.log");
    let file = std::fs::File::create(&log).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("QUILLO_DB")
        .envs(vars.iter().copied())
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_BUILT_RECORD_DIR", record_dir(dir))
        .env("QUILLO_SECRET", SECRET)
        .stdin(Stdio::null())
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(secs);
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let out = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    (code, out)
}

fn quillo(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    quillo_env(dir, args, 60, &[])
}

/// `quillo run main.qwe` in `d`, a program with no routes, which ends once it has loaded: its exit
/// code and what it printed and logged.
fn run(d: &Path, args: &[&str]) -> (i32, String) {
    let mut all = vec!["run"];
    all.extend_from_slice(args);
    all.push("main.qwe");
    let (code, out) = quillo(d, &all);
    (code.unwrap_or_else(|| panic!("it did not end: {}", out)), out)
}

/// `quillo run main.qwe --port 0` in `dir` with `opts`: the guard, the port, the log.
fn serve(dir: &Path, opts: &[&str]) -> (Guard, u16, PathBuf) {
    let log = dir.join("run.log");
    let file = std::fs::File::create(&log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--port", "0"])
        .args(opts)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("QUILLO_DB")
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_BUILT_RECORD_DIR", record_dir(dir))
        .env("QUILLO_SECRET", SECRET)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(file)
        .spawn()
        .unwrap();
    let mut g = Guard::new(child);
    let port = port_in_log(&mut g.0, &log, Duration::from_secs(60));
    (g, port, log)
}

fn write(dir: &Path, name: &str, text: &str) {
    let p = dir.join(name);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(p, text).unwrap();
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// Waits up to `secs` for `log` to hold `what`.
fn log_has(log: &Path, what: &str, secs: u64) -> bool {
    let until = Instant::now() + Duration::from_secs(secs);
    loop {
        if read(log).contains(what) {
            return true;
        }
        if Instant::now() > until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The names of the tables, views and triggers of the SQLite file `db`, sorted.
fn schema_names(db: &Path) -> Vec<String> {
    let c = rusqlite::Connection::open(db).unwrap();
    let mut s = c.prepare("select name from sqlite_master where type in ('table', 'view', 'trigger') order by name").unwrap();
    s.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

/// The rows of `_quillo_mine` in the SQLite file `db`, in their order.
fn mine_rows(db: &Path) -> Vec<String> {
    let c = rusqlite::Connection::open(db).unwrap();
    let mut s = c.prepare("select x from _quillo_mine order by rowid").unwrap();
    s.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

/// What SQLite's authorizer says when it refuses a taken file's write to one of Quillo's own tables.
const OWN_WRITE: &str = "db: Quillo's own tables (_quillo_...) are not written by a taken file's SQL where a quillo.confirmed holds the run";

/// What the text check says when a taken file's SQL names one of Quillo's own tables.
fn own_named(file: &str) -> String {
    format!("db: Quillo's own tables (_quillo_...) are refused for {}'s SQL where a quillo.confirmed holds the run: they hold what Quillo does (push subscriptions, jobs, tokens), and this statement names one", file)
}

/// The main file of the SQLite tests: Quillo's own table stands in as `_quillo_mine`, with a
/// trigger that writes it after an insert into `users`, a view whose insert writes it, and a
/// trigger that empties it when a kit's row is deleted; then the kit's `steal()`.
const OWN_MAIN: &str = "import kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\ndb.exec(\"create table if not exists users (name text)\")\ndb.exec(\"create table if not exists _quillo_mine (x text)\")\ndb.exec(\"create trigger if not exists audit after insert on users begin insert into _quillo_mine values (new.name); end\")\ndb.exec(\"create view if not exists names as select name from users\")\ndb.exec(\"create trigger if not exists via_view instead of insert on names begin insert into _quillo_mine values ('view:' || new.name); end\")\ndb.exec(\"create table if not exists kits (name text)\")\ndb.exec(\"create trigger if not exists kit_gone after delete on kits begin delete from _quillo_mine; end\")\nprint(\"main: \" + str(db.exec(\"insert into users values ('main')\")))\nprint(\"main alter: \" + str(db.exec(\"alter table users add column age int\")))\nprint(steal())\n";

// ---- src/db.rs authorize: the backstop behind the text check -----------------------------------

/// Under a quillo.confirmed SQLite's authorizer refuses a taken file's statement that writes one
/// of Quillo's own tables where the text names none: an insert that fires the main file's trigger
/// writing one, an insert into a view whose `instead of` trigger writes one, a delete whose trigger
/// empties one, and an insert into one through `db.shards(...).all`, which reads no text; each
/// says why, and nothing lands. The kit's statements that fire nothing of Quillo's run, an `alter
/// table` (a column renamed, a table renamed) too, as the main file's own trigger and `alter table`
/// do (src/db.rs authorize, FEEDBACK83.md Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_taken_files_statement_that_fires_a_write_to_quillos_own_tables_is_refused() {
    let d = dir("backstop");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "main.qwe", OWN_MAIN);
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let said = (r) => is_err(r) ? r.error : \"ran\"\n  let out = []\n  for sql in [\"insert into users (name) values ('kit')\", \"insert into names values ('kit')\", \"insert into kits values ('k')\", \"delete from kits\", \"update users set name = 'x' where name = 'nobody'\", \"alter table users rename column age to years\", \"alter table kits rename to kits2\"] {\n    out = push(out, said(db.exec(sql)))\n  }\n  let s = db.shards([\"data/app.db\"])\n  out = push(out, said(s.all(\"insert into _quillo_mine values ('kit')\")))\n  return \"kit:\\n\" + join(out, \"\\n\")\n}\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/app.db\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("main: { changes: 1, last_id: 1 }\nmain alter: { changes: 0, last_id: null }\n"), "{}", out);
    let want = ["kit:", OWN_WRITE, OWN_WRITE, "ran", OWN_WRITE, "ran", "ran", "ran", OWN_WRITE].join("\n");
    assert!(out.contains(&format!("{}\n", want)), "{}", out);
    assert_eq!(mine_rows(&d.join("data/app.db")), ["main"]);
    assert_eq!(schema_names(&d.join("data/app.db")), ["_quillo_mine", "audit", "kit_gone", "kits2", "names", "users", "via_view"]);
    // The kit's rows: its insert into kits, and nothing of the refused ones.
    let c = rusqlite::Connection::open(d.join("data/app.db")).unwrap();
    let users: Vec<String> = c.prepare("select name from users order by rowid").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(users, ["main"]);
    let years: i64 = c.query_row("select count(*) from pragma_table_info('users') where name = 'years'", [], |r| r.get(0)).unwrap();
    assert_eq!(years, 1);
    let kits: i64 = c.query_row("select count(*) from kits2", [], |r| r.get(0)).unwrap();
    assert_eq!(kits, 1, "the delete was refused whole");
}

/// BUG (found by this coverage work; FEEDBACK83.md Q-1300): SQLite's authorizer judges a statement
/// as it is compiled, and a statement with `?` (`db.exec` with values, every `db.query` and
/// `db.one`) is kept compiled by its text, spaces at its ends taken away (rusqlite's
/// `prepare_cached`). A taken file confirmed for nothing that runs the text the main file ran
/// before gets the main file's compiled statement, never judged for it: its insert fires the main
/// file's trigger and writes `_quillo_mine`, which the same insert written anew (`(?, null)`) is
/// refused. What it should do: judge the statement for the file that runs it, as the one written
/// anew is (coverage of feedback82-85; branch feedback82-85).
#[test]
#[ignore = "bug: a statement kept compiled for the main file is not judged again for a taken file"]
fn a_statement_kept_compiled_for_the_main_file_is_judged_again_for_a_taken_file() {
    let d = dir("cached");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(
        &d,
        "main.qwe",
        &OWN_MAIN.replace("print(steal())", "print(\"main ?: \" + str(db.exec(\"insert into users (name) values (?)\", \"main ?\")))\nprint(\"main q: \" + str(db.query(\"insert into users (name) values (?) returning name\", \"main q\")))\nprint(steal())"),
    );
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let said = (r) => is_err(r) ? r.error : \"ran\"\n  let anew = db.exec(\"insert into users (name, age) values (?, null)\", \"kit anew\")\n  let same = db.exec(\"insert into users (name) values (?)\", \"kit same\")\n  let spaced = db.exec(\"  insert into users (name) values (?)\\n\", \"kit spaced\")\n  let q = db.query(\"insert into users (name) values (?) returning name\", \"kit q\")\n  return \"kit: \" + join([said(anew), said(same), said(spaced), said(q)], \" | \")\n}\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n");
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains(&format!("kit: {}\n", [OWN_WRITE; 4].join(" | "))), "{}", out);
    assert_eq!(mine_rows(&d.join("data/app.db")), ["main", "main ?", "main q"]);
}

/// BUG (found by this coverage work; FEEDBACK83.md Q-1300): `db.shards(...).all(sql)` runs a taken
/// file's SQL with no look at its text (`own_tables_refused`), which `db.query` and the rest have:
/// a kit confirmed to write the program's database file reads `_quillo_mine` (in a real program
/// `_quillo_push`'s endpoints and keys, `_quillo_mcp_tokens`, `_quillo_idempotency`'s answers)
/// and renames its own table into Quillo's (`alter table kk rename to _quillo_kk`), which SQLite's
/// authorizer does not show; the same by `db.query` and `db.exec` is refused. Planted so, a
/// `_quillo_push` row of the kit's endpoint gets the main file's `notification.send`, the attack
/// of Q-1300. What it should do: refuse both as `db.query` does (coverage of feedback82-85; branch
/// feedback82-85).
#[test]
#[ignore = "bug: shards.all reads and renames into Quillo's own tables for a taken file"]
fn shards_all_refuses_quillos_own_tables_to_a_taken_file() {
    let d = dir("shards");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\ndb.exec(\"create table if not exists _quillo_mine (x text)\")\ndb.exec(\"insert into _quillo_mine values ('quillo own row')\")\nprint(steal())\n");
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let said = (r) => is_err(r) ? r.error : str(r)\n  let s = db.shards([\"data/app.db\"])\n  let q = db.query(\"select x from _quillo_mine\")\n  let all = s.all(\"select x from _quillo_mine\")\n  let made = s.all(\"create table if not exists kk (x text)\")\n  let renamed = s.all(\"alter table kk rename to _quillo_kk\")\n  return \"kit:\\n\" + join([said(q), said(all), said(made), said(renamed)], \"\\n\")\n}\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/app.db\"]\n");
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let refused = own_named("kit/k.qwe");
    assert!(out.contains(&format!("kit:\n{}\n{}\n[]\n{}\n", refused, refused, refused)), "{}", out);
    assert!(!out.contains("quillo own row"), "{}", out);
    assert!(!schema_names(&d.join("data/app.db")).contains(&"_quillo_kk".to_string()));
}

/// BUG (found by this coverage work; FEEDBACK83.md Q-1300): on PostgreSQL nothing is behind the
/// text check (`own_table_named`, which looks for `_quillo_`), and PostgreSQL names a table in
/// other spellings: a kit confirmed for nothing reads and writes `_quillo_mine` (in a real program
/// `_quillo_push`, which Quillo makes in the program's database) as `U&"\005Fquillo_mine"`, and
/// writes it from a `do` block whose `execute` puts the name together. The plain name is refused.
/// What it should do: refuse a taken file's SQL that reaches one of Quillo's own tables on
/// PostgreSQL too (coverage of feedback82-85; branch feedback82-85).
#[test]
#[ignore = "bug: on PostgreSQL a taken file reaches Quillo's own tables by another spelling"]
fn on_postgresql_quillos_own_tables_are_refused_to_a_taken_file_in_any_spelling() {
    let Some(pg) = support::postgres("cov8285r2") else {
        eprintln!("PostgreSQL is not installed: skipped");
        return;
    };
    let url = pg.database("own_tables");
    let d = dir("pg-own");
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists _quillo_mine (x text)\")\ndb.exec(\"insert into _quillo_mine values ('quillo own row')\")\nprint(steal())\nprint(\"mine: \" + str(db.query(\"select x from _quillo_mine\")))\n");
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let said = (r) => is_err(r) ? r.error : str(r)\n  let plain = db.query(\"select x from _quillo_mine\")\n  let escaped = db.query(\"select x from U&\\\"\\\\005Fquillo_mine\\\"\")\n  let put = db.exec(\"insert into U&\\\"\\\\005Fquillo_mine\\\" values ('kit')\")\n  let built = db.exec(\"do $$ begin execute 'insert into _qu' || 'illo_mine values (''kit do'')'; end $$\")\n  return \"kit:\\n\" + join([said(plain), said(escaped), said(put), said(built)], \"\\n\")\n}\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = quillo_env(&d, &["run", "main.qwe"], 60, &[("QUILLO_DB", &url)]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains(&format!("kit:\n{}\n", own_named("kit/k.qwe"))), "{}", out);
    assert!(!out.contains("\n[{ x: \"quillo own row\" }]\n"), "the kit read it: {}", out);
    assert!(out.contains("mine: [{ x: \"quillo own row\" }]\n"), "the kit wrote it: {}", out);
}

/// BUG (found by this coverage work; FEEDBACK83.md Q-1300): a connection's functions
/// (`db.connect(...).exec`, `.query`, ..., a `db.shards` value's too), read as its fields, are the
/// builtins themselves, given no stand-in (the map's first value is its number, so it is no module
/// to `carries_any`): handed to another file's helper and called there, `exec` is `db.exec` on the
/// program's database as the helper's file. A kit confirmed for nothing attached `data/lib.db`,
/// which only the helper's file was confirmed to write, and made it; `db.exec` handed so is held to
/// the kit, and its own attach is refused. What it should do: hold a connection's function taken
/// as a value to the file that took it, as `db.exec` is (coverage of feedback82-85; branch
/// feedback82-85).
#[test]
#[ignore = "bug: a connection's function taken as a value is held to the helper's file"]
fn a_connections_function_handed_to_a_helper_is_held_to_the_file_that_took_it() {
    let d = dir("conn-fn");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "lib/config.qwe", "fn apply(f, x) = f(x)\n");
    write(
        &d,
        "kit/k.qwe",
        "use ../lib/config.qwe\ncomponent Badge() { <b>b</b> }\nfn steal() {\n  let said = (r) => is_err(r) ? r.error : str(r)\n  let c = db.connect(\":memory:\")\n  let via = apply(c.exec, \"attach database 'data/lib.db' as l\")\n  let named = apply(db.exec, \"attach database 'data/lib.db' as l2\")\n  let own = db.exec(\"attach database 'data/lib.db' as l3\")\n  return \"kit:\\n\" + join([said(via), said(named), said(own)], \"\\n\")\n}\n",
    );
    write(&d, "main.qwe", "use lib/config.qwe\nimport kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\nprint(steal())\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/app.db\"]\n\n[\"lib/config.qwe\"]\nwrites = [\"data/lib.db\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let refused = "db: data/lib.db is not confirmed for kit/k.qwe (quillo confirm)";
    assert!(out.contains(&format!("kit:\n{}\n{}\n{}\n", refused, refused, refused)), "{}", out);
    assert!(!d.join("data/lib.db").exists(), "the kit made data/lib.db");
}

// ---- src/db.rs, mcp.rs, i18n.rs: a taken file's calls for the whole site, its migrations ------

/// A taken file's `db.open` and `db.system` of a path it was confirmed to write set every file's
/// database, and are refused until confirmed for the site, and its `revoke_mcp_token` (the
/// visitor's) until confirmed for it: `quillo check` says each at its line, `--no-check` gives each
/// call an error that names the file. A migration of its folder that is a link to a file it was not
/// confirmed to read is refused at that file, and nothing is applied (src/db.rs site_refused and
/// migrate, src/mcp.rs revoke_mcp_token, FEEDBACK83.md Q-1300 and Q-1301; coverage of
/// feedback82-85; branch feedback82-85).
#[cfg(unix)]
#[test]
fn a_taken_files_calls_for_the_whole_site_and_a_linked_migration_are_refused() {
    let d = dir("site-calls");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "km/001_a.sql", "create table kk (x int);\n");
    write(&d, "secret/b.sql", "create table stolen (x int);\n");
    std::os::unix::fs::symlink("../secret/b.sql", d.join("km/002_b.sql")).unwrap();
    write(&d, "main.qwe", "import kit/k.qwe use Badge, kit_open, kit_tenants, kit_system, kit_revoke, kit_migrate\ndb.open(\"data/app.db\")\nprint(\"kit open: \" + str(kit_open()))\nprint(\"kit tenants: \" + str(kit_tenants()))\nprint(\"kit system: \" + str(kit_system()))\nprint(\"kit revoke: \" + str(kit_revoke()))\nprint(\"kit migrate: \" + str(kit_migrate() ?? \"refused\"))\nprint(\"main db: \" + str(db.query(\"select count(*) as n from sqlite_master\")))\n");
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nfn kit_open() = db.open(\"data/kit.db\")\nfn kit_system() = db.system(\"data/sys.db\")\nfn kit_revoke() = revoke_mcp_token(\"abc\")\nfn kit_migrate() {\n  let c = db.connect(\"data/kit.db\")\n  return c.migrate(\"km\")\n}\nfn kit_tenants() = db.open(\"data/t/{tenant}.db\")\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/kit.db\", \"data/sys.db\", \"data/t/\"]\nfiles = [\"km/\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    for l in ["kit/k.qwe:4: error: site-wide db.open: not confirmed for this file", "kit/k.qwe:5: error: site-wide db.system: not confirmed for this file", "kit/k.qwe:6: error: visitor's revoke_mcp_token: not confirmed for this file"] {
        assert!(out.contains(l), "{}: {}", l, out);
    }
    let (code, out) = run(&d, &["--no-check"]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("kit open: err(\"db: db.open sets every file's database, and is not confirmed for kit/k.qwe (quillo confirm)\")\n"), "{}", out);
    assert!(out.contains("kit tenants: err(\"db: db.open sets every file's database, and is not confirmed for kit/k.qwe (quillo confirm)\")\n"), "{}", out);
    assert!(!d.join("data/t").exists());
    assert!(out.contains("kit system: err(\"db: db.system sets every file's database, and is not confirmed for kit/k.qwe (quillo confirm)\")\n"), "{}", out);
    assert!(out.contains("kit revoke: err(\"revoke_mcp_token is not confirmed for kit/k.qwe (quillo confirm)\")\n"), "{}", out);
    assert!(out.contains("quillo: kit/k.qwe: revoke_mcp_token is not applied: it is not confirmed for this file"), "{}", out);
    assert!(out.contains("kit migrate: refused\n") && out.contains("quillo: kit/k.qwe: the path secret/b.sql was refused"), "{}", out);
    // The main file's database is the one it opened, and the kit's has no migration in it.
    assert!(out.contains("main db: [{ n: 0 }]\n"), "{}", out);
    assert!(!d.join("data/sys.db").exists());
    assert!(schema_names(&d.join("data/kit.db")).is_empty(), "{:?}", schema_names(&d.join("data/kit.db")));
}

/// A migration whose statement SQLite's authorizer refuses says the hold's words, after its
/// file's name, not SQLite's `not authorized` alone: the main file's `attach` of a file outside
/// what it was confirmed to write stops the start, naming both, and makes nothing there; the same
/// folder of a plain statement is applied as before (src/db.rs migrate_as, FEEDBACK83.md Q-1300;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_migration_the_hold_refuses_says_why_after_its_files_name() {
    let d = dir("migrate-why");
    std::fs::create_dir_all(d.join("data")).unwrap();
    std::fs::create_dir_all(d.join("outside")).unwrap();
    write(&d, "m/001_a.sql", "attach database 'outside/x.db' as o;\n");
    write(&d, "main.qwe", "db.open(\"data/app.db\")\nprint(\"applied: \" + str(db.migrate(\"m\")))\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\nfiles = [\"m/\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let (code, out) = run(&d, &[]);
    assert_ne!(code, 0, "{}", out);
    assert!(out.contains("main.qwe:2: db.migrate: migration 001_a.sql failed: outside/x.db is not confirmed for main.qwe (quillo confirm)"), "{}", out);
    assert!(!out.contains("not authorized") && !out.contains("applied: "), "{}", out);
    assert!(!d.join("outside/x.db").exists());
    // A failure of its own: SQLite's words, as they were.
    write(&d, "m/001_a.sql", "create tabel t (x int);\n");
    let (code, out) = run(&d, &[]);
    assert_ne!(code, 0, "{}", out);
    assert!(out.contains("main.qwe:2: db.migrate: migration 001_a.sql failed: near \"tabel\": syntax error"), "{}", out);
    write(&d, "m/001_a.sql", "create table t (x int);\n");
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("applied: [\"001_a.sql\"]\n"), "{}", out);
}

/// `translations(folder)` reads its folder as the file builtins read a path: under a
/// quillo.confirmed the main file confirmed for nothing is refused it, said at its line, and the
/// start stops; confirmed for the folder, it loads and the page's words are its (src/i18n.rs
/// translations, FEEDBACK83.md Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn translations_of_a_folder_not_confirmed_stops_the_start() {
    let d = dir("translations");
    write(&d, "locales/en.json", "{\"hi\": \"Hello\"}\n");
    write(&d, "main.qwe", "translations(\"locales\")\nprint(\"loaded: \" + t(\"hi\"))\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("main.qwe:1: error: reads locales/: not confirmed for this file"), "{}", out);
    let (code, out) = run(&d, &["--no-check"]);
    assert_eq!(code, 1, "{}", out);
    assert!(out.contains("main.qwe:1: `translations`: locales is not confirmed for main.qwe (quillo confirm)"), "{}", out);
    assert!(!out.contains("loaded"), "{}", out);
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nfiles = [\"locales/\"]\n");
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("loaded: Hello\n"), "{}", out);
}

// ---- src/interp.rs: what a held run reads as a value ----------------------------------------------

/// Under a quillo.confirmed, a field read by its name is the value it always was: a global map's
/// field in a function (`cfg.port`), a module's member it has not (`http.nope`, null) and the map's
/// length (`http.len`), and a builtin's field it has not is an error at its line (`read_file.curry`;
/// `quillo check` says the three). A map of builtins named whole gives each that reaches out its
/// stand-in, and leaves one that reaches nothing as it is: a connection's `exec` in it, handed to
/// another file's helper, is held to the kit (its attach refused) and `math.sqrt` works. A page's
/// state holds no function in a map or an error value, however deep, and starts null, said in the
/// log; one of data with an error in it is kept. A builtin named for the first time by many
/// functions of `parallel` at once is held to their file in each (src/interp.rs field_of_name,
/// named_here, holds_function, named_fn; FEEDBACK83.md Q-1300; coverage of feedback82-85; branch
/// feedback82-85).
#[test]
fn under_a_hold_a_field_read_by_its_name_is_the_value_it_was() {
    let d = dir("fields");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "lib/config.qwe", "fn apply(f, x) = f(x)\n");
    write(
        &d,
        "kit/k.qwe",
        "use ../lib/config.qwe\ncomponent Badge() { <b>b</b> }\nfn kit_map() {\n  let m = { e: db.connect(\":memory:\").exec, s: math.sqrt }\n  let g = m\n  let a = apply(g.e, \"attach database 'data/lib.db' as l\")\n  return str(a) + \" \" + str(g.s(16))\n}\n",
    );
    write(
        &d,
        "main.qwe",
        "use lib/config.qwe\nimport kit/k.qwe use Badge, kit_map\nstate m = { r: read_file }\nstate e = err(read_file)\nstate deep = { a: [{ b: err({ c: [upper] }) }] }\nstate data = { a: [1, { b: err(\"x\") }] }\nlet cfg = { port: 8080 }\nfn port() = cfg.port\nroute GET / = <html><body><p>{str(m)} {str(e)} {str(deep)} {str(data)} {port()} {str(http.nope)} {http.len}</p></body></html>\nroute GET /curry {\n  let _c = read_file.curry\n  return \"x\"\n}\nroute GET /map = kit_map()\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n\n[\"lib/config.qwe\"]\nwrites = [\"data/lib.db\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    for l in ["main.qwe:9: error: `http` has no `nope`", "main.qwe:9: error: `http` has no `len`", "main.qwe:11: error: fn has no field `curry`"] {
        assert!(out.contains(l), "{}: {}", l, out);
    }
    let (g, port, log) = serve(&d, &["--no-check"]);
    let page = send(port, "GET", "/", &[], "");
    assert_eq!(page.status, 200, "{}", read(&log));
    assert!(page.body.contains("<p>null null null { a: [1, { b: err(&quot;x&quot;) }] } 8080 null 7</p>"), "{}", page.body);
    for s in ["m", "e", "deep"] {
        assert!(log_has(&log, &format!("quillo: the page's state `{}`: under quillo.confirmed a page's state holds no function", s), 30), "{}: {}", s, read(&log));
    }
    assert!(!read(&log).contains("the page's state `data`"), "{}", read(&log));
    let curry = send(port, "GET", "/curry", &[], "");
    assert_eq!(curry.status, 500);
    assert!(log_has(&log, "main.qwe:11: fn has no field `curry`", 30), "{}", read(&log));
    // A builtin's error is the visitor's as `internal error`; the log names the file it was held to.
    assert_eq!(send(port, "GET", "/map", &[], "").body, "err(\"internal error\") 4.0");
    assert!(log_has(&log, "quillo: kit/k.qwe: the path data/lib.db was refused: it is not confirmed for this file", 30), "{}", read(&log));
    drop(g);
    assert!(!d.join("data/lib.db").exists());
    // Many functions at once naming builtins for the first time, each held to their file.
    let r = dir("race");
    let mut kit = String::from("use ../lib/config.qwe\nfn race() {\n  let fs = [");
    for i in 0..32 {
        kit.push_str(&format!("{}() => is_err(apply(read_file, \"x\")) ? \"held\" : \"ran\"", if i > 0 { ", " } else { "" }));
    }
    kit.push_str("]\n  return parallel(fs)\n}\n");
    write(&r, "lib/config.qwe", "fn apply(f, x) = f(x)\n");
    write(&r, "kit/k.qwe", &kit);
    write(&r, "x", "X\n");
    write(&r, "main.qwe", "use lib/config.qwe\nimport kit/k.qwe use race\nprint(\"race: \" + join(race(), \" \"))\nprint(\"lib: \" + apply(read_file, \"x\"))\n");
    write(&r, "quillo.confirmed", "[\"main.qwe\"]\nfiles = [\"x\"]\n\n[\"lib/config.qwe\"]\nfiles = [\"x\"]\n");
    let (code, out) = run(&r, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains(&format!("race: {}\nlib: X\n", ["held"; 32].join(" "))), "{}", out);
}

/// Under a quillo.confirmed a scheduled job's run, claimed in Quillo's own table on a worker of its
/// own before any of the program's code there, writes as the main file's code: each tick of
/// `schedule("1s", tick)` is a row, and the job's request is in the log (src/db.rs statements_of,
/// FEEDBACK83.md Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_scheduled_job_runs_as_the_main_files_code_under_a_hold() {
    let d = dir("schedule");
    write(&d, "main.qwe", "db.exec(\"create table ticks (n int)\")\nfn tick() {\n  db.exec(\"insert into ticks values (1)\")\n}\nschedule(\"1s\", tick)\nroute GET /ticks = str(db.one(\"select count(*) as n from ticks\").n)\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let (g, port, log) = serve(&d, &[]);
    assert!(log_has(&log, "POST /_quillo/job/tick?at=", 60), "{}", read(&log));
    let until = Instant::now() + Duration::from_secs(60);
    let n = loop {
        let n: i64 = send(port, "GET", "/ticks", &[], "").body.parse().unwrap();
        if n > 0 || Instant::now() > until {
            break n;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    drop(g);
    assert!(n > 0, "{}", read(&log));
    assert!(!read(&log).contains("not confirmed"), "{}", read(&log));
}

/// A Rust plugin's function taken as a value under a quillo.confirmed is the function itself, held
/// by its SHA-256 and called by its name, never given a stand-in: handed to a helper it answers, a
/// function with a name of 16,400 letters too, past what an entry of the names seen holds (src/
/// interp.rs carries_origin and name_carries_first, FEEDBACK83.md Q-1300; coverage of
/// feedback82-85; branch feedback82-85).
#[test]
fn a_rust_plugins_function_taken_as_a_value_under_a_hold_is_called_as_it_is() {
    let d = dir("native-value");
    let long = "h".repeat(16_400);
    write(&d, "hi.qwer", &format!("//! version: 1.0.0\n//! effects: none\npub fn hi() -> String {{ \"hi\".into() }}\npub fn {}() -> i64 {{ 7 }}\n", long));
    write(&d, "main.qwe", &format!("use \"hi.qwer\"\nlet f = hi\nlet g = {}\nfn call(h) = h()\nprint(\"called: \" + call(f) + \" \" + str(call(g)) + \" \" + str(call(g)))\n", long));
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    assert!(read(&d.join("quillo.confirmed")).contains("[\"hi.qwer\"]\nsha256 = "), "{}", read(&d.join("quillo.confirmed")));
    let (code, out) = quillo_env(&d, &["run", "main.qwe"], 600, &[]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("called: hi 7 7\n"), "{}", out);
}

// ---- src/head.rs: a head's noscript and a meta's attributes --------------------------------------

/// A head that says `pairs`, each at depth 0 in no component, in their order.
fn said_head(pairs: &[(&str, &str)]) -> quillo::head::Head {
    let mut h = quillo::head::Head::default();
    for (k, v) in pairs {
        h.say(quillo::head::KEYS.iter().position(|x| x == k).unwrap(), 0, 0, v.to_string());
        h.called();
    }
    h
}

/// `page` with `h`'s tags in its head.
fn with_head(page: &str, h: &quillo::head::Head) -> String {
    let mut b = page.as_bytes().to_vec();
    quillo::head::apply(&mut b, h);
    String::from_utf8(b).unwrap()
}

/// What `page_head({ title: "T" })` writes.
const TT: &str = "<title>T</title><meta property=\"og:title\" content=\"T\">";

/// What `page_head({ description: "D" })` writes.
const DD: &str = "<meta name=\"description\" content=\"D\"><meta property=\"og:description\" content=\"D\">";

/// A title that closes a head's noscript (as scripting off reads it, which a crawler does) is cut,
/// or left, by what follows it to the noscript's end: a `<` that starts nothing is text, which
/// ends the head either way, and the title is cut; a second title whose text runs past
/// `</noscript>`, or to the page's end, has it cut where no `</head>` came before, and left where
/// one did; a style that runs past `</noscript>` has it left, and one that never ends has it cut; a
/// tag or a comment that holds `</noscript>` in it has it left. Each page's head read as parse5
/// reads it, scripting on and off, holds `page_head`'s title and no other with scripting on
/// (src/head.rs moves, FEEDBACK84.md Q-1320; coverage of feedback82-85; branch feedback82-85).
#[test]
fn what_follows_a_title_that_closes_a_noscript_decides_whether_it_is_cut() {
    let h = said_head(&[("title", "T")]);
    let doc = |head: &str| format!("<!doctype html><html><head>{head}</head><body>b</body></html>");
    for (head, want) in [
        // Text, `<3`: the head ends after the title either way; the title is the head's, cut.
        ("<noscript><title>a</title><3</noscript><title>Old</title>".to_string(), format!("{TT}<noscript><3</noscript>")),
        ("<noscript><title>a</title>< b</noscript><title>Old</title>".to_string(), format!("{TT}<noscript>< b</noscript>")),
        // A second title past `</noscript>`: cut where no `</head>` came first, left where one did.
        ("<noscript><title>a</title><title>b</noscript></title><title>Old</title>".to_string(), format!("{TT}<noscript><title>b</noscript></title>")),
        ("<noscript><title>a</title></head><title>b</noscript></title><title>Old</title>".to_string(), format!("{TT}<noscript><title>a</title></head><title>b</noscript></title>")),
        // A style past `</noscript>`: left.
        ("<noscript><title>a</title><style></noscript></style><title>Old</title>".to_string(), format!("{TT}<noscript><title>a</title><style></noscript></style>")),
        // A meta or a comment that holds `</noscript>`: left (the text after it ends the head).
        ("<noscript><title>a</title><meta content=\"</noscript>\"></noscript><title>Old</title>".to_string(), format!("{TT}<noscript><title>a</title><meta content=\"</noscript>\"></noscript><title>Old</title>")),
        ("<noscript><title>a</title><!-- </noscript> --></noscript><title>Old</title>".to_string(), format!("{TT}<noscript><title>a</title><!-- </noscript> --></noscript><title>Old</title>")),
    ] {
        assert_eq!(with_head(&doc(&head), &h), doc(&want), "{:?}", head);
    }
    // Pages with no `</title>` or `</style>` after the noscript: a title to the page's end, cut
    // (none came before), or left (after `</head>`); a style that never ends, cut.
    for (page, want) in [
        ("<html><head><noscript><title>a</title><title>b</noscript></head><body>b</body></html>", format!("<html><head>{TT}<noscript><title>b</noscript></head><body>b</body></html>")),
        ("<html><head><noscript><title>a</title></head><title>b</noscript></head><body>b</body></html>", format!("<html><head>{TT}<noscript><title>a</title></head><title>b</noscript></head><body>b</body></html>")),
        ("<html><head><noscript><title>a</title><style></noscript></head><body>b</body></html>", format!("<html><head>{TT}<noscript><style></noscript></head><body>b</body></html>")),
    ] {
        assert_eq!(with_head(page, &h), want, "{:?}", page);
    }
}

/// A meta whose last attribute's value runs to its end (`<meta ="x>`, which the browser reads as
/// an attribute named `="x`: a tag ends at its first `>` outside a value) has no name or property,
/// and is left; the title after it is the head's, and cut. A description meta of plain attributes
/// is cut as before (src/head.rs attr_of, FEEDBACK84.md Q-1320; coverage of feedback82-85; branch
/// feedback82-85).
#[test]
fn a_meta_whose_attribute_runs_to_its_end_has_no_name_and_is_left() {
    let h = said_head(&[("title", "T"), ("description", "D")]);
    let doc = |head: &str| format!("<!doctype html><html><head>{head}</head><body>b</body></html>");
    for (head, want) in [
        ("<meta =\"x><title>Old</title>", format!("<meta =\"x>{TT}{DD}")),
        ("<meta content=old =\"x><title>Old</title>", format!("<meta content=old =\"x>{TT}{DD}")),
        ("<meta name=\"description\" content=\"old\"><title>Old</title>", format!("{TT}{DD}")),
    ] {
        assert_eq!(with_head(&doc(head), &h), doc(&want), "{:?}", head);
    }
}

/// BUG (found by this coverage work; FEEDBACK84.md Q-1320): `attr_of` reads `="x` at the start of
/// an attribute as `=` and a quoted value, where the browser (and `tag_close`) reads `="x` as the
/// attribute's name: `<meta ="x name="description" content="old">` is a description meta to the
/// browser (parse5, scripting on and off: `name=description content=old`), and `page_head`'s
/// description leaves it in the head, so the page has two. What it should do: read an attribute
/// as `tag_close` and the browser do, and cut it (coverage of feedback82-85; branch feedback82-85).
#[test]
#[ignore = "bug: a meta whose first attribute starts with = is not read as the browser reads it"]
fn a_meta_whose_attribute_starts_with_an_equals_sign_is_read_as_the_browser_reads_it() {
    let h = said_head(&[("description", "D")]);
    let doc = |head: &str| format!("<!doctype html><html><head>{head}</head><body>b</body></html>");
    assert_eq!(with_head(&doc("<meta =\"x name=\"description\" content=\"old\">"), &h), doc(DD));
}

// ---- src/native.rs private_dir: a folder not this user's alone ----------------------------------

/// A disk image of the FAT file system, mounted where the test says with every folder in it
/// `rwxrwxrwx`, as FAT keeps no modes: detached when it ends.
#[cfg(target_os = "macos")]
struct Fat {
    mount: PathBuf,
}

#[cfg(target_os = "macos")]
impl Fat {
    /// The image at `img`, mounted at `mount`; None where this Mac does not let the test do it.
    fn mount(img: &Path, mount: &Path) -> Option<Fat> {
        let ok = |c: &mut Command| c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        let timed = |args: &[&str]| -> Option<String> {
            let mut c = Command::new(args[0]).args(&args[1..]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
            let until = Instant::now() + Duration::from_secs(60);
            loop {
                if c.try_wait().ok()?.is_some() {
                    break;
                }
                if Instant::now() > until {
                    let _ = c.kill();
                    let _ = c.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let mut out = String::new();
            std::io::Read::read_to_string(c.stdout.as_mut()?, &mut out).ok()?;
            c.wait().ok()?.success().then_some(out)
        };
        let i = img.to_str()?;
        timed(&["hdiutil", "create", "-size", "4m", "-fs", "MS-DOS", "-volname", "QTMP", i])?;
        let attached = timed(&["hdiutil", "attach", "-nomount", i])?;
        let dev = attached.lines().find(|l| l.contains("DOS_FAT"))?.split_whitespace().next()?.to_string();
        std::fs::create_dir_all(mount).ok()?;
        let fat = Fat { mount: mount.to_path_buf() };
        let whole = dev.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('s').to_string();
        if !ok(Command::new("mount").args(["-t", "msdos", "-o", "-m=777", &dev]).arg(mount)) {
            let _ = timed(&["hdiutil", "detach", &whole]);
            return None;
        }
        Some(fat)
    }
}

#[cfg(target_os = "macos")]
impl Drop for Fat {
    fn drop(&mut self) {
        let _ = Command::new("hdiutil").args(["detach", "-force"]).arg(&self.mount).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

/// A pinned plugin is built in a private folder that only this user may enter: where the
/// temporary folder keeps no modes (a FAT disk, where every folder is `rwxrwxrwx`), the folder
/// made is refused, said, and taken away, and nothing is built (src/native.rs private_dir,
/// FEEDBACK82.md Q-1285; coverage of feedback82-85; branch feedback82-85). macOS only: the test
/// mounts a disk image, which it skips where the Mac does not let it.
#[cfg(target_os = "macos")]
#[test]
fn a_pinned_build_is_refused_a_private_folder_others_may_enter() {
    let d = dir("pin-fat");
    let app = d.join("app");
    std::fs::create_dir_all(&app).unwrap();
    let Some(fat) = Fat::mount(&d.join("fat.dmg"), &d.join("mnt")) else {
        eprintln!("no FAT disk image could be mounted here: skipped");
        return;
    };
    let src = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n";
    write(&app, "hi.qwer", src);
    write(&app, "main.qwe", "use \"hi.qwer\"\nprint(hi())\n");
    use sha2::Digest;
    let sha: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|x| format!("{:02x}", x)).collect();
    write(&app, "pin.toml", &format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", sha));
    let t = fat.mount.display().to_string();
    let (code, out) = quillo_env(&app, &["run", "main.qwe", "--no-check", "--policy", "pin.toml"], 120, &[("TMPDIR", &t)]);
    let made: Vec<String> = std::fs::read_dir(&fat.mount).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("quillo-pinned-")).collect()).unwrap_or_default();
    drop(fat);
    assert_eq!(code, Some(1), "{}", out);
    let real = d.join("mnt").canonicalize().unwrap_or_else(|_| d.join("mnt"));
    assert!(out.contains(&format!("the private folder {}/quillo-pinned-", real.display())) && out.contains(" is not this user's alone"), "{}", out);
    assert!(!out.contains("building ") && !out.contains("\nhi\n"), "{}", out);
    assert!(made.is_empty(), "left: {:?}", made);
}
