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
        ("db.exec", "create or replace index kit_i on kits (x)", runs("`create index`")),
        ("db.exec", "alter", runs(ALTER_UNPLACED)),
        ("db.exec", "alter \"kits\" add y int", runs(ALTER_UNPLACED)),
        ("db.exec", "with kit_w as (select 1) create table kit_x (x int)", runs(CREATE_INSIDE)),
        ("db.query", "select nextval('kit_seq'", runs("the function nextval given a table's name worked out as it runs")),
        ("db.exec", "drop", begins("drop")),
        ("db.exec", "drop \"kits\"", begins("drop")),
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
/// alone; `explain` alone, of `explain`, and of `(select ...)`; `explain extended grant` and
/// `explain format = json show` judged by the word after them; a table's engine by a quoted name
/// (`` `connect` ``, `` `CONNECT` `` with its file) or by a string, or none after `engine`; `drop
/// temporary function`, `drop` of a quoted name and alone; a `create` after `explain`. Run: `alter
/// ignore table`, `alter online table` and `alter online ignore table`, whose columns are added;
/// `explain format = json`, `explain extended` and `explain partitions` of a select; a table whose
/// engine is `` `InnoDB` ``; `drop temporary table if exists`; a statement with a `;` at its end.
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
        ("db.query", "explain", begins("explain")),
        ("db.query", "explain explain select 1", begins("explain")),
        ("db.query", "explain (select 1)", begins("explain")),
        ("db.query", "explain extended grant select on kit_t to root", begins("grant")),
        ("db.query", "explain format = json show tables", begins("show")),
        ("db.exec", "create table kit_c (x int) engine = `connect`", reaches("`engine = connect`")),
        ("db.exec", "create table kit_c (x int) engine = `CONNECT` table_type = DOS file_name = '/etc/hosts'", reaches("`engine = connect`")),
        ("db.exec", "create table kit_c (x int) engine = 'connect'", reaches("`engine = `")),
        ("db.exec", "alter table kit_t engine", reaches("`engine = `")),
        ("db.exec", "drop temporary function kit_f", begins("drop function")),
        ("db.exec", "drop `kit_t`", begins("drop")),
        ("db.exec", "drop", begins("drop")),
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
/// for each tenant says the error of that shard's connection, outside a tenant, before any shard
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
    assert!(out.contains(&format!("all: err(\"db: {}\")\ncount: 2\n", no_tenant)), "{}", out);
    assert!(column(&d.join("data/a.db"), "select x from t").is_empty());
    std::fs::remove_file(d.join("quillo.confirmed")).unwrap();
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains(&format!("all: err(\"db: shard 2: {}\")\n", no_tenant)), "{}", out);
    assert_eq!(column(&d.join("data/a.db"), "select x from t"), ["caf\u{e9}"]);
}
