//! The static checker: what it reports, and what it must not.

use std::path::Path;
use quillo::check::{check_source, Severity};

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new("."))
        .diagnostics
        .iter()
        .map(|d| format!("{}{} {}", d.line, if d.severity == Severity::Warning { " warning:" } else { "" }, d.message))
        .collect()
}

/// Exactly one diagnostic, containing `want`.
fn one(src: &str, want: &str) {
    let d = diags(src);
    assert!(d.len() == 1 && d[0].contains(want), "expected one diagnostic containing {:?}, got {:#?}\n--- source ---\n{}", want, d, src);
}

fn clean(src: &str) {
    let d = diags(src);
    assert!(d.is_empty(), "expected no diagnostics, got {:#?}\n--- source ---\n{}", d, src);
}

#[test]
fn names() {
    one("let x = 1\nprint(y)", "2 unknown name `y`");
    one("let count = 1\nprint(cuont)", "did you mean `count`?");
    one("print(later)\nlet later = 1", "`later` is used before it is defined (line 2)");
    one("fn f() { nope = 1 }", "cannot assign to `nope`: it was never declared");
    one("len = 3", "cannot assign to `len`: it is a builtin");
    one("fn f() = request.path", "`request` only exists in routes");
    // Function bodies run later, so they may use what is defined after them.
    clean("fn f() = later + 1\nlet later = 1\nprint(f())");
    clean("route GET / = request.path");
    clean("server fn me() = request.ip");
}

#[test]
fn calls() {
    one("fn add(a, b) = a + b\nadd(1)", "missing argument `b` in call to `add`");
    one("fn add(a, b) = a + b\nadd(1, 2, 3)", "`add` takes 2 arguments, got 3");
    one("fn f(x: int) = x\nf(\"a\")", "argument `x` of `f` must be int, got str");
    one("upper(1)", "argument 1 of `upper` must be str, got int");
    one("let s = \"x\"\ns(1)", "cannot call `s`: it is str, not a function");
    one("[1, 2] | map(1)", "argument 2 of `map` must be fn, got int");
    one("5.sort()", "argument 1 of `sort` must be list | null, got int");
    one("let n = 5\nn.nonsense", "int has no field `nonsense`");
    one("json.nope(1)", "`json` has no function `nope`");
    one("fn f(x: int = \"a\") = x", "the default of `x` is str, but it must be int");
    clean("fn f(x: int | str) = x\nf(1)\nf(\"a\")\nf(int(\"3\") ?? 0)");
    clean("let xs = [3, 1]\nxs.sort().first()\nxs.len\n[1, 2] | map((x) => x * 2) | sum");
    clean("let m = { greet: (n) => n }\nm.greet(1)\nm.anything");
    clean("print(1, 2, 3)\njson.stringify({ a: 1 }, true)\nlet d = db.query(\"select ?\", 1)");
}

#[test]
fn operators() {
    one("let x = [1] + 1", "cannot add list and int");
    one("let x = \"a\" * 2", "cannot multiply str and int");
    one("let x = {} < 1", "cannot compare map with int");
    one("let x = -\"a\"", "cannot negate str");
    one("let x = 10 / 0", "division by zero");
    one("for x in 5 => x", "cannot loop over int");
    clean("let a = 1 + 2.5\nlet b = \"n\" + 1\nlet c = [1] + [2]\nlet d = { a: 1 } + { b: 2 }\nlet e = 1 < 2.5\nlet f = \"a\" < \"b\"\nprint(a, b, c, d, e, f)");
    // Unknown types never produce operator errors.
    clean("fn f(a, b) = a + b * a - b / a\nprint(f(1, 2))");
    clean("let n = int(\"5\") ?? 0\nprint(n + 1)");
}

#[test]
fn return_and_error_types() {
    one("fn f() -> int = \"x\"", "`f` must return int, but this returns str");
    one("fn f(c) -> int = c ? 1 : \"no\"", "`f` must return int, but this returns str");
    one("fn f(c) -> int { if c { return [] }\n1 }", "`f` must return int, but this returns list");
    one("fn f() -> int ! str = err(42)", "`f` declares errors as `! str`, but this returns err(int)");
    one("fn g() ! int = err(1)\nfn f() -> int ! str = g()? + 1", "`?` passes on err(int), but `f` declares errors as `! str`");
    clean("fn f(s: str) -> int ! str = match int(s) {\n int as n if n >= 0 => n\n _ => err(`bad: ${s}`)\n}");
    clean("fn f(x) -> int = x\nfn g() -> str? = null\nfn h() -> int = int(\"4\")?");
}

#[test]
fn effects() {
    one("fn f() with db = now()", "`now` needs the time effect, but `f` is declared `with db`");
    one("fn load() with http = 1\nfn f() with db = load()", "`load` needs the http effect, but `f` is declared `with db`");
    one("fn f() with db = [1] | map((_) => env(\"X\"))", "`env` needs the env effect");
    one("fn f() with db = http.get(\"http://x\")", "`http.get` needs the http effect");
    let d = diags("fn helper() = env(\"HOME\")\nfn f() with db = helper()");
    assert!(d.len() == 1 && d[0].contains("warning: `helper` may use the env effect"), "{:#?}", d);
    clean("fn f() with db, time = db.query(\"select ?\", now())\nfn g() with db, time, http = f()");
    one("fn f() with db, time = db.query(\"select ?\", now())\nfn g() with db = f()", "`f` needs the time effect, but `g` is declared `with db`");
    clean("fn f() with db = db.one(\"select 1\")\nfn g() = f()");
}

#[test]
fn views_and_components() {
    let d = diags("view Card(title: str, children) = <div>{title}{children}</div>\nlet x = <Card titel=\"x\"/>\nprint(x)");
    assert_eq!(d, vec!["2 <Card> has no parameter `titel`", "2 <Card> needs `title`"]);
    one("view Card(title: str) = <h1>{title}</h1>\nlet x = <Card/>", "<Card> needs `title`");
    one("view Card(n: int) = <h1>{n}</h1>\nlet x = <Card n=\"x\"/>", "`n` of <Card> must be int, got str");
    one("let x = <Nope/>", "unknown name `Nope`");
    one("let x = <button @click=\"no\"></button>", "`@click` needs a function, got str");
    one("component C() = <p></p>\nlet x = C()", "`C` is a component; write <C ... /> instead");
    clean("view Card(title: str, children) = <div>{title}{children}</div>\nlet x = <Card title=\"a\">kids</Card>\nprint(x)");
}

#[test]
fn components_must_run_in_the_browser() {
    one("let todos = []\nfn add(t) { todos = todos | push(t) }\ncomponent C() = <p>{len(todos)}</p>", "`todos` is a server variable that changes");
    one("component C() = <p>{env(\"HOME\")}</p>", "`env` is not available in the browser");
    one("component C() = <p>{db.query(\"x\")}</p>", "`db.query` is not available in the browser");
    one("component C() = <p>{reply 404}</p>", "`reply` only works on the server");
    one("server fn save(x) = x\ncomponent C() = <p>{save(1)}</p>", "`save` waits on the server");
    one("component C() { ~n = 0\n<input bind:value={zz}/> }", "unknown name `zz`");
    clean(
        "let title = \"Todos\"\nserver fn save(x: str) -> list = [x]\nserver fn load() -> list = []\n\
         component C(start: int = 0) {\n ~n = start\n ~items = load()\n ~draft = \"\"\n\
         <div>\n <h1>{title}</h1>\n <button @click={() => { n += 1 }}>+</button>\n\
         <input bind:value={draft}/>\n <button @click={() => { items = save(draft) }}>save</button>\n\
         {for i in items => <p key={i}>{i}</p>}\n </div>\n}",
    );
}

#[test]
fn server_fn_parameters_must_be_sendable() {
    one("server fn f(cb: fn) = cb()", "parameter `cb` of server fn `f` is fn, which the browser cannot send");
}

#[test]
fn unused_variables_are_warnings() {
    let d = diags("fn f() {\n let unused = 1\n let _ok = 2\n 3\n}");
    assert_eq!(d, vec!["2 warning: unused variable `unused` (name it `_unused` if that is intended)"]);
}

#[test]
fn calling_a_variable_that_shadows_a_builtin_is_flagged() {
    let d = diags("fn f(xs) {\n let last = min([1, len(xs)])\n last(xs)\n}");
    assert_eq!(d, vec!["3 warning: `last` here is your variable (line 2), not the builtin `last`; rename the variable to call the builtin"]);
    // Your own function of the same name is fine.
    assert!(diags("fn f(xs) {\n let last = (x) => x\n last(xs)\n}").is_empty());
}

#[test]
fn routes() {
    one("route GET /a/:id/:id = id", "path parameter `id` appears twice");
    one("route GET /x = reply 999", "reply status must be an HTTP status code (100-599), got 999");
    one("route GET /x/:id(int) = id + \"\" + nope", "unknown name `nope`");
    clean("route GET /users/:id(int) ? { page: int = 1 } { json: { name: str } } = { id, page, name, path: request.path }");
    clean("route GET /files/*path = path");
}

#[test]
fn parse_errors_are_diagnostics() {
    one("let = 3", "1 expected a pattern");
}

#[test]
fn symbols_and_references_for_the_editor() {
    let a = check_source("fn add(a: int, b: int = 10) -> int = a + b\nlet total = add(1)\ncomponent C() = <p>{total}</p>", "t.qwe", Path::new("."));
    let names: Vec<_> = a.symbols.iter().map(|s| (s.name.as_str(), s.line, s.detail.as_str())).collect();
    assert!(names.contains(&("add", 1, "fn add(a: int, b: int = …) -> int")), "{:#?}", names);
    assert!(names.contains(&("total", 2, "let total: int")), "{:#?}", names);
    let r = a.references.iter().find(|r| r.name == "add" && r.line == 2).unwrap();
    assert_eq!(r.def, Some(("t.qwe".to_string(), 1)));
    let r = a.references.iter().find(|r| r.name == "len" || r.name == "total" && r.line == 3).unwrap();
    assert_eq!(r.def, Some(("t.qwe".to_string(), 2)));
}

#[test]
fn examples_are_clean() {
    for f in ["examples/hello.qwe", "examples/orders.qwe", "examples/todo.qwe", "examples/notes.qwe", "tests/browser/app.qwe"] {
        let a = quillo::check::check_file(Path::new(f));
        assert!(a.diagnostics.is_empty(), "{}: {:#?}", f, a.diagnostics);
    }
}
