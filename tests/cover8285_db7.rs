//! Edge tests of the fail-closed reader of a taken file's SQL under a `quillo.confirmed` on
//! PostgreSQL and MySQL/MariaDB (FEEDBACK83.md, Q-1300; the user's decision of 2026-11-24, "Refuse
//! text-run SQL"): a NUL, a `create` or an `alter` of what it cannot place, a `create` inside
//! another statement, a `drop` of another kind, an `explain` of what may not run, a MySQL table's
//! engine by a quoted name; SQLite's authorizer of a virtual table; and the refusals of a
//! connection's tenants, migrations and transactions to a file whose connection number now names
//! another file's connection, and of `shards.all` before any shard runs (src/db.rs; branch
//! feedback82-85). Each test asserts what a developer sees: the answer, the message, the rows.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::temp_dir;

// ---- helpers, as tests/cover8285_db5.rs has them -----------------------------------------------

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
    Dir(temp_dir(&format!("cov8285db7-{}", name)))
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
        .env_remove("QUILLO_SYSTEM_DB")
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

/// `quillo check main.qwe` in `d`, which must pass.
fn checked(d: &Path) {
    let (code, out) = quillo_env(d, &["check", "main.qwe"], 120, &[]);
    assert_eq!(code, Some(0), "{}", out);
}

/// `quillo run main.qwe` in `d` with `vars`, a program with no routes, which ends once it has
/// loaded: its exit code and what it printed and logged.
fn run(d: &Path, vars: &[(&str, &str)]) -> (i32, String) {
    let (code, out) = quillo_env(d, &["run", "main.qwe"], 120, vars);
    (code.unwrap_or_else(|| panic!("it did not end: {}", out)), out)
}

fn write(dir: &Path, name: &str, text: &str) {
    let p = dir.join(name);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(p, text).unwrap();
}

/// The first column of each row `sql` gives in the SQLite file `db`, as text, in its order.
fn column(db: &Path, sql: &str) -> Vec<String> {
    let c = rusqlite::Connection::open(db).unwrap();
    let mut s = c.prepare(sql).unwrap();
    s.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

/// A Quillo string of `s`, as written in a `.qwe` file: `\r` and a NUL written as their escapes.
fn lit(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\0', "\\0"))
}

/// A kit taken for `Badge`, whose `steal()` makes each call of `calls` (a function and its SQL),
/// and says what each gave, a line each: the error, or what it gave.
fn kit(calls: &[(&str, &str)]) -> String {
    let lines: Vec<String> = calls.iter().map(|(f, sql)| format!("    said({}({}))", f, lit(sql))).collect();
    format!(
        "component Badge() {{\n  return <b>b</b>\n}}\nfn steal() {{\n  let said = (r) => is_err(r) ? r.error : str(r)\n  return \"kit:\\n\" + join([\n{}\n  ], \"\\n\")\n}}\n",
        lines.join(",\n")
    )
}

const TAIL: &str = "Run it from the main file's SQL, or from a migration the main file runs";

/// What the hold says when a taken file's SQL has the database run SQL from text (or may).
fn runs(what: &str) -> String {
    format!("db: {} has the database run SQL from text, or name a table from text, which is refused for kit/k.qwe's SQL where a quillo.confirmed holds the run: what it does is worked out as it runs, where no reading of the statement sees it, Quillo's own tables too. {}", what, TAIL)
}

/// What the hold says when a taken file's SQL reaches the server's files or another database.
fn reaches(what: &str) -> String {
    format!("db: {} reaches the database server's own files or programs, or another database, which is refused for kit/k.qwe's SQL where a quillo.confirmed holds the run. {}", what, TAIL)
}

/// What the hold says when a taken file's SQL cannot be read for certain.
fn unsure(what: &str) -> String {
    format!("db: this statement has {}, which this reading cannot read for certain as the server would, and what it cannot read is refused for kit/k.qwe's SQL where a quillo.confirmed holds the run. {}", what, TAIL)
}

/// What the hold says when a taken file's statement begins with a word it may not begin with.
fn begins(word: &str) -> String {
    format!("db: this statement begins with `{}`, which a taken file's statement may not begin with where a quillo.confirmed holds the run: it may begin with select, insert, update, delete, with, values, table, replace, explain, create, alter, drop or truncate (kit/k.qwe's SQL). {}", word, TAIL)
}

const NUL: &str = "a NUL character";
const CREATE_UNPLACED: &str = "`create` of what this reading cannot place";
const ALTER_UNPLACED: &str = "`alter` of what this reading cannot place";
const CREATE_INSIDE: &str = "`create` inside another statement";

// ---- PostgreSQL ---------------------------------------------------------------------------------

/// Under a quillo.confirmed on PostgreSQL a taken file's statement the reader cannot place is
/// refused and says so: a NUL in it (at the end, and after a unicode value); `create` alone, of a
/// quoted name (`create "kit_t"`, `create "café"`), and `create or replace index`; `alter` alone
/// and of a quoted name; a `create` inside a `with`; a sequence's function whose `(` is not
/// closed; `drop` alone and of a quoted name, and `drop materialized function`; and what follows
/// `explain (...)`'s options is judged as a statement's first word (`grant`, `execute`, `do`).
/// Run: `explain (costs off) select`, `explain (verbose, costs off) select`, a statement with a `;` at
/// its end, and `drop materialized view`, whose view is gone. `explain` alone, or its options left
/// open, is no refusal: PostgreSQL's syntax error is said. Nothing the kit was refused is made
/// (src/db.rs text_run, creates_ok, text_run_in, plain_arguments, head_refused; FEEDBACK83.md
/// Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn on_postgresql_what_the_reader_cannot_place_is_refused_and_says_so() {
    let Some(pg) = support::postgres("cov8285db7") else {
        eprintln!("PostgreSQL is not installed: skipped");
        return;
    };
    let url = pg.database("cannot_place");
    let d = dir("pg");
    let refused: Vec<(&str, &str, String)> = vec![
        ("db.query", "select 1 as a\0", unsure(NUL)),
        ("db.query", "select 'caf\u{e9} \u{1f600}' as a, '\0' as b", unsure(NUL)),
        ("db.exec", "create", runs(CREATE_UNPLACED)),
        ("db.exec", "create \"kit_t\" (x int)", runs(CREATE_UNPLACED)),
        ("db.exec", "create \"caf\u{e9}\" (x int)", runs(CREATE_UNPLACED)),
        ("db.exec", "create or replace index kit_i on kits (x)", runs("`create or replace index`")),
        ("db.exec", "alter", runs(ALTER_UNPLACED)),
        ("db.exec", "alter \"kits\" add y int", runs(ALTER_UNPLACED)),
        ("db.exec", "with kit_w as (select 1) create table kit_x (x int)", runs(CREATE_INSIDE)),
        ("db.query", "select nextval('kit_seq'", runs("the function nextval given a table's name worked out as it runs")),
        ("db.exec", "drop", of("drop", None)),
        ("db.exec", "drop \"kits\"", of("drop", None)),
        ("db.exec", "drop materialized function kit_f()", begins("drop function")),
        ("db.exec", "explain (costs off) grant select on kits to public", begins("grant")),
        ("db.query", "explain (costs off) execute kit_s", runs("`execute`")),
        ("db.query", "explain (costs off) do $$ begin end $$", runs("`do`")),
    ];
    let ran: Vec<(&str, &str, &str)> = vec![
        ("db.query", "explain (costs off) select * from kits", "[{ \"QUERY PLAN\": \"Seq Scan on kits\" }]"),
        ("db.query", "explain (verbose, costs off) select 1 as one", "[{ \"QUERY PLAN\": \"Result\" }, { \"QUERY PLAN\": \"  Output: 1\" }]"),
        ("db.query", "select 1 as one;", "[{ one: 1 }]"),
        ("db.query", "select '\u{e9}' as two; ", "[{ two: \"\u{e9}\" }]"),
        ("db.exec", "drop materialized view kit_mv", "{ changes: 0, last_id: null }"),
        ("db.query", "explain", "db: syntax error at end of input [42601]"),
        ("db.query", "explain (costs off", "db: syntax error at end of input [42601]"),
    ];
    let mut calls: Vec<(&str, &str)> = refused.iter().map(|(f, s, _)| (*f, *s)).collect();
    calls.extend(ran.iter().map(|(f, s, _)| (*f, *s)));
    write(&d, "kit/k.qwe", &kit(&calls));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists kits (x int primary key)\")\ndb.exec(\"create sequence if not exists kit_seq\")\ndb.exec(\"create materialized view if not exists kit_mv as select 1 as x\")\nprint(steal())\nprint(\"made: \" + str(db.query(\"select relname as n from pg_class where relname in ('kit_t', 'kit_x', 'kit_i', 'kit_mv', 'caf\u{e9}') order by 1\")))\nprint(\"kits: \" + str(db.query(\"select count(*) as n from information_schema.columns where table_name = 'kits'\")))\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    let mut want = vec!["kit:".to_string()];
    want.extend(refused.iter().map(|(_, _, w)| w.clone()));
    want.extend(ran.iter().map(|(_, _, w)| w.to_string()));
    assert!(out.contains(&format!("{}\n", want.join("\n"))), "{}", out);
    assert!(out.contains("made: []\n"), "{}", out);
    assert!(out.contains("kits: [{ n: 1 }]\n"), "{}", out);
}

// ---- MySQL / MariaDB ----------------------------------------------------------------------------

/// Under a quillo.confirmed on MySQL (MariaDB) a taken file's statement the reader cannot place is
/// refused and says so: a NUL; `alter online user`, `alter ignore` of a string and `alter online`
/// alone; `explain` alone and of `explain` (round 6: saying `explain` of what);
/// `explain extended grant` and
/// `explain format = json show` judged by the word after them; a table's engine by a quoted name
/// (`` `connect` ``, `` `CONNECT` `` with its file) or by a string, or none after `engine`; `drop
/// temporary function`, `drop` of a quoted name and alone; a `create` after `explain`. Run: `alter
/// ignore table`, `alter online table` and `alter online ignore table`, whose columns are added;
/// `explain format = json`, `explain extended` and `explain partitions` of a select, and (round 6)
/// `explain (select 1)`; a table whose engine is `` `InnoDB` ``; `drop temporary table if
/// exists`; a statement with a `;` at its end.
/// Nothing the kit was refused is made (src/db.rs text_run, text_run_in, head_refused,
/// table_options_refused; FEEDBACK83.md Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn on_mysql_what_the_reader_cannot_place_is_refused_and_says_so() {
    let Some(my) = support::mariadb("cov8285db7") else {
        eprintln!("MariaDB is not installed: skipped");
        return;
    };
    let url = my.url("cannot_place");
    let d = dir("my");
    let refused: Vec<(&str, &str, String)> = vec![
        ("db.query", "select 1 as a\0", unsure(NUL)),
        ("db.exec", "alter online user root identified by 'x'", runs("`alter user`")),
        ("db.exec", "alter ignore 'kit_t' add y int", runs(ALTER_UNPLACED)),
        ("db.exec", "alter online", runs(ALTER_UNPLACED)),
        ("db.query", "explain", of("explain", None)),
        ("db.query", "explain explain select 1", of("explain", Some("`explain`"))),
        ("db.query", "explain extended grant select on kit_t to root", begins("grant")),
        ("db.query", "explain format = json show tables", begins("show")),
        ("db.exec", "create table kit_c (x int) engine = `connect`", reaches("`engine = connect`")),
        ("db.exec", "create table kit_c (x int) engine = `CONNECT` table_type = DOS file_name = '/etc/hosts'", reaches("`engine = connect`")),
        ("db.exec", "create table kit_c (x int) engine = 'connect'", reaches("`engine = connect`")),
        ("db.exec", "alter table kit_t engine", reaches("`engine = `")),
        ("db.exec", "drop temporary function kit_f", begins("drop function")),
        ("db.exec", "drop `kit_t`", of("drop", None)),
        ("db.exec", "drop", of("drop", None)),
        ("db.query", "explain create table kit_x (x int)", runs(CREATE_INSIDE)),
    ];
    let none = "{ changes: 0, last_id: null }";
    let ran: Vec<(&str, &str, &str)> = vec![
        ("db.exec", "alter ignore table kit_t add column z int", none),
        ("db.exec", "alter online table kit_t add column w int", none),
        ("db.exec", "alter online ignore table kit_t add column v int", none),
        ("db.query", "explain extended select * from kit_t", "[{ id: 1, select_type: \"SIMPLE\", table: \"kit_t\", type: \"ALL\", possible_keys: null, key: null, key_len: null, ref: null, rows: \"1\", filtered: 100.0, Extra: \"\" }]"),
        ("db.query", "explain partitions select * from kit_t", "[{ id: 1, select_type: \"SIMPLE\", table: \"kit_t\", partitions: null, type: \"ALL\", possible_keys: null, key: null, key_len: null, ref: null, rows: \"1\", Extra: \"\" }]"),
        ("db.exec", "create table if not exists kit_e (x int) engine = `InnoDB`", none),
        ("db.exec", "drop temporary table if exists kit_tmp", none),
        ("db.query", "explain (select 1)", "[{ id: 1, select_type: \"SIMPLE\", table: null, type: null, possible_keys: null, key: null, key_len: null, ref: null, rows: null, Extra: \"No tables used\" }]"),
        ("db.query", "select 1 as one;", "[{ one: 1 }]"),
    ];
    let mut calls: Vec<(&str, &str)> = refused.iter().map(|(f, s, _)| (*f, *s)).collect();
    calls.extend(ran.iter().map(|(f, s, _)| (*f, *s)));
    // `explain format = json`: its plan's JSON, asked apart (its costs are the server's).
    calls.push(("db.query", "explain format = json select * from kit_t"));
    write(&d, "kit/k.qwe", &kit(&calls));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists kit_t (x int primary key) engine = InnoDB\")\ndb.exec(\"insert into kit_t values (1)\")\nprint(steal())\nprint(\"cols: \" + str(db.query(\"select column_name as c from information_schema.columns where table_schema = database() and table_name = 'kit_t' order by ordinal_position\")))\nprint(\"tables: \" + str(db.query(\"select table_name as t from information_schema.tables where table_schema = database() order by 1\")))\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    let mut want = vec!["kit:".to_string()];
    want.extend(refused.iter().map(|(_, _, w)| w.clone()));
    want.extend(ran.iter().map(|(_, _, w)| w.to_string()));
    assert!(out.contains(&format!("{}\n[{{ EXPLAIN: \"{{\\n  \\\"query_block\\\": {{\\n", want.join("\n"))), "{}", out);
    assert!(out.contains("\\\"table_name\\\": \\\"kit_t\\\""), "{}", out);
    assert!(out.contains("cols: [{ c: \"x\" }, { c: \"z\" }, { c: \"w\" }, { c: \"v\" }]\n"), "{}", out);
    assert!(out.contains("tables: [{ t: \"kit_e\" }, { t: \"kit_t\" }]\n"), "{}", out);
}

// ---- SQLite's authorizer: virtual tables ----------------------------------------------------------

/// What SQLite's authorizer says when it refuses a taken file's write to one of Quillo's own tables.
const OWN_WRITE: &str = "db: Quillo's own tables (_quillo_...) are not written by a taken file's SQL where a quillo.confirmed holds the run";

/// Under a quillo.confirmed on SQLite a taken file's virtual table is judged by SQLite's authorizer
/// by its name and by the tables its module makes: an FTS5 table of the kit's own is made, written,
/// searched (a unicode word too) and dropped; one named `_quillo` (FTS5) or `_QUILLO` (R*Tree),
/// which no text check sees as one of Quillo's own (no `_quillo_` in it), is refused, as the
/// tables its module makes for it (`_quillo_data`, `_QUILLO_node`) are; `_quillo_v` is refused by
/// the text. The main file's FTS5 table reads for the kit. No `_quillo` table is left (src/db.rs
/// authorize, SQLITE_CREATE_VTABLE, SQLITE_DROP_VTABLE; FEEDBACK83.md Q-1300; coverage of
/// feedback82-85; branch feedback82-85).
#[test]
fn on_sqlite_a_taken_files_virtual_table_is_judged_by_its_name_and_the_tables_it_makes() {
    let d = dir("vtab");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(
        &d,
        "kit/k.qwe",
        &kit(&[
            ("db.exec", "create virtual table kit_v using fts5(body)"),
            ("db.exec", "insert into kit_v (body) values ('quillo holds the run'), ('caf\u{e9} cr\u{e8}me')"),
            ("db.query", "select body from kit_v where kit_v match 'holds'"),
            ("db.query", "select body from kit_v where kit_v match 'caf\u{e9}'"),
            ("db.exec", "drop table kit_v"),
            ("db.exec", "create virtual table _quillo using fts5(a)"),
            ("db.exec", "create virtual table _QUILLO using rtree(id, a, b)"),
            ("db.exec", "create virtual table _quillo_v using fts5(a)"),
            ("db.query", "select body from docs where docs match 'main'"),
        ]),
    );
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\ndb.exec(\"create virtual table if not exists docs using fts5(body)\")\ndb.exec(\"insert into docs (body) values ('the main file')\")\nprint(steal())\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/app.db\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let own_named = "db: Quillo's own tables (_quillo_...) are refused for kit/k.qwe's SQL where a quillo.confirmed holds the run: they hold what Quillo does (push subscriptions, jobs, tokens), and this statement names one";
    // FTS5 writes its `_config` row as it makes the table: SQLite counts it as the statement's
    // change; a virtual table's rows set no rowid SQLite gives as the last one.
    let want = [
        "kit:",
        "{ changes: 1, last_id: null }",
        "{ changes: 2, last_id: null }",
        "[{ body: \"quillo holds the run\" }]",
        "[{ body: \"caf\u{e9} cr\u{e8}me\" }]",
        "{ changes: 0, last_id: null }",
        OWN_WRITE,
        OWN_WRITE,
        own_named,
        "[{ body: \"the main file\" }]",
    ];
    assert!(out.contains(&format!("{}\n", want.join("\n"))), "{}", out);
    let db = d.join("data/app.db");
    assert!(column(&db, "select name from sqlite_master where lower(name) like '\\_quillo%' escape '\\'").is_empty());
    assert!(column(&db, "select name from sqlite_master where name like 'kit%'").is_empty());
}

// ---- a connection whose number now names another file's ------------------------------------------

/// Under a quillo.confirmed a kit's own connection (`db.connect("data/k.db")`, confirmed), whose
/// number the main file's `db.system` again moves onto the main file's connection to a database a
/// function works out for each tenant, is refused that connection's transactions, migrations,
/// tenants and `tenant_exists`, each saying so: the function is the main file's, not the kit's.
/// Its statement is refused by the tenant it runs for none. Unmoved, its transaction and query run
/// and its migration is applied. The main file's same connection is its own (src/db.rs conn_judged,
/// transaction, migrate, per_tenant; FEEDBACK83.md Q-1300; coverage of feedback82-85; branch
/// feedback82-85).
#[test]
fn a_connection_number_moved_onto_another_files_function_connection_is_refused_its_calls() {
    let d = dir("moved");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, open_mine, use_mine\ndb.system(\"data/sys1.db\")\nlet mine = open_mine()\nlet f = db.connect((t) => \"data/t_\" + t + \".db\")\nif env(\"MOVE\") == \"yes\" {\n  db.system(\"data/sys2.db\")\n}\nprint(use_mine(mine, env(\"STEP\")))\nprint(\"main: \" + str(f.transaction(() => \"main ran\")))\n",
    );
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn open_mine() {\n  return db.connect(\"data/k.db\")\n}\nfn use_mine(mine, step) {\n  let said = (r) => is_err(r) ? r.error : str(r)\n  if step == \"migrate\" {\n    return \"applied: \" + str(mine.migrate(\"kit/m\"))\n  }\n  if step == \"tenants\" {\n    return \"tenants: \" + str(mine.tenants())\n  }\n  if step == \"exists\" {\n    return \"exists: \" + str(mine.tenant_exists(\"acme\"))\n  }\n  return \"kit: \" + said(mine.transaction(() => \"kit ran\")) + \" | \" + said(mine.query(\"select 1 as one\"))\n}\n",
    );
    write(&d, "kit/m/001_k.sql", "create table k (x text);\ninsert into k values ('caf\u{e9}');\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/k.db\"]\nfiles = [\"kit/m/\"]\n");
    checked(&d);
    let theirs = "this connection's databases are worked out by a function of the file that opened it, and it is not kit/k.qwe's (quillo confirm)";
    let no_tenant = "db: the database is one per tenant ({tenant} in its target), and this code runs for none: see tenant_from and with_tenant";
    let clean = || {
        for e in std::fs::read_dir(d.join("data")).unwrap().flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    };
    // Moved: each refused, and the kit's own database never reached.
    let (code, out) = run(&d, &[("MOVE", "yes"), ("STEP", "tx")]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains(&format!("kit: db: {} | {}\nmain: main ran\n", theirs, no_tenant)), "{}", out);
    for (step, line, what) in [("migrate", 10, "db.migrate: "), ("tenants", 13, "`db.tenants`: "), ("exists", 16, "`db.tenant_exists`: ")] {
        clean();
        let (code, out) = run(&d, &[("MOVE", "yes"), ("STEP", step)]);
        assert_ne!(code, 0, "{}", out);
        assert!(out.contains(&format!("kit/k.qwe:{}: {}{}\n  in use_mine, called at main.qwe:8\n", line, what, theirs)), "{}", out);
        assert!(!out.contains("applied") && !out.contains("main: "), "{}", out);
        assert!(!d.join("data/k.db").exists() || column(&d.join("data/k.db"), "select name from sqlite_master where name = 'k'").is_empty(), "{}", out);
    }
    // Unmoved: the kit's own connection, its own.
    clean();
    let (code, out) = run(&d, &[("MOVE", "no"), ("STEP", "tx")]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains(&format!("kit: kit ran | [{{ one: 1 }}]\nmain: err({:?})\n", no_tenant)), "{}", out);
    clean();
    let (code, out) = run(&d, &[("MOVE", "no"), ("STEP", "migrate")]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("applied: [\"001_k.sql\"]\n"), "{}", out);
    assert_eq!(column(&d.join("data/k.db"), "select x from k"), ["caf\u{e9}"]);
}

// ---- shards.all ---------------------------------------------------------------------------------

/// Under a quillo.confirmed `db.shards(...).all` with a shard whose database a function works out
/// for each tenant says the error of that shard's connection, with its number (round 6), outside a
/// tenant, before any shard
/// runs the statement: the first shard's insert writes no row. Without a quillo.confirmed the first
/// shard runs it and the second's error is said with its number (src/db.rs shard_all; FEEDBACK83.md
/// Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn shards_all_says_a_shards_error_before_any_shard_runs_where_a_run_is_held() {
    let d = dir("shards");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(
        &d,
        "main.qwe",
        "let s = db.shards([\"data/a.db\", (t) => \"data/s_\" + t + \".db\"])\ndb.connect(\"data/a.db\").exec(\"create table if not exists t (x text)\")\nprint(\"all: \" + str(s.all(\"insert into t values ('caf\u{e9}') returning x\")))\nprint(\"count: \" + str(s.count))\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n");
    checked(&d);
    let no_tenant = "the database is one per tenant ({tenant} in its target), and this code runs for none: see tenant_from and with_tenant";
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains(&format!("all: err(\"db: shard 2: {}\")\ncount: 2\n", no_tenant)), "{}", out);
    assert!(column(&d.join("data/a.db"), "select x from t").is_empty());
    std::fs::remove_file(d.join("quillo.confirmed")).unwrap();
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains(&format!("all: err(\"db: shard 2: {}\")\n", no_tenant)), "{}", out);
    assert_eq!(column(&d.join("data/a.db"), "select x from t"), ["caf\u{e9}"]);
}

// Fix round 6

/// What SQLite's authorizer says when it refuses a taken file's `analyze`.
const ANALYZE: &str = "db: `analyze` is not run by a taken file's SQL where a quillo.confirmed holds the run: the statistics it writes and reads back (sqlite_stat1, sqlite_stat4) hold every table's values, Quillo's own too";

/// What SQLite's authorizer says when it refuses a taken file's trigger.
const TRIGGER: &str = "db: a taken file's SQL makes no trigger where a quillo.confirmed holds the run: its statements would run inside other statements, the main file's too, as theirs";

/// What SQLite's authorizer says when it refuses a taken file's read of a table holding other
/// tables' values.
fn mirrors(table: &str) -> String {
    format!("db: {} is not read by a taken file's SQL where a quillo.confirmed holds the run: it holds other tables' values, Quillo's own too", table)
}

/// What the hold says when a taken file's `drop` or MySQL's `explain` is of what it may not be of.
fn of(head: &str, what: Option<&str>) -> String {
    let may = if head == "drop" { "a table, an index, a view or a sequence, its kind named (`drop table t`)" } else { "a statement it may run: select, insert, update, delete, with, values, table or replace" };
    format!("db: `{}` of {} is refused for kit/k.qwe's SQL where a quillo.confirmed holds the run: a taken file's `{}` is of {}. {}", head, what.unwrap_or("what this reading cannot place"), head, may, TAIL)
}

/// Under a quillo.confirmed on SQLite a taken file's `analyze` is refused, of every table, of one
/// and of a schema, in any letters, and so are its reads of what holds other tables' values:
/// `sqlite_stat1`, `sqlite_stat4` (its sampled index values: Quillo's own rows, which the main
/// file's `analyze` put there), by a `count(*)`, by another spelling and through the main file's
/// view of it, and `dbstat`, as a table and as a function; and its trigger, temporary or not, on the
/// main file's table, whose statements would run as the main file's (a temporary one copied
/// `sqlite_stat4` into the kit's table as the main file's insert ran). Run: the kit's query that SQLite plans by
/// the statistics, `sqlite_master`'s names of them, a string `'sqlite_stat4'`, a unicode value. The
/// main file reads them and runs `analyze` after. Without a quillo.confirmed the same kit runs
/// `analyze` and reads `sqlite_stat4`, as in 0.8.10 (src/db.rs authorize, SQLITE_ANALYZE,
/// SQLITE_READ, SQLITE_CREATE_TRIGGER; FEEDBACK83.md Q-1300, found by the coverage tests; branch
/// feedback82-85).
#[test]
fn on_sqlite_a_taken_files_analyze_and_its_reads_of_the_statistics_are_refused() {
    let d = dir("stat4");
    std::fs::create_dir_all(d.join("data")).unwrap();
    let refused: Vec<(&str, &str, String)> = vec![
        ("db.exec", "analyze", ANALYZE.into()),
        ("db.exec", "analyze kits", ANALYZE.into()),
        ("db.exec", "ANALYZE main", ANALYZE.into()),
        ("db.query", "select tbl, idx, stat from sqlite_stat1", mirrors("sqlite_stat1")),
        ("db.query", "select idx, quote(sample) as s from sqlite_stat4", mirrors("sqlite_stat4")),
        ("db.query", "select count(*) as n from main.SQLITE_STAT4", mirrors("sqlite_stat4")),
        ("db.query", "select * from stats", mirrors("sqlite_stat4")),
        ("db.query", "select name, pgsize from dbstat", mirrors("dbstat")),
        ("db.query", "select count(*) as n from dbstat('main')", mirrors("dbstat")),
        ("db.exec", "create temp trigger kit_tt after insert on kits begin insert into kit_copy select idx, quote(sample) from sqlite_stat4; end", TRIGGER.into()),
        ("db.exec", "create trigger if not exists kit_t after insert on kits begin select 1; end", TRIGGER.into()),
        ("db.exec", "CREATE TEMPORARY TRIGGER kit_t3 BEFORE DELETE ON kits BEGIN SELECT 'caf\u{e9}'; END", TRIGGER.into()),
    ];
    let ran: Vec<(&str, &str, &str)> = vec![
        ("db.query", "select y from kits where y = 'k'", "[{ y: \"k\" }]"),
        ("db.query", "select count(*) as n from sqlite_master where name like 'sqlite_stat%'", "[{ n: 2 }]"),
        ("db.query", "select 'sqlite_stat4' as s", "[{ s: \"sqlite_stat4\" }]"),
        ("db.query", "select y || '\u{e9}' as y from kits order by y", "[{ y: \"k\u{e9}\" }, { y: \"l\u{e9}\" }]"),
    ];
    let mut calls: Vec<(&str, &str)> = refused.iter().map(|(f, s, _)| (*f, *s)).collect();
    calls.extend(ran.iter().map(|(f, s, _)| (*f, *s)));
    write(&d, "kit/k.qwe", &kit(&calls));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\ndb.exec(\"create table if not exists _quillo_mine (x text)\")\ndb.exec(\"create index if not exists _quillo_mine_x on _quillo_mine (x)\")\ndb.exec(\"insert into _quillo_mine values ('push endpoint caf\u{e9}'), ('mcp token hash')\")\ndb.exec(\"create table if not exists kits (y text)\")\ndb.exec(\"create index if not exists kits_y on kits (y)\")\ndb.exec(\"insert into kits values ('k'), ('l')\")\ndb.exec(\"analyze\")\ndb.exec(\"create view if not exists stats as select idx, quote(sample) as s from sqlite_stat4\")\nprint(steal())\nprint(\"main: \" + str(db.query(\"select count(*) > 0 as n from sqlite_stat4 where idx = 'kits_y'\")) + \" \" + str(db.exec(\"analyze\")))\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/app.db\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let mut want = vec!["kit:".to_string()];
    want.extend(refused.iter().map(|(_, _, w)| w.clone()));
    want.extend(ran.iter().map(|(_, _, w)| w.to_string()));
    assert!(out.contains(&format!("{}\nmain: [{{ n: 1 }}] {{ changes: 0, last_id: null }}\n", want.join("\n"))), "{}", out);
    // Quillo's own rows, as `sqlite_stat4` keeps them (`quote(sample)`'s hex of "mcp token" and
    // "push endpoint"), and as written.
    assert!(!out.contains("6D637020746F6B656E") && !out.contains("7075736820656E64706F696E74") && !out.contains("token hash"), "{}", out);
    // Unheld: as in 0.8.10.
    std::fs::remove_file(d.join("data/app.db")).unwrap();
    std::fs::remove_file(d.join("quillo.confirmed")).unwrap();
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(!out.contains("is not r") && out.contains("idx: \"_quillo_mine_x\""), "{}", out);
}

/// Under a quillo.confirmed on MySQL (MariaDB) a column named `engine` is no table option: a
/// taken file's table with one first, one with it among others and keys on it (whole, and its
/// first 3 characters), an index on it, a column added after it, its type changed, and a table made
/// from a query of it are made. The option is judged as before by a name, a string (in other
/// letters, and with a backslash), after a `,` of an `alter`, and as a partition's, which sets
/// the table's; an `engine = ...` inside a column list is refused (MariaDB's syntax error). A
/// string's engine of the list (`'memory'`, `"Aria"`) is made. `explain` of a query in brackets
/// runs, one or two deep; of `(grant ...)` is judged by `grant`, of `(explain ...)` and of `()`
/// refused, saying so; MariaDB's `create or replace` of an index and of a sequence are refused,
/// saying `or replace` (src/db.rs table_options_refused, head_refused, creates_ok; FEEDBACK83.md
/// Q-1300, found by the coverage tests; branch feedback82-85).
#[test]
fn on_mysql_a_column_named_engine_is_no_table_option_and_explain_takes_a_query_in_brackets() {
    let Some(my) = support::mariadb("cov8285db7f") else {
        eprintln!("MariaDB is not installed: skipped");
        return;
    };
    let url = my.url("engine_column");
    let d = dir("myengine");
    let connect = reaches("`engine = connect`");
    let refused: Vec<(&str, &str, String)> = vec![
        ("db.exec", "create table kit_p (x int) partition by range (x) (partition p0 values less than (10) engine = connect)", connect.clone()),
        ("db.exec", "alter table kit_e add column w int, engine connect", connect.clone()),
        ("db.exec", "create table kit_c (x int) engine 'Connect'", connect.clone()),
        ("db.exec", "create table kit_c (x int) engine = 'inno\\db'", reaches("`engine = inno\\db`")),
        ("db.exec", "create table kit_c (x int, engine = connect)", connect.clone()),
        ("db.exec", "create or replace index kit_ei on kit_e (engine)", runs("`create or replace index`")),
        ("db.exec", "create or replace sequence kit_q", runs("`create or replace sequence`")),
        ("db.query", "explain (grant select on kit_e to root)", begins("grant")),
        ("db.query", "explain (explain select 1)", of("explain", Some("`explain`"))),
        ("db.query", "explain ()", of("explain", None)),
    ];
    let none = "{ changes: 0, last_id: null }";
    let plan = "[{ id: 1, select_type: \"SIMPLE\", table: null, type: null, possible_keys: null, key: null, key_len: null, ref: null, rows: null, Extra: \"No tables used\" }]";
    let ran: Vec<(&str, &str, &str)> = vec![
        ("db.exec", "create table kit_e (engine int, y varchar(9))", none),
        ("db.exec", "create table kit_f (x int, engine varchar(9), key (engine), key kit_fk (engine(3)))", none),
        ("db.exec", "create index kit_ei on kit_e (engine)", none),
        ("db.exec", "alter table kit_e add column z int after engine", none),
        ("db.exec", "alter table kit_e modify engine bigint", none),
        ("db.exec", "create table kit_g as select engine, '\u{e9}' as e from kit_e", none),
        ("db.exec", "create table kit_s (x int) engine 'memory'", none),
        ("db.exec", "create table kit_a (x int) engine = \"Aria\"", none),
        ("db.query", "explain (select 1)", plan),
        ("db.query", "explain ((select 1))", plan),
    ];
    let mut calls: Vec<(&str, &str)> = refused.iter().map(|(f, s, _)| (*f, *s)).collect();
    calls.extend(ran.iter().map(|(f, s, _)| (*f, *s)));
    write(&d, "kit/k.qwe", &kit(&calls));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\nprint(steal())\nprint(\"tables: \" + str(db.query(\"select table_name as t, engine as e from information_schema.tables where table_schema = database() order by 1\")))\nprint(\"cols: \" + str(db.query(\"select column_name as c, data_type as t from information_schema.columns where table_schema = database() and table_name = 'kit_e' order by ordinal_position\")))\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    let mut want = vec!["kit:".to_string()];
    want.extend(refused.iter().map(|(_, _, w)| w.clone()));
    want.extend(ran.iter().map(|(_, _, w)| w.to_string()));
    assert!(out.contains(&format!("{}\n", want.join("\n"))), "{}", out);
    assert!(
        out.contains("tables: [{ t: \"kit_a\", e: \"Aria\" }, { t: \"kit_e\", e: \"InnoDB\" }, { t: \"kit_f\", e: \"InnoDB\" }, { t: \"kit_g\", e: \"InnoDB\" }, { t: \"kit_s\", e: \"MEMORY\" }]\n"),
        "{}",
        out
    );
    assert!(out.contains("cols: [{ c: \"engine\", t: \"bigint\" }, { c: \"z\", t: \"int\" }, { c: \"y\", t: \"varchar\" }]\n"), "{}", out);
}

// Fix round 7

/// A kit taken for `Badge`, whose `steal()` makes each call of `calls` (a function, its SQL and the
/// values given after it, as written in Quillo, or none) and says what each gave, a line each:
/// the error, or `ran`.
fn kit_ran(calls: &[(&str, &str, &str)]) -> String {
    let lines: Vec<String> = calls
        .iter()
        .map(|(f, sql, values)| if values.is_empty() { format!("    said({}({}))", f, lit(sql)) } else { format!("    said({}({}, {}))", f, lit(sql), values) })
        .collect();
    format!(
        "component Badge() {{\n  return <b>b</b>\n}}\nfn steal() {{\n  let said = (r) => is_err(r) ? r.error : \"ran\"\n  return \"kit:\\n\" + join([\n{}\n  ], \"\\n\")\n}}\n",
        lines.join(",\n")
    )
}

/// Under a quillo.confirmed on MySQL (MariaDB) a taken file's `engine` is judged by where the
/// server takes a table's options, not by the word before it: for tables named `after`, `modify`
/// and `engine`, `alter table after engine = CSV` (quoted, schema-qualified, after another option,
/// after a `,`, an `alter`'s clause before it), `create table ... engine = csv select` and a
/// partition's `engine`, added or by `partition by`, are refused; a column named `engine` added,
/// changed, renamed and dropped, an index named `engine` on the table `engine`, `alter table
/// engine engine = InnoDB` and `order by` run. Every table is InnoDB after (src/db.rs
/// table_options_refused; FEEDBACK83.md Q-1300, found by the review of a65644e; branch
/// feedback82-85).
#[test]
fn on_mysql_a_tables_engine_is_judged_where_the_server_takes_it_whatever_the_tables_name() {
    let Some(my) = support::mariadb("cov8285db7g") else {
        eprintln!("MariaDB is not installed: skipped");
        return;
    };
    let url = my.url("engine_place");
    let d = dir("myplace");
    let csv = reaches("`engine = csv`");
    let calls: Vec<(&str, &str, String)> = vec![
        ("db.exec", "create table if not exists `after` (x int not null)", "ran".into()),
        ("db.exec", "create table if not exists `modify` (x int not null)", "ran".into()),
        ("db.exec", "create table if not exists engine (x int not null)", "ran".into()),
        ("db.exec", "alter table after engine = CSV", csv.clone()),
        ("db.exec", "alter table `after` engine = CSV", csv.clone()),
        ("db.exec", "alter table modify engine = CSV", csv.clone()),
        ("db.exec", "alter table engine engine = CSV", csv.clone()),
        ("db.exec", "alter table engine_place.after engine CSV", csv.clone()),
        ("db.exec", "alter table after row_format = dynamic engine csv", csv.clone()),
        ("db.exec", "alter table after add column y int not null, engine csv", csv.clone()),
        ("db.exec", "alter table after add column y2 int not null after x, engine = connect", reaches("`engine = connect`")),
        ("db.exec", "create table if not exists modify2 engine = csv select 1 as x", csv.clone()),
        ("db.exec", "alter table after add partition (partition p3 values less than (30) engine = csv)", csv.clone()),
        ("db.exec", "alter table after partition by range (x) (partition p0 values less than (10) engine = csv)", csv.clone()),
        ("db.exec", "alter table engine engine = InnoDB", "ran".into()),
        ("db.exec", "alter table after add column engine int", "ran".into()),
        ("db.exec", "alter table after modify column engine bigint", "ran".into()),
        ("db.exec", "alter table after change engine engine2 int", "ran".into()),
        ("db.exec", "alter table after drop column engine2", "ran".into()),
        ("db.exec", "create index engine on engine (x)", "ran".into()),
        ("db.exec", "alter table after order by x", "ran".into()),
    ];
    let made: Vec<(&str, &str, &str)> = calls.iter().map(|(f, s, _)| (*f, *s, "")).collect();
    write(&d, "kit/k.qwe", &kit_ran(&made));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\nprint(steal())\nprint(\"engines: \" + str(db.query(\"select table_name as t, engine as e from information_schema.tables where table_schema = database() order by 1\")))\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    let mut want = vec!["kit:".to_string()];
    want.extend(calls.iter().map(|(_, _, w)| w.clone()));
    assert!(out.contains(&format!("{}\n", want.join("\n"))), "{}", out);
    assert!(out.contains("engines: [{ t: \"after\", e: \"InnoDB\" }, { t: \"engine\", e: \"InnoDB\" }, { t: \"modify\", e: \"InnoDB\" }]\n"), "{}", out);
}

/// Under a quillo.confirmed on SQLite a taken file makes no virtual table over `dbstat` (temporary
/// or not, the module in capitals or quoted) or `sqlite_dbpage`, judged by the module's name
/// SQLite gives as it compiles the statement; an FTS5 table of its own is made and dropped. A
/// limit, said: the main file's own virtual table over `dbstat` is read by the kit by its name,
/// as SQLite names no module as a table is read (src/db.rs authorize, SQLITE_CREATE_VTABLE;
/// FEEDBACK83.md Q-1300, found by the review of a65644e; branch feedback82-85).
#[test]
fn on_sqlite_a_taken_file_makes_no_virtual_table_over_dbstat() {
    let d = dir("vdbstat");
    std::fs::create_dir_all(d.join("data")).unwrap();
    let over = |m: &str| format!("db: a virtual table over {} is not made by a taken file's SQL where a quillo.confirmed holds the run: it holds other tables' values, Quillo's own too", m);
    let calls: Vec<(&str, &str, String)> = vec![
        ("db.exec", "create virtual table temp.k using dbstat(main)", over("dbstat")),
        ("db.exec", "create virtual table k2 using DBSTAT", over("dbstat")),
        ("db.exec", "create virtual table if not exists temp.\"k3\" using \"dbstat\"(main)", over("dbstat")),
        ("db.query", "select name from k", "db: no such table: k".into()),
        ("db.exec", "create virtual table k4 using sqlite_dbpage", over("sqlite_dbpage")),
        ("db.exec", "create virtual table k5 using fts5(caf\u{e9})", "ran".into()),
        ("db.exec", "drop table k5", "ran".into()),
        ("db.query", "select count(*) > 0 as n from pages where name = 'kits'", "ran".into()),
    ];
    let made: Vec<(&str, &str, &str)> = calls.iter().map(|(f, s, _)| (*f, *s, "")).collect();
    write(&d, "kit/k.qwe", &kit_ran(&made));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\ndb.exec(\"create table if not exists kits (x text)\")\ndb.exec(\"create virtual table if not exists pages using dbstat(main)\")\nprint(steal())\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/app.db\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let mut want = vec!["kit:".to_string()];
    want.extend(calls.iter().map(|(_, _, w)| w.clone()));
    assert!(out.contains(&format!("{}\n", want.join("\n"))), "{}", out);
    assert_eq!(column(&d.join("data/app.db"), "select name from sqlite_master where name like 'k%' order by 1"), ["kits"]);
}

/// The SQL a kit ordinarily writes, each statement run from a taken file under a quillo.confirmed
/// and none refused: tables with defaults, checks, foreign keys and generated columns, indexes
/// (unique, on an expression, GIN), inserts of several rows, upserts, `returning`, updates and
/// deletes with a join or a subquery, joins with `group by` and `having`, CTEs (recursive too),
/// window functions, JSON, dates, `like`/`ilike` with values, `alter table` (columns, a
/// constraint, a rename), views, `explain`, sequences, a full-text table, `truncate`, unicode
/// values; on SQLite, PostgreSQL and MySQL (MariaDB), each its own line. And SQLite's pragmas a
/// kit may use to read (`table_info`, `table_xinfo`, `index_info`, `index_xinfo`, `table_list`,
/// `foreign_keys`, `journal_mode` quoted too, `foreign_key_list`, `foreign_key_check`,
/// `index_list`, `user_version` schema-qualified too, `integrity_check(5)`, `quick_check(t)`,
/// `compile_options`, `database_list`, `function_list`, `data_version`, `schema_version`,
/// `wal_checkpoint`, `pragma_index_list()`) run; `pragma optimize`, which runs `analyze` here, is
/// refused as `analyze` is. Round 8: the settings this test ran (`user_version = 3`,
/// `synchronous`, `busy_timeout`, `foreign_keys = off`, `journal_mode = delete`) are refused now,
/// the decision of round 8, and are in the test after it (src/db.rs text_run, head_refused,
/// creates_ok, table_options_refused, authorize; FEEDBACK83.md Q-1300, the review of a65644e;
/// branch feedback82-85).
#[test]
fn the_sql_a_kit_ordinarily_writes_runs_under_a_hold_on_each_database() {
    let sqlite: Vec<(&str, &str, &str)> = vec![
        ("db.exec", "create table if not exists k_users (id integer primary key, name text not null, email text unique, created text default (datetime('now')), age int check (age >= 0))", ""),
        ("db.exec", "create table if not exists k_posts (id integer primary key autoincrement, user_id int references k_users(id) on delete cascade, title text, body text, size real generated always as (length(title) * 1.0) virtual)", ""),
        ("db.exec", "create index if not exists k_posts_user on k_posts (user_id)", ""),
        ("db.exec", "create unique index if not exists k_users_lname on k_users (lower(name))", ""),
        ("db.exec", "insert into k_users (name, email, age) values ('ann', 'ann@x', 30), ('bob', 'bob@x', 40)", ""),
        ("db.exec", "insert into k_users (name, email, age) values ('ann2', 'ann@x', 31) on conflict (email) do update set name = excluded.name", ""),
        ("db.exec", "insert or ignore into k_users (name, email) values ('ann2', 'ann@x')", ""),
        ("db.exec", "insert into k_posts (user_id, title, body) select id, 'hello ' || name, 'caf\u{e9}' from k_users", ""),
        ("db.query", "insert into k_users (name, email, age) values (?, ?, ?) returning id", "\"cy\", \"cy@x\", 5"),
        ("db.exec", "update k_users set age = age + 1 where name = :n", "{n: \"bob\"}"),
        ("db.query", "select u.name, count(p.id) as n from k_users u left join k_posts p on p.user_id = u.id group by u.name having count(p.id) >= 0 order by u.name", ""),
        ("db.query", "with recent as (select * from k_posts order by id desc limit 5) select count(*) as n from recent", ""),
        ("db.query", "with recursive c(x) as (select 1 union all select x + 1 from c where x < 5) select sum(x) as s from c", ""),
        ("db.query", "select name, row_number() over (order by age desc) as r, avg(age) over () as a, lag(name) over (order by id) as prev from k_users order by r", ""),
        ("db.query", "select json_object('name', name, 'age', age) as j from k_users order by id limit 1", ""),
        ("db.query", r#"select json_extract('{"a":[1,2,3]}', '$.a[1]') as v, '{"a":1}' ->> '$.a' as w"#, ""),
        ("db.query", "select value from json_each('[1,2,3]') order by value", ""),
        ("db.query", "select * from k_users where name like ? order by id limit ? offset ?", "\"a%\", 10, 0"),
        ("db.query", "select coalesce(null, 'x') as c, ifnull(null, 1) as i, cast('12' as integer) as n, strftime('%Y', '2026-01-02') as y, upper('\u{e9}') as e", ""),
        ("db.query", "select name from k_users where id in (select user_id from k_posts) and exists (select 1 from k_posts where k_posts.user_id = k_users.id)", ""),
        ("db.query", "select case when age > 30 then 'old' else 'young' end as c, count(*) filter (where age > 0) as n from k_users group by 1", ""),
        ("db.exec", "alter table k_users add column bio text default ''", ""),
        ("db.exec", "alter table k_users rename column bio to about", ""),
        ("db.exec", "alter table k_posts rename to k_posts2", ""),
        ("db.exec", "alter table k_posts2 rename to k_posts", ""),
        ("db.query", "explain query plan select * from k_posts where user_id = 1", ""),
        ("db.query", "explain select 1", ""),
        ("db.exec", "delete from k_posts where id in (select id from k_posts where title like 'hello b%')", ""),
        ("db.exec", "create view if not exists k_named as select name from k_users", ""),
        ("db.query", "select * from k_named", ""),
        ("db.exec", "drop view if exists k_named", ""),
        ("db.exec", "create temp table if not exists k_tmp (x int)", ""),
        ("db.exec", "insert into k_tmp values (1)", ""),
        ("db.exec", "replace into k_users (id, name, email, age) values (1, 'ann', 'ann@x', 30)", ""),
        ("db.exec", "create virtual table if not exists k_fts using fts5(body)", ""),
        ("db.exec", "insert into k_fts (body) values ('quillo holds the run')", ""),
        ("db.query", "select body from k_fts where k_fts match 'holds'", ""),
        ("db.exec", "drop table if exists k_fts", ""),
        ("db.exec", "create table if not exists k_kv (k text primary key, v text) without rowid", ""),
        ("db.exec", "create table if not exists k_strict (id integer primary key, n int) strict", ""),
        ("db.query", "select group_concat(name, ',') as names from (select name from k_users order by name)", ""),
        ("db.query", "select printf('%05d', 42) as p, substr('quillo', 1, 3) as s, instr('quillo', 'll') as i, replace('a-b', '-', '+') as r", ""),
        ("db.exec", "drop index if exists k_posts_user", ""),
        ("db.exec", "vacuum", ""),
        ("db.query", "pragma table_info(k_users)", ""),
        ("db.query", "select name from pragma_table_info('k_users') order by cid", ""),
        ("db.query", "pragma foreign_keys", ""),
        ("db.query", "pragma journal_mode", ""),
        ("db.query", "pragma foreign_key_list(k_posts)", ""),
        ("db.query", "pragma index_list(k_users)", ""),
        ("db.query", "pragma user_version", ""),
        ("db.query", "pragma integrity_check", ""),
        ("db.query", "pragma compile_options", ""),
        ("db.query", "pragma database_list", ""),
        // Round 8: more of the reads a kit may use (its settings are refused: see below).
        ("db.query", "pragma table_xinfo(k_users)", ""),
        ("db.query", "pragma index_info(k_users_lname)", ""),
        ("db.query", "pragma index_xinfo(k_users_lname)", ""),
        ("db.query", "pragma table_list", ""),
        ("db.query", "pragma table_list(k_users)", ""),
        ("db.query", "pragma foreign_key_check", ""),
        ("db.query", "pragma foreign_key_check(k_posts)", ""),
        ("db.query", "pragma integrity_check(5)", ""),
        ("db.query", "pragma quick_check(k_users)", ""),
        ("db.query", "pragma main.user_version", ""),
        ("db.query", "pragma \"journal_mode\"", ""),
        ("db.query", "select name from pragma_index_list('k_users')", ""),
        ("db.query", "pragma function_list", ""),
        ("db.query", "pragma data_version", ""),
        ("db.query", "pragma schema_version", ""),
        ("db.query", "pragma wal_checkpoint", ""),
    ];
    let pg: Vec<(&str, &str, &str)> = vec![
        ("db.exec", "create table if not exists k_users (id serial primary key, name text not null, email text unique, created timestamptz default now(), age int check (age >= 0), meta jsonb default '{}')", ""),
        ("db.exec", "create table if not exists k_posts (id bigint generated always as identity primary key, user_id int references k_users(id) on delete cascade, title text, words int generated always as (length(title)) stored)", ""),
        ("db.exec", "create index if not exists k_posts_user on k_posts (user_id)", ""),
        ("db.exec", "create unique index if not exists k_users_lower on k_users (lower(name))", ""),
        ("db.exec", "create index if not exists k_users_meta on k_users using gin (meta)", ""),
        ("db.exec", r#"insert into k_users (name, email, age, meta) values ('ann', 'ann@x', 30, '{"a": 1}'), ('bob', 'bob@x', 40, '{"b": 2}')"#, ""),
        ("db.exec", "insert into k_users (name, email, age) values ('ann2', 'ann@x', 31) on conflict (email) do update set name = excluded.name", ""),
        ("db.exec", "insert into k_users (name, email) values ('ann', 'ann@x') on conflict do nothing", ""),
        ("db.query", "insert into k_posts (user_id, title) select id, 'hello ' || name from k_users returning id", ""),
        ("db.query", "update k_users set age = age + 1 where name = :n returning age", "{n: \"bob\"}"),
        ("db.query", "select u.name, count(p.id) as n from k_users u left join k_posts p on p.user_id = u.id group by u.name having count(p.id) >= 0 order by u.name", ""),
        ("db.query", "with recent as (select * from k_posts order by id desc limit 5) select count(*) as n from recent", ""),
        ("db.query", "with recursive c(x) as (select 1 union all select x + 1 from c where x < 5) select sum(x) as s from c", ""),
        ("db.query", "select name, rank() over (partition by age > 30 order by id) as r, lag(name) over (order by id) as prev from k_users order by id", ""),
        ("db.query", r#"select meta->>'a' as a, meta ? 'a' as has, meta @> '{"a": 1}' as c, jsonb_build_object('n', name) as j from k_users order by id"#, ""),
        ("db.query", "select jsonb_agg(name order by name) as names, json_build_array(1, 2) as a, to_jsonb(array[1,2]) as b from k_users", ""),
        ("db.query", "select now()::date = current_date as today, extract(year from now()) > 2000 as y, date_trunc('day', now()) <= now() as d", ""),
        ("db.query", "select * from generate_series(1, 3) g", ""),
        ("db.query", "select string_agg(name, ',' order by name) as s, array_agg(id) as ids from k_users", ""),
        ("db.query", "select coalesce(null, 1) as c, greatest(1, 2) as g, nullif(1, 1) as n, md5('x') as m, to_char(42, '000') as t", ""),
        ("db.query", "select count(*) filter (where age > 30) as old from k_users", ""),
        ("db.query", "select name from k_users where name = any(?)", "[\"ann\", \"bob\"]"),
        ("db.query", "select name from k_users where name ilike ? order by id limit ? offset ?", "\"a%\", 5, 0"),
        ("db.query", "select exists(select 1 from k_users) as e, gen_random_uuid() is not null as u", ""),
        ("db.exec", "alter table k_users add column bio text default ''", ""),
        ("db.exec", "alter table k_users alter column bio set default 'x'", ""),
        ("db.exec", "alter table k_users add constraint k_age check (age < 200)", ""),
        ("db.exec", "alter table k_users rename column bio to about", ""),
        ("db.query", "explain select * from k_posts where user_id = 1", ""),
        ("db.query", "explain (costs off) select 1", ""),
        ("db.query", "explain analyze select 1", ""),
        ("db.exec", "delete from k_posts where user_id in (select id from k_users where name = 'nobody')", ""),
        ("db.exec", "create or replace view k_v as select name from k_users", ""),
        ("db.query", "select * from k_v", ""),
        ("db.exec", "drop view if exists k_v", ""),
        ("db.exec", "create materialized view if not exists k_mv as select count(*) as n from k_users", ""),
        ("db.exec", "create sequence if not exists k_seq", ""),
        ("db.query", "select nextval('k_seq') as n", ""),
        ("db.exec", "create table if not exists k_like (like k_users including all)", ""),
        ("db.exec", "create table if not exists k_parted (id int, at date) partition by range (at)", ""),
        ("db.exec", "truncate table k_posts restart identity cascade", ""),
        ("db.query", "select x::numeric(10, 2) as n, '2026-01-02'::date + interval '1 day' as d from (values (1.234)) v(x)", ""),
        ("db.query", "select name from k_users order by id for update skip locked", ""),
        ("db.query", "select distinct on (age) age, name from k_users order by age, name", ""),
        ("db.query", "select age, sum(age) over w as s from k_users window w as (order by id)", ""),
        ("db.exec", "insert into k_users (name, email, age) values (?, ?, ?)", "\"d\u{e9}e\", \"d@x\", 7"),
        ("db.query", "select 'a''b' as q, $$dollar$$ as d, E'tab\\there' as e", ""),
        ("db.exec", "drop table if exists k_like", ""),
    ];
    let mysql: Vec<(&str, &str, &str)> = vec![
        ("db.exec", "create table if not exists k_users (id int auto_increment primary key, name varchar(50) not null, email varchar(80) unique, created timestamp default current_timestamp, age int check (age >= 0), meta json) engine=InnoDB default charset=utf8mb4", ""),
        ("db.exec", "create table if not exists k_posts (id int auto_increment primary key, user_id int, title varchar(100), words int as (char_length(title)) stored, foreign key (user_id) references k_users(id) on delete cascade, index (user_id)) engine = InnoDB", ""),
        ("db.exec", "create index k_posts_title on k_posts (title(10))", ""),
        ("db.exec", r#"insert into k_users (name, email, age, meta) values ('ann', 'ann@x', 30, '{"a": 1}'), ('bob', 'bob@x', 40, '{"b": 2}')"#, ""),
        ("db.exec", "insert into k_users (name, email, age) values ('ann2', 'ann@x', 31) on duplicate key update name = values(name)", ""),
        ("db.exec", "insert ignore into k_users (name, email) values ('ann', 'ann@x')", ""),
        ("db.exec", "insert into k_users set name = 'zed', email = 'z@x', age = 9", ""),
        ("db.exec", "replace into k_users (id, name, email, age) values (1, 'ann', 'ann@x', 30)", ""),
        ("db.exec", "insert into k_posts (user_id, title) select id, concat('hello ', name) from k_users", ""),
        ("db.exec", "update k_users u join k_posts p on p.user_id = u.id set u.age = u.age + 1 where u.name = ?", "\"bob\""),
        ("db.exec", "delete p from k_posts p join k_users u on u.id = p.user_id where u.name = 'nobody'", ""),
        ("db.query", "select u.name, count(p.id) as n from k_users u left join k_posts p on p.user_id = u.id group by u.name having count(p.id) >= 0 order by u.name", ""),
        ("db.query", "with recent as (select * from k_posts order by id desc limit 5) select count(*) as n from recent", ""),
        ("db.query", "with recursive c(x) as (select 1 union all select x + 1 from c where x < 5) select sum(x) as s from c", ""),
        ("db.query", "select name, row_number() over (order by age desc) as r, lag(name) over (order by id) as prev from k_users order by id", ""),
        ("db.query", r#"select json_object('n', name) as j, json_extract(meta, '$.a') as a, json_unquote(json_extract('{"x":"y"}', '$.x')) as x from k_users order by id"#, ""),
        ("db.query", "select json_arrayagg(name) as names from k_users", ""),
        ("db.query", "select now() > '2000-01-01' as n, date_format('2026-01-02', '%Y') as y, datediff('2026-01-02', '2026-01-01') as d", ""),
        ("db.query", "select group_concat(name order by name separator ',') as s from k_users", ""),
        ("db.query", "select * from k_users where name like concat(?, '%') order by id limit ? offset ?", "\"a\", 5, 0"),
        ("db.query", "select if(age > 30, 'old', 'young') as c, ifnull(null, 1) as i, coalesce(null, 2) as k, md5('x') as m from k_users order by id", ""),
        ("db.query", "select last_insert_id() >= 0 as l, concat(:a, '-', :a) as s", "{a: \"v\"}"),
        ("db.exec", "alter table k_users add column bio text", ""),
        ("db.exec", "alter table k_users add index k_users_age (age)", ""),
        ("db.exec", "alter table k_users modify column bio varchar(200)", ""),
        ("db.exec", "alter table k_users change column bio about varchar(200)", ""),
        ("db.exec", "alter table k_users drop column about", ""),
        ("db.exec", "alter table k_users add column z int, algorithm = inplace, lock = none", ""),
        ("db.exec", "alter table k_users engine = InnoDB", ""),
        ("db.exec", "alter table k_users row_format = dynamic", ""),
        ("db.query", "explain select * from k_posts where user_id = 1", ""),
        ("db.query", "explain format = json select 1", ""),
        ("db.exec", "create or replace view k_v as select name from k_users", ""),
        ("db.query", "select * from k_v", ""),
        ("db.exec", "drop view if exists k_v", ""),
        ("db.exec", "create temporary table if not exists k_t (x int)", ""),
        ("db.exec", "drop temporary table if exists k_t", ""),
        ("db.exec", "create table if not exists k_e (engine varchar(10), kind int) engine = InnoDB", ""),
        ("db.exec", "create table if not exists k_like like k_users", ""),
        ("db.exec", "create table if not exists k_sel as select id, name from k_users", ""),
        ("db.exec", "truncate table k_posts", ""),
        ("db.query", "select cast('12' as signed) as n, convert('\u{e9}' using utf8mb4) as e, char_length('\u{e9}') as c", ""),
        ("db.query", "select name from k_users order by id limit 1 for update", ""),
        ("db.exec", "drop table if exists k_like, k_sel", ""),
    ];
    let ran = |calls: &[(&str, &str, &str)], last: &[&str]| {
        let mut want: Vec<&str> = vec!["kit:"];
        want.extend(std::iter::repeat_n("ran", calls.len() - last.len()));
        want.extend(last);
        format!("{}\n", want.join("\n"))
    };
    // SQLite: the kit's tables in the main file's database, which it is confirmed to write.
    let d = dir("ordinary");
    std::fs::create_dir_all(d.join("data")).unwrap();
    let mut lite = sqlite.clone();
    lite.push(("db.exec", "pragma optimize", ""));
    write(&d, "kit/k.qwe", &kit_ran(&lite));
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\nprint(steal())\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/app.db\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains(&ran(&lite, &[ANALYZE])), "{}", out);
    // PostgreSQL and MySQL: the program's database.
    for (name, calls) in [("pg", &pg), ("my", &mysql)] {
        let (_pg, _my);
        let url = if name == "pg" {
            let Some(pg) = support::postgres("cov8285db7o") else {
                eprintln!("PostgreSQL is not installed: skipped");
                continue;
            };
            let url = pg.database("ordinary");
            _pg = pg;
            url
        } else {
            let Some(my) = support::mariadb("cov8285db7o") else {
                eprintln!("MariaDB is not installed: skipped");
                continue;
            };
            let url = my.url("ordinary");
            _my = my;
            url
        };
        let d = dir(&format!("ordinary-{}", name));
        write(&d, "kit/k.qwe", &kit_ran(calls));
        write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\nprint(steal())\n");
        write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
        checked(&d);
        let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
        assert_eq!(code, 0, "{}", out);
        assert!(out.contains(&ran(calls, &[])), "{}: {}", name, out);
    }
}

// Fix round 8

/// What the hold says when a taken file's SQL sets a SQLite pragma.
fn sets(name: &str, value: &str) -> String {
    format!("db: `pragma {} = {}` sets the database connection's or file's state, which every file's later statements see, and is refused for kit/k.qwe's SQL where a quillo.confirmed holds the run: a taken file's pragmas only read (`pragma {}`). {}", name, value, name, TAIL)
}

/// Under a quillo.confirmed on SQLite a taken file's pragma given a value is refused, as a `set`
/// is on PostgreSQL and MySQL: every spelling (`= v`, the call form `(v)`, the name quoted, in
/// capitals, `main.`), the settings of the shared connection and of the file (`journal_mode`,
/// `query_only`, `schema_version`, `foreign_keys`, `user_version` given a unicode and an empty
/// value, `cache_size` a negative one, `synchronous`, `busy_timeout`) and the pragmas that act
/// given one (`wal_checkpoint(truncate)`, `optimize(0x10002)`), each named in the refusal. Its
/// reads run (`table_info(t)`, `journal_mode`, `query_only`). The main file's own settings run,
/// and none of the kit's reached the connection: its journal is WAL, its keys on, its
/// `user_version` the main file's 7, and it writes (src/db.rs authorize, SQLITE_PRAGMA,
/// pragma_reads; FEEDBACK83.md Q-1300, the coordinator's round 8; branch feedback82-85).
#[test]
fn on_sqlite_a_taken_files_pragma_settings_are_refused_and_its_reads_run() {
    let d = dir("pragmas");
    std::fs::create_dir_all(d.join("data")).unwrap();
    let calls: Vec<(&str, &str, String)> = vec![
        ("db.exec", "pragma journal_mode = delete", sets("journal_mode", "delete")),
        ("db.exec", "pragma journal_mode(delete)", sets("journal_mode", "delete")),
        ("db.exec", "pragma \"journal_mode\" = delete", sets("journal_mode", "delete")),
        ("db.exec", "pragma main.journal_mode = delete", sets("journal_mode", "delete")),
        ("db.exec", "pragma journal_mode = off", sets("journal_mode", "off")),
        ("db.exec", "PRAGMA Query_Only = 1", sets("query_only", "1")),
        ("db.exec", "pragma schema_version = 5", sets("schema_version", "5")),
        ("db.exec", "pragma foreign_keys = off", sets("foreign_keys", "off")),
        ("db.exec", "pragma user_version = 'caf\u{e9}'", sets("user_version", "caf\u{e9}")),
        ("db.exec", "pragma user_version = ''", sets("user_version", "")),
        ("db.exec", "pragma user_version = 3", sets("user_version", "3")),
        ("db.exec", "pragma cache_size = -2000", sets("cache_size", "-2000")),
        ("db.exec", "pragma synchronous = normal", sets("synchronous", "normal")),
        ("db.exec", "pragma busy_timeout = 1000", sets("busy_timeout", "1000")),
        ("db.exec", "pragma wal_checkpoint(truncate)", sets("wal_checkpoint", "truncate")),
        ("db.exec", "pragma optimize(0x10002)", sets("optimize", "0x10002")),
        ("db.query", "pragma table_info(kits)", "ran".into()),
        ("db.query", "pragma journal_mode", "ran".into()),
        ("db.query", "pragma query_only", "ran".into()),
        ("db.exec", "insert into kits values ('k')", "ran".into()),
    ];
    let made: Vec<(&str, &str, &str)> = calls.iter().map(|(f, s, _)| (*f, *s, "")).collect();
    write(&d, "kit/k.qwe", &kit_ran(&made));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\ndb.exec(\"create table if not exists kits (x text)\")\nprint(\"main: \" + str(db.exec(\"pragma user_version = 7\")) + \" \" + str(db.query(\"pragma cache_size\")))\nprint(steal())\nprint(\"after: \" + str(db.query(\"pragma journal_mode\")) + str(db.query(\"pragma foreign_keys\")) + str(db.query(\"pragma user_version\")) + str(db.query(\"pragma query_only\")) + \" \" + str(db.query(\"select count(*) as n from kits\")))\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/app.db\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let mut want = vec!["kit:".to_string()];
    want.extend(calls.iter().map(|(_, _, w)| w.clone()));
    assert!(out.contains("main: { changes: 0, last_id: null } [{ cache_size: -2000 }]\n"), "{}", out);
    assert!(out.contains(&format!("{}\n", want.join("\n"))), "{}", out);
    assert!(out.contains("after: [{ journal_mode: \"wal\" }][{ foreign_keys: 1 }][{ user_version: 7 }][{ query_only: 0 }] [{ n: 1 }]\n"), "{}", out);
}

/// Under a quillo.confirmed a taken file's SQLite migration that turns foreign keys off to rebuild
/// a table, as SQLite's own steps do, is applied: Quillo turns them off around it and on again
/// (migrate.rs), which the hold lets through while a migration runs, and the child's row stays.
/// Its migration that sets another pragma (`user_version`) is refused, saying so after the
/// migration's name, and `db.migrate` stops (src/db.rs authorize, migrate_as, MIGRATING;
/// FEEDBACK83.md Q-1300, the coordinator's round 8; branch feedback82-85).
#[test]
fn a_taken_files_migration_turns_foreign_keys_off_as_quillo_does_and_sets_no_other_pragma() {
    let d = dir("migpragma");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let k = db.connect(\"data/k.db\")\n  return \"kit: \" + str(k.migrate(\"kit/m\")) + \" \" + str(k.query(\"select p from child\")) + \" \" + str(k.query(\"pragma foreign_keys\"))\n}\n",
    );
    write(&d, "kit/m/001_a.sql", "create table parent (id integer primary key);\ncreate table child (id integer primary key, p int references parent(id) on delete cascade);\ninsert into parent values (1);\ninsert into child values (1, 1);\n");
    write(&d, "kit/m/002_rebuild.sql", "pragma foreign_keys = off;\ncreate table parent2 (id integer primary key, name text default 'caf\u{e9}');\ninsert into parent2 (id) select id from parent;\ndrop table parent;\nalter table parent2 rename to parent;\npragma foreign_keys = on;\n");
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\nprint(steal())\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/k.db\"]\nfiles = [\"kit/m/\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("kit: [\"001_a.sql\", \"002_rebuild.sql\"] [{ p: 1 }] [{ foreign_keys: 1 }]\n"), "{}", out);
    // A migration that sets another pragma: refused, and the run stops.
    std::fs::remove_file(d.join("data/k.db")).unwrap();
    write(&d, "kit/m/003_v.sql", "pragma user_version = 9;\n");
    let (code, out) = run(&d, &[]);
    assert_ne!(code, 0, "{}", out);
    assert!(out.contains(&format!("kit/k.qwe:6: db.migrate: migration 003_v.sql failed: {}\n", &sets("user_version", "9")["db: ".len()..])), "{}", out);
    assert_eq!(column(&d.join("data/k.db"), "select name from sqlite_master where name = 'child'"), ["child"]);
}
