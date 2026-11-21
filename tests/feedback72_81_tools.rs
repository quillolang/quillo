//! Slice T of FEEDBACK72, 73, 79 and 80, tools and builtins (branch feedback72-81): what a
//! script has (FEEDBACK72.md, Q-1013), `date.format`'s locales said (FEEDBACK73.md, Q-1021), the
//! REPL's null (FEEDBACK73.md, Q-1023), what Quillo writes teaching `onclick` and `value={x}`
//! (FEEDBACK72.md, Q-1030), the docs of `int` and `live` (FEEDBACK79.md, Q-1253; FEEDBACK73.md,
//! Q-1024), and `crypto.verify_password`'s time for any hash (FEEDBACK80.md, Q-1264).

mod support;

use quillo::interp::Ctrl;
use quillo::Interp;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const TOO_BIG: &str = "the script made a value larger than 1 MB";

fn quillo() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_quillo"))
}

/// `cmd` run to its end with `input` on its stdin, killed past two minutes.
fn finish(cmd: &mut Command, input: &[u8]) -> Output {
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let fed = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let out = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        b
    });
    let err = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        b
    });
    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{:?} did not end within two minutes", cmd);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _ = fed.join();
    Output { status, stdout: out.join().unwrap(), stderr: err.join().unwrap() }
}

fn text(o: &Output) -> (String, String) {
    (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}

/// A quillo run of `src` to its end: what it printed, and the most memory it held resident, as
/// `wait4` says it of that process alone, in the system's unit (KB on Linux, bytes on macOS): the
/// tests read a ratio of two. Killed past two minutes.
fn peak(src: &str) -> (String, i64) {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
    // A folder for each call: `temp_dir` gives one name a folder for each process, emptied
    // first, and the tests run at once.
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let d = support::temp_dir(&format!("f7281-peak-{}", NEXT.fetch_add(1, Relaxed)));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let mut child = Command::new(quillo()).arg(d.join("main.qwe")).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let pid = child.id() as libc::pid_t;
    let mut out = child.stdout.take().unwrap();
    let read = std::thread::spawn(move || {
        let mut b = String::new();
        let _ = out.read_to_string(&mut b);
        b
    });
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let watch = {
        let done = done.clone();
        std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(120);
            while !done.load(Relaxed) {
                if Instant::now() > end {
                    // SAFETY: a signal to the child this test started, not yet waited for.
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        })
    };
    let mut status = 0;
    // SAFETY: rusage is plain data, written by wait4.
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    let got = loop {
        // SAFETY: waits for this test's own child, with valid pointers.
        let r = unsafe { libc::wait4(pid, &mut status, 0, &mut ru) };
        if r != -1 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            break r;
        }
    };
    done.store(true, Relaxed);
    let _ = watch.join();
    assert_eq!(got, pid, "{}", std::io::Error::last_os_error());
    let _ = std::fs::remove_dir_all(&d);
    (read.join().unwrap(), ru.ru_maxrss as i64)
}

/// A program loaded in this process.
fn loaded(src: &str) -> Interp {
    let mut it = Interp::new(".");
    it.load_source(src, "case.qwe").unwrap_or_else(|e| panic!("{}\n--- source ---\n{}", e, src));
    it
}

/// What `name` gives, called with `args`, as the REPL writes it.
fn call(it: &mut Interp, name: &str, args: Vec<quillo::Value>) -> String {
    let f = quillo::value::lookup(&it.globals, name).unwrap();
    match it.call(&f, args, 0) {
        Ok(v) | Err(Ctrl::Return(v)) => v.repr(),
        Err(Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(Ctrl::Reply(_)) => "REPLY".into(),
    }
}

/// What `body`, a function's body, gives.
fn eval(body: &str) -> String {
    call(&mut loaded(&format!("fn __t() {{\n{}\n}}\n", body)), "__t", vec![])
}

/// What a script gives, run with `vars` (Quillo's text for a map) for up to a minute: the
/// limits of a debug build's clock are not what these tests are about.
fn script(src: &str, vars: &str) -> String {
    eval(&format!("return run_script({:?}, {}, {{ timeout: 60000 }})", src, vars))
}

/// The error value a script gives, `line 1: ` and `msg`.
fn refused(msg: &str) -> String {
    format!("err({:?})", format!("line 1: {}", msg))
}

// ---- Q-1013: run_script's modules ------------------------------------------------------------

// Q-1013: a script had the builtins of `SAFE` and `date`, so `math.sqrt`, `stats.mean`, `re.test`
// and `format_number` were each an unknown name in a spreadsheet's formula.
#[test]
fn a_script_has_math_stats_re_bit_format_number_and_dec() {
    for (src, want) in [
        ("math.sqrt(16.0)", "4.0"),
        ("math.pi > 3.14 && math.pi < 3.15", "true"),
        ("math.floor(-2.5)", "-3"),
        ("stats.mean([1, 2, 3])", "2.0"),
        ("stats.median(xs)", "2"),
        ("stats.stdev([2, 4, 4, 4, 5, 5, 7, 9]) > 2.0", "true"),
        ("re.test(\"abc\", \"b\")", "true"),
        ("re.find_all(\"a1b22c333\", \"\\\\d+\") | map((m) => m.text)", "[\"1\", \"22\", \"333\"]"),
        ("re.replace(\"naïve café ☕\", \"é\", \"e\")", "\"naïve cafe ☕\""),
        ("re.split(\"a,b;c\", \"[,;]\")", "[\"a\", \"b\", \"c\"]"),
        ("re.valid(\"(\")", "false"),
        ("bit.and(6, 3)", "2"),
        ("bit.shl(1, 4)", "16"),
        ("dec(\"1.10\") + dec(\"2.20\")", "3.30m"),
        ("format_number(1234.5, \"en-US\", 2)", "\"1,234.50\""),
        ("format_number(1234.5, \"de-DE\", 1)", "\"1.234,5\""),
        ("format_number(-0.5, \"fr-FR\", 1)", "\"-0,5\""),
        // The forms a script had, still there.
        ("sqrt(16.0)", "4.0"),
        ("date.add_days(\"2026-11-17\", 1)", "\"2026-11-18\""),
        ("upper(\"é\")", "\"É\""),
        // Empty and one item.
        ("re.find_all(\"\", \"a\")", "[]"),
        ("stats.mean([7])", "7.0"),
        ("math.fsum([])", "0.0"),
    ] {
        assert_eq!(script(src, "{ xs: [3, 1, 2] }"), want, "{}", src);
    }
    // What a module refuses is the script's error value, with its line.
    assert!(script("stats.mean([])", "{}").starts_with("err(\"line 1: "), "{}", script("stats.mean([])", "{}"));
    assert!(script("1\nmath.sqrt(-1)", "{}").starts_with("err(\"line 2: "), "{}", script("1\nmath.sqrt(-1)", "{}"));
}

// Q-1013: a script reads only what it is given: `format_number` with no locale is en-US, where the
// program's call takes the visitor's.
#[test]
fn a_scripts_format_number_takes_no_locale_but_its_own() {
    let mut it = loaded(
        "translations({ en: { hi: \"hi\" }, de: { hi: \"hallo\" } })\n\
         fn both() = with_locale(\"de\", () => [format_number(1234.5), run_script(\"format_number(1234.5)\", {}), run_script(\"format_number(1234.5, null, 2)\", {}), run_script(\"format_number(1234.5, l)\", { l: \"de-DE\" })])\n",
    );
    assert_eq!(call(&mut it, "both", vec![]), "[\"1.234,5\", \"1,234.5\", \"1,234.50\", \"1.234,5\"]");
    assert_eq!(script("format_number()", "{}"), refused("`format_number` expects a number as argument 1, got nothing"));
}

// Q-1013: what a script does not have was "unknown name `range`", which read as a typo; it says
// what it is, and why.
#[test]
fn what_a_script_does_not_have_is_said_as_left_out() {
    let huge = "it can make a huge value from a small one";
    let outside = "a script has only the builtins the guide lists for `run_script`, which reach nothing outside it: no database, files, network, clock, randomness, sessions or replies";
    for (src, name, why) in [
        ("range(3)", "range", huge),
        ("repeat(\"a\", 3)", "repeat", huge),
        ("vec.zeros(3)", "vec", huge),
        ("join_on([], [], \"id\")", "join_on", huge),
        ("markdown(\"*x*\")", "markdown", "a script gives values, not markup"),
        ("now()", "now", outside),
        ("db.query(\"select 1\")", "db", outside),
        ("random()", "random", outside),
        ("json.parse(\"1\")", "json", outside),
    ] {
        assert_eq!(script(src, "{}"), refused(&format!("`{}` is not available in a script: {}", name, why)), "{}", src);
    }
    // A name of the script's own, as it wrote it, and one of Quillo's notes, are as they were.
    assert_eq!(script("totl + 1", "{}"), refused("unknown name `totl`"));
    assert_eq!(script("ünïcødé", "{}"), refused("unknown name `ünïcødé`"));
    assert_eq!(script("break", "{}"), refused("unknown name `break`"));
    // A builtin's name given as a variable is the variable.
    assert_eq!(script("range + now", "{ range: 2, now: 3 }"), "5");
}

// Q-1013: `[...xs]` and `{ ...m }` are written as calls of Quillo's own (`__concat`, `__merge`), and
// a script's spread was "unknown name `__concat`". A spread makes what its lists hold, refused
// before it is made where that is more than 1 MB.
#[test]
fn a_scripts_spreads_work_and_are_held_to_what_it_may_make() {
    assert_eq!(script("[...xs, 3]", "{ xs: [1, \"é\"] }"), "[1, \"é\", 3]");
    assert_eq!(script("{ ...m, b: 2 }", "{ m: { a: 1 } }"), "{ a: 1, b: 2 }");
    assert_eq!(script("[...xs]", "{ xs: [] }"), "[]");
    assert_eq!(script("{ ...m }", "{ m: null }"), "{}");
    // 16 bytes an int in a list: 65,536 of them fit in 1 MB, one more does not.
    let spread = |n: usize| eval(&format!("let xs = 0..32768\nreturn run_script(\"len([...xs, ...xs, ...more])\", {{ xs: xs, more: 0..{} }}, {{ timeout: 60000 }})", n));
    assert_eq!(spread(0), "65536");
    assert_eq!(spread(1), refused(TOO_BIG));
    assert_eq!(eval("let xs = 0..131072\nreturn run_script(\"len([...xs, ...xs, ...xs, ...xs, ...xs, ...xs, ...xs, ...xs])\", { xs: xs }, { timeout: 60000 })"), refused(TOO_BIG));
    // A script's own name never says Quillo's.
    assert_eq!(script("__nope(1)", "{}"), refused("unknown name `__nope`"));
}

// Q-1013 (found in review): a script that gave a name of its world a lambda (`upper = (x) => x`)
// kept that world, 9 KB, for good: the world held the lambda, whose scope held the world. It is
// let go of after each script. As in 0.8.9, where it was made for each script and kept as well.
#[test]
fn a_script_that_names_a_lambda_in_its_world_leaves_nothing_behind() {
    let run = |n: u32| {
        peak(&format!(
            "let i = 0\nwhile i < {} {{\n    let r = run_script(\"upper = (x) => x\\nmath = {{ f: (y) => upper(y) }}\\nlet keep = () => math\\nmath.f(1)\")\n    i += 1\n}}\nprint(\"done\")\n",
            n
        ))
    };
    let (out, few) = run(2000);
    assert_eq!(out, "done\n");
    let (_, many) = run(20000);
    assert!(many * 2 < few * 3, "{} after 20,000 scripts, {} after 2,000", many, few);
}

// Q-1013 (found measuring it): a script's world, its builtins and modules, is made once on each
// thread and copied for each script, where it was made again each time. What one script assigns
// there is its own: the next script, on the same thread, has Quillo's.
#[test]
fn what_a_script_assigns_to_its_world_stays_its_own() {
    let mut it = loaded(
        "fn go() = [\n\
         run_script(\"upper = 5\\nupper\", {}), run_script(\"upper(\\\"é\\\")\", {}),\n\
         run_script(\"date = null\\ndate\", {}), run_script(\"date.add_days(\\\"2026-11-17\\\", 1)\", {}),\n\
         run_script(\"math = { sqrt: (x) => 0 }\\nmath.sqrt(16.0)\", {}), run_script(\"math.sqrt(16.0)\", {}),\n\
         run_script(\"format_number = 1\\nformat_number\", {}), run_script(\"format_number(1234.5)\", {}),\n\
         run_script(\"date.add_days = 1\", {}), run_script(\"date.add_days(\\\"2026-11-17\\\", 1)\", {}),\n\
         run_script(\"upper\", { upper: 7 }), run_script(\"upper(\\\"a\\\")\", {})\n\
         ]\n",
    );
    assert_eq!(
        call(&mut it, "go", vec![]),
        "[5, \"É\", null, \"2026-11-18\", 0, 4.0, 1, \"1,234.5\", err(\"line 1: a map's fields are not set in place: write `m = set(m, \\\"add_days\\\", …)`\"), \"2026-11-18\", 7, \"A\"]"
    );
    // And on another thread, its own world.
    let other = std::thread::spawn(|| eval("return run_script(\"[upper(\\\"b\\\"), math.sqrt(4.0), date.add_days(\\\"2026-12-31\\\", 1)]\", {})")).join().unwrap();
    assert_eq!(other, "[\"B\", 2.0, \"2027-01-01\"]");
}

// Q-1013: `stats.histogram` makes two lists of up to a million numbers from one int; a script's
// is held to 1 MB, refused before it is made where the bins alone are more.
#[test]
fn a_scripts_histogram_is_held_to_what_a_script_may_make() {
    let bins = |n: u32| script(&format!("len(stats.histogram(xs, {}).counts)", n), "{ xs: [1, 2, 3] }");
    assert_eq!(bins(1), "1");
    assert_eq!(bins(1000), "1000");
    // 43 bytes and 32 a bin, as `too_big` counts: 32,766 bins fit in 1 MB, 32,767 do not.
    assert_eq!(bins(32766), "32766");
    assert_eq!(bins(32767), refused(TOO_BIG));
    assert_eq!(bins(32769), refused(TOO_BIG));
    assert_eq!(bins(1_000_000), refused(TOO_BIG));
    // Edges given as a list, and the program's own call, as they were.
    assert_eq!(script("stats.histogram([1, 2, 3], [0, 2, 4]).counts", "{}"), "[1, 2]");
    assert_eq!(eval("return len(stats.histogram([1, 2, 3], 100000).counts)"), "100000");
    // A whole float, as the call takes it, held as the int is: 1000000.0 made its 99 MB first
    // (found in review).
    let bins = |n: &str| script(&format!("len(stats.histogram(xs, {}).counts)", n), "{ xs: [1, 2, 3] }");
    assert_eq!(bins("32766.0"), "32766");
    assert_eq!(bins("32767.0"), refused(TOO_BIG));
    assert_eq!(bins("1000000.0"), refused(TOO_BIG));
    assert!(bins("2.5").contains("expects an int"), "{}", bins("2.5"));
}

/// The 1 MB of a's and the shorter texts the `re` tests give their scripts.
const AS: &str = "let a = repeat(\"a\", 1048575)\n";

// Q-1013: `re` in a script: what its matches make stops at a script's 1 MB as it is made. A list of
// a million matches, or `re.replace(s, "", s)`, a million copies of a 1 MB text, were made before
// the script's value was counted.
#[test]
fn re_in_a_script_stops_at_what_a_script_may_make() {
    let run = |vars: &str, src: &str| eval(&format!("{}return run_script({:?}, {}, {{ timeout: 60000 }})", AS, src, vars));
    // Each match of "a" counts 88 bytes: 11,915 fit, 11,916 do not.
    assert_eq!(run("{ t: slice(a, 0, 11915) }", "len(re.find_all(t, \"a\"))"), "11915");
    assert_eq!(run("{ t: slice(a, 0, 11916) }", "len(re.find_all(t, \"a\"))"), refused(TOO_BIG));
    // A match at every place of 1 MB, as a list, or as the parts between.
    assert_eq!(run("{ t: a }", "len(re.find_all(t, \"\"))"), refused(TOO_BIG));
    assert_eq!(run("{ t: slice(a, 0, 200000) }", "len(re.split(t, \"\"))"), refused(TOO_BIG));
    assert_eq!(run("{ t: slice(a, 0, 1000) }", "len(re.split(t, \"\"))"), "1002");
    // A text made from a text at each match, stopped as it grows.
    assert_eq!(run("{ t: a }", "len(re.replace(t, \"\", t))"), refused(TOO_BIG));
    assert_eq!(run("{ t: slice(a, 0, 2000) }", "len(re.replace(t, \"\", t))"), refused(TOO_BIG));
    assert_eq!(run("{ t: slice(a, 0, 2000) }", "len(re.replace(t, \"a\", \"$0$0\"))"), "4000");
    // A text of 1 MB, and one byte more.
    assert_eq!(run("{ t: a }", "len(re.replace(t, \"^\", \"b\"))"), "1048576");
    assert_eq!(run("{ t: a }", "len(re.replace(t, \"^\", \"bb\"))"), refused(TOO_BIG));
    // A function for each match pays its steps.
    assert_eq!(run("{ t: \"a1b2\" }", "re.replace(t, \"\\\\d\", (m) => str(int(m.text) * 2))"), "\"a2b4\"");
    // The program's own calls make what they make.
    assert_eq!(eval(&format!("{}return len(re.find_all(slice(a, 0, 200000), \"a\"))", AS)), "200000");
    assert_eq!(eval(&format!("{}return len(re.split(slice(a, 0, 200000), \"\"))", AS)), "200002");
}

// Q-1013 (found in review): `re.replace` looked at a script's 1 MB after each match, but wrote the
// match's whole text first: 128 KB put in 8,192 times by `$0` was 1.9 GB. Each part is asked about
// before it is written: the one match's 256 MB is never made.
#[test]
fn re_replace_in_a_script_stops_before_a_match_writes_too_much() {
    let run = |times: usize| {
        peak(&format!(
            "let t = repeat(\"a\", 65536)\nlet w = repeat(\"$0\", {})\nprint(run_script(\"len(re.replace(t, \\\"a+\\\", w))\", {{ t: t, w: w }}, {{ timeout: 60000 }}))\n",
            times
        ))
    };
    let (fits, base) = run(8);
    assert_eq!(fits.trim(), "524288");
    let (over, most) = run(4096);
    assert_eq!(over.trim(), refused(TOO_BIG));
    assert!(most < base * 2, "{} against {}", most, base);
    // `$<name>` and `$1` too, and text around them.
    let mut it = loaded("fn go(t, w) = run_script(\"len(re.replace(t, \\\"(?<all>a+)\\\", w))\", { t: t, w: w }, { timeout: 60000 })\n");
    let s = |t: &str| quillo::Value::str(t);
    for w in ["$<all>", "$1", "x$1y", "é$0"] {
        let many = w.repeat(4096);
        assert_eq!(call(&mut it, "go", vec![s(&"a".repeat(65536)), s(&many)]), refused(TOO_BIG), "{}", w);
    }
    assert_eq!(call(&mut it, "go", vec![s(&"a".repeat(1000)), s("<$<all>|$1>")]), "2003");
}

// Q-1013 (found in review): what `re` holds of a match while it works counts its groups: 1,024
// empty `()` on 32 KB of text held 13,108 matches of 1,024 spans each, 344 MB, in `re.find_all`, and
// `re.split` and `re.replace` held as many again. The script is refused first.
#[test]
fn re_in_a_script_counts_each_matchs_groups() {
    let groups = "()".repeat(512);
    let run = |n: usize| {
        peak(&format!(
            "let t = repeat(\"a\", {})\nlet g = {:?}\nfor c in [\"len(re.find_all(t, g))\", \"len(re.split(t, g))\", \"len(re.replace(t, g, \\\"\\\"))\"] {{\n    print(run_script(c, {{ t: t, g: g }}, {{ timeout: 60000 }}))\n}}\n",
            n, groups
        ))
    };
    let (few, base) = run(50);
    assert_eq!(few, "51\n52\n50\n");
    let (many, most) = run(8192);
    assert_eq!(many, format!("{0}\n{0}\n{0}\n", refused(TOO_BIG)));
    assert!(most < base * 2, "{} against {}", most, base);
    // A pattern with a group or two, as before.
    assert_eq!(script("len(re.find_all(t, \"(a)|(b)\"))", "{ t: \"abab\" }"), "4");
    assert_eq!(script("re.split(\"a1b2c\", \"(\\\\d)\")", "{}"), "[\"a\", \"b\", \"c\"]");
}

/// 1,000 a's and b's, the same each run.
fn ab_text() -> String {
    let mut x: u32 = 1;
    (0..1000)
        .map(|_| {
            x = x.wrapping_mul(1103515245).wrapping_add(12345);
            if (x >> 16) & 1 == 0 { 'a' } else { 'b' }
        })
        .collect()
}

// Q-1013: a search may take its guard's time, and the script's clock, looked at each 256 steps,
// would let 128 of them run in a row past it: a script's `re` looks at it as each call begins.
#[test]
fn re_in_a_script_ends_at_the_scripts_time() {
    let mut it = loaded(&format!(
        "fn texts() = map(0..300, (i) => slice({:?}, i % 7) + \"ab\")\nfn go(ts, ms) = run_script(\"let n = 0\\nwhile n < len(ts) {{ re.test(ts[n], \\\"[ab]*a[ab]{{494}}c\\\")\\n n += 1 }}\\nn\", {{ ts: ts }}, {{ timeout: ms }})\n",
        ab_text()
    ));
    let ts = quillo::value::lookup(&it.globals, "texts").map(|f| it.call(&f, vec![], 0).ok().unwrap()).unwrap();
    // Each search of a text under 1 KB, which costs no step for what it reads.
    let mut per = Duration::MAX;
    for _ in 0..2 {
        let t = Instant::now();
        let all = call(&mut it, "go", vec![ts.clone(), quillo::Value::Int(60000)]);
        per = per.min(t.elapsed() / 300);
        assert_eq!(all, "300");
    }
    // A time of 25 searches: the clock, looked at after 32 steps and then each 256, about 3 a
    // search, looked at it again some 95 searches in.
    let ms = (per.as_secs_f64() * 1000.0 * 25.0).ceil().clamp(1.0, 60000.0) as u64;
    let mut cut = Duration::MAX;
    for _ in 0..3 {
        let t = Instant::now();
        let out = call(&mut it, "go", vec![ts.clone(), quillo::Value::Int(ms as i64)]);
        cut = cut.min(t.elapsed());
        assert_eq!(out, format!("err(\"line 2: the script took longer than its {} ms\")", ms));
    }
    assert!(cut < per * 60, "{:?} for a script of {} ms, each search {:?}", cut, ms, per);
}

// ---- Q-1021: date.format's locales -----------------------------------------------------------

const DATES: &str = "let d = date.parse(\"2026-11-17T09:10:06+09:00\")\n";

// Q-1021: a locale with no date names was English in silence, where `format_number` and
// `money.format` say theirs once: now said once for each name, at most 64.
#[test]
fn a_locale_with_no_date_names_is_said_once_and_written_in_english() {
    let d = support::temp_dir("f7281-dates");
    let long = "q".repeat(150);
    let mut src = String::from(DATES);
    for l in ["ja-JP", "ja-JP", "xx-YY", "de-DE", "en-XX", "fr-BE", "", "日本", long.as_str(), "zh_CN"] {
        src.push_str(&format!("print(date.format(d, {{ locale: {:?}, style: \"full\" }}))\n", l));
    }
    for i in 0..70 {
        src.push_str(&format!("date.format(d, {{ locale: \"q{}-ZZ\" }})\n", i));
    }
    std::fs::write(d.join("main.qwe"), &src).unwrap();
    let o = finish(Command::new(quillo()).arg(d.join("main.qwe")), b"");
    let (out, err) = text(&o);
    assert!(o.status.success(), "{}{}", out, err);
    let said = |l: &str| err.matches(&format!("quillo: warning: no date data for locale {:?}: written in English\n", l)).count();
    for l in ["ja-JP", "xx-YY", "", "日本", "zh_CN"] {
        assert_eq!(said(l), 1, "{}: {}", l, err);
    }
    // A language there with a region that is not: its language's names, and nothing said.
    for l in ["de-DE", "en-XX", "fr-BE"] {
        assert_eq!(said(l), 0, "{}: {}", l, err);
    }
    // A long name cut at 100 characters, as a name in an error is.
    assert_eq!(said(&format!("{}…", "q".repeat(100))), 1, "{}", err);
    assert_eq!(err.matches("no date data for locale").count(), 64, "{}", err);
    assert_eq!(said("q57-ZZ"), 1);
    assert_eq!(said("q58-ZZ"), 0);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "Tuesday, November 17, 2026");
    assert_eq!(lines[3], "Dienstag, 17. November 2026");
    assert!(lines.iter().filter(|l| **l == "Tuesday, November 17, 2026").count() >= 7, "{}", out);
    // Money's and numbers' warning is as it was.
    std::fs::write(d.join("main.qwe"), "print(format_number(1.5, \"xx-YY\", 1))\n").unwrap();
    let (out, err) = text(&finish(Command::new(quillo()).arg(d.join("main.qwe")), b""));
    assert_eq!(out, "1.5\n");
    assert!(err.contains("no money and number data for locale \"xx-YY\": written as en-US") && !err.contains("no date data"), "{}", err);
    let _ = std::fs::remove_dir_all(&d);
}

/// `node` given `js`, ended past two minutes.
fn node(js: &str) -> Option<Output> {
    if !Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
        return None;
    }
    let f = std::env::temp_dir().join(format!("quillo-f7281-{}.cjs", std::process::id()));
    std::fs::write(&f, js).unwrap();
    let mut cmd = Command::new("node");
    cmd.arg(&f);
    let o = finish(&mut cmd, b"");
    let _ = std::fs::remove_file(&f);
    Some(o)
}

// Q-1021: the browser's `date.format` says it as the server does, once for each name.
#[test]
fn the_browsers_date_format_says_it_too() {
    let long = "q".repeat(150);
    let it = loaded(&format!(
        "fn __t() {{\n{}return [date.format(d, {{ locale: \"ja-JP\", style: \"full\" }}), date.format(d, {{ locale: \"ja-JP\" }}), date.format(d, {{ locale: \"xx-YY\", style: \"short\" }}), date.format(d, {{ locale: \"de-DE\", style: \"full\" }}), date.format(d, {{ locale: \"en-XX\" }}), date.format(d, {{ locale: {:?} }}), date.format(d, {{ locale: \"日本\" }})]\n}}\n",
        DATES, long
    ));
    let js = quillo::client::test_script(&it, "__t").unwrap();
    let Some(o) = node(&js) else { return };
    let (out, err) = text(&o);
    assert!(o.status.success(), "{}", err);
    assert_eq!(
        out.trim(),
        "[\"Tuesday, November 17, 2026\", \"Nov 17, 2026\", \"11/17/26\", \"Dienstag, 17. November 2026\", \"Nov 17, 2026\", \"Nov 17, 2026\", \"Nov 17, 2026\"]"
    );
    let said = |l: &str| err.matches(&format!("quillo: warning: no date data for locale {}: written in English", serde_json::to_string(l).unwrap())).count();
    assert_eq!((said("ja-JP"), said("xx-YY"), said("日本"), said(&format!("{}…", "q".repeat(100)))), (1, 1, 1, 1), "{}", err);
    assert_eq!((said("de-DE"), said("en-XX")), (0, 0), "{}", err);
}

// ---- Q-1023: the REPL's null -----------------------------------------------------------------

fn repl(input: &str) -> (String, String) {
    let d = support::temp_dir("f7281-repl");
    let mut cmd = Command::new(quillo());
    cmd.arg("repl").current_dir(&d);
    let o = finish(&mut cmd, input.as_bytes());
    let _ = std::fs::remove_dir_all(&d);
    assert!(o.status.success(), "{:?}", text(&o));
    text(&o)
}

// Q-1023: an expression whose value is null printed nothing, as a statement does: `p.age` looked
// like a line that did nothing. A name, a field, an index or a literal whose value is null prints
// `null`; a call whose value is null prints nothing, as before (`null` after each `print` or
// `db.open` would be noise, FEEDBACK73 says), nor does an `if`, a `match` or a block; nor a
// statement.
#[test]
fn the_repl_prints_null_for_an_expression_and_nothing_for_a_statement() {
    let lines = [
        ("let p = { name: \"Rex\", \"âge\": null }", ""),
        ("p.age", "null"),
        ("p.name", "\"Rex\""),
        ("p[\"âge\"]", "null"),
        ("let xs = [1]", ""),
        ("xs[9]", "null"),
        ("xs[-1]", "1"),
        ("null", "null"),
        ("let nothing = null", ""),
        ("nothing", "null"),
        ("p.age.more", "null"),
        // A call's null, whatever is called, shows nothing.
        ("get(p, \"age\")", ""),
        ("find([1, 2], (x) => x > 5)", ""),
        ("(() => null)()", ""),
        ("fn f() = null", ""),
        ("f()", ""),
        ("fn g() { print(\"g ✓\") }", ""),
        ("g()", "g ✓"),
        ("[null]", "[null]"),
        ("p.age == null", "true"),
        ("if false { 1 }", ""),
        ("if false { 1 } else { null }", ""),
        ("if true { 2 } else { null }", "2"),
        ("match 1 { 2 => 3, _ => null }", ""),
        ("let n = 0", ""),
        ("n = 1", ""),
        ("n += 1", ""),
        ("while n < 3 { n += 1 }", ""),
        ("n", "3"),
        ("print(\"said ✓\")", "said ✓"),
        ("sleep(0)", ""),
        ("db.open(\":memory:\")", ""),
        ("math.isnan(1.0)", "false"),
        ("let print = (x) => null", ""),
        ("print(1)", ""),
        // What `return` gives, as before.
        ("return null", "null"),
    ];
    let input: String = lines.iter().map(|(l, _)| format!("{}\n", l)).collect();
    let want: String = lines.iter().filter(|(_, w)| !w.is_empty()).map(|(_, w)| format!("{}\n", w)).collect();
    let (out, err) = repl(&input);
    assert_eq!(out, want, "{}", err);
    assert_eq!(err, "");
    // Empty input, and one expression.
    assert_eq!(repl(""), (String::new(), String::new()));
    assert_eq!(repl("null").0, "null\n");
}

// ---- Q-1030: what Quillo writes teaches the guide's spelling -------------------------------------

/// The old spellings, as `quillo new` and the hints wrote them.
const OLD: &[&str] = &["@click=", "@submit=", "@change=", "@input=", "bind:value="];

fn walk(d: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// `quillo new` in `d` with `args`: the folder it made.
fn made(d: &Path, args: &[&str]) -> PathBuf {
    let mut cmd = Command::new(quillo());
    cmd.arg("new").args(args).current_dir(d);
    let o = finish(&mut cmd, b"");
    assert!(o.status.success(), "{:?}", text(&o));
    d.join(args[0])
}

/// Serves the program in `d` and asks for `/`: its status and body.
fn first_page(d: &Path) -> (u16, String) {
    let child = Command::new(quillo())
        .args(["run", "--port", "0"])
        .current_dir(d)
        .env("QUILLO_SECRET", "a test secret, long enough to sign sessions")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let port = support::port_in_stderr(&mut g.0, "main.qwe", Instant::now() + Duration::from_secs(60));
    let a = support::send(port, "GET", "/", &[], "");
    (a.status, a.body)
}

// Q-1030: `quillo new` wrote `@submit`, `bind:value`, `@change` and `@click`, which the guide no
// longer shows; both templates now write `onsubmit`, `value={draft}`, `onchange` and `onclick`,
// still check clean and serve.
#[test]
fn quillo_new_writes_the_guides_spelling_and_still_checks_and_serves() {
    let d = support::temp_dir("f7281-new");
    for (args, wants) in [(&["todo"][..], &["<form onsubmit={", "<input value={draft}", "onchange={() => show(toggle(t.id))}"][..]), (&["mini", "--minimal"][..], &["<button onclick={() => { count += 1 }}>"][..])] {
        let p = made(&d, args);
        let mut files = Vec::new();
        walk(&p, &mut files);
        let all: String = files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect();
        for old in OLD {
            assert!(!all.contains(old), "{:?} writes {}", args, old);
        }
        for want in wants {
            assert!(all.contains(want), "{:?} lacks {}", args, want);
        }
        let mut cmd = Command::new(quillo());
        cmd.arg("check").current_dir(&p);
        let (out, err) = text(&finish(&mut cmd, b""));
        assert!((out.clone() + &err).contains("ok (") && !(out.clone() + &err).contains("warning"), "{:?}: {}{}", args, out, err);
        let (status, body) = first_page(&p);
        assert_eq!(status, 200, "{:?}: {}", args, body);
        assert!(body.contains("data-c"), "{:?}: a component on the page: {}", args, body);
    }
    // The sample it copies is the same file.
    assert_eq!(std::fs::read_to_string(d.join("todo/ui/todos.qwe")).unwrap(), include_str!("../app/ui/todos.qwe"));
    let _ = std::fs::remove_dir_all(&d);
}

/// What `quillo check` says of `src`, each message with its line.
fn says(src: &str) -> Vec<String> {
    quillo::check::check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().map(|d| format!("{} {}", d.line, d.message)).collect()
}

// Q-1030: the hints, given for a program written with `onclick`, taught `@click`, `@input` and
// `@change`: each says it the guide's way.
#[test]
fn the_hints_teach_onclick_oninput_and_onchange() {
    let has = |src: &str, want: &str| {
        let d = says(src);
        assert!(d.iter().any(|m| m.contains(want)), "{}\n--- wanted ---\n{}\n--- got ---\n{:?}", src, want, d);
        for m in &d {
            for old in ["@click={", "@input={", "@change={", "(@click)", "`@click`?", "`@clik`"] {
                assert!(!m.contains(old), "{}: {}", src, m);
            }
        }
    };
    has("component C() {\n    state count = 0\n    <button onclick={count += 1}>+</button>\n}", "an assignment is not a value: a handler is a function, `onclick={() => { count += ... }}`");
    has("server fn total() -> int = 42\ncomponent C() {\n    <p>{total()}</p>\n}", "`total` waits on the server, so <C> can only call it from an event handler (onclick={() => { ... }}) or to set state: state x = total(...)");
    has("component C() {\n    state n = 0\n    <button onclik={() => { n += 1 }}>{n}</button>\n}", "`onclik` is no event the browser knows: did you mean `onclick`?");
    has("component C() {\n    state n = 0\n    <button @clik={() => { n += 1 }}>{n}</button>\n}", "`onclik` is no event the browser knows: did you mean `onclick`?");
    has("component C() {\n    state form = { name: \"\" }\n    <input value={form.name}/>\n}", "take what changes with `oninput={(e) => ...}`, or add `readonly`");
    has("component C() {\n    state form = { on: false }\n    <input type=\"checkbox\" checked={form.on}/>\n}", "take what changes with `onchange={(e) => ...}`");
    has("component C() {\n    let s = notification.state()\n    <p>{s}</p>\n}", "call it from an event handler (onclick), or set a state with it");
    has("component C() {\n    state n = 0\n    <div @mouseEnter={() => { n += 1 }}>{n}</div>\n}", "`@mouseEnter` never fires: the browser's event names are lower case, `onmouseenter` (or React's `onMouseEnter`)");
    has("let x = <button @click=\"no\"></button>", "`@click` needs a function, got str: a handler is a function, `onclick={() => ...}`; text is never run as code");
    has("route GET /a ? { u: str } = <div onclick={`go('${u}')`}>\"x\"</div>", "takes only text written here, as it runs as code: `onclick` needs a function, `onclick={() => ...}`");
    // An attribute's slip is said as it was.
    has("let x = <a hreff=\"/\">x</a>", "`hreff` is no attribute the browser knows: did you mean `href`?");
    // The browser's compiler, where the checker was not asked.
    let e = Interp::new(".").load_source("server fn save(x) = x\ncomponent C() = <p>{1.save()}</p>", "case.qwe").err().map(|e| e.to_string()).unwrap_or_default();
    assert_eq!(e, "case.qwe:2: <C> (compiled for the browser): `save` waits on the server, so a component can only call it from an event handler (onclick={() => { ... }}) or to set state: state x = save(...)");
    // `@click` and `bind:value` still work: no hint for them.
    assert_eq!(says("component C() {\n    state n = 0\n    state draft = \"\"\n    <div><button @click={() => { n += 1 }}>{n}</button><input bind:value={draft}/></div>\n}"), Vec::<String>::new());
}

/// `quillo lsp` in `d`, its messages read on a thread of their own, so that every wait has a
/// deadline (as tests/feedback70.rs's).
struct Lsp {
    _g: support::Guard,
    stdin: std::process::ChildStdin,
    got: std::sync::mpsc::Receiver<serde_json::Value>,
    next: u64,
}

impl Lsp {
    fn open(d: &Path, text: &str) -> (Lsp, String) {
        use std::io::BufRead;
        std::fs::write(d.join("main.qwe"), text).unwrap();
        let uri = format!("file://{}", d.join("main.qwe").display());
        let child = Command::new(quillo()).arg("lsp").current_dir(d).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let mut g = support::Guard::new(child);
        let mut out = std::io::BufReader::new(g.0.stdout.take().unwrap());
        let stdin = g.0.stdin.take().unwrap();
        let (tx, got) = std::sync::mpsc::channel();
        std::thread::spawn(move || loop {
            let mut len = 0;
            loop {
                let mut line = String::new();
                if out.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                if line.trim_end().is_empty() {
                    break;
                }
                if let Some(v) = line.trim_end().strip_prefix("Content-Length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut buf = vec![0; len];
            if out.read_exact(&mut buf).is_err() || tx.send(serde_json::from_slice(&buf).unwrap_or_default()).is_err() {
                return;
            }
        });
        let mut l = Lsp { _g: g, stdin, got, next: 0 };
        l.ask("initialize", serde_json::json!({ "capabilities": {} }));
        l.send(serde_json::json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
        l.send(serde_json::json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": { "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } } }));
        (l, uri)
    }

    fn send(&mut self, msg: serde_json::Value) {
        let body = msg.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        self.stdin.flush().unwrap();
    }

    fn ask(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.next += 1;
        let id = self.next;
        self.send(serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            match self.got.recv_timeout(end.saturating_duration_since(Instant::now())) {
                Ok(m) if m.get("id") == Some(&serde_json::json!(id)) => return m["result"].clone(),
                Ok(_) => {}
                Err(_) => panic!("the language server did not answer {}", method),
            }
        }
    }
}

// Q-1030: the hover over `component` said "Handlers: `@click={() => { count += 1 }}`".
#[test]
fn the_component_hover_shows_onclick() {
    let d = support::temp_dir("f7281-hover");
    let (mut l, uri) = Lsp::open(&d, "component C() {\n    state count = 0\n    <button onclick={() => { count += 1 }}>{count}</button>\n}\n");
    let h = l.ask("textDocument/hover", serde_json::json!({ "textDocument": { "uri": uri }, "position": { "line": 0, "character": 3 } }));
    let shown = h["contents"]["value"].as_str().unwrap_or("").to_string();
    assert!(shown.contains("Handlers: `onclick={() => { count += 1 }}`."), "{}", shown);
    assert!(!shown.contains('@'), "{}", shown);
    drop(l);
    let _ = std::fs::remove_dir_all(&d);
}

fn chrome() -> Option<PathBuf> {
    let node = Command::new("node").args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"]).output();
    if !node.map(|o| o.status.success()).unwrap_or(false) {
        return None;
    }
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium", "/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// The program `quillo new` made in `d`, served, and `body` run in its page in Chrome once the
/// runtime has started, with `$` (querySelector) and `tick`: what it gave back.
fn in_page(d: &Path, body: &str) -> Option<serde_json::Value> {
    let chrome = chrome()?;
    let mut server = support::Guard::new(
        Command::new(quillo())
            .args(["run", "--port", "0"])
            .current_dir(d)
            .env("QUILLO_SECRET", "a test secret, long enough to sign sessions")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let port = support::port_in_stderr(&mut server.0, "main.qwe", Instant::now() + Duration::from_secs(60));
    let page = format!(
        r#"(async () => {{
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {{}}).booted; i++) await tick(30);
  const $ = (q) => document.querySelector(q);
  const until = async (f) => {{ for (let i = 0; i < 200 && !f(); i++) await tick(25); return f(); }};
  let out;
  try {{ out = await (async () => {{
{body}
  }})(); }} catch (e) {{ out = {{ exception: String((e && e.stack) || e) }}; }}
  out.errors = window.__quilloErrors;
  document.body.dataset.result = JSON.stringify(out);
}})();"#
    );
    std::fs::write(d.join("t.js"), page).unwrap();
    let run = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/run.js");
    let mut cmd = Command::new("node");
    cmd.arg(&run).arg(&chrome).arg(format!("http://127.0.0.1:{}/", port)).arg(d.join("profile")).arg("60000").arg(d.join("t.js")).env_remove("QUILLO_PAGE_FOCUS").env_remove("QUILLO_PAGE_CDP");
    let o = finish(&mut cmd, b"");
    assert!(o.status.success(), "{:?}", text(&o));
    Some(serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{}: {:?}", e, text(&o))))
}

// Q-1030: the templates' `value={draft}`, `onsubmit` and `onchange` do in the browser what
// `bind:value`, `@submit` and `@change` did: typing sets the draft, adding clears it, ticking
// marks the row; and the minimal one's `onclick` counts.
#[test]
fn the_new_apps_work_in_the_browser_written_the_guides_way() {
    let d = support::temp_dir("f7281-new-browser");
    let todo = made(&d, &["todo"]);
    let Some(got) = in_page(
        &todo,
        r#"const input = $("form input");
  input.focus();
  input.value = "Buy milk ✓";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await tick(50);
  $("form button").click();
  await until(() => $("li"));
  const added = $("li") && $("li").textContent.trim();
  const cleared = $("form input").value;
  $("li input[type=checkbox]").click();
  await until(() => $("li.done"));
  return { added, cleared, done: !!$("li.done"), left: $("p.muted").textContent };"#,
    ) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        let _ = std::fs::remove_dir_all(&d);
        return;
    };
    assert_eq!(got["added"], "Buy milk ✓", "{}", got);
    assert_eq!(got["cleared"], "", "{}", got);
    assert_eq!(got["done"], true, "{}", got);
    assert!(got["left"].as_str().unwrap_or("").starts_with("0 of 1 left"), "{}", got);
    assert!(got["errors"].is_null() || got["errors"] == serde_json::json!([]), "{}", got);
    let mini = made(&d, &["mini", "--minimal"]);
    let got = in_page(&mini, r#"$("button").click(); await until(() => $("button").textContent.includes("1")); $("button").click(); await until(() => $("button").textContent.includes("2")); return { said: $("button").textContent };"#).unwrap();
    assert_eq!(got["said"], "Clicked 2 times", "{}", got);
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Q-1253 and Q-1024: the docs of `int` and `live` -----------------------------------------

fn doc(name: &str) -> (&'static str, &'static str) {
    quillo::builtin_docs::DOCS.iter().find(|(n, _, _)| *n == name).map(|(_, p, w)| (*p, *w)).unwrap()
}

// Q-1253: `int` said "text of digits", and takes signs, leading zeros and white space at either end.
// What it now says is what it does.
#[test]
fn int_says_what_text_it_takes() {
    // A hover's line, held to 140 bytes (tests/lsp.rs); the guide has the rest.
    let (_, says) = doc("int");
    assert!(says.contains("text: digits with an optional sign, leading zeros and outer spaces;"), "{}", says);
    for (src, want) in [
        ("int(\"42\")", "42"),
        ("int(\"+42\")", "42"),
        ("int(\"-42\")", "-42"),
        ("int(\"007\")", "7"),
        ("int(\" \\t42\\n\")", "42"),
        ("int(\"9223372036854775807\")", "9223372036854775807"),
        ("int(\"-9223372036854775808\")", "-9223372036854775808"),
    ] {
        assert_eq!(eval(&format!("return {}", src)), want, "{}", src);
    }
    for t in ["1.0", "1e3", "0x0A", "1_000", "", " ", "+", "--1", "4 2", "9223372036854775808", "٤٢"] {
        assert_eq!(eval(&format!("return is_err(int({:?}))", t)), "true", "{:?}", t);
    }
    for t in ["1.0", "1e3"] {
        assert!(says.contains(&format!("`\"{}\"`", t)), "{:?}: {}", t, says);
    }
}

// Q-1024: `live`'s arguments were "load, args", read as a list (`live(search, [q])`): they are
// passed one by one.
#[test]
fn live_says_its_arguments_are_passed_one_by_one() {
    let (params, told) = doc("live");
    assert_eq!(params, "load, args...");
    assert!(told.contains("given the rest one by one: `live(search, q)`, not `live(search, [q])`"), "{}", told);
    assert_eq!(quillo::check::call_shape("live"), "live(load, args...)");
    // The checker's own shape of a call takes the rest one by one too (slice C2).
    assert!(says("component C() {\n  state c = live()\n  <p>{c}</p>\n}").contains(&"2 missing argument `load` in call to `live`: live(load, ...args)".to_string()));
    // And so they are: `live(search, q)` calls `search(q)`.
    let mut it = loaded("server fn search(q: str) = upper(q)\nfn go() = live(search, \"é\")\n");
    assert_eq!(call(&mut it, "go", vec![]), "\"É\"");
}

// ---- Q-1264: crypto.verify_password's time ---------------------------------------------------

// Q-1264: `crypto.verify_password` took its full time for a real hash, null, "" or a bcrypt text,
// but none for a PHC text of an algorithm it does not verify (`$scrypt$...`) or one it cannot read:
// the time told an attacker such an account was there. Each takes the stand-in's work now.
#[test]
fn verify_password_takes_as_long_for_any_hash_it_cannot_verify() {
    let mut it = loaded("fn make(pw) = crypto.hash_password(pw)\nfn check(pw, h) = crypto.verify_password(pw, h)\n");
    let h = quillo::value::lookup(&it.globals, "make").map(|f| it.call(&f, vec![quillo::Value::str("right ✓")], 0).ok().unwrap()).unwrap();
    let s = quillo::Value::str;
    assert_eq!(call(&mut it, "check", vec![s("right ✓"), h.clone()]), "true");
    // Each hash timed by turns with the real one, the best of five of each: under the load of the
    // other tests, a real hash timed once at the start took twice what the stand-in took later.
    // What this guards against took a microsecond against 10 ms.
    let mut timed = |hash: &quillo::Value| {
        let (mut best, mut real) = (Duration::MAX, Duration::MAX);
        for _ in 0..5 {
            for (h, out) in [(hash, &mut best), (&h, &mut real)] {
                let t = Instant::now();
                assert_eq!(call(&mut it, "check", vec![s("wrong"), h.clone()]), "false", "{}", h.repr());
                *out = (*out).min(t.elapsed());
            }
        }
        (best, real)
    };
    for hash in [
        quillo::Value::Nil,
        s(""),
        s("plain text"),
        s("日本語"),
        s("$2b$10$N9qo8uLOickgx2ZMRZoMyeIjZAgcfl7p92ldGxad68LJZdL17lhWy"),
        s("$scrypt$ln=16,r=8,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaA"),
        s("$pbkdf2-sha256$i=1000$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaA"),
        // Argon2's own, with no hash, parameters it cannot take, or a version it has not.
        s("$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ"),
        s("$argon2id$v=19$m=1,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaA"),
        s("$argon2id$v=99$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaA"),
        s("$argon2xx$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaA"),
    ] {
        let (t, real) = timed(&hash);
        assert!(t * 4 >= real, "{}: {:?}, a real hash {:?}", hash.repr(), t, real);
    }
}
