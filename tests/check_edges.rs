//! The checker's rarer diagnostics, each with its exact words and line, and
//! what it says of a whole program: its files, style sheets and classes.

mod support;

use quillo::check::{check_file, check_file_opts, check_source, check_source_opts, Diagnostic, Options, Severity, SymbolKind};
use std::path::{Path, PathBuf};
use support::temp_dir;

fn shown(d: &Diagnostic) -> String {
    let sev = match d.severity {
        Severity::Error => "",
        Severity::Warning => " warning:",
        Severity::Hint => " hint:",
    };
    format!("{}{} {}", d.line, sev, d.message)
}

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().map(shown).collect()
}

fn says(src: &str, want: &[&str]) {
    assert_eq!(diags(src), want, "\n--- source ---\n{}", src);
}

fn clean(src: &str) {
    says(src, &[]);
}

fn strict(src: &str) -> Vec<String> {
    let opts = Options { strict_null: true, ..Default::default() };
    check_source_opts(src, "t.qwe", Path::new("."), opts).diagnostics.iter().map(shown).collect()
}

/// A folder of files for one test, gone when it ends.
struct App(PathBuf);

impl App {
    fn new(name: &str, files: &[(&str, &str)]) -> App {
        let d = temp_dir(&format!("check-{}", name));
        for (f, text) in files {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        App(d.canonicalize().unwrap())
    }

    fn main(&self) -> PathBuf {
        self.0.join("main.qwe")
    }

    /// Each diagnostic with its file's name, its severity and its line.
    fn check(&self, opts: Options) -> Vec<String> {
        let dir = format!("{}/", self.0.display());
        check_file_opts(&self.main(), Default::default(), opts)
            .diagnostics
            .iter()
            .map(|d| format!("{}:{}", d.file.strip_prefix(&dir).unwrap_or(&d.file), shown(d)).replace(&dir, ""))
            .collect()
    }

    fn css(&self) -> Vec<String> {
        self.check(Options { css: true, ..Default::default() })
    }

    fn classes(&self) -> Vec<String> {
        self.check(Options { classes: true, ..Default::default() })
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_diagnostic_prints_its_file_line_and_severity() {
    let d = |line, severity| Diagnostic { file: "a.qwe".into(), line, severity, message: "no".into() };
    assert_eq!(d(3, Severity::Error).to_string(), "a.qwe:3: error: no");
    assert_eq!(d(3, Severity::Warning).to_string(), "a.qwe:3: warning: no");
    assert_eq!(d(3, Severity::Hint).to_string(), "a.qwe:3: hint: no");
    assert_eq!(d(0, Severity::Error).to_string(), "a.qwe: error: no");
}

#[test]
fn a_program_that_cannot_be_read_says_so() {
    let a = check_file(Path::new("/nonexistent/quillo/app.qwe"));
    assert_eq!(a.diagnostics.len(), 1);
    assert_eq!(a.diagnostics[0].line, 0);
    assert_eq!(a.diagnostics[0].severity, Severity::Error);
    assert_eq!(a.diagnostics[0].file, "/nonexistent/quillo/app.qwe");
    assert!(a.diagnostics[0].message.starts_with("cannot read file: "), "{}", a.diagnostics[0].message);
    assert_eq!(a.errors().count(), 1);
}

#[test]
fn the_same_mistake_on_a_line_is_said_once() {
    says("print(y, y)", &["1 unknown name `y`"]);
    says("print(y); print(y)\nprint(y)", &["1 unknown name `y`", "2 unknown name `y`"]);
}

#[test]
fn a_long_map_is_cut_short_in_its_type() {
    let a = check_source("let m = { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7 }\nprint(m.a)", "t.qwe", Path::new("."));
    let s = a.symbols.iter().find(|s| s.name == "m").unwrap();
    assert_eq!(s.detail, "let m: { a: int, b: int, c: int, d: int, e: int, f: int, … }");
    assert_eq!(s.kind, SymbolKind::Variable);
}

#[test]
fn ranges_are_lists_of_ints_and_of_nothing_else() {
    clean("let r = 1..3\nprint(r | map((n) => n + 1))");
    says("let r = 1..3\nprint(r | map((n) => n.name))", &["2 int has no field `name`"]);
    says("let r = \"a\"..\"b\"\nprint(r)", &["1 cannot make a range from str and str"]);
    says("print(true .. 3)", &["1 cannot make a range from bool and int"]);
    says("print(7 / \"a\")\nprint(7 % \"a\")\nprint([1] % 2)", &[
        "1 cannot divide int and str",
        "2 cannot take the remainder of int and str",
        "3 cannot take the remainder of list and int",
    ]);
}

#[test]
fn what_only_the_top_level_may_declare() {
    says("fn f() {\n route GET / = 1\n}", &["2 routes must be declared at the top level"]);
    says("fn f() {\n use \"x.qwe\"\n}", &["2 `use` must be at the top level"]);
    says("fn f() {\n mount /x = \"x.qwe\"\n}", &["2 `mount` must be at the top level"]);
    says("fn f() {\n type Thing = { a: int }\n}", &["2 `type` must be at the top level"]);
    says("fn f() {\n server fn g() = 1\n g()\n}", &["2 `server fn` and `component` must be declared at the top level"]);
    says("fn f() {\n component G() = <p></p>\n 1\n}", &["2 `server fn` and `component` must be declared at the top level"]);
}

#[test]
fn a_service_inside_a_function_is_refused() {
    let d = diags("fn f() {\n service x = \"gone.qwe\"\n}");
    assert_eq!(d.last().unwrap(), "2 `service` must be at the top level", "{:?}", d);
}

#[test]
fn compound_assignment_needs_values_that_add() {
    says("fn f() {\n let x = 1\n x -= \"a\"\n x\n}", &["3 cannot subtract any and str"]);
    says("fn f() {\n let x = [1]\n x += 1\n x\n}", &[]);
}

#[test]
fn a_bare_return_is_null() {
    clean("fn f() {\n return\n}");
    let d = diags("fn f() -> int {\n let x = 1\n print(x)\n return\n}");
    assert_eq!(d.len(), 1, "{:?}", d);
    assert!(d[0].ends_with("`f` must return int, but this returns null"), "{:?}", d);
}

#[test]
fn a_bare_return_is_reported_on_its_own_line() {
    says("fn f() -> int {\n let x = 1\n print(x)\n return\n}", &["4 `f` must return int, but this returns null"]);
}

#[test]
fn functions_inside_functions_keep_their_own_rules() {
    says("fn f() with db {\n fn g() with http = 1\n g()\n}", &[
        "2 `g` needs the http effect, but `f` is declared `with db`",
        "3 `g` needs the http effect, but `f` is declared `with db`",
    ]);
    says("fn f() {\n fn g() -> int { return \"a\" }\n g()\n}", &["2 `g` must return int, but this returns str"]);
    says("fn f() {\n fn g() -> int = 1\n g().name\n}", &["3 int has no field `name`"]);
}

#[test]
fn recursion_gives_what_its_other_branches_give() {
    clean("fn f(n) {\n if n { return g(n) }\n 1\n}\nfn g(n) = f(n)\nprint(f(1))");
    clean("fn f(n) = f(n)\nprint(f(1).x)");
}

#[test]
fn effects_are_followed_through_helpers() {
    says("fn g() = db.one(\"select 1\")\nfn f() with time = g() + g()", &["2 warning: `g` may use the db effect, but `f` is declared `with time`"]);
    says("fn h() = http.get(\"https://a.test/\")\nfn g() = h() + h()\nfn f() with time = g() + h()", &[
        "3 warning: `g` may use the http effect, but `f` is declared `with time`",
        "3 warning: `h` may use the http effect, but `f` is declared `with time`",
    ]);
    says("fn g() = 1\nfn f(xs) with db = xs | env", &["2 `env` needs the env effect, but `f` is declared `with db`"]);
    says(
        "fn g(xs) = xs | upper\nfn f(x) with time {\n let a = g(x)\n a\n}\n",
        &[],
    );
}

#[test]
fn a_call_to_a_service_in_a_function_limited_to_others_is_an_error() {
    let app = App::new("svc-direct", &[
        ("billing.qwe", "server fn invoice() = 1\n"),
        ("main.qwe", "service billing = \"billing.qwe\"\nfn f() with db = billing.invoice()\n"),
    ]);
    assert_eq!(app.check(Options::default()), ["main.qwe:2 `billing.invoice` needs the http effect, but `f` is declared `with db`"]);
}

#[test]
fn a_call_to_a_service_in_a_helper_is_an_http_effect() {
    let app = App::new("svc-effect", &[
        ("billing.qwe", "server fn invoice() = 1\n"),
        ("main.qwe", "service billing = \"billing.qwe\"\nfn load() = billing.invoice()\nfn f() with db = load()\n"),
    ]);
    assert_eq!(app.check(Options::default()), ["main.qwe:3 warning: `load` may use the http effect, but `f` is declared `with db`"]);
}

#[test]
fn a_transaction_that_retries_must_not_send() {
    let w = "the function given to `db.transaction` with retries uses the http effect: it runs again when the transaction conflicts, and what it sent is not taken back; send it after the transaction";
    says("fn g() {\n  db.transaction({ retries: 3 }, () => http.get(\"https://x.test\"))\n}", &[&format!("2 warning: {}", w)]);
    clean("fn g() {\n  db.transaction({ retries: 0 }, () => http.get(\"https://x.test\"))\n}");
    clean("fn g() {\n  db.transaction({ retries: 3 }, () => db.one(\"select 1\"))\n}");
}

#[test]
fn patterns_bind_what_they_match() {
    says("fn f(x) = match x {\n 1 as n => n.name\n _ => 0\n}", &["2 int has no field `name`"]);
    says("fn f(x: int) = match x {\n _ as m => m.name\n}", &["2 int has no field `name`"]);
    says("fn f(x) = match x {\n [a, ..rest] => rest.name\n _ => 0\n}", &["2 list has no field `name`"]);
    clean("fn f(x) = match x {\n [..] => 0\n [a, ..] => a\n { a } as r => r\n _ => 1\n}");
}

#[test]
fn indexing_needs_something_with_items() {
    says("let x = 5\nprint(x[0])", &["2 cannot index int with int"]);
    says("print(\"abc\"[\"a\"])", &["1 cannot index str with str"]);
    clean("print({ a: 1 }[\"a\"], { a: 1 }[\"b\"])");
    says("print({ a: 1 }[\"a\"].name)", &["1 int has no field `name`"]);
    clean("print({ a: 1 }[\"b\"] ?? 2)");
}

#[test]
fn a_reply_status_is_a_number() {
    says("fn f() {\n  reply \"a\"\n}", &["2 reply status must be an int, got str"]);
    clean("fn f(code) {\n  reply code\n}");
}

#[test]
fn a_pipe_binds_looser_than_an_operator() {
    let line = |sym: &str, f: &str| format!("1 `xs | {f}(...) {sym} ...` reads as `xs | ({f}(...) {sym} ...)`: `|` binds looser than `{sym}`, so put the pipe in parentheses: `(xs | {f}(...)) {sym} ...`");
    says("print([1] | find((x) => x == 1) != null)", &[&line("!=", "find")]);
    for sym in ["+", "-", "*", "/", "%", "==", "!=", "<", "<=", ">", ">=", "&&", "||"] {
        says(&format!("print([1] | len {} 1)", sym), &[&line(sym, "len")]);
    }
    // `..` is left alone: it says what the right side is.
    says("print(1 | abs .. 3)", &["1 cannot make a range from fn and int"]);
    says("print(1 | \"x\")", &["1 cannot call `this value`: it is str, not a function"]);
    says("print(1 | (2 + 3))", &["1 cannot call `this value`: it is int, not a function"]);
}

#[test]
fn a_variable_named_like_a_module_hides_it() {
    says("fn f() {\n let json = 5\n json.parse(\"1\")\n}", &["3 `json.parse`: `json` here is int, a variable, which hides the module `json`: rename the variable"]);
    says("fn f(json: str) = json.parse(\"1\")", &["1 `json.parse`: `json` here is str, a variable, which hides the module `json`: rename the variable"]);
    // A map of the program's own is what the call means.
    clean("let stats = { load: () => 1 }\nprint(stats.load())");
    clean("fn f() {\n let json = { parse: (s) => s }\n json.parse(\"1\")\n}");
}

#[test]
fn a_top_level_variable_named_like_a_module_or_builtin_hides_it_in_every_file() {
    let app = App::new("hides", &[
        ("lib.qwe", "let json = 5\nlet first = 7\n"),
        ("main.qwe", "use \"lib.qwe\"\nfn f() = json.parse(\"1\")\nfn g(xs) = first(xs)\n"),
    ]);
    assert_eq!(app.check(Options::default()), [
        "main.qwe:2 `json.parse`: `json` is the program's variable from lib.qwe:1, which hides the module `json` in every file: rename the variable",
        "main.qwe:3 cannot call `first`: it is int, the program's variable from lib.qwe:2, which hides the builtin `first` in every file",
        "lib.qwe:1 warning: `json` hides the module `json` in every file of the program, and main.qwe:2 calls `json.parse`: pick another name",
        "lib.qwe:2 warning: `first` hides the builtin `first` in every file of the program, and main.qwe:3 calls the builtin: pick another name",
    ]);
}

#[test]
fn a_method_needs_a_function_of_that_name() {
    says("let x = true\nprint(x.foo())", &["2 bool has no method `foo`"]);
    says("print(5.foo(1))", &["1 int has no method `foo`"]);
    clean("fn f(x) = x.foo()");
}

#[test]
fn a_local_function_called_as_a_method_is_used() {
    clean("fn f() {\n let adder = (x) => x + 10\n 1.adder()\n}\nprint(f())");
}

#[test]
fn strict_null_looks_at_the_right_side_too() {
    assert_eq!(strict("fn f(n: int?) = 1 + n"), ["1 warning: the right side may be null here (it is int | null)"]);
    assert_eq!(strict("fn f(n: int?) = n + 1"), ["1 warning: the left side may be null here (it is int | null)"]);
}

#[test]
fn removing_a_key_leaves_the_others() {
    clean("print(remove({ a: 1, b: 2 }, \"a\").b)");
    says("print(remove({ a: 1, b: 2 }, \"a\").a)", &["1 this map has no field `a` (did you mean `b`?)"]);
    clean("print(remove({ a: 1 }, \"a\").anything)");
    clean("fn f(k) = remove({ a: 1, b: 2 }, k).c");
    clean("fn f(m: map?) = remove(m, \"a\").c");
}

#[test]
fn truthiness_names_what_it_can() {
    says("fn f(p: { n: int }) = !p.n", &["1 warning: `p.n` counts as true even when it is 0 (unlike JavaScript), so `!p.n` is false: write `p.n == 0`"]);
    says("fn f() = !(1 + 2)", &["1 warning: this value counts as true even when it is 0 (unlike JavaScript), so `!x` is false: write `x == 0`"]);
}

#[test]
fn http_options_are_checked_where_they_are_written() {
    let host = |n: u32, f: &str| format!("{} warning: the host `http.{}` goes to comes from a value: if a visitor can choose it, they can reach this server's own network (localhost, a cloud's metadata service). Pass {{ public_only: true }} as its options (or public_only: false, when it is meant to reach inside)", n, f);
    says("fn f(u) = http.get(u, {}, { nope: 1 })", &[
        "1 `http.get`: unknown option `nope` (the options are redirects, public_only, cache, timeout)",
        &host(1, "get"),
    ]);
    says("fn f(u) = http.post(u, {}, {}, { retries: 2 })", &[
        "1 `http.post`: unknown option `retries` (the options are redirects, public_only, cache, timeout)",
        &host(1, "post"),
    ]);
    // Options made elsewhere may say it; a piped URL is not checked here.
    clean("fn h(u, o) = http.get(u, {}, o)");
    clean("fn i(u) = u | http.get");
    clean("fn g(u) = http.stream(u, (x) => x)");
}

#[test]
fn a_url_written_only_as_a_value_is_not_fixed() {
    let host = |n: u32| format!("{} warning: the host `http.get` goes to comes from a value: if a visitor can choose it, they can reach this server's own network (localhost, a cloud's metadata service). Pass {{ public_only: true }} as its options (or public_only: false, when it is meant to reach inside)", n);
    says("fn f(u) = http.get(`${u}`)", &[&host(1)]);
    clean("fn g() {\n let a = \"https://x.test/\"\n http.get(`${a}`)\n}");
    says("fn h(u, v) = http.get(`${u}${v}/x`)", &[&host(1)]);
}

#[test]
fn sql_through_a_block_and_a_bare_join() {
    let sql = "1 warning: `db.query` with SQL built from values: a value containing a quote can change the query (SQL injection); write `?` in the SQL and pass the values after it";
    says("fn f(n: str) with db = db.query([\"select \", n] | join)", &[sql]);
    clean("fn g() with db = db.query([\"select \", \"x\"] | join)");
    says("fn h(n: str) with db = db.query({ `select ${n}` })", &[sql]);
    clean("fn i() with db = db.query({ \"select 1\" })");
    clean("fn j(c) with db = db.query(\"select \" + { c ? \"a\" : \"b\" })");
    says("fn k(c: str) with db = db.query(\"select \" + { c })", &[sql]);
}

#[test]
fn a_spread_needs_what_it_spreads() {
    says("print({...5})\nprint([...5])\nprint({...[1]})\nprint([...\"a\"])", &[
        "1 `{...x}` spreads a map (or null), but x is int",
        "2 `[...x]` spreads a list (or null), but x is int",
        "3 `{...x}` spreads a map (or null), but x is list",
        "4 `[...x]` spreads a list (or null), but x is str",
    ]);
}

#[test]
fn live_needs_a_server_fn_by_name_in_a_state() {
    says("fn g() = 1\ncomponent C() {\n  state a = live(g)\n  <p>{a}</p>\n}", &["3 `live` needs a server fn: `g` is not one"]);
    says("component C() {\n  state b = live(() => 1)\n  <p>{b}</p>\n}", &["2 `live` needs a server fn by name: state items = live(load_items, ...)"]);
    says("component C() {\n  state c = live()\n  <p>{c}</p>\n}", &["2 missing argument `arg1` in call to `live`"]);
    says("server fn s() = 1\ncomponent C() {\n  let x = live(s)\n  <p>{x}</p>\n}", &["3 in <C>, `live(...)` sets state: state items = live(load_items, ...)"]);
    clean("server fn s() = 1\nfn f() = live(s)");
}

#[test]
fn a_live_loader_through_a_later_call_follows_only_the_service() {
    let app = App::new("live-later", &[
        ("ledger.qwe", "server fn add() = 1\n"),
        ("main.qwe", "service ledger = \"ledger.qwe\"\nserver fn load() = ledger.later.add()\ncomponent C() {\n  state n = live(load)\n  <p>{n}</p>\n}\n"),
    ]);
    let d = app.check(Options::default());
    assert_eq!(d.len(), 1, "{:?}", d);
    assert!(d[0].starts_with("main.qwe:4 warning: `load` gets its data from the service `ledger`"), "{:?}", d);
}

#[test]
fn markup_attributes_are_checked() {
    says("view Card() = <p></p>\nprint(<Card @click={() => 1}/>)", &["2 <Card> is a view; `@click` handlers go on HTML elements"]);
    says("component C() {\n state a = \"\"\n <input bind:focus={a}/>\n}", &["3 `bind:focus` is not supported (use bind:value or bind:checked)"]);
    says("component C() {\n <input bind:value=\"x\"/>\n}", &["2 `bind:value` needs a variable: bind:value={name}"]);
    says("let g = 1\ncomponent C() {\n <input bind:value={g}/>\n}", &["3 `bind:value` needs component state or a local variable, like ~g"]);
    says("fn f(Foo: int) = <Foo/>", &["1 <Foo> is not a view: it is int"]);
    says("server fn Save() = 1\ncomponent C() = <Save/>", &["2 <Save> is a server fn, not a view (in <C>)"]);
}

#[test]
fn nested_shapes_say_where_they_differ() {
    says("fn f(p: { a: { b: int } }) = p\nprint(f({ a: { b: \"x\" } }))", &["2 argument `p` of `f` has `a` that has `b` as str, but it must be int"]);
    says("fn f(p: list<{ a: int }>) = p\nprint(f([{ a: \"x\" }]))", &["2 argument `p` of `f` holds an item that has `a` as str, but it must be int"]);
    says("fn f(p: { a: list<int> }) = p\nprint(f({ a: [\"x\"] }))", &["2 argument `p` of `f` has `a` that must be list<int>, got list<str>"]);
}

#[test]
fn state_may_change_type_through_its_handlers() {
    clean("component C() {\n state n = 0\n <button @click={() => { n = 1\n  n += 2\n  n -= 1 }}>{n}</button>\n}");
}

#[test]
fn a_field_set_on_something_with_no_fields() {
    says("component C() {\n let n = 5\n <p @mount={(el) => { n.x = 1 }}></p>\n}", &["3 int has no field `x` to set: only the page's objects (an element, an event) do"]);
}

#[test]
fn assigning_to_a_global_from_a_component() {
    says("let g = 1\ncomponent C() {\n <button @click={() => { g = 2 }}>x</button>\n}", &["3 <C> runs in the browser, but `g` is a server variable that changes; pass it as a prop or read it with a `server fn`"]);
}

#[test]
fn route_groups_bind_their_patterns() {
    clean("routes /admin { headers: { host: h } } {\n route GET /a = h\n}");
    says("routes /admin { headers: { zzqqxx: \"x\" } } {\n route GET /a = zzqqxx\n}", &["2 unknown name `zzqqxx`"]);
}

#[test]
fn a_services_options_are_checked() {
    let app = App::new("svc-opts", &[("billing.qwe", "server fn f() = 1\n")]);
    let run = |src: &str| {
        std::fs::write(app.main(), src).unwrap();
        app.check(Options::default())
    };
    assert_eq!(run("service billing = \"billing.qwe\" { timout: \"2s\" }\n"), ["main.qwe:1 service billing: no option `timout` (did you mean `timeout`?)"]);
    assert_eq!(run("service billing = \"billing.qwe\" { zebra: \"2s\" }\n"), ["main.qwe:1 service billing: no option `zebra` (timeout, retries, failures, cooldown)"]);
}

#[test]
fn a_types_defaults_are_of_its_fields() {
    says("type Thing = { a: int = \"x\", b: str = \"y\", c: int }\nprint(1)", &["1 the default of `a` in `Thing` is str, which is not int"]);
}

#[test]
fn a_mount_that_is_not_there_is_an_error_at_the_mount() {
    let app = App::new("mount-gone", &[("main.qwe", "mount /x = \"gone.qwe\"\n")]);
    let d = app.check(Options::default());
    assert_eq!(d.len(), 1, "{:?}", d);
    assert!(d[0].starts_with("main.qwe:1 cannot read ") && d[0].ends_with("gone.qwe: no such file"), "{:?}", d);
}

#[test]
fn a_used_folder_cannot_be_read() {
    let app = App::new("use-dir", &[("main.qwe", "use \"lib.qwe\"\n"), ("lib.qwe/x", "")]);
    let d = app.check(Options::default());
    assert_eq!(d.len(), 1, "{:?}", d);
    assert!(d[0].starts_with("main.qwe:1 cannot read "), "{:?}", d);
}

#[test]
fn newer_syntax_in_a_used_file_is_noted() {
    let app = App::new("newer", &[("lib.qwe", "let x = 1 # 2\n"), ("main.qwe", "use \"lib.qwe\"\n")]);
    let a = check_file(&app.main());
    assert_eq!(a.syntax_errors.len(), 1);
    assert!(a.syntax_errors[0].0.ends_with("lib.qwe") && a.syntax_errors[0].1 == 1, "{:?}", a.syntax_errors);
}

#[test]
fn names_of_a_file_that_does_not_parse_are_still_known() {
    let app = App::new("unparsed", &[
        ("lib.qwe", "server fn saved() = 1\nfn ok() = 2\nlet (x = 3\nview Row() = <p></p>\ncomponent Box() = <p></p>\ntype Mine = { a: int }\nlet 1x = 2\nfn\n"),
        ("main.qwe", "use \"lib.qwe\"\nprint(saved(), ok(), <Row/>, <Box/>)\nprint(zzqqxx())\n"),
    ]);
    let d = app.check(Options::default());
    assert_eq!(d.len(), 2, "{:?}", d);
    assert!(d[0].starts_with("lib.qwe:3 "), "{:?}", d);
    assert_eq!(d[1], "main.qwe:3 unknown name `zzqqxx`");
}

// ---- classes ------------------------------------------------------------------

#[test]
fn a_class_that_nothing_writes_is_warned_about() {
    let app = App::new("css-unused", &[(
        "main.qwe",
        "css(\".used { color: red }\n.never { color: blue }\n.q-own { color: red }\")\nroute GET / = <p.used>x</p>\n",
    )]);
    assert_eq!(app.css(), ["main.qwe:2 warning: `.never` is styled here, but nothing in the program writes it"]);
    assert_eq!(app.check(Options::default()), Vec::<String>::new());
}

#[test]
fn a_program_may_write_a_class_in_many_ways() {
    let app = App::new("css-written", &[
        ("svc.qwe", "server fn f() = \"from-service\"\n"),
        (
            "main.qwe",
            r#"service svc = "svc.qwe"
css(".plain { a: b } .tmpl { a: b } .card-big { a: b } .matched { a: b } .alt { a: b } .bound { a: b } .rec { a: b } .list { a: b } .from-service { a: b } .from-script { a: b } .elsewhere-x { a: b }")
/* set elsewhere: elsewhere- */
fn kind(x) = match x {
    "matched" => 1
    "alt" | "other" => 2
    "bound" as b => b
    { a: "rec" } => 3
    ["list", ..] => 4
    _ => 5
}
fn size(s) = `tmpl card-${s}`
fn tail(s) = `${s} tail`
route GET / = "plain"
"#,
        ),
        ("public/app.js", "el.classList.add('from-script')\n"),
        ("public/deep/more.mjs", "// tmpl\n"),
        ("public/readme.txt", "never counted\n"),
        ("public/sheet.css", "/* set elsewhere: elsewhere- */\n.elsewhere-y { a: b }\n.lonely { a: b }\n"),
    ]);
    assert_eq!(app.css(), ["public/sheet.css:3 warning: `.lonely` is styled here, but nothing in the program writes it"]);
}

#[test]
fn a_file_that_does_not_parse_may_write_any_class_it_holds() {
    let app = App::new("css-unparsed", &[
        ("lib.qwe", "view V() = <p.hidden>{\n"),
        ("main.qwe", "use \"lib.qwe\"\ncss(\".hidden { a: b } .other { a: b }\")\n"),
    ]);
    let d = app.css();
    assert!(d.iter().any(|m| m == "main.qwe:2 warning: `.other` is styled here, but nothing in the program writes it"), "{:?}", d);
    assert!(!d.iter().any(|m| m.contains(".hidden")), "{:?}", d);
}

#[test]
fn a_class_with_no_rule_is_a_hint_and_a_near_miss_a_warning() {
    let app = App::new("classes", &[(
        "main.qwe",
        "css(\".chip { a: b }\")\nroute GET /a = <p.chip class=\"chipp dashboard\">x</p>\nroute GET /b = <p class={\"chip\"}>y</p>\n",
    )]);
    assert_eq!(app.classes(), [
        "main.qwe:2 warning: no style sheet has a rule for `.chipp` (did you mean `chip`?)",
        "main.qwe:2 hint: no style sheet has a rule for `.dashboard`",
    ]);
    assert_eq!(app.css(), ["main.qwe:2 warning: no style sheet has a rule for `.chipp` (did you mean `chip`?)"]);
}

#[test]
fn classes_are_read_from_every_form_of_a_class_attribute() {
    let app = App::new("classes-forms", &[(
        "main.qwe",
        r#"css(".ok { a: b }")
fn a(on) = <p class={on ? "ok" : "okk"}>1</p>
fn b(x) = <p class={match x { 1 => "okk", _ => "ok" }}>2</p>
fn c(on) = <p class={on && "okk"}>3</p>
fn d(x) = <p class={x || "okk"}>4</p>
fn e(x) = <p class={x ?? "okk"}>5</p>
fn f() = <p class={["ok", "okk"]}>6</p>
fn g(x) = <p class={`okk ${x}ok glued${x} ok`}>7</p>
fn h(x) = <p class={x}>8</p>
fn i(on) = <p class={on ? "ok" : "ok"}>9</p>
"#,
    )]);
    let w = |n: u32| format!("main.qwe:{} warning: no style sheet has a rule for `.okk` (did you mean `ok`?)", n);
    // `x || "okk"` is the habit `quillo check` warns about too (FEEDBACK19.md,
    // Q-150; branch feedback19-fixes).
    let blank = "main.qwe:5 warning: this value counts as true even when it is \"\" (unlike JavaScript), so `||` keeps it unless it is null: write `filled(x) ?? ...`, or `x ?? ...` to replace only null".to_string();
    assert_eq!(app.classes(), [w(2), w(3), w(4), w(5), blank, w(6), w(7), w(8)]);
}

#[test]
fn classes_in_an_if_with_blocks_are_read() {
    let app = App::new("classes-if", &[(
        "main.qwe",
        "css(\".ok { a: b }\")\nfn a(on) = <p class={if on { \"ok\" } else { \"okk\" }}>1</p>\n",
    )]);
    assert_eq!(app.classes(), ["main.qwe:2 warning: no style sheet has a rule for `.okk` (did you mean `ok`?)"]);
}

#[test]
fn a_linked_sheet_may_style_anything() {
    let app = App::new("classes-linked", &[(
        "main.qwe",
        "css(\".chip { a: b }\")\nroute GET / = <html><head><link rel=\"stylesheet\" href=\"https://cdn.test/x.css\"/></head><body><p.chipp>x</p></body></html>\n",
    )]);
    assert_eq!(app.classes(), Vec::<String>::new());
}

#[test]
fn a_local_link_does_not_stop_the_class_check() {
    let app = App::new("classes-local", &[(
        "main.qwe",
        "css(\".chip { a: b }\")\nroute GET / = <html><head><link rel=\"stylesheet\" href=\"/x.css\"/></head><body><p.chipp>x</p></body></html>\n",
    )]);
    assert_eq!(app.classes(), ["main.qwe:2 warning: no style sheet has a rule for `.chipp` (did you mean `chip`?)"]);
}

#[test]
fn scripts_and_sheets_under_public_excuse_a_class() {
    let app = App::new("classes-public", &[
        ("main.qwe", "route GET / = <p class=\"chipper set-by-sheet shown\">x</p>\n"),
        ("public/a.css", "/* set elsewhere: set-by */\n.chip { a: b }\n"),
        ("public/b.js", "el.classList.add('shown')\n"),
        ("public/deep/c.txt", "text\n"),
    ]);
    assert_eq!(app.classes(), ["main.qwe:1 hint: no style sheet has a rule for `.chipper`"]);
}

// ---- what is left -------------------------------------------------------------

#[test]
fn a_list_of_either_holds_either() {
    says("fn f(c) {\n let m = c ? [1] : [\"a\"]\n m | map((x) => x.name)\n}", &["3 int | str has no field `name`"]);
    says("fn f() = ([1] + [\"a\"]) | map((x) => x.name)", &["1 int | str has no field `name`"]);
}

#[test]
fn a_top_level_let_may_take_a_value_apart() {
    clean("let [a, b] = [1, 2]\nprint(a + b)");
    clean("let { a } = { a: 1 }\nprint(a)");
    clean("let [c, ..d] = [1, 2]\nprint(c, d)");
    clean("let { p: { q } } = { p: { q: 1 } }\nprint(q)");
    clean("let { p: int } = { p: 1 }\nprint(p)");
    clean("let [a] | [b] = [1]\nprint(a, b)");
    clean("let [a] as all = [1]\nprint(a, all)");
    clean("let 1 = 1\nlet _ = 2\nlet int = 3\nprint(1)");
    says("let [a, a2] = [1, 2]\nlet [a, b] = [3, 4]\nfn a() = 1\nprint(a, a2, b)", &["3 `a` is defined twice, at t.qwe:1 and t.qwe:3: the second replaces the first everywhere, so rename one"]);
}

#[test]
fn a_top_level_return_or_try_is_checked_too() {
    clean("return 1");
    clean("print(int(\"a\")?)");
    clean("fn f(x: int) = x?\nprint(f(1) + 1)");
    clean("return");
}

#[test]
fn a_branch_with_no_value_is_null() {
    says("fn f(on) -> int = if on { 1 }", &["1 `f` must return int, but this returns null"]);
}

#[test]
fn a_module_called_as_a_function_is_not_one() {
    says("fn g() = http(\"x\")\nfn f() with time = g()", &["1 cannot call `http`: it is map, not a function"]);
}

#[test]
fn a_call_of_later_on_something_else_is_a_plain_call() {
    clean("fn f(a) = a.later.b()");
}

#[test]
fn http_without_its_url_is_only_that_mistake() {
    says("fn f() = http.get()", &["1 missing argument `arg1` in call to `http.get`"]);
    clean("fn f(u) = u | http.get({})");
}

#[test]
fn js_without_a_name_says_what_is_missing() {
    says("print(js())", &["1 missing argument `arg1` in call to `js`"]);
    says("print(js_new())", &["1 missing argument `arg1` in call to `js_new`"]);
}

#[test]
fn a_live_loader_may_call_anything_it_is_given() {
    clean("server fn load(x) = x.y.later.z()\nserver fn other(x) = x.y.z()\nserver fn third(x) = x()()\ncomponent C() {\n state a = live(load)\n state b = live(other)\n state c = live(third)\n <p>{a}{b}{c}</p>\n}");
}

#[test]
fn a_response_has_its_three_fields_and_no_others() {
    clean("fn f() {\n let r = respond(200, \"x\")\n [r.status, r.headers, r.body]\n}");
    says("fn f() {\n let r = respond(200, \"x\")\n r.name\n}", &["3 response has no field `name`"]);
}

#[test]
fn a_helper_called_through_a_call_counts_for_nothing() {
    clean("fn g(f) = f()()\nfn h() with time = g(() => () => 1)");
}

#[test]
fn only_the_lambda_in_its_place_is_typed() {
    says("fn f() = sort_by([1], (x) => x.name, (y) => y.name)", &["1 int has no field `name`", "1 `sort_by` takes 2 arguments, got 3"]);
    says("fn f() = sort_by([1], (x) => x.name)", &["1 int has no field `name`"]);
}

#[test]
fn a_list_that_is_never_empty_has_a_first_and_a_max() {
    assert_eq!(strict("fn shout(n: int) = n\nfn f(xs: list<int>) = shout(first(xs))"), ["2 warning: argument `n` of `shout` may be null here (it is int | null), but it must be int"]);
    assert_eq!(strict("fn shout(n: int) = n\nfn f(xs: list<int>) = shout(first(xs + [1]))"), Vec::<String>::new());
    assert_eq!(strict("fn shout(n: int) = n\nfn f(xs: list<int>) = shout(first([1] + xs))"), Vec::<String>::new());
    assert_eq!(strict("fn shout(n: int) = n\nfn f(xs: list<int>) = shout(max(xs + [0]))"), Vec::<String>::new());
    assert_eq!(strict("fn shout(n: int) = n\nfn f(xs: list<int>, ys: list<int>) = shout(max(xs + ys))"), ["2 warning: argument `n` of `shout` may be null here (it is int | null), but it must be int"]);
}

#[test]
fn an_assignment_anywhere_makes_a_variable_unknown() {
    clean("fn f(el) {\n let x = 1\n el.v = { x = \"a\"\n 1 }\n x.name\n}");
    clean("let x = 1\nreturn { x = \"a\"\n x.name }");
    says("let o = 1\nlet x = 1\no.v = { x = \"a\"\n 2 }\nprint(x.name)", &["3 cannot set `v`: only the page's objects (an element, an event) have fields that are set, in a component"]);
    says("let x = 1\nlet o = { { x = \"a\"\n 2 } }\no.v = 2\nprint(x.name)", &["4 cannot set `v`: only the page's objects (an element, an event) have fields that are set, in a component"]);
    says("let x = 1\nreturn { x.name }", &["2 int has no field `name`"]);
}

#[test]
fn a_view_counts_as_what_it_may_put_on_the_page() {
    let p = |n: &str| format!("2 <{}>, whose outer element is a <div>, cannot go inside a <p>: the browser ends the <p> before it, so the page is not what was written, and its components are made again in the browser; make the <p> a <div>, or use a <span>", n);
    says("view A(on: bool = true) = on && <div></div>\nview V() = <p><A/></p>", &[&p("A")]);
    says("view A(on: bool = true) = on || <div></div>\nview V() = <p><A/></p>", &[&p("A")]);
    says("view A() = [<div></div>, <div></div>]\nview V() = <p><A/></p>", &[&p("A")]);
    says("view A() {\n let _x = 1\n}\nview V() = <p><A/></p>", &[]);
}

#[test]
fn a_view_of_views_is_followed_only_a_few_deep() {
    let chain = |n: usize| {
        let mut s = String::from("view V0() = <div></div>\n");
        for i in 1..n {
            s.push_str(&format!("view V{}() = <V{}/>\n", i, i - 1));
        }
        s.push_str(&format!("view Top() = <p><V{}/></p>\n", n - 1));
        s
    };
    let d = diags(&chain(7));
    assert_eq!(d.len(), 1, "{:?}", d);
    assert!(d[0].starts_with("8 <V6>, whose outer element is a <div>, cannot go inside a <p>"), "{:?}", d);
    clean(&chain(9));
}

#[test]
fn a_chain_of_replies_counts_as_nesting() {
    let src = format!("fn f() = {}200\n", "reply ".repeat(600));
    // On a stack the size quillo gives its work: a test thread's is too small for this in a debug build.
    std::thread::Builder::new().stack_size(64 << 20).spawn(move || says(&src, &["1 the code nests too deeply (more than 256 levels)"])).unwrap().join().unwrap();
}

#[test]
fn a_long_chain_of_replies_is_refused_not_a_crash() {
    let app = App::new("reply-chain", &[("main.qwe", &format!("fn f() = {}200\n", "reply ".repeat(30_000)))]);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(app.main()).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stderr).contains("nests too deeply"));
}

#[test]
fn a_function_from_a_plugin_is_not_a_view() {
    let app = App::new("plugin-tag", &[
        ("kv.qwer", "pub fn get(key: &str) -> Option<String> { let _ = key; None }\n"),
        ("main.qwe", "import \"kv.qwer\" use get as Getter\nprint(<Getter/>)\n"),
    ]);
    assert_eq!(app.check(Options::default()), ["main.qwe:2 <Getter> is a builtin function, not a view"]);
}

#[test]
fn a_component_called_as_a_function_is_still_read_through() {
    says("component C() {\n state n = 0\n <button @click={() => { n = 1 }}>{n}</button>\n}\nprint(C())", &["5 `C` is a component; write <C ... /> instead of calling it"]);
}
