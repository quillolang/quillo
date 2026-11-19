//! FEEDBACK75.md, the security audit of 0.8.7, slice C (branch feedback75): the checker,
//! `run_script` and the prefetch guard. The checker's types held to a size, a depth and a budget,
//! with their unions made once (Q-1053); a script's steps charged for what a builtin reads, and its
//! time held to 100 ms unless `timeout` says (Q-1055); a script's method names looked up, not kept
//! (Q-1076); `quillo check`'s GET-that-writes warning through `http.stream`, `udp.send`, a channel's
//! messages, a name for a function and SQL a `let` holds (Q-1084); its SQL warning through helpers,
//! `str`, `trim`, `lower`, `replace`, a map's field, a lambda and a joined list (Q-1085); a confined
//! check quoting no `.rs` file's text (Q-1095); a prefetch refused at `http.request`, `http.stream`,
//! `udp.send` and a channel's messages (Q-1062); `quillo.policy` never served (Q-1088); and a
//! warning for a `*` stream's rule that never reads the stream's name (Q-1050). Every server is one
//! this test starts, on this machine.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    support::temp_dir(&format!("feedback75c-{}", name))
}

/// A folder removed when the test ends, passed or not.
struct Gone(PathBuf);

impl Drop for Gone {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// How much longer a debug build takes, for time bounds.
fn slow() -> u32 {
    if cfg!(debug_assertions) {
        10
    } else {
        1
    }
}

/// What a check of `main.qwe` holding `src` in `d` says, as "severity: message" lines, and how long
/// it took; `confined` as `check_program` checks a program's users' code.
fn check_in(d: &Path, src: &str, confined: bool) -> (Vec<String>, Duration) {
    check_timed(d, src, confined, None)
}

/// `check_in`, a confined check given `time` in place of its 2 s.
fn check_timed(d: &Path, src: &str, confined: bool, time: Option<Duration>) -> (Vec<String>, Duration) {
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let opts = quillo::check::Options { confined, time, ..Default::default() };
    let t = Instant::now();
    let a = quillo::check::check_file_opts(&d.join("main.qwe"), HashMap::new(), opts);
    let took = t.elapsed();
    let said = a.diagnostics.iter().map(|x| format!("{}: {}", if x.severity == quillo::check::Severity::Error { "error" } else { "warning" }, x.message)).collect();
    (said, took)
}

fn check(name: &str, src: &str) -> Vec<String> {
    let d = dir(name);
    let _gone = Gone(d.clone());
    check_in(&d, src, false).0
}

/// The warnings among what a check says, without their advice after the first colon past the
/// quoted part, enough to tell one from another.
fn warnings(said: &[String]) -> Vec<String> {
    said.iter().filter(|l| l.starts_with("warning: ")).cloned().collect()
}

/// `src`'s last value, as the interpreter writes it.
fn run(src: &str) -> String {
    let mut it = quillo::Interp::new(".");
    it.load_source(&format!("let out = {}\n", src), "t.qwe").unwrap_or_else(|e| panic!("{}", e));
    quillo::value::lookup(&it.globals, "out").unwrap().repr()
}

/// `src` loaded as a program: its error, if it stops.
fn load_error(src: &str) -> String {
    let mut it = quillo::Interp::new(".");
    match it.load_source(src, "t.qwe") {
        Ok(_) => String::new(),
        Err(e) => e.to_string(),
    }
}

// ---- Q-1053: the checker's types ---------------------------------------------------------------

/// `let xN = [x(N-1), {a: x(N-1)}]`, N lines, then a call that is wrong for it.
fn doubling(n: usize, call: &str) -> String {
    let mut src = String::from("fn f(x: int) { return x }\nfn g(x: list<int>) { return x }\nlet x0 = 1\n");
    for i in 1..=n {
        src.push_str(&format!("let x{} = [x{}, {{a: x{}}}]\n", i, i - 1, i - 1));
    }
    src.push_str(&format!("let r = {}(x{})\n", call, n));
    src
}

/// The report's 699-byte program, sent to a route that checks it with `check_program` on a server
/// held to 64 MB: answered at once with the one error it has, and the server serves on, where it
/// was killed after 41 s (FEEDBACK75.md, Q-1053; branch feedback75).
#[test]
fn the_699_byte_program_is_checked_at_once_by_check_program() {
    let d = dir("bomb");
    let _gone = Gone(d.clone());
    std::fs::write(
        d.join("main.qwe"),
        "route GET / = \"ok\"\nroute POST /check {\n    let found = check_program(\"plugin.qwe\", { \"plugin.qwe\": request.body })\n    return json.stringify(found)\n}\n",
    )
    .unwrap();
    let s = support::serve_with(&d.join("main.qwe"), &["--max-memory", "64MB"]);
    let bomb = doubling(26, "f").replace("fn g(x: list<int>) { return x }\n", "");
    assert!(bomb.len() < 760, "{}", bomb.len());
    let t = Instant::now();
    let a = support::send(s.port, "POST", "/check", &[("content-type", "text/plain"), ("sec-fetch-site", "same-origin")], &bomb);
    assert_eq!(a.status, 200, "{}", a.body);
    assert!(a.body.contains("argument `x` of `f` must be int, got list"), "{}", a.body);
    assert!(t.elapsed() < Duration::from_secs(2 * slow() as u64), "{:?}", t.elapsed());
    assert_eq!(support::send(s.port, "GET", "/", &[], "").body, "ok");
}

/// One line either side of the report's 26, and a line or two: each checked at once, with the
/// same one error, through `quillo check`'s own checks and a confined one (FEEDBACK75.md, Q-1053;
/// branch feedback75).
#[test]
fn types_made_of_two_of_the_one_before_check_at_once() {
    let d = dir("lines");
    let _gone = Gone(d.clone());
    for n in [0, 1, 2, 25, 26, 27, 60] {
        for confined in [false, true] {
            let (said, took) = check_in(&d, &doubling(n, "f"), confined);
            let want = if n == 0 { vec![] } else { vec!["error: argument `x` of `f` must be int, got list".to_string()] };
            assert_eq!(said, want, "{} lines", n);
            assert!(took < Duration::from_secs(slow() as u64), "{} lines: {:?}", n, took);
        }
    }
}

/// A message that writes such a type out, as a list's wrong item does, says its first 64 lists and
/// maps and then their kinds: 2^26 copies were written (FEEDBACK75.md, Q-1053; branch feedback75).
#[test]
fn a_type_written_in_a_message_is_written_to_a_bound() {
    for n in [3, 26] {
        let said = check(&format!("text{}", n), &doubling(n, "g"));
        assert_eq!(said.len(), 1, "{:?}", said);
        assert!(said[0].starts_with("error: argument `x` of `g` must be list<int>, got list<list<"), "{}", said[0]);
        assert!(said[0].len() < 8_000, "{} bytes", said[0].len());
    }
    // A small one is written whole, as it was.
    let said = check("text-small", "fn g(x: list<int>) { return x }\nlet r = g([[1], [2]])\n");
    assert!(said.iter().any(|l| l.contains("list<int>")), "{:?}", said);
}

/// The other shapes the auditor tried: a map of two of the one before, two lists of maps that swap
/// sides joined into one, maps of 50 such fields, and a choice of a value, a list of it or a map
/// of it on each of 60 lines: each checked at once (FEEDBACK75.md, Q-1053; branch feedback75).
#[test]
fn other_types_that_grow_on_each_line_check_at_once() {
    let mut two = String::from("fn f(x: int) { return x }\nlet x0 = 1\n");
    for i in 1..=40 {
        two.push_str(&format!("let x{} = {{ a: x{}, b: x{} }}\n", i, i - 1, i - 1));
    }
    two.push_str("let r = f(x40)\n");
    let mut swap = String::from("fn f(x: list<{a: int, b: int}>) { return x }\nfn pick(c: bool) {\n    let x0 = 1\n    let y0 = \"s\"\n");
    for i in 1..=40 {
        swap.push_str(&format!("    let x{} = {{ a: x{}, b: y{} }}\n    let y{} = {{ a: y{}, b: x{} }}\n", i, i - 1, i - 1, i, i - 1, i - 1));
    }
    swap.push_str("    let z = c ? x40 : y40\n    return f([z])\n}\n");
    let mut wide = String::from("fn f(x: int) { return x }\nlet x0 = 1\n");
    for i in 1..30 {
        let fields: Vec<String> = (0..50).map(|j| format!("f{}: x{}", j, i - 1)).collect();
        wide.push_str(&format!("let x{} = {{ {} }}\n", i, fields.join(", ")));
    }
    wide.push_str("let r = f(x29)\n");
    let mut choice = String::from("fn f(x: int) { return x }\nfn pick(value: bool) {\n    let x0 = 1\n");
    for i in 1..=60 {
        choice.push_str(&format!("    let x{} = value ? x{} : (value ? [x{}] : {{k{}: x{}}})\n", i, i - 1, i - 1, i, i - 1));
    }
    choice.push_str("    return f(x60)\n}\n");
    let d = dir("shapes");
    let _gone = Gone(d.clone());
    for (name, src, want) in [
        ("two", &two, "error: argument `x` of `f` must be int, got map"),
        ("swap", &swap, "error: argument `x` of `f` holds an item that has `a` as "),
        ("wide", &wide, "error: argument `x` of `f` must be int, got map"),
        ("choice", &choice, ""),
    ] {
        for confined in [false, true] {
            let (said, took) = check_in(&d, src, confined);
            assert!(took < Duration::from_secs(slow() as u64), "{}: {:?}", name, took);
            if want.is_empty() {
                assert!(!said.iter().any(|l| l.contains("too complex")), "{}: {:?}", name, said);
            } else {
                assert!(said.iter().any(|l| l.starts_with(want)), "{}: {:?}", name, said);
            }
        }
    }
}

const TOO_COMPLEX: &str = "error: the code is too complex to check: its types grew past what a check keeps, or the check took too long";

/// `let xN = c ? x(N-1) : {aN: 1}`: each line a map of one field more, all of them kept. Past a
/// million cells a confined check ends with one error, in place of what it said; another takes the
/// types past them as unknown and says what 0.8.8 said, as it does well below (FEEDBACK75.md,
/// Q-1053, the second part found in review; branch feedback75).
#[test]
fn a_check_whose_types_pass_its_room_ends_with_one_error() {
    let chain = |n: usize| {
        let mut src = String::from("fn grow(c: bool) {\n    let x0 = { a0: 1 }\n");
        for i in 1..=n {
            src.push_str(&format!("    let x{} = c ? x{} : {{ a{}: 1 }}\n", i, i - 1, i));
        }
        src.push_str(&format!("    return x{}.a0 + \"\"\n}}\n", n));
        src
    };
    let d = dir("cells");
    let _gone = Gone(d.clone());
    let (said, took) = check_in(&d, &chain(1_000), false);
    assert!(!said.iter().any(|l| l.contains("too complex")), "{:?}", said);
    assert!(took < Duration::from_secs(5 * slow() as u64), "{:?}", took);
    let (said, took) = check_in(&d, &chain(1_600), true);
    assert_eq!(said, vec![TOO_COMPLEX.to_string()]);
    assert!(took < Duration::from_secs(5 * slow() as u64), "{:?}", took);
    let (said, took) = check_in(&d, &chain(1_600), false);
    assert!(!said.iter().any(|l| l.contains("too complex")), "{:?}", said);
    assert!(took < Duration::from_secs(5 * slow() as u64), "{:?}", took);
    // The next check on the thread starts afresh.
    assert_eq!(check_in(&d, "let x = 1 + 1\nprint(x)\n", true).0, Vec::<String>::new());
}

/// A list of 1,500 maps each with a field of its own, 40 KB, which 0.8.8 checked in 0.05 s and
/// served: `quillo check` passes it and `quillo run` serves it (FEEDBACK75.md, Q-1053, found in
/// review; branch feedback75).
#[test]
fn a_program_past_the_room_is_checked_and_served() {
    let rows: Vec<String> = (0..1_500).map(|i| format!("{{ k{}: \"v{}\" }}", i, i)).collect();
    let src = format!("let rows = [ {} ]\nroute GET / = str(len(rows))\n", rows.join(", "));
    let d = dir("rows");
    let _gone = Gone(d.clone());
    let (said, took) = check_in(&d, &src, false);
    assert_eq!(said, Vec::<String>::new());
    assert!(took < Duration::from_secs(slow() as u64), "{:?}", took);
    let s = support::serve(&d.join("main.qwe"), &[]);
    assert_eq!(support::send(s.port, "GET", "/", &[], "").body, "1500");
}

/// `n` functions and `n` calls of names a letter or two off theirs.
fn typos(n: usize) -> String {
    let mut src: Vec<String> = (0..n).map(|i| format!("fn helper_number_{}(x) {{ return x }}", i)).collect();
    src.extend((0..n).map(|i| format!("print(helpr_numbr_{}(1))", i)));
    src.join("\n") + "\n"
}

/// A confined check ends at its time (2 s unless given) with one error, read as it walks the code
/// and as it reads functions through their calls; a small one says each name (FEEDBACK75.md,
/// Q-1053; branch feedback75).
#[test]
fn a_confined_check_ends_at_its_time() {
    let d = dir("time");
    let _gone = Gone(d.clone());
    let (said, took) = check_timed(&d, &typos(4_000), true, Some(Duration::from_millis(50)));
    assert_eq!(said, vec![TOO_COMPLEX.to_string()]);
    assert!(took >= Duration::from_millis(50) && took < Duration::from_secs(2 * slow() as u64), "{:?}", took);
    let chain: Vec<String> = std::iter::once("fn s0(c) = sql_name(c)".to_string()).chain((1..30_000).map(|i| format!("fn s{}(c) = s{}(c)", i, i - 1))).collect();
    let (said, _) = check_timed(&d, &(chain.join("\n") + "\n"), true, Some(Duration::from_millis(1)));
    assert_eq!(said, vec![TOO_COMPLEX.to_string()]);
    let (said, _) = check_in(&d, &typos(3), true);
    assert_eq!(said.len(), 3, "{:?}", said);
    assert!(said.iter().all(|l| l.contains("unknown name `helpr_numbr_") && l.contains("(did you mean `helper_number_")), "{:?}", said);
}

/// "Did you mean": 4,000 names a letter off 4,000 functions, which took 11 s, stop being offered
/// where the check's budget is spent, each offered one right; 20 unknown names of 1,000 letters
/// among 1,000 such, which took 31 s, are measured and get none; a name a letter off, or two
/// swapped, is still offered as 0.8.8 offered it, in any case, Unicode too (FEEDBACK75.md, Q-1120,
/// found in review; branch feedback75).
#[test]
fn did_you_mean_is_found_at_once() {
    let d = dir("near");
    let _gone = Gone(d.clone());
    // The work is held to the check's budget, not to a time: the names are offered in order until it
    // is spent, each the right one, then none (well past half of them get theirs). The time bound
    // is only a guard against what took 11 s growing back, far above the budget's 1 to 2 s.
    let (said, took) = check_in(&d, &typos(4_000), false);
    assert_eq!(said.len(), 4_000);
    let offered = said.iter().take_while(|l| l.contains("(did you mean `")).count();
    assert!((1_000..4_000).contains(&offered), "{} offered", offered);
    for (i, l) in said.iter().enumerate() {
        if i < offered {
            assert!(l.ends_with(&format!("unknown name `helpr_numbr_{}` (did you mean `helper_number_{}`?)", i, i)), "{}", l);
        } else {
            assert!(l.ends_with(&format!("unknown name `helpr_numbr_{}`", i)), "past the budget: {}", l);
        }
    }
    assert!(took < Duration::from_secs(20 * slow() as u64), "{:?}", took);
    let mut long: Vec<String> = (0..1_000).map(|i| format!("let {}{:04} = {}", "n".repeat(990), i, i)).collect();
    long.extend((0..20).map(|i| format!("print({}{:04})", "m".repeat(990), i)));
    let (said, took) = check_in(&d, &(long.join("\n") + "\n"), false);
    assert_eq!(said.len(), 20, "{:?}", said.first());
    assert!(said.iter().all(|l| !l.contains("did you mean")), "{:?}", said.first());
    assert!(took < Duration::from_secs(20 * slow() as u64), "{:?}", took);
    for (src, want) in [
        ("let total = 1\nprint(totl)\n", "(did you mean `total`?)"),
        ("let total = 1\nprint(tot)\n", "(did you mean `t`?)"),
        ("let total = 1\nprint(ttoal)\n", "(did you mean `total`?)"),
        ("let total = 1\nprint(TOTL)\n", "(did you mean `total`?)"),
        ("let größe = 1\nprint(grösse)\n", "(did you mean `größe`?)"),
    ] {
        let said = check_in(&d, src, false).0;
        assert!(said.len() == 1 && said[0].ends_with(want), "{}: {:?}", src, said);
    }
    let said = check_in(&d, "let total = 1\nprint(qqqq)\n", false).0;
    assert!(!said[0].contains("did you mean"), "{:?}", said);
}

/// One map written with 100,000 fields, which took 14.9 s, is checked at once, and a key written
/// twice is said once, the last value kept (FEEDBACK75.md, Q-1053, found in review; branch
/// feedback75).
#[test]
fn a_map_of_many_fields_is_checked_at_once() {
    let d = dir("wide-map");
    let _gone = Gone(d.clone());
    let fields: Vec<String> = (0..100_000).map(|i| format!("f{}: {}", i, i)).collect();
    let (said, took) = check_in(&d, &format!("let m = {{ {} }}\nprint(m.f5 + 1)\n", fields.join(", ")), false);
    assert_eq!(said, Vec::<String>::new());
    assert!(took < Duration::from_secs(2 * slow() as u64), "{:?}", took);
    let (said, _) = check_in(&d, "let m = { a: 1, b: 2, a: \"x\" }\nprint(m.a * 2)\n", false);
    assert_eq!(said, vec!["warning: the key `a` is written twice in this map: the last value is the one kept".to_string(), "error: cannot multiply str and int".to_string()]);
}

/// Each function read once and again only when one it calls is found: a chain of 30,000 helpers
/// built SQL, or made it with `sql_name`, took 19.6 s and 23 s; each is checked at once, through
/// `check_program` too, and says what it did (FEEDBACK75.md, Q-1053, found in review; branch
/// feedback75).
#[test]
fn a_chain_of_helpers_is_read_once() {
    let n = if cfg!(debug_assertions) { 6_000 } else { 30_000 };
    let built: Vec<String> = std::iter::once("fn f0(x) { return \"select '\" + x + \"'\" }".to_string()).chain((1..n).map(|i| format!("fn f{}(x) {{ return f{}(x) }}", i, i - 1))).collect();
    let built = format!("db.open(\":memory:\")\n{}\nroute GET /a = db.query(f{}(request.query.x))\nroute GET /b = db.query(f{}(\"users\"))\n", built.join("\n"), n - 1, n - 1);
    let named: Vec<String> = std::iter::once("fn s0(c) = sql_name(c)".to_string()).chain((1..n).map(|i| format!("fn s{}(c) = s{}(c)", i, i - 1))).collect();
    let named = format!("db.open(\":memory:\")\n{}\nroute GET /a = db.query(\"select * from t order by \" + s{}(request.query.c))\n", named.join("\n"), n - 1);
    let d = dir("chain");
    let _gone = Gone(d.clone());
    for confined in [false, true] {
        let (said, took) = check_in(&d, &built, confined);
        assert_eq!(said.len(), 1, "{:?}", said);
        assert!(said[0].starts_with("warning: `db.query` with SQL built from values"), "{:?}", said);
        assert!(took < Duration::from_secs(slow() as u64), "{:?}", took);
        let (said, took) = check_in(&d, &named, confined);
        assert_eq!(said, Vec::<String>::new());
        assert!(took < Duration::from_secs(slow() as u64), "{:?}", took);
    }
}

/// Data tables written out keep their fields known and their mistakes said, as in 0.8.8: 1,000
/// records of 10 fields keyed by id, and 21 languages of 500 keys (FEEDBACK75.md, Q-1053, found in
/// review; branch feedback75). A confined check holds them to its own limits.
#[test]
fn data_tables_keep_their_mistakes_said() {
    let records: Vec<String> = (1..=1_000)
        .map(|i| format!("u{i}: {{ id: {i}, name: \"user{i}\", email: \"u{i}@x.org\", age: {}, admin: false, city: \"c{i}\", zip: \"{}\", score: {i}.5, tags: [\"a\", \"b\"], team: \"t{}\" }}", 20 + i % 50, 10_000 + i, i % 7))
        .collect();
    let users = format!("let users = {{\n    {}\n}}\nprint(users.u5.name * 2)\nprint(users.u7.nosuch)\nprint(users.u9.age + 1)\n", records.join(",\n    "));
    let langs = ["en", "fr", "de", "es", "it", "pt", "nl", "sv", "da", "no", "fi", "pl", "cs", "sk", "hu", "ro", "bg", "el", "tr", "ru", "uk"];
    let tables: Vec<String> = langs.iter().map(|l| format!("{}: {{ {} }}", l, (0..500).map(|j| format!("k{}: \"{} text {}\"", j, l, j)).collect::<Vec<_>>().join(", "))).collect();
    let i18n = format!("let t = {{\n    {}\n}}\nprint(t.en.k10 * 2)\nprint(t.fr.nosuch)\nprint(t.de.k3 - 1)\nprint(t.xx)\nprint(t.ru.k499)\n", tables.join(",\n    "));
    let d = dir("tables");
    let _gone = Gone(d.clone());
    let (said, _) = check_in(&d, &users, false);
    assert_eq!(said, vec!["error: cannot multiply str and int".to_string(), "error: this map has no field `nosuch` (it has admin, age, city, email, id, name, score, tags)".to_string()]);
    let (said, _) = check_in(&d, &i18n, false);
    assert_eq!(
        said,
        vec![
            "error: cannot multiply str and int".to_string(),
            "error: this map has no field `nosuch` (it has k0, k1, k10, k100, k101, k102, k103, k104)".to_string(),
            "error: cannot subtract str and int".to_string(),
            "error: this map has no field `xx` (did you mean `bg`?)".to_string(),
        ]
    );
    let (said, _) = check_in(&d, &users, true);
    assert!(!said.iter().any(|l| l.contains("too complex")), "{:?}", said);
}

/// `p` and `i` written in base 36, as the reviewer's generator names its variables and functions.
fn nm(p: &str, mut i: usize) -> String {
    const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = String::from(p);
    loop {
        out.push(D[i % 36] as char);
        i /= 36;
        if i == 0 {
            return out;
        }
    }
}

/// `quillo run` of a program that checks `file` in `d` with `check_program`, under a heap of 256 MiB
/// that the process may not grow past (mimalloc's own limits), given `secs`: whether it ended well,
/// and what it printed.
fn checked_in_256_mib(d: &Path, file: &str, secs: u64) -> (bool, String) {
    std::fs::write(d.join("host.qwe"), format!("let r = check_program({:?})\nprint(\"diags: \" + str(len(r)))\n", file)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "host.qwe"])
        .current_dir(d)
        .env("MIMALLOC_RESERVE_OS_MEMORY", "256MiB")
        .env("MIMALLOC_ARENA_RESERVE", "256MiB")
        .env("MIMALLOC_DISALLOW_OS_ALLOC", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let mut out = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + Duration::from_secs(secs);
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break Some(st);
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (status.is_some_and(|s| s.success()), reader.join().unwrap())
}

/// A chain of assignments in one function, `v2 = v1` ... `v20000 = v19999`, 277 KB: each value is
/// read about once, where each round read them all again, 24.7 s through `check_program`; it says
/// the 39,998 errors 0.8.8 said, within its time, and a plain check of 10,000 its 19,998
/// (FEEDBACK75.md, Q-1085, found in review; branch feedback75).
#[test]
fn a_chain_of_assignments_is_read_once() {
    let chain = |n: usize| {
        let mut l = vec!["fn f(q) {".to_string(), format!("  {} = q", nm("v", 1))];
        l.extend((2..=n).map(|i| format!("  {} = {}", nm("v", i), nm("v", i - 1))));
        l.push("  return 1".into());
        l.push("}".into());
        l.join("\n") + "\n"
    };
    let d = dir("assigns");
    let _gone = Gone(d.clone());
    let (said, took) = check_in(&d, &chain(20_000), true);
    assert!(took < Duration::from_millis(3_000 * slow() as u64), "{:?}", took);
    if cfg!(debug_assertions) {
        assert!(said.len() == 39_998 || said == vec![TOO_COMPLEX.to_string()], "{}", said.len());
    } else {
        assert_eq!(said.len(), 39_998);
    }
    let (said, took) = check_in(&d, &chain(10_000), false);
    assert_eq!(said.len(), 19_998);
    assert!(said[0].contains("cannot assign to `v1`: it was never declared"), "{}", said[0]);
    assert!(took < Duration::from_secs(3 * slow() as u64), "{:?}", took);
}

/// 200 `match` arms, or `for ... =>` bodies, one inside another around a list of 200,000, in a
/// function whose result may be built SQL: each arm is read as it is, where a copy of it was held
/// at each level, 2.6 GB; `check_program` reads them under a heap of 256 MiB (FEEDBACK75.md,
/// Q-1085, found in review; branch feedback75).
#[test]
fn nested_branches_are_not_copied() {
    let d = dir("nested");
    let _gone = Gone(d.clone());
    let big = format!("[{}]", vec!["1"; 200_000].join(","));
    let (mut m, mut f) = (big.clone(), big);
    for i in 0..200 {
        m = format!("match x {{ _ => {} }}", m);
        f = format!("for a{} in x => {}", i, f);
    }
    std::fs::write(d.join("mat.qwe"), format!("fn f(x) {{\n  let y = {}\n  return str(y)\n}}\nprint(f(true))\n", m)).unwrap();
    std::fs::write(d.join("forc.qwe"), format!("fn f(x) {{\n  let y = {}\n  return str(y)\n}}\nprint(f([1]))\n", f)).unwrap();
    for file in ["mat.qwe", "forc.qwe"] {
        let t = Instant::now();
        let (ended, out) = checked_in_256_mib(&d, file, 30 * slow() as u64);
        assert!(ended, "{}: {}", file, out);
        assert!(out.starts_with("diags: "), "{}: {}", file, out);
        assert!(t.elapsed() < Duration::from_secs(3 * slow() as u64), "{}: {:?}", file, t.elapsed());
    }
}

/// A caller written before its 10,000 helpers (`sql_name`, built SQL, or written after them) is
/// read once more when they are found, not once for each: it took 1.2 to 1.7 s, and a confined
/// check said "too complex"; with a list of 30,000 built texts in it, 5 s and 1.9 GB. Each passes
/// `check_program` at once, under a heap of 256 MiB (FEEDBACK75.md, Q-1085, found in review; branch
/// feedback75).
#[test]
fn a_caller_before_its_helpers_is_read_once() {
    let n = 10_000;
    let calls: Vec<String> = (0..n).map(|i| format!("  {}(x)", nm("t", i))).collect();
    let helpers = |body: &str| (0..n).map(|i| format!("fn {}(x) {{ return {} }}", nm("t", i), body)).collect::<Vec<_>>();
    let caller = |extra: &str| format!("fn f(x) {{\n{}\n{}  return x\n}}", calls.join("\n"), extra);
    let fan_s = format!("{}\n{}\nprint(f(\"a\"))\n", caller(""), helpers("sql_name(x)").join("\n"));
    let fan_b = format!("{}\n{}\nprint(f(\"a\"))\n", caller(""), helpers("\"s\" + x").join("\n"));
    let fan_l = format!("{}\n{}\nprint(f(\"a\"))\n", helpers("sql_name(x)").join("\n"), caller(""));
    let list = format!("  let p = [{}]\n", vec!["\"s\" + x"; 30_000].join(","));
    let fan_p = format!("{}\n{}\nprint(f(\"a\"))\n", caller(&list), helpers("\"s\" + x").join("\n"));
    let d = dir("fan");
    let _gone = Gone(d.clone());
    for (name, src) in [("fan_s", &fan_s), ("fan_b", &fan_b), ("fan_l", &fan_l)] {
        let (said, took) = check_in(&d, src, true);
        assert!(!said.iter().any(|l| l.contains("too complex")), "{}: {:?}", name, said.first());
        assert!(took < Duration::from_secs(slow() as u64), "{}: {:?}", name, took);
        let (_, took) = check_in(&d, src, false);
        assert!(took < Duration::from_secs(slow() as u64), "{}: {:?}", name, took);
    }
    std::fs::write(d.join("fan_p.qwe"), &fan_p).unwrap();
    let t = Instant::now();
    let (ended, out) = checked_in_256_mib(&d, "fan_p.qwe", 30 * slow() as u64);
    assert!(ended && out.starts_with("diags: "), "{}", out);
    assert!(t.elapsed() < Duration::from_secs(2 * slow() as u64), "{:?}", t.elapsed());
}

/// A function that assigns, calls a chain of 100 helpers written after it (the last `sql_name`), and
/// holds 100 `let`s each the block of the next around a list of 300,000: its graph of who reads
/// what is made once, each name given to its innermost `let`, and the helpers are read before it,
/// so it is not read again a round for each; it took 54 s, and `check_program` 8.3 s before "too
/// complex". And a list of 250,000 with a chain of 300 helpers that build SQL, 0.97 s: each name
/// in the list is read once (FEEDBACK75.md, Q-1085, found in review; branch feedback75).
#[test]
fn a_function_is_read_once_through_a_chain_of_helpers() {
    let chain = |n: usize, last: &str| (0..n).map(|i| if i + 1 == n { format!("fn t{}(c) {{ return {} }}", i, last) } else { format!("fn t{}(c) {{ return t{}(c) }}", i, i + 1) }).collect::<Vec<_>>().join("\n");
    let calls = |n: usize, v: &str| (0..n).map(|i| format!("  t{}({})", i, v)).collect::<Vec<_>>().join("\n");
    let mut nest = format!("[{}]", vec!["q"; 300_000].join(","));
    for i in 0..100 {
        nest = format!("{{ let a{} = {}\n a{} }}", i, nest, i);
    }
    let rounds = format!("fn f(q) {{\n  s = q\n{}\n  let top = {}\n  return 1\n}}\n{}\n", calls(100, "q"), nest, chain(100, "sql_name(c)"));
    let list = format!("fn f(x) {{\n{}\n  let p = [{}]\n  return x\n}}\n{}\n", calls(300, "x"), vec!["x"; 250_000].join(","), chain(300, "\"s\" + c"));
    let d = dir("rounds");
    let _gone = Gone(d.clone());
    for (name, src) in [("rounds", &rounds), ("list", &list)] {
        let (said, took) = check_in(&d, src, false);
        assert!(!said.iter().any(|l| l.contains("too complex")), "{}", name);
        assert!(took < Duration::from_secs(slow() as u64), "{}: {:?}", name, took);
        let (said, took) = check_in(&d, src, true);
        assert!(!said.iter().any(|l| l.contains("too complex")) || cfg!(debug_assertions), "{}", name);
        assert!(took < Duration::from_millis(1_000 * slow() as u64), "{}: {:?}", name, took);
    }
}

/// "Did you mean" in a plain check keeps 0.8.8's answers on programs full of mistakes: 3,000
/// unknown names of letters not ASCII, and 3,000 of 67 letters, each among 3,000 functions, get
/// every suggestion, each the right one, within the check's budget (about a second), where 0.8.8
/// took 5 s and 82 s; a confined check keeps its limits (FEEDBACK75.md, Q-1120, found in review;
/// branch feedback75).
#[test]
fn did_you_mean_keeps_its_answers_in_a_plain_check() {
    let n = if cfg!(debug_assertions) { 1_000 } else { 3_000 };
    let uni: Vec<String> = (0..n).map(|i| format!("fn fé_{:05}_é(x) {{ return x }}", i)).chain((0..n).map(|i| format!("print(fè_{:05}_è(1))", i))).collect();
    let x = "x".repeat(60);
    let long: Vec<String> = (0..n).map(|i| format!("fn h_{}{:05}(x) {{ return x }}", x, i)).chain((0..n).map(|i| format!("print(h_{}y{:05}(1))", &x[1..], i))).collect();
    let d = dir("near-plain");
    let _gone = Gone(d.clone());
    for (name, src, first) in [("uni", uni.join("\n") + "\n", "fé_00000_é".to_string()), ("long", long.join("\n") + "\n", format!("h_{}00000", x))] {
        // Every name gets its own suggestion, as on 0.8.8: the work fits in the budget. The time
        // bound is only a guard, far above the budget's 1 to 2 s.
        let (said, took) = check_in(&d, &src, false);
        assert_eq!(said.len(), n, "{}", name);
        assert!(said[0].ends_with(&format!("(did you mean `{}`?)", first)), "{}: {}", name, said[0]);
        for (i, l) in said.iter().enumerate() {
            let want = if name == "uni" { format!("(did you mean `fé_{:05}_é`?)", i) } else { format!("(did you mean `h_{}{:05}`?)", x, i) };
            assert!(l.ends_with(&want), "{}: {}", name, l);
        }
        assert!(took < Duration::from_secs(20 * slow() as u64), "{}: {:?}", name, took);
        let (said, _) = check_in(&d, &src, true);
        assert!(said.len() == n || said == vec![TOO_COMPLEX.to_string()], "{}", name);
        if name == "long" && said.len() == n {
            assert!(!said[0].contains("did you mean"), "{}", said[0]);
        }
    }
}

/// In a confined check, a map written with 10,000 fields knows them, and says a field it lacks;
/// past that size a type's fields are unknown, as `any`'s are, and nothing is said. Another check
/// knows them past it (FEEDBACK75.md, Q-1053; branch feedback75).
#[test]
fn a_type_past_its_size_is_taken_as_unknown() {
    let map = |n: usize| {
        let fields: Vec<String> = (0..n).map(|i| format!("f{}: 1", i)).collect();
        format!("let m = {{ {} }}\nprint(m.titel)\n", fields.join(", "))
    };
    let d = dir("size");
    let _gone = Gone(d.clone());
    let said = check_in(&d, &map(10_000), true).0;
    assert!(said.iter().any(|l| l.contains("no field `titel`")), "{:?}", said);
    let said = check_in(&d, &map(10_001), true).0;
    assert!(!said.iter().any(|l| l.contains("titel")), "{:?}", said);
    let said = check_in(&d, &map(10_001), false).0;
    assert!(said.iter().any(|l| l.contains("no field `titel`")), "{:?}", said);
    // Unicode names are fields like any other.
    let said = check("size-unicode", "let m = { café: 1, naïve: 2 }\nprint(m.cafe)\n");
    assert!(said.iter().any(|l| l.contains("no field `cafe`")), "{:?}", said);
}

/// In a confined check a list nested 15 deep is known to its numbers; one nested past the depth is
/// known as a list, and what it holds as unknown: each said at once. Another check knows one 40
/// deep, as 0.8.8 did (FEEDBACK75.md, Q-1053; branch feedback75).
#[test]
fn a_type_past_its_depth_is_taken_as_unknown() {
    let nested = |n: usize| format!("fn g(x: list<str>) {{ return x }}\nlet r = g([{}1{}])\n", "[".repeat(n - 1), "]".repeat(n - 1));
    let d = dir("depth");
    let _gone = Gone(d.clone());
    let said = check_in(&d, &nested(15), true).0;
    assert_eq!(said.len(), 1, "{:?}", said);
    assert!(said[0].contains(&format!("{}int{}", "list<".repeat(14), ">".repeat(14))), "{}", said[0]);
    let said = check_in(&d, &nested(40), true).0;
    assert_eq!(said.len(), 1, "{:?}", said);
    assert!(!said[0].contains("int"), "{}", said[0]);
    let said = check_in(&d, &nested(40), false).0;
    assert_eq!(said.len(), 1, "{:?}", said);
    assert!(said[0].contains("int"), "{}", said[0]);
}

// ---- Q-1055: a script's steps and time -------------------------------------------------------

/// The report's 143-byte script, sent to a route that runs it: it ends at once with "too many
/// steps", and a request made meanwhile is answered at once, where both waited 13 s
/// (FEEDBACK75.md, Q-1055; branch feedback75).
#[test]
fn the_143_byte_script_ends_at_once() {
    let d = dir("script");
    let _gone = Gone(d.clone());
    std::fs::write(d.join("main.qwe"), "route GET / = \"ok\"\nroute POST /rule {\n    let r = run_script(request.body, { value: 5 })\n    return is_err(r) ? r.message : str(r)\n}\n").unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let src = "let t = join(map(0..300, (i) => \"a\"), \"\")\nlet xs = split(join(map(0..300, (i) => t), \"\"), \"\")\nlet n = 0\nwhile true { n = n + len(unique(xs)) }\n";
    assert_eq!(src.len(), 143);
    let port = s.port;
    let t = Instant::now();
    let rule = std::thread::spawn(move || support::send(port, "POST", "/rule", &[("content-type", "text/plain"), ("sec-fetch-site", "same-origin")], src));
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(support::send(port, "GET", "/", &[], "").body, "ok");
    let a = rule.join().unwrap();
    assert!(a.body.contains("the script took too many steps") || a.body.contains("the script took longer than its 100 ms"), "{}", a.body);
    assert!(t.elapsed() < Duration::from_secs(slow() as u64), "{:?}", t.elapsed());
}

/// A builtin pays a step for each KB it reads: 9,000 rounds of `len` of a text of 1,023 bytes
/// fit in 20,000 steps, of 1,024 they do not, as of 512 two-byte letters; a list's length, which is
/// not read through, costs nothing more (FEEDBACK75.md, Q-1055; branch feedback75).
#[test]
fn a_builtin_pays_for_what_it_reads() {
    // Each given 20 s, so that the steps say, in a debug build too.
    let rounds = |text: &str| run(&format!("run_script(\"let n = 0\\nfor i in 0..9000 {{ n = len(s) }}\\nn\", {{ s: {} }}, {{ timeout: 20000 }})", text));
    assert_eq!(rounds("join(map(0..1023, (i) => \"a\"), \"\")"), "1023");
    assert!(rounds("join(map(0..1024, (i) => \"a\"), \"\")").contains("too many steps"));
    assert!(rounds("join(map(0..512, (i) => \"é\"), \"\")").contains("too many steps"));
    assert_eq!(rounds("join(map(0..511, (i) => \"é\"), \"\") + \"a\""), "512");
    assert_eq!(rounds("\"\""), "0");
    assert_eq!(rounds("range(0, 4096)"), "4096");
    // `==` reads both sides.
    let equal = |a: usize, b: usize| run(&format!("run_script(\"let n = 0\\nfor i in 0..12000 {{ if s == t {{ n = n + 1 }} }}\\nn\", {{ s: join(map(0..{}, (i) => \"a\"), \"\"), t: join(map(0..{}, (i) => \"a\"), \"\") }}, {{ timeout: 20000 }})", a, b));
    assert_eq!(equal(511, 512), "0");
    assert!(equal(512, 512).contains("too many steps"));
    // `unique` of a list it reads through, as the report's script did, in a loop of its own.
    let e = run("run_script(\"let xs = split(join(map(0..2000, (i) => \\\"a\\\"), \\\"\\\"), \\\"\\\")\\nfor i in 0..2000 { unique(xs) }\", {}, { timeout: 20000 })");
    assert!(e.contains("too many steps"), "{}", e);
    // An ordinary rule is as it was.
    assert_eq!(run("run_script(\"value > 3 ? null : `at least 4, got ${value}`\", { value: 5 })"), "null");
}

/// `sort_by` and `group_by` pay for the keys they compare and write out, as a builtin pays for what
/// it reads: 200 rounds of 20 keys of 4 KB do not fit in 20,000 steps, where they cost 21 steps a
/// round; keys of one letter do (FEEDBACK75.md, Q-1055; branch feedback75).
#[test]
fn sort_by_and_group_by_pay_for_their_keys() {
    for f in ["sort_by", "group_by"] {
        let rounds = |key: &str| run(&format!("run_script(\"let xs = 0..20\\nlet n = 0\\nfor i in 0..200 {{ n = len({}(xs, (x) => k)) }}\\nn\", {{ k: {} }}, {{ timeout: 20000 }})", f, key));
        assert!(rounds("join(map(0..4096, (i) => \"a\"), \"\")").contains("too many steps"), "{}", f);
        assert_eq!(rounds("\"a\""), if f == "sort_by" { "20" } else { "1" }, "{}", f);
    }
}

/// `out = push(out, x)` pays for `out` once, as made, so the loops 0.8.8 ran still run, to the
/// same last round: 1,504 pushes and 1,566 `out + [x]`, as on 0.8.8, and one more does not fit
/// (FEEDBACK75.md, Q-1055, found in review; branch feedback75).
#[test]
fn a_push_loop_pays_once() {
    for (how, n, fits) in [("push(out, i)", 1_504, true), ("push(out, i)", 1_505, false), ("out + [i]", 1_566, true), ("out + [i]", 1_567, false)] {
        let r = run(&format!("run_script(\"let out = []\\nfor i in 0..{} {{ out = {} }}\\nlen(out)\", {{}}, {{ timeout: 20000 }})", n, how));
        if fits {
            assert_eq!(r, n.to_string(), "{}", how);
        } else {
            assert!(r.contains("too many steps"), "{}: {}", how, r);
        }
    }
}

/// The rest a list pattern takes is a list made, paid for as one: `let [h, ..t] = huge` of 3
/// million items copied them for nothing, 1.36 s past a 150 ms timeout. Past 1 MB it is refused at
/// once; a small one is as before (FEEDBACK75.md, Q-1055, found in review; branch feedback75).
#[test]
fn a_patterns_rest_is_paid_for() {
    let t = Instant::now();
    let r = run("run_script(\"let n = 0\\nfor i in 0..100 { let [h, ..t] = huge\\nn = n + len(t) }\\nn\", { huge: range(0, 3000000) }, { timeout: 150 })");
    assert!(r.contains("the script made a value larger than 1 MB"), "{}", r);
    assert!(t.elapsed() < Duration::from_millis(1_500 * slow() as u64), "{:?}", t.elapsed());
    assert_eq!(run("run_script(\"let [h, ..t] = xs\\n[h, t]\", { xs: [1, 2, 3] })"), "[1, [2, 3]]");
    assert_eq!(run("run_script(\"let [h, ..t] = xs\\nlen(t)\", { xs: [1] })"), "0");
    let r = run("run_script(\"let n = 0\\nfor i in 0..400 { let [h, ..t] = xs\\nn = n + len(t) }\\nn\", { xs: range(0, 20000) }, { timeout: 20000 })");
    assert!(r.contains("too many steps"), "{}", r);
}

/// A script stops after 100 ms, or the `timeout` it is given: a helper that waits 1 ms, called 1,000
/// times, ends at its time; given 2 s, the script ends by itself (FEEDBACK75.md, Q-1055; branch
/// feedback75).
#[test]
fn a_script_stops_at_its_time() {
    let timed = |opts: &str| {
        let t = Instant::now();
        let r = run(&format!("run_script(\"let n = 0\\nfor i in 0..1000 {{ n = n + wait() }}\\nn\", {{ wait: () => {{\n    sleep(1)\n    return 1\n}} }}{})", opts));
        (r, t.elapsed())
    };
    let (r, took) = timed("");
    assert!(r.contains("the script took longer than its 100 ms"), "{}", r);
    assert!(took >= Duration::from_millis(100) && took < Duration::from_millis(100 * slow() as u64 + 400), "{:?}", took);
    let (r, took) = timed(", { timeout: \"300ms\" }");
    assert!(r.contains("the script took longer than its 300 ms"), "{}", r);
    assert!(took >= Duration::from_millis(300), "{:?}", took);
    let (r, _) = timed(", { timeout: 20000 }");
    assert_eq!(r, "1000");
    // The limits and one either side.
    assert_eq!(run("run_script(\"1 + 1\", {}, { timeout: 1 })"), "2");
    assert_eq!(run("run_script(\"1 + 1\", {}, { timeout: \"60s\" })"), "2");
    assert_eq!(run("run_script(\"1 + 1\", {}, { timeout: null })"), "2");
    for bad in ["0", "60001", "\"fast\"", "\"100\"", "-5", "1.5"] {
        let e = load_error(&format!("run_script(\"1\", {{}}, {{ timeout: {} }})", bad));
        assert!(e.contains("`run_script`: `timeout` is milliseconds from 1 to 60000"), "{}: {}", bad, e);
    }
    let e = load_error("run_script(\"1\", {}, { time: 5 })");
    assert!(e.contains("`run_script`: unknown option `time` (the option is timeout)"), "{}", e);
}

/// A script a helper runs inside another ends by the outer one's time, whatever its own `timeout`
/// says (FEEDBACK75.md, Q-1055; branch feedback75).
#[test]
fn a_script_inside_a_script_ends_by_the_outer_time() {
    let t = Instant::now();
    let r = run(
        "run_script(\"inner()\", { inner: () => run_script(\"let n = 0\\nfor i in 0..1000 { n = n + wait() }\\nn\", { wait: () => {\n    sleep(1)\n    return 1\n} }, { timeout: 5000 }) })",
    );
    assert!(r.contains("the script took longer than its 100 ms"), "{}", r);
    assert!(t.elapsed() < Duration::from_millis(100 * slow() as u64 + 900), "{:?}", t.elapsed());
}

// ---- Q-1076: a script's method names ----------------------------------------------------------

/// A method name a script writes, called on a value that has none, is looked up and not kept for
/// good, and its error says the first 100 characters of it; a name of 100 is said whole
/// (FEEDBACK75.md, Q-1076; branch feedback75).
#[test]
fn a_scripts_method_names_are_not_kept() {
    let name = format!("q1076_{}", "z".repeat(5000));
    for src in [format!("let m = 1\\nm = m.{}()", name), format!("let m = 1\\nm.{}()", name)] {
        let e = run(&format!("run_script(\"{}\", {{}})", src));
        assert!(e.contains("int has no method `q1076_zzz") && e.contains("…`"), "{:.300}", e);
        assert!(e.len() < 400, "{} bytes", e.len());
    }
    assert!(quillo::sym::Sym::known(&name).is_none(), "the name was kept");
    let hundred = format!("q1076b{}", "y".repeat(94));
    let e = run(&format!("run_script(\"let m = 1\\nm = m.{}()\", {{}})", hundred));
    assert!(e.contains(&format!("int has no method `{}`", hundred)), "{}", e);
    assert!(quillo::sym::Sym::known(&hundred).is_none(), "the name was kept");
    // An unknown name, and a name a parse error quotes, are cut too (found in review).
    let e = run(&format!("run_script(\"{}\", {{}})", name));
    assert!(e.contains("unknown name `q1076_zzz") && e.contains("…`") && e.len() < 400, "{:.300}", e);
    let e = run(&format!("run_script(\"let = {}\", {{}})", name));
    assert!(e.len() < 600, "{:.300}", e);
    // A script's own error value is its own, whole.
    let own = run(&format!("len(run_script(\"err(t)\", {{ t: \"{}\" }}).error)", "x".repeat(500)));
    assert_eq!(own, "500");
    // A method a script may call, in place, as before.
    assert_eq!(run("run_script(\"let xs = [1]\\nxs = xs.push(2)\\nxs\", {})"), "[1, 2]");
    assert_eq!(run("run_script(\"let m = { a: 1 }\\nm = m.set(\\\"b\\\", 2)\\nm.b\", {})"), "2");
}

// ---- Q-1084: quillo check's GET-that-writes warning --------------------------------------------

/// The report's routes: a stream sent with POST and a body, `http.request` with its method in a
/// `let`, `udp.send`, a function called through a name a `let` gives it, and SQL a `let` holds are
/// each warned of now, as `db.exec` was; and their forms that do not write are not
/// (FEEDBACK75.md, Q-1084; branch feedback75).
#[test]
fn get_routes_that_write_in_more_ways_are_warned_of() {
    let src = r#"db.open(":memory:")
fn save_visit(u) { db.exec("insert into visits (user) values (?)", u) }
route GET /a {
    http.stream("https://api.example.com/charge", (d) => print(d), { method: "POST", body: { who: session().user } })
    return "ok"
}
route GET /b {
    let m = "POST"
    http.request(m, "https://api.example.com/charge", { who: session().user })
    return "ok"
}
route GET /c {
    udp.send("10.0.0.5", 514, "vote " + str(session().user))
    return "ok"
}
route GET /d {
    let w = save_visit
    w(session().user)
    return "ok"
}
route GET /e {
    let q = "insert into visits (user) values (?)"
    db.query(q, session().user)
    return "ok"
}
route GET /f {
    channel.broadcast("room", str(session().user))
    return "ok"
}
route GET /g {
    http.stream("https://api.example.com/feed", (d) => print(d), { body: "x" + str(session().user) })
    return "ok"
}
"#;
    let said = warnings(&check("writes", src));
    let wants = [
        "`GET /a` writes (`http.stream` with `method: \"POST\"`)",
        "`GET /b` writes (`http.request(\"POST\", ...)`)",
        "`GET /c` writes (`udp.send`)",
        "`GET /d` writes (`db.exec(\"insert into visits (user) values (?)\")`, in `save_visit`)",
        "`GET /e` writes (`db.query(\"insert into visits (user) values (?)\")`, SQL that writes)",
        "`GET /f` writes (`channel.broadcast`)",
        "`GET /g` writes (`http.stream` with a `body`)",
    ];
    for w in wants {
        assert!(said.iter().any(|l| l.contains(w)), "{}: {:#?}", w, said);
    }
    assert_eq!(said.len(), wants.len(), "{:#?}", said);
    let quiet = r#"db.open(":memory:")
fn save_visit(u) { db.exec("insert into visits (user) values (?)", u) }
fn nothing(u) { return u }
route GET /a {
    http.stream("https://api.example.com/feed", (d) => print(d), { method: "GET", headers: { who: str(session().user) } })
    return "ok"
}
route GET /b {
    let m = "GET"
    http.request(m, "https://api.example.com/x", { who: session().user })
    return "ok"
}
route GET /c {
    let m = "POST"
    m = "HEAD"
    http.request(m, "https://api.example.com/x", { who: session().user })
    return "ok"
}
route GET /d {
    let w = save_visit
    w = nothing
    w(session().user)
    return "ok"
}
route GET /e {
    let q = "select * from visits where user = ?"
    return db.query(q, session().user)
}
route GET /f {
    let w = save_visit
    return str(session().user) + str(w != null)
}
"#;
    assert_eq!(warnings(&check("quiet", quiet)), Vec::<String>::new());
}

// ---- Q-1085: quillo check's SQL warning --------------------------------------------------------

/// The auditor's 17 shapes of SQL built from what a visitor sends: each warned of, the nine that
/// were not among them (a helper, `str`, `.replace`, `replace`, a list a variable holds, a map's
/// field, `lower`, a lambda, a template piped to `trim`); `sql_name` and `?` are not
/// (FEEDBACK75.md, Q-1085; branch feedback75).
#[test]
fn sql_built_through_helpers_and_text_functions_is_warned_of() {
    let src = r#"db.open(":memory:")
fn where_name(n) = "name = '" + n + "'"
fn build(n: str) -> str { return "select * from t where name = '" + n + "'" }
route GET /a1 = db.query("select * from t where name = '" + request.query.n + "'")
route GET /a2 = db.query(`select * from t where name = '${request.query.n}'`)
route GET /b1 = db.query(build(request.query.n))
route GET /b2 = db.query("select * from t where " + where_name(request.query.n))
route GET /b3 = db.query(str("select * from t where name = '" + request.query.n + "'"))
route GET /b4 = db.query(trim(`select * from t where name = '${request.query.n}'`))
route GET /b5 = db.query("select * from t where name = '{n}'".replace("{n}", request.query.n))
route GET /b6 = db.query(replace("select * from t where name = '{n}'", "{n}", request.query.n))
route GET /b7 {
    let parts = ["select * from t where name = '", request.query.n, "'"]
    return db.query(join(parts, ""))
}
route GET /b8 {
    let q = { text: "select * from t where name = '" + request.query.n + "'" }
    return db.query(q.text)
}
route GET /b9 {
    let q = "select * from t"
    if request.query.n != null { q = q + " where name = '" + request.query.n + "'" }
    return db.query(q)
}
route GET /b10 {
    let sort = request.query.sort ?? "id"
    return db.query(`select * from t order by ${sort}`)
}
route GET /b11 {
    let col = request.query.col
    return db.query(`select * from t order by ${sql_name(col)}`)
}
route GET /b12 = db.query(lower("SELECT * FROM t WHERE name = '" + request.query.n + "'"))
route GET /b13 = db.query(sql`select * from t where name = '${request.query.n}'`)
route GET /b14 {
    let f = (n) => "select * from t where name = '" + n + "'"
    return db.query(f(request.query.n))
}
route GET /b16 = db.query(["select * from t where name = '", request.query.n, "'"] | join(""))
route GET /b17 = db.query(`select * from t where name = '${request.query.n}'` | trim)
route GET /b18 = db.query(later_built(request.query.n))
route GET /ok1 = db.query("select * from t where name = ?", request.query.n)
route GET /ok2 = db.query(trim("select * from t"))
route GET /ok3 = db.query(safe_sort("id"))
route GET /ok4 = db.query(lower(str("SELECT 1")))
route GET /ok5 = db.query(replace("select * from t order by {c}", "{c}", sql_name(request.query.c)))
fn later_built(n) {
    let q = "select * from t where name = '" + n + "'"
    return q
}
fn safe_sort(c) = `select * from t order by ${sql_name(c)}`
"#;
    let said = warnings(&check("sql", src));
    let d = dir("sql-lines");
    let _gone = Gone(d.clone());
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let a = quillo::check::check_file(&d.join("main.qwe"));
    let mut warned: Vec<u32> = a.diagnostics.iter().filter(|x| x.message.contains("with SQL built from values")).map(|x| x.line).collect();
    warned.sort();
    assert_eq!(warned, vec![4, 5, 6, 7, 8, 9, 10, 11, 14, 18, 23, 27, 33, 34, 37, 39, 40, 41], "{:#?}", said);
}

/// The routes whose SQL warning `src` gets: the route each warned line is in.
fn sql_warned(name: &str, src: &str) -> Vec<String> {
    let d = dir(name);
    let _gone = Gone(d.clone());
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let a = quillo::check::check_file(&d.join("main.qwe"));
    let lines: Vec<&str> = src.lines().collect();
    let mut out: Vec<String> = a
        .diagnostics
        .iter()
        .filter(|x| x.message.contains("with SQL built from values"))
        .filter_map(|x| lines[..x.line as usize].iter().rev().find(|l| l.starts_with("route ")).and_then(|l| l.split_whitespace().nth(2)).map(str::to_string))
        .collect();
    out.sort();
    out
}

/// Only what is built is said: a map's other field and a list's other item, a helper given only
/// SQL-safe values, written out or a list of them, and a `sql_name` helper that adds safe text to
/// its variable, are not; a helper given what the visitor
/// sends is, and one that builds with `s = s + n`, in a loop or a `return` inside an `if` is said
/// too (FEEDBACK75.md, Q-1085, found in review; branch feedback75).
#[test]
fn sql_warnings_say_only_what_is_built() {
    let src = r#"db.open(":memory:")
fn const_q(t) { return "select * from " + t }
fn in_list(ids) {
    let s = "select * from t where id in ("
    for id in ids { s = s + str(id) + "," }
    return s + "0)"
}
fn pick(kind, x) {
    if kind == "a" { return "select * from a where n = '" + x + "'" }
    return "select 1"
}
fn grow(n) {
    let s = "select * from t where n = "
    s += n
    return s
}
fn grown(n) {
    let s = "select * from t where n = "
    s = s + n
    return s
}
fn order_by(col, desc) {
    let c = sql_name(col)
    if desc { c = c + " desc" }
    return c
}
route GET /ok1 = db.query(const_q("users"))
route GET /ok2 {
    let page = { sql: "select * from pages", title: "x" + request.query.t }
    return db.query(page.sql)
}
route GET /ok3 {
    let row = ["select 1", "x" + request.query.t]
    return db.query(row[0])
}
route GET /ok4 = db.query(in_list([1, 2, 3]))
route GET /ok5 = db.query(pick("b", "x"))
route GET /ok6 = db.query("select * from t order by " + order_by(request.query.c, true))
route GET /bad1 = db.query(const_q(request.query.t))
route GET /bad2 {
    let page = { sql: "select * from pages where t = '" + request.query.t + "'", title: "x" }
    return db.query(page.sql)
}
route GET /bad3 {
    let row = ["select 1", "select * from t where n = " + request.query.n]
    return db.query(row[1])
}
route GET /bad4 = db.query(in_list(request.query.ids))
route GET /bad5 = db.query(pick("a", request.query.x))
route GET /bad6 = db.query(grow(request.query.n))
route GET /bad7 = db.query(grown(request.query.n))
"#;
    assert_eq!(sql_warned("sql-only", src), vec!["/bad1", "/bad2", "/bad3", "/bad4", "/bad5", "/bad6", "/bad7"]);
}

// ---- Q-1095: a confined check of a `.rs` file -------------------------------------------------

/// A confined check (`check_program`) of code that uses a `.rs` file says which function cannot
/// cross, and none of the file's text, where it quoted a "type" from its `:` to the next `,`
/// (FEEDBACK75.md, Q-1095; branch feedback75).
#[test]
fn a_confined_check_quotes_no_rust_text() {
    let d = dir("rust");
    let _gone = Gone(d.clone());
    std::fs::write(
        d.join("k2.rs"),
        "pub fn leak(a: DATABASE_URL=postgres://admin:hunter2@db.internal/prod\nSTRIPE_KEY=sk_live_51HxxSECRET\n) -> u8 { 1 }\npub fn r(a: i64) -> AWS_SECRET_ACCESS_KEY_wJalrXUtnFEMI { 1 }\npub fn fine(a: i64) -> i64 { a + 1 }\npub fn café(a: Vec<Point>) -> i64 { 1 }\n",
    )
    .unwrap();
    let src = "use k2.rs\nprint(fine(1))\n";
    let (said, _) = check_in(&d, src, true);
    let text = said.join("\n");
    for secret in ["hunter2", "sk_live", "AWS_SECRET", "postgres", "Point"] {
        assert!(!text.contains(secret), "{}: {}", secret, text);
    }
    assert!(text.contains("`leak` is not exported"), "{}", text);
    assert!(text.contains("`r` is not exported"), "{}", text);
    assert!(!text.contains("`fine`"), "{}", text);
    // `quillo check`, run by the program's own author, says the types as it did.
    let (said, _) = check_in(&d, src, false);
    assert!(said.join("\n").contains("AWS_SECRET_ACCESS_KEY_wJalrXUtnFEMI"), "{:?}", said);
}

// ---- Q-1062: the prefetch guard ---------------------------------------------------------------

/// What a test listener got: each HTTP request line and its body, read within `within`.
fn sink() -> (u16, std::sync::mpsc::Receiver<String>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { break };
            let tx = tx.clone();
            std::thread::spawn(move || {
                s.set_read_timeout(Some(Duration::from_secs(5))).ok();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while let Ok(n) = s.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf);
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let len = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
                        if body.len() >= len {
                            let _ = tx.send(format!("{} {}", head.lines().next().unwrap_or(""), body));
                            let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok");
                            break;
                        }
                    }
                }
            });
        }
    });
    (port, rx)
}

/// A GET that sends `http.request` with POST, `http.stream` with POST or a body, a UDP packet or a
/// channel's message is refused to a prefetch (503, "not prefetched") and sends nothing, as one
/// with `http.post` was; followed, it sends. Its reading forms (GET, HEAD, a stream without a body)
/// are prefetched as before (FEEDBACK75.md, Q-1062; branch feedback75).
#[test]
fn a_prefetch_sends_nothing_out() {
    let (web, got) = sink();
    let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    udp.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    let udp_port = udp.local_addr().unwrap().port();
    let d = dir("prefetch");
    let _gone = Gone(d.clone());
    let src = format!(
        r#"route GET /r1 {{
    http.stream("http://127.0.0.1:{web}/stream-post", (d) => print(d), {{ method: "POST", body: {{ who: "r1" }} }})
    return <p>r1</p>
}}
route GET /r2 {{
    http.request("post", "http://127.0.0.1:{web}/request-post", {{ who: "r2" }})
    return <p>r2</p>
}}
route GET /r3 {{
    udp.send("127.0.0.1", {udp_port}, "vote r3")
    return <p>r3</p>
}}
route GET /r4 {{
    channel.broadcast("room", "r4")
    return <p>r4</p>
}}
route GET /r5 {{
    http.stream("http://127.0.0.1:{web}/stream-body", (d) => print(d), {{ body: "r5" }})
    return <p>r5</p>
}}
route GET /g1 {{
    http.request("GET", "http://127.0.0.1:{web}/request-get")
    return <p>g1</p>
}}
route GET /g2 {{
    http.stream("http://127.0.0.1:{web}/stream-get", (d) => print(d), {{ method: "get" }})
    return <p>g2</p>
}}
route GET /g3 {{
    http.request("HEAD", "http://127.0.0.1:{web}/request-head")
    return <p>g3</p>
}}
"#
    );
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let prefetch = [("x-quillo-prefetch", "1"), ("sec-purpose", "prefetch")];
    for p in ["/r1", "/r2", "/r3", "/r4", "/r5"] {
        let a = support::send(s.port, "GET", p, &prefetch, "");
        assert_eq!(a.status, 503, "{}: {}", p, a.body);
    }
    assert!(got.recv_timeout(Duration::from_millis(300)).is_err(), "a prefetch sent a request");
    let mut b = [0u8; 64];
    assert!(udp.recv(&mut b).is_err(), "a prefetch sent a packet");
    for (p, want) in [("/r1", "POST /stream-post"), ("/r2", "POST /request-post"), ("/r5", "POST /stream-body")] {
        let a = support::send(s.port, "GET", p, &[], "");
        assert_eq!(a.status, 200, "{}: {}", p, a.body);
        let line = got.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(line.starts_with(want), "{}: {}", p, line);
    }
    assert_eq!(support::send(s.port, "GET", "/r3", &[], "").status, 200);
    let n = udp.recv(&mut b).unwrap();
    assert_eq!(&b[..n], b"vote r3");
    assert_eq!(support::send(s.port, "GET", "/r4", &[], "").status, 200);
    for (p, want) in [("/g1", "GET /request-get"), ("/g2", "GET /stream-get"), ("/g3", "HEAD /request-head")] {
        let a = support::send(s.port, "GET", p, &prefetch, "");
        assert_eq!(a.status, 200, "{}: {}", p, a.body);
        let line = got.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(line.starts_with(want), "{}: {}", p, line);
    }
}

// ---- Q-1088: quillo.policy ---------------------------------------------------------------------

/// `quillo serve DIR` reads DIR/quillo.policy and serves the folder's other files, but not the
/// policy, in any case of its name or folder; a file whose name only begins so is served
/// (FEEDBACK75.md, Q-1088; branch feedback75).
#[test]
fn quillo_serve_keeps_the_policy_to_itself() {
    let d = dir("serve");
    let _gone = Gone(d.clone());
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::write(d.join("index.html"), "<p>hi</p>").unwrap();
    std::fs::write(d.join("quillo.policy"), "[env]\nnames = [\"STRIPE_KEY\"]\n").unwrap();
    std::fs::write(d.join("sub/quillo.policy"), "[env]\nnames = [\"OTHER_KEY\"]\n").unwrap();
    std::fs::write(d.join("quillo.policy.txt"), "notes").unwrap();
    std::fs::write(d.join("pölicy.txt"), "ü").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["serve", ".", "--port", "0"])
        .current_dir(&d)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let port = support::port_in_stderr(&mut child, "quillo serve", Instant::now() + Duration::from_secs(20));
    let guard = support::Guard(child);
    assert_eq!(support::send(port, "GET", "/index.html", &[], "").status, 200);
    for p in ["/quillo.policy", "/sub/quillo.policy", "/quillo.policy.", "/quillo.policy%20"] {
        let a = support::send(port, "GET", p, &[], "");
        assert_eq!(a.status, 404, "{}: {}", p, a.body);
        assert!(!a.body.contains("KEY"), "{}: {}", p, a.body);
    }
    if cfg!(target_os = "macos") || cfg!(windows) {
        assert_eq!(support::send(port, "GET", "/QUILLO.POLICY", &[], "").status, 404);
    }
    assert_eq!(support::send(port, "GET", "/quillo.policy.txt", &[], "").body, "notes");
    assert_eq!(support::send(port, "GET", "/p%C3%B6licy.txt", &[], "").body, "ü");
    drop(guard);
}

/// `quillo serve . --policy prod.policy` keeps that policy to itself too, by any case of its name
/// where the file system folds it; another file is served (FEEDBACK75.md, Q-1088, found in review;
/// branch feedback75).
#[test]
fn quillo_serve_keeps_the_policy_it_was_given_to_itself() {
    let d = dir("serve-policy");
    let _gone = Gone(d.clone());
    std::fs::write(d.join("index.html"), "<p>hi</p>").unwrap();
    std::fs::write(d.join("prod.policy"), "[env]\nnames = [\"STRIPE_KEY\"]\n").unwrap();
    std::fs::write(d.join("prod.policy.txt"), "notes").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["serve", ".", "--port", "0", "--policy", "prod.policy"])
        .current_dir(&d)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let port = support::port_in_stderr(&mut child, "quillo serve --policy", Instant::now() + Duration::from_secs(20));
    let guard = support::Guard(child);
    assert_eq!(support::send(port, "GET", "/index.html", &[], "").status, 200);
    for p in ["/prod.policy", "/prod.policy."] {
        let a = support::send(port, "GET", p, &[], "");
        assert_eq!(a.status, 404, "{}: {}", p, a.body);
        assert!(!a.body.contains("STRIPE"), "{}", a.body);
    }
    if cfg!(target_os = "macos") || cfg!(windows) {
        assert_eq!(support::send(port, "GET", "/PROD.POLICY", &[], "").status, 404);
    }
    assert_eq!(support::send(port, "GET", "/prod.policy.txt", &[], "").body, "notes");
    drop(guard);
    // A hard link to it, by another name, made before the server started: the same file
    // (found in review).
    #[cfg(unix)]
    {
        std::fs::hard_link(d.join("prod.policy"), d.join("hard.txt")).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .args(["serve", ".", "--port", "0", "--policy", "prod.policy"])
            .current_dir(&d)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        support::stop_with_tests(&child);
        let port = support::port_in_stderr(&mut child, "quillo serve --policy, linked", Instant::now() + Duration::from_secs(20));
        let _guard = support::Guard(child);
        let a = support::send(port, "GET", "/hard.txt", &[], "");
        assert_eq!(a.status, 404, "{}", a.body);
        assert!(!a.body.contains("STRIPE"), "{}", a.body);
        assert_eq!(support::send(port, "GET", "/prod.policy.txt", &[], "").body, "notes");
    }
}

/// A program's own `files(".", path)` keeps its policy as `quillo serve` does (FEEDBACK75.md,
/// Q-1088; branch feedback75).
#[test]
fn files_keeps_the_policy_to_itself() {
    let d = dir("files");
    let _gone = Gone(d.clone());
    std::fs::write(d.join("main.qwe"), "route GET /*path = files(\".\", path)\n").unwrap();
    std::fs::write(d.join("quillo.policy"), "[env]\nnames = [\"STRIPE_KEY\"]\n").unwrap();
    std::fs::write(d.join("a.txt"), "a").unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    assert_eq!(support::send(s.port, "GET", "/a.txt", &[], "").body, "a");
    assert_eq!(support::send(s.port, "GET", "/quillo.policy", &[], "").status, 404);
}

// ---- Q-1050: a `*` stream's rule that never reads the name ------------------------------------

/// `media.on("call:*", ...)`: a `publish` or `watch` of one parameter, or of two whose second is
/// never read, is warned of, by name; one that reads it, one that lets no one in (`false`), a rule
/// for one stream, and a function the checker cannot read are not (FEEDBACK75.md, Q-1050; branch
/// feedback75).
#[test]
fn a_star_streams_rule_that_never_reads_the_name_is_warned_of() {
    let src = r#"route GET /login ? { user: str } = set_session({ user: user })
fn anyone(peer) { return session().user != null }
fn mine(peer, name) { return name.ends_with(":" + str(session().user)) }
media.on("call:*", { publish: (peer) => session().user != null, watch: (peer, name) => name != "" })
media.on("room:*", { publish: (peer, name) => session().user != null, watch: anyone })
media.on("talk:*", { publish: mine, watch: mine })
media.on("show:*", { publish: (peer) => false, watch: (peer, name) => {
    return false
} })
media.on("lobby", { publish: (peer) => session().user != null, watch: (peer) => true })
"#;
    let said = warnings(&check("star", src));
    let start = |name: &str, rule: &str| format!("warning: `media.on(\"{}\")`'s `{}` never reads which stream is asked for", name, rule);
    let wants = [start("call:*", "publish"), start("room:*", "publish"), start("room:*", "watch")];
    for w in &wants {
        assert!(said.iter().any(|l| l.starts_with(w.as_str())), "{}: {:#?}", w, said);
    }
    assert_eq!(said.len(), wants.len(), "{:#?}", said);
    // The advice the warning gives passes the check.
    let fixed = r#"media.on("call:*", { publish: (peer, name) => name == "call:" + session().user, watch: (peer, name) => name == "call:" + session().user })
"#;
    assert_eq!(warnings(&check("star-fixed", fixed)), Vec::<String>::new());
}
