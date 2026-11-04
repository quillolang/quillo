//! Everyday programs against Python (FEEDBACK29.md, Q-350 to Q-360; branch feedback29-fixes):
//! what a failed script says, updates that must not copy, and a script as a tool. Each fix is
//! tested on its edges too: aliases that must keep their value, unicode, empty values, wrong types.

mod support;

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use support::{send, serve};

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback29-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

fn run(name: &str, src: &str) -> Output {
    let d = dir(name, &[("main.qwe", src)]);
    Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).arg("--no-check").stdin(Stdio::null()).output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn said(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// What it prints, which must be the whole of it.
fn prints(name: &str, src: &str, want: &str) {
    let o = run(name, src);
    assert_eq!(out(&o), want, "{}", said(&o));
    assert!(o.status.success(), "{}", said(&o));
}

/// It finishes in time: a copy each round would not, at this size.
fn quick(name: &str, src: &str, want: &str, limit: u64) {
    let start = Instant::now();
    prints(name, src, want);
    assert!(start.elapsed() < Duration::from_secs(limit), "{}: {:?}", name, start.elapsed());
}

// ---- Q-350: an error that reaches the top ----------------------------------------------------

#[test]
fn a_try_at_the_top_stops_with_its_message_and_status() {
    let o = run("top-try", "print(\"before\")\nlet n = int(\"z\")?\nprint(\"after\")\n");
    assert_eq!(out(&o), "before\n");
    assert_eq!(o.status.code(), Some(1));
    assert!(said(&o).contains("not an int"), "{}", said(&o));
    assert!(said(&o).contains("main.qwe"), "{}", said(&o));
}

#[test]
fn the_edges_of_an_error_at_the_top() {
    // A function's `?` hands it to the caller, who may deal with it.
    prints("top-handled", "fn f() {\n    int(\"z\")?\n    1\n}\nprint(is_err(f()))\n", "true\n");
    // `??` deals with it there.
    prints("top-guarded", "let n = int(\"z\") ?? 0\nprint(n)\n", "0\n");
    // A plain `return` at the top is no failure, and nor is a value that is fine.
    prints("top-return", "print(\"a\")\nreturn 3\n", "a\n");
    prints("top-value", "let n = int(\"5\")?\nprint(n)\n", "5\n");
    // An explicit `return err(...)` has nobody to give it to either.
    let o = run("top-return-err", "return err(\"gave up\")\n");
    assert_eq!(o.status.code(), Some(1));
    assert!(said(&o).contains("gave up"), "{}", said(&o));
    // From a file it uses, the program fails too.
    let d = dir("top-used", &[("lib.qwe", "let n = int(\"z\")?\n"), ("main.qwe", "use lib.qwe\nprint(\"after\")\n")]);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).arg("--no-check").output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(out(&o), "");
}

/// A program with routes that fails as it loads does not start half made.
#[test]
fn a_server_does_not_start_after_a_failure_at_the_top() {
    let d = dir("top-server", &[("main.qwe", "let n = int(\"z\")?\nroute GET / = \"up\"\n")]);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).args(["--no-check", "--port", "0"]).env("QUILLO_SECRET", "a secret for this test, long enough to use").stdin(Stdio::null()).output().unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(said(&o).contains("not an int"), "{}", said(&o));
}

// ---- Q-352: however the update is written ------------------------------------------------------

#[test]
fn an_update_does_not_copy_however_it_is_written() {
    for (name, line) in [
        ("plain", "xs = push(xs, i)"),
        ("pipe", "xs = xs | push(i)"),
        ("method", "xs = xs.push(i)"),
        ("add", "xs += [i]"),
        ("plus", "xs = xs + [i]"),
    ] {
        let src = format!("fn f(n) {{\n    let xs = []\n    for i in 0..n {{\n        {}\n    }}\n    [len(xs), xs[0], xs[n - 1]]\n}}\nprint(f(200000))\n", line);
        quick(&format!("grow-{}", name), &src, "[200000, 0, 199999]\n", 8);
    }
    for (name, line) in [("plain", "m = set(m, str(i % 50), i)"), ("pipe", "m = m | set(str(i % 50), i)"), ("method", "m = m.set(str(i % 50), i)")] {
        let src = format!("fn f(n) {{\n    let m = {{}}\n    for i in 0..n {{\n        {}\n    }}\n    len(m)\n}}\nprint(f(300000))\n", line);
        quick(&format!("map-{}", name), &src, "50\n", 8);
    }
    let lists = "fn f(n) {\n    let xs = range(0, n)\n    for j in 0..n {\n        xs = xs | set(j, 1)\n    }\n    sum(xs)\n}\nprint(f(200000))\n";
    quick("set-pipe", lists, "200000\n", 8);
    let lambda = "print(len(reduce(range(0, 200000), {}, (m, i) => m | set(str(i % 9), i))))\n";
    quick("reduce-pipe", lambda, "9\n", 8);
}

/// A list shared with another name is copied, or that name would change.
#[test]
fn an_alias_keeps_its_value() {
    for (name, update, want) in [
        ("push", "a = push(a, 3)", "[1, 2, 3] [1, 2]\n"),
        ("pipe", "a = a | push(3)", "[1, 2, 3] [1, 2]\n"),
        ("method", "a = a.push(3)", "[1, 2, 3] [1, 2]\n"),
        ("add", "a += [3]", "[1, 2, 3] [1, 2]\n"),
        ("plus", "a = a + [3]", "[1, 2, 3] [1, 2]\n"),
        ("set", "a = a | set(0, 9)", "[9, 2] [1, 2]\n"),
    ] {
        prints(&format!("alias-{}", name), &format!("fn f() {{\n    let a = [1, 2]\n    let b = a\n    {}\n    print(a, b)\n}}\nf()\n", update), want);
    }
    // A closure and a list holding it, the same.
    prints("alias-closure", "fn f() {\n    let a = [1]\n    let g = () => a\n    let keep = [a]\n    a = a | push(2)\n    print(a, keep, g())\n}\nf()\n", "[1, 2] [[1]] [1, 2]\n");
    prints("alias-map", "fn f() {\n    let m = { a: 1 }\n    let n = m\n    m = m | set(\"b\", 2)\n    print(m, n)\n}\nf()\n", "{ a: 1, b: 2 } { a: 1 }\n");
    // An item that reads the list holds what it was.
    prints("alias-self", "fn f() {\n    let a = [1]\n    a = a | push(a)\n    print(a)\n}\nf()\n", "[1, [1]]\n");
}

#[test]
fn the_edges_of_an_update() {
    // Adding nothing, and adding to nothing.
    prints("add-empty", "fn f() {\n    let a = [1]\n    a += []\n    let b = []\n    b += [2]\n    print(a, b)\n}\nf()\n", "[1] [2]\n");
    // Adding a list to itself takes what it held.
    prints("add-self", "fn f() {\n    let a = [1, 2]\n    a += a\n    print(a)\n}\nf()\n", "[1, 2, 1, 2]\n");
    // Not a list: `+` as ever.
    prints("add-number", "fn f() {\n    let n = 1\n    n += 2\n    let s = \"a\"\n    s += \"b\"\n    s = s + \"c\"\n    print(n, s)\n}\nf()\n", "3 abc\n");
    let o = run("add-wrong", "fn f() {\n    let a = [1]\n    a += 2\n}\nf()\n");
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    // A function a map holds is called, not the builtin.
    prints("held-function", "fn f() {\n    let m = { push: (x) => x + 1 }\n    print(m.push(1))\n    let r = m.push(2)\n    print(r)\n}\nf()\n", "2\n3\n");
    prints("held-assign", "fn f() {\n    let m = { set: (x) => { a: x } }\n    m = m.set(1)\n    print(m)\n}\nf()\n", "{ a: 1 }\n");
    // A program's own `push` is its own.
    prints("own-push", "fn push(xs, x) = [x]\nfn f() {\n    let a = [1]\n    a = a | push(2)\n    print(a)\n}\nf()\n", "[2]\n");
    // A failed update leaves the error to say so, on a stop.
    let o = run("set-outside", "fn f() {\n    let a = [1]\n    a = a | set(5, 1)\n}\nf()\n");
    assert_eq!(o.status.code(), Some(1));
    assert!(said(&o).contains("outside a list"), "{}", said(&o));
}

// ---- Q-353: a script's top level ---------------------------------------------------------------

#[test]
fn a_scripts_top_level_does_not_copy() {
    quick("top-push", "let xs = []\nfor i in 0..200000 {\n    xs = xs | push(i)\n}\nprint(len(xs))\n", "200000\n", 8);
    quick("top-set", "let m = {}\nfor i in 0..300000 {\n    m = set(m, str(i % 9), i)\n}\nprint(len(m))\n", "9\n", 8);
    quick("top-add", "let xs = []\nfor i in 0..200000 {\n    xs += [i]\n}\nprint(len(xs))\n", "200000\n", 8);
    // A function reading the global sees it each round.
    prints("top-read", "let xs = []\nfn n() = len(xs)\nfor i in 0..3 {\n    xs = xs | push(n())\n}\nprint(xs)\n", "[0, 1, 2]\n");
    prints("top-alias", "let a = [1]\nlet b = a\na = a | push(2)\nprint(a, b)\n", "[1, 2] [1]\n");
}

/// Once serving, other requests read a global, so it must never be seen empty.
#[test]
fn a_global_stays_whole_under_a_server() {
    let src = "let xs = [0]\nroute GET /grow {\n    xs = xs | push(len(xs))\n    sleep(1)\n    len(xs)\n}\nroute GET /read = len(xs)\n";
    let d = dir("global-served", &[("main.qwe", src)]);
    let s = serve(&d.join("main.qwe"), &[]);
    let port = s.port;
    let writers: Vec<_> = (0..4).map(|_| std::thread::spawn(move || {
        for _ in 0..40 {
            send(port, "GET", "/grow", &[], "");
        }
    })).collect();
    // Each read that is answered sees it whole, and never less than the one before. Under a
    // loaded machine a read may be told to wait (a 503); that says nothing of the list.
    let (mut last, mut read) = (0, 0);
    for _ in 0..200 {
        let a = send(port, "GET", "/read", &[], "");
        if a.status != 200 {
            continue;
        }
        let n: i64 = a.body.parse().unwrap_or_else(|_| panic!("{:?}", a.body));
        assert!(n >= last, "{} after {}", n, last);
        last = n;
        read += 1;
    }
    assert!(read > 100, "{} of 200 answered", read);
    for w in writers {
        w.join().unwrap();
    }
    assert_eq!(send(port, "GET", "/read", &[], "").body, "161");
}

// ---- Q-355: a text read by index -----------------------------------------------------------------

#[test]
fn a_text_read_by_index_is_not_quadratic() {
    let ascii = "fn f(s) {\n    let c = 0\n    for i in 0..len(s) {\n        if s[i] == \"x\" {\n            c += 1\n        }\n    }\n    c\n}\nprint(f(repeat(\"x\", 2000000)))\n";
    quick("index-ascii", ascii, "2000000\n", 15);
    let wide = "fn f(s) {\n    let c = 0\n    let i = 0\n    while i < len(s) {\n        if s[i] == \"é\" {\n            c += 1\n        }\n        i += 1\n    }\n    c\n}\nprint(f(repeat(\"aé\", 500000)))\n";
    quick("index-wide", wide, "500000\n", 15);
}

#[test]
fn the_edges_of_a_text_read_by_index() {
    let long = "abcdefghij".repeat(10);
    let src = format!(
        "let s = \"{long}\"\nprint(s[0], s[99], s[-1], s[-100], s[100], s[-101], len(s))\nlet e = \"\"\nprint(e[0], e[-1], len(e))\nlet u = \"héllo 😀\"\nprint(u[1], u[6], u[-1], len(u), u[7])\nlet l = repeat(\"é😀a\", 40)\nprint(l[0], l[1], l[2], l[3], l[119], l[-1], l[-2], l[120], len(l))\nprint(\"a\"[0], \"é\"[0], \"😀\"[0])\n"
    );
    prints("index-edges", &src, "a j j a null null 100\nnull null 0\né 😀 😀 7 null\né 😀 a é a a 😀 null 120\na é 😀\n");
    // Two long texts in turn: what is kept is for the right one.
    let two = "let a = repeat(\"a\", 100)\nlet b = repeat(\"é\", 90)\nprint(a[5], b[5], a[5], len(a), len(b), len(a))\n";
    prints("index-two", two, "a é a 100 90 100\n");
    // A text made anew at the same place is not taken for the last one.
    let anew = "fn f() {\n    for i in 0..3 {\n        let s = repeat(str(i), 100)\n        print(s[0], len(s))\n    }\n}\nf()\n";
    prints("index-anew", anew, "0 100\n1 100\n2 100\n");
}

// ---- Q-356: a loop has no list's limit ------------------------------------------------------------

#[test]
fn a_loop_goes_past_a_lists_limit() {
    let src = "fn f(n) {\n    let s = 0\n    for i in 0..n {\n        s += 1\n    }\n    s\n}\nprint(f(10))\nprint(f(10))\nprint(f(10))\nprint(f(10))\nprint(f(20000000))\n";
    prints("loop-past", src, "10\n10\n10\n10\n20000000\n");
    prints("loop-range", "fn g(n) {\n    let s = 0\n    for i in range(0, n) {\n        s += 1\n    }\n    s\n}\nprint(g(10000001))\n", "10000001\n");
    // What makes a list keeps it.
    for (name, src) in [("dots", "let n = 20000000\nprint(len(0..n))\n"), ("range", "let n = 20000000\nprint(len(range(0, n)))\n"), ("keep", "fn f(n) = for i in 0..n => i\nprint(len(f(20000000)))\n")] {
        let o = run(&format!("list-{}", name), src);
        assert_eq!(o.status.code(), Some(1), "{}: {}", name, out(&o));
        assert!(said(&o).contains("range is too large"), "{}: {}", name, said(&o));
    }
    // Backwards, it runs no times.
    prints("loop-back", "fn f() {\n    for i in 5..1 {\n        print(i)\n    }\n    print(\"done\")\n}\nf()\n", "done\n");
}

#[test]
fn a_loop_whose_list_no_one_takes_makes_none_anywhere() {
    // At a script's top, where a loop is most often written.
    prints("top-loop", "let t = 0\nfor i in range(0, 10000001) {\n    t += 1\n}\nprint(t)\n", "10000001\n");
    // As the last thing in another loop's body, at the top and in a function.
    prints("inner-loop", "let t = 0\nfor i in 0..2 {\n    for j in 0..10000001 {\n        t += 1\n    }\n}\nprint(t)\n", "20000002\n");
    prints("inner-fn", "fn f() {\n    let t = 0\n    for i in 0..2 {\n        for j in 0..10000001 {\n            t += 1\n        }\n    }\n    t\n}\nprint(f())\n", "20000002\n");
    prints("in-while", "let t = 0\nlet k = 0\nwhile k < 1 {\n    for j in 0..10000001 {\n        t += 1\n    }\n    k += 1\n}\nprint(t)\n", "10000001\n");
    // In a file it uses.
    let d = dir("used-loop", &[("lib.qwe", "let total = 0\nfor i in 0..10000001 {\n    total += 1\n}\n"), ("main.qwe", "use lib.qwe\nprint(total)\n")]);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).arg("--no-check").output().unwrap();
    assert_eq!(out(&o), "10000001\n", "{}", said(&o));
    // What is kept is still a list, with its limit: an outer loop's list of inner ones.
    prints("kept-inner", "fn f() = for i in 0..2 => for j in 0..3 => i * j\nprint(f())\n", "[[0, 0, 0], [0, 1, 2]]\n");
    let o = run("kept-inner-big", "fn f() = for i in 0..1 => for j in 0..10000001 => j\nprint(len(f()))\n");
    assert!(said(&o).contains("range is too large"), "{}", said(&o));
    // The last statement of a program shows its list, as the REPL has it.
    prints("last-kept", "print(1)\nfor i in 0..3 {\n    i\n}\n", "1\n");
}

// ---- Q-351: `pow` of ints -----------------------------------------------------------------------

#[test]
fn pow_of_ints_is_exact_or_says_so() {
    prints("pow-fits", "print(pow(2, 62), pow(3, 39), pow(-2, 63), pow(0, 0), pow(5, 0), pow(1, 1000000), pow(-1, 1000001))\n", "4611686018427387904 4052555153018976267 -9223372036854775808 1 1 1 -1\n");
    for src in ["print(pow(2, 63))\n", "print(pow(2, 200))\n", "print(pow(3, 40))\n", "print(pow(-2, 64))\n", "print(pow(10, 19))\n"] {
        let o = run("pow-over", src);
        assert_eq!(o.status.code(), Some(1), "{}: {}", src, out(&o));
        assert!(said(&o).contains("integer overflow"), "{}: {}", src, said(&o));
    }
    // `??` guards it, floats stay floats, and `math.pow` is the float.
    prints("pow-floats", "print(pow(2, 200) ?? \"too big\")\nprint(pow(2.0, 3), pow(2, -1), pow(4, 0.5))\nprint(math.pow(2, 3))\n", "too big\n8.0 0.5 2.0\n8.0\n");
}

// ---- Q-358: lists order ----------------------------------------------------------------------------

#[test]
fn lists_compare_with_the_operators_too() {
    prints("order", "print([1, 2] < [1, 3], [1, 3] > [1, 2], [1, 2] <= [1, 2], [1, 2] >= [1, 2], [1] < [1, 0], [] < [1], [[1, 2]] < [[1, 3]])\n", "true true true true true true true\n");
    let o = run("order-mixed", "print([1, \"a\"] < [1, 2])\n");
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    let d = dir("order-check", &[("main.qwe", "let a = [1, 2]\nprint(a < [1, 3])\n")]);
    let c = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
}

// ---- the runtime ------------------------------------------------------------------------------

/// Two functions of one name are one function, the last, with no word from JavaScript: a helper
/// named `classApply` took the place of the one every `class={...}` runs on.
#[test]
fn the_runtime_has_one_function_of_each_name() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/client/runtime.js")).unwrap();
    let mut seen = std::collections::HashSet::new();
    for line in src.lines() {
        let rest = line.strip_prefix("async function ").or_else(|| line.strip_prefix("function "));
        let Some(rest) = rest else { continue };
        let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect();
        assert!(seen.insert(name.clone()), "`{}` is written twice in runtime.js", name);
    }
    assert!(seen.len() > 100, "{}", seen.len());
}

// ---- Q-357: a script as a command-line tool ----------------------------------------------------

/// `quillo run main.qwe` in `d`, with `opts` before the program and `input` piped in.
fn tool(d: &std::path::Path, opts: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .args(opts)
        .arg(d.join("main.qwe"))
        .arg("--no-check")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(input.as_bytes()).unwrap();
    drop(stdin);
    child.wait_with_output().unwrap()
}

#[test]
fn a_path_outside_is_read_only_where_it_is_allowed() {
    let outside = support::temp_dir("feedback29-outside");
    std::fs::write(outside.join("a.txt"), "outside é\n").unwrap();
    let path = outside.join("a.txt").display().to_string();
    let d = dir("allow", &[("main.qwe", &format!("print(read_file({:?}))\n", path))]);
    // Not named: refused, as before.
    let o = tool(&d, &[], "");
    assert!(out(&o).contains("must stay inside the program's folder"), "{}", out(&o));
    // Named: read, by its own path, and through `..` from the program's folder.
    let o = tool(&d, &["--allow-path", &outside.display().to_string()], "");
    assert_eq!(out(&o), "outside é\n\n", "{}", said(&o));
    let rel = pathdiff(&outside.join("a.txt"), &d);
    std::fs::write(d.join("main.qwe"), format!("print(read_file({:?}))\n", rel)).unwrap();
    let o = tool(&d, &["--allow-path", &outside.display().to_string()], "");
    assert_eq!(out(&o), "outside é\n\n", "{}: {}", rel, said(&o));
    // Written, folders made, in the named folder only.
    let new = outside.join("made/deeper/b.txt");
    std::fs::write(d.join("main.qwe"), format!("print(write_file({:?}, \"x\"))\nprint(read_file({:?}))\n", new.display().to_string(), new.display().to_string())).unwrap();
    let o = tool(&d, &["--allow-path", &outside.display().to_string()], "");
    assert_eq!(out(&o), "null\nx\n", "{}", said(&o));
}

#[test]
fn the_edges_of_an_allowed_path() {
    let outside = support::temp_dir("feedback29-allowed");
    let beside = support::temp_dir("feedback29-beside");
    std::fs::write(beside.join("secret.txt"), "no").unwrap();
    let allow = outside.display().to_string();
    let read = |name: &str, path: String| {
        let d = dir(name, &[("main.qwe", &format!("let r = read_file({:?})\nprint(is_err(r))\n", path))]);
        out(&tool(&d, &["--allow-path", &allow], ""))
    };
    // Out of the named folder by `..`, or into another: refused.
    assert_eq!(read("up", format!("{}/../{}/secret.txt", allow, beside.file_name().unwrap().to_string_lossy())), "true\n");
    assert_eq!(read("other", beside.join("secret.txt").display().to_string()), "true\n");
    // A link inside it to a file outside it: where the link leads is what counts.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(beside.join("secret.txt"), outside.join("link.txt")).unwrap();
        assert_eq!(read("link", outside.join("link.txt").display().to_string()), "true\n");
    }
    // A folder whose name starts with the named one's is another folder.
    let sibling = format!("{}-more", allow);
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::write(format!("{}/x.txt", sibling), "x").unwrap();
    assert_eq!(read("prefix", format!("{}/x.txt", sibling)), "true\n");
    // Not there, or not a folder: `quillo run` will not start.
    let d = dir("allow-missing", &[("main.qwe", "print(1)\n")]);
    let o = tool(&d, &["--allow-path", "/no/such/folder"], "");
    assert_eq!(o.status.code(), Some(2), "{}", out(&o));
    assert!(said(&o).contains("--allow-path"), "{}", said(&o));
    let o = tool(&d, &["--allow-path", &beside.join("secret.txt").display().to_string()], "");
    assert_eq!(o.status.code(), Some(2), "{}", out(&o));
    // Twice, and with `=`.
    let other = beside.display().to_string();
    let d = dir("allow-two", &[("main.qwe", &format!("print(read_file({:?}))\n", beside.join("secret.txt").display().to_string()))]);
    let o = tool(&d, &["--allow-path", &allow, &format!("--allow-path={}", other)], "");
    assert_eq!(out(&o), "no\n", "{}", said(&o));
}

/// The way from `base` to `p`, with `..`.
fn pathdiff(p: &std::path::Path, base: &std::path::Path) -> String {
    let (p, base) = (p.canonicalize().unwrap(), base.canonicalize().unwrap());
    let common = p.components().zip(base.components()).take_while(|(a, b)| a == b).count();
    let ups = base.components().count() - common;
    let rest: Vec<String> = p.components().skip(common).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    format!("{}{}", "../".repeat(ups), rest.join("/"))
}

#[test]
fn a_script_reads_what_is_piped_to_it() {
    let d = dir("stdin", &[("main.qwe", "let s = stdin()\nprint(len(s), len(split(trim(s), \"\\n\")))\nprint(upper(s))\n")]);
    let o = tool(&d, &[], "a\nbé\n😀\n");
    assert_eq!(out(&o), "7 3\nA\nBÉ\n😀\n\n", "{}", said(&o));
    // Nothing piped is "".
    let o = tool(&d, &[], "");
    assert_eq!(out(&o), "0 1\n\n", "{}", said(&o));
    // A line at a time, to the end, with or without a last line break, and `\r\n`.
    let d = dir("lines", &[("main.qwe", "let n = 0\nlet line = read_line()\nwhile line != null {\n    print(`[${line}]`)\n    n += 1\n    line = read_line()\n}\nprint(n)\n")]);
    assert_eq!(out(&tool(&d, &[], "a\nb\n")), "[a]\n[b]\n2\n");
    assert_eq!(out(&tool(&d, &[], "a\r\n\r\nlast")), "[a]\n[]\n[last]\n3\n");
    assert_eq!(out(&tool(&d, &[], "")), "0\n");
    // Bytes that are not UTF-8 are replaced, not refused.
    let d = dir("stdin-bytes", &[("main.qwe", "print(len(stdin()))\n")]);
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).arg("--no-check").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&[b'a', 0xff, b'b']).unwrap();
    assert_eq!(out(&child.wait_with_output().unwrap()), "3\n");
}

#[test]
fn a_script_ends_with_the_status_it_gives() {
    let d = dir("exit", &[("main.qwe", "print(\"before\")\nexit(3)\nprint(\"after\")\n")]);
    let o = tool(&d, &[], "");
    assert_eq!((o.status.code(), out(&o)), (Some(3), "before\n".to_string()), "{}", said(&o));
    // From inside a function, and with no status.
    let d = dir("exit-deep", &[("main.qwe", "fn stop() {\n    exit()\n}\nfor i in 0..10 {\n    if i == 2 {\n        stop()\n    }\n    print(i)\n}\n")]);
    let o = tool(&d, &[], "");
    assert_eq!((o.status.code(), out(&o)), (Some(0), "0\n1\n".to_string()), "{}", said(&o));
    for (i, (bad, why)) in [("256", "0 to 255"), ("-1", "0 to 255"), ("\"1\"", "0 to 255"), ("1.0", "0 to 255")].iter().enumerate() {
        let d = dir(&format!("exit-bad-{}", i), &[("main.qwe", &format!("exit({})\n", bad))]);
        let o = tool(&d, &[], "");
        assert_eq!(o.status.code(), Some(1), "{}", bad);
        assert!(said(&o).contains(why), "{}: {}", bad, said(&o));
    }
    let d = dir("exit-255", &[("main.qwe", "exit(255)\n")]);
    assert_eq!(tool(&d, &[], "").status.code(), Some(255));
}

#[test]
fn a_server_keeps_its_input_and_its_life() {
    let d = dir("exit-served", &[("main.qwe", "route GET /exit = exit(0)\nroute GET /stdin = stdin()\nroute GET /line = read_line()\nroute GET /ok = \"ok\"\n")]);
    let s = serve(&d.join("main.qwe"), &[]);
    for path in ["/exit", "/stdin", "/line"] {
        let a = send(s.port, "GET", path, &[], "");
        assert_eq!(a.status, 500, "{}: {}", path, a.body);
    }
    // It is still there.
    assert_eq!(send(s.port, "GET", "/ok", &[], "").body, "ok");
}

// ---- Q-359: what a habit from Python or JavaScript is told ---------------------------------------

#[test]
fn a_habit_is_told_what_to_write() {
    let cases = [
        ("print(2 in [1, 2])", "contains(xs, x)"),
        ("let x = 2\nprint(1 < x < 3)", "a < x && x < b"),
        ("let x = 2\nprint(x ** 2)", "pow(x, 2)"),
        ("let xs = [1, 2, 3]\nprint(xs[1:3])", "slice"),
        ("let xs = [1, 2, 3]\nprint(xs[::2])", ""),
        ("let xs = [1]\nprint(for x in xs if x > 0 => x)", "filter"),
        ("fn f(a, c) = a\nprint(f(1, c: 9))", "pass a map"),
        ("fn f(...xs) = xs", ""),
        ("let xs = [1]\nprint(len(...xs))", "spread goes in a list or a map"),
        ("let m = {}\nlet k = \"a\"\nprint({ ...m, [k]: 1 })", "set("),
        ("try {\n    print(1)\n}", "errors are values"),
        ("class A {\n}", "`type`"),
        ("fn f() {\n    yield 1\n}", "return a list"),
        ("throw err(\"x\")", "return err("),
        ("def f(x) = x", "fn name(x)"),
    ];
    for (i, (src, want)) in cases.iter().enumerate() {
        let d = dir(&format!("habit-{}", i), &[("main.qwe", &format!("{}\n", src))]);
        let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).arg("--no-check").output().unwrap();
        assert_eq!(o.status.code(), Some(1), "{}", src);
        let said = said(&o);
        assert!(!said.contains("expected an expression") && !said.contains("expected end of statement"), "{}: {}", src, said);
        assert!(said.contains(want), "{}: {}", src, said);
    }
    // What is not a habit is read as before: `in` in `for`, `<` alone, `*` alone, a spread in a list.
    prints("habit-fine", "for x in [1] {\n    print(x)\n}\nprint(1 < 2, 2 * 3, [...[1], 2], { ...{ a: 1 }, b: 2 })\n", "1\ntrue 6 [1, 2] { a: 1, b: 2 }\n");
    // A name that is a habit's word elsewhere is a name.
    prints("habit-names", "let class = 1\nlet yield = 2\nprint(class + yield)\n", "3\n");
}

// ---- Q-347: what fails at once is said by `quillo check` ---------------------------------------

fn check(name: &str, src: &str) -> String {
    let d = dir(name, &[("main.qwe", src)]);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn check_says_what_run_refuses_of_the_page() {
    let page = |code: &str| format!("component C() {{\n    state o = \"\"\n    <button @click={{() => {{ {} }}}}>{{o}}</button>\n}}\nroute GET / = <html><body><C/></body></html>\n", code);
    let said = check("js-timeout", &page("js(\"setTimeout\", () => { o = \"a\" }, 50)"));
    assert!(said.contains("does not call setTimeout") && said.contains("run_after(ms, f)"), "{}", said);
    let said = check("js-reflect", &page("js(\"Reflect.set\", js(\"window\"), \"a\", 1)"));
    assert!(said.contains("steps around the checks"), "{}", said);
    let said = check("js-window-reflect", &page("js(\"window.Reflect.apply\")"));
    assert!(said.contains("steps around the checks"), "{}", said);
    let said = check("js-bind", &page("let f = js(\"navigator.geolocation.getCurrentPosition.bind\")"));
    assert!(said.contains("warning") && said.contains("`bind`"), "{}", said);
    // What `quillo run` takes, check takes.
    let said = check("js-fine", &page("o = str(js(\"Math.max\", 1, 2))"));
    assert!(said.contains("ok"), "{}", said);
    let said = check("js-callish", &page("let f = js(\"calls\")"));
    assert!(!said.contains("`call`"), "{}", said);
}

/// `defs` and `body` on the server and, compiled, in Node: what each gives, an error's text too.
fn both(defs: &str, body: &str) -> Option<(String, String)> {
    if !Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
        return None;
    }
    let src = format!("{}\nfn __t() {{\n{}\n}}\n", defs, body);
    let mut it = quillo::Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let server = match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) | Err(quillo::interp::Ctrl::Return(v)) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(quillo::interp::Ctrl::Reply(_)) => "REPLY".into(),
    };
    let js = quillo::client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let o = support::node(&js).expect("node");
    Some((server, String::from_utf8_lossy(&o.stdout).trim().to_string() + &String::from_utf8_lossy(&o.stderr)))
}

#[test]
fn a_mistake_from_javascript_is_told_what_to_write() {
    for body in ["let xs = [1, 2]\nxs.length", "let s = \"ab\"\ns.length"] {
        let (server, page) = both("", body).unwrap_or_default();
        assert!(server.contains("`len(x)`"), "{}", server);
        if !page.is_empty() {
            assert!(page.contains("`len(x)`"), "{}", page);
        }
    }
    // A miss is null, not -1, on both sides; other comparisons say what they said.
    let (server, page) = both("", "let i = index_of([1, 2], 5)\ni < 0").unwrap_or_default();
    assert!(server.contains("not -1"), "{}", server);
    assert!(page.is_empty() || page.contains("not -1"), "{}", page);
    let (server, _) = both("", "[1] < \"a\"").unwrap_or_default();
    assert!(!server.contains("not -1"), "{}", server);
    // A map's own `length` is a field like any.
    let (server, page) = both("", "let m = { length: 3 }\nm.length").unwrap_or_default();
    assert_eq!(server, "3");
    assert!(page.is_empty() || page == "3", "{}", page);
}

// ---- Q-345: a style with no element of its own -------------------------------------------------

#[test]
fn a_global_style_reaches_the_page_through_a_component() {
    let src = "let v_css = css`#p { color: rgb(1, 2, 3) }`\nview Inner() = <p id=\"p\">inner</p>\nview V() {\n    style global v_css\n    <Inner/>\n}\nview Plain() = <p>plain</p>\nroute GET / = <html><head></head><body><V/></body></html>\nroute GET /plain = <html><head></head><body><Plain/></body></html>\nroute GET /twice = <html><head></head><body><V/><V/></body></html>\n";
    let d = dir("style-unmarked", &[("main.qwe", src)]);
    let s = serve(&d.join("main.qwe"), &[]);
    let sheet = |path: &str| -> Option<String> {
        let a = send(s.port, "GET", path, &[], "");
        let at = a.body.find("/_quillo/s/")?;
        let href: String = a.body[at..].chars().take_while(|c| *c != '"').collect();
        Some(send(s.port, "GET", &href, &[], "").body)
    };
    for path in ["/", "/twice", "/"] {
        let css = sheet(path).unwrap_or_else(|| panic!("{}: no sheet", path));
        assert!(css.contains("rgb(1, 2, 3)"), "{}: {}", path, css);
    }
    // A page without that view has no sheet for it.
    assert!(sheet("/plain").map_or(true, |c| !c.contains("rgb(1, 2, 3)")));
}

#[test]
fn children_not_given_are_null_on_the_server_too() {
    let src = "view Box(children) = <div class=\"box\">{children == null ? \"empty\" : children}</div>\ncomponent Bare(children) {\n    state c = null\n    <aside>{children == null && (c != null ? <div>open</div> : <p>none</p>)}</aside>\n}\ncomponent Holds() {\n    <div><Bare/></div>\n}\nroute GET /views = <html><body><Box/>|<Box><b>x</b></Box>|<Box>{\"\"}</Box>|<Box>{null}</Box></body></html>\nroute GET /nested = <html><body><Holds/></body></html>\nroute GET /top = <html><body><Bare/></body></html>\nroute GET /given = <html><body><Bare><i>k</i></Bare></body></html>\n";
    let d = dir("children", &[("main.qwe", src)]);
    let s = serve(&d.join("main.qwe"), &[]);
    let body = send(s.port, "GET", "/views", &[], "").body;
    // None given; one; an expression that gives nothing is still a child.
    assert!(body.contains("<div class=\"box\">empty</div>|<div class=\"box\"><b>x</b></div>|<div class=\"box\"></div>|<div class=\"box\"></div>"), "{}", body);
    assert!(send(s.port, "GET", "/nested", &[], "").body.contains("<p>none</p>"));
    let top = send(s.port, "GET", "/top", &[], "").body;
    assert!(top.contains("<p>none</p>") && top.contains("data-p=\"{&quot;children&quot;:null}\""), "{}", top);
    let given = send(s.port, "GET", "/given", &[], "").body;
    assert!(given.contains("<aside></aside>") && given.contains("&lt;i&gt;k&lt;/i&gt;"), "{}", given);
}

#[test]
fn a_component_called_as_a_function_says_so() {
    let src = "component Dead() {\n    state n = 0\n    <button @click={() => { n += 1 }}>{n}</button>\n}\nview Alive() = <p>alive</p>\nlet make = Dead\nlet plain = Alive\nroute GET /dead = <html><body>{make()}</body></html>\nroute GET /mapped = <html><body>{[1] | map(Dead)}</body></html>\nroute GET /view = <html><body>{plain()}</body></html>\nroute GET /tag = <html><body><Dead/></body></html>\n";
    let d = dir("called", &[("main.qwe", src)]);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    let said = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    assert!(said.contains("`make` is a component; write <Dead ... />"), "{}", said);
    assert!(!said.contains("`plain`"), "{}", said);
    let s = support::serve_unchecked(&d.join("main.qwe"));
    for path in ["/dead", "/mapped"] {
        assert_eq!(send(s.port, "GET", path, &[], "").status, 500, "{}", path);
    }
    assert!(send(s.port, "GET", "/view", &[], "").body.contains("<p>alive</p>"));
    assert!(send(s.port, "GET", "/tag", &[], "").body.contains("quillo-island"));
}

// ---- Q-348: a parameter named like a global -----------------------------------------------------

#[test]
fn a_local_named_like_a_global_is_its_own_on_both_sides() {
    let ok = [
        "let rows = [1]\nfn other(rows) {\n    rows = rows + [1]\n    rows\n}\ncomponent P() { <b>{len(rows)}</b> }\nroute GET / = <html><body><P/></body></html>\nroute GET /o = str(other([2]))\n",
        "let rows = [1]\nfn other() {\n    let rows = [3]\n    rows = rows + [1]\n    rows\n}\ncomponent P() { <b>{len(rows)}</b> }\nroute GET / = <html><body><P/></body></html>\nroute GET /o = str(other())\n",
        "let rows = [1]\nfn other(xs) = xs | map((rows) => { rows = rows + 1\n    rows })\ncomponent P() { <b>{len(rows)}</b> }\nroute GET / = <html><body><P/></body></html>\nroute GET /o = str(other([2]))\n",
    ];
    for (i, src) in ok.iter().enumerate() {
        let d = dir(&format!("shadow-{}", i), &[("main.qwe", src)]);
        let c = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
        assert!(c.status.success(), "{}: {}", i, String::from_utf8_lossy(&c.stdout));
        let s = serve(&d.join("main.qwe"), &[]);
        assert!(send(s.port, "GET", "/", &[], "").body.contains("<b>1</b>"), "{}", i);
        assert_eq!(send(s.port, "GET", "/o", &[], "").status, 200, "{}", i);
    }
    // One that changes the global itself is refused, by both.
    let d = dir("shadow-real", &[("main.qwe", "let rows = [1]\nfn grow() {\n    rows = rows + [1]\n}\ncomponent P() { <b>{len(rows)}</b> }\nroute GET / = <html><body><P/></body></html>\n")]);
    let c = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    let said_check = String::from_utf8_lossy(&c.stdout).into_owned() + &String::from_utf8_lossy(&c.stderr);
    assert!(said_check.contains("server variable that changes"), "{}", said_check);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).arg("--no-check").arg("--port").arg(support::free_port().to_string()).output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(said(&o).contains("server variable that the program changes"), "{}", said(&o));
}
