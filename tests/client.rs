//! The browser compiler: compiled code must compute what the server computes.
//! Each case runs on the interpreter and, compiled to JS, in Node.

use std::process::Command;
use quillo::Interp;

fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Runs `body` as the body of a function on the server and in Node.
fn both(defs: &str, body: &str) -> (String, String) {
    both_with("", defs, body)
}

/// Like `both`, with JavaScript run in Node before the program.
fn both_with(prelude: &str, defs: &str, body: &str) -> (String, String) {
    let src = format!("{}\nfn __t() {{\n{}\n}}\n", defs, body);
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let server = match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(quillo::interp::Ctrl::Return(v)) => v.repr(),
        Err(quillo::interp::Ctrl::Reply(_)) => "REPLY".into(),
    };
    let js = quillo::client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let js = format!("{}\n{}", prelude, js);
    let out = Command::new("node").arg("-e").arg(&js).output().expect("node");
    let browser = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(out.status.success(), "node failed: {}\n{}", String::from_utf8_lossy(&out.stderr), js);
    (server, browser)
}

fn same(defs: &str, body: &str) {
    let (s, b) = both(defs, body);
    assert_eq!(s, b, "\n--- defs ---\n{}\n--- body ---\n{}", defs, body);
}

/// Both fail, with the same message.
fn same_error(defs: &str, body: &str, needle: &str) {
    let (s, b) = both(defs, body);
    assert!(s.starts_with("ERROR") && s.contains(needle), "server: {}", s);
    assert!(b.starts_with("ERROR") && b.contains(needle), "browser: {}", b);
}

#[test]
fn parallel_runs_the_same_in_the_browser() {
    if !node_available() {
        return;
    }
    same("fn a() = 1\nfn b(x) = x * 2", "parallel(() => a(), () => b(21), () => err(\"x\"))");
    same("", "parallel()");
    same_error("", "parallel(() => 1, 2)", "parallel");
}

#[test]
fn a_proto_key_is_an_ordinary_key() {
    if !node_available() {
        return;
    }
    same("", "let m = set({ a: 1 }, \"__proto__\", { p: 1 })\n[keys(m), m.a, m[\"__proto__\"]]");
}

#[test]
fn values_and_operators() {
    if !node_available() {
        return;
    }
    same("", "[7 / 2, 6 / 2, -7 / 2, 1 / 4, floor(7 / 2), ceil((10 + 540) / 36), 2.5 / 2]");
    same("", "[1 + 2 * 3, 7 / 2, 7 % 3, -4, 2 < 3, \"a\" < \"b\", 1 == 1, [1, 2] == [1, 2], { a: 1 } == { a: 1 }, 1 != 2]");
    same("", "[\"a\" + 1, 1 + \"b\", [1] + [2], { a: 1 } + { b: 2 }, 0..4, !null, !0]");
    same("", "let name = \"Ada\"\n`Hi ${name}, ${1 + 1} ${[1, \"x\"]} ${null} ${{ k: true }}`");
    same("", "[null ?? 1, err(\"x\") ?? 2, 3 ?? 4, null && 1, 0 || null, true && 5, 0 && 1, false || \"x\"]");
    same("", "let xs = [10, 20, 30]\n[xs[0], xs[-1], xs[9], \"héllo\"[1], { a: 1 }[\"a\"], null[0], { a: { b: 2 } }.a.b, null.x]");
    same_error("", "1 / 0", "division by zero");
    same_error("", "[1] + 1", "cannot add list and int");
    same_error("", "{} < 1", "cannot compare map with int");
}

#[test]
fn functions_and_closures() {
    if !node_available() {
        return;
    }
    same("fn add(a: int, b: int = 10) -> int = a + b", "[add(1), add(1, 2)]");
    same("fn make() { let n = 0\n() => { n += 1\nn } }", "let c = make()\nc()\nc()\nc()");
    same("fn fact(n) = n <= 1 ? 1 : n * fact(n - 1)", "fact(10)");
    same("", "let double = (x) => x * 2\n[1, 2, 3] | map(double) | filter((x) => x > 2)");
    same("fn f(x) { if x > 0 { return \"pos\" }\n\"other\" }", "[f(1), f(-1)]");
    same_error("fn add(a: int, b: int = 10) -> int = a + b", "add(\"x\")", "argument `a` of `add` must be int, got str");
    same_error("fn add(a, b) = a", "add(1, 2, 3)", "`add` takes 2 arguments, got 3");
    same_error("fn add(a, b) = a", "add()", "missing argument `a` in call to `add`");
    same_error("fn f() -> str = 1", "f()", "`f` must return str, but returned int 1");
    same_error("fn f() -> int ! str = err(1)", "f()", "declares errors as `! str`");
}

#[test]
fn pipelines_and_methods() {
    if !node_available() {
        return;
    }
    same("", "[3, 1, 2].sort().first()");
    same("", "[\"a\", \"b\"].len");
    same("fn total(xs) = xs | sum", "[[1, 2], [3]] | map(total)");
    same("", "let m = { greet: (n) => `hi ${n}` }\nm.greet(\"x\")");
    same("", "[{ n: 3 }, { n: 1 }] | sort_by((o) => o.n) | map((o) => o.n)");
    same("", "\"a,b,c\" | split(\",\") | map(upper) | join(\"-\")");
}

#[test]
fn errors_try_and_coalesce() {
    if !node_available() {
        return;
    }
    let defs = "fn parse_age(s: str) -> int ! str = match int(s) {\n int as n if n >= 0 => n\n _ => err(`bad age: ${s}`)\n}\nfn next_year(s) = parse_age(s)? + 1";
    same(defs, "[next_year(\"41\"), next_year(\"x\"), int(\"12\"), is_err(int(\"zz\")), float(\"1.5\")]");
    same("", "let e = err({ code: 7 })\n[e.error.code, is_err(e), unwrap(5)]");
    same_error("", "unwrap(err(\"boom\"))", "unwrap of an error: boom");
}

#[test]
fn control_flow_and_patterns() {
    if !node_available() {
        return;
    }
    same("", "for x in [1, 2, 3] => x * x");
    same("", "for [k, v] in { a: 1, b: 2 } => `${k}=${v}`");
    same("", "for c in \"abc\" => upper(c)");
    let m = "fn kind(e) = match e {\n { type: \"click\", x: int, y: int } => `click ${x},${y}`\n { type: \"key\", key: \"Enter\" | \"Return\" } => \"enter\"\n [first, ..rest] => `list ${first} ${len(rest)}`\n int as n if n < 0 => \"negative\"\n { limit: int = 20 } => `limit ${limit}`\n _ => \"other\"\n}";
    same(m, "[kind({ type: \"click\", x: 1, y: 2 }), kind({ type: \"key\", key: \"Return\" }), kind([1, 2, 3]), kind(-5), kind({}), kind({ limit: 5 }), kind(\"z\")]");
    same("", "let [a, b] = [1, 2]\nlet { name, age: int as years } = { name: \"Ada\", age: 36 }\n[a, b, name, years]");
    same("", "if 1 > 2 { \"a\" } else if 2 > 1 { \"b\" } else { \"c\" }");
    same_error("", "match 5 { \"x\" => 1 }", "no `match` arm matches 5");
    same_error("", "let [a] = [1, 2]\na", "`let` pattern does not match");
}

#[test]
fn standard_library() {
    if !node_available() {
        return;
    }
    same("", "[upper(\"a\"), lower(\"B\"), trim(\"  x \"), split(\"a b\"), join([1, 2], \",\"), contains(\"abc\", \"b\"), contains([1, 2], 2), starts_with(\"abc\", \"ab\"), ends_with(\"abc\", \"bc\"), replace(\"aXbX\", \"X\", \"-\")]");
    same("", "[slice([1, 2, 3, 4], 1, -1), slice(\"hello\", -3), len(\"héllo\"), len({ a: 1 }), type_of(1), type_of(\"s\"), type_of([]), type_of(null), str(42)]");
    same("", "[reduce([1, 2, 3], 0, (a, x) => a + x), sum([1, 2, 3]), min([3, 1, 2]), max([3, 1, 2]), reverse([1, 2]), reverse(\"ab\"), push([1], 2), range(3), range(2, 4), enumerate([\"a\"])]");
    same("", "let m = { a: 1, b: 2 }\n[keys(m), values(m), get(m, \"a\"), get(m, \"z\", 0), has(m, \"b\"), set(m, \"c\", 3), remove(m, \"a\"), merge(m, { a: 9 }), remove([1, 2, 1], 1)]");
    same("", "[any([1, 2], (x) => x > 1), all([1, 2], (x) => x > 1), find([1, 2, 3], (x) => x > 1), abs(-3), round(2.5), floor(2.7), ceil(2.1), pow(2, 10)]");
    same("", "[unique([1, 2, 1]), flatten([[1], [2, 3], 4]), zip([1, 2], [\"a\", \"b\"]), take([1, 2, 3], 2), drop(\"hello\", 3), count([1, 2, 3], (x) => x > 1), index_of(\"héllo\", \"l\"), repeat(\"ab\", 2)]");
    same("", "group_by([1, 2, 3, 4], (x) => x % 2 == 0 ? \"even\" : \"odd\")");
    same("", "[json.stringify({ a: [1, null, true] }), json.parse(\"\\{\\\"x\\\": [1, 2]\\}\"), is_err(json.parse(\"nope\")), escape(\"<a href='x'>\")]");
}

#[test]
fn compile_errors_name_what_cannot_run_in_the_browser() {
    let cases = [
        ("let todos = []\nfn add(t) { todos = todos | push(t) }\ncomponent C() = <p>{len(todos)}</p>", "`todos` is a server variable"),
        ("component C() = <p>{request.path}</p>", "`request` is only available on the server"),
        ("component C() = <p>{db.query(\"select 1\")}</p>", "`db` is not available in the browser"),
        ("component C() = <p>{env(\"HOME\")}</p>", "`env` is not available in the browser"),
        ("component C() = <p>{nope}</p>", "unknown name `nope`"),
        ("server fn save(x) = x\ncomponent C() = <p>{save(1)}</p>", "`save` waits on the server"),
        ("component C() { let x = 1\n<button @click={() => { x = 2 }}></button> }", ""),
        ("component C() = <p>{() => reply 404}</p>", "`reply` only works on the server"),
        ("let key = env(\"HOME\")\ncomponent C() = <p>{key}</p>", "`key` is read from the server's environment"),
        ("let home = env(\"HOME\") ?? \"\"\nlet shown = upper(home)\ncomponent C() = <p>{shown}</p>", "`shown` is read from"),
        ("let rows = db.query(\"select 1\")\ncomponent C() = <p>{len(rows)}</p>", "`rows` is read from"),
    ];
    for (src, want) in cases {
        let r = Interp::new(".").load_source(src, "c.qwe");
        if want.is_empty() {
            assert!(r.is_ok(), "{}: {:?}", src, r.err().map(|e| e.to_string()));
        } else {
            let e = r.err().map(|e| e.to_string()).unwrap_or_default();
            assert!(e.contains(want) && e.contains("<C>"), "{}\n-> {}", src, e);
        }
    }
}

#[test]
/// The one known difference: JS numbers do not remember being written `2.0`.
fn browser_floats_with_no_fraction_print_as_ints() {
    let (s, b) = both("", "7.5");
    assert_eq!(s, "7.5");
    assert_eq!(b, "7.5");
    let (s, b) = both("", "2.0");
    assert_eq!((s.as_str(), b.as_str()), ("2.0", "2"));
}

#[test]
fn rounding_and_union_record_patterns_match_the_server() {
    if !node_available() {
        return;
    }
    // Halves round away from zero on both sides (JavaScript's Math.round(-2.5) is -2).
    same("", "[round(-2.5), round(2.5), round(-0.5), round(-1.25, 1), round(1.25, 1), round(-3.7)]");
    same("fn page_of(q) = match q {\n { page: int | null } => [page]\n _ => \"no\"\n}", "[page_of({ page: 3 }), page_of({ page: null }), page_of({ page: \"x\" })]");
}

#[test]
fn recursive_functions_that_wait_on_the_server_are_awaited() {
    if !node_available() {
        return;
    }
    // A server that echoes the first argument back.
    let echo = "globalThis.fetch = async (url, o) => ({ ok: true, json: async () => ({ ok: JSON.parse(o.body)[0] }) });";
    let defs = "server fn echo(x) = x\nfn sum_to(n) = n == 0 ? 0 : echo(n) + sum_to(n - 1)\nfn even(n) = n == 0 ? echo(true) : odd(n - 1)\nfn odd(n) = n == 0 ? false : even(n - 1)";
    let (server, browser) = both_with(echo, defs, "[sum_to(3), even(4), odd(3)]");
    assert_eq!(server, "[6, true, true]");
    assert_eq!(browser, server);
}

#[test]
fn js_calls_name_what_they_call_in_the_code() {
    let load = |body: &str| {
        let mut it = Interp::new(".");
        let src = format!("component C(name: str) {{\n    <button @click={{() => {{ {} }}}}>x</button>\n}}\n", body);
        match it.load_source(&src, "case.qwe") {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        }
    };
    assert_eq!(load(r#"js("lib.go", 1)"#), "");
    assert_eq!(load(r#"js_new("L.Map", "map")"#), "");
    assert!(load("js(name, 1)").contains("written out"), "{}", load("js(name, 1)"));
    assert!(load(r#"js("eval", "1")"#).contains("runs text as code"));
    assert!(load(r#"js("window.setTimeout", "x")"#).contains("runs text as code"));
    assert!(load(r#"js("a..b")"#).contains("dotted name"));
}

#[test]
fn js_cannot_be_passed_around_or_given_its_name_by_data() {
    let load = |body: &str| {
        let mut it = Interp::new(".");
        let src = format!("component C(name: str) {{\n    <button @click={{() => {{ {} }}}}>x</button>\n}}\n", body);
        match it.load_source(&src, "case.qwe") {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        }
    };
    assert!(load("let j = js\nj(name, 1)").contains("can only be called"), "{}", load("let j = js\nj(name, 1)"));
    assert!(load("name.js(1)").contains("written out"), "{}", load("name.js(1)"));
    assert!(load("name | js(1)").contains("written out"), "{}", load("name | js(1)"));
    assert_eq!(load("let x = name.js\nx"), "");
}

#[test]
fn the_pages_objects_do_not_run_text_as_code() {
    if !node_available() {
        return;
    }
    let prelude = "globalThis.window = globalThis; globalThis.ran = 0; globalThis.lib = { el: new (class El { setAttribute(k, v) { globalThis.ran++; } })() };";
    let run = |body: &str| both_with(prelude, "", body).1;
    assert!(run(r#"js("window").eval("ran = 1")"#).contains("runs text as code"), "{}", run(r#"js("window").eval("ran = 1")"#));
    assert!(run(r#"let f = js("window").setTimeout
f("ran = 1")"#).contains("runs text as code"));
    assert!(run(r#"js("window").constructor"#).contains("runs text as code"));
    assert!(run(r#"js("lib.el").setAttribute("onclick", "x")"#).contains("runs text as code"));
    assert!(run(r#"js("lib.el").setAttribute("href", "javascript:x")"#).contains("javascript: URL"));
    assert_eq!(run(r#"js("lib.el").setAttribute("title", "x")"#), "null");
    assert_eq!(run("js(\"ran\")"), "0");
}

#[test]
fn constants_holding_functions_are_made_in_the_browser_too() {
    if !node_available() {
        return;
    }
    let defs = "let double = (x) => x * 2\n\
                let rules = { math: [double, (x) => x + 1], text: [(x) => `${x}!`] }\n\
                let plugins = [{ id: \"a\", rules }, { id: \"b\", rules: { math: [(x) => x - 100] } }]\n\
                fn run(kind, v) = plugins | map((p) => p.rules[kind] ?? []) | flatten | map((r) => r(v))";
    same(defs, "[run(\"math\", 5), run(\"text\", \"hi\")]");
    // One the program changes stays on the server.
    let mut it = Interp::new(".");
    let src = "let fs = [(x) => x]\nfn grow() { fs = fs + [(x) => x] }\ncomponent C() { <p>{len(fs)}</p> }\n";
    let e = it.load_source(src, "case.qwe").err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("changes"), "{}", e);
}

#[test]
fn state_is_declared_with_state_or_tilde() {
    let src = r#"
component C(start: int = 1) {
    state count = start
    ~other = count + 1
    let state = { label: "a name like any other" }
    <p>{count} {other} {state.label}</p>
}
route GET / = <C start={5} />
"#;
    let mut it = Interp::new(".");
    it.load_source(src, "case.qwe").unwrap();
    let r = it.handle(&quillo::server::HttpRequest { method: "GET".into(), url: "/".into(), headers: vec![("Host".into(), "x".into())], body: Vec::new(), remote: "127.0.0.1".into() });
    let html = String::from_utf8_lossy(&r.body).into_owned();
    assert!(html.contains("5 6 a name like any other"), "{}", html);
    let a = quillo::check::check_source(src, "main.qwe", std::path::Path::new("."));
    assert!(a.errors().next().is_none(), "{:?}", a.errors().map(|e| e.message.clone()).collect::<Vec<_>>());
    let js = quillo::client::test_script(&it, "C");
    assert!(js.is_ok(), "{:?}", js.err());
}

#[test]
fn url_encode_matches_the_server() {
    if !node_available() {
        return;
    }
    same("", r#"[url_encode("a b&c=d/é!*'()~"), url_encode(42), url_encode({ q: "rust & go", page: 2, tag: null, "a b": true })]"#);
    let (s, _) = both("", r#"url_encode({ q: "a b", page: 2, tag: null })"#);
    assert_eq!(s, r#""q=a%20b&page=2""#);
}

#[test]
fn while_loops_run_the_same_in_the_browser() {
    if !node_available() {
        return;
    }
    same("fn digits(n: int) {\n let out = []\n let x = n\n while x > 0 {\n out = [x % 10] + out\n x = floor(x / 10)\n }\n out\n}", "[digits(9051), digits(0)]");
    same("", "let i = 0\nlet log = []\nlet r = while i < 3 {\n log = push(log, i)\n i += 1\n}\n[log, r]");
    // A server call in the condition is awaited each time round.
    let echo = "globalThis.fetch = async (url, o) => ({ ok: true, json: async () => ({ ok: JSON.parse(o.body)[0] }) });";
    let (s, b) = both_with(echo, "server fn echo(x) = x", "let n = 0\nwhile echo(n) < 4 { n += 1 }\nn");
    assert_eq!((s.as_str(), b.as_str()), ("4", "4"));
}

/// tests for `re` (src/re.rs and runtime.js): every case gives the same on
/// both sides, matches and errors alike.
#[test]
fn regular_expressions_match_the_same_in_the_browser() {
    if !node_available() {
        return;
    }
    let cases: &[(&str, &str, &str)] = &[
        // (text, pattern, flags)
        ("Hi @ana and @bob_2, not a@b.c", r"(?:^|[^\w@])@(\w+)", ""),
        ("Hi @ana and @Bob", r"@(?<user>[a-z]+)", "i"),
        ("baaa", "a*", ""),
        ("", "x*", ""),
        ("café 😀 naïve", r"\w+", ""),
        ("café 😀 naïve", r"\b\w+\b", ""),
        ("a1 b22 c333", r"([a-z])(\d+)", ""),
        ("one\ntwo\r\nthree", r"^\w+$", "m"),
        ("one\ntwo\r\nthree", r"^\w+$", ""),
        ("a\nb\rc\u{2028}d", "a.b", ""),
        ("a\nb", "a.b", "s"),
        ("tab\there\u{a0}nbsp\u{3000}wide", r"\S+", ""),
        ("x😀y😀z", "😀", ""),
        ("x😀y", "[😀-😂]", ""),
        ("aaa", "a{2}", ""),
        ("aaaa", "a{1,2}?", ""),
        ("ÄäAa", "ä", "i"),
        ("a.b*c", r"\.|\*", ""),
        ("price: $40, $5", r"\$(\d+)", ""),
        ("ab", r"(a)|(b)", ""),
        ("x-y_z", r"[\w-]+", ""),
        ("a\u{e9}b", r"é|\u{1F600}|😀", ""),
        ("[x] {y}", r"\[\w\]|\{\w\}", ""),
        ("aXbxc", "x", "i"),
    ];
    for (s, p, f) in cases {
        let q = |x: &str| serde_json::to_string(x).unwrap();
        let body = format!(
            "let s = {}\nlet p = {}\nlet f = {}\n[re.test(s, p, f), re.find(s, p, f), re.find_all(s, p, f), re.split(s, p, f), re.replace(s, p, \"<$0|$1>\", f), re.replace(s, p, (m) => len(m.text), f)]",
            q(s), q(p), q(f)
        );
        same("", &body);
    }
    same("", r#"re.replace("ana bob", "(?<first>\\w)(\\w*)", "$<first>.$2$$")"#);
    // What either side would read differently is an error on both, for the same reason.
    for (p, why) in [
        ("(?=a)", "lookahead"),
        ("(?<!a)b", "lookbehind"),
        ("(a)\\1", "backreferences"),
        ("\\p{L}", "Unicode classes"),
        ("(?i)a", "flags go in the last argument"),
        ("[a[b]]", "write `\\[`"),
        ("[]", "empty class"),
        ("a**", "nothing to repeat"),
        ("*a", "nothing to repeat"),
        ("a{2,1}", "n is more than m"),
        ("a{,3}", "`{` on its own"),
        ("a]", "`]` on its own"),
        ("(a", "`(` without its `)`"),
        ("a)", "`)` without its `(`"),
        ("[z-a]", "lower character"),
        ("[\\d-z]", "between two characters"),
        ("\\q", "not one of the escapes"),
        ("\\-", "not one of the escapes"),
        ("\\uD83D", "surrogate"),
        ("(?<a>x)(?<a>y)", "two groups are named `a`"),
        ("a{1001}", "more than 1000"),
        ("[a&&b]", "two `&`"),
    ] {
        let body = format!("re.test(\"abc\", {})", serde_json::to_string(p).unwrap());
        same_error("", &body, why);
        let (s, b) = both("", &body);
        assert_eq!(s, b, "{}", p);
    }
    same_error("", "re.test(\"a\", \"a\", \"g\")", "the flags are");
    same_error("", "re.replace(\"a\", \"(a)\", \"$2\")", "the pattern has 1 group");
    same_error("", "re.replace(\"a\", \"a\", \"$<x>\")", "no group is named `x`");
    same_error("", "re.find(\"a\", 1)", "expects a str as argument 2");
}

#[test]
fn html_made_from_text_is_that_text_in_the_browser() {
    if !node_available() {
        return;
    }
    same("", r#"[str(raw("<b>x</b>")), len(str(raw("<i>é</i>")))]"#);
}
