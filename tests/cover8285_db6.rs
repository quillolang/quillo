//! Edge tests of the fail-closed reader of a taken file's SQL under a `quillo.confirmed`
//! (FEEDBACK83.md, Q-1300; the user's decision of 2026-11-24, "Refuse text-run SQL"): the escapes
//! of PostgreSQL's `E'...'` and `U&'...'` strings and MySQL's strings as each server works them
//! out (`unescaped`, `unicode_texts`), MySQL's bit strings (`bytes_name_own`) and `/*M! */`
//! comments, PostgreSQL's nested comments, and what the reader cannot read for certain: a comment,
//! a string, a quoted name or a dollar quote not closed (`tokens`) (src/db.rs; branch
//! feedback82-85). Each test asserts what a developer sees: the answer or the refusal, word for
//! word, and that nothing was written.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::temp_dir;

// ---- helpers, as tests/cover8285_db5.rs has them ------------------------------------------------

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
    Dir(temp_dir(&format!("cov8285db6-{}", name)))
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

/// A Quillo string of `s`, as written in a `.qwe` file: `\r` written as its escape, which a file
/// read as source would take for a line's end.
fn lit(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r"))
}

/// A kit taken for `Badge`, whose `steal()` makes each call of `calls` (a function, `db.query` or
/// `db.exec`, and its SQL) and says what each gave, a line each: the error, or what it gave.
fn kit(calls: &[(&str, &str)]) -> String {
    let lines: Vec<String> = calls.iter().map(|(f, sql)| format!("    said({}({}))", f, lit(sql))).collect();
    format!(
        "component Badge() {{\n  return <b>b</b>\n}}\nfn steal() {{\n  let said = (r) => is_err(r) ? r.error : str(r)\n  return \"kit:\\n\" + join([\n{}\n  ], \"\\n\")\n}}\n",
        lines.join(",\n")
    )
}

/// What the hold says when a taken file's SQL names one of Quillo's own tables.
fn own_named(file: &str) -> String {
    format!("db: Quillo's own tables (_quillo_...) are refused for {}'s SQL where a quillo.confirmed holds the run: they hold what Quillo does (push subscriptions, jobs, tokens), and this statement names one", file)
}

/// What the hold says when a taken file's SQL cannot be read for certain.
fn unsure(what: &str, file: &str) -> String {
    format!("db: this statement has {}, which this reading cannot read for certain as the server would, and what it cannot read is refused for {}'s SQL where a quillo.confirmed holds the run. Run it from the main file's SQL, or from a migration the main file runs", what, file)
}

const COMMENT_OPEN: &str = "a comment not closed";
const STRING_OPEN: &str = "a string or a quoted name not closed";

/// Each call of `calls` (a function, its SQL and what the kit's line says: `own` for the refusal of
/// Quillo's own tables, `comment` or `string` for what is not closed, else the rows as written),
/// and the lines `steal()` gives for them.
fn calls_and_said<'a>(calls: &'a [(&'a str, &'a str, &'a str)]) -> (Vec<(&'a str, &'a str)>, String) {
    let file = "kit/k.qwe";
    let mut want = vec!["kit:".to_string()];
    for (_, _, said) in calls {
        want.push(match *said {
            "own" => own_named(file),
            "comment" => unsure(COMMENT_OPEN, file),
            "string" => unsure(STRING_OPEN, file),
            rows => rows.to_string(),
        });
    }
    (calls.iter().map(|(f, s, _)| (*f, *s)).collect(), format!("{}\n", want.join("\n")))
}

/// The bits of `s`'s bytes, as MySQL's `B'...'` and `0b...` write them.
fn bits(s: &str) -> String {
    s.bytes().map(|b| format!("{:08b}", b)).collect()
}

// ---- PostgreSQL ------------------------------------------------------------------------------------

/// Under a quillo.confirmed on PostgreSQL a taken file's `E'...'` and `U&'...'` strings are worked
/// out as PostgreSQL works them out: an octal escape past 255 keeps its low byte (`\537` is `_`),
/// so `E'\537quillo\537mine'` names Quillo's own table and is refused; one or two octal digits end
/// at a backslash or the string's end (`E'\60\61\7'` is `01` and a bell); `\b \n \r \t \f` are
/// their control characters; a `\u` or a `U&` escape that is half of a surrogate pair (`\uD83D`,
/// `\D83D`) is no character by itself, PostgreSQL joining the pair to 😀; a letter before four hex
/// digits in a `U&` string is itself, the backslash's escape worked out (`U&'x0041\0041'` is
/// `x0041A`); `ſ` and the Kelvin sign are no letters of Quillo's tables. A comment nests: one
/// inside another hides `execute` and runs, and the outer one not closed is refused as what the
/// reader cannot read for certain, as is a comment, a string (a plain one ending in a backslash,
/// which a session with `standard_conforming_strings` off reads on), an `E'...'` whose last quote
/// a backslash escapes, a quoted name and a dollar quote (`$q$` closed only by `$q$`, not `$Q$`)
/// not closed, each with `execute` in it. Nothing is written to Quillo's table (src/db.rs
/// unescaped, unicode_texts, names_own, space_end_shut, tokens; FEEDBACK83.md Q-1300; coverage of
/// feedback82-85; branch feedback82-85).
#[test]
fn on_postgresql_a_taken_files_escapes_comments_and_text_not_closed_are_read_as_postgresql_reads_them() {
    let Some(pg) = support::postgres("cov8285db6") else {
        eprintln!("PostgreSQL is not installed: skipped");
        return;
    };
    let url = pg.database("escapes");
    let d = dir("pg");
    let calls: Vec<(&str, &str, &str)> = vec![
        ("db.query", r"select E'\537quillo\537mine' as s", "own"),
        ("db.exec", r"insert into kits values (E'\537quillo\537mine')", "own"),
        ("db.query", r"select E'\60\61\7' = E'01\x07' as s, E'\60' as zero", "[{ s: true, zero: \"0\" }]"),
        ("db.query", r"select encode(convert_to(E'\b\n\r\t\f', 'UTF8'), 'hex') as h", "[{ h: \"080a0d090c\" }]"),
        ("db.query", r"select E'\uD83D\uDE00' as s", "[{ s: \"\u{1f600}\" }]"),
        ("db.query", r"select U&'\D83D\DE00' as s", "[{ s: \"\u{1f600}\" }]"),
        ("db.query", r"select U&'x0041\0041' as s", "[{ s: \"x0041A\" }]"),
        ("db.query", "select '\u{17f} \u{212a}' as s", "[{ s: \"\u{17f} \u{212a}\" }]"),
        ("db.query", "select 1 /* a /* b */ execute */ as one", "[{ one: 1 }]"),
        ("db.query", "select $q$ execute $q$ as s", "[{ s: \" execute \" }]"),
        ("db.query", "select 1 /* /* execute */ as one", "comment"),
        ("db.query", "select 1 as one /* execute", "comment"),
        ("db.query", "select $q$ execute", "string"),
        ("db.query", "select $q$ execute $Q$ as s", "string"),
        ("db.query", "select 'execute", "string"),
        ("db.query", r"select 'execute\' as s", "string"),
        ("db.query", r"select E'execute\' as s", "string"),
        ("db.query", "select \"execute", "string"),
    ];
    let (made, said) = calls_and_said(&calls);
    write(&d, "kit/k.qwe", &kit(&made));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists kits (x text)\")\ndb.exec(\"create table if not exists _quillo_mine (x text)\")\nprint(steal())\nprint(\"mine: \" + str(db.query(\"select x from _quillo_mine\")) + \" kits: \" + str(db.query(\"select x from kits\")))\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains(&said), "{}", out);
    assert!(out.contains("mine: [] kits: []\n"), "{}", out);
}

// ---- MySQL (MariaDB) -------------------------------------------------------------------------------

/// Under a quillo.confirmed on MySQL (MariaDB) a taken file's strings are worked out as MySQL works
/// them out: `\0` is a NUL (`hex('a\0b')` is `610062`), `\%` and `\_` keep their backslash (four
/// characters), `\Z` is 0x1A, `\b \n \r \t` their control characters and `\f` is `f`, no escape of
/// MySQL's; hex and bit strings (`X'41'`, `B'01000010'`, `0b01000011`, `0x44`, `b'1'`) that name
/// nothing run, and a bit string that is the SQL of a statement naming Quillo's own table, as
/// `B'...'`, as `0b...` and with the first byte's leading 0 left out, is refused as naming it, as is
/// the same SQL in a string inside a `/*M! */` comment, whose text MariaDB runs: the hold reads it
/// as SQL, where a plain comment's text is none. A comment does not nest (`/* /* execute */` is
/// closed), and a comment, a string and a quoted name not closed, with `execute` in them, are
/// refused as what the reader cannot read for certain. Nothing is written to Quillo's table
/// (src/db.rs unescaped, bytes_name_own, read_own, space_end_shut, tokens; FEEDBACK83.md Q-1300;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn on_mysql_a_taken_files_escapes_bit_strings_and_text_not_closed_are_read_as_mysql_reads_them() {
    let Some(my) = support::mariadb("cov8285db6") else {
        eprintln!("MariaDB is not installed: skipped");
        return;
    };
    let url = my.url("escapes");
    let d = dir("my");
    let own_sql = bits("insert into _quillo_mine values ('kit bits')");
    let by_bits = format!("execute immediate B'{}'", own_sql);
    let by_0b = format!("execute immediate 0b{}", own_sql);
    // `i` is 0x69, `01101001`: its leading 0 left out, the bits are padded on the left as MySQL pads them.
    let by_short = format!("execute immediate b'{}'", &own_sql[1..]);
    let calls: Vec<(&str, &str, &str)> = vec![
        ("db.query", r"select hex('a\0b') as h", "[{ h: \"610062\" }]"),
        ("db.query", r"select '\%\_' as s, length('\%\_') as n", "[{ s: \"\\\\%\\\\_\", n: 4 }]"),
        ("db.query", r"select hex('\Z') as z, '\f' as f, hex('\b\n\r\t') as c", "[{ z: \"1A\", f: \"f\", c: \"080A0D09\" }]"),
        ("db.query", "select X'41' as a, B'01000010' as b, 0b01000011 as c, 0x44 as d, b'1' = 1 as e", "[{ a: \"A\", b: \"B\", c: \"C\", d: \"D\", e: 1 }]"),
        ("db.exec", &by_bits, "own"),
        ("db.exec", &by_0b, "own"),
        ("db.exec", &by_short, "own"),
        ("db.exec", r"/*M!100000 execute immediate 'insert into _qu\illo_mine values (5)' */", "own"),
        ("db.query", "select 1 /* /* execute */ as one", "[{ one: 1 }]"),
        ("db.query", "select 1 as one /* execute", "comment"),
        ("db.query", "select 'execute", "string"),
        ("db.query", r"select 'execute\' as s", "string"),
        ("db.query", "select `execute", "string"),
        ("db.query", "select \"execute", "string"),
    ];
    let (made, said) = calls_and_said(&calls);
    write(&d, "kit/k.qwe", &kit(&made));
    write(
        &d,
        "main.qwe",
        "import kit/k.qwe use Badge, steal\ndb.exec(\"create table if not exists _quillo_mine (x text)\")\nprint(steal())\nprint(\"mine: \" + str(db.query(\"select x from _quillo_mine\")))\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    checked(&d);
    let (code, out) = run(&d, &[("QUILLO_DB", &url)]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains(&said), "{}", out);
    assert!(out.contains("mine: []\n"), "{}", out);
}
