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

/// A Quillo string of `s`, as written in a `.qwe` file.
fn lit(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
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

/// The lines `steal()` gives for `refused` (each the hold's refusal of what it names) then `ran`.
fn kit_said(refused: &[&str], ran: usize, file: &str) -> String {
    let mut want = vec!["kit:".to_string()];
    want.extend(refused.iter().map(|w| text_run(w, file)));
    want.extend(std::iter::repeat_n("ran".to_string(), ran));
    format!("{}\n", want.join("\n"))
}

/// Under a quillo.confirmed on PostgreSQL a taken file's statement that has the database run SQL
/// from text, or name a table from text, is refused, says what it found and what to do, and
/// nothing of it runs: `do` (in capitals, in mixed case after a comment and spaces, and as a second
/// statement after `;`), `prepare`, `execute` (after `explain analyze` too), `call`, `create [or
/// replace] function`/`procedure`, `create trigger` (and inside `create schema`), `create rule`,
/// `create event trigger`, `copy ... program`, the `*_to_xml` functions (in a schema, by a quoted
/// name, by a `U&` name whose escape hides a letter), `ts_stat`, `dblink`, `nextval`/`setval`/`currval`, `to_regclass`, and `regclass` and
/// `oid` as types (`::regclass` of a text, of a value, `::pg_catalog.regclass`, `cast(... as
/// regclass)`, `regclass '...'`, `::oid` of a number). The same words in a string, a dollar quote,
/// a comment or a quoted name run, as do `create table`, `oid` as a column and SQL naming none of
/// them. The main file's same statements run, and the kit's without a quillo.confirmed (src/db.rs
/// text_run, FEEDBACK83.md Q-1300, the user's decision of 2026-11-24; branch feedback82-85).
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
        ("db.exec", "create schema kit_s create table kit_t (x int) create trigger kit_tt after insert on kit_t for each row execute function main_f()", None, "`create trigger`"),
        ("db.exec", "create rule kit_r as on insert to kits do instead nothing", None, "`create rule`"),
        ("db.exec", "create event trigger kit_e on ddl_command_start execute function main_f()", None, "`create event`"),
        ("db.exec", "copy kits to program 'cat'", None, "`copy ... program`"),
        ("db.query", "select query_to_xml('select 1', true, false, '')", None, "the function query_to_xml"),
        ("db.query", "select pg_catalog.table_to_xmlschema('kits', true, false, '')", None, "the function table_to_xmlschema"),
        ("db.query", "select \"cursor_to_xml\"('c', 1, true, false, '')", None, "the function cursor_to_xml"),
        ("db.query", r#"select U&"\0071uery_to_xml"('select 1', true, false, '')"#, None, "the function query_to_xml"),
        ("db.query", "select ts_stat('select 1')", None, "the function ts_stat"),
        ("db.query", "select dblink('host=x', 'select 1')", None, "the function dblink"),
        ("db.query", "select nextval('kit_seq')", None, "the function nextval"),
        ("db.query", "select SetVal('kit_seq', 1)", None, "the function setval"),
        ("db.query", "select currval('kit_seq')", None, "the function currval"),
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
        ("db.query", "select count(*) as n from pg_class where oid > 0 and relname = 'kits'", None),
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
/// `/*! */` comment (its version's digits before the word too), and as a second statement after `;`. The same words in a string, a comment
/// (`#`, `-- `, `/* */`) or a quoted name run, as do `create table` and `do 1`. The main file's
/// `execute immediate` runs (src/db.rs text_run, FEEDBACK83.md Q-1300, the user's decision of
/// 2026-11-24; branch feedback82-85).
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
        ("db.exec", "create definer = current_user function kit_f() returns int return 1", None, "`create function`"),
        ("db.exec", "create trigger kit_t before insert on kits for each row set new.x = new.x", None, "`create trigger`"),
        ("db.exec", "create event kit_e on schedule every 1 day do select 1", None, "`create event`"),
        ("db.exec", "alter event main_e disable", None, "`alter event`"),
        ("db.exec", "begin not atomic select 1; end", None, "`begin not atomic`"),
        ("db.exec", "/*! call kit_p() */", None, "`call`"),
        ("db.exec", "/*!50000call kit_p() */", None, "`call`"),
        ("db.exec", "select 1; execute immediate 'select 1'", None, "`execute immediate`"),
    ];
    let ran: Vec<(&str, &str, Option<&str>)> = vec![
        ("db.query", "select 'call p(); execute immediate \\'x\\'; create procedure' as s", None),
        ("db.query", "select 1 as one # call p()\n", None),
        ("db.query", "select 1 as one -- execute immediate 'x'\n", None),
        ("db.query", "select 1 /* create trigger t */ as one", None),
        ("db.query", "select 1 as `call`, 2 as `execute`", None),
        ("db.exec", "create table if not exists kits2 (x int)", None),
        ("db.exec", "do 1", None),
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
/// number (`{ "$db": 0, query: db.query }`) is judged as the connection it names, for the file that
/// runs the statement: confirmed for nothing, its `query`, `exec`, `db.transaction` and `db.atomic`
/// on it are refused, nothing read; a number that is no connection says so; a second kit confirmed
/// to write that database reads it through the same map, and so does the main file (src/db.rs
/// conn_ok, FEEDBACK83.md Q-1300, found by the review of S3's fixes; branch feedback82-85).
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
        "component Badge() {\n  return <b>b</b>\n}\nfn steal() {\n  let said = (r) => is_err(r) ? r.error : str(r)\n  let m = { \"$db\": 0, query: db.query, exec: db.exec, transaction: db.transaction }\n  let none = { \"$db\": 99, query: db.query }\n  return \"kit:\\n\" + join([said(m.query(\"select v from secrets\")), said(m.exec(\"delete from secrets\")), said(m.transaction(() => 1)), said(db.atomic([m], () => 1)), said(none.query(\"select 1\"))], \"\\n\")\n}\n",
    );
    write(&d, "lib/l.qwe", "component Card() {\n  return <i>c</i>\n}\nfn read_secret() {\n  let m = { \"$db\": 0, query: db.query }\n  return str(m.query(\"select v from secrets\"))\n}\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n\n[\"lib/l.qwe\"]\nwrites = [\"data/secret.db\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[]);
    assert_eq!(code, 0, "{}", out);
    let refused = "db: data/secret.db is not confirmed for kit/k.qwe (quillo confirm)";
    let want = ["kit:", refused, refused, refused, refused, "db: this connection is not one of this program's"].join("\n");
    assert!(out.contains(&format!("{}\n", want)), "{}", out);
    assert!(out.contains("lib: [{ v: \"main secret\" }]\nmain: [{ v: \"main secret\" }]\n"), "{}", out);
    assert_eq!(column(&d.join("data/secret.db"), "select v from secrets"), ["main secret"]);
}

/// Under a quillo.confirmed a kit confirmed for nothing that writes a connection's map itself, by
/// `set`, as a literal or by `merge`, with `db.one` and `db.exec` and a connection's number, gets
/// no connection by it: the number of a helper file's connection (`libc`, which only the helper was
/// confirmed to write), of the main file's own, and of one never made are each refused, reading and
/// writing, and `db.connect` of a target already open (the helper's, the main file's) is refused as
/// a new one would be; nothing is written, and the helper's and the main file's own calls of them
/// run. `quillo check` names the kit's `db.connect` calls (src/db.rs conn_ok, FEEDBACK83.md
/// Q-1300, found by the review of S3's fixes; branch feedback82-85).
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
    let mut want = vec!["kit:".to_string()];
    for (n, why) in [(0, lib), (1, main), (7, none)] {
        want.extend(std::iter::repeat_n(format!("{}: {} | {}", n, why, why), 3));
    }
    want.push(format!("connect: {}", lib));
    want.push(format!("connect main's: {}", main));
    assert!(out.contains(&format!("{}\n", want.join("\n"))), "{}", out);
    assert!(out.contains("after: [{ v: \"lib secret\" }] [{ v: \"main secret\" }]\n"), "{}", out);
    for db in ["data/lib.db", "data/main.db", "data/app.db"] {
        assert!(column(&d.join(db), "select name from sqlite_master where name = 'kit_wrote'").is_empty(), "{}", db);
    }
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
