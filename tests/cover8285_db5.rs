//! Edge tests of the fixes to the hold of Quillo's own tables (`_quillo_*`) under a
//! `quillo.confirmed` that the coverage tests of 0038a97 found (FEEDBACK83.md, Q-1300): every way a
//! taken file's SQL reaches a database is judged as `db.query`'s is (`db.shards(...).all`, a row of
//! `db.exec_many`), a statement kept compiled is judged for the code that runs it, SQLite's
//! authorizer refuses a read of one of them through the main file's view or trigger, and a
//! statement is read as PostgreSQL and MySQL read it, its names and strings with their escapes
//! worked out (src/db.rs; branch feedback82-85). Each test asserts what a developer sees: the
//! answer, the message, the database's rows.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::temp_dir;

// ---- helpers, as tests/cover8285_run2.rs has them -----------------------------------------------

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
    Dir(temp_dir(&format!("cov8285db5-{}", name)))
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

/// `quillo check main.qwe` in `d`, which must pass.
fn checked(d: &Path) {
    let (code, out) = quillo_env(d, &["check", "main.qwe"], 60, &[]);
    assert_eq!(code, Some(0), "{}", out);
}

/// `quillo run main.qwe` in `d` with `vars`, a program with no routes, which ends once it has
/// loaded: its exit code and what it printed and logged.
fn run(d: &Path, vars: &[(&str, &str)]) -> (i32, String) {
    let (code, out) = quillo_env(d, &["run", "main.qwe"], 60, vars);
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

/// A Quillo string of `s`, as written in a `.qwe` file: `\r` written as its escape, which a file
/// read as source would take for a line's end.
fn lit(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r"))
}

/// A kit taken for `Badge`, whose `steal()` makes each call of `calls` (a function, `db.query` or
/// `db.exec`, and its SQL) and says what each gave, a line each: the error, or `ran`.
fn kit(calls: &[(&str, &str)]) -> String {
    let lines: Vec<String> = calls.iter().map(|(f, sql)| format!("    said({}({}))", f, lit(sql))).collect();
    format!(
        "component Badge() {{\n  return <b>b</b>\n}}\nfn steal() {{\n  let said = (r) => is_err(r) ? r.error : \"ran\"\n  return \"kit:\\n\" + join([\n{}\n  ], \"\\n\")\n}}\n",
        lines.join(",\n")
    )
}

/// What the text check says when a taken file's SQL names one of Quillo's own tables.
fn own_named(file: &str) -> String {
    format!("db: Quillo's own tables (_quillo_...) are refused for {}'s SQL where a quillo.confirmed holds the run: they hold what Quillo does (push subscriptions, jobs, tokens), and this statement names one", file)
}

/// What SQLite's authorizer says when it refuses a taken file's write to one of Quillo's own tables.
const OWN_WRITE: &str = "db: Quillo's own tables (_quillo_...) are not written by a taken file's SQL where a quillo.confirmed holds the run";

/// What SQLite's authorizer says when it refuses a taken file's read of one of Quillo's own tables.
const OWN_READ: &str = "db: Quillo's own tables (_quillo_...) are not read by a taken file's SQL where a quillo.confirmed holds the run";

/// The quillo.confirmed of the SQLite tests: the main file writes `data/`, the kit the program's
/// database file alone.
const CONFIRMED: &str = "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/app.db\"]\n";

/// The main file of the SQLite tests, which takes `uses` of the kit: Quillo's own table stands in
/// as `_quillo_mine`, written by a trigger after an insert into `users` and read by the view
/// `mine_view`; then `body`.
fn sqlite_main(uses: &str, body: &str) -> String {
    format!(
        "import kit/k.qwe use {}\ndb.open(\"data/app.db\")\ndb.exec(\"create table if not exists users (name text)\")\ndb.exec(\"create table if not exists kits (name text)\")\ndb.exec(\"create table if not exists _quillo_mine (x text)\")\ndb.exec(\"create trigger if not exists audit after insert on users begin insert into _quillo_mine values (new.name); end\")\ndb.exec(\"create view if not exists mine_view as select x from _quillo_mine\")\ndb.exec(\"create view if not exists names as select name from users\")\n{}",
        uses, body
    )
}

// ---- db.exec_many: its rows are the program's statements -----------------------------------------

/// Under a quillo.confirmed a taken file's `db.exec_many` is judged by SQLite's authorizer row by
/// row, as `db.exec` is: an insert of two rows, or of one, into `users`, whose main-file trigger
/// writes `_quillo_mine`, is refused whole, where it ran as Quillo's own statement and wrote it;
/// its rows into a table with no such trigger go in, none for no rows, and `_quillo_mine` named in
/// its text is refused by the text. The main file's same `exec_many` is the program's own, and its
/// trigger writes (src/db.rs exec_many, FEEDBACK83.md Q-1300, found by the coverage
/// tests; branch feedback82-85).
#[test]
fn a_taken_files_exec_many_is_judged_by_sqlites_authorizer_row_by_row() {
    let d = dir("many");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "main.qwe", &sqlite_main("Badge, steal", "print(\"main: \" + str(db.exec_many(\"insert into users (name) values (?)\", [[\"m1\"], [\"m2\"]])))\nprint(steal())\n"));
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let said = (r) => is_err(r) ? r.error : str(r)\n  return \"kit:\\n\" + join([\n    said(db.exec_many(\"insert into users (name) values (?)\", [[\"k1\"], [\"k2\"]])),\n    said(db.exec_many(\"insert into users (name) values (?)\", [[\"k3\"]])),\n    said(db.exec_many(\"insert into kits (name) values (?)\", [[\"k1\"], [\"k2\"]])),\n    said(db.exec_many(\"insert into kits (name) values (?)\", [])),\n    said(db.exec_many(\"insert into _QUILLO_mine (x) values (?)\", [[\"k4\"]]))\n  ], \"\\n\")\n}\n",
    );
    write(&d, "quillo.confirmed", CONFIRMED);
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let want = ["main: { changes: 2 }", "kit:", OWN_WRITE, OWN_WRITE, "{ changes: 2 }", "{ changes: 0 }", &own_named("kit/k.qwe")].join("\n");
    assert!(out.contains(&format!("{}\n", want)), "{}", out);
    let db = d.join("data/app.db");
    assert_eq!(column(&db, "select x from _quillo_mine order by rowid"), ["m1", "m2"]);
    assert_eq!(column(&db, "select name from users order by rowid"), ["m1", "m2"]);
    assert_eq!(column(&db, "select name from kits order by rowid"), ["k1", "k2"]);
}

// ---- authorize: a read of Quillo's tables through the main file's view or trigger ---------------

/// Under a quillo.confirmed SQLite's authorizer refuses a taken file's statement that reads one of
/// Quillo's own tables through the main file's view (its column, `count(*)`, which reads none of
/// them, and `select 1`), or through a trigger its insert fires, which reads one to write the
/// program's table: each says why, and nothing lands. A view of the program's own tables reads as
/// it did, and the main file reads its view (src/db.rs authorize, SQLITE_READ, FEEDBACK83.md
/// Q-1300, found by the coverage tests; branch feedback82-85).
#[test]
fn a_taken_files_read_of_quillos_tables_through_the_main_files_view_or_trigger_is_refused() {
    let d = dir("read");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(
        &d,
        "main.qwe",
        &sqlite_main("Badge, steal", "db.exec(\"create table if not exists log (x text)\")\ndb.exec(\"create trigger if not exists copy after insert on kits begin insert into log select x from _quillo_mine; end\")\ndb.exec(\"insert into users values ('m1')\")\nprint(\"main: \" + str(db.query(\"select x from mine_view\")))\nprint(steal())\n"),
    );
    write(
        &d,
        "kit/k.qwe",
        &kit(&[
            ("db.query", "select x from mine_view"),
            ("db.query", "select count(*) as n from mine_view"),
            ("db.one", "select 1 as one from mine_view"),
            ("db.exec", "insert into kits values ('k1')"),
            ("db.query", "select name from names"),
            ("db.query", "select count(*) as n from users"),
        ]),
    );
    write(&d, "quillo.confirmed", CONFIRMED);
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let want = ["main: [{ x: \"m1\" }]", "kit:", OWN_READ, OWN_READ, OWN_READ, OWN_READ, "ran", "ran"].join("\n");
    assert!(out.contains(&format!("{}\n", want)), "{}", out);
    let db = d.join("data/app.db");
    assert!(column(&db, "select name from kits").is_empty());
    assert!(column(&db, "select x from log").is_empty());
}

// ---- compiled_as: a statement kept compiled is the code's that runs it ------------------------------

/// Under a quillo.confirmed a statement kept compiled is judged for each code that runs it: the
/// kit's insert, refused, leaves nothing for the main file's same text, which runs and fires its
/// trigger; run by the kit again after the main file, by `db.query`, `db.one` or `db.exec`, it is
/// refused again. The kit's statements that touch nothing of Quillo's run every time, as the main
/// file's same text does, unicode values too; an error says the program's SQL, at its own offset,
/// not the text it is kept under; an empty text is said as ever (src/db.rs compiled_as,
/// FEEDBACK83.md Q-1300, found by the coverage tests; branch feedback82-85).
#[test]
fn a_statement_kept_compiled_is_judged_for_each_code_that_runs_it() {
    let d = dir("compiled");
    std::fs::create_dir_all(d.join("data")).unwrap();
    let insert = "insert into users (name) values (?)";
    let kits = "insert into kits (name) values (?)";
    write(
        &d,
        "main.qwe",
        &sqlite_main("Badge, first, again", &format!(
            "print(first())\nprint(\"main: \" + str(db.exec({i}, \"m1\")) + \" \" + str(db.query({i} + \" returning name\", \"m2\")) + \" \" + str(db.exec({k}, \"m3\")))\nprint(again())\n",
            i = lit(insert),
            k = lit(kits)
        )),
    );
    write(
        &d,
        "kit/k.qwe",
        &format!(
            "component Badge() {{\n  return <b>b</b>\n}}\nlet said = (r) => is_err(r) ? r.error : str(r)\nfn first() {{\n  return \"first: \" + join([said(db.exec({i}, \"k1\")), said(db.exec({k}, \"k1\")), said(db.exec({k}, \"caf\u{e9} \u{1f600}\"))], \" | \")\n}}\nfn again() {{\n  return \"again: \" + join([said(db.exec({i}, \"k2\")), said(db.query({i} + \" returning name\", \"k3\")), said(db.one({i} + \" returning name\", \"k4\")), said(db.exec({k}, \"k2\")), said(db.query(\"selec name from users where name = ?\", \"x\")), said(db.query(\"select name fro users where name = ?\", \"x\")), said(db.query(\"\"))], \" | \")\n}}\n",
            i = lit(insert),
            k = lit(kits)
        ),
    );
    write(&d, "quillo.confirmed", CONFIRMED);
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains(&format!("first: {} | {{ changes: 1, last_id: 1 }} | {{ changes: 1, last_id: 2 }}\n", OWN_WRITE)), "{}", out);
    assert!(out.contains("main: { changes: 1, last_id: 1 } [{ name: \"m2\" }] { changes: 1, last_id: 3 }\n"), "{}", out);
    let errors = [
        OWN_WRITE,
        OWN_WRITE,
        OWN_WRITE,
        "{ changes: 1, last_id: 4 }",
        "db: near \"selec\": syntax error in selec name from users where name = ? at offset 0",
        "db: near \"users\": syntax error in select name fro users where name = ? at offset 16",
        "db: the SQL has no statement in it (it is empty, or only comments)",
    ];
    assert!(out.contains(&format!("again: {}\n", errors.join(" | "))), "{}", out);
    assert!(!out.contains("/*"), "{}", out);
    let db = d.join("data/app.db");
    assert_eq!(column(&db, "select x from _quillo_mine order by rowid"), ["m1", "m2"]);
    assert_eq!(column(&db, "select name from kits order by rowid"), ["k1", "caf\u{e9} \u{1f600}", "m3", "k2"]);
}

// ---- shards.all ---------------------------------------------------------------------------------

/// Under a quillo.confirmed `db.shards(...).all` judges a taken file's SQL as `db.query` does, on
/// two shards: Quillo's own table named in brackets, in quotes in other letters, or in backquotes is
/// refused, and through a shard's connection (`.on(key)`); SQL naming none runs on each shard and
/// gives both shards' rows. The main file's own `all` reads its table (src/db.rs shard_all,
/// FEEDBACK83.md Q-1300, found by the coverage tests; branch feedback82-85).
#[test]
fn shards_all_judges_a_taken_files_sql_in_each_spelling() {
    let d = dir("shards");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\nlet s = db.shards([\"data/app.db\", \"data/two.db\"])\ns.all(\"create table if not exists _quillo_mine (x text)\")\ns.all(\"create table if not exists users (name text)\")\ndb.connect(\"data/app.db\").exec(\"insert into users values ('a1')\")\ndb.connect(\"data/two.db\").exec(\"insert into users values ('t1')\")\ndb.connect(\"data/two.db\").exec(\"insert into _quillo_mine values ('own two')\")\nprint(\"main: \" + str(s.all(\"select x from _quillo_mine\")))\nprint(steal())\n",
    );
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let said = (r) => is_err(r) ? r.error : str(r)\n  let s = db.shards([\"data/app.db\", \"data/two.db\"])\n  return \"kit:\\n\" + join([\n    said(s.all(\"select x from [_quillo_mine]\")),\n    said(s.all(\"select x from \\\"_QUILLO_MINE\\\"\")),\n    said(s.all(\"select x from `_quillo_mine`\")),\n    said(s.on(\"key\").query(\"select x from _quillo_mine\")),\n    said(s.all(\"select name from users order by name\"))\n  ], \"\\n\")\n}\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/app.db\", \"data/two.db\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let refused = own_named("kit/k.qwe");
    let want = ["main: [{ x: \"own two\" }]", "kit:", &refused, &refused, &refused, &refused, "[{ name: \"a1\" }, { name: \"t1\" }]"].join("\n");
    assert!(out.contains(&format!("{}\n", want)), "{}", out);
}

// ---- migrations ---------------------------------------------------------------------------------

/// Under a quillo.confirmed a taken file's migration is read as each database would read it, its
/// database not known as the folder is judged: on SQLite a file naming Quillo's own table only as
/// PostgreSQL spells it (`U&"\005F..."`), or as MySQL does (`'_qu\illo_mine'`), is refused before any
/// file runs, its name said; one naming none is applied, unicode in it too (src/db.rs migrate,
/// own_table_named, FEEDBACK83.md Q-1300, found by the coverage tests; branch feedback82-85).
#[test]
fn a_taken_files_migration_naming_quillos_tables_as_a_server_spells_them_is_refused() {
    let d = dir("migrate");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\nprint(steal())\n");
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let c = db.connect(\"data/k.db\")\n  return \"applied: \" + str(c.migrate(\"kit/m\"))\n}\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/k.db\"]\nfiles = [\"kit/m/\"]\n");
    for (name, text) in [("001_a.sql", "create table t (x text);\ninsert into t select x from U&\"\\005Fquillo_mine\";\n"), ("001_a.sql", "create table t (x text);\ninsert into t values ('_qu\\illo_mine');\n")] {
        write(&d, &format!("kit/m/{}", name), text);
        checked(&d);
        let (code, out) = run(&d, &[]);
        assert_ne!(code, 0, "{}", out);
        assert!(out.contains("db.migrate: kit/m/001_a.sql: Quillo's own tables (_quillo_...) are refused for kit/k.qwe's SQL"), "{}", out);
        assert!(!out.contains("applied: "), "{}", out);
        assert!(!d.join("data/k.db").exists() || column(&d.join("data/k.db"), "select name from sqlite_master where name = 't'").is_empty());
    }
    write(&d, "kit/m/001_a.sql", "create table t (x text);\ninsert into t values ('caf\u{e9}');\n");
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("applied: [\"001_a.sql\"]\n"), "{}", out);
    assert_eq!(column(&d.join("data/k.db"), "select x from t"), ["caf\u{e9}"]);
}

// ---- own_table_named: names and strings as each database reads them ------------------------------

/// Under a quillo.confirmed on SQLite a name of Quillo's own table in letters outside ASCII that a
/// database's case makes ASCII ones (İ and ı as i) is refused to a taken file, as MySQL reads
/// `_QUİLLO_MINE` as `_quillo_mine`; a name in other letters (`café`, `_quillö_x`), and a value with
/// İ in it, run (src/db.rs names_own, FEEDBACK83.md Q-1300, found by the coverage
/// tests; branch feedback82-85).
#[test]
fn a_name_of_quillos_tables_in_letters_a_database_folds_is_refused_to_a_taken_file() {
    let d = dir("fold");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "main.qwe", &sqlite_main("Badge, steal", "print(steal())\n"));
    write(
        &d,
        "kit/k.qwe",
        &kit(&[
            ("db.exec", "create table _QU\u{130}LLO_x (a)"),
            ("db.exec", "create table _qu\u{131}llo_x (a)"),
            ("db.exec", "create table caf\u{e9} (a)"),
            ("db.exec", "insert into kits values ('_QU\u{130}LLO')"),
            ("db.exec", "create table _quill\u{f6}_x (a)"),
        ]),
    );
    write(&d, "quillo.confirmed", CONFIRMED);
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let refused = own_named("kit/k.qwe");
    let want = ["kit:", &refused, &refused, "ran", "ran", "ran"].join("\n");
    assert!(out.contains(&format!("{}\n", want)), "{}", out);
}

/// The SQL of a string nested `n` strings deep: `select '''x''' as s` is two.
fn nested(n: usize) -> String {
    let mut inner = "x".to_string();
    for _ in 0..n {
        inner = format!("'{}'", inner.replace('\'', "''"));
    }
    format!("select {} as s", inner)
}

/// Under a quillo.confirmed on PostgreSQL a taken file's SQL is read as PostgreSQL reads it: Quillo's
/// own table named as `U&"..."` (its escape the backslash, `\+`, or another by `UESCAPE`, lower-case
/// hex, in a schema), as a string by `E'...'` (hex, octal, `\u`, a backslash before a letter), by
/// `U&'...'`, in parts across a line (a comment too) or in a `U&` string's parts, in a string read
/// as SQL (`query_to_xml`, `E'...'`'s text, a `do` block's body, an `execute` in it), or a plain
/// string's backslashes as escapes where a session would turn `standard_conforming_strings` off:
/// each is refused, with nothing read or written. Strings nested 7 and 8 deep run, 9 deep is taken as
/// naming one; SQL naming none runs, `U&` and `E'...'` with unicode too; the main file's own `U&`
/// name reads its table (src/db.rs read_own, FEEDBACK83.md Q-1300, found by the coverage tests;
/// branch feedback82-85).
#[test]
fn on_postgresql_a_taken_files_sql_is_read_as_postgresql_reads_it() {
    let Some(pg) = support::postgres("cov8285db5") else {
        eprintln!("PostgreSQL is not installed: skipped");
        return;
    };
    let url = pg.database("read_as");
    let d = dir("pg");
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists _quillo_mine (x text)\")\ndb.exec(\"insert into _quillo_mine values ('quillo own row')\")\nprint(steal())\nprint(\"mine: \" + str(db.query(\"select x from U&\\\"\\\\005Fquillo_mine\\\"\")))\n");
    let refused_calls: Vec<(&str, String)> = vec![
        ("db.query", r#"select x from U&"\005Fquillo_mine""#.into()),
        ("db.exec", r#"insert into U&"\005Fquillo_mine" values ('kit')"#.into()),
        ("db.query", r#"select x from U&"!005Fquillo_mine" UESCAPE '!'"#.into()),
        ("db.query", r#"select x from U&"\+00005Fquillo_mine""#.into()),
        ("db.query", r#"select x from public.u&"\005fquillo_mine""#.into()),
        ("db.query", r#"select x from U&"\005F\0071\0075\0069llo_mine""#.into()),
        ("db.query", r#"select table_to_xml(E'\x5fquillo_mine', true, false, '')"#.into()),
        ("db.query", r#"select table_to_xml(E'\137quillo_mine', true, false, '')"#.into()),
        ("db.query", r#"select table_to_xml(e'_\quillo_mine', true, false, '')"#.into()),
        ("db.query", r#"select table_to_xml(E'\u005Fquillo_mine', true, false, '')"#.into()),
        ("db.query", r#"select table_to_xml(U&'\005Fquillo_mine', true, false, '')"#.into()),
        ("db.query", r#"select table_to_xml(U&'!005Fquillo_mine' UESCAPE '!', true, false, '')"#.into()),
        ("db.query", "select table_to_xml('_qu'\n'illo_mine', true, false, '')".into()),
        ("db.query", "select table_to_xml('_qu' -- c\n'illo_mine', true, false, '')".into()),
        ("db.query", "select table_to_xml(U&'\\005F'\n'quillo_mine', true, false, '')".into()),
        ("db.query", r#"select query_to_xml('select x from U&"\005Fquillo_mine"', true, false, '')"#.into()),
        ("db.query", r#"select query_to_xml(E'select x from U&"\\005Fquillo_mine"', true, false, '')"#.into()),
        ("db.exec", r#"do $$ begin insert into U&"\005Fquillo_mine" values ('kit do'); end $$"#.into()),
        ("db.exec", r#"do $f$ begin execute 'insert into U&"\005Fquillo_mine" values (''kit exec'')'; end $f$"#.into()),
        ("db.query", r#"select table_to_xml('\x5fquillo_mine', true, false, '')"#.into()),
        ("db.query", nested(9)),
    ];
    let run_calls: Vec<(&str, String)> = vec![
        ("db.query", nested(7)),
        ("db.query", nested(8)),
        ("db.query", "select 1 as one".into()),
        ("db.query", r#"select U&"\0061" from (select 1 as a) t"#.into()),
        ("db.query", r#"select E'caf\u00e9 \x41' as s, U&'\00e9' as t"#.into()),
        ("db.query", "select 'it''s' || $$ a $$ as s".into()),
    ];
    let calls: Vec<(&str, &str)> = refused_calls.iter().chain(run_calls.iter()).map(|(f, s)| (*f, s.as_str())).collect();
    write(&d, "kit/k.qwe", &kit(&calls));
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    let refused = own_named("kit/k.qwe");
    let mut want = vec!["kit:".to_string()];
    want.extend(std::iter::repeat_n(refused, refused_calls.len()));
    want.extend(std::iter::repeat_n("ran".to_string(), run_calls.len()));
    assert!(out.contains(&format!("{}\n", want.join("\n"))), "{}", out);
    assert!(out.contains("mine: [{ x: \"quillo own row\" }]\n"), "{}", out);
}

/// Under a quillo.confirmed on MySQL (MariaDB) a taken file's SQL is read as MySQL reads it: Quillo's
/// own table named `_QUİLLO_MINE`, which MySQL keeps in lower case as `_quillo_mine`, and the SQL of
/// an `execute immediate` (run as text by `db.exec`) naming it with a backslash before a letter, in
/// `"..."`, in two strings with a comment between, as a hex string (`X'...'`, `0x...`), with a
/// character set before it (`_utf8mb4'...'`), inside a `/*! */` comment or an `execute immediate`
/// of its own: each is refused, with nothing written. SQL naming none runs, a string with escapes
/// in it too (an `execute immediate` of it ran here until part B, by which a taken file's `execute
/// immediate` is refused whatever it runs: `on_mysql_a_taken_files_sql_run_from_text_is_refused`);
/// the main file reads its table
/// (src/db.rs read_own, FEEDBACK83.md Q-1300, found by the coverage tests; branch feedback82-85).
#[test]
fn on_mysql_a_taken_files_sql_is_read_as_mysql_reads_it() {
    let Some(my) = support::mariadb("cov8285db5") else {
        eprintln!("MariaDB is not installed: skipped");
        return;
    };
    let url = my.url("read_as");
    let d = dir("my");
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists _quillo_mine (x text)\")\ndb.exec(\"create table if not exists kits (x text)\")\ndb.exec(\"insert into _quillo_mine values ('quillo own row')\")\nprint(steal())\nprint(\"mine: \" + str(db.query(\"select x from _quillo_mine\")))\n");
    let hex: String = "insert into _quillo_mine values ('kit hex')".bytes().map(|b| format!("{:02x}", b)).collect();
    let refused_calls: Vec<(&str, String)> = vec![
        ("db.exec", "insert into _QU\u{130}LLO_MINE values ('kit')".into()),
        ("db.exec", r#"execute immediate 'insert into _qu\illo_mine values (''kit'')'"#.into()),
        ("db.exec", r#"execute immediate "insert into _qu\illo_mine values ('kit')""#.into()),
        ("db.exec", "execute immediate 'insert into _qu' /* c */ 'illo_mine values (1)'".into()),
        ("db.exec", format!("execute immediate X'{}'", hex)),
        ("db.exec", format!("execute immediate 0x{}", hex)),
        ("db.exec", r#"execute immediate _utf8mb4'insert into _qu\illo_mine values (2)'"#.into()),
        ("db.exec", r#"/*! execute immediate 'insert into _qu\illo_mine values (3)' */"#.into()),
        ("db.exec", r#"execute immediate 'execute immediate ''insert into _qu\\illo_mine values (4)'' '"#.into()),
    ];
    let run_calls: Vec<(&str, String)> = vec![
        ("db.exec", "insert into kits values ('caf\u{e9}')".into()),
        ("db.exec", r#"insert into kits values ('it\'s')"#.into()),
        ("db.query", "select 'it''s' as s".into()),
    ];
    let calls: Vec<(&str, &str)> = refused_calls.iter().chain(run_calls.iter()).map(|(f, s)| (*f, s.as_str())).collect();
    write(&d, "kit/k.qwe", &kit(&calls));
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    let refused = own_named("kit/k.qwe");
    let mut want = vec!["kit:".to_string()];
    want.extend(std::iter::repeat_n(refused, refused_calls.len()));
    want.extend(std::iter::repeat_n("ran".to_string(), run_calls.len()));
    assert!(out.contains(&format!("{}\n", want.join("\n"))), "{}", out);
    assert!(out.contains("mine: [{ x: \"quillo own row\" }]\n"), "{}", out);
}

// ---- db.connect: a connection another file opened ------------------------------------------------

/// Under a quillo.confirmed a taken file naming a database the main file has a connection to gets
/// it only where it was confirmed for that database, as a connection it opened itself would be:
/// confirmed for nothing, `db.connect("data/secret.db")`, the same spelled `./data/secret.db`, and
/// `db.shards([...])` of it are refused, and nothing is read; a second kit confirmed to write it gets
/// the same connection, in every call; the main file's own calls of it run. `quillo check` names the
/// kit's connection as not confirmed, as before (src/db.rs reuse_ok,
/// FEEDBACK83.md Q-1300, found by the coverage tests; branch feedback82-85).
#[test]
fn a_connection_another_file_opened_is_judged_for_the_file_that_names_it() {
    let d = dir("reuse");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\nimport lib/l.qwe use Card, read_secret\nlet s = db.connect(\"data/secret.db\")\ns.exec(\"create table if not exists secrets (v text)\")\ns.exec(\"insert into secrets values ('main secret')\")\nprint(steal())\nprint(\"lib: \" + read_secret() + \" \" + read_secret())\nprint(\"main: \" + str(db.connect(\"data/secret.db\").query(\"select v from secrets\")))\n",
    );
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let said = (r) => is_err(r) ? r.error : str(r.query(\"select v from secrets\"))\n  let shards = db.shards([\"data/secret.db\"])\n  return \"kit:\\n\" + join([said(db.connect(\"data/secret.db\")), said(db.connect(\"./data/secret.db\")), is_err(shards) ? shards.error : str(shards.all(\"select v from secrets\"))], \"\\n\")\n}\n",
    );
    write(&d, "lib/l.qwe", "component Card() {\n  return <i>c</i>\n}\nfn read_secret() = str(db.connect(\"data/secret.db\").query(\"select v from secrets\"))\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"lib/l.qwe\"]\nwrites = [\"data/secret.db\"]\n");
    // `quillo check` names the kit's connection, as it names each thing not confirmed; the run is
    // where it is refused.
    let (code, out) = quillo_env(&d, &["check", "main.qwe"], 60, &[]);
    assert_ne!(code, Some(0), "{}", out);
    assert!(out.contains("kit/k.qwe:7: error: writes data/secret.db: not confirmed for this file"), "{}", out);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let refused = "db: data/secret.db is not confirmed for kit/k.qwe (quillo confirm)";
    assert!(out.contains(&format!("kit:\n{}\n{}\n{}\n", refused, refused, refused)), "{}", out);
    assert!(out.contains("lib: [{ v: \"main secret\" }] [{ v: \"main secret\" }]\nmain: [{ v: \"main secret\" }]\n"), "{}", out);
    assert_eq!(out.matches("main secret").count(), 3, "{}", out);
}

// ---- text_run: SQL a server runs from text (part B) ---------------------------------------------

/// What the hold says when a taken file's SQL has the database run SQL from text, `what` being
/// what it found.
fn text_run(what: &str, file: &str) -> String {
    format!("db: {} has the database run SQL from text, or name a table from text, which is refused for {}'s SQL where a quillo.confirmed holds the run: what it does is worked out as it runs, where no reading of the statement sees it, Quillo's own tables too. Run it from the main file's SQL, or from a migration the main file runs", what, file)
}

/// A kit whose `steal()` makes each call of `calls` (a function and its SQL, and a value after it
/// when there is one), and says what each gave, a line each: the error, or `ran`.
fn kit_with(calls: &[(&str, &str, Option<&str>)]) -> String {
    let lines: Vec<String> = calls
        .iter()
        .map(|(f, sql, v)| match v {
            Some(v) => format!("    said({}({}, {}))", f, lit(sql), lit(v)),
            None => format!("    said({}({}))", f, lit(sql)),
        })
        .collect();
    format!(
        "component Badge() {{\n  return <b>b</b>\n}}\nfn steal() {{\n  let said = (r) => is_err(r) ? r.error : \"ran\"\n  return \"kit:\\n\" + join([\n{}\n  ], \"\\n\")\n}}\n",
        lines.join(",\n")
    )
}

/// What the hold says when a taken file's SQL reaches the server's files or programs.
fn reaches(what: &str, file: &str) -> String {
    format!("db: {} reaches the database server's own files or programs, or another database, which is refused for {}'s SQL where a quillo.confirmed holds the run. Run it from the main file's SQL, or from a migration the main file runs", what, file)
}

/// What the hold says when a taken file's SQL names a catalog of other tables' data or SQL.
fn reads(what: &str, file: &str) -> String {
    format!("db: {} holds other tables' data or other statements' SQL, which is refused for {}'s SQL where a quillo.confirmed holds the run. Run it from the main file's SQL, or from a migration the main file runs", what, file)
}

/// What the hold says when a taken file's SQL cannot be read for certain.
fn unsure(what: &str, file: &str) -> String {
    format!("db: this statement has {}, which this reading cannot read for certain as the server would, and what it cannot read is refused for {}'s SQL where a quillo.confirmed holds the run. Run it from the main file's SQL, or from a migration the main file runs", what, file)
}

/// The refusal `w` says: `reaches:`, `reads:` or `unsure:` before what was found for those kinds,
/// what was found alone for SQL run from text (`text_run`).
fn refusal(w: &str, file: &str) -> String {
    match (w.strip_prefix("reaches:"), w.strip_prefix("reads:"), w.strip_prefix("unsure:")) {
        (Some(x), _, _) => reaches(x, file),
        (_, Some(x), _) => reads(x, file),
        (_, _, Some(x)) => unsure(x, file),
        _ => text_run(w, file),
    }
}

/// What MySQL's `/*! */` is refused as.
const EXECUTABLE_COMMENT: &str = "unsure:a `/*! */` comment, whose SQL runs or not as the server's version says";

/// The lines `steal()` gives for `refused` (each the hold's refusal of what it names, `refusal`)
/// then `ran`.
fn kit_said(refused: &[&str], ran: usize, file: &str) -> String {
    let mut want = vec!["kit:".to_string()];
    want.extend(refused.iter().map(|w| refusal(w, file)));
    want.extend(std::iter::repeat_n("ran".to_string(), ran));
    format!("{}\n", want.join("\n"))
}

/// Under a quillo.confirmed on PostgreSQL a taken file's statement that has the database run SQL
/// from text, or name a table from text, is refused, says what it found and what to do, and
/// nothing of it runs: `do` (in capitals, in mixed case after a comment and spaces, and as a second
/// statement after `;`), `prepare`, `execute` (after `explain analyze` too), `call`, `create [or
/// replace] function`/`procedure`, `create trigger`, `create schema` (with a trigger in it),
/// `create rule`, `create event trigger`, `copy` (to a program), the `*_to_xml` functions (in a schema, by a quoted
/// name, by a `U&` name whose escape hides a letter), `ts_stat`, `dblink`, `nextval`/`setval`/`currval`, `to_regclass`, and `regclass` and
/// `oid` as types (`::regclass` of a text, of a value, `::pg_catalog.regclass`, `cast(... as
/// regclass)`, `regclass '...'`, `::oid` of a number). The same words in a string, a dollar quote,
/// a comment or a quoted name run, as do `create table`, `oid` as a column and SQL naming none of
/// them. The main file's same statements run, and the kit's without a quillo.confirmed (src/db.rs
/// text_run, FEEDBACK83.md Q-1300, the user's decision of 2026-11-24; branch feedback82-85).
/// Changed by review 6 of aebe60e's fixes (fail closed): `create schema` is refused for what it
/// makes, a schema not being on what a taken file may make, before the trigger in it; and `copy`
/// for reaching the server's files or programs, whatever it reaches.
/// Changed by review 7 of 64df6c0's fixes: a sequence's function given one string runs, the name
/// judged with the statement's others, and is refused given a name worked out as it runs (a
/// concatenation, a parameter, a cast); `oid` as `pg_class`'s column is no longer a line that runs,
/// `pg_...` names being refused.
#[test]
fn on_postgresql_a_taken_files_sql_run_from_text_is_refused() {
    let Some(pg) = support::postgres("cov8285db5b") else {
        eprintln!("PostgreSQL is not installed: skipped");
        return;
    };
    let url = pg.database("text_run");
    let d = dir("pg-text");
    let refused: Vec<(&str, &str, Option<&str>, &str)> = vec![
        ("db.exec", "DO $$ begin perform 1; end $$", None, "`do`"),
        ("db.exec", "/* a comment */  \n Do $$ begin perform 1; end $$", None, "`do`"),
        ("db.exec", "select 1; do $$ begin perform 1; end $$", None, "`do`"),
        ("db.exec", "prepare kit_p as select 1", None, "`prepare`"),
        ("db.exec", "execute kit_p", None, "`execute`"),
        ("db.exec", "explain analyze execute kit_p", None, "`execute`"),
        ("db.exec", "call kit_proc()", None, "`call`"),
        ("db.exec", "create function kit_f() returns int language sql as $$ select 1 $$", None, "`create function`"),
        ("db.exec", "CREATE OR REPLACE PROCEDURE kit_p() language sql as $$ select 1 $$", None, "`create procedure`"),
        ("db.exec", "create trigger kit_t after insert on kits for each row execute function main_f()", None, "`create trigger`"),
        ("db.exec", "create schema kit_s create table kit_t (x int) create trigger kit_tt after insert on kit_t for each row execute function main_f()", None, "`create schema`"),
        ("db.exec", "create rule kit_r as on insert to kits do instead nothing", None, "`create rule`"),
        ("db.exec", "create event trigger kit_e on ddl_command_start execute function main_f()", None, "`create event`"),
        ("db.exec", "copy kits to program 'cat'", None, "reaches:`copy`"),
        ("db.query", "select query_to_xml('select 1', true, false, '')", None, "the function query_to_xml"),
        ("db.query", "select pg_catalog.table_to_xmlschema('kits', true, false, '')", None, "the function table_to_xmlschema"),
        ("db.query", "select \"cursor_to_xml\"('c', 1, true, false, '')", None, "the function cursor_to_xml"),
        ("db.query", r#"select U&"\0071uery_to_xml"('select 1', true, false, '')"#, None, "the function query_to_xml"),
        ("db.query", "select ts_stat('select 1')", None, "the function ts_stat"),
        ("db.query", "select dblink('host=x', 'select 1')", None, "the function dblink"),
        ("db.query", "select nextval('kit' || '_seq')", None, "the function nextval given a table's name worked out as it runs"),
        ("db.query", "select SetVal($1, 1)", Some("kit_seq"), "the function setval given a table's name worked out as it runs"),
        ("db.query", "select currval(kit_seq::text)", None, "the function currval given a table's name worked out as it runs"),
        ("db.query", "select to_regclass('kits')", None, "the function to_regclass"),
        ("db.query", "select 'kits'::regclass", None, "a table's name (regclass) worked out as it runs"),
        ("db.query", "select $1::regclass", Some("kits"), "a table's name (regclass) worked out as it runs"),
        ("db.query", "select x::pg_catalog.regclass from (select 'kits' as x) t", None, "a table's name (regclass) worked out as it runs"),
        ("db.query", "select cast('kits' as REGCLASS)", None, "a table's name (regclass) worked out as it runs"),
        ("db.query", "select regclass 'kits'", None, "a table's name (regclass) worked out as it runs"),
        ("db.query", "select 16384::oid", None, "a table's number (oid) worked out as it runs"),
    ];
    let ran: Vec<(&str, &str, Option<&str>)> = vec![
        ("db.query", "select 'do $$ x $$; execute p; call p(); create function f(); query_to_xml(' as s", None),
        ("db.query", "select $q$ do execute call regclass nextval( $q$ as s", None),
        ("db.query", "select 1 /* do execute call create function query_to_xml( ::regclass */ as one -- do\n", None),
        ("db.query", "select 1 as \"do\", 2 as \"execute\", 3 as \"call\", 4 as \"regclass\"", None),
        ("db.query", "select count(*) as n from kits where x is null or x > 0", None),
        ("db.query", "select nextval('kit_seq') as n", None),
        ("db.exec", "create table if not exists kits2 (x int)", None),
        ("db.query", "select count(*) as n from kits", None),
    ];
    let mut calls: Vec<(&str, &str, Option<&str>)> = refused.iter().map(|(f, s, v, _)| (*f, *s, *v)).collect();
    calls.extend(ran.iter().copied());
    write(&d, "kit/k.qwe", &kit_with(&calls));
    let main = "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists kits (x int)\")\ndb.exec(\"create sequence if not exists kit_seq\")\ndb.exec(\"create or replace function main_f() returns trigger language plpgsql as $$ begin return new; end $$\")\nlet said = (r) => is_err(r) ? r.error : \"ran\"\nprint(\"main: \" + join([said(db.exec(\"do $$ begin perform 1; end $$\")), said(db.query(\"select query_to_xml('select 1', true, false, '') as x\")), said(db.query(\"select 'kits'::regclass::text as t\")), said(db.query(\"select nextval('kit_seq') as n\"))], \" \"))\nprint(steal())\nprint(\"functions: \" + str(db.query(\"select proname from pg_proc where proname like 'kit%' order by 1\")))\n";
    write(&d, "main.qwe", main);
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("main: ran ran ran ran\n"), "{}", out);
    let whats: Vec<&str> = refused.iter().map(|(_, _, _, w)| *w).collect();
    assert!(out.contains(&kit_said(&whats, ran.len(), "kit/k.qwe")), "{}", out);
    assert!(out.contains("functions: []\n"), "{}", out);
    // Without a quillo.confirmed the kit's are the program's, and run as they did.
    std::fs::remove_file(d.join("quillo.confirmed")).unwrap();
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    assert!(!out.contains("has the database run SQL from text"), "{}", out);
    assert!(out.contains("functions: [{ proname: \"kit_f\" }, { proname: \"kit_p\" }]\n"), "{}", out);
}

/// Under a quillo.confirmed on MySQL (MariaDB) a taken file's statement that has the database run
/// SQL from text is refused, says what it found and what to do, and nothing of it runs: `prepare
/// ... from`, `execute`, `execute immediate` (in mixed case), `call`, `create procedure`/`function`
/// (with a definer), `create trigger`, `create event`, `alter event`, `begin not atomic`, inside a
/// `/*! */` comment (its version's digits before the word too), and as a second statement after
/// `;`. The same words in a string, a comment (`#`, `-- `, `/* */`) or a quoted name run, as does
/// `create table`. The main file's `execute immediate` runs (src/db.rs text_run, FEEDBACK83.md
/// Q-1300, the user's decision of 2026-11-24; branch feedback82-85). Changed by review 6 of
/// aebe60e's fixes (fail closed): a definer before what `create` makes is refused as itself, `begin
/// not atomic` as a compound statement, `/*! */` as what cannot be read for certain, and `do 1`,
/// which ran, is refused, `do` being refused where a statement begins.
#[test]
fn on_mysql_a_taken_files_sql_run_from_text_is_refused() {
    let Some(my) = support::mariadb("cov8285db5b") else {
        eprintln!("MariaDB is not installed: skipped");
        return;
    };
    let url = my.url("text_run");
    let d = dir("my-text");
    let refused: Vec<(&str, &str, Option<&str>, &str)> = vec![
        ("db.exec", "prepare kit_s from 'select 1'", None, "`prepare`"),
        ("db.exec", "execute kit_s", None, "`execute`"),
        ("db.exec", "ExEcUtE ImMeDiAtE 'select 1'", None, "`execute immediate`"),
        ("db.exec", "call kit_p()", None, "`call`"),
        ("db.exec", "create procedure kit_p() select 1", None, "`create procedure`"),
        ("db.exec", "create definer = current_user function kit_f() returns int return 1", None, "`create definer`"),
        ("db.exec", "create trigger kit_t before insert on kits for each row set new.x = new.x", None, "`create trigger`"),
        ("db.exec", "create event kit_e on schedule every 1 day do select 1", None, "`create event`"),
        ("db.exec", "alter event main_e disable", None, "`alter event`"),
        ("db.exec", "begin not atomic select 1; end", None, "a compound statement (`begin`)"),
        ("db.exec", "/*! call kit_p() */", None, EXECUTABLE_COMMENT),
        ("db.exec", "/*!50000call kit_p() */", None, EXECUTABLE_COMMENT),
        ("db.exec", "select 1; execute immediate 'select 1'", None, "`execute immediate`"),
        ("db.exec", "do 1", None, "`do`"),
    ];
    let ran: Vec<(&str, &str, Option<&str>)> = vec![
        ("db.query", "select 'call p(); execute immediate \\'x\\'; create procedure' as s", None),
        ("db.query", "select 1 as one # call p()\n", None),
        ("db.query", "select 1 as one -- execute immediate 'x'\n", None),
        ("db.query", "select 1 /* create trigger t */ as one", None),
        ("db.query", "select 1 as `call`, 2 as `execute`", None),
        ("db.exec", "create table if not exists kits2 (x int)", None),
    ];
    let mut calls: Vec<(&str, &str, Option<&str>)> = refused.iter().map(|(f, s, v, _)| (*f, *s, *v)).collect();
    calls.extend(ran.iter().copied());
    write(&d, "kit/k.qwe", &kit_with(&calls));
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists kits (x int)\")\nprint(\"main: \" + (is_err(db.exec(\"execute immediate 'insert into kits values (1)'\")) ? \"refused\" : \"ran\"))\nprint(steal())\nprint(\"routines: \" + str(db.query(\"select routine_name from information_schema.routines where routine_schema = database()\")))\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("main: ran\n"), "{}", out);
    let whats: Vec<&str> = refused.iter().map(|(_, _, _, w)| *w).collect();
    assert!(out.contains(&kit_said(&whats, ran.len(), "kit/k.qwe")), "{}", out);
    assert!(out.contains("routines: []\n"), "{}", out);
}

/// Under a quillo.confirmed on PostgreSQL a taken file's migration is judged by the text that runs,
/// as its statements are: one with `create function` is refused, its name said, nothing of it
/// applied; the main file's same migration is applied (src/db.rs migration_judge, src/migrate.rs
/// read_judged, FEEDBACK83.md Q-1300; branch feedback82-85).
#[test]
fn on_postgresql_a_taken_files_migration_run_from_text_is_refused() {
    let Some(pg) = support::postgres("cov8285db5c") else {
        eprintln!("PostgreSQL is not installed: skipped");
        return;
    };
    let url = pg.database("migrate_text");
    let d = dir("pg-migrate");
    let m = "create function made_f() returns int language sql as $$ select 1 $$;\n";
    write(&d, "kit/m/001_a.sql", m);
    write(&d, "m/001_a.sql", m);
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let c = db.connect(env(\"KIT_DB\"))\n  return \"kit: \" + str(c.migrate(\"kit/m\"))\n}\n");
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\nprint(\"main: \" + str(db.migrate(\"m\")))\nprint(steal())\n");
    let host = url.split('@').nth(1).and_then(|h| h.split('/').next()).unwrap().split(':').next().unwrap().to_string();
    write(&d, "quillo.confirmed", &format!("[\"main.qwe\"]\nfiles = [\"m/\"]\n\n[\"kit/k.qwe\"]\nfiles = [\"kit/m/\"]\nhttp = [\"{}\"]\n", host));
    let kit_db = pg.database("migrate_kit");
    let (code, out) = run(&d, &[("QUILLO_DB", &url), ("KIT_DB", &kit_db)]);
    assert_ne!(code, 0, "{}", out);
    assert!(out.contains("main: [\"001_a.sql\"]\n"), "{}", out);
    assert!(out.contains(&format!("db.migrate: kit/m/001_a.sql: {}", text_run("`create function`", "kit/k.qwe").trim_start_matches("db: "))), "{}", out);
}

// ---- conn_ok: a connection judged where its statement runs -------------------------------------

/// Under a quillo.confirmed a map a taken file writes that names the main file's connection by its
/// number (`{ "$db": 0, query: db.query }`) reaches no connection: its `db.query`, `db.exec` and
/// `db.transaction` are the kit's, made for no connection, and run on the program's database, which
/// is every file's (`SELECT` finds no `secrets` there); `db.atomic` of it is judged for the kit as
/// the connection it names, and refused; a number that is no connection runs on the program's
/// database too. A helper confirmed to write that database, and the main file itself, reach it by
/// such a map no more (src/db.rs conn_of, src/interp.rs stand_in_conn, FEEDBACK83.md Q-1300, found
/// by the review of S3's fixes; branch feedback82-85). Changed by review 6 of aebe60e's fixes: a
/// map's `$db` is not taken for a stand-in's call, which runs on the connection it was made for
/// (none here), where this test had each statement on connection 0 judged and refused.
#[test]
fn a_connection_map_a_taken_file_writes_is_judged_as_the_connection_it_names() {
    let d = dir("forged");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\nimport lib/l.qwe use Card, read_secret\nlet s = db.connect(\"data/secret.db\")\ns.exec(\"create table if not exists secrets (v text)\")\ns.exec(\"insert into secrets values ('main secret')\")\nprint(steal())\nprint(\"lib: \" + read_secret())\nlet m = { \"$db\": 0, query: db.query }\nprint(\"main: \" + str(m.query(\"select v from secrets\")))\n",
    );
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let said = (r) => is_err(r) ? r.error : str(r)\n  let m = { \"$db\": 0, query: db.query, exec: db.exec, transaction: db.transaction }\n  let none = { \"$db\": 99, query: db.query }\n  return \"kit:\\n\" + join([said(m.query(\"select v from secrets\")), said(m.exec(\"delete from secrets\")), said(m.transaction(() => 1)), said(db.atomic([m], () => 1)), said(none.query(\"select 1 as one\"))], \"\\n\")\n}\n",
    );
    write(&d, "lib/l.qwe", "component Card() {\n  return <i>c</i>\n}\nfn read_secret() {\n  let m = { \"$db\": 0, query: db.query }\n  return str(m.query(\"select v from secrets\"))\n}\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"lib/l.qwe\"]\nwrites = [\"data/secret.db\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let none = "db: no such table: secrets";
    let want = ["kit:", none, none, "1", "db: data/secret.db is not confirmed for kit/k.qwe (quillo confirm)", "[{ one: 1 }]"].join("\n");
    assert!(out.contains(&format!("{}\n", want)), "{}", out);
    assert!(out.contains("lib: err(\"db: no such table: secrets\")\nmain: err(\"db: no such table: secrets\")\n"), "{}", out);
    assert_eq!(column(&d.join("data/secret.db"), "select v from secrets"), ["main secret"]);
}

/// Under a quillo.confirmed a kit confirmed for nothing that writes a connection's map itself, by
/// `set`, as a literal or by `merge`, with `db.one` and `db.exec` and a connection's number, gets
/// no connection by it: with the number of a helper file's connection (`libc`, which only the
/// helper was confirmed to write), of the main file's own, or of one never made, its statements run
/// on the program's database, every file's, and none on the connection named; `db.connect` of a
/// target already open (the helper's, the main file's) is refused as a new one would be; nothing is
/// written to those, and the helper's and the main file's own calls of them run. `quillo check`
/// names the kit's `db.connect` calls (src/db.rs conn_of and conn_ok, FEEDBACK83.md Q-1300, found
/// by the review of S3's fixes; branch feedback82-85). Changed by review 6 of aebe60e's fixes: a
/// map's `$db` is not taken for a stand-in's call (the kit's `db.one` is made for no connection),
/// where this test had each refused by `conn_ok` as the connection it named.
#[test]
fn a_connection_map_a_taken_file_writes_by_set_literal_or_merge_reaches_no_connection() {
    let d = dir("forge");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "lib/config.qwe", "let libc = db.connect(\"data/lib.db\")\nfn lib_init() {\n  libc.exec(\"create table if not exists s (v text)\")\n  libc.exec(\"insert into s values ('lib secret')\")\n  return true\n}\n");
    write(
        &d,
        "main.qwe",
        "use lib/config.qwe\nimport kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\nlib_init()\nlet mc = db.connect(\"data/main.db\")\nmc.exec(\"create table if not exists m (v text)\")\nmc.exec(\"insert into m values ('main secret')\")\nprint(steal())\nprint(\"after: \" + str(libc.query(\"select v from s\")) + \" \" + str(mc.query(\"select v from m\")))\n",
    );
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let said = (r) => is_err(r) ? r.error : str(r)\n  let out = []\n  for n in [0, 1, 7] {\n    let by_set = set(set(set({}, \"$db\", n), \"one\", db.one), \"exec\", db.exec)\n    let by_literal = { \"$db\": n, one: db.one, exec: db.exec }\n    let by_merge = merge({ one: db.one, exec: db.exec }, { \"$db\": n })\n    for m in [by_set, by_literal, by_merge] {\n      out = push(out, str(n) + \": \" + said(m.one(\"select 1 as x\")) + \" | \" + said(m.exec(\"create table if not exists kit_wrote (x int)\")))\n    }\n  }\n  out = push(out, \"connect: \" + said(db.connect(\"data/lib.db\")))\n  out = push(out, \"connect main's: \" + said(db.connect(\"data/main.db\")))\n  return \"kit:\\n\" + join(out, \"\\n\")\n}\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/app.db\", \"data/main.db\"]\n\n[\"lib/config.qwe\"]\nwrites = [\"data/lib.db\"]\n");
    let (code, out) = quillo_env(&d, &["check", "main.qwe"], 60, &[]);
    assert_ne!(code, Some(0), "{}", out);
    assert!(out.contains("writes data/lib.db: not confirmed for this file") && out.contains("writes data/main.db: not confirmed for this file"), "{}", out);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let lib = "db: data/lib.db is not confirmed for kit/k.qwe (quillo confirm)";
    let main = "db: data/main.db is not confirmed for kit/k.qwe (quillo confirm)";
    let none = "db: this connection is not one of this program's";
    let _ = none;
    let mut want = vec!["kit:".to_string()];
    for n in [0, 1, 7] {
        want.extend(std::iter::repeat_n(format!("{}: {{ x: 1 }} | {{ changes: 0, last_id: null }}", n), 3));
    }
    want.push(format!("connect: {}", lib));
    want.push(format!("connect main's: {}", main));
    assert!(out.contains(&format!("{}\n", want.join("\n"))), "{}", out);
    assert!(out.contains("after: [{ v: \"lib secret\" }] [{ v: \"main secret\" }]\n"), "{}", out);
    for db in ["data/lib.db", "data/main.db"] {
        assert!(column(&d.join(db), "select name from sqlite_master where name = 'kit_wrote'").is_empty(), "{}", db);
    }
    assert_eq!(column(&d.join("data/app.db"), "select name from sqlite_master where name = 'kit_wrote'"), ["kit_wrote"]);
}

/// Under a quillo.confirmed a tenant's migrations are judged as the code that set them, not as the
/// code that first opens the tenant: the main file's migration making a table of Quillo's name
/// (`_quillo_audit`) is applied when a kit's `with_tenant` opens the tenant first, where it was
/// refused as the kit's SQL and the kit's insert failed with it (src/db.rs tenant_backend,
/// FEEDBACK83.md Q-1300, found making part B; branch feedback82-85).
#[test]
fn a_tenants_migrations_are_judged_as_the_code_that_set_them() {
    let d = dir("tenant-mig");
    std::fs::create_dir_all(d.join("data/t")).unwrap();
    write(&d, "m/001_a.sql", "create table _quillo_audit (x text);\ncreate table notes (x text);\n");
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let r = with_tenant(\"acme\", () => db.exec(\"insert into notes values (?)\", \"kit note\"))\n  return \"kit: \" + (is_err(r) ? r.error : str(r))\n}\n");
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\ndb.open(\"data/t/{tenant}.db\")\ndb.migrate(\"m\")\nprint(steal())\nprint(\"main: \" + str(with_tenant(\"acme\", () => db.query(\"select x from notes\"))))\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\nfiles = [\"m/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/t/\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("quillo: applied migration 001_a.sql for tenant acme\nkit: { changes: 1, last_id: 1 }\nmain: [{ x: \"kit note\" }]\n"), "{}", out);
    assert_eq!(column(&d.join("data/t/acme.db"), "select name from sqlite_master where name = '_quillo_audit'"), ["_quillo_audit"]);
}

// ---- round 3: what the reader cannot read for certain is refused (review 6 of aebe60e) ----------

/// Under a quillo.confirmed on PostgreSQL a taken file's statement is read fail closed:
/// - `--` ends at `\r` as PostgreSQL ends it (`--\rDO ...` and `--\r\nDO ...` are a `do` block);
///   `\f` and `\v` do not end it, as PostgreSQL's do not.
/// - `execute`, `deallocate` anywhere (`create table ... as execute`, after `explain analyze`,
///   after `;`).
/// - A `create` or `alter` of what a taken file may not make: an extension, an operator (whose
///   `function = ts_stat` names a function with no `(`), an aggregate, a cast, a language, a
///   schema, a server, a foreign table, a policy, a type, a domain, `or replace function`; `alter
///   function`, `alter system`, `alter role`.
/// - tablefunc's `crosstab` and `connectby`, `ts_rewrite`.
/// - What reaches the server's files (`pg_read_file`, `lo_import`, `copy` from a file, `load`,
///   `import foreign schema`).
/// - The catalogs of other tables' data and other statements' SQL (`pg_stats`,
///   `pg_catalog.pg_statistic`, `pg_stat_activity`, `pg_prepared_statements`, a quoted
///   `"pg_stats_ext"`).
/// Each says what it found. What a taken file may make runs: a table (unlogged, `if not exists`,
/// `as select`; a temp one is a session's, which Quillo refuses on a shared connection), a unique index, an index `concurrently`, `or replace view`, a
/// materialized view, a sequence, and `alter` of each. So do `on conflict do nothing` and `do
/// update`, `case`, a column named `"execute"`, and the words in a string, a dollar quote and a
/// comment. The main file's `do` runs, and its `select 1 --\r; begin`, which began a transaction
/// left open on a shared connection, is refused as one (src/db.rs text_run, space_end; src/sql.rs
/// line_end; FEEDBACK83.md Q-1300, found by review 6 of aebe60e; branch feedback82-85).
#[test]
fn on_postgresql_a_taken_files_statement_is_read_fail_closed() {
    let Some(pg) = support::postgres("cov8285db5r3") else {
        eprintln!("PostgreSQL is not installed: skipped");
        return;
    };
    let url = pg.database("fail_closed");
    let d = dir("pg-closed");
    let refused: Vec<(&str, &str, Option<&str>, &str)> = vec![
        ("db.exec", "--\rDO $q$ begin perform 1; end $q$ /*\n*/", None, "`do`"),
        ("db.exec", "-- c\r\nDO $q$ begin perform 1; end $q$", None, "`do`"),
        ("db.exec", "create temp table kit_t as execute main_s('x')", None, "`execute`"),
        ("db.query", "explain analyze create table kit_t2 as execute main_s('x')", None, "`execute`"),
        ("db.exec", "select 1 as one; execute main_s('x')", None, "`execute`"),
        ("db.exec", "deallocate all", None, "`deallocate`"),
        ("db.exec", "create extension if not exists tablefunc", None, "`create extension`"),
        ("db.exec", "create operator ~~~ (rightarg = text, function = ts_stat)", None, "`create operator`"),
        ("db.exec", "create aggregate kit_a (sfunc = int4pl, stype = int)", None, "`create aggregate`"),
        ("db.exec", "create cast (text as int) with inout", None, "`create cast`"),
        ("db.exec", "create language plpgsql", None, "`create language`"),
        ("db.exec", "create schema kit_s", None, "`create schema`"),
        ("db.exec", "create server kit_srv foreign data wrapper x", None, "`create server`"),
        ("db.exec", "create foreign table kit_f (x int) server kit_srv", None, "`create foreign`"),
        ("db.exec", "create policy kit_p on kits using (true)", None, "`create policy`"),
        ("db.exec", "create type kit_e as enum ('a')", None, "`create type`"),
        ("db.exec", "create domain kit_d as int", None, "`create domain`"),
        ("db.exec", "create or replace function kit_f() returns int language sql as $$ select 1 $$", None, "`create function`"),
        ("db.exec", "alter function main_f() rename to kit_f", None, "`alter function`"),
        ("db.exec", "ALTER SYSTEM set work_mem = '1MB'", None, "`alter system`"),
        ("db.exec", "alter role quillo superuser", None, "`alter role`"),
        ("db.query", "select * from crosstab('select 1') as t(a int)", None, "the function crosstab"),
        ("db.query", "select * from connectby('kits', 'x', 'x', '1', 0) as t(a text, b text, c int)", None, "the function connectby"),
        ("db.query", "select ts_rewrite('a'::tsquery, 'select 1')", None, "the function ts_rewrite"),
        ("db.query", "select pg_read_file('/etc/hosts')", None, "reaches:the function pg_read_file"),
        ("db.query", "select lo_import('/etc/hosts')", None, "reaches:the function lo_import"),
        ("db.exec", "copy kits from '/etc/hosts'", None, "reaches:`copy`"),
        ("db.exec", "load 'auto_explain'", None, "reaches:`load`"),
        ("db.exec", "import foreign schema public from server kit_srv into public", None, "reaches:`import`"),
        ("db.query", "select * from pg_stats", None, "reads:the catalog pg_stats"),
        ("db.query", "select * from pg_catalog.pg_statistic", None, "reads:the catalog pg_statistic"),
        ("db.query", "select query from pg_stat_activity", None, "reads:the catalog pg_stat_activity"),
        ("db.query", "select statement from pg_prepared_statements", None, "reads:the catalog pg_prepared_statements"),
        ("db.query", "select * from \"pg_stats_ext\"", None, "reads:the catalog pg_stats_ext"),
    ];
    let ran: Vec<(&str, &str, Option<&str>)> = vec![
        ("db.exec", "insert into kits values (1) on conflict do nothing", None),
        ("db.exec", "insert into kits values (1) on conflict (x) do update set x = excluded.x", None),
        ("db.query", "select 1 as \"execute\", 2 as \"call\", 3 as \"do\"", None),
        ("db.query", "select 'execute call do create function pg_stats crosstab(' as s", None),
        ("db.query", "select $q$ execute deallocate do $q$ as s", None),
        ("db.query", "select 1 /* execute call */ as one -- do pg_stats\n", None),
        ("db.query", "select 1 as one --\x0c do $q$ $q$\n", None),
        ("db.query", "select 1 as one --\x0b execute\n", None),
        ("db.query", "select case when 1 = 1 then 'a' else 'b' end as c", None),
        ("db.exec", "create table if not exists kit_t3 (x int)", None),
        ("db.exec", "create table if not exists kit_t4 as select 1 as x", None),
        ("db.exec", "create unlogged table if not exists kit_t5 (x int)", None),
        ("db.exec", "create unique index if not exists kit_i on kit_t3 (x)", None),
        ("db.exec", "create index concurrently if not exists kit_i2 on kit_t3 (x)", None),
        ("db.exec", "create or replace view kit_v as select 1 as x", None),
        ("db.exec", "create materialized view if not exists kit_mv as select 1 as x", None),
        ("db.exec", "create sequence if not exists kit_seq", None),
        ("db.exec", "alter table kit_t3 add column y int", None),
        ("db.exec", "alter index kit_i rename to kit_i3", None),
        ("db.exec", "alter sequence kit_seq restart", None),
        ("db.exec", "alter view kit_v rename to kit_v2", None),
    ];
    let mut calls: Vec<(&str, &str, Option<&str>)> = refused.iter().map(|(f, s, v, _)| (*f, *s, *v)).collect();
    calls.extend(ran.iter().copied());
    write(&d, "kit/k.qwe", &kit_with(&calls));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists kits (x int primary key)\")\ndb.exec(\"create or replace function main_f() returns trigger language plpgsql as $$ begin return new; end $$\")\nprint(\"main: \" + str(db.exec(\"do $$ begin perform 1; end $$\")))\nprint(steal())\nprint(\"made: \" + str(db.query(\"select proname as n from pg_proc where proname like 'kit%' union all select nspname from pg_namespace where nspname like 'kit%' order by 1\")))\ndb.exec(\"select 1 --\\r; begin\")\nprint(\"not here\")\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_ne!(code, 0, "{}", out);
    assert!(out.contains("main: { changes: 0, last_id: null }\n"), "{}", out);
    let whats: Vec<&str> = refused.iter().map(|(_, _, _, w)| *w).collect();
    assert!(out.contains(&kit_said(&whats, ran.len(), "kit/k.qwe")), "{}", out);
    assert!(out.contains("made: []\n"), "{}", out);
    assert!(out.contains("this SQL begins or ends a transaction") && !out.contains("not here"), "{}", out);
}

/// Under a quillo.confirmed on MySQL (MariaDB) a taken file's statement is read fail closed:
/// - A `/*! */` or `/*M! */` comment, with a version above the server's or none, its `'` hiding
///   what follows from a reading that takes it as SQL, is refused as what cannot be read for
///   certain.
/// - `execute` and `call` after `set statement ... for`, and `deallocate`.
/// - The compound statements (`if`, `while`, `repeat`, `case`, `declare`, a label before `loop`).
/// - A definer, or `sql security`, before what `create` makes; `or replace function`;
///   `create database` and `create user`; `alter user` and `alter database`.
/// - `into outfile`, `load_file`, `load data`.
/// - performance_schema, the `sys` schema, `mysql.general_log` and the processlist (`mysql.` refused
///   as the `mysql` schema since review 7 of 64df6c0's fixes, before its `general_log`).
/// What a taken file may make runs: a table (`if not exists`, `or replace`), a unique index, a
/// view with `algorithm = merge`, and `alter table`. So do `if()` and `case` in a select, `on
/// duplicate key update`, a column named `` `execute` ``, and `/*!` in a string or `/* ! */`. The
/// main file's `/*!50000 ... */` runs (src/db.rs text_run, tokens; FEEDBACK83.md Q-1300, found by
/// review 6 of aebe60e; branch feedback82-85).
#[test]
fn on_mysql_a_taken_files_statement_is_read_fail_closed() {
    let Some(my) = support::mariadb("cov8285db5r3") else {
        eprintln!("MariaDB is not installed: skipped");
        return;
    };
    let url = my.url("fail_closed");
    let d = dir("my-closed");
    let refused: Vec<(&str, &str, Option<&str>, &str)> = vec![
        ("db.exec", "/*!99999 ' */ call kit_p() -- '", None, EXECUTABLE_COMMENT),
        ("db.exec", "/*M!100000 select 1 */", None, EXECUTABLE_COMMENT),
        ("db.query", "select 1 as one /*!*/", None, EXECUTABLE_COMMENT),
        ("db.exec", "set statement max_statement_time=60 for execute immediate 'select 1'", None, "`execute immediate`"),
        ("db.exec", "SET STATEMENT max_statement_time=60 FOR CALL kit_p()", None, "`call`"),
        ("db.exec", "deallocate prepare kit_s", None, "`deallocate`"),
        ("db.exec", "if 1 then select 1; end if", None, "a compound statement (`if`)"),
        ("db.exec", "while 0 do select 1; end while", None, "a compound statement (`while`)"),
        ("db.exec", "repeat select 1; until 1 end repeat", None, "a compound statement (`repeat`)"),
        ("db.exec", "case when 1 then select 1; end case", None, "a compound statement (`case`)"),
        ("db.exec", "declare x int", None, "a compound statement (`declare`)"),
        ("db.exec", "kit_l: loop leave kit_l; end loop", None, "a compound statement (a label)"),
        ("db.exec", "create definer = current_user procedure kit_p() select 1", None, "`create definer`"),
        ("db.exec", "create sql security definer view kit_v as select 1 as x", None, "`create sql`"),
        ("db.exec", "create or replace function kit_f() returns int return 1", None, "`create function`"),
        ("db.exec", "create database kit_db", None, "`create database`"),
        ("db.exec", "create user kit", None, "`create user`"),
        ("db.exec", "alter user root identified by 'x'", None, "`alter user`"),
        ("db.exec", "alter database fail_closed character set utf8mb4", None, "`alter database`"),
        ("db.query", "select 1 into outfile '/tmp/kit.txt'", None, "reaches:`into outfile`"),
        ("db.query", "select load_file('/etc/hosts') as f", None, "reaches:the function load_file"),
        ("db.exec", "load data infile '/etc/hosts' into table kits", None, "reaches:`load`"),
        ("db.query", "select * from performance_schema.events_statements_history", None, "reads:the catalog performance_schema"),
        ("db.query", "select * from sys.statement_analysis", None, "reads:the `sys` schema"),
        ("db.query", "select * from mysql.general_log", None, "reads:the `mysql` schema"),
        ("db.query", "select * from information_schema.processlist", None, "reads:the catalog processlist"),
    ];
    let ran: Vec<(&str, &str, Option<&str>)> = vec![
        ("db.query", "select '/*! x */' as s", None),
        ("db.query", "select 1 /* ! not */ as one", None),
        ("db.query", "select 1 as `execute`, 2 as `call`", None),
        ("db.query", "select if(1, 2, 3) as v, case when 1 then 'a' end as c", None),
        ("db.exec", "create table if not exists kit_t (x int primary key)", None),
        ("db.exec", "create or replace table kit_t2 (x int)", None),
        ("db.exec", "create unique index kit_i on kit_t2 (x)", None),
        ("db.exec", "create algorithm = merge view kit_v2 as select 1 as x", None),
        ("db.exec", "alter table kit_t2 add column y int", None),
        ("db.exec", "insert into kit_t (x) values (1) on duplicate key update x = 2", None),
    ];
    let mut calls: Vec<(&str, &str, Option<&str>)> = refused.iter().map(|(f, s, v, _)| (*f, *s, *v)).collect();
    calls.extend(ran.iter().copied());
    write(&d, "kit/k.qwe", &kit_with(&calls));
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists kits (x int)\")\nprint(\"main: \" + str(db.query(\"select 1 /*!50000 + 1 */ as two\")))\nprint(steal())\nprint(\"routines: \" + str(db.query(\"select routine_name from information_schema.routines where routine_schema = database()\")))\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("main: [{ two: 2 }]\n"), "{}", out);
    let whats: Vec<&str> = refused.iter().map(|(_, _, _, w)| *w).collect();
    assert!(out.contains(&kit_said(&whats, ran.len(), "kit/k.qwe")), "{}", out);
    assert!(out.contains("routines: []\n"), "{}", out);
}

/// Under a quillo.confirmed a taken file's migration on MySQL (MariaDB) with a compound statement
/// (`if 1 then execute immediate ...; end if;`), which ran in a kit's own `c.migrate`, is refused
/// by the text that runs, its name said; nothing of it is applied (src/db.rs text_run,
/// migration_judge; FEEDBACK83.md Q-1300, found by review 6 of aebe60e; branch feedback82-85).
#[test]
fn on_mysql_a_taken_files_migration_with_a_compound_statement_is_refused() {
    let Some(my) = support::mariadb("cov8285db5r3m") else {
        eprintln!("MariaDB is not installed: skipped");
        return;
    };
    let url = my.url("migrate_compound");
    let kit_db = my.url("migrate_compound_kit");
    let d = dir("my-migrate");
    write(&d, "kit/m/001_a.sql", "create table if not exists kit_notes (x int);\nif 1 then execute immediate concat('insert into kit_', 'notes values (1)'); end if;\n");
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let c = db.connect(env(\"KIT_DB\"))\n  return \"kit: \" + str(c.migrate(\"kit/m\"))\n}\n");
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\nprint(steal())\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n\n[\"kit/k.qwe\"]\nfiles = [\"kit/m/\"]\nhttp = [\"127.0.0.1\"]\n");
    let (code, out) = run(&d, &[("QUILLO_DB", &url), ("KIT_DB", &kit_db)]);
    assert_ne!(code, 0, "{}", out);
    assert!(out.contains(&format!("db.migrate: kit/m/001_a.sql: {}", text_run("a compound statement (`if`)", "kit/k.qwe").trim_start_matches("db: "))), "{}", out);
    assert!(!out.contains("kit: ["), "{}", out);
}

/// Under a quillo.confirmed a stand-in runs on the connection it was made for, whatever map it is
/// put in: the main file's `db.query` handed to a kit, put with `"$db": 0` (the main file's
/// `db.connect("data/secret.db")`), runs on the program's database, not on that connection, and so
/// does the kit's own `db.query` there; a connection's own function put in a map with another
/// number runs on its own connection (the kit's `data/k.db`), not the number's. The main file's
/// connection reads as ever. Without a quillo.confirmed nothing is held, and the map's number is
/// the connection, as before (src/db.rs conn_of, src/interp.rs stand_in_conn and held_to;
/// FEEDBACK83.md Q-1300, found by review 6 of aebe60e; branch feedback82-85).
#[test]
fn a_stand_in_runs_on_the_connection_it_was_made_for() {
    let d = dir("bound");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\nlet secret = db.connect(\"data/secret.db\")\nsecret.exec(\"create table if not exists s (x text)\")\nsecret.exec(\"delete from s\")\nsecret.exec(\"insert into s values ('top-secret')\")\ndb.exec(\"create table if not exists notes (x text)\")\nprint(steal(db.query))\nprint(\"main: \" + str(secret.query(\"select x from s\")))\n");
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn steal(q) {\n  let said = (r) => is_err(r) ? r.error : str(r)\n  let mine = db.connect(\"data/k.db\")\n  mine.exec(\"create table if not exists k (x text)\")\n  mine.exec(\"delete from k\")\n  mine.exec(\"insert into k values ('kit row')\")\n  return \"kit:\\n\" + join([said(q(\"select count(*) as n from notes\")), said({ \"$db\": 0, query: q }.query(\"select x from s\")), said({ \"$db\": 0, query: db.query }.query(\"select x from s\")), said({ \"$db\": 0, query: mine.query }.query(\"select x from k\")), said(set({}, \"$db\", 0).x)], \"\\n\")\n}\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/k.db\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let want = ["kit:", "[{ n: 0 }]", "db: no such table: s", "db: no such table: s", "[{ x: \"kit row\" }]"].join("\n");
    assert!(out.contains(&format!("{}\n", want)), "{}", out);
    assert!(out.contains("main: [{ x: \"top-secret\" }]\n"), "{}", out);
    assert!(!out.contains("kit:\n[{ n: 0 }]\n[{ x: \"top-secret\" }]"), "{}", out);
    // Without a quillo.confirmed: the map's number is the connection.
    std::fs::remove_file(d.join("quillo.confirmed")).unwrap();
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("kit:\n[{ n: 0 }]\n[{ x: \"top-secret\" }]\n[{ x: \"top-secret\" }]\n"), "{}", out);
}

/// Under a quillo.confirmed a connection's number that `db.system` moves is judged again: a kit
/// confirmed to write `data/a.db` reads it by its own `db.connect`, the main file's `db.system`
/// again takes the system database's place away, and the kit's same map, whose number is now the
/// main file's `data/b.db`'s, is refused there, where the verdict kept for the number let it read
/// `b secret` (src/db.rs closed_at, CONN_GEN; FEEDBACK83.md Q-1300, found by review 6 of aebe60e;
/// branch feedback82-85).
#[test]
fn a_connections_number_moved_by_db_system_is_judged_again() {
    let d = dir("moved");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, first, again\ndb.system(\"data/sys1.db\")\nlet a = db.connect(\"data/a.db\")\nlet b = db.connect(\"data/b.db\")\na.exec(\"create table if not exists t (v text)\")\na.exec(\"delete from t\")\na.exec(\"insert into t values ('a row')\")\nb.exec(\"create table if not exists t (v text)\")\nb.exec(\"delete from t\")\nb.exec(\"insert into t values ('b secret')\")\nlet k = first()\ndb.system(\"data/sys2.db\")\nprint(again(k))\n",
    );
    write(
        &d,
        "kit/k.qwe",
        "component Badge() {\n  return <b>b</b>\n}\nfn first() {\n  let k = db.connect(\"data/a.db\")\n  print(\"first: \" + str(k.query(\"select v from t\")))\n  return k\n}\nfn again(k) {\n  let r = k.query(\"select v from t\")\n  return \"again: \" + (is_err(r) ? r.error : str(r))\n}\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/a.db\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("first: [{ v: \"a row\" }]\n"), "{}", out);
    assert!(out.contains("again: db: data/b.db is not confirmed for kit/k.qwe (quillo confirm)\n"), "{}", out);
    assert!(!out.contains("b secret"), "{}", out);
}

/// Under a quillo.confirmed a taken file's migration folder reached by a link inside the program
/// is read where it really is, as the path the system opened (`opened_path`) says, and applied;
/// one of its files that is a link to a file outside what the kit may read is refused, and nothing
/// applied (src/migrate.rs read_judged; FEEDBACK83.md Q-1300, found by review 6 of aebe60e; branch
/// feedback82-85). A folder swapped for a link as a file is read, which the check catches, is a
/// race no test makes.
#[test]
fn a_taken_files_migration_folder_by_a_link_is_read_where_it_is() {
    let d = dir("mig-link");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "kit/real/001_a.sql", "create table t (x text);\ninsert into t values ('caf\u{e9}');\n");
    std::os::unix::fs::symlink(d.join("kit/real"), d.join("kit/m")).unwrap();
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let c = db.connect(\"data/k.db\")\n  return \"applied: \" + str(c.migrate(\"kit/m\"))\n}\n");
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\nprint(steal())\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"kit/k.qwe\"]\nwrites = [\"data/k.db\"]\nfiles = [\"kit/\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("applied: [\"001_a.sql\"]\n"), "{}", out);
    assert_eq!(column(&d.join("data/k.db"), "select x from t"), ["caf\u{e9}"]);
    // A file of it that is a link to one outside what the kit may read.
    write(&d, "outside/002_b.sql", "create table u (x text);\n");
    std::os::unix::fs::symlink(d.join("outside/002_b.sql"), d.join("kit/real/002_b.sql")).unwrap();
    let (code, out) = run(&d, &[]);
    assert_ne!(code, 0, "{}", out);
    assert!(out.contains("db.migrate: ") && out.contains("not confirmed for kit/k.qwe"), "{}", out);
    assert!(column(&d.join("data/k.db"), "select name from sqlite_master where name = 'u'").is_empty());
}

// ---- round 4: a statement's first word, session state, engines, `pg_` (review 7 of 64df6c0) ----

/// What the hold says when a taken file's statement begins with a word it may not begin with.
fn begins(word: &str, file: &str) -> String {
    format!("db: this statement begins with `{}`, which a taken file's statement may not begin with where a quillo.confirmed holds the run: it may begin with select, insert, update, delete, with, values, table, replace, explain, create, alter, drop or truncate ({}'s SQL). Run it from the main file's SQL, or from a migration the main file runs", word, file)
}

/// What the hold says when a taken file's SQL names one of PostgreSQL's `pg_...`.
fn pg_name(name: &str, file: &str) -> String {
    format!("db: {} is one of PostgreSQL's own catalogs and functions (`pg_...`), which hold or reach other tables and statements, and is refused for {}'s SQL where a quillo.confirmed holds the run. Run it from the main file's SQL, or from a migration the main file runs", name, file)
}

/// What the hold says of a character outside ASCII just before `\` or a backtick.
const WIDE: &str = "unsure:a character outside ASCII just before a `\\` or a backtick, which a connection's GBK, SJIS or Big5 reads as one character with it";

/// The refusals `steal()` gives for `refused`, each `begins:`, `pg:`, or what `refusal` takes.
fn said_r4(refused: &[&str], ran: usize, file: &str) -> String {
    let mut want = vec!["kit:".to_string()];
    want.extend(refused.iter().map(|w| match (w.strip_prefix("begins:"), w.strip_prefix("pg:")) {
        (Some(x), _) => begins(x, file),
        (_, Some(x)) => pg_name(x, file),
        _ => refusal(w, file),
    }));
    want.extend(std::iter::repeat_n("ran".to_string(), ran));
    format!("{}\n", want.join("\n"))
}

/// Under a quillo.confirmed on PostgreSQL a taken file's statement may begin only with select,
/// insert, update, delete, with, values, table, explain of one, create and alter of what it may
/// make, drop of a table, an index, a view or a sequence, and truncate:
/// - `grant`, `revoke`, `set` (`set local client_encoding = 'SJIS'` in a transaction, the way a
///   kit made the server read its next string otherwise), `reset`, `show`, `comment`, `lock`,
///   `vacuum`, `analyze`, `cluster`, `reindex`, `refresh`, `listen`, `notify`, `discard`,
///   `checkpoint`, `drop schema`, `drop function`, and a statement in brackets are refused.
/// - Any `pg_...` name (`pg_class`, `pg_prepared_statement()`, `pg_stat_get_activity(null)`,
///   `pg_sequences`, `pg_relation_size(...)` by an implicit cast, a quoted `"pg_tables"`) is
///   refused, `pg_catalog.` judged by the name after it.
/// - `set_config` is refused.
/// - A sequence's or a privilege's function given a name worked out as it runs is refused.
/// - A character outside ASCII just before `\` or a backtick is refused.
/// Run: `on conflict do update`, `with ... insert`, `explain select`, `table`, `values`, `drop
/// table`, `truncate`, `nextval('kit_seq')`, `has_table_privilege('kits', 'select')`,
/// `pg_catalog.now()`, and a non-ASCII string with no `\` or backtick after it (src/db.rs
/// head_refused, after_wide, regclass_fn; FEEDBACK83.md Q-1300, found by review 7 of 64df6c0;
/// branch feedback82-85).
#[test]
fn on_postgresql_a_taken_files_statement_may_begin_only_with_what_it_may_do() {
    let Some(pg) = support::postgres("cov8285db5r4") else {
        eprintln!("PostgreSQL is not installed: skipped");
        return;
    };
    let url = pg.database("first_word");
    let d = dir("pg-first");
    let refused: Vec<(&str, &str, Option<&str>, &str)> = vec![
        ("db.exec", "grant select on all tables in schema public to public", None, "begins:grant"),
        ("db.exec", "revoke select on kits from public", None, "begins:revoke"),
        ("db.exec", "set local client_encoding = 'SJIS'", None, "begins:set"),
        ("db.exec", "SET standard_conforming_strings = off", None, "begins:set"),
        ("db.exec", "reset all", None, "begins:reset"),
        ("db.query", "show work_mem", None, "begins:show"),
        ("db.exec", "comment on table kits is 'x'", None, "begins:comment"),
        ("db.exec", "lock table kits", None, "begins:lock"),
        ("db.exec", "vacuum kits", None, "begins:vacuum"),
        ("db.exec", "analyze kits", None, "begins:analyze"),
        ("db.exec", "cluster kits", None, "begins:cluster"),
        ("db.exec", "reindex table kits", None, "begins:reindex"),
        ("db.exec", "refresh materialized view kit_mv", None, "begins:refresh"),
        ("db.exec", "listen kit", None, "begins:listen"),
        ("db.exec", "notify kit", None, "begins:notify"),
        ("db.exec", "discard all", None, "begins:discard"),
        ("db.exec", "checkpoint", None, "begins:checkpoint"),
        ("db.exec", "drop schema public cascade", None, "begins:drop schema"),
        ("db.exec", "drop function main_f()", None, "begins:drop function"),
        ("db.query", "(select 1 as one)", None, "begins:"),
        ("db.query", "select relname from pg_class", None, "pg:pg_class"),
        ("db.query", "select name, statement from pg_prepared_statement()", None, "pg:pg_prepared_statement"),
        ("db.query", "select count(*) as n from pg_stat_get_activity(null)", None, "pg:pg_stat_get_activity"),
        ("db.query", "select sequencename from pg_sequences", None, "pg:pg_sequences"),
        ("db.query", "select pg_relation_size('_quil' || 'lo_mine') as s", None, "pg:pg_relation_size"),
        ("db.query", "select * from pg_catalog.\"pg_tables\"", None, "pg:pg_tables"),
        ("db.query", "select set_config('standard_conforming_strings', 'off', false)", None, "the function set_config"),
        ("db.query", "select has_table_privilege('_quil' || 'lo_mine', 'select') as p", None, "the function has_table_privilege given a table's name worked out as it runs"),
        ("db.query", "select nextval($1) as n", Some("kit_seq"), "the function nextval given a table's name worked out as it runs"),
        ("db.query", "select E'\u{4e01}\\\\' as a", None, WIDE),
        ("db.query", "select '\u{e9}`' as a", None, WIDE),
    ];
    let ran: Vec<(&str, &str, Option<&str>)> = vec![
        ("db.exec", "insert into kits values (1) on conflict (x) do update set x = excluded.x", None),
        ("db.exec", "with t as (select 2 as x) insert into kits select x from t on conflict do nothing", None),
        ("db.query", "explain select * from kits", None),
        ("db.query", "table kits", None),
        ("db.query", "values (1), (2)", None),
        ("db.exec", "create table if not exists kit_t (x int)", None),
        ("db.exec", "truncate kit_t", None),
        ("db.exec", "drop table kit_t", None),
        ("db.query", "select nextval('kit_seq') as n", None),
        ("db.query", "select has_table_privilege('kits', 'select') as p", None),
        ("db.query", "select pg_catalog.now() is not null as t", None),
        ("db.query", "select '\u{4e01}\u{e9} caf\u{e9}' || 'x' as s, 1 as pg", None),
        ("db.exec", "update kits set x = x where x = 1", None),
        ("db.exec", "delete from kits where x = 3", None),
    ];
    let mut calls: Vec<(&str, &str, Option<&str>)> = refused.iter().map(|(f, s, v, _)| (*f, *s, *v)).collect();
    calls.extend(ran.iter().copied());
    write(&d, "kit/k.qwe", &kit_with(&calls));
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists kits (x int primary key)\")\ndb.exec(\"create sequence if not exists kit_seq\")\ndb.exec(\"create materialized view if not exists kit_mv as select 1 as x\")\ndb.exec(\"create or replace function main_f() returns trigger language plpgsql as $$ begin return new; end $$\")\nprint(\"main: \" + str(db.query(\"select relname from pg_class where relname = 'kits'\")))\nprint(steal())\nprint(\"schema: \" + str(db.query(\"select count(*) as n from pg_namespace where nspname = 'public'\")))\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("main: [{ relname: \"kits\" }]\n"), "{}", out);
    let whats: Vec<&str> = refused.iter().map(|(_, _, _, w)| *w).collect();
    assert!(out.contains(&said_r4(&whats, ran.len(), "kit/k.qwe").replace(&begins("", "kit/k.qwe"), &"db: this statement's first word cannot be placed, which a taken file's statement may not begin with where a quillo.confirmed holds the run: it may begin with select, insert, update, delete, with, values, table, replace, explain, create, alter, drop or truncate (kit/k.qwe's SQL). Run it from the main file's SQL, or from a migration the main file runs".to_string())), "{}", out);
    assert!(out.contains("schema: [{ n: 1 }]\n"), "{}", out);
}

/// Under a quillo.confirmed on MySQL (MariaDB) a taken file's statement may begin only with select,
/// insert, update, delete, with, values, replace, explain of one, create, alter, drop and
/// truncate:
/// - `set names gbk`, `set @x`, `charset`, `show` (`show engine innodb status`), `install soname`,
///   `uninstall`, `handler`, `lock tables`, `rename`, `flush`, `kill`, `purge`, `xa`, `optimize`,
///   `repair`, `checksum`, `use`, `help`, `describe`, and `explain` of a table are refused.
/// - A table may take only InnoDB, Aria, MyISAM or MEMORY: CONNECT (which read `/etc/hosts`),
///   FEDERATED and MERGE are refused, in `create` and `alter`, and so are `file_name`, `table_type`,
///   `connection`, `union = (...)` and `data directory`.
/// - information_schema's `innodb_*` and the `mysql` schema are refused.
/// - A character outside ASCII just before `\` or a backtick is refused.
/// Run: `replace into`, `explain select`, `with ... select`, `drop table`, `truncate table`, a
/// table with `engine = InnoDB`, `engine=Aria`, `ENGINE MEMORY`, and a non-ASCII string with no `\`
/// after it (src/db.rs head_refused, table_options_refused, after_wide; FEEDBACK83.md Q-1300, found
/// by review 7 of 64df6c0; branch feedback82-85).
#[test]
fn on_mysql_a_taken_files_statement_may_begin_only_with_what_it_may_do() {
    let Some(my) = support::mariadb("cov8285db5r4") else {
        eprintln!("MariaDB is not installed: skipped");
        return;
    };
    let url = my.url("first_word");
    let d = dir("my-first");
    let refused: Vec<(&str, &str, Option<&str>, &str)> = vec![
        ("db.exec", "set names gbk", None, "begins:set"),
        ("db.exec", "set @x = 1", None, "begins:set"),
        ("db.exec", "charset gbk", None, "begins:charset"),
        ("db.query", "show tables", None, "begins:show"),
        ("db.query", "show engine innodb status", None, "begins:show"),
        ("db.exec", "install soname 'ha_connect'", None, "begins:install"),
        ("db.exec", "uninstall soname 'ha_connect'", None, "begins:uninstall"),
        ("db.exec", "handler kits open", None, "begins:handler"),
        ("db.exec", "lock tables kits read", None, "begins:lock"),
        ("db.exec", "rename table kits to kits9", None, "begins:rename"),
        ("db.exec", "flush tables", None, "begins:flush"),
        ("db.exec", "kill 1", None, "begins:kill"),
        ("db.exec", "purge binary logs before now()", None, "begins:purge"),
        ("db.exec", "xa start 'x'", None, "begins:xa"),
        ("db.exec", "optimize table kits", None, "begins:optimize"),
        ("db.exec", "repair table kits", None, "begins:repair"),
        ("db.exec", "checksum table kits", None, "begins:checksum"),
        ("db.exec", "use mysql", None, "begins:use"),
        ("db.exec", "help 'select'", None, "begins:help"),
        ("db.query", "describe kits", None, "begins:describe"),
        ("db.query", "explain kits", None, "begins:kits"),
        ("db.exec", "create table kit_c (line varchar(500)) engine=CONNECT table_type=DOS file_name='/etc/hosts'", None, "reaches:`engine = connect`"),
        ("db.exec", "create table kit_f (x int) engine = federated connection = 'mysql://root@127.0.0.1/x/t'", None, "reaches:`engine = federated`"),
        ("db.exec", "create table kit_m (x int) engine = merge union = (kits)", None, "reaches:`engine = merge`"),
        ("db.exec", "create table kit_u (x int) union = (kits)", None, "reaches:`union = (...)`"),
        ("db.exec", "create table kit_t2 (x int) table_type = DOS", None, "reaches:`table_type`"),
        ("db.exec", "create table kit_d (x int) data directory = '/tmp'", None, "reaches:`data directory`"),
        ("db.exec", "alter table kits engine = connect", None, "reaches:`engine = connect`"),
        ("db.query", "select * from information_schema.innodb_trx", None, "reads:the catalog innodb_trx"),
        ("db.query", "select * from information_schema.INNODB_LOCK_WAITS", None, "reads:the catalog innodb_lock_waits"),
        ("db.query", "select user from mysql.user", None, "reads:the `mysql` schema"),
        ("db.query", "select '\u{4e01}\\\\' as a", None, WIDE),
        ("db.query", "select 1 as `\u{e9}`", None, WIDE),
    ];
    let ran: Vec<(&str, &str, Option<&str>)> = vec![
        ("db.exec", "replace into kits values (5)", None),
        ("db.query", "explain select * from kits", None),
        ("db.query", "with t as (select 1 as x) select x from t", None),
        ("db.exec", "create table if not exists kit_i (x int) engine = InnoDB", None),
        ("db.exec", "create table if not exists kit_a (x int) engine=Aria", None),
        ("db.exec", "create table if not exists kit_mem (x int) ENGINE MEMORY", None),
        ("db.exec", "truncate table kit_i", None),
        ("db.exec", "drop table kit_a", None),
        ("db.query", "select '\u{4e01}\u{e9} caf\u{e9}' as s", None),
    ];
    let mut calls: Vec<(&str, &str, Option<&str>)> = refused.iter().map(|(f, s, v, _)| (*f, *s, *v)).collect();
    calls.extend(ran.iter().copied());
    write(&d, "kit/k.qwe", &kit_with(&calls));
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists kits (x int)\")\nprint(\"main: \" + str(db.query(\"show tables\")))\nprint(steal())\nprint(\"tables: \" + str(db.query(\"select table_name as t from information_schema.tables where table_schema = database() order by 1\")))\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("main: ["), "{}", out);
    let whats: Vec<&str> = refused.iter().map(|(_, _, _, w)| *w).collect();
    assert!(out.contains(&said_r4(&whats, ran.len(), "kit/k.qwe")), "{}", out);
    // The tables made: the kit's InnoDB and MEMORY ones and the main file's, none of the refused.
    let tables = out.lines().find(|l| l.starts_with("tables: ")).unwrap_or_default();
    for t in ["\"kit_i\"", "\"kit_mem\"", "\"kits\""] {
        assert!(tables.contains(t), "{}", out);
    }
    assert_eq!(tables.matches("t: ").count(), 3, "{}", out);
}

// ---- review 8 of cb0ce59's fixes -----------------------------------------------------------------

/// What MariaDB's `[` is refused as.
const BRACKET: &str = "unsure:a `[`, which MariaDB's sql_mode MSSQL takes for the start of a quoted name";

/// Under a quillo.confirmed on MySQL (MariaDB) a taken file's `[` outside a string, a comment or a
/// quoted name is refused: under `sql_mode=MSSQL` (the server's here, set by a run before; a main
/// file's `set sql_mode` stays on its connection too), `[...]` is a quoted name, and `select 1 as
/// [a']; execute immediate concat(0x..., 0x...) -- '` wrote Quillo's own table, the `'` in it
/// opening a string for the reading that the server did not; `select [x] from kits` is refused too. A `[` in a string (a JSON path `'$[0]'`, `'[a'']'`),
/// in each kind of comment and in a backtick name runs, under MSSQL, and the main file's `[y]`
/// runs; `_quillo_mine` keeps the main file's row alone (src/db.rs tokens, may_run_text;
/// FEEDBACK83.md Q-1300, found by review 8 of cb0ce59; branch feedback82-85).
#[test]
fn on_mysql_a_taken_files_bracket_is_refused_as_sql_mode_mssql_reads_it() {
    let Some(my) = support::mariadb("cov8285db5r5") else {
        eprintln!("MariaDB is not installed: skipped");
        return;
    };
    let url = my.url("brackets");
    let d = dir("my-brackets");
    let refused: Vec<(&str, &str, Option<&str>, &str)> = vec![
        ("db.exec", "select 1 as [a']; execute immediate concat(0x696e7365727420696e746f205f7175696c, 0x6c6f5f6d696e652076616c75657320283229) -- '", None, BRACKET),
        ("db.query", "select [x] from kits", None, BRACKET),
    ];
    let ran: Vec<(&str, &str, Option<&str>)> = vec![
        ("db.query", "select json_extract('[1,2]', '$[0]') as v", None),
        ("db.query", "select '[a'']' as s", None),
        ("db.query", "select 1 as x /* [a' */", None),
        ("db.query", "select 1 as x -- [a'", None),
        ("db.query", "select 1 as x # [a'", None),
        ("db.query", "select 1 as `[a'`", None),
    ];
    let mut calls: Vec<(&str, &str, Option<&str>)> = refused.iter().map(|(f, s, v, _)| (*f, *s, *v)).collect();
    calls.extend(ran.iter().copied());
    // The server's mode is set by a run of its own, so that each connection of the next is opened
    // under it, whichever of the pool's a statement takes.
    write(&d, "main.qwe", "db.exec(\"set global sql_mode = 'MSSQL'\")\n");
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    write(&d, "kit/k.qwe", &kit_with(&calls));
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists kits (x int)\")\ndb.exec(\"create table if not exists _quillo_mine (x varchar(40))\")\ndb.exec(\"insert into _quillo_mine values ('main-secret')\")\nprint(\"mode: \" + str(db.one(\"select @@session.sql_mode as m\")))\nprint(\"main: \" + str(db.query(\"select x as [y] from kits\")))\nprint(steal())\nprint(\"rows: \" + str(db.query(\"select x from _quillo_mine order by 1\")))\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.lines().any(|l| l.starts_with("mode: ") && l.contains("MSSQL")), "{}", out);
    assert!(out.contains("main: []\n"), "{}", out);
    let whats: Vec<&str> = refused.iter().map(|(_, _, _, w)| *w).collect();
    assert!(out.contains(&kit_said(&whats, ran.len(), "kit/k.qwe")), "{}", out);
    assert!(out.contains("rows: [{ x: \"main-secret\" }]\n"), "{}", out);
}

/// The refusal of a function that takes a table, `f`, given anything but the literals it may take.
fn given_from_text(f: &str) -> String {
    format!("the function {} given a table's name worked out as it runs", f)
}

/// Under a quillo.confirmed on PostgreSQL a taken file's call of a function that takes a table
/// (`regclass_fn`, PostgreSQL 17's `pg_proc` list) takes the table by a plain string with a letter
/// in it, and a number only where the function takes a value:
/// - A bare word is refused: `setval(n, 1000000)` and `nextval(n)` of
///   `(select '_quil' || 'lo_mine_id_seq' as n) t` moved Quillo's own sequence;
///   `has_table_privilege(n, ...)`, `gin_clean_pending_list(n)`, `currval(kit_seq)`, a word as
///   `setval`'s value and as `brin_summarize_range`'s block are refused too.
/// - `row_security_active` and `brin_summarize_new_values` of a concatenation are refused, as the
///   others were.
/// - A table by its number is refused: `nextval('16390')`, `col_description(16384, 1)`; and a
///   string with a prefix or a backslash (`E'kit_seq'`).
/// Run: `nextval('kit_seq')`, `setval('kit_seq', 5, true)`, `has_column_privilege('kits', 'x',
/// 'select')`, `has_sequence_privilege`, `row_security_active('kits')`,
/// `brin_summarize_new_values('kit_brin')`, `brin_summarize_range('kit_brin', 0)`, a JSON path
/// `'$[0]'`, and the names in a string or a comment. The main file's `setval(n, 2)` of the same
/// subquery runs, and Quillo's sequence is where the main file left it (src/db.rs regclass_fn,
/// plain_arguments; FEEDBACK83.md Q-1300, found by review 8 of cb0ce59; branch feedback82-85).
#[test]
fn on_postgresql_a_function_that_takes_a_table_takes_it_by_a_literal() {
    let Some(pg) = support::postgres("cov8285db5r5") else {
        eprintln!("PostgreSQL is not installed: skipped");
        return;
    };
    let url = pg.database("by_literal");
    let d = dir("pg-literal");
    let refused: Vec<(&str, &str, Option<&str>, String)> = vec![
        ("db.query", "select setval(n, 1000000) as v from (select '_quil' || 'lo_mine_id_seq' as n) t", None, given_from_text("setval")),
        ("db.query", "select nextval(n) as v from (select '_quil' || 'lo_mine_id_seq' as n) t", None, given_from_text("nextval")),
        ("db.query", "select has_table_privilege(n, 'select') as v from (select '_quil' || 'lo_mine' as n) t", None, given_from_text("has_table_privilege")),
        ("db.query", "select row_security_active('_quil' || 'lo_mine') as v", None, given_from_text("row_security_active")),
        ("db.query", "select brin_summarize_new_values('_quil' || 'lo_mine_pkey') as v", None, given_from_text("brin_summarize_new_values")),
        ("db.query", "select gin_clean_pending_list(n) as v from (select 'kit_brin' as n) t", None, given_from_text("gin_clean_pending_list")),
        ("db.query", "select currval(kit_seq) as v", None, given_from_text("currval")),
        ("db.query", "select setval('kit_seq', n) as v from (select 5 as n) t", None, given_from_text("setval")),
        ("db.query", "select brin_summarize_range('kit_brin', n) as v from (select 0 as n) t", None, given_from_text("brin_summarize_range")),
        ("db.query", "select nextval('16390') as v", None, given_from_text("nextval")),
        ("db.query", "select col_description(16384, 1) as v", None, given_from_text("col_description")),
        ("db.query", "select nextval(E'kit_seq') as v", None, given_from_text("nextval")),
    ];
    let ran: Vec<(&str, &str, Option<&str>)> = vec![
        ("db.query", "select nextval('kit_seq') as n", None),
        ("db.query", "select setval('kit_seq', 5, true) as n", None),
        ("db.query", "select has_column_privilege('kits', 'x', 'select') as p", None),
        ("db.query", "select has_sequence_privilege('kit_seq', 'usage') as p", None),
        ("db.query", "select row_security_active('kits') as v", None),
        ("db.query", "select brin_summarize_new_values('kit_brin') as v", None),
        ("db.query", "select brin_summarize_range('kit_brin', 0) as v", None),
        ("db.query", "select jsonb_path_query_first('[1, 2]', '$[0]') as v", None),
        ("db.query", "select '[' || 'nextval(n)' as s, 1 as x /* setval(n, 1) [ */", None),
    ];
    let mut calls: Vec<(&str, &str, Option<&str>)> = refused.iter().map(|(f, s, v, _)| (*f, *s, *v)).collect();
    calls.extend(ran.iter().copied());
    write(&d, "kit/k.qwe", &kit_with(&calls));
    write(&d, "main.qwe", "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists kits (x int primary key)\")\ndb.exec(\"create index if not exists kit_brin on kits using brin (x)\")\ndb.exec(\"create sequence if not exists kit_seq\")\ndb.exec(\"create table if not exists _quillo_mine (id serial primary key, x text)\")\ndb.exec(\"insert into _quillo_mine (x) values ('tok-A'), ('tok-B')\")\nprint(\"main: \" + str(db.one(\"select setval(n, 2) as v from (select '_quil' || 'lo_mine_id_seq' as n) t\")))\nprint(steal())\nprint(\"seq: \" + str(db.one(\"select last_value as v from _quillo_mine_id_seq\")))\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("main: { v: 2 }\n"), "{}", out);
    let whats: Vec<&str> = refused.iter().map(|(_, _, _, w)| w.as_str()).collect();
    assert!(out.contains(&said_r4(&whats, ran.len(), "kit/k.qwe")), "{}", out);
    assert!(out.contains("seq: { v: 2 }\n"), "{}", out);
}
