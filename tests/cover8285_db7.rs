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
    format!("db: this statement begins with `{}`, which a taken file's statement may not begin with where a quillo.confirmed holds the run: it may begin with select, insert, update, delete, with, values, table, replace, merge, explain, create, alter, drop, truncate or refresh materialized view (kit/k.qwe's SQL). {}", word, TAIL)
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
        // Round 9: `merge`, `refresh materialized view`, `setval` given a value, a column named `call`.
        ("db.exec", "merge into k_users u using (select 'ann@x' as email, 99 as age) s on u.email = s.email when matched then update set age = s.age when not matched then do nothing", ""),
        ("db.exec", "refresh materialized view k_mv", ""),
        ("db.query", "select setval('k_seq', ?) as n", "100"),
        ("db.exec", "create table if not exists k_calls (id int primary key, call text)", ""),
        ("db.exec", "insert into k_calls (id, call) values (1, 'caf\u{e9}')", ""),
        ("db.query", "select call from k_calls where call is not null", ""),
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
        // Round 11: a temporary table is refused on MySQL too (below); `drop temporary` of none runs.
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
/// a table, as SQLite's own steps do, is refused (round 9, which decides it: round 8 applied it):
/// Quillo's own turn of the keys around the file, outside its transaction, goes on, and the file's
/// own `pragma foreign_keys = off`, inside it, is refused, saying so after the migration's name;
/// the parent is not rebuilt and the child's row stays. Its migration that sets another pragma
/// (`user_version`) is refused the same way, and `db.migrate` stops (src/db.rs authorize,
/// migrate_as, MIGRATING; FEEDBACK83.md Q-1300, the coordinator's rounds 8 and 9; branch
/// feedback82-85).
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
    assert_ne!(code, 0, "{}", out);
    // 001 was applied (its own transaction committed), 002 refused inside its own.
    assert!(out.contains(&format!("kit/k.qwe:6: db.migrate: migration 002_rebuild.sql failed: {}\n", &sets("foreign_keys", "off")["db: ".len()..])), "{}", out);
    assert_eq!(column(&d.join("data/k.db"), "select name from pragma_table_info('parent') order by cid"), ["id"]);
    assert_eq!(column(&d.join("data/k.db"), "select cast(p as text) from child"), ["1"]);
    // A migration that sets another pragma: refused, and the run stops.
    std::fs::remove_file(d.join("data/k.db")).unwrap();
    std::fs::remove_file(d.join("kit/m/002_rebuild.sql")).unwrap();
    write(&d, "kit/m/003_v.sql", "pragma user_version = 9;\n");
    let (code, out) = run(&d, &[]);
    assert_ne!(code, 0, "{}", out);
    assert!(out.contains(&format!("kit/k.qwe:6: db.migrate: migration 003_v.sql failed: {}\n", &sets("user_version", "9")["db: ".len()..])), "{}", out);
    assert_eq!(column(&d.join("data/k.db"), "select name from sqlite_master where name = 'child'"), ["child"]);
}

// Fix round 9

/// What the hold says when a taken file makes a temporary table, view or index on SQLite.
fn temporary(kind: &str, name: &str) -> String {
    format!("db: a temporary {} (`{}`) stands for the program's of its name in every file's later statements on the shared connection, and is refused for kit/k.qwe's SQL where a quillo.confirmed holds the run: a taken file makes its tables in the database. {}", kind, name, TAIL)
}

/// Under a quillo.confirmed on SQLite a taken file makes no temporary table, view or index, by
/// every spelling (`create temp`, `create temporary`, `if not exists`, `as select`, `create table
/// temp.x`, `"temp".x`, `TEMP.x`, a virtual table in `temp`, a unicode name): one stood for the
/// main file's `kits` in every later statement on the shared connection, the main file reading
/// the kit's rows and a visitor's sign-up lost at the stop. Run: an index of the kit's on the main
/// file's table, a table in a database the kit attached. `pragma m2.journal_mode = off` of that
/// database is refused as any setting is (decided: refusing it is the simpler rule). The main
/// file reads its own `kits` after, and its insert goes there (src/db.rs authorize,
/// SQLITE_CREATE_TEMP_*, temp_refused; FEEDBACK83.md Q-1300, found by the live review of 726b006;
/// branch feedback82-85).
#[test]
fn on_sqlite_a_taken_file_makes_no_temporary_table_view_or_index() {
    let d = dir("temps");
    std::fs::create_dir_all(d.join("data")).unwrap();
    let calls: Vec<(&str, &str, String)> = vec![
        ("db.exec", "create temp table kits (x text)", temporary("table", "kits")),
        ("db.exec", "create temporary table if not exists k2 (x text)", temporary("table", "k2")),
        ("db.exec", "create table temp.kits (x text)", temporary("table", "kits")),
        ("db.exec", "create table \"temp\".kits (x text)", temporary("table", "kits")),
        ("db.exec", "create table TEMP.kits (x text)", temporary("table", "kits")),
        ("db.exec", "create temp table \"k\u{ef}ts\" (x text)", temporary("table", "k\u{ef}ts")),
        ("db.exec", "create temp table k3 as select * from kits", temporary("table", "k3")),
        ("db.exec", "create temp view kits as select 'kit-view' as x", temporary("view", "kits")),
        ("db.exec", "create temporary view kv2 as select 1 as one", temporary("view", "kv2")),
        ("db.exec", "create view temp.kv3 as select 1 as one", temporary("view", "kv3")),
        ("db.exec", "create virtual table temp.kf using fts5(body)", temporary("virtual table", "kf")),
        ("db.exec", "create index if not exists k_i on kits (x)", "ran".into()),
        ("db.exec", "attach ':memory:' as m2", "ran".into()),
        ("db.exec", "create table m2.kits (x text)", "ran".into()),
        ("db.exec", "pragma m2.journal_mode = off", sets("journal_mode", "off")),
        ("db.query", "select x from kits", "ran".into()),
    ];
    let made: Vec<(&str, &str, &str)> = calls.iter().map(|(f, s, _)| (*f, *s, "")).collect();
    write(&d, "kit/k.qwe", &kit_ran(&made));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\ndb.exec(\"create table if not exists kits (x text)\")\ndb.exec(\"insert into kits values ('main-real')\")\nprint(steal())\nprint(\"main: \" + str(db.query(\"select x from kits\")) + \" \" + str(db.exec(\"insert into kits values ('visitor')\")))\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/app.db\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let mut want = vec!["kit:".to_string()];
    want.extend(calls.iter().map(|(_, _, w)| w.clone()));
    assert!(out.contains(&format!("{}\nmain: [{{ x: \"main-real\" }}] {{ changes: 1, last_id: 2 }}\n", want.join("\n"))), "{}", out);
    assert_eq!(column(&d.join("data/app.db"), "select x from kits order by rowid"), ["main-real", "visitor"]);
}

/// Under a quillo.confirmed a taken file's SQLite migration begins or ends no transaction: each
/// file runs in one of Quillo's own, which its `commit` ended, and `commit; pragma foreign_keys =
/// 00; begin;` then ran its pragma outside any, leaving the program's keys off. Refused before the
/// file runs, saying so after its name, by every spelling (`commit`, `END TRANSACTION`, `rollback`
/// after a comment, `savepoint`), and nothing of it is made. A word `commit` in a string or a
/// comment, and in a table's name, is none; that file is applied and the keys are on (src/db.rs
/// migration_judge; FEEDBACK83.md Q-1300, found by the live review of 726b006; branch
/// feedback82-85).
#[test]
fn a_taken_files_sqlite_migration_begins_or_ends_no_transaction() {
    let d = dir("migtx");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let k = db.connect(\"data/app.db\")\n  return \"kit: \" + str(k.migrate(\"kit/m\")) + \" \" + str(k.query(\"pragma foreign_keys\"))\n}\n",
    );
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\nprint(steal())\nprint(\"main: \" + str(db.query(\"pragma foreign_keys\")))\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/app.db\"]\nfiles = [\"kit/m/\"]\n");
    let refused = "kit/k.qwe:6: db.migrate: kit/m/001_a.sql: a taken file's migration begins or ends no transaction where a quillo.confirmed holds the run: each file runs in one of Quillo's own, which its `commit` or `rollback` would end (kit/k.qwe's SQL). Run it from the main file's migrations\n";
    for tail in ["commit;\npragma foreign_keys = 00;\nbegin;\n", "END TRANSACTION;\n", "/* x */ rollback;\n", "savepoint s;\nrelease s;\n"] {
        let _ = std::fs::remove_file(d.join("data/app.db"));
        write(&d, "kit/m/001_a.sql", &format!("create table if not exists kp (id integer primary key);\n{}", tail));
        let (code, out) = run(&d, &[]);
        assert_ne!(code, 0, "{}: {}", tail, out);
        assert!(out.contains(refused), "{}: {}", tail, out);
        assert!(column(&d.join("data/app.db"), "select name from sqlite_master where name = 'kp'").is_empty(), "{}", tail);
    }
    let _ = std::fs::remove_file(d.join("data/app.db"));
    write(&d, "kit/m/001_a.sql", "create table if not exists kcommit (x text);\ninsert into kcommit values ('commit; caf\u{e9}'); -- commit\n/* rollback */\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("kit: [\"001_a.sql\"] [{ foreign_keys: 1 }]\nmain: [{ foreign_keys: 1 }]\n"), "{}", out);
    assert_eq!(column(&d.join("data/app.db"), "select x from kcommit"), ["commit; caf\u{e9}"]);
}

/// Under a quillo.confirmed on PostgreSQL a taken file's SQL names none of the types whose value
/// is a catalog object named by text, wherever they are: a column `regclass` (a table's name then
/// stored from `'_quil' || 'lo_jobs'`), quoted, schema-qualified `regtype`, casts to `regtype`,
/// `REGPROC`, `regrole`, the array `_regclass`, a quoted `"regclass"` in a cast (and `::regclass`,
/// said as before); a quoted `"regclass"` as an alias is a name and runs. It writes `merge` (an insert, an
/// update, `then do nothing`; `then do $$...$$` refused), `refresh materialized view` (plain,
/// `concurrently`, `with [no] data`; `refresh` alone refused), `setval` given a value (a
/// parameter, an expression, a subquery; a parameter as the sequence refused),
/// `has_table_privilege` given a user first (a parameter as the table refused), and columns named
/// `call`, `prepare` and `deallocate`; `call`, `prepare` and `deallocate` as statements are refused,
/// and `execute` anywhere (`explain analyze execute`, `create table ... as execute`) (src/db.rs
/// text_run_in, reg_type, head_refused, plain_arguments; FEEDBACK83.md Q-1300, found by the live
/// review of 726b006; branch feedback82-85).
#[test]
fn on_postgresql_the_reg_types_are_refused_and_merge_refresh_setval_and_a_column_named_call_run() {
    let Some(pg) = support::postgres("cov8285db7r9") else {
        eprintln!("PostgreSQL is not installed: skipped");
        return;
    };
    let url = pg.database("round9");
    let d = dir("pgr9");
    let reg = |t: &str| runs(&format!("the type {}, by which a text names a catalog object as it runs,", t));
    let calls: Vec<(&str, &str, &str, String)> = vec![
        ("db.exec", "create table if not exists kreg (r regclass)", "", reg("regclass")),
        ("db.exec", "create table if not exists kreg2 (r \"regclass\")", "", reg("regclass")),
        ("db.exec", "create table if not exists kreg3 (r pg_catalog.regtype)", "", reg("regtype")),
        ("db.query", "select 'int4'::regtype as t", "", reg("regtype")),
        ("db.query", "select cast('kits' as REGPROC) as p", "", reg("regproc")),
        ("db.query", "select 'x'::regrole as r", "", reg("regrole")),
        ("db.query", "select 'kits'::regclass as c", "", runs("a table's name (regclass) worked out as it runs")),
        ("db.query", "select '{kits}'::_regclass as a", "", reg("_regclass")),
        ("db.query", "select cast('kits' as \"regclass\") as c", "", runs("a table's name (regclass) worked out as it runs")),
        ("db.query", "select 1 as \"regclass\", 2 as \"call\"", "", "ran".into()),
        ("db.exec", "merge into kk using (select 'm' as x) s on kk.x = s.x when not matched then insert values (s.x) when matched then do nothing", "", "ran".into()),
        ("db.exec", "merge into kk using (select 'm' as x) s on kk.x = s.x when matched then update set x = s.x || '\u{e9}'", "", "ran".into()),
        ("db.exec", "merge into kk using (select 'm' as x) s on kk.x = s.x when matched then do $$ begin end $$", "", runs("`do`")),
        ("db.exec", "refresh materialized view k_mv", "", "ran".into()),
        ("db.exec", "refresh materialized view concurrently k_mvu", "", "ran".into()),
        ("db.exec", "refresh materialized view k_mv with no data", "", "ran".into()),
        ("db.exec", "refresh materialized view k_mv with data", "", "ran".into()),
        ("db.exec", "refresh k_mv", "", begins("refresh")),
        ("db.query", "select setval('k_seq', ?) as n", "100", "ran".into()),
        ("db.query", "select setval('k_seq', ? + 1, true) as n", "100", "ran".into()),
        ("db.query", "select setval('k_seq', (select 7)) as n", "", "ran".into()),
        ("db.query", "select setval(?, 5) as n", "\"k_seq\"", runs("the function setval given a table's name worked out as it runs")),
        ("db.query", "select has_table_privilege(current_user, 'kits', 'select') as p", "", "ran".into()),
        ("db.query", "select has_table_privilege(current_user, ?, 'select') as p", "\"kits\"", runs("the function has_table_privilege given a table's name worked out as it runs")),
        ("db.exec", "create table if not exists kc (id int primary key, call text, prepare text, deallocate text)", "", "ran".into()),
        ("db.exec", "insert into kc (id, call, prepare, deallocate) values (1, 'c', 'p', 'd')", "", "ran".into()),
        ("db.query", "select call, prepare, deallocate from kc where call = 'c'", "", "ran".into()),
        ("db.exec", "call kp()", "", runs("`call`")),
        ("db.exec", "prepare kps as select 1", "", runs("`prepare`")),
        ("db.exec", "deallocate kps", "", runs("`deallocate`")),
        ("db.query", "explain analyze execute kps", "", runs("`execute`")),
        ("db.exec", "create table kx as execute kps", "", runs("`execute`")),
    ];
    let made: Vec<(&str, &str, &str)> = calls.iter().map(|(f, s, v, _)| (*f, *s, *v)).collect();
    write(&d, "kit/k.qwe", &kit_ran(&made));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists kits (x text)\")\ndb.exec(\"create table if not exists kk (x text)\")\ndb.exec(\"create sequence if not exists k_seq\")\ndb.exec(\"create materialized view if not exists k_mv as select 1 as n\")\ndb.exec(\"create materialized view if not exists k_mvu as select 1 as n\")\ndb.exec(\"create unique index if not exists k_mvu_n on k_mvu (n)\")\nprint(steal())\nprint(\"kk: \" + str(db.query(\"select x from kk\")) + \" made: \" + str(db.query(\"select relname as n from pg_class where relname like 'kreg%' or relname = 'kx' order by 1\")))\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    let mut want = vec!["kit:".to_string()];
    want.extend(calls.iter().map(|(_, _, _, w)| w.clone()));
    assert!(out.contains(&format!("{}\n", want.join("\n"))), "{}", out);
    assert!(out.contains("kk: [{ x: \"m\u{e9}\" }] made: []\n"), "{}", out);
}

// Fix round 10

/// Under a quillo.confirmed a taken file's SQLite migration is judged where SQLite runs it. Its
/// text is read as SQLite reads a parameter (`:a(...)`, `@a(...)`, `#a(...)`, `$a(...)`, `:a::b(...)`,
/// to the first `)` or space, a `'` and a `;` in it): `select :a(x');commit;pragma foreign_keys=off;
/// begin;select :b(');` was one statement to that reading and a `commit` to SQLite, and the keys
/// were left off for the program; each is refused before the file runs. And SQLite's authorizer
/// refuses the file's own transaction statements, `explain commit`, `explain savepoint s` and
/// `explain release s`, which no reading of the text takes for one, where Quillo's own `begin`
/// and `commit` around the file run as its own. Nothing of a refused file is made; a file with
/// `:a(x)` in a string and a comment is applied, and the keys are on (src/sql.rs sqlite_var_end,
/// src/db.rs authorize, tx_refused, src/migrate.rs own; FEEDBACK83.md Q-1300, found by the live
/// review of b3a2998; branch feedback82-85).
#[test]
fn a_taken_files_sqlite_migration_is_judged_where_sqlite_runs_it() {
    let d = dir("migtcl");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  return \"kit: \" + str(db.migrate(\"kit/m\")) + \" \" + str(db.query(\"pragma foreign_keys\"))\n}\n");
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\ndb.exec(\"create table if not exists parent (id integer primary key)\")\ndb.exec(\"create table if not exists child (id integer primary key, p integer not null references parent(id))\")\nprint(steal())\nprint(\"main: \" + str(db.query(\"pragma foreign_keys\")) + \" \" + db.exec(\"insert into child (id, p) values (1, 999)\").error)\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/app.db\"]\nfiles = [\"kit/m/\"]\nsite = [\"db.migrate\"]\n");
    let by_text = "kit/k.qwe:5: db.migrate: kit/m/001_a.sql: a taken file's migration begins or ends no transaction where a quillo.confirmed holds the run: each file runs in one of Quillo's own, which its `commit` or `rollback` would end (kit/k.qwe's SQL). Run it from the main file's migrations\n";
    let as_run = |what: &str| format!("kit/k.qwe:5: db.migrate: migration 001_a.sql failed: `{}` in a taken file's migration is refused where a quillo.confirmed holds the run: each file runs in a transaction of Quillo's own, which it would begin or end (kit/k.qwe's SQL). Run it from the main file's migrations\n", what);
    let cases: Vec<(&str, String)> = vec![
        ("create table kk (x text);\nselect :a(x');commit;pragma foreign_keys=off;begin;select :b(');\n", by_text.to_string()),
        ("create table kk (x text);\nselect :a::b(x');commit;pragma foreign_keys=off;begin;select :c(');\n", by_text.to_string()),
        ("create table kk (x text);\nselect @a(x');end;select :b(');\n", by_text.to_string()),
        ("create table kk (x text);\nselect #a(x');rollback;select :b(');\n", by_text.to_string()),
        ("create table kk (x text);\nselect $caf\u{e9}(x');commit;select :b(');\n", by_text.to_string()),
        ("create table kk (x text);\nexplain commit;\n", as_run("commit")),
        ("create table kk (x text);\nexplain savepoint s;\n", as_run("savepoint")),
        ("create table kk (x text);\nexplain release s;\n", as_run("release")),
    ];
    for (file, refused) in &cases {
        let _ = std::fs::remove_file(d.join("data/app.db"));
        write(&d, "kit/m/001_a.sql", file);
        let (code, out) = run(&d, &[]);
        assert_ne!(code, 0, "{}: {}", file, out);
        assert!(out.contains(refused.as_str()), "{}: {}", file, out);
        assert!(column(&d.join("data/app.db"), "select name from sqlite_master where name = 'kk'").is_empty(), "{}", file);
    }
    let _ = std::fs::remove_file(d.join("data/app.db"));
    write(&d, "kit/m/001_a.sql", "create table kk (x text);\ninsert into kk values (':a(x'');commit;'); -- :a(x');commit;\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("kit: [\"001_a.sql\"] [{ foreign_keys: 1 }]\nmain: [{ foreign_keys: 1 }] db: FOREIGN KEY constraint failed\n"), "{}", out);
    assert_eq!(column(&d.join("data/app.db"), "select x from kk"), [":a(x');commit;"]);
}

/// Under a quillo.confirmed on PostgreSQL a function that takes a table is judged by its table's
/// argument, its arguments counted at the commas outside brackets of every kind: `array['a','b']`
/// in `has_table_privilege('quillo', case when array['a','b'] ... then '_quil' || 'lo_jobs' end,
/// 'select')` had the user judged as the table; the same in `has_column_privilege` with four
/// arguments, `has_sequence_privilege` with a `'{a,b}'` cast, a count it takes none of, and a bracket
/// closed by another kind, are refused. Run: the user and a plain table, the table alone, an array's
/// element as `setval`'s value. And a reg* value made without its type named is refused by the
/// function's name: `regclassin(textout(...))` (and `create table ... as select` of it),
/// `regprocedurein`, `to_regtype`, `to_regtypemod`, `regtypeout`, quoted and in capitals,
/// `regclassrecv`; `textout` and `get_current_ts_config` run. No `kreg` is made (src/db.rs
/// plain_arguments, reg_named; FEEDBACK83.md Q-1300, found by the live review of b3a2998; branch
/// feedback82-85).
#[test]
fn on_postgresql_a_tables_argument_is_counted_past_every_bracket_and_the_reg_functions_are_refused() {
    let Some(pg) = support::postgres("cov8285db7r10") else {
        eprintln!("PostgreSQL is not installed: skipped");
        return;
    };
    let url = pg.database("round10");
    let d = dir("pgr10");
    let given = |f: &str| runs(&format!("the function {} given a table's name worked out as it runs", f));
    let reg = |f: &str| runs(&format!("the function {}, by which a text names a catalog object as it runs,", f));
    let calls: Vec<(&str, &str, String)> = vec![
        ("db.query", "select has_table_privilege('quillo', case when array['a','b'] is not null then '_quil' || 'lo_jobs' end, 'select') as p", given("has_table_privilege")),
        ("db.query", "select has_column_privilege('quillo', case when array['a','b'] is not null then '_quil' || 'lo_jobs' end, 'name', 'select') as p", given("has_column_privilege")),
        ("db.query", "select has_sequence_privilege('quillo', case when '{a,b}'::text[] is not null then 'k_seq' end, 'usage') as p", given("has_sequence_privilege")),
        ("db.query", "select has_table_privilege('kits', 'select', 'x', 'y') as p", given("has_table_privilege")),
        ("db.query", "select has_table_privilege('kits', 'select']) as p", given("has_table_privilege")),
        ("db.query", "select has_table_privilege(current_user, 'kits', 'select') as p", "ran".into()),
        ("db.query", "select has_table_privilege('kits', 'select') as p", "ran".into()),
        ("db.query", "select setval('k_seq', (array[5, 6])[1]) as n", "ran".into()),
        ("db.query", "select regclassin(textout('_quil' || 'lo_jobs')) as r", reg("regclassin")),
        ("db.exec", "create table kreg as select regclassin(textout('kits')) as r", reg("regclassin")),
        ("db.query", "select regprocedurein(textout('now()')) as p", reg("regprocedurein")),
        ("db.query", "select to_regtype('int4') as t", reg("to_regtype")),
        ("db.query", "select to_regtypemod('int4') as t", reg("to_regtypemod")),
        ("db.query", "select regtypeout('int4') as t", reg("regtypeout")),
        ("db.query", "select \"regclassin\"(textout('kits')) as r", reg("regclassin")),
        ("db.query", "select RegClassRecv(null) as r", reg("regclassrecv")),
        ("db.query", "select textout('caf\u{e9}') is not null as t, get_current_ts_config() is not null as c", "ran".into()),
    ];
    let made: Vec<(&str, &str, &str)> = calls.iter().map(|(f, s, _)| (*f, *s, "")).collect();
    write(&d, "kit/k.qwe", &kit_ran(&made));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists kits (x text)\")\ndb.exec(\"create sequence if not exists k_seq\")\ndb.exec(\"create table if not exists _quillo_jobs (name text primary key)\")\nprint(steal())\nprint(\"made: \" + str(db.query(\"select relname as n from pg_class where relname = 'kreg'\")))\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    let mut want = vec!["kit:".to_string()];
    want.extend(calls.iter().map(|(_, _, w)| w.clone()));
    assert!(out.contains(&format!("{}\nmade: []\n", want.join("\n"))), "{}", out);
}

// Fix round 11

/// What every refusal of a file in the program's own `.quillo` ends with.
const OWN_QUILLO: &str = "it is in the program's .quillo folder, which holds its built plugins, their lock and its keys";

/// In any run, with or without a quillo.confirmed, SQLite opens no database file in the program's
/// own `.quillo`, by its name in any letters or where the `.quillo` link leads: an attached one by
/// its name as written, worked out in the SQL or given as a value, and `VACUUM INTO`'s copy, as
/// written and given as a value, are refused as SQLite opens them, and a tenant's database is
/// refused before its file is made (`db.open(".quillo/{tenant}.lock")` then `new_tenant`). Run: a
/// database attached in `data/`, `:memory:`, `VACUUM INTO` a file in `data/`. Nothing is made in
/// `.quillo` (src/db.rs quillo_open, Running, new_tenant; FEEDBACK85.md Q-1340, found by the final
/// review of a6cee54; branch feedback82-85).
#[test]
fn no_database_file_is_opened_or_made_in_the_programs_own_quillo() {
    let d = dir("quillodb");
    for f in ["data", ".quillo/native", "store/native", "other"] {
        std::fs::create_dir_all(d.join(f)).unwrap();
    }
    std::os::unix::fs::symlink("../store", d.join("other/.quillo")).unwrap();
    write(&d, "other/k.qwe", "fn noop() {\n  return 1\n}\n");
    let calls: Vec<(&str, &str, &str, &str)> = vec![
        ("db.exec", "attach '.quillo/x.db' as q", "", ".quillo/x.db"),
        ("db.exec", "attach '.quil' || 'lo/c.db' as q", "", ".quillo/c.db"),
        ("db.exec", "attach ? as q", "\".quillo/p.db\"", ".quillo/p.db"),
        ("db.exec", "attach '.QUILLO/plugins.lock' as l", "", ".QUILLO/plugins.lock"),
        ("db.exec", "vacuum into '.quillo/native/libhi-0000.dylib'", "", ".quillo/native/libhi-0000.dylib"),
        ("db.exec", "vacuum into ?", "\".quillo/.quillo-secret\"", ".quillo/.quillo-secret"),
        ("db.exec", "vacuum into 'store/plugins.lock'", "", "store/plugins.lock"),
        ("db.exec", "attach 'store/native/caf\u{e9}.db' as s", "", "store/native/caf\u{e9}.db"),
        ("db.exec", "attach 'data/other.db' as o", "", ""),
        ("db.exec", "attach ':memory:' as m", "", ""),
        ("db.exec", "vacuum into 'data/copy.db'", "", ""),
    ];
    let lines: Vec<String> = calls.iter().map(|(f, s, v, _)| format!("print(said({}({}{})))", f, lit(s), if v.is_empty() { String::new() } else { format!(", {}", v) })).collect();
    write(
        &d,
        "main.qwe",
        &format!(
            "import other/k.qwe use noop\nlet said = (r) => is_err(r) ? r.error : \"ran\"\ndb.open(\"data/app.db\")\ndb.exec(\"create table if not exists t (x text)\")\n{}\ndb.open(\".quillo/{{tenant}}.lock\")\nprint(said(new_tenant(\"plugins\", () => null)))\n",
            lines.join("\n")
        ),
    );
    let (code, out) = run(&d, &[]);
    assert_ne!(code, 0, "new_tenant's refusal stops the run: {}", out);
    let printed: Vec<&str> = out.lines().filter(|l| l.starts_with("db: ") || *l == "ran").collect();
    assert_eq!(printed.len(), calls.len(), "{}", out);
    for ((_, sql, _, file), line) in calls.iter().zip(&printed) {
        if file.is_empty() {
            assert_eq!(*line, "ran", "{}: {}", sql, out);
        } else {
            assert!(line.ends_with(&format!("{}: {}", file, OWN_QUILLO)), "{}: {}", sql, out);
        }
    }
    assert!(out.contains(&format!("`new_tenant`: ./.quillo/plugins.lock: {}", OWN_QUILLO)) || out.contains(&format!(".quillo/plugins.lock: {}", OWN_QUILLO)), "{}", out);
    let made: Vec<String> = ["", "native"].iter().flat_map(|f| std::fs::read_dir(d.join(".quillo").join(f)).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned())).filter(|n| n != "native").collect();
    assert!(made.is_empty(), "{:?}", made);
    assert!(std::fs::read_dir(d.join("store/native")).unwrap().next().is_none() && !d.join("store/plugins.lock").exists());
    assert!(d.join("data/copy.db").exists() && d.join("data/other.db").exists());
}

/// Under a quillo.confirmed on SQLite a taken file detaches no database: `detach b; attach
/// ':memory:' as b` swapped the main file's attached `b` for the kit's, its later inserts going to
/// memory and its reads giving the kit's rows. `detach b`, `DETACH DATABASE "b"` and a `detach` of
/// one the kit attached are refused; `attach ... as b` again is SQLite's own error; a new name is
/// attached. The main file's `b.events` keeps its row (src/db.rs authorize, SQLITE_DETACH,
/// detach_refused; FEEDBACK83.md Q-1300, found by the final review of a6cee54; branch
/// feedback82-85).
#[test]
fn on_sqlite_a_taken_file_detaches_no_database() {
    let d = dir("detach");
    std::fs::create_dir_all(d.join("data")).unwrap();
    let detach = |name: &str| format!("db: `detach {}` is refused for kit/k.qwe's SQL where a quillo.confirmed holds the run: the databases attached on the program's connection are what every file's later statements reach by their names. {}", name, TAIL);
    let calls: Vec<(&str, &str, String)> = vec![
        ("db.exec", "detach b", detach("b")),
        ("db.exec", "DETACH DATABASE \"b\"", detach("b")),
        ("db.exec", "attach ':memory:' as b", "db: database b is already in use".into()),
        ("db.exec", "attach ':memory:' as k\u{e9}", "ran".into()),
        ("db.exec", "detach k\u{e9}", detach("k\u{e9}")),
        ("db.query", "select who from b.events", "ran".into()),
    ];
    let made: Vec<(&str, &str, &str)> = calls.iter().map(|(f, s, _)| (*f, *s, "")).collect();
    write(&d, "kit/k.qwe", &kit_ran(&made));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\ndb.exec(\"attach 'data/b.db' as b\")\ndb.exec(\"create table if not exists b.events (who text)\")\nprint(steal())\nprint(\"main: \" + str(db.exec(\"insert into b.events values ('main')\")) + \" \" + str(db.query(\"select who from b.events\")))\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/app.db\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let mut want = vec!["kit:".to_string()];
    want.extend(calls.iter().map(|(_, _, w)| w.clone()));
    assert!(out.contains(&format!("{}\nmain: {{ changes: 1, last_id: 1 }} [{{ who: \"main\" }}]\n", want.join("\n"))), "{}", out);
    assert_eq!(column(&d.join("data/b.db"), "select who from events"), ["main"]);
}

/// Under a quillo.confirmed a taken file makes no temporary table, view or sequence on PostgreSQL
/// and MySQL (MariaDB) either: inside the main file's `db.transaction` a kit's `create temp table
/// users` had the main file's insert go into it, gone at the commit. `create temp`, `create
/// temporary ... if not exists`, `global temporary`, a temporary view and sequence, and
/// PostgreSQL's `select ... into temp`, are refused; a table named `temp` is made and written. On
/// PostgreSQL `setseed` (in any letters), by which a kit fixed the main file's later `random()`, and
/// pg_trgm's `set_limit` are refused as a `set` is. The main file's row is in its `users` (src/db.rs
/// text_run_in, TextRun::Temp, TextRun::Session; FEEDBACK83.md Q-1300, found by the final review of
/// a6cee54; branch feedback82-85).
#[test]
fn temporary_tables_and_setseed_are_refused_to_a_taken_file_on_postgresql_and_mysql() {
    let temp = |kind: &str| format!("db: a temporary {} stands for the program's of its name in every later statement on its connection, the main file's in a transaction too, and is refused for kit/k.qwe's SQL where a quillo.confirmed holds the run: a taken file makes its tables in the database. {}", kind, TAIL);
    let session = |f: &str| format!("db: the function {} sets the connection's own state, which every file's later statements on the pooled connection see, and is refused for kit/k.qwe's SQL where a quillo.confirmed holds the run, as a `set` is. {}", f, TAIL);
    let pg_calls: Vec<(&str, &str, String)> = vec![
        ("db.exec", "create temp table users (name text)", temp("table")),
        ("db.exec", "create temporary table if not exists users (name varchar(50))", temp("table")),
        ("db.exec", "create global temporary table k1 (x int)", temp("table")),
        ("db.exec", "create temp view kv as select 1 as one", temp("view")),
        ("db.exec", "create temporary sequence ks", temp("sequence")),
        ("db.query", "select 1 as x into temp kt", temp("table")),
        ("db.query", "select setseed(0.25) as s", session("setseed")),
        ("db.query", "select SetSeed(0.5) as s", session("setseed")),
        ("db.query", "select set_limit(0.1) as s", session("set_limit")),
        ("db.exec", "insert into users (name) values ('caf\u{e9}')", "ran".into()),
        ("db.exec", "create table if not exists temp (x int)", "ran".into()),
        ("db.exec", "insert into temp values (1)", "ran".into()),
    ];
    let my_calls: Vec<(&str, &str, String)> = vec![
        ("db.exec", "create temporary table users (name varchar(50))", temp("table")),
        ("db.exec", "create temporary table if not exists users (name varchar(50))", temp("table")),
        ("db.exec", "CREATE TEMPORARY SEQUENCE ks", temp("sequence")),
        ("db.exec", "insert into users (name) values ('caf\u{e9}')", "ran".into()),
    ];
    for (name, calls) in [("pg", &pg_calls), ("my", &my_calls)] {
        let (_pg, _my);
        let url = if name == "pg" {
            let Some(pg) = support::postgres("cov8285db7r11") else {
                eprintln!("PostgreSQL is not installed: skipped");
                continue;
            };
            let url = pg.database("round11");
            _pg = pg;
            url
        } else {
            let Some(my) = support::mariadb("cov8285db7r11") else {
                eprintln!("MariaDB is not installed: skipped");
                continue;
            };
            let url = my.url("round11");
            _my = my;
            url
        };
        let d = dir(&format!("temp-{}", name));
        let made: Vec<(&str, &str, &str)> = calls.iter().map(|(f, s, _)| (*f, *s, "")).collect();
        write(&d, "kit/k.qwe", &kit_ran(&made));
        write(
            &d,
            "main.qwe",
            "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists users (name varchar(50))\")\nprint(db.transaction(() => {\n  let k = steal()\n  db.exec(\"insert into users values ('main')\")\n  return k\n}))\nprint(\"users: \" + str(db.query(\"select name from users order by name\")))\n",
        );
        write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
        checked(&d);
        let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
        assert_eq!(code, 0, "{}", out);
        let mut want = vec!["kit:".to_string()];
        want.extend(calls.iter().map(|(_, _, w)| w.clone()));
        assert!(out.contains(&format!("{}\nusers: [{{ name: \"caf\u{e9}\" }}, {{ name: \"main\" }}]\n", want.join("\n"))), "{}: {}", name, out);
    }
}

/// Under a quillo.confirmed a taken file's migrations are recorded under its folder before their
/// names (`kit/m/002_lock_down.sql`), the main file's under their names as they always were: a
/// kit confirmed for `db.migrate` that ran its `002_lock_down.sql` first had the main file's own
/// `migrations/002_lock_down.sql` never run ("changed after it was applied; it is not run again").
/// Now both are applied, each said by its name, on SQLite and on PostgreSQL, and the main file's
/// table is there (src/db.rs migration_prefix, src/migrate.rs run_keyed; FEEDBACK83.md Q-1300,
/// found by the final review of a6cee54; branch feedback82-85).
#[test]
fn a_taken_files_migrations_are_recorded_apart_from_the_main_files() {
    for name in ["sqlite", "pg"] {
        let _pg;
        let url = if name == "pg" {
            let Some(pg) = support::postgres("cov8285db7r11m") else {
                eprintln!("PostgreSQL is not installed: skipped");
                continue;
            };
            let url = pg.database("round11m");
            _pg = pg;
            url
        } else {
            String::new()
        };
        let d = dir(&format!("migkey-{}", name));
        std::fs::create_dir_all(d.join("data")).unwrap();
        let open = if name == "sqlite" { "db.open(\"data/app.db\")\n" } else { "" };
        write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  return \"kit: \" + str(db.migrate(\"kit/m\"))\n}\n");
        write(&d, "kit/m/002_lock_down.sql", "create table kit_lock (x text);\n");
        write(&d, "migrations/002_lock_down.sql", "create table lock_down (x text);\ninsert into lock_down values ('main ran caf\u{e9}');\n");
        write(
            &d,
            "main.qwe",
            &format!("import kit/k.qwe use Badge, steal\n{}print(steal())\nprint(\"main: \" + str(db.migrate(\"migrations\")))\nprint(\"rows: \" + str(db.query(\"select name from _quillo_migrations order by name\")))\nprint(\"locked: \" + str(db.query(\"select x from lock_down\")))\n", open),
        );
        let kit_writes = if name == "sqlite" { "writes = [\"data/app.db\"]\n" } else { "" };
        write(&d, "quillo.confirmed", &format!("[\"main.qwe\"]\nwrites = [\"data/\"]\nfiles = [\"migrations/\"]\n\n[\"kit/k.qwe\"]\n{}files = [\"kit/m/\"]\nsite = [\"db.migrate\"]\n", kit_writes));
        checked(&d);
        let vars: Vec<(&str, &str)> = if name == "pg" { vec![("QUILLO_DB", &url)] } else { vec![] };
        let (code, out) = run(&d, &vars);
        assert_eq!(code, 0, "{}: {}", name, out);
        assert!(
            out.contains("kit: [\"002_lock_down.sql\"]\nquillo: applied migration 002_lock_down.sql\nmain: [\"002_lock_down.sql\"]\nrows: [{ name: \"002_lock_down.sql\" }, { name: \"kit/m/002_lock_down.sql\" }]\nlocked: [{ x: \"main ran caf\u{e9}\" }]\n"),
            "{}: {}",
            name,
            out
        );
    }
}

/// In any run, with or without a quillo.confirmed, SQLite opens or makes no database where a write
/// of the program's is refused, for every program of the process: a mounted program's `.quillo`
/// through its link (`shop/.quillo -> qs`, in any letters), a source of the main program's or the
/// mounted one's, `.env.local` and `.ENV` (a SQLite `.env.local` stopped the next start), the
/// program's quillo.confirmed, and the journal or log of the database it has open, each by
/// `attach` or `vacuum into`; another database, the open one attached again, and a copy in `data/`
/// run. Nothing is made and the sources are as they were (src/builtins.rs own_file_apart, src/db.rs
/// quillo_open; FEEDBACK85.md Q-1340, found by the re-review of 00e83c3; branch feedback82-85).
#[test]
fn no_database_file_is_opened_or_made_where_a_write_of_the_programs_is_refused() {
    let d = dir("ownfiledb");
    for f in ["data", "shop/qs"] {
        std::fs::create_dir_all(d.join(f)).unwrap();
    }
    std::os::unix::fs::symlink("qs", d.join("shop/.quillo")).unwrap();
    write(&d, "shop/app.qwe", "route GET / = \"s\"\n");
    let (quillo, source, settings, confirmed, open) = (OWN_QUILLO, "it is one of the program's own files", "it is the program's settings (.env)", "it is what the program was confirmed to reach (quillo.confirmed)", "it is the database the program has open");
    let calls: Vec<(&str, &str, &str)> = vec![
        ("attach 'shop/qs/plugins.lock' as k", "shop/qs/plugins.lock", quillo),
        ("attach 'shop/QS/x.db' as k2", "shop/QS/x.db", quillo),
        ("vacuum into 'main.qwe'", "main.qwe", source),
        ("attach 'shop/app.qwe' as m", "shop/app.qwe", source),
        ("vacuum into '.env.local'", ".env.local", settings),
        ("attach '.ENV' as e", ".ENV", settings),
        ("vacuum into 'quillo.confirmed'", "quillo.confirmed", confirmed),
        ("attach 'data/app.db-journal' as j", "data/app.db-journal", open),
        ("vacuum into 'data/app.db-wal'", "data/app.db-wal", open),
        ("attach 'data/other.db' as o", "", ""),
        ("attach 'data/app.db' as again", "", ""),
        ("vacuum into 'data/copy.db'", "", ""),
    ];
    let lines: Vec<String> = calls.iter().map(|(s, _, _)| format!("print(said(db.exec({})))", lit(s))).collect();
    let main = format!(
        "mount /shop = \"shop/app.qwe\"\nlet said = (r) => is_err(r) ? r.error : \"ran\"\ndb.open(\"data/app.db\")\ndb.exec(\"create table if not exists t (x text)\")\ndb.exec(\"insert into t values ('a')\")\n{}\nexit(0)\n",
        lines.join("\n")
    );
    write(&d, "main.qwe", &main);
    let (code, out) = quillo_env(&d, &["run", "--no-check", "main.qwe"], 120, &[]);
    assert_eq!(code, Some(0), "{}", out);
    let printed: Vec<&str> = out.lines().filter(|l| l.starts_with("db: ") || *l == "ran").collect();
    assert_eq!(printed.len(), calls.len(), "{}", out);
    for ((sql, file, why), line) in calls.iter().zip(&printed) {
        if file.is_empty() {
            assert_eq!(*line, "ran", "{}: {}", sql, out);
        } else {
            assert!(line.ends_with(&format!("/{}: {}", file, why)), "{}: {}", sql, line);
        }
    }
    for f in ["shop/qs/plugins.lock", "shop/qs/x.db", ".env.local", ".ENV", ".env", "quillo.confirmed"] {
        assert!(!d.join(f).exists(), "{} was made", f);
    }
    // The open database's own log and journal are SQLite's, and hold no database.
    for f in ["data/app.db-journal", "data/app.db-wal"] {
        let head = std::fs::read(d.join(f)).unwrap_or_default();
        assert!(!head.starts_with(b"SQLite format 3"), "{} was made a database", f);
    }
    assert_eq!(std::fs::read_to_string(d.join("main.qwe")).unwrap(), main);
    assert_eq!(std::fs::read_to_string(d.join("shop/app.qwe")).unwrap(), "route GET / = \"s\"\n");
    assert!(d.join("data/copy.db").exists() && d.join("data/other.db").exists());
}

/// A tenant's database in a `.quillo`, the program's (`db.open(".quillo/native/lib{tenant}")`) or a
/// mounted program's through its link (`shop/.quillo -> qs`), is refused by `new_tenant` before its
/// folder or its file is made: the first left a 0-byte `.quillo/native/libbeta` for the next start
/// to load as a plugin's library (src/db.rs new_tenant; FEEDBACK85.md Q-1340, found by the
/// re-review of 00e83c3; branch feedback82-85).
#[test]
fn a_tenants_database_in_a_quillo_is_refused_before_anything_is_made() {
    let d = dir("tenantquillo");
    for f in [".quillo/native", "shop/qs"] {
        std::fs::create_dir_all(d.join(f)).unwrap();
    }
    std::os::unix::fs::symlink("qs", d.join("shop/.quillo")).unwrap();
    write(&d, "shop/app.qwe", "route GET / = \"s\"\n");
    for (target, tenant, file) in [(".quillo/native/lib{tenant}", "beta", ".quillo/native/libbeta"), ("shop/qs/{tenant}.db", "acme", "shop/qs/acme.db"), ("shop/.quillo/sub/{tenant}.db", "acme", "shop/.quillo/sub/acme.db")] {
        write(&d, "main.qwe", &format!("mount /shop = \"shop/app.qwe\"\ntenant_from(\"example.com\")\ndb.open({})\nprint(new_tenant({}, () => 1))\nexit(0)\n", lit(target), lit(tenant)));
        let (code, out) = quillo_env(&d, &["run", "--no-check", "main.qwe"], 120, &[]);
        assert_ne!(code, Some(0), "{}: {}", target, out);
        assert!(out.contains(&format!("`new_tenant`: ./{}: {}", file, OWN_QUILLO)), "{}: {}", target, out);
        let left: Vec<String> = [".quillo/native", "shop/qs"].iter().flat_map(|f| std::fs::read_dir(d.join(f)).unwrap().flatten().map(|e| e.path().display().to_string())).collect();
        assert!(left.is_empty(), "{}: {:?}", target, left);
    }
}

// Fix round 12

/// `db.open` and `db.connect` of a file where a write of the program's is refused (quillo.confirmed,
/// `.env.local`, `.ENV`, a source) are refused before any file is made: each was refused as SQLite
/// opened it and left an empty file, and an empty quillo.confirmed held the next start. A database
/// elsewhere (`app.env`) opens; the next start runs (src/db.rs connect; FEEDBACK85.md Q-1340, found
/// by the verdict review of 8989870; branch feedback82-85).
#[test]
fn no_file_is_made_by_a_refused_db_open_or_db_connect() {
    let d = dir("openmade");
    let (settings, confirmed, source) = ("it is the program's settings (.env)", "it is what the program was confirmed to reach (quillo.confirmed)", "it is one of the program's own files");
    let calls: Vec<(&str, &str, &str)> = vec![
        ("db.open(\"quillo.confirmed\")", "quillo.confirmed", confirmed),
        ("db.connect(\"quillo.confirmed\")", "quillo.confirmed", confirmed),
        ("db.connect(\".env.local\")", ".env.local", settings),
        ("db.connect(\".ENV\")", ".ENV", settings),
        ("db.connect(\"main.qwe\")", "main.qwe", source),
        ("db.connect(\"app.env\")", "", ""),
    ];
    let lines: Vec<String> = calls.iter().map(|(c, _, _)| format!("print(said({}))", c)).collect();
    let main = format!("let said = (r) => is_err(r) ? r.error : \"ran\"\n{}\nexit(0)\n", lines.join("\n"));
    write(&d, "main.qwe", &main);
    let (code, out) = quillo_env(&d, &["run", "--no-check", "main.qwe"], 120, &[]);
    assert_eq!(code, Some(0), "{}", out);
    let printed: Vec<&str> = out.lines().filter(|l| l.starts_with("db: ") || *l == "ran").collect();
    assert_eq!(printed.len(), calls.len(), "{}", out);
    for ((call, file, why), line) in calls.iter().zip(&printed) {
        if file.is_empty() {
            assert_eq!(*line, "ran", "{}: {}", call, out);
        } else {
            assert!(line.ends_with(&format!("{}: {}", file, why)), "{}: {}", call, line);
        }
    }
    for f in ["quillo.confirmed", ".env.local", ".ENV", ".env"] {
        assert!(!d.join(f).exists(), "{} was made", f);
    }
    assert_eq!(std::fs::read_to_string(d.join("main.qwe")).unwrap(), main);
    assert!(d.join("app.env").exists());
    let (code, out) = quillo_env(&d, &["run", "--no-check", "main.qwe"], 120, &[]);
    assert_eq!(code, Some(0), "the next start: {}", out);
}

/// `new_tenant` makes no tenant whose database, or a folder made for it, is where a write of the
/// program's is refused, and says so: `db.open("{tenant}.confirmed")` then `new_tenant("quillo",
/// ...)` made an empty quillo.confirmed and gave the tenant's function's value, as if it were made.
/// So `.env.{tenant}` and `{tenant}.confirmed/x.db` (the folder quillo.confirmed); nothing is made
/// (src/db.rs claim_tenant, made_refused; FEEDBACK85.md Q-1340, found by the verdict review of
/// 8989870; branch feedback82-85).
#[test]
fn a_tenant_where_a_write_of_the_programs_is_refused_is_not_made() {
    let d = dir("tenantmade");
    let (settings, confirmed) = ("it is the program's settings (.env)", "it is what the program was confirmed to reach (quillo.confirmed)");
    for (target, tenant, file, why) in [("{tenant}.confirmed", "quillo", "quillo.confirmed", confirmed), (".env.{tenant}", "prod", ".env.prod", settings), ("{tenant}.confirmed/x.db", "quillo", "quillo.confirmed", confirmed)] {
        write(&d, "main.qwe", &format!("tenant_from(\"example.com\")\ndb.open({})\nprint(new_tenant({}, () => \"made\"))\nexit(0)\n", lit(target), lit(tenant)));
        let (code, out) = quillo_env(&d, &["run", "--no-check", "main.qwe"], 120, &[]);
        assert_ne!(code, Some(0), "{}: {}", target, out);
        assert!(!out.lines().any(|l| l == "made"), "{}: {}", target, out);
        assert!(out.contains(&format!("`new_tenant`: ./{}: {}", file, why)), "{}: {}", target, out);
        let mut left: Vec<String> = std::fs::read_dir(d.join(".")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != "main.qwe" && n != "cmd.log").collect();
        left.sort();
        assert!(left.is_empty(), "{}: {:?}", target, left);
    }
}
