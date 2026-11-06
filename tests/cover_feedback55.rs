//! The lines FEEDBACK55 added that no other test ran (branch feedback55): each test makes the
//! case that reaches them, and says what it must give. Checks of a program and functions of the
//! library are asked here; what needs a server, a socket, a plugin or a database starts one,
//! under a deadline. Tests with PostgreSQL or MariaDB are skipped when they are not installed.

mod support;

use quillo::check::{check_source, Severity};
use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// ---- helpers ----------------------------------------------------------------------------

/// A fresh folder for one test.
fn folder(name: &str) -> PathBuf {
    support::temp_dir(&format!("cover-f55-{}", name))
}

/// What `quillo check` says of `src`, a line each: "line: message", "line warning: message".
fn said(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new("."))
        .diagnostics
        .iter()
        .map(|d| match d.severity {
            Severity::Error => format!("{}: {}", d.line, d.message),
            Severity::Warning => format!("{} warning: {}", d.line, d.message),
            Severity::Hint => format!("{} hint: {}", d.line, d.message),
        })
        .collect()
}

/// What `body` gives, run as a function's body on `it`, as written by `repr`; a run-time error as
/// "ERROR: ...".
fn ev(it: &mut Interp, body: &str) -> String {
    it.load_source(&format!("fn __t() {{\n{}\n}}\n", body), "case.qwe").unwrap_or_else(|x| panic!("{}\n{}", x, body));
    match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    }
}

/// A program of `src` in a folder of its own, its files there too, ready for requests.
fn program(name: &str, src: &str, files: &[(&str, &[u8])]) -> (PathBuf, Interp) {
    let d = folder(name);
    for (f, bytes) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    std::fs::write(d.join("app.qwe"), src).unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("app.qwe")).unwrap_or_else(|e| panic!("{}\n{}", e, src));
    (d, it)
}

/// A request to `it`, in the process.
fn ask(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: &[u8]) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "localhost".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: h, body: body.to_vec(), remote: "127.0.0.1".into() })
}

/// `quillo args` in `cwd` with `stdin`, under a deadline: its exit code, stdout and stderr.
fn quillo(cwd: &Path, args: &[&str], secs: u64) -> (Option<i32>, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (mut o, mut e) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let out = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = o.read_to_string(&mut s);
        s
    });
    let err = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = e.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + Duration::from_secs(secs);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("quillo {:?} did not end", args);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (status.code(), out.join().unwrap(), err.join().unwrap())
}

/// Whether there is a Rust to build plugins with.
fn cargo() -> bool {
    Command::new("cargo").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// `f` until it gives something, for at most `secs`.
fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ---- check.rs ---------------------------------------------------------------------------

// A builtin the docs do not list (Quillo's own, called by the code it makes) is told its call
// with its parameters numbered.
#[test]
fn a_builtin_without_docs_is_told_with_numbered_arguments() {
    assert_eq!(quillo::check::call_shape("__site"), "__site(arg1, arg2)");
    assert_eq!(quillo::check::too_many_args("__answer", 3), "`__answer` takes 2 arguments, got 3: __answer(arg1, arg2)");
    // One the docs list keeps its names; one with no fixed count none.
    assert_eq!(quillo::check::call_shape("split"), "split(s, sep)");
    assert_eq!(quillo::check::call_shape("no_such_builtin"), "no_such_builtin()");
}

// A route's answer that holds a function of the program's is told through a block that makes a
// function of another name; one that calls the block's own function holds none.
#[test]
fn a_function_a_block_makes_hides_the_programs() {
    let warn = "warning: this route's answer holds a function at `a` (the function `helper`): JSON has none, so the answer is an internal error (500): leave it out, or call it";
    let src = "fn helper() = 1\nroute GET /a {\n    fn helper() = 2\n    { n: helper() }\n}\nroute GET /b {\n    fn other() = 2\n    { a: helper, b: other() }\n}\n";
    assert_eq!(said(src), [format!("6 {}", warn)]);
    // With no block of its own, the program's function is told.
    assert_eq!(said("fn helper() = 1\nroute GET /c = { a: helper }\n"), [format!("2 {}", warn)]);
}

// `set_cookie` that is a parameter is the program's own: its options are not the builtin's.
#[test]
fn a_set_cookie_of_the_programs_own_has_any_options() {
    let got = said("fn f(set_cookie) {\n    set_cookie(\"t\", 1, { sameSite: \"x\" })\n}\nroute GET / = f((a, b, c) => null)\n");
    assert!(got.iter().all(|d| !d.contains("has no option")), "{:?}", got);
    assert!(said("route GET / { set_cookie(\"t\", \"v\", { sameSite: \"x\" }); \"ok\" }\n").iter().any(|d| d.contains("`set_cookie` has no option `sameSite`: write it `same_site`")));
}

// `every` and `schedule` as the start reads them: a first argument written out of any kind, a
// negative float, one not written out; options without a name, or not written out; a function
// whose name is no tick's, a builtin, or a field.
#[test]
fn every_written_out_is_judged_as_the_start_judges_it() {
    let refused = "2: `every` on the server takes at least 1 millisecond: every(50, step)";
    assert_eq!(said("fn step() = null\nevery(true, step)\n"), [refused.to_string(), "2: argument 1 of `every` must be num, got bool".to_string()]);
    assert_eq!(said("fn step() = null\nevery(-1.5, step)\n"), [refused]);
    assert_eq!(said("fn step() = null\nevery(-0.5, step)\n").len(), 1);
    assert_eq!(said("fn step() = null\nevery(1.5, step)\n"), Vec::<String>::new());
    // Not written out: nothing said of it, though it may be wrong when it runs.
    assert_eq!(said("fn step() = null\nlet ms = 100\nevery(-ms, step)\n"), Vec::<String>::new());
    assert_eq!(said("fn step() = null\nlet ms = 100\nevery(ms, step)\n"), Vec::<String>::new());
    // Options with no name: named after the function, so twice is refused; options in a
    // variable: not followed.
    assert_eq!(
        said("fn step() = null\nevery(100, step)\nevery(100, step, { other: 1 })\n"),
        ["3: `every`: there is already a tick named \"step\"; give this one another with { name: ... }"]
    );
    assert_eq!(said("fn step() = null\nlet opts = { name: \"x\" }\nevery(50, step, opts)\nevery(50, step, opts)\n"), Vec::<String>::new());
    // A function whose name is no tick's is named for its line: two on two lines are two.
    assert_eq!(said("fn tické() = null\nevery(100, tické)\nevery(100, tické)\n"), Vec::<String>::new());
    // A builtin, a field, a call: not followed.
    assert_eq!(said("every(100, print)\nevery(100, print)\n"), Vec::<String>::new());
    assert_eq!(said("fn tock() = null\nlet jobs = { run: tock }\nevery(100, jobs.run)\nevery(100, jobs.run)\n"), Vec::<String>::new());
    // The same for `schedule`.
    assert_eq!(said("fn job() = null\nschedule(\"5m\", job)\nschedule(\"10m\", job, { other: 1 })\n"), ["3: `schedule`: there is already a job named \"job\"; give this one another with { name: ... }"]);
}

// A function called at the top level adds its ticks each time: those named in a way not written
// out, with options that are no map, with a lambda and no name, or with a name no tick has, are
// not followed; the rest are.
#[test]
fn a_helpers_ticks_are_followed_as_far_as_they_are_written() {
    let src = "fn tick() = null\nfn named() = \"n\"\nfn tické() = null\nfn local() = null\nfn setup() {\n    every(1000, tick, { name: named() })\n    every(1000, tick, 5)\n    every(1000, (x) => x)\n    every(1000, tické)\n    every(1000, local, {})\n}\nsetup()\nsetup()\n";
    assert_eq!(
        said(src),
        ["7: argument 3 of `every` must be map | null, got int", "13: `every`: there is already a tick named \"local\"; give this one another with { name: ... }"]
    );
    // Called once: nothing twice.
    assert_eq!(said(&src.replace("setup()\nsetup()\n", "setup()\n")), ["7: argument 3 of `every` must be map | null, got int"]);
    // A tick added the same way twice by one call is one call: nothing said for it.
    let twice = "fn tick() = null\nfn setup() = [every(1000, tick)]\nsetup()\n";
    assert_eq!(said(twice), Vec::<String>::new());
}

// SQL made with `join` over what is not a list of SQL-safe text is warned of: `join()` with no
// list, `map` with too few or too many arguments, a list in a variable, a function that is no
// name, a lambda that defines a function, a map's own `map`. A call's list mapped with
// `sql_name` is safe.
#[test]
fn sql_joined_from_what_is_not_safe_is_warned_of() {
    let warn = "warning: `db.exec` with SQL built from values: a value containing a quote can change the query (SQL injection); write `?` in the SQL and pass the values after it";
    let head = "let cols = [\"title\", \"body\"]\nfn names() = [\"a\", \"b\"]\nlet fs = [sql_name]\n";
    for (sql, more) in [
        ("join()", Some("4: missing argument `xs` in call to `join`: join(xs, sep)")),
        ("join(map(cols), \", \")", Some("4: missing argument `f`, a function, in call to `map`: map(xs, f)")),
        ("cols | map(sql_name, 1) | join(\", \")", Some("4: `map` takes 2 arguments, got 3: map(xs, f)")),
        ("join(cols, \", \")", None),
        ("join(map(cols, fs[0]), \", \")", None),
        ("join(map(cols, (c) => {\n    fn q(x) = x\n    sql_name(c)\n}), \", \")", None),
        ("join({ a: \"x\" }.map(sql_name), \", \")", None),
    ] {
        let src = format!("{}route POST /a = db.exec(`select ${{{}}} from t`)\n", head, sql);
        let mut want: Vec<String> = more.into_iter().map(String::from).collect();
        want.push(format!("4 {}", warn));
        assert_eq!(said(&src), want, "{}", sql);
    }
    for sql in ["names().map(sql_name).join(\", \")", "join(map(cols, sql_name), \", \")", "join(map(cols, (c) => sql_name(c)), \", \")"] {
        let src = format!("{}route POST /a = db.exec(`select ${{{}}} from t`)\n", head, sql);
        assert_eq!(said(&src), Vec::<String>::new(), "{}", sql);
    }
}

// After a `return` at a file's top level, the first thing below it is said never to run, by its
// own line, whatever it is.
#[test]
fn what_follows_a_top_level_return_is_told_at_its_line() {
    let warn = "warning: this never runs: the `return` at line 1 leaves the file first, so nothing below it is defined";
    for below in ["route GET / = \"x\"", "type User = { a: int }", "return", "let x = 1", "service billing = \"b.qwe\"", "print(1)"] {
        assert_eq!(said(&format!("return\n\n{}\n", below)), [format!("3 {}", warn)], "{}", below);
    }
    // Nothing below it: nothing said.
    assert_eq!(said("let x = 1\nreturn\n"), Vec::<String>::new());
}

// ---- island.rs --------------------------------------------------------------------------

// A component's root, from just after its tag name: what is not an attribute of an island goes
// out as written, a `/` of a tag written closed too.
#[test]
fn a_roots_attributes_are_kept_but_an_islands() {
    let strip = |rest: &str| {
        let mut out = String::new();
        quillo::island::root_without_island_attrs(rest, &mut out);
        out
    };
    assert_eq!(strip("/>"), "/>");
    assert_eq!(strip(" data-e=\"1\" class=\"k\"/>tail"), " class=\"k\"/>tail");
    assert_eq!(strip(" class=\"a\" data-c=\"X\" data-l=x id=\"é\">in</p>"), " class=\"a\" id=\"é\">in</p>");
    // Glued to a value before it, a name is no attribute of its own: as written.
    assert_eq!(strip(" a=\"1\"data-e=\"2\">"), " a=\"1\"data-e=\"2\">");
    assert_eq!(strip(""), "");
}

// ---- server.rs: live data's topics -------------------------------------------------------

// Topics in a POST's body (`tm=1`) asked of the program itself, not a worker: each `%` that
// starts no `%25` or `%2C` is kept as written; a body past what a page may follow is refused.
#[test]
fn topics_after_the_address_are_read_by_the_program_too() {
    let (_d, mut it) = program("live-tm", "route GET /pub/*t {\n    publish(t)\n    \"ok\"\n}\n", &[]);
    let seq = |it: &mut Interp, since: u64, body: &str| -> serde_json::Value {
        let r = ask(it, "POST", &format!("/_quillo/live?since={}&tm=1", since), &[("Content-Type", "text/plain")], body.as_bytes());
        assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
        serde_json::from_slice(&r.body).unwrap()
    };
    for (topic, sent) in [("50%off", "50%off"), ("a%41", "a%41"), ("a%2", "a%2"), ("%", "%"), ("x,y", "x%2Cy"), ("p%q", "p%25q"), ("é%", "é%")] {
        let before = seq(&mut it, 0, "")["seq"].as_u64().unwrap();
        let enc: String = topic.bytes().map(|b| format!("%{:02X}", b)).collect();
        assert_eq!(ask(&mut it, "GET", &format!("/pub/{}", enc), &[], b"").status, 200);
        let got = seq(&mut it, before, &format!("{},other", sent));
        assert_eq!(got["changed"], serde_json::json!([topic]), "{}", topic);
    }
    // At the limit, and one byte past it.
    let most = 256 * (448 * 3 + 1);
    let r = ask(&mut it, "POST", "/_quillo/live?since=0&tm=1", &[], "a".repeat(most).as_bytes());
    assert_eq!(r.status, 200);
    let r = ask(&mut it, "POST", "/_quillo/live?since=0&tm=1", &[], "a".repeat(most + 1).as_bytes());
    assert_eq!((r.status, String::from_utf8_lossy(&r.body).into_owned()), (413, "more topics than a page follows\n".to_string()));
}

// ---- sql.rs -----------------------------------------------------------------------------

use quillo::sql::{scan, Dialect};

// Numbers with a signed exponent are one number, and an operator ends where a comment starts:
// what follows is read as it is.
#[test]
fn sql_numbers_and_operators_end_where_postgresql_ends_them() {
    let s = scan(Dialect::Postgres, "select 1e-3 + ?, 2E+10 * ?, 3e5");
    assert_eq!((s.count, s.native()), (2, "select 1e-3 + $1, 2E+10 * $2, 3e5"));
    // `+--`: the `+`, then a comment, whose `?` is no value.
    let s = scan(Dialect::Postgres, "select 1 +-- ?\n, ? as v");
    assert_eq!((s.count, s.native()), (1, "select 1 +-- ?\n, $1 as v"));
    let s = scan(Dialect::Postgres, "select 2 */* ? */ ? as v");
    assert_eq!((s.count, s.native()), (1, "select 2 */* ? */ $1 as v"));
}

// `create` that makes no routine: a `;` soon after it, or none of the words that say what it
// makes in the first sixteen: statements end at their `;`.
#[test]
fn sql_create_of_no_routine_ends_at_its_semicolon() {
    for d in Dialect::ALL {
        let s = scan(d, "create ; select ?");
        assert_eq!((s.stmts.len(), s.count), (2, 1), "{:?}", d);
        let s = scan(d, "create a b c d e f g h i j k l m n o p q; select ?");
        assert_eq!((s.stmts.len(), s.count), (2, 1), "{:?}", d);
    }
    // A function's body is its own, as before.
    let s = scan(Dialect::Postgres, "create function f() returns int language sql begin atomic select 1; end; select ?");
    assert_eq!(s.count, 1);
}

// MySQL: no statement that commits by itself has a first word longer than `uninstall`.
#[test]
fn mysql_reads_long_first_words_as_no_commit() {
    assert_eq!(scan(Dialect::Mysql, "deallocate prepare s").commits_by_itself(), None);
    assert_eq!(scan(Dialect::Mysql, "checksum_table t").commits_by_itself(), None);
    assert!(scan(Dialect::Mysql, "uninstall plugin x").commits_by_itself().is_some());
    assert!(scan(Dialect::Mysql, "truncate t").commits_by_itself().is_some());
}

// MySQL's column of a value: an insert's by its place in the column list, but not one inside
// a function's parentheses; an insert of one row with nothing after it can take more rows, but
// not MySQL's `values row(...)`.
#[test]
fn mysql_values_are_matched_to_columns_only_in_a_tuple() {
    let s = scan(Dialect::Mysql, "insert into t (a, b) values (?, ?)");
    assert_eq!((s.column(1), s.column(2)), (Some("a"), Some("b")));
    assert_eq!(s.values_tuple(2).as_deref(), Some("insert into t (a, b) values "));
    assert_eq!(scan(Dialect::Mysql, "insert into t (a) select coalesce(?, 1)").column(1), None);
    assert_eq!(scan(Dialect::Mysql, "insert into t (a, b) values row(?, ?)").values_tuple(2), None);
    assert_eq!(scan(Dialect::Mysql, "insert into t (a, b) values (?, ?)").values_tuple(3), None);
}

// SQLite's columns kept as floats: a `create table` with no column list (`as select`), and an
// `alter table ... add` of something that is no column (`add (x money)`), have none.
#[test]
fn sqlite_lossy_columns_are_read_only_where_columns_are() {
    let lossy = |sql: &str| scan(Dialect::Sqlite, sql).lossy.iter().map(|(t, c, ty)| format!("{}.{} {}", t, c, ty)).collect::<Vec<_>>();
    assert_eq!(lossy("create table t (m money, d decimal(10, 2), n numeric(5, 2), x numeric, i int)"), ["t.m money", "t.d decimal", "t.n numeric"]);
    assert_eq!(lossy("create table t as select 1.5 as m"), Vec::<String>::new());
    assert_eq!(lossy("create table if not exists u like t"), Vec::<String>::new());
    assert_eq!(lossy("alter table t add (x money)"), Vec::<String>::new());
    assert_eq!(lossy("alter table t add column x money"), ["t.x money"]);
    assert_eq!(lossy("alter table t add y dec"), ["t.y dec"]);
    assert_eq!(lossy("alter table t add z int"), Vec::<String>::new());
}

// PostgreSQL's `U&"..."` names: `\XXXX` is the character, `\\` a backslash, and a `\` before
// anything else is kept.
#[test]
fn postgresql_unicode_names_are_read_as_postgresql_reads_them() {
    let calls = |sql: &str| scan(Dialect::Postgres, sql).calls.iter().map(|c| c.to_string()).collect::<Vec<_>>();
    assert_eq!(calls(r#"select U&"set\005fconfig"('a', 'b', false)"#), ["set_config"]);
    assert!(scan(Dialect::Postgres, r#"select U&"set\005fconfig"('a', 'b', false)"#).changes_session(&[]));
    assert_eq!(calls(r#"select U&"a\\b"(1)"#), [r"a\b"]);
    assert_eq!(calls(r#"select U&"a\zz"(1)"#), [r"a\zz"]);
    assert_eq!(calls(r#"select U&"\+01F600x"(1)"#), ["😀x"]);
}

// `set_config`'s third value as a `?` beside `$1`: not one of the values PostgreSQL is given, so
// what it says cannot be known, and the session is taken to change.
#[test]
fn set_config_with_a_value_that_is_not_given_changes_the_session() {
    use quillo::value::Value;
    assert!(scan(Dialect::Postgres, "select set_config('a', 'b', ?), $1").changes_session(&[Value::Bool(true)]));
    assert!(scan(Dialect::Postgres, "select set_config('a', 'b', ?)").changes_session(&[Value::Bool(false)]));
    assert!(!scan(Dialect::Postgres, "select set_config('a', 'b', ?)").changes_session(&[Value::Bool(true)]));
}

// Values by name: a `?` among names has none, which is said; one map for SQL with no value is
// none.
#[test]
fn values_by_name_and_maps_for_no_values() {
    use quillo::value::{Map, Value};
    let mut m = Map::new();
    m.insert("a".into(), Value::Int(1));
    let one = [Value::map(m)];
    for d in Dialect::ALL {
        assert_eq!(scan(d, "select :a, ?").values(&one).err().unwrap(), "no value for ?2", "{:?}", d);
        assert_eq!(scan(d, "select 1").values(&one).unwrap().len(), 0, "{:?}", d);
        assert_eq!(scan(d, "select :a").values(&one).unwrap().len(), 1, "{:?}", d);
    }
}

// The values PostgreSQL's error is about: those inside the innermost parentheses around it; a
// `)` with no `(` before it closes nothing.
#[test]
fn values_near_an_error_skip_a_close_with_no_open() {
    let sql = "select $1) + ($2, $3) + $4";
    assert_eq!(quillo::sql::values_near(sql, sql.find("$3").unwrap()), [2, 3]);
    assert_eq!(quillo::sql::values_near(sql, 0), [1, 2, 3, 4]);
    assert_eq!(quillo::sql::values_near("select ($1", 8), [1]);
}

// ---- value.rs ---------------------------------------------------------------------------

// `unique` of functions keeps each function once, by what it is; of answers, each, as two are
// never equal.
#[test]
fn unique_keeps_functions_once_and_every_answer() {
    let mut it = Interp::new(".");
    it.load_source("fn f(x) = x\nfn g(x) = x\n", "case.qwe").unwrap();
    assert_eq!(ev(&mut it, "len(unique([f, f, g, f]))"), "2");
    assert_eq!(ev(&mut it, "len(unique([respond(200, \"x\"), respond(200, \"x\"), 1, 1]))"), "3");
    assert_eq!(ev(&mut it, "len(unique([dec(\"1\"), f, 1.0, f, respond(200, \"x\")]))"), "3");
}

// The hasher of keys made already: bytes written go in as a number, the last eight kept.
#[test]
fn the_hasher_of_made_keys_keeps_what_it_is_given() {
    use std::hash::Hasher;
    let mut h = quillo::value::Hashed::default();
    h.write(&[1, 2]);
    assert_eq!(h.finish(), 0x0102);
    h.write(&[3, 4, 5, 6, 7, 8, 9, 10]);
    assert_eq!(h.finish(), 0x030405060708090a);
    h.write_u64(7);
    assert_eq!(h.finish(), 7);
    h.write(&[]);
    assert_eq!(h.finish(), 7);
}

// An error as a key, where a change is made in place: given back, as the long way gives it.
#[test]
fn an_error_as_a_key_in_a_variable_is_given_back_in_place() {
    use quillo::value::{update_inside, Map, Value};
    let mut it = Interp::new(".");
    assert_eq!(ev(&mut it, "let e = int(\"q\")\nlet m = { a: {} }\nm = set(m, \"a\", set(m[\"a\"], e, 1))\nm"), "{ a: err(\"not an int: \\\"q\\\"\") }");
    assert_eq!(ev(&mut it, "let k = \"b\"\nlet m = { a: {} }\nm = set(m, \"a\", set(m[\"a\"], k, 1))\nm"), "{ a: { b: 1 } }");
    let got = ev(&mut it, "let e = int(\"q\")\nlet m = { a: {} }\nm = set(m, e, set(get(m, e, {}), \"x\", 1))\nm");
    assert!(got.starts_with("ERROR: `set` expects a map as argument 1, got error"), "{}", got);
    // In the library: no change made, for the long way to say what is wrong.
    let mut whole = Value::map(Map::new());
    assert!(!update_inside(&mut whole, &Value::err(Value::str("no")), Some(Value::map(Map::new())), vec![Value::str("x"), Value::Int(1)]));
    let mut inner = Map::new();
    inner.insert("a".into(), Value::map(Map::new()));
    let mut whole = Value::map(inner);
    assert!(!update_inside(&mut whole, &Value::str("a"), None, vec![Value::err(Value::str("no")), Value::Int(1)]));
    assert_eq!(whole.repr(), "{ a: {} }");
    assert!(update_inside(&mut whole, &Value::str("a"), None, vec![Value::str("k"), Value::Int(1)]));
    assert_eq!(whole.repr(), "{ a: { k: 1 } }");
}

// ---- i18n.rs ----------------------------------------------------------------------------

// Numeric text with an exponent needs its digits: `1e` and `1e+` are no number.
#[test]
fn an_exponent_without_digits_is_no_number() {
    use quillo::i18n::{plural_of, Operands};
    for bad in ["1e", "1e+", "1E-", "2.5e", "e5", ""] {
        assert!(Operands::of_text(bad).is_none(), "{}", bad);
    }
    assert_eq!(plural_of("en", &Operands::of_text("1e0").unwrap()), "one");
    assert_eq!(plural_of("en", &Operands::of_text("1.0").unwrap()), "other");
    assert_eq!(plural_of("en", &Operands::of_text("1E+0").unwrap()), "one");
}

// ---- error.rs ---------------------------------------------------------------------------

// The files a load read, noted inside another's noting: each is told the files, the outer one
// those of the inner too.
#[test]
fn files_read_inside_another_noting_are_the_outers_too() {
    let d = folder("noting");
    let (a, b) = (d.join("a.qwe"), d.join("b.qwe"));
    std::fs::write(&a, "1").unwrap();
    let ((inner, outer_only), all) = quillo::error::noting_reads(|| {
        let _ = quillo::error::read_source(&a);
        let (_, inner) = quillo::error::noting_reads(|| quillo::error::read_source(&b).is_err());
        (inner, true)
    });
    assert!(outer_only);
    assert_eq!(inner, [b.clone()]);
    assert_eq!(all, [a.clone(), b.clone()]);
    // Alone, nothing is kept after.
    let (_, none) = quillo::error::noting_reads(|| ());
    assert!(none.is_empty());
}

// ---- statics.rs -------------------------------------------------------------------------

// Where the system gives no identity, a file is another by its tag.
#[test]
fn a_file_without_an_identity_is_judged_by_its_tag() {
    let d = folder("tag");
    let p = d.join("f.txt");
    std::fs::write(&p, "one").unwrap();
    let info = quillo::statics::info(&p).unwrap();
    let m = std::fs::metadata(&p).unwrap();
    assert!(!quillo::statics::replaced(None, &info.etag, &p, &m));
    assert!(quillo::statics::replaced(None, "W/\"other\"", &p, &m));
    std::fs::write(&p, "three").unwrap();
    assert!(quillo::statics::replaced(None, &info.etag, &p, &std::fs::metadata(&p).unwrap()));
}

// A file read whole that has fewer bytes than were asked for gives what it has; a folder opened
// as a file gives its error; a range past the end is short, said so.
#[test]
fn a_file_shorter_than_looked_at_is_read_as_it_is() {
    let d = folder("short");
    let p = d.join("f.bin");
    std::fs::write(&p, b"hello").unwrap();
    let f = std::fs::File::open(&p).unwrap();
    assert_eq!(&*quillo::statics::read_whole(&f, 10).unwrap(), b"hello");
    assert_eq!(&*quillo::statics::read_whole(&f, 5).unwrap(), b"hello");
    assert_eq!(&*quillo::statics::read_whole(&f, 0).unwrap(), b"");
    let dir = std::fs::File::open(&d).unwrap();
    assert!(quillo::statics::read_whole(&dir, 4).is_err());
    assert_eq!(quillo::statics::read_range(&p, 1, 3).unwrap(), b"ell");
    assert_eq!(quillo::statics::read_range(&p, 3, 9).unwrap_err().kind(), std::io::ErrorKind::UnexpectedEof);
}

// A folder as the system names it: a link to one pointed elsewhere is the other folder at once,
// and more folders than are kept are each right.
#[cfg(unix)]
#[test]
fn a_folder_is_named_anew_when_its_link_moves() {
    let d = folder("dirs");
    for f in ["rel7", "rel8"] {
        std::fs::create_dir_all(d.join(f)).unwrap();
    }
    std::os::unix::fs::symlink("rel7", d.join("cur")).unwrap();
    let real = |p: &Path| quillo::statics::real_dir(p).unwrap();
    assert!(real(&d.join("cur")).ends_with("rel7"));
    assert!(real(&d.join("cur")).ends_with("rel7"));
    std::fs::remove_file(d.join("cur")).unwrap();
    std::os::unix::fs::symlink("rel8", d.join("cur")).unwrap();
    assert!(real(&d.join("cur")).ends_with("rel8"));
    assert_eq!(quillo::statics::real_dir(&d.join("none")), None);
    let many = d.join("many");
    for i in 0..1100 {
        std::fs::create_dir_all(many.join(i.to_string())).unwrap();
    }
    for i in 0..1100 {
        assert!(real(&many.join(i.to_string())).ends_with(format!("many/{}", i)), "{}", i);
    }
    assert!(real(&many.join("7")).ends_with("many/7"));
}

// ---- builtins.rs: `files`, `file` and the program's own files ----------------------------

/// A file of `n` bytes of `b`, and a moment after it, so its change is no longer too near to
/// trust a look at it (`statics::racy`).
fn written_a_while_ago(p: &Path, n: usize, b: u8) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, vec![b; n]).unwrap();
    std::thread::sleep(Duration::from_millis(120));
}

// A large file `files` serves, asked for with HEAD or a conditional: judged once, and the look
// kept, so the next is answered from it; a 304 when the tag matches, a 412 when `If-Match` does
// not.
#[test]
fn a_large_file_judged_once_is_answered_from_the_look() {
    let (d, mut it) = program("large-head", "route GET /s/*p = files(\"public\", p)\n", &[]);
    written_a_while_ago(&d.join("public/big.bin"), 2 << 20, b'a');
    let r = ask(&mut it, "HEAD", "/s/big.bin", &[], b"");
    assert_eq!(r.status, 200);
    let etag = r.header("etag").unwrap().to_string();
    assert_eq!(ask(&mut it, "HEAD", "/s/big.bin", &[], b"").status, 200);
    assert_eq!(ask(&mut it, "GET", "/s/big.bin", &[("If-None-Match", &etag)], b"").status, 304);
    assert_eq!(ask(&mut it, "HEAD", "/s/big.bin", &[("If-None-Match", &etag)], b"").status, 304);
    assert_eq!(ask(&mut it, "GET", "/s/big.bin", &[("If-Match", "\"other\"")], b"").status, 412);
    // The edge: a file of 1 MiB is small, one byte more is large; both answered the same.
    written_a_while_ago(&d.join("public/edge.bin"), 1 << 20, b'b');
    written_a_while_ago(&d.join("public/over.bin"), (1 << 20) + 1, b'c');
    for f in ["edge.bin", "over.bin"] {
        let r = ask(&mut it, "HEAD", &format!("/s/{}", f), &[], b"");
        assert_eq!(r.status, 200, "{}", f);
        let etag = r.header("etag").unwrap().to_string();
        assert_eq!(ask(&mut it, "GET", &format!("/s/{}", f), &[("If-None-Match", &etag)], b"").status, 304, "{}", f);
    }
}

// A large file whose real place is the journal of a SQLite database, through a link whose own
// name is no journal's: refused when it is opened, for HEAD, a conditional and a range alike.
#[cfg(unix)]
#[test]
fn a_large_journal_behind_a_link_is_refused_however_it_is_asked_for() {
    let (d, mut it) = program("large-wal", "route GET /s/*p = files(\"public\", p)\n", &[]);
    let mut db = b"SQLite format 3\0".to_vec();
    db.resize(4096, 0);
    std::fs::create_dir_all(d.join("data")).unwrap();
    std::fs::write(d.join("data/x.db"), db).unwrap();
    written_a_while_ago(&d.join("data/x.db-wal"), 2 << 20, b'w');
    std::fs::create_dir_all(d.join("public")).unwrap();
    std::os::unix::fs::symlink("../data/x.db-wal", d.join("public/big.bin")).unwrap();
    assert_eq!(ask(&mut it, "HEAD", "/s/big.bin", &[], b"").status, 404);
    assert_eq!(ask(&mut it, "GET", "/s/big.bin", &[("If-None-Match", "*")], b"").status, 404);
    assert_eq!(ask(&mut it, "GET", "/s/big.bin", &[("Range", "bytes=0-99")], b"").status, 404);
    assert_eq!(ask(&mut it, "GET", "/s/big.bin", &[("Range", "bytes=100-199")], b"").status, 404);
    // The same bytes under a name of their own, no database's journal: served.
    std::fs::rename(d.join("data/x.db-wal"), d.join("data/plain.bin")).unwrap();
    std::fs::remove_file(d.join("public/big.bin")).unwrap();
    std::os::unix::fs::symlink("../data/plain.bin", d.join("public/big.bin")).unwrap();
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(ask(&mut it, "HEAD", "/s/big.bin", &[], b"").status, 200);
    let r = ask(&mut it, "GET", "/s/big.bin", &[("Range", "bytes=100-199")], b"");
    assert_eq!((r.status, r.body.len(), r.header("content-range")), (206, 100, Some("bytes 100-199/2097152")));
}

// `files` of a folder named through a link to one that is there, then a folder that is not:
// nothing there is served, a 404.
#[cfg(unix)]
#[test]
fn files_of_a_folder_that_is_not_there_behind_a_link_is_a_miss() {
    let (d, mut it) = program("link-missing", "route GET /m/*p = files(\"link/missing\", p)\nroute GET /r/*p = files(\"link\", p)\n", &[("real/a.txt", b"here")]);
    std::os::unix::fs::symlink("real", d.join("link")).unwrap();
    assert_eq!(ask(&mut it, "GET", "/m/a.txt", &[], b"").status, 404);
    assert_eq!(ask(&mut it, "GET", "/m/", &[], b"").status, 404);
    let r = ask(&mut it, "GET", "/r/a.txt", &[], b"");
    assert_eq!((r.status, r.body.as_slice()), (200, b"here".as_slice()));
}

/// `p` saved anew, as an editor saves: written beside it, then moved over it.
fn saved_anew(p: &Path, text: &str) {
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, text).unwrap();
    std::fs::rename(&tmp, p).unwrap();
}

// The key's earlier versions are kept as its own, eight at most: a hard link that holds the
// first is refused while it is among them, and served once eight later ones have been kept.
#[cfg(unix)]
#[test]
fn a_keys_earlier_versions_are_kept_eight_at_most() {
    let (d, mut it) = program("eight", "route GET /s/*p = files(\"public\", p)\n", &[("key.pem", b"FIRST"), ("public/x.txt", b"x")]);
    let key = d.join("key.pem");
    std::fs::hard_link(&key, d.join("public/copy.pem")).unwrap();
    quillo::builtins::protect(&key, quillo::builtins::OWN_TLS_KEY);
    assert_eq!(ask(&mut it, "GET", "/s/copy.pem", &[], b"").status, 404);
    // Each later version kept by a link of its own, so no file's number is used again for
    // another (ext4 does that) while this runs.
    std::fs::create_dir_all(d.join("kept")).unwrap();
    saved_anew(&key, "v1");
    assert_eq!(ask(&mut it, "GET", "/s/copy.pem", &[], b"").status, 404, "the first, held by the link alone");
    for v in 1..=8 {
        quillo::builtins::protect(&key, quillo::builtins::OWN_TLS_KEY);
        assert_eq!(ask(&mut it, "GET", "/s/copy.pem", &[], b"").status, if v < 8 { 404 } else { 200 }, "after {} more", v);
        if v < 8 {
            std::thread::sleep(Duration::from_millis(2));
            std::fs::hard_link(&key, d.join(format!("kept/v{}", v))).unwrap();
            saved_anew(&key, &format!("v{}", v + 1));
        }
    }
    let r = ask(&mut it, "GET", "/s/copy.pem", &[], b"");
    assert_eq!((r.status, r.body.as_slice()), (200, b"FIRST".as_slice()));
    assert_eq!(ask(&mut it, "GET", "/s/x.txt", &[], b"").status, 200);
}

// The key named by its path, saved anew since: what is at its path now is the key, by its
// identity, when a program would delete it.
#[test]
fn the_key_saved_anew_is_still_the_key() {
    let (d, mut it) = program("named-key", "", &[("tls.pem", b"one"), ("notes.txt", b"n")]);
    let key = d.join("tls.pem");
    quillo::builtins::protect_named(&key, quillo::builtins::OWN_TLS_KEY);
    saved_anew(&key, "two");
    assert_eq!(ev(&mut it, "delete_file(\"tls.pem\")"), "err(\"delete_file: will not delete tls.pem: it is the server's TLS key\")");
    assert_eq!(std::fs::read_to_string(&key).unwrap(), "two");
    assert_eq!(ev(&mut it, "delete_file(\"notes.txt\")"), "null");
    assert!(!d.join("notes.txt").exists());
}

// A program whose folder is given by a path that is not there, relative: a file in it is no file
// of its own, and deleting one not there is nothing.
#[test]
fn a_folder_given_relative_and_not_there_holds_nothing_of_its_own() {
    let rel = format!("no-such-folder-cover-f55-{}", std::process::id());
    let mut it = Interp::new(rel.as_str());
    assert_eq!(ev(&mut it, "delete_file(\"x.txt\")"), "null");
    assert_eq!(ev(&mut it, "delete_file(\"sub/x.txt\")"), "null");
    assert!(!Path::new(&rel).exists());
}

// `every` met as a component is made on the server is the browser's: it takes milliseconds and a
// function, and says so, or the page is the error.
#[test]
fn every_in_a_component_takes_milliseconds_and_a_function() {
    let src = "fn step() = null\ncomponent Tick() {\n    every(100)\n    <p>t</p>\n}\ncomponent Fine() {\n    every(100, step)\n    <p>fine</p>\n}\ncomponent Late() {\n    every(\"soon\", step)\n    <p>late</p>\n}\nroute GET /tick = <html><head><title>t</title></head><body><Tick/></body></html>\nroute GET /fine = <html><head><title>t</title></head><body><Fine/></body></html>\nroute GET /late = <html><head><title>t</title></head><body><Late/></body></html>\n";
    let (_d, mut it) = program("every-component", src, &[]);
    assert_eq!(ask(&mut it, "GET", "/tick", &[], b"").status, 500);
    assert_eq!(ask(&mut it, "GET", "/late", &[], b"").status, 500);
    let r = ask(&mut it, "GET", "/fine", &[], b"");
    assert_eq!(r.status, 200);
    assert!(String::from_utf8_lossy(&r.body).contains("<p>fine</p>"));
}

// ---- interp.rs --------------------------------------------------------------------------

// An answer's headers as `respond` takes them back: a header twice is its last, `set-cookie`
// each, a `null` one before another the other.
#[test]
fn an_answers_headers_read_back_as_respond_takes_them() {
    let mut it = Interp::new(".");
    assert_eq!(
        ev(&mut it, "respond(200, \"x\", { \"X-A\": \"1\", \"x-a\": \"2\", \"set-cookie\": [\"a=1\", \"b=2\", \"c=3\"] }).headers"),
        "{ \"x-a\": \"2\", \"set-cookie\": [\"a=1\", \"b=2\", \"c=3\"] }"
    );
    assert_eq!(ev(&mut it, "respond(200, \"x\", { \"Set-Cookie\": null, \"set-cookie\": [\"a=1\"] }).headers"), "{ \"set-cookie\": \"a=1\" }");
    assert_eq!(ev(&mut it, "respond(200, \"x\", { \"Set-Cookie\": [\"z=1\"], \"set-cookie\": [\"a=1\", \"b=ü\"] }).headers"), "{ \"set-cookie\": [\"z=1\", \"a=1\", \"b=ü\"] }");
    assert_eq!(ev(&mut it, "respond(200, \"x\", { \"set-cookie\": [] }).headers"), "{}");
}

// A select's `value` given by a spread, or as a bare attribute (true), picks its option as a
// written one does.
#[test]
fn a_selects_value_from_a_spread_or_bare_picks_its_option() {
    let src = "let attrs = { value: \"b\", name: \"pick\" }\nroute GET /a = <select value><option value=\"true\">t</option><option value=\"x\">x</option></select>\nroute GET /b = <select {...attrs}><option value=\"a\">a</option><option value=\"b\">b</option></select>\nroute GET /c = <select {...{ value: \"é\" }}><option value=\"é\">é</option><option>o</option></select>\n";
    let (_d, mut it) = program("select", src, &[]);
    let page = |it: &mut Interp, p: &str| String::from_utf8(ask(it, "GET", p, &[], b"").body).unwrap();
    assert_eq!(page(&mut it, "/a"), "<select><option selected value=\"true\">t</option><option value=\"x\">x</option></select>");
    assert_eq!(page(&mut it, "/b"), "<select name=\"pick\"><option value=\"a\">a</option><option selected value=\"b\">b</option></select>");
    assert_eq!(page(&mut it, "/c"), "<select><option selected value=\"é\">é</option><option>o</option></select>");
}

// `floor(a / b)` of two ints in a script, which counts its steps: exact, as anywhere.
#[test]
fn a_scripts_integer_division_is_exact() {
    let mut it = Interp::new(".");
    assert_eq!(ev(&mut it, "run_script(\"floor(7 / 2)\")"), "3");
    assert_eq!(ev(&mut it, "run_script(\"floor(-7 / 2)\")"), "-4");
    assert_eq!(ev(&mut it, "run_script(\"ceil(7 / 2)\")"), "4");
    assert_eq!(ev(&mut it, "run_script(\"floor(9007199254740995 / 2)\")"), "4503599627370497");
    assert_eq!(ev(&mut it, "run_script(\"floor(7.5 / 2)\")"), "3");
}

// A component rendered again, as a child set its state while it started: its live state is the
// second render's, sent with the page.
#[test]
fn a_component_rendered_again_keeps_its_live_state() {
    let src = "let hits = 3\nserver fn load_hits() -> int = hits\ncomponent Child(on_ready) {\n    on_ready(5)\n    <span>child</span>\n}\ncomponent Parent() {\n    state count = 0\n    state h = live(load_hits)\n    <div><p id=\"c\">{count} {h}</p><Child on_ready={(n) => { count = n }}/></div>\n}\nroute GET / = <html><head><title>t</title></head><body><Parent/></body></html>\n";
    assert!(said(src).is_empty(), "{:?}", said(src));
    let (_d, mut it) = program("again-live", src, &[]);
    let page = String::from_utf8(ask(&mut it, "GET", "/", &[], b"").body).unwrap();
    assert!(page.contains("<p id=\"c\">5 3</p>"), "{}", page);
    assert!(page.contains("data-s=\"{&quot;count&quot;:5,&quot;h&quot;:3}\" data-l=\"{&quot;h&quot;:{&quot;f&quot;:&quot;load_hits&quot;"), "{}", page);
}

// ---- db.rs ------------------------------------------------------------------------------

// A value no database can store, inside a list or a map given as a value: refused, naming it.
#[test]
fn what_cannot_be_stored_inside_a_value_is_refused() {
    let (_d, mut it) = program("storable", "db.open(\"t.db\")\ndb.exec(\"create table t (v text)\")\n", &[]);
    for (v, what) in [("[(x) => x]", "fn"), ("{ f: print }", "fn"), ("[[1, respond(200, \"x\")]]", "response")] {
        let got = ev(&mut it, &format!("db.exec(\"insert into t (v) values (?)\", {})", v));
        assert!(got.contains(&format!("value 1 holds a {w}, and a {w} cannot be stored in the database", w = what)), "{}: {}", v, got);
    }
    assert_eq!(ev(&mut it, "db.exec(\"insert into t (v) values (?)\", [1, \"é\", null]).changes"), "1");
    assert_eq!(ev(&mut it, "db.query(\"select v from t\")"), "[{ v: \"[1,\\\"é\\\",null]\" }]");
}

// ---- install.rs -------------------------------------------------------------------------

// `quillo install --help` (or `-h`) is answered with the usage on stdout, and succeeds.
#[test]
fn install_help_is_the_usage_on_stdout() {
    let d = folder("install-help");
    for flag in ["--help", "-h"] {
        let (code, out, err) = quillo(&d, &["install", flag], 30);
        assert_eq!(code, Some(0), "{}", err);
        assert!(out.starts_with("usage:\n  quillo install <name>[@version]..."), "{}", out);
        assert!(err.is_empty(), "{}", err);
    }
    let (code, out, err) = quillo(&d, &["install", "--nope"], 30);
    assert_eq!(code, Some(2));
    assert!(out.is_empty() && err.starts_with("quillo install: ") && err.contains("usage:"), "{}", err);
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
}

// `install::main` given `--help` by a caller of the library (the command answers `--help`
// itself, before it): the usage, and success.
#[test]
fn install_main_takes_help_as_the_command_does() {
    assert_eq!(quillo::install::main(&["--help".to_string()]), 0);
    assert_eq!(quillo::install::main(&["x".to_string(), "-h".to_string()]), 0);
    assert_eq!(quillo::install::main(&["--nope".to_string()]), 2);
}

// ---- jit.rs -----------------------------------------------------------------------------

// A float below an int, and at or below one, in compiled code: exactly, at 2^53 too; and in a
// loop that wrote a list first, an int above a float and at or above one.
#[test]
fn compiled_code_compares_a_float_with_an_int_on_its_right_exactly() {
    let defs = "fn flt(a: float, b: int) -> bool = a < b\nfn fle(a: float, b: int) -> bool = a <= b\n";
    let mut it = Interp::new(".");
    it.load_source(defs, "case.qwe").unwrap();
    let before = quillo::jit::compiled();
    let cases = [
        ("flt(9007199254740992.0, 9007199254740993)", "true"),
        ("flt(9007199254740992.0, 9007199254740992)", "false"),
        ("fle(9007199254740992.0, 9007199254740992)", "true"),
        ("fle(9007199254740992.0, 9007199254740991)", "false"),
        ("flt(9223372036854775808.0, 9223372036854775807)", "false"),
        ("fle(-9223372036854775808.0, -9223372036854775808)", "true"),
        ("flt(1.5, 2)", "true"),
        ("flt(2.5, 2)", "false"),
        ("fle(-0.0, 0)", "true"),
        ("flt(-math.inf, -5)", "true"),
        ("flt(math.nan, 1) ?? \"refused\"", "\"refused\""),
    ];
    for _ in 0..3 {
        for (call, want) in cases {
            assert_eq!(ev(&mut it, call), want, "{}", call);
        }
    }
    assert!(quillo::jit::compiled() > before, "nothing was compiled");
    let loops = quillo::jitloop::compiled();
    let mut it = Interp::new(".");
    it.load_source("let xs = []\nlet c = 0\nlet lim = 9007199254740992.0\nfor i in 9007199254740000..9007199254741000 {\n    xs = xs | push(i)\n    if i > lim { c += 1 }\n    if i >= lim { c += 1000 }\n    if lim <= i { c += 1000000 }\n}\nlet r = [c, len(xs)]\n", "main.qwe").unwrap();
    assert!(quillo::jitloop::compiled() > loops, "the loop was not compiled");
    // ...993 to ...999 are above 2^53, ...992 to ...999 at or above it.
    assert_eq!(ev(&mut it, "r"), "[8008007, 1000]");
}

// ---- client.rs --------------------------------------------------------------------------

/// `body` run as a function on the server and, compiled, in Node: both answers, or None
/// without Node.
fn on_both(defs: &str, body: &str) -> Option<(String, String)> {
    if !Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success()) {
        return None;
    }
    let src = format!("{}\nfn __t() {{\n{}\n}}\n", defs, body);
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let server = match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    };
    let js = quillo::client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let out = support::node(&js).expect("node");
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stderr), js);
    Some((server, String::from_utf8_lossy(&out.stdout).trim().to_string()))
}

// `math`'s other functions of a division (not floor, ceil or trunc, which divide in ints) take
// the float, the same in the browser.
#[test]
fn math_of_a_division_is_the_same_in_the_browser() {
    for (body, want) in [("math.sqrt(9 / 4)", "1.5"), ("math.pow(9 / 4, 2)", "5.0625"), ("math.cbrt(-27 / 8)", "-1.5"), ("math.floor(7 / 2)", "3"), ("math.trunc(-7 / 2)", "-3")] {
        let Some((server, browser)) = on_both("", body) else { return };
        assert_eq!(server, want, "{}", body);
        assert_eq!(browser, server, "{}", body);
    }
}

// Rows of a `for` whose block ends in no element have no key: the bundle keys them by place,
// and the page shows none of them.
#[test]
fn rows_of_a_block_with_no_element_are_keyed_by_place() {
    let src = "component Rows() {\n    state xs = [1, 2]\n    <ul id=\"u\">{for x in xs { let _y = x }}</ul>\n}\ncomponent Keyed() {\n    state xs = [1, 2]\n    <ul id=\"k\">{for x in xs {\n        let y = x * 10\n        <li key={y}>{y}</li>\n    }}</ul>\n}\nroute GET / = <html><head><title>t</title></head><body><Rows/><Keyed/></body></html>\n";
    assert!(said(src).is_empty(), "{:?}", said(src));
    let (_d, mut it) = program("row-keys", src, &[]);
    let page = String::from_utf8(ask(&mut it, "GET", "/", &[], b"").body).unwrap();
    assert!(page.contains("<ul id=\"u\"></ul>") && page.contains("<ul id=\"k\"><li>10</li><li>20</li></ul>"), "{}", page);
    let at = page.find("/_quillo/b/").unwrap_or_else(|| panic!("no bundle in {}", page));
    let url = &page[at..at + page[at..].find('"').unwrap()];
    let js = String::from_utf8(ask(&mut it, "GET", url, &[], b"").body).unwrap();
    let rows: Vec<&str> = js.match_indices("W.each(").map(|(i, _)| &js[i..(i + 80).min(js.len())]).collect();
    assert_eq!(rows.len(), 2, "{}", js);
    assert!(rows.iter().filter(|r| r.contains(".v), null, ")).count() == 1, "{:?}", rows);
}

// ---- a server's sockets: live topics after the address, and a page that reads late -------

/// A page's WebSocket, by hand, its frames as a page masks them.
struct Ws(std::net::TcpStream);

impl Ws {
    fn open(port: u16, path: &str) -> Ws {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        write!(s, "GET {} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", path).unwrap();
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            s.read_exact(&mut b).unwrap();
            head.push(b[0]);
        }
        assert!(head.starts_with(b"HTTP/1.1 101"), "{}", String::from_utf8_lossy(&head));
        Ws(s)
    }

    /// A frame: its first byte (FIN and opcode), its payload, masked or not.
    fn frame(&mut self, first: u8, data: &[u8], masked: bool) {
        let mut f = vec![first];
        let m = if masked { 0x80 } else { 0 };
        if data.len() < 126 {
            f.push(m | data.len() as u8);
        } else if data.len() <= 0xffff {
            f.push(m | 126);
            f.extend_from_slice(&(data.len() as u16).to_be_bytes());
        } else {
            f.push(m | 127);
            f.extend_from_slice(&(data.len() as u64).to_be_bytes());
        }
        let mask = [7, 1, 3, 9];
        if masked {
            f.extend_from_slice(&mask);
            f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        } else {
            f.extend_from_slice(data);
        }
        self.0.write_all(&f).unwrap();
    }

    fn text(&mut self, t: &str) {
        self.frame(0x81, t.as_bytes(), true);
    }

    /// The next text message (pings and pongs passed over); None once the connection ends.
    fn read(&mut self) -> Option<String> {
        loop {
            let mut h = [0u8; 2];
            self.0.read_exact(&mut h).ok()?;
            let mut n = (h[1] & 0x7f) as u64;
            if n == 126 {
                let mut l = [0u8; 2];
                self.0.read_exact(&mut l).ok()?;
                n = u16::from_be_bytes(l) as u64;
            } else if n == 127 {
                let mut l = [0u8; 8];
                self.0.read_exact(&mut l).ok()?;
                n = u64::from_be_bytes(l);
            }
            let mut data = vec![0u8; n as usize];
            self.0.read_exact(&mut data).ok()?;
            match h[0] & 0x0f {
                1 => return Some(String::from_utf8(data).unwrap()),
                8 => return None,
                _ => {}
            }
        }
    }
}

/// A folder with `main.qwe` holding `src`.
fn main_dir(name: &str, src: &str) -> PathBuf {
    let d = folder(name);
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

/// The live data's number now, at `port`.
fn live_seq(port: u16) -> u64 {
    let a = support::send(port, "GET", "/_quillo/live?since=0&t=none", &[], "");
    serde_json::from_str::<serde_json::Value>(&a.body).unwrap_or_else(|e| panic!("{}: {}", e, a.body))["seq"].as_u64().unwrap()
}

// A live page's topics in its socket's first message: short, in two parts, with a topic too
// long to be told of (skipped as it comes) before one that is not; a first message that is not
// text, or not masked as a page masks it, ends the socket.
#[test]
fn a_live_sockets_first_message_takes_any_form_a_page_sends() {
    let d = main_dir("live-ws", "route GET /bump/*t {\n    publish(t)\n    \"ok\"\n}\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let changed = |ws: &mut Ws| -> serde_json::Value { serde_json::from_str::<serde_json::Value>(&ws.read().expect("closed")).unwrap()["changed"].clone() };
    // Short: one frame of fewer than 126 bytes.
    let mut ws = Ws::open(s.port, &format!("/_quillo/live?since={}&tm=1", live_seq(s.port)));
    ws.text("x:1,y:2");
    assert_eq!(support::send(s.port, "GET", "/bump/x:1", &[], "").body, "ok");
    assert_eq!(changed(&mut ws), serde_json::json!(["x:1"]));
    // In two frames, the second its continuation.
    let mut ws = Ws::open(s.port, &format!("/_quillo/live?since={}&tm=1", live_seq(s.port)));
    ws.frame(0x01, b"x:", true);
    ws.frame(0x80, "é2".as_bytes(), true);
    support::send(s.port, "GET", "/bump/x:%C3%A92", &[], "");
    assert_eq!(changed(&mut ws), serde_json::json!(["x:é2"]));
    // A topic past what can be told of, then one that can: the first skipped as it comes.
    let mut ws = Ws::open(s.port, &format!("/_quillo/live?since={}&tm=1", live_seq(s.port)));
    let long = "a".repeat(448 * 3 + 50);
    ws.frame(0x01, long.as_bytes(), true);
    ws.frame(0x80, b"aaaa,y:3", true);
    support::send(s.port, "GET", "/bump/y:3", &[], "");
    assert_eq!(changed(&mut ws), serde_json::json!(["y:3"]));
    // Binary first, or unmasked: the socket ends, nothing told.
    let mut ws = Ws::open(s.port, &format!("/_quillo/live?since={}&tm=1", live_seq(s.port)));
    ws.frame(0x82, b"x:1", true);
    assert_eq!(ws.read(), None);
    let mut ws = Ws::open(s.port, &format!("/_quillo/live?since={}&tm=1", live_seq(s.port)));
    ws.frame(0x81, b"x:1", false);
    assert_eq!(ws.read(), None);
    // The server goes on.
    assert_eq!(support::send(s.port, "GET", "/bump/z", &[], "").body, "ok");
}

// A file `files` serves of between 64 KiB and 1 MiB, asked for first in a running server: read
// off the request's thread, whole, and the same from memory next time.
#[test]
fn a_mid_sized_file_is_read_off_the_requests_thread() {
    let d = main_dir("mid-file", "route GET /s/*p = files(\"public\", p)\n");
    std::fs::create_dir_all(d.join("public")).unwrap();
    for (f, n) in [("mid.bin", 200 << 10), ("edge.bin", (64 << 10) + 1), ("small.bin", 64 << 10)] {
        std::fs::write(d.join("public").join(f), vec![b'm'; n]).unwrap();
    }
    std::thread::sleep(Duration::from_millis(120));
    let s = support::serve(&d.join("main.qwe"), &[]);
    for (f, n) in [("mid.bin", 200 << 10), ("edge.bin", (64 << 10) + 1), ("small.bin", 64 << 10)] {
        for _ in 0..2 {
            let a = support::send(s.port, "GET", &format!("/s/{}", f), &[], "");
            assert_eq!((a.status, a.body.len()), (200, n), "{}", f);
        }
    }
}

// A service `--split` started stops with its app: what comes on its stdin is no reason to stop;
// its end is, and so is a stdin that cannot be read.
#[test]
fn a_service_stops_when_its_stdin_ends_or_fails() {
    let d = main_dir("stop-with-app", "route GET / = \"up\"\n");
    let start = |stdin: Stdio| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .args(["run", "main.qwe", "--port", "0"])
            .current_dir(&d)
            .env(quillo::service::STOP_WITH_APP, "1")
            .stdin(stdin)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        support::stop_with_tests(&child);
        let err = child.stderr.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                if let Some(p) = line.split("listening on http://127.0.0.1:").nth(1).and_then(|p| p.trim().parse::<u16>().ok()) {
                    let _ = tx.send(p);
                }
            }
        });
        (support::Guard(child), rx)
    };
    let ended = |g: &mut support::Guard, secs: u64| eventually(secs, "the service did not stop", || g.0.try_wait().unwrap());
    let (mut g, rx) = start(Stdio::piped());
    let port = rx.recv_timeout(Duration::from_secs(20)).expect("no port");
    let mut stdin = g.0.stdin.take().unwrap();
    stdin.write_all(b"anything at all\n").unwrap();
    stdin.flush().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(support::send(port, "GET", "/", &[], "").body, "up");
    assert!(g.0.try_wait().unwrap().is_none());
    drop(stdin);
    assert!(ended(&mut g, 15).success());
    // A stdin that is a folder: reading it fails, and the service stops as it would at its end.
    let (mut g, _rx) = start(Stdio::from(std::fs::File::open(&d).unwrap()));
    ended(&mut g, 15);
}

// ---- modules.rs -------------------------------------------------------------------------

// A main file that is a link, its target written `./real/main.qwe`, named from its own folder:
// its `use`s are found beside the file it leads to.
#[cfg(unix)]
#[test]
fn a_main_file_linked_with_a_dot_finds_its_files_beside_its_target() {
    let d = folder("dot-link");
    std::fs::create_dir_all(d.join("real")).unwrap();
    std::fs::write(d.join("real/main.qwe"), "use lib.qwe\nprint(two())\n").unwrap();
    std::fs::write(d.join("real/lib.qwe"), "fn two() = 2\n").unwrap();
    std::os::unix::fs::symlink("./real/main.qwe", d.join("link.qwe")).unwrap();
    let (code, out, err) = quillo(&d, &["check", "link.qwe"], 60);
    assert_eq!(code, Some(0), "{}{}", out, err);
    let (code, out, err) = quillo(&d, &["run", "link.qwe"], 60);
    assert_eq!((code, out.as_str()), (Some(0), "2\n"), "{}", err);
    // And through two links, the second written the same way.
    std::os::unix::fs::symlink("./link.qwe", d.join("again.qwe")).unwrap();
    let (code, out, err) = quillo(&d, &["run", "again.qwe"], 60);
    assert_eq!((code, out.as_str()), (Some(0), "2\n"), "{}", err);
}

// ---- native.rs --------------------------------------------------------------------------

// A Rust file's functions, read past what is no attribute (`#!` and no `[`), attributes with a
// raw string or brackets inside, and one left open at the end.
#[test]
fn a_rust_files_functions_are_read_past_any_attribute() {
    let names = |src: &str| quillo::native::parse(src).exports.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
    assert_eq!(names("#!x\npub fn a(n: i64) -> i64 { n }\n"), ["a"]);
    assert_eq!(names("#[doc = r\"x]y\"]\npub fn b() -> i64 { 1 }\npub fn c() -> i64 { 2 }\n"), ["b", "c"]);
    assert_eq!(names("#[doc = r#\"]]\"#]\npub fn d() -> i64 { 1 }\n"), ["d"]);
    assert_eq!(names("#[cfg_attr(test, doc = concat![\"a\", \"]\"])]\npub fn e() -> i64 { 1 }\n"), ["e"]);
    assert_eq!(names("pub fn f() -> i64 { 1 }\n#[cfg(x)"), ["f"]);
    assert_eq!(names("pub fn g() -> i64 { 1 }\n#[cfg(any(a, b)"), ["g"]);
    // A reference inside another type cannot be made from a value: the type to take instead,
    // a slice of anything as a Vec of it.
    let problems = |src: &str| quillo::native::parse(src).problems.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>();
    assert_eq!(
        problems("pub fn h(x: Option<&[Vec<i64>]>) -> i64 { 0 }\n"),
        ["`h` is not exported: a parameter of type `Option<&[Vec<i64>]>` cannot be made from a Quillo value: take `Option<Vec<Vec<i64>>>`"]
    );
    assert_eq!(
        problems("pub fn k(x: Vec<&[[u8; 4]]>, y: Option<&str>) -> i64 { 0 }\n"),
        ["`k` is not exported: a parameter of type `Vec<&[[u8; 4]]>` cannot be made from a Quillo value: take `Vec<Vec<[u8; 4]>>`"]
    );
}

// A plugin's answers: maps of floats, one with a NaN beside another value, read as the floats;
// a slice of lists as its parameter; and the file used whole twice, loaded once.
#[test]
fn a_plugins_maps_of_floats_and_slices_of_lists_cross() {
    if !cargo() {
        return;
    }
    let rust = "//! effects: none\nuse std::collections::HashMap;\npub fn rows2() -> Vec<HashMap<String, f64>> {\n    let mut a = HashMap::new();\n    a.insert(\"a\".to_string(), 1.0);\n    let mut b = HashMap::new();\n    b.insert(\"b\".to_string(), f64::NAN);\n    b.insert(\"c\".to_string(), 2.5);\n    vec![a, b]\n}\npub fn many() -> HashMap<String, f64> { let mut m = HashMap::new(); m.insert(\"a\".to_string(), f64::INFINITY); m.insert(\"b\".to_string(), 1.5); m }\npub fn total(xs: &[Vec<i64>]) -> i64 { xs.iter().flatten().sum() }\n";
    let main = "use n.rs\nuse n.rs\nlet r = rows2()\nprint(r[0], r[1].c, r[1].b == r[1].b, type_of(r[1].b))\nprint(many(), total([[1, 2], [3]]), total([]), total([[]]))\n";
    let d = folder("plugin-maps");
    std::fs::write(d.join("n.rs"), rust).unwrap();
    std::fs::write(d.join("main.qwe"), main).unwrap();
    let (code, out, err) = quillo(&d, &["check", "main.qwe"], 300);
    assert_eq!(code, Some(0), "{}{}", out, err);
    let (code, out, err) = quillo(&d, &["run", "main.qwe"], 300);
    assert_eq!(code, Some(0), "{}", err);
    assert_eq!(out, "{ a: 1.0 } 2.5 false float\n{ a: inf, b: 1.5 } 6 0 0\n");
}

// ---- PostgreSQL and MariaDB ------------------------------------------------------------

use std::sync::{Arc, Mutex, Weak};

/// A PostgreSQL server for these tests, shared by those running at once; None when it is not
/// installed.
fn pg_server() -> Option<Arc<support::Pg>> {
    static SHARED: Mutex<(Weak<support::Pg>, u32)> = Mutex::new((Weak::new(), 0));
    let mut g = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = g.0.upgrade() {
        return Some(s);
    }
    g.1 += 1;
    let s = Arc::new(support::postgres(&format!("cf55pg{}", g.1))?);
    g.0 = Arc::downgrade(&s);
    Some(s)
}

/// The same for MariaDB.
fn maria_server() -> Option<Arc<support::Maria>> {
    static SHARED: Mutex<(Weak<support::Maria>, u32)> = Mutex::new((Weak::new(), 0));
    let mut g = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = g.0.upgrade() {
        return Some(s);
    }
    g.1 += 1;
    let s = Arc::new(support::mariadb(&format!("cf55my{}", g.1))?);
    g.0 = Arc::downgrade(&s);
    Some(s)
}

/// A program on the database at `url`, in a folder of its own.
fn on_db(name: &str, url: &str, more: &str) -> Interp {
    let d = folder(name);
    let mut it = Interp::new(&d);
    let src = format!("db.open(\"{}\")\n{}", url, more);
    it.load_source(&src, "app.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    it
}

// In a running server, requests share PostgreSQL's connections (pipelined): a value with no
// type in the SQL is sent as what it is there too, once worked out and then kept, and as text
// where PostgreSQL takes no other; more such statements than are kept are each right.
#[test]
fn values_with_no_type_are_typed_on_the_shared_connections_too() {
    let Some(pg) = pg_server() else { return };
    let url = pg.database("cf55_lanes");
    let src = format!(
        "db.open(\"{}\")\nfn typed(v) = [v, type_of(v)]\nroute GET /v = typed(db.one(\"select ? as v\", 5).v)\nroute GET /big = typed(db.one(\"select ? as v\", 5000000000).v)\nroute GET /upper = typed(db.one(\"select upper(?) as u\", 12345).u)\nroute GET /many {{\n    let total = 0\n    for i in 0..300 {{\n        total = total + db.one(\"select ? as v, \" + str(i) + \" as i\", 1).v\n    }}\n    total\n}}\nroute GET /text = typed(db.one(\"select ? as v\", \"5\").v)\ndb.exec(\"create table if not exists lt (name text)\")\ndb.exec(\"insert into lt values ('42')\")\nroute GET /twice = [db.one(\"select ? as w\", 6).w, db.one(\"select ? as w\", 6).w]\nroute GET /mixed {{\n    let r = db.one(\"select greatest(?, ?) as g, (select count(*) from lt where name = ?) as c\", 10, 9, 42)\n    [typed(r.g), r.c]\n}}\n",
        url
    );
    let d = main_dir("pg-lanes", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    for _ in 0..2 {
        assert_eq!(support::send(s.port, "GET", "/v", &[], "").body, "[5,\"int\"]");
        assert_eq!(support::send(s.port, "GET", "/big", &[], "").body, "[5000000000,\"int\"]");
        assert_eq!(support::send(s.port, "GET", "/upper", &[], "").body, "[\"12345\",\"str\"]");
        assert_eq!(support::send(s.port, "GET", "/text", &[], "").body, "[\"5\",\"str\"]");
        // One value PostgreSQL takes only as text beside one it takes as an int: the second try
        // sends each as it can.
        assert_eq!(support::send(s.port, "GET", "/mixed", &[], "").body, "[[10,\"int\"],1]");
        // Twice in one request, on one connection: the second as worked out the first time.
        assert_eq!(support::send(s.port, "GET", "/twice", &[], "").body, "[6,6]");
    }
    assert_eq!(support::send(s.port, "GET", "/many", &[], "").body, "300");
    assert_eq!(support::send(s.port, "GET", "/many", &[], "").body, "300");
}

// The same on a request's own connection: more statements with values typed than are kept, each
// right; PostgreSQL's JSON with escapes for control characters read as those characters.
#[test]
fn postgresql_typed_statements_past_what_is_kept_and_json_escapes() {
    let Some(pg) = pg_server() else { return };
    let url = pg.database("cf55_typed");
    let mut it = on_db("pg-typed", &url, "");
    assert_eq!(ev(&mut it, "let t = 0\nfor i in 0..300 {\n    t = t + db.one(\"select ? as v, \" + str(i) + \" as i\", 1).v\n}\nt"), "300");
    assert_eq!(ev(&mut it, "db.one(\"select ? as v, 7 as i\", 2).v"), "2");
    let json = r#"let v = db.one("select '[\"\\b\\f\\r\\n\\t\\/\"]'::jsonb as v, '{\"k\": \"\\b\"}'::json as w")
[len(v.v[0]), v.v[0] == "\u{8}\u{c}\r\n\t/", v.w.k == "\u{8}"]"#;
    assert_eq!(ev(&mut it, json), "[6, true, true]");
}

// More function names looked up than are kept: the names known are let go, and a function of
// the program's that changes the session is still run on a connection of its own, which is
// reset, so the setting reaches no other request.
#[test]
fn postgresql_functions_looked_up_past_what_is_kept_are_still_known() {
    let Some(pg) = pg_server() else { return };
    let url = pg.database("cf55_fns");
    let mut it = on_db("pg-fns", &url, "db.exec(\"create function cf55_setter() returns int language plpgsql as $$ begin perform set_config('app.who', 'leak', false); return 1; end $$\")\n");
    let calls: Vec<String> = (0..4100).map(|i| format!("cf55_none_{}(1)", i)).collect();
    let got = ev(&mut it, &format!("db.one(\"select {}\")", calls.join(", ")));
    assert!(got.contains("does not exist"), "{}", &got[..got.len().min(300)]);
    assert_eq!(ev(&mut it, "db.one(\"select cf55_setter() as x\").x"), "1");
    assert_eq!(ev(&mut it, "db.one(\"select coalesce(current_setting('app.who', true), '') as w\").w"), "\"\"");
}

// A connection PostgreSQL ends under a statement: in a `do` block run alone, in a transaction,
// and in a migration, the error is given back and the connection is let go, not given to the
// next request: what follows runs.
#[test]
fn a_connection_ended_under_a_statement_is_let_go() {
    let Some(pg) = pg_server() else { return };
    let url = pg.database("cf55_killed");
    let d = folder("pg-killed");
    std::fs::create_dir_all(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_end.sql"), "select pg_terminate_backend(pg_backend_pid());\n").unwrap();
    let mut it = Interp::new(&d);
    it.load_source(&format!("db.open(\"{}\")\n", url), "app.qwe").unwrap();
    let ended = ev(&mut it, "db.exec(\"do $$ begin perform pg_terminate_backend(pg_backend_pid()); end $$\")");
    assert!(ended.starts_with("err("), "{}", ended);
    assert_eq!(ev(&mut it, "db.one(\"select 1 as x\").x"), "1");
    let ended = ev(&mut it, "db.transaction(() => db.one(\"select pg_terminate_backend(pg_backend_pid()) as t\"))");
    assert!(ended.starts_with("err(") || ended.starts_with("ERROR"), "{}", ended);
    assert_eq!(ev(&mut it, "db.one(\"select 2 as x\").x"), "2");
    let ended = ev(&mut it, "db.migrate()");
    assert!(ended.contains("migration 001_end.sql failed"), "{}", ended);
    assert_eq!(ev(&mut it, "db.one(\"select 3 as x\").x"), "3");
}

// MariaDB with a replica: more function names looked up than are kept, and more tables' columns
// asked of the server than are kept: each let go, and what follows is right.
#[test]
fn mariadb_names_and_columns_past_what_is_kept() {
    let Some(my) = maria_server() else { return };
    let url = my.url("cf55_kept");
    quillo::my::Pool::open_as(&url, true).unwrap();
    let d = folder("maria-kept");
    let mut it = Interp::new(&d);
    // Nothing written first: a request that wrote reads its own writes from the primary.
    // Two replicas: the first is left out a while once a read fails there.
    it.load_source(&format!("db.open(\"{u}\", {{ replicas: [\"{u}\", \"{u}\"] }})\n", u = url), "app.qwe").unwrap();
    let calls: Vec<String> = (0..4100).map(|i| format!("cf55_none_{}(1)", i)).collect();
    let got = ev(&mut it, &format!("db.query(\"select {}\")", calls.join(", ")));
    assert!(got.starts_with("err("), "{}", &got[..got.len().min(300)]);
    assert_eq!(ev(&mut it, "db.one(\"select upper('é') as u\").u"), "\"É\"");
    assert_eq!(ev(&mut it, "db.exec(\"create table if not exists bits (a bit(8))\").changes"), "0");
    let tables: Vec<String> = (0..4100).map(|i| format!("cf55_t{}", i)).collect();
    let got = ev(&mut it, &format!("db.query(\"select 1 from {} where a = ?\", \"101\")", tables.join(", ")));
    assert!(got.starts_with("err("), "{}", &got[..got.len().min(300)]);
    assert_eq!(ev(&mut it, "db.exec(\"insert into bits (a) values (?)\", \"00000101\").changes"), "1");
    assert_eq!(ev(&mut it, "db.query(\"select a from bits where a = ?\", \"00000101\")"), "[{ a: \"00000101\" }]");
}

// More large files judged than the looks kept: the looks are let go, and each file is answered
// as before (files of 2 MiB that take no room: holes).
#[test]
fn more_large_files_judged_than_are_kept_are_each_answered() {
    let (d, mut it) = program("many-large", "route GET /s/*p = files(\"public\", p)\n", &[]);
    std::fs::create_dir_all(d.join("public")).unwrap();
    for i in 0..4100 {
        std::fs::File::create(d.join(format!("public/{}.bin", i))).unwrap().set_len(2 << 20).unwrap();
    }
    std::thread::sleep(Duration::from_millis(120));
    for round in 0..2 {
        for i in (0..4100).step_by(if round == 0 { 1 } else { 97 }) {
            let r = ask(&mut it, "HEAD", &format!("/s/{}.bin", i), &[], b"");
            assert_eq!((r.status, r.header("content-length")), (200, None), "{}", i);
        }
    }
}

// ---- lsp.rs -----------------------------------------------------------------------------

// A file of a program opened by a path relative to the server's folder, from under a folder of
// its own: checked as part of its program, from the program's main file.
#[test]
fn a_file_opened_by_a_relative_path_is_checked_with_its_program() {
    let d = folder("lsp-relative");
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::write(d.join("main.qwe"), "use lib/util.qwe\nuse lib/pages.qwe\nroute GET / = home()\n").unwrap();
    std::fs::write(d.join("lib/util.qwe"), "fn shout(s: str) -> str = upper(s) + \"!\"\n").unwrap();
    std::fs::write(d.join("lib/pages.qwe"), "import lib/util.qwe use shout\nfn home() = shout(\"hi\")\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("lsp").current_dir(&d).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    support::stop_with_tests(&child);
    let mut stdin = child.stdin.take().unwrap();
    let out = child.stdout.take().unwrap();
    let _g = support::Guard(child);
    let (tx, rx) = std::sync::mpsc::channel::<serde_json::Value>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let mut out = std::io::BufReader::new(out);
        loop {
            let mut len = 0;
            loop {
                let mut line = String::new();
                if out.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                if let Some(v) = line.strip_prefix("Content-Length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut buf = vec![0; len];
            if out.read_exact(&mut buf).is_err() || tx.send(serde_json::from_slice(&buf).unwrap()).is_err() {
                return;
            }
        }
    });
    let mut send = |msg: serde_json::Value| {
        let body = msg.to_string();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
    };
    let uri = "lib/pages.qwe";
    let diags = |rx: &std::sync::mpsc::Receiver<serde_json::Value>| loop {
        let m = rx.recv_timeout(Duration::from_secs(30)).expect("no diagnostics");
        if m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri {
            break m["params"]["diagnostics"].clone();
        }
    };
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "capabilities": {} } }));
    send(serde_json::json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    send(serde_json::json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": { "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": "import lib/util.qwe use shout\nfn home() = shout(\"hi\")\n" } } }));
    assert_eq!(diags(&rx), serde_json::json!([]));
    send(serde_json::json!({ "jsonrpc": "2.0", "method": "textDocument/didChange", "params": { "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": "import lib/util.qwe use shout\nfn home() = shuot(\"hi\")\n" }] } }));
    let d = diags(&rx);
    assert!(d.as_array().is_some_and(|a| a.len() == 1) && d[0]["message"].as_str().unwrap().contains("unknown name `shuot` (did you mean `shout`?)"), "{}", d);
}

// ---- found while covering ----

// Q-809: a plugin's map of one key whose value is NaN or an infinity came back as the glue's own
// map of it (`{ k: { "\u0000f64": "NaN" } }`), where one of two keys was right: every shape now
// gives the float back, inside lists and maps, an f32's too; a map of one finite float, an empty
// one and the text "NaN" are as they were, and a map given in comes back the same.
#[test]
fn a_plugins_map_of_one_float_that_json_has_no_number_for_comes_back_as_the_float() {
    if !cargo() {
        return;
    }
    let rust = r#"//! effects: none
use std::collections::HashMap;
fn one_of(k: &str, x: f64) -> HashMap<String, f64> {
    let mut m = HashMap::new();
    m.insert(k.to_string(), x);
    m
}
pub fn one() -> HashMap<String, f64> { one_of("k", f64::NAN) }
pub fn rows() -> Vec<HashMap<String, f64>> { vec![one(), one_of("a", f64::INFINITY), one_of("b", 1.5), HashMap::new()] }
pub fn nested() -> HashMap<String, HashMap<String, f32>> {
    let mut inner = HashMap::new();
    inner.insert("inner".to_string(), f32::NEG_INFINITY);
    let mut m = HashMap::new();
    m.insert("outer".to_string(), inner);
    m
}
pub fn deep() -> Vec<Vec<HashMap<String, Option<f64>>>> {
    let mut a = HashMap::new();
    a.insert("x".to_string(), Some(-f64::INFINITY));
    let mut b = HashMap::new();
    b.insert("y".to_string(), None);
    vec![vec![a, b], vec![]]
}
pub fn finite() -> HashMap<String, f64> { one_of("k", 2.5) }
pub fn wide() -> HashMap<String, f64> { one_of("ключ ü", f64::NAN) }
pub fn texts() -> HashMap<String, String> { let mut m = HashMap::new(); m.insert("k".to_string(), "NaN".to_string()); m }
pub fn back(m: HashMap<String, f64>) -> HashMap<String, f64> { m }
pub fn backs(v: Vec<HashMap<String, f32>>) -> Vec<HashMap<String, f32>> { v }
"#;
    let main = "use n.rs\nlet a = one()\nprint(a, a.k == a.k, type_of(a.k))\nprint(rows(), nested(), deep())\nprint(finite(), wide(), texts())\nlet nan = 0.0 / 0.0\nprint(back({ k: nan }), back({ k: -1.0 / 0.0 }), back({ k: 0.5 }), back({}), backs([{ s: 1.0 / 0.0 }, {}]))\n";
    let d = folder("plugin-one-key");
    std::fs::write(d.join("n.rs"), rust).unwrap();
    std::fs::write(d.join("main.qwe"), main).unwrap();
    let (code, out, err) = quillo(&d, &["check", "main.qwe"], 300);
    assert_eq!(code, Some(0), "{}{}", out, err);
    let (code, out, err) = quillo(&d, &["run", "main.qwe"], 300);
    assert_eq!(code, Some(0), "{}", err);
    assert_eq!(
        out,
        "{ k: NaN } false float\n\
         [{ k: NaN }, { a: inf }, { b: 1.5 }, {}] { outer: { inner: -inf } } [[{ x: -inf }, { y: null }], []]\n\
         { k: 2.5 } { \"ключ ü\": NaN } { k: \"NaN\" }\n\
         { k: NaN } { k: -inf } { k: 0.5 } {} [{ s: inf }, {}]\n"
    );
}

// Q-792: a function the route's block makes itself, by `fn` or a lambda bound by `let`, is a
// function in the answer as the program's of that name is: check said nothing and the route
// answered 500. A call of it is a value; a `let` of a value after the `fn` hides it, and a `fn`
// after the `let` is the function again; a `match` arm's name is a value.
#[test]
fn a_function_the_routes_block_makes_is_told_in_its_answer() {
    let warn = |line: u32, at: &str, name: &str| format!("{} warning: this route's answer holds a function at `{}` (the function `{}`): JSON has none, so the answer is an internal error (500): leave it out, or call it", line, at, name);
    let src = "fn helper() = 1
route GET /a {
    fn helper() = 2
    { a: helper }
}
route GET /b {
    fn helper() = 2
    { n: helper() }
}
route GET /c {
    let f = (x) => x
    { a: [1, f] }
}
route GET /d {
    let helper = 5
    { a: helper }
}
route GET /e {
    fn g() = 1
    let g = 2
    { a: g }
}
route GET /f {
    let g = 2
    fn g() = 1
    { a: g }
}
route GET /g {
    fn größe() = 3
    if true {
        let x = 1
        { \"ü k\": größe, n: x }
    } else {
        { \"ü k\": x2() }
    }
}
fn x2() = 4
route GET /h/:id {
    fn h() = 1
    match id {
        \"1\" => { a: h() }
        h => { a: h }
    }
}
route GET /i {
    let f = (x) => x * 2
    { a: f(3), b: helper(), c: [helper()] }
}
";
    assert_eq!(said(src), [warn(2, "a", "helper"), warn(10, "a[1]", "f"), warn(23, "a", "g"), warn(28, "[\"ü k\"]", "größe")]);
    // What check tells of is what the route does: an internal error; what it does not, an answer.
    let (_d, mut it) = program("block-fn-answer", src, &[]);
    for (url, status, body) in [
        ("/a", 500, ""),
        ("/c", 500, ""),
        ("/f", 500, ""),
        ("/g", 500, ""),
        ("/b", 200, "{\"n\":2}"),
        ("/d", 200, "{\"a\":5}"),
        ("/e", 200, "{\"a\":2}"),
        ("/h/2", 200, "{\"a\":\"2\"}"),
        ("/i", 200, "{\"a\":6,\"b\":1,\"c\":[1]}"),
    ] {
        let r = ask(&mut it, "GET", url, &[], b"");
        assert_eq!(r.status, status, "{}: {}", url, String::from_utf8_lossy(&r.body));
        if status == 200 {
            assert_eq!(String::from_utf8_lossy(&r.body), body, "{}", url);
        }
    }
}

// The values a PostgreSQL refusal is about: at a function's name (as written, schema-qualified,
// quoted), the values in its parentheses alone; at an operator, those beside it; else as before.
#[test]
fn values_near_a_function_or_an_operator_are_its_own() {
    use quillo::sql::values_near;
    let sql = "select upper($1) as u, $2 as v, pg_catalog.lower($3) as l, \"f\"($4, ($5)) as q, $6 = name";
    assert_eq!(values_near(sql, sql.find("upper").unwrap()), [1]);
    assert_eq!(values_near(sql, sql.find("upper").unwrap() + 2), [1]);
    assert_eq!(values_near(sql, sql.find("pg_catalog").unwrap()), [3]);
    assert_eq!(values_near(sql, sql.find("\"f\"").unwrap()), [4, 5]);
    assert_eq!(values_near(sql, sql.find("= name").unwrap()), [6]);
    // A name with no parentheses after it, an operator with no value beside it: as before.
    assert_eq!(values_near(sql, sql.find("as v").unwrap()), [1, 2, 3, 4, 5, 6]);
    assert_eq!(values_near("select 1 + 2, $1", 9), [1]);
    // An empty call, one never closed, `name.` with nothing after it.
    assert_eq!(values_near("select now(), $1", 7), Vec::<usize>::new());
    assert_eq!(values_near("select f($1, $2", 7), [1, 2]);
    assert_eq!(values_near("select t. ($1), $2", 7), [1, 2]);
}

/// Each database, fresh for `name`: SQLite (a file), and PostgreSQL and MariaDB where they are
/// installed. As (kind, URL), and the servers, held while the test runs.
#[allow(clippy::type_complexity)]
fn every_db(name: &str) -> (Vec<(&'static str, String)>, (Option<Arc<support::Pg>>, Option<Arc<support::Maria>>)) {
    let mut out = vec![("sqlite", folder(&format!("db-{}", name)).join("app.db").to_string_lossy().into_owned())];
    let pg = pg_server();
    if let Some(pg) = &pg {
        out.push(("postgres", pg.database(&format!("cf55_{}", name))));
    }
    let my = maria_server();
    if let Some(my) = &my {
        out.push(("mariadb", my.url(&format!("cf55_{}", name))));
    }
    (out, (pg, my))
}

// Q-731: PostgreSQL's refusal of `upper(?)` with an int is at the function's name, outside its
// parentheses, and every value went as text again: `select upper(?), ? as v` gave v as "5" where
// SQLite and MariaDB give 5. Only the values the refusal is about go as text now: the same
// answers on the three, with ints, floats, text, nulls and decimals, several functions in one
// statement, an operator against a text column, and in a transaction.
#[test]
fn a_value_beside_a_function_that_takes_text_keeps_its_type_everywhere() {
    let (dbs, _held) = every_db("beside_fn");
    for (kind, url) in &dbs {
        let mut it = on_db(&format!("beside-fn-{}", kind), url, "db.exec(\"create table bt (id int, name varchar(20))\")\ndb.exec(\"insert into bt values (1, '42')\")\n");
        for (body, want) in [
            ("db.one(\"select upper(?) as u, ? as v\", 12345, 5)", "{ u: \"12345\", v: 5 }"),
            ("db.one(\"select ? as v, upper(?) as u\", 5, 12345)", "{ v: 5, u: \"12345\" }"),
            ("db.one(\"select upper(?) as u, ? as v\", 2.5, -0.5)", "{ u: \"2.5\", v: -0.5 }"),
            ("db.one(\"select upper(?) as u, ? as v, ? as w\", \"ab\", \"5\", \"ü\")", "{ u: \"AB\", v: \"5\", w: \"ü\" }"),
            ("db.one(\"select upper(?) as u, ? as v\", 12345, null)", "{ u: \"12345\", v: null }"),
            ("db.one(\"select upper(?) as u, ? as v\", null, 5)", "{ u: null, v: 5 }"),
            ("db.one(\"select upper(?) as u, ? as v\", 2.50m, 7)", "{ u: \"2.50\", v: 7 }"),
            ("db.one(\"select upper(?) as u, ? as v\", 2147483648, -2147483649)", "{ u: \"2147483648\", v: -2147483649 }"),
            ("db.one(\"select upper(?) as a, lower(?) as b, ltrim(?) as c, ? as v, ? as w\", 1, 2, 3, 4, 0.25)", "{ a: \"1\", b: \"2\", c: \"3\", v: 4, w: 0.25 }"),
            ("db.one(\"select lower(upper(?)) as u, ? as v\", 9, 5)", "{ u: \"9\", v: 5 }"),
            ("db.one(\"select id, ? as v from bt where name = ?\", 5, 42)", "{ id: 1, v: 5 }"),
            ("db.one(\"select substring('hello', ?, ?) as s, upper(?) as u, ? as v\", 2, 3, 1, 5)", "{ s: \"ell\", u: \"1\", v: 5 }"),
        ] {
            assert_eq!(ev(&mut it, body), want, "{}: {}", kind, body);
            assert_eq!(ev(&mut it, &format!("db.transaction(() => {})", body)), want, "{} in a transaction: {}", kind, body);
        }
    }
}

// The same on the connections a running server's requests share (PostgreSQL's pipelined ones);
// and the number of refused tries stays bounded: five functions that take text are each refused
// once and the value beside them is an int; with more, every value goes as PostgreSQL inferred
// it, after at most six tries.
#[test]
fn a_value_beside_a_function_keeps_its_type_on_shared_connections_in_bounded_tries() {
    let Some(pg) = pg_server() else { return };
    let url = pg.database("cf55_beside_lanes");
    let src = format!(
        "db.open(\"{}\")\nfn typed(v) = [v, type_of(v)]\nroute GET /one = typed(db.one(\"select upper(?) as u, ? as v\", 12345, 5).v)\nroute GET /float = typed(db.one(\"select upper(?) as u, ? as v\", 1, 2.5).v)\nroute GET /three = db.one(\"select upper(?) as a, lower(?) as b, ltrim(?) as c, ? as v\", 1, 2, 3, 4)\n",
        url
    );
    let d = main_dir("pg-beside-lanes", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    for _ in 0..2 {
        assert_eq!(support::send(s.port, "GET", "/one", &[], "").body, "[5,\"int\"]");
        assert_eq!(support::send(s.port, "GET", "/float", &[], "").body, "[2.5,\"float\"]");
        assert_eq!(support::send(s.port, "GET", "/three", &[], "").body, "{\"a\":\"1\",\"b\":\"2\",\"c\":\"3\",\"v\":4}");
    }
    let mut it = on_db("pg-beside-tries", &url, "");
    // Each refusal is logged with its statement, which says which test's it is.
    let refusals = |pg: &support::Pg, mark: &str| pg.log().matches(mark).count();
    let five = "upper(?) as a, lower(?) as b, ltrim(?) as c, rtrim(?) as d, md5(?) as e";
    assert_eq!(ev(&mut it, &format!("db.one(\"select {}, ? as cf55_five\", 1, 2, 3, 4, 5, 6).cf55_five", five)), "6");
    assert_eq!(refusals(&pg, "cf55_five"), 5);
    let got = ev(&mut it, &format!("db.one(\"select {}, upper(?) as f, ? as cf55_six\", 1, 2, 3, 4, 5, 6, 7)", five));
    assert!(got.starts_with("{ a: \"1\", b: \"2\", c: \"3\", d: \"4\", e: \"e4da3b7fbbce2345d7772b0674a318d5\", f: \"6\", cf55_six: "), "{}", got);
    assert_eq!(refusals(&pg, "cf55_six"), 6);
}

// `math` has no `round` (it is Python's math module, whose `round` is a builtin; the guide lists
// `floor ceil trunc`): `math.round(7 / 2)` went as `round(7 / 2)` on the server and gave 4, where
// the browser refused it. Refused on both now, as `math.round(3.5)` and a module in a variable
// are; check says so; the forms the guide gives are exact on both.
#[test]
fn math_round_is_refused_on_both_sides_and_the_guides_forms_are_exact() {
    for body in ["math.round(7 / 2)", "math.round(3.5)", "let m = math\nm.round(7 / 2)", "let x = 5\nx.floor(7 / 2)"] {
        let Some((server, browser)) = on_both("", body) else { return };
        assert!(server.starts_with("ERROR: "), "{}: {}", body, server);
        assert_eq!(browser, server, "{}", body);
    }
    for (body, want) in [
        ("math.floor(7 / 2)", "3"),
        ("math.ceil(7 / 2)", "4"),
        ("math.trunc(-7 / 2)", "-3"),
        ("round(7 / 2)", "4"),
        ("round(-7 / 2)", "-4"),
        ("(7 / 2).round()", "4"),
        ("7 / 2 | round", "4"),
        ("let m = math\nm.floor(9007199254740995 / 2)", "4503599627370497"),
        ("round(9007199254740995 / 2)", "4503599627370498"),
    ] {
        let Some((server, browser)) = on_both("", body) else { return };
        assert_eq!(server, want, "{}", body);
        assert_eq!(browser, server, "{}", body);
    }
    assert_eq!(said("print(math.round(7 / 2))"), ["1: `math` has no function `round`"]);
}
