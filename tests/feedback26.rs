//! What 100 plugins found (FEEDBACK26.md; branch feedback26-fixes): an error's text must not
//! reach a visitor by being turned into text, and a failure must not pass for a no.

mod support;

use std::path::PathBuf;
use std::process::{Command, Stdio};
use support::{send, serve};

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback26-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

const SECRET: &str = "secret-path";

const LEAKS: &str = r#"
fn r() = read_file("/nope/secret-path.txt")

route GET /value = r()
route GET /map { let x = r(); { x: x } }
route GET /text { let x = r(); `value: ${x}` }
route GET /str { let x = r(); { line: str(x) } }
route GET /plus { let x = r(); "value: " + str(x) }
route GET /markup { let x = r(); <p>{`${x}`}</p> }
route GET /attr { let x = r(); <p title={`${x}`}>t</p> }
route GET /inner { let x = r(); <p>{x}</p> }
route GET /json { let x = r(); { line: json.stringify({ x }) } }
route GET /join { let x = r(); join([x], ",") }
route GET /nested { let x = r(); str({ a: [x] }) }
route GET /own { let x = err("your card was declined"); `value: ${x}` }
route GET /own_str = str(err("your card was declined"))
route GET /printed {
    let x = r()
    print(x)
    print(`it failed: ${x}`)
    "done"
}
route GET /inside {
    let x = r()
    let s = `value: ${x}`
    print(s)
    s
}
"#;

#[test]
fn a_hidden_errors_text_never_reaches_a_visitor() {
    let d = dir("leaks", &[("main.qwe", LEAKS)]);
    let s = serve(&d.join("main.qwe"), &[]);
    for path in ["/value", "/map", "/text", "/str", "/plus", "/markup", "/attr", "/inner", "/json", "/join", "/nested", "/inside"] {
        let a = send(s.port, "GET", path, &[], "");
        assert!(!a.body.contains(SECRET) && !a.body.contains("read_file"), "{}: {}", path, a.body);
        assert!(a.body.contains("internal error"), "{}: {}", path, a.body);
    }
    // What the program wrote is its own to show.
    assert!(send(s.port, "GET", "/own", &[], "").body.contains("your card was declined"));
    assert!(send(s.port, "GET", "/own_str", &[], "").body.contains("your card was declined"));
}

/// The same with instances, which start another way.
#[test]
fn it_holds_with_instances() {
    let d = dir("instances", &[("main.qwe", LEAKS)]);
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).args(["--port", &port.to_string(), "--instances", "2"]).env("QUILLO_SECRET", "a secret for this test, long enough to use").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    support::stop_with_tests(&child);
    let _server = support::Guard::new(child);
    while !support::listening(port) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let a = send(port, "GET", "/text", &[], "");
    assert_eq!(a.body, "value: err(\"internal error\")");
}

/// What is hidden from a visitor goes to whoever runs the server: to the log with its
/// request, and to `print`.
#[test]
fn the_log_and_print_keep_the_text() {
    let d = dir("log", &[("main.qwe", LEAKS)]);
    let port = support::free_port();
    let log = std::fs::File::create(d.join("log.txt")).unwrap();
    let out = std::fs::File::create(d.join("out.txt")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).args(["--port", &port.to_string()]).env("QUILLO_SECRET", "a secret for this test, long enough to use").stdin(Stdio::null()).stdout(out).stderr(log).spawn().unwrap();
    support::stop_with_tests(&child);
    let _server = support::Guard::new(child);
    while !support::listening(port) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    send(port, "GET", "/text", &[("x-request-id", "abc123")], "");
    assert_eq!(send(port, "GET", "/printed", &[], "").body, "done");
    let logged = std::fs::read_to_string(d.join("log.txt")).unwrap();
    assert!(logged.contains("GET /text (rid abc123)") && logged.contains(SECRET), "{}", logged);
    let printed = std::fs::read_to_string(d.join("out.txt")).unwrap();
    assert_eq!(printed.matches(SECRET).count(), 2, "{}", printed);
    assert!(printed.contains("it failed: err("), "{}", printed);
}

/// Under `--watch` errors go to their author, and a script has no visitors.
#[test]
fn a_script_sees_its_errors() {
    let d = dir("script", &[("main.qwe", "let x = read_file(\"/nope/secret-path.txt\")\nprint(str(x))\nprint(`${x}`)\nprint(json.stringify(x))\n")]);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).args(["--no-check"]).output().unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    assert_eq!(text.matches(SECRET).count(), 3, "{}{}", text, String::from_utf8_lossy(&o.stderr));
}

const PREDICATES: &str = r#"
fn r(p) = read_file(p)

route GET /filter { let g = filter(["/a", "/b"], (p) => r(p)? != ""); { g: g, is: is_err(g) } }
route GET /find { let g = find(["/a"], (p) => r(p)); { is: is_err(g) } }
route GET /any { let g = any(["/a"], (p) => r(p)); { is: is_err(g) } }
route GET /all { let g = all(["/a"], (p) => r(p)); { is: is_err(g) } }
route GET /count { let g = count(["/a"], (p) => r(p)); { is: is_err(g) } }
route GET /stops {
    let g = filter(["/a"], (p) => r(p))?
    { g: g, after: "ran" }
}
route GET /good = filter([1, 2, 3], (x) => x > 1)
"#;

/// Q-198: a predicate that failed has said neither yes nor no.
#[test]
fn a_predicate_that_fails_is_a_failure() {
    let d = dir("predicates", &[("main.qwe", PREDICATES)]);
    let s = serve(&d.join("main.qwe"), &[]);
    for path in ["/find", "/any", "/all", "/count"] {
        assert_eq!(send(s.port, "GET", path, &[], "").body, "{\"is\":true}", "{}", path);
    }
    assert_eq!(send(s.port, "GET", "/filter", &[], "").body, "{\"g\":{\"error\":\"internal error\"},\"is\":true}");
    assert_eq!(send(s.port, "GET", "/stops", &[], "").status, 500);
    assert_eq!(send(s.port, "GET", "/good", &[], "").body, "[2,3]");
}

#[test]
fn check_says_a_try_at_the_end_of_a_lambda_does_nothing() {
    let warned = |src: &str| {
        let d = dir("try", &[("main.qwe", src)]);
        let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
        String::from_utf8_lossy(&o.stderr).contains("`?` here changes nothing")
    };
    assert!(warned("fn f(x) = int(x)\nprint(map([\"1\"], (i) => f(i)?))\n"));
    assert!(warned("fn f(x) = int(x)\nprint(map([\"1\"], (i) => {\n    print(i)\n    f(i)?\n}))\n"));
    // It does something here: what comes after it does not run.
    assert!(!warned("fn f(x) = int(x)\nprint(map([\"1\"], (i) => f(i)? + 1))\n"));
    assert!(!warned("fn f(x) = int(x)\nfn g(x) {\n    let v = f(x)?\n    v + 1\n}\nprint(g(\"1\"))\n"));
}

// ---- FEEDBACK25.md ------------------------------------------------------------------------

fn checked(name: &str, src: &str) -> String {
    let d = dir(name, &[("main.qwe", src)]);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// Q-305: `x?.y` reads as JavaScript's, and is not.
#[test]
fn check_says_what_a_try_before_a_field_is() {
    let page = |body: &str| format!("component C() {{\n    state s = 1\n    let shown = s == 1 ? null : {{ text: \"a\" }}\n    <p>{{{}}}</p>\n}}\nroute GET / = <html><body><C/></body></html>\n", body);
    assert!(checked("chain", &page("shown?.text")).contains("is not optional chaining"));
    assert!(!checked("chain2", &page("shown.text")).contains("optional chaining"));
    // Leaving when a call failed is what `?` is for.
    assert!(!checked("chain3", "server fn f(x) = http.get(x)?.json\nroute GET / = f(\"https://a.example\")\n").contains("optional chaining"));
}

/// Q-319: it works while `a` holds, the case tried first.
#[test]
fn check_says_how_a_pipe_binds_after_and() {
    let warned = "fn f(xs, y) = y != null && xs | any((x) => x == 1)\nprint(f([1], 2))\n";
    assert!(checked("pipe", warned).contains("`|` binds looser than `&&`"));
    assert!(!checked("pipe2", "fn f(xs, y) = y != null && (xs | any((x) => x == 1))\nprint(f([1], 2))\n").contains("binds looser"));
}

/// Q-321: a decimal is fine there, and rounding it would lose the cents.
#[test]
fn a_division_in_money_is_no_mistake() {
    let src = "fn part(price, left: int, days: int) = money.of(price * left / days, \"USD\", \"half_up\")\nprint(part(100, 1, 3))\n";
    assert!(!checked("money", src).contains("divides evenly"), "{}", checked("money", src));
    assert!(checked("money2", "fn n(a) = range(0, a / 4)\nprint(n(8))\n").contains("divides evenly"));
}

/// Q-322: it leaves by the `return` or not at all.
#[test]
fn a_while_true_that_returns_has_a_return_type() {
    let src = "fn first_even(xs: list) -> int {\n    let i = 0\n    while true {\n        if xs[i] % 2 == 0 {\n            return xs[i]\n        }\n        i += 1\n    }\n}\nprint(first_even([1, 2]))\n";
    assert!(!checked("loop", src).contains("must return"), "{}", checked("loop", src));
    let ends = "fn f(n: int) -> int {\n    while n > 0 {\n        return n\n    }\n}\nprint(f(1))\n";
    assert!(checked("loop2", ends).contains("must return"), "{}", checked("loop2", ends));
}

/// Q-315: instant on the server, a hung tab in the browser.
#[test]
fn check_says_a_pattern_can_hang_a_tab() {
    let warned = |p: &str| checked("pattern", &format!("print(re.test(\"aaa\", {:?}))\n", p)).contains("can hang a browser tab");
    assert!(warned("(a+)+$"));
    assert!(warned("(x*)*"));
    assert!(warned("((a+))+"));
    assert!(!warned("(a+)b+"));
    assert!(!warned("[(a+)]+"));
    assert!(!warned("a{1,8}"));
    assert!(!warned("\\(a+\\)+"));
}

/// Q-324: what a JavaScript habit gets is what to write instead.
#[test]
fn a_habit_from_javascript_says_what_to_write() {
    for (src, says) in [
        ("route GET / = <></>\n", "no `<>` fragments"),
        ("let a = 1, b = 2\n", "one `let` to a line"),
        ("print(map([[1, 2]], ([a, b]) => a + b))\n", "plain names, not patterns"),
        ("fn f() {\n    let walk = (n) => n > 0 ? walk(n - 1) : 0\n    walk(1)\n}\nprint(f())\n", "does not exist until its `let` ends"),
        ("let k = \"a\"\nlet m = { [k]: 1 }\n", "`set(m, k, v)`"),
        ("route GET /s {\n    session.x = 1\n    { ok: true }\n}\n", "set_session"),
    ] {
        let out = checked("habit", src);
        assert!(out.contains(says), "{}: {}", src, out);
    }
}

/// Q-314: a list is changed in place though its new item reads it, and stays a value.
#[test]
fn set_on_a_list_reading_itself_costs_its_length() {
    let src = "fn fill(n) {\n    let xs = range(0, n)\n    for i in range(1, n) {\n        xs = set(xs, i, xs[i - 1] + 1)\n    }\n    xs[n - 1]\n}\nfn keeps() {\n    let a = [1, 2]\n    let b = a\n    a = set(a, 0, a[1])\n    [a, b]\n}\nprint(fill(100000))\nprint(keeps())\n";
    let d = dir("set", &[("main.qwe", src)]);
    let start = std::time::Instant::now();
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout), "99999\n[[2, 2], [1, 2]]\n", "{}", String::from_utf8_lossy(&o.stderr));
    assert!(start.elapsed() < std::time::Duration::from_secs(10), "{:?}", start.elapsed());
}

/// Q-318: `??` guards against what a value can cause, not against what must end a request.
#[test]
fn a_guard_does_not_hold_up_what_must_end() {
    let src = "route GET /slow {\n    let n = 0\n    let r = (() => {\n        while true {\n            n = n + 1\n        }\n    })() ?? \"caught\"\n    r\n}\n";
    let d = dir("guard", &[("main.qwe", src)]);
    // Started by `serve_with`, which gives up on a server that stopped or never listened, where
    // this waited for good on one that lost its port to another test.
    let s = support::serve_with(&d.join("main.qwe"), &["--max-compute", "200ms", "--no-check"]);
    assert_eq!(send(s.port, "GET", "/slow", &[], "").status, 500);
}

// ---- the rest of FEEDBACK26.md --------------------------------------------------------------

const VALUES: &str = r#"
fn pick(k: str) = json.parse(k)

route GET /map { let r = pick("{\"k\":1}"); { e: r.error, m: r.message, f: error(r) } }
route GET /text { let s = pick("\"text\""); { e: s.error, f: error(s) } }
route GET /whole { let n = pick("5"); { e: n.error, f: error(n) } }
route GET /list { let l = pick("[1]"); { e: l.error } }
route GET /nothing { { e: error(null) } }
route GET /failed { let r = err("said no"); { e: r.error, m: r.message, f: error(r) } }
route GET /own { let r = pick("{"); { is: is_err(r), hidden: r } }
"#;

/// Q-199: asking a value whether it failed gives one answer, whatever it is.
#[test]
fn asking_for_an_error_is_safe_on_any_value() {
    let d = dir("errors", &[("main.qwe", VALUES)]);
    let s = serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| send(s.port, "GET", p, &[], "");
    assert_eq!(get("/map").body, "{\"e\":null,\"m\":null,\"f\":null}");
    assert_eq!(get("/text").body, "{\"e\":null,\"f\":null}");
    assert_eq!(get("/whole").body, "{\"e\":null,\"f\":null}");
    assert_eq!(get("/list").body, "{\"e\":null}");
    assert_eq!(get("/nothing").body, "{\"e\":null}");
    assert_eq!(get("/failed").body, "{\"e\":\"said no\",\"m\":\"said no\",\"f\":\"said no\"}");
    assert!(get("/own").body.contains("internal error"));
}

const REPEATED: &str = r#"
route GET /x = { query: request.query, raw: request.query_string }
route GET /many ? { a: [str] } = { a }
route GET /numbers ? { n: [int] } = { n }
route POST /form = { form: request.form }
route POST /list { form: { tag: [str] } } = { tag }
"#;

/// Q-200: what was sent can be had, and a list in a pattern matches.
#[test]
fn a_repeated_name_can_be_read() {
    let d = dir("repeated", &[("main.qwe", REPEATED)]);
    let s = serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| send(s.port, "GET", p, &[], "");
    let form = |p: &str, b: &str| send(s.port, "POST", p, &[("content-type", "application/x-www-form-urlencoded"), ("sec-fetch-site", "same-origin")], b);
    // What it was, for those who read a single value.
    assert_eq!(get("/x?a=1&a=2").body, "{\"query\":{\"a\":\"2\"},\"raw\":\"a=1&a=2\"}");
    assert_eq!(get("/many?a=1&a=2").body, "{\"a\":[\"1\",\"2\"]}");
    assert_eq!(get("/many?a=1").body, "{\"a\":[\"1\"]}");
    assert_eq!(get("/many?a=x%20y").body, "{\"a\":[\"x y\"]}");
    assert_eq!(get("/many").body, "{\"a\":[]}", "a group with nothing ticked sends nothing");
    assert_eq!(get("/numbers?n=1&n=22").body, "{\"n\":[1,22]}");
    assert_eq!(get("/numbers?n=1&n=x").status, 400);
    assert_eq!(form("/form", "tag=a&tag=b").body, "{\"form\":{\"tag\":\"b\"}}");
    assert_eq!(form("/list", "tag=a&tag=b").body, "{\"tag\":[\"a\",\"b\"]}");
    assert_eq!(form("/list", "other=1").body, "{\"tag\":[]}");
}

/// Q-202: what follows `--` is the program's.
#[test]
fn a_program_takes_its_arguments() {
    let d = dir("args", &[("main.qwe", "print(args())\nprint(len(args()))\n")]);
    let run = |extra: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).args(extra).output().unwrap();
        (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
    };
    assert_eq!(run(&[]).0, "[]\n0\n");
    assert_eq!(run(&["--", "a", "b c", "--port"]).0, "[\"a\", \"b c\", \"--port\"]\n3\n");
    assert_eq!(run(&["--no-check", "--", "x"]).0, "[\"x\"]\n1\n");
    assert!(run(&["a"]).1.contains("unknown argument"));
}
