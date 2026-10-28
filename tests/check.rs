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
    clean("let m = { greet: (n) => n }\nm.greet(1)\nm.greet");
    // A map whose fields are known has no others: a misspelled key would read as null.
    one("let m = { greet: (n) => n }\nm.anything", "this map has no field `anything` (it has greet)");
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
    // `http` goes from the page, once it has arrived: from handlers.
    one("component C() = <p>{http.get(\"https://x.test/\").status}</p>", "`http.get` in <C> runs in the browser");
    one("component C() { ~r = http.get(\"https://x.test/\")\n<p>{r}</p> }", "`http.get` in <C> runs in the browser");
    one("fn load() = http.get(\"https://x.test/\").json\ncomponent C() { ~r = load()\n<p>{r}</p> }", "`load` calls `http`");
    one("component C() = <button @click={() => http.stream(\"https://x.test/\", (p) => p)}>x</button>", "`http.stream` is the server's");
    clean("fn load(q) = http.post(\"https://x.test/\", { q: q }).json\ncomponent C() {\n ~r = null\n <button @click={() => { r = load(1) }}>{r}</button>\n}");
    clean("component C() {\n ~r = null\n <input type=\"file\" @change={(e) => { r = http.put(\"https://bucket.test/k\", e.target.files[0]).status }}/>\n}");
    // Variables named PUBLIC_... say the browser may see them.
    clean("component C() = <p>{env(\"PUBLIC_MAP_TOKEN\") ?? \"none\"}</p>");
    one("component C() = <p>{env(\"MAP_TOKEN\")}</p>", "name the variable PUBLIC_");
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
    assert_eq!(d, vec![
        "3 warning: `last` here is your variable (line 2), not the builtin `last`; rename the variable to call the builtin",
        "3 cannot call `last`: it is int, not a function",
    ]);
    // Your own function of the same name is fine.
    assert!(diags("fn f(xs) {\n let last = (x) => x\n last(xs)\n}").is_empty());
}

#[test]
fn a_global_can_take_a_builtins_name() {
    assert!(diags("let first = { a: 1 }\nprint(first.a)\nfn f() = first.a").is_empty(), "{:?}", diags("let first = { a: 1 }\nprint(first.a)"));
    assert!(diags("let t = { status: 200 }\nprint(t.status)").is_empty());
}

#[test]
fn javascript_truthiness_is_flagged_where_it_differs() {
    // In Quillo only null, false and errors are false: "" and 0 are true.
    one("fn f(name: str) = name || \"anonymous\"", "1 warning: `name` counts as true even when it is \"\" (unlike JavaScript), so `||` keeps it: write `name != \"\" ? name : ...`");
    one("fn f(name: str | null) = name || \"x\"", "or `name ?? ...` to replace only null");
    one("view Badge(n: int) = <b>{n}</b>\nview V(count: int) = <p>{count && <Badge n={count}/>}</p>", "2 warning: `count` counts as true even when it is 0 (unlike JavaScript), so `&&` always goes on: write `count != 0 && ...`");
    one("fn f(s: str) = !s", "so `!s` is false: write `s == \"\"`");
    one("fn f(s: str) = s ? 1 : 2", "so this condition holds: write `s != \"\"`");
    one("fn f(n: float) { if n { 1 } else { 2 } }", "write `n != 0`");
    one("fn f(p: map) {\n let n = len(p)\n !n\n}", "3 warning: `n` counts as true even when it is 0");
    // Not where the meaning is the same, or the type is not known.
    clean("fn f(ok: bool, x) = (ok || x) && !ok ? x : null");
    clean("fn f(x: int | null) = x ?? 0");
    clean("fn f(u: map) = u.name || \"x\"");
    clean("fn f(xs: list) = xs && len(xs)");
    clean("fn f(s: str) = s != \"\" ? s : \"x\"");
    // A value that may be null is tested for null first: `null > 0` fails.
    one("let i = index_of(\"abc\", \"a\")\nif i { print(i) }", "write `i != null && i != 0`, or `i != null` to test only for null");
    one("fn f(n: int | null) = !n", "write `n == null || n == 0`");
    one("fn f(n: int | null) = n && 1", "write `n != null && n != 0 && ...`");
}

#[test]
fn inferred_returns_count_return_and_try() {
    // `return 1` makes `f` int | str, so `* 2` is not certainly wrong.
    clean("fn f(x) { if x { return 1 }; \"a\" }\nprint(f(true) * 2)");
    // `?` can make `g` give back an error, which has `.error`.
    clean("fn g(s) { let n = int(s)?; n }\nprint(g(\"x\").error)\nprint(g(\"2\") ?? 0)");
    clean("let h = (s) => { let n = int(s)?\n n }\nprint(h(\"x\").error)");
    one("fn f() { return \"a\" }\nprint(f() * 2)", "cannot multiply str and int");
}

#[test]
fn a_line_starting_with_minus_is_its_own_statement() {
    one("let a = 5\nlet b = 2\nlet total = a\n-b\nprint(total)", "4 warning: a line starting with `-` is a statement of its own");
    clean("let a = 5\nlet b = 2\nlet total = a\n  - b\nprint(total)");
    clean("fn f(x) {\n let y = x * 2\n -y\n}\nprint(f(1))");
}

#[test]
fn record_patterns_with_a_union_bind_the_field() {
    clean("match { page: 3 } {\n { page: int | null } => print(page + 1)\n _ => print(0)\n}");
    one("match { page: 3 } {\n { page: int | null } => upper(page)\n _ => print(0)\n}", "argument 1 of `upper` must be str, got int | null");
}

#[test]
fn spread_attributes() {
    one("let n = 5\nlet x = <p {...n}></p>", "`{...}` in <p> needs a map, got int");
    // A spread may supply required props, so they are not reported missing.
    clean("view Card(title: str) = <b>{title}</b>\nlet props = { title: \"t\" }\nprint(<Card {...props}/>)");
    one("view Card(title: str) = <b>{title}</b>\nlet props = { title: \"t\" }\nprint(<Card {...props} titel=\"x\"/>)", "<Card> has no parameter `titel`");
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
    for f in ["app/main.qwe", "tests/programs/hello.qwe", "tests/programs/orders.qwe", "tests/programs/todo.qwe", "tests/programs/notes.qwe", "tests/browser/app.qwe"] {
        let a = quillo::check::check_file(Path::new(f));
        assert!(a.diagnostics.is_empty(), "{}: {:#?}", f, a.diagnostics);
    }
}

#[test]
fn sql_built_from_values_is_a_warning() {
    one("fn f(n: str) with db = db.query(`select * from t where n = '${n}'`)", "1 warning: `db.query` with SQL built from values");
    one("fn f(n: str) with db = db.exec(\"delete from t where n = '\" + n + \"'\")", "SQL injection");
    one("fn f(n: str) with db = db.one(sql`select * from t where n = '${n}'`)", "SQL injection");
    clean("fn f(n: str) with db = db.query(\"select * from t where n = ?\", n)");
    clean("fn f() with db = db.query(sql`select * from t`)");
    // Names put in with sql_name and sql_param are checked as they are made.
    clean("fn f(t: str, cols: list) with db = db.exec(`insert into ${sql_name(t)} (${sql_name(cols)}) values (${sql_param(cols)})`, {})");
    clean("fn f(t: str, k: str) with db = db.exec(`alter table ${sql_name(t)} add column ${sql_name(k)} ${k == \"n\" ? \"NUMERIC\" : \"TEXT\"}`)");
    one("fn f(t: str, k: str) with db = db.exec(`alter table ${sql_name(t)} add column ${k}`)", "SQL injection");
    // Fixed text chosen by `match` or `if`, also through a `let`.
    clean("fn f(sort: str) with db {\n  let order = match sort { \"new\" => \"t.created desc\", _ => \"score desc\" }\n  db.query(\"select * from t order by \" + order)\n}");
    clean("fn f(sort: str) with db = db.query(\"select * from t order by \" + (sort == \"new\" ? \"created\" : \"score\") + \" desc\")");
    clean("let order = \"score desc\"\nfn f() with db = db.query(`select * from t order by ${order}`)");
    one("fn f(sort: str) with db {\n  let order = sort\n  db.query(\"select * from t order by \" + order)\n}", "SQL injection");
    one("fn f(sort: str) with db {\n  let order = \"score\"\n  order = sort\n  db.query(\"select * from t order by \" + order)\n}", "SQL injection");
    one("fn f(order: str) with db = db.query(\"select * from t \" + \"order by \" + order)", "SQL injection");
}

#[test]
fn sql_built_from_values_is_found_however_it_reaches_the_query() {
    // What the review wrote, each missed then: the SQL built in a `let`
    // first, a connection from `db.connect`, the pipe form, and `join`.
    one("fn f(n: str) with db {\n    let q = `select * from t where n = '${n}'`\n    db.query(q)\n}", "`db.query` with SQL built from values");
    one("fn f(n: str) with db {\n    let q = \"select * from t where n = '\" + n\n    let r = q + \"'\"\n    db.one(r)\n}", "SQL injection");
    one("fn f(n: str) with db {\n    let q = \"select * from t\"\n    q += \" where n = '\" + n + \"'\"\n    db.query(q)\n}", "SQL injection");
    one("fn f(n: str) with db {\n    let a = db.connect(\"a.db\")\n    a.query(`select * from t where n = '${n}'`)\n}", "`a.query` with SQL built from values");
    one("fn f(n: str) with db = db.connect(\"a.db\").exec(\"delete from t where n = \" + n)", "SQL injection");
    one("fn f(n: str) with db = `select * from t where n = '${n}'` | db.query", "`db.query` with SQL built from values");
    one("fn f(n: str) with db {\n    let a = db.connect(\"a.db\")\n    `select * from t where n = '${n}'` | a.query\n}", "`a.query` with SQL built from values");
    one("fn f(n: str) with db = db.query(join([\"select * from t where n = '\", n, \"'\"], \"\"))", "SQL injection");
    one("fn f(n: str) with db = db.query([\"select * from t where n =\", n] | join(\" \"))", "SQL injection");
    // Text written in the program, kept in variables, joined or piped, and
    // names made with sql_name, are not values.
    clean("fn f(n: str) with db {\n    let cols = \"id, name\"\n    let q = `select ${cols} from t where n = ?`\n    db.query(q, n)\n}");
    clean("fn f(t: str, n: str) with db {\n    let q = join([\"select * from\", sql_name(t), \"where n = ?\"], \" \")\n    db.query(q, n)\n}");
    clean("fn f(n: str) with db {\n    let a = db.connect(\"a.db\")\n    \"select * from t where n = ?\" | a.query(n)\n}");
    clean("fn f(n: str) with db = `select * from t where n = ${\"?\"}` | db.query(n)");
    // A piped value is the SQL, not the argument after it.
    clean("fn f(n: str) with db = \"select * from t where n = ?\" | db.query(`${n}`)");
    // A variable of the same name elsewhere is another variable.
    clean("fn g(n: str) = `n is ${n}`\nfn f(n: str) with db {\n    let q = \"select 1\"\n    db.query(q)\n}\nfn h(n: str) { let q = `x ${n}`\nq }");
}

#[test]
fn names_in_sql_are_checked() {
    let run = |src: &str| match quillo::eval_str(src) {
        Ok(v) => v.display().to_string(),
        Err(e) => e.to_string(),
    };
    assert_eq!(run(r#"sql_name("o_pitch")"#), "\"o_pitch\"");
    assert_eq!(run(r#"sql_name(["f_a", "f_b"])"#), "\"f_a\", \"f_b\"");
    assert_eq!(run(r#"sql_param(["title", "f_a"])"#), ":title, :f_a");
    for bad in [r#"sql_name("x\"; drop table users; --")"#, r#"sql_name("1x")"#, r#"sql_name("")"#, r#"sql_param("a b")"#] {
        assert!(run(bad).contains("is not a name SQL can take") || run(bad).contains("takes a name"), "{}: {}", bad, run(bad));
    }
}

#[test]
fn lists_know_what_they_hold() {
    // A lambda given to a builtin takes what the list holds.
    one("fn f() = [1, 2] | map((x) => x.name)", "int has no field `name`");
    one("fn f() = [\"a\"] | filter((s) => s > 1)", "cannot compare str with int");
    one("fn f(xs: list<int>) = xs | reduce(0, (acc, x) => acc + x.size)", "int has no field `size`");
    one("fn f() { for x in [1, 2] => x.name }", "int has no field `name`");
    // What builtins give back: an item, the same list, a list of results.
    one("fn f() = first([1]).name", "int has no field `name`");
    one("fn f() = [1, 2] | map((x) => str(x)) | map((s) => s.size)", "str has no field `size`");
    one("fn f() = split(\"a b\", \" \") | map((w) => w + 1 - 1)", "cannot subtract str and int");
    // Unknown lists stay unknown; mixed ones are either.
    clean("fn f(xs) = xs | map((x) => x.name)");
    clean("fn f() = [1, \"a\"] | map((x) => x + 1)");
    clean("fn f(xs) = ([1] + xs) | map((x) => x.name)");
}

#[test]
fn maps_know_their_fields() {
    one("fn f() {\n let p = { title: \"x\" }\n p.titel\n}", "3 this map has no field `titel` (did you mean `title`?)");
    one("fn f() = merge({ a: 1 }, { b: 2 }).c", "has no field `c`");
    one("fn f() = set({ a: 1 }, \"b\", 2).c", "has no field `c`");
    one("fn f() = [{ id: 1 }] | map((r) => r.idd)", "did you mean `id`?");
    // A field one branch has is there; maps that grow, or come from elsewhere, are open.
    clean("fn f(c) {\n let m = c ? { a: 1 } : { b: 2 }\n [m.a, m.b]\n}");
    clean("fn f(x) = merge({ a: 1 }, x).anything");
    clean("fn f() {\n let m = {}\n m.anything\n}");
    clean("fn f() = { a: [1] }.len");
    clean("fn f(rows) = rows | map((r) => r.anything)");
}

#[test]
fn annotations_say_what_lists_and_maps_hold() {
    one("fn total(xs: list<int>) = sum(xs)\nfn f() = total([\"a\"])", "argument `xs` of `total` must be list<int>, got list<str>");
    one("fn tl(p: { title: str }) = p.title\nfn f() = tl({ name: \"x\" })", "argument `p` of `tl` has no field `title`");
    one("fn tl(p: { title: str }) = p.title\nfn f() = tl({ title: 1 })", "has `title` as int, but it must be str");
    one("fn tl(p: { title: str }) = p.titel", "this map has no field `titel`");
    one("fn tl(ps: list<{ id: int }>) = ps | map((p) => p.di)", "this map has no field `di`");
    // Optional fields may be missing; extra ones are fine.
    clean("fn tl(p: { title: str, note: str? }) = p.title\nfn f() = tl({ title: \"x\", extra: 1 })");
    clean("fn tl(p: { title: str }) -> { title: str, n: int } = merge(p, { n: 1 })");
    let prog = quillo::parser::Parser::new("fn t(p: { title: str, tags: list<str> }) = 1").parse_program().unwrap();
    let quillo::ast::Stmt::Fn(d) = &prog[0] else { unreachable!() };
    assert!(quillo::check::def_text(d).contains("p: { title: str, tags: list<str> }"), "{}", quillo::check::def_text(d));
}

fn strict(src: &str) -> Vec<String> {
    let opts = quillo::check::Options { strict_null: true, ..Default::default() };
    quillo::check::check_source_opts(src, "t.qwe", Path::new("."), opts)
        .diagnostics
        .iter()
        .map(|d| format!("{}{} {}", d.line, if d.severity == Severity::Warning { " warning:" } else { "" }, d.message))
        .collect()
}

#[test]
fn strict_null_warns_where_null_does_not_work() {
    let pre = "fn shout(s: str) = upper(s)\nfn maybe(x: int) = x > 1 ? \"a\" : null\n";
    let warn = |body: &str, want: &str| {
        let d = strict(&format!("{}{}", pre, body));
        assert!(d.len() == 1 && d[0].contains(want), "{:?}, wanted {:?} for {}", d, want, body);
    };
    let ok = |body: &str| {
        let d = strict(&format!("{}{}", pre, body));
        assert!(d.is_empty(), "{:?} for {}", d, body);
    };
    warn("fn f() = shout(maybe(2))", "argument `s` of `shout` may be null here (it is str | null), but it must be str");
    warn("fn f(n: int?) = n + 1", "the left side may be null here (it is int | null)");
    warn("fn f(x: int) -> str = maybe(x)", "`f` must return str, but this may be null");
    // Checks narrow the type.
    ok("fn f() {\n let m = maybe(2)\n m != null ? shout(m) : \"\"\n}");
    ok("fn f() {\n let m = maybe(2)\n if m == null { return \"\" }\n shout(m)\n}");
    ok("fn f() {\n let m = maybe(2)\n m != null && shout(m) != \"\"\n}");
    ok("fn f() {\n let m = maybe(2)\n m == null || shout(m) != \"\"\n}");
    ok("fn ks(m: map) = keys(m)\nfn f(m: map?) = if m { ks(m) } else { [] }");
    ok("fn f(n: int?) = match n {\n null => 0\n k => k + 1\n}");
    ok("fn f() = shout(maybe(2) ?? \"\")");
    // Values of unknown type, and lists that are never empty, are fine.
    ok("fn f(x) = shout(x)");
    ok("fn f(a: int) = max([a, 1]) + 1");
    ok("fn f() = shout(split(\"a b\", \" \")[1])");
    // A field with a default is never null, unless the default is.
    ok("route GET /t ? { q: str = \"\" } = shout(q)");
    ok("fn f(m) = match m {\n { q: str = \"x\" } => shout(q)\n _ => \"\"\n}");
    warn("fn f(m) = match m {\n { q: str = null } => shout(q)\n _ => \"\"\n}", "argument `s` of `shout` may be null");
    // Without the option, none of this is reported.
    assert!(diags(&format!("{}fn f() = shout(maybe(2))", pre)).is_empty());
}

#[test]
fn service_calls_are_checked_against_what_the_service_offers() {
    let d = std::env::temp_dir().join(format!("quillo-check-services-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("billing.qwe"), "server fn invoice(customer: str, cents: int, note: str = \"\") -> map = { customer, cents }\nfn inner() = 1\n").unwrap();
    let diags = |src: &str| -> Vec<String> {
        quillo::check::check_source(src, "main.qwe", &d).diagnostics.iter().map(|x| format!("{} {}", x.line, x.message)).collect()
    };
    let pre = "service billing = \"billing.qwe\"\n";
    assert!(diags(&format!("{}fn f() = billing.invoice(\"ana\", 1200)", pre)).is_empty());
    assert!(diags(&format!("{}fn f() = billing.invoice(\"ana\", 1200, \"a mug\")", pre)).is_empty());
    let d1 = diags(&format!("{}fn f() = billing.invoce(\"ana\", 1)", pre));
    assert!(d1.len() == 1 && d1[0].contains("service `billing` has no server fn `invoce` (did you mean `invoice`?)"), "{:?}", d1);
    // Only server fns are offered.
    assert!(diags(&format!("{}fn f() = billing.inner()", pre))[0].contains("has no server fn `inner`"));
    let d2 = diags(&format!("{}fn f() = billing.invoice(\"ana\", \"lots\")", pre));
    assert!(d2.len() == 1 && d2[0].contains("argument `cents` of `billing.invoice` must be int, got str"), "{:?}", d2);
    assert!(diags(&format!("{}fn f() = billing.invoice(\"ana\")", pre))[0].contains("missing argument `cents`"));
    // It may fail on the way: an error value.
    assert!(diags(&format!("{}fn f() -> map = billing.invoice(\"ana\", 1)?", pre)).is_empty());
    // The browser does not call services; a function limited to the database neither.
    assert!(diags(&format!("{}component C() {{ ~x = billing.invoice(\"a\", 1)\n <p>{{x}}</p> }}", pre)).iter().any(|m| m.contains("calls another service, which only the server does")));
    assert!(diags(&format!("{}fn f() with db = billing.invoice(\"a\", 1)", pre)).iter().any(|m| m.contains("needs the http effect")));
    // Made later: the same call, checked the same; its result is the call's id.
    assert!(diags(&format!("{}fn f() -> str = billing.later.invoice(\"ana\", 1200)", pre)).is_empty());
    let d3 = diags(&format!("{}fn f() = billing.later.invoice(\"ana\", \"lots\")", pre));
    assert!(d3.len() == 1 && d3[0].contains("argument `cents` of `billing.invoice` must be int, got str"), "{:?}", d3);
    assert!(diags(&format!("{}fn f() = billing.later.nope()", pre))[0].contains("has no server fn `nope`"));
    // How it is called, after the file.
    assert!(diags("service billing = \"billing.qwe\" { timeout: \"2s\", retries: 1 }\nfn f() = billing.invoice(\"a\", 1)").is_empty());
    // A file that is not there.
    assert!(diags("service gone = \"gone.qwe\"\n")[0].contains("cannot read"));
    // Services using one another are each checked once.
    std::fs::write(d.join("a.qwe"), "service b = \"b.qwe\"\nserver fn ping() = b.pong()\n").unwrap();
    std::fs::write(d.join("b.qwe"), "service a = \"a.qwe\"\nserver fn pong() = 1\n").unwrap();
    assert!(diags("service a = \"a.qwe\"\nfn f() = a.ping()").is_empty());
}

#[test]
fn errors_say_when_a_name_is_a_keyword_or_hides_a_builtin() {
    one("server fn reply(t: str) = t", "found `reply`, which is a keyword: pick another name");
    one("let reply = 1", "`reply` is a keyword, so it cannot be a name here");
    // Hiding a builtin is a warning, and a call that goes wrong says so too.
    let d = diags("fn notify(user: int, text: str) = user\nfn f() = notify(\"Posted\", \"x\")");
    assert!(d.len() == 2 && d[0].starts_with("1 warning: `notify` hides the builtin `notify`") && d[1].contains("which hides the builtin `notify`"), "{:?}", d);
    one("fn first(xs: list) = xs[0]\nprint(first([1]))", "1 warning: `first` hides the builtin `first`");
    // A function of the program's own with no builtin of that name: no note.
    let d = diags("fn post(text: str) = text\nfn f() = post(1)");
    assert!(d.len() == 1 && !d[0].contains("hides"), "{:?}", d);
}

#[test]
fn while_loops_are_checked() {
    clean("fn f(n: int) {\n  let i = 0\n  while i < n { i += 1 }\n  i\n}");
    one("fn f() {\n  while nope { }\n}", "unknown name `nope`");
    // As with `if`: 0 is true.
    one("fn f(n: int) {\n  while n { }\n}", "`n` counts as true even when it is 0");
}

#[test]
fn patterns_written_out_are_checked() {
    clean("fn f(s: str) = re.find_all(s, \"@(\\\\w+)\", \"i\")");
    one("fn f(s: str) = re.test(s, \"(a\")", "`re.test`: `(` without its `)`");
    one("fn f(s: str) = re.replace(s, \"a\", \"b\", \"x\")", "the flags are");
    // A pattern made as the program runs is checked then.
    clean("fn f(s: str, p: str) = re.test(s, p)");
}

/// The note that code may be for a newer Quillo follows only syntax this one
/// does not know, not an error that names the rule broken.
#[test]
fn only_unknown_syntax_may_be_for_a_newer_quillo() {
    let newer = |src: &str| !check_source(src, "t.qwe", Path::new(".")).syntax_errors.is_empty();
    assert!(newer("let x = 1 # 2"));
    assert!(newer("fn f() => 1"));
    assert!(newer("let s = \"\\q\""));
    assert!(!newer("fn reply(x) = x"));
    assert!(!newer("let reply = 1"));
    assert!(!newer("fn f() { g() = 1 }"));
    assert!(!newer("let s = \"unterminated"));
}

/// A name the program defines twice is an error naming both places, in one
/// file or across `use`d ones: the second would replace the first
/// everywhere. One that hides a builtin is a warning.
#[test]
fn a_name_defined_twice_is_an_error() {
    one("fn helper() = 1\nfn helper() = 2", "2 `helper` is defined twice, at t.qwe:1 and t.qwe:2");
    one("fn helper() = 1\nlet helper = 5", "`helper` is defined twice");
    one("type User = { n: int }\nfn User() = 1", "`User` is defined twice");
    one("fn len(x) = 99", "1 warning: `len` hides the builtin `len`");
    // `let` twice in one file gives the variable again.
    clean("let x = 1\nlet x = x + 1\nprint(x)");
    let d = std::env::temp_dir().join(format!("quillo-twice-{}", std::process::id()));
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::write(d.join("lib/x.qwe"), "fn helper() = 1\nlet shared = 1\n").unwrap();
    std::fs::write(d.join("lib/y.qwe"), "fn helper() = 2\nlet shared = 2\n").unwrap();
    std::fs::write(d.join("main.qwe"), "use \"lib/x.qwe\"\nuse \"lib/y.qwe\"\nprint(helper() + shared)\n").unwrap();
    let a = quillo::check::check_file(&d.join("main.qwe"));
    let msgs: Vec<String> = a.diagnostics.iter().map(|d| format!("{}:{} {}", d.file.rsplit('/').next().unwrap(), d.line, d.message)).collect();
    assert!(msgs.iter().any(|m| m.starts_with("y.qwe:1 `helper` is defined twice, at lib/x.qwe:1 and lib/y.qwe:1")), "{:#?}", msgs);
    assert!(msgs.iter().any(|m| m.starts_with("y.qwe:2 `shared` is defined twice, at lib/x.qwe:2 and lib/y.qwe:2")), "{:#?}", msgs);
    let _ = std::fs::remove_dir_all(&d);
}

/// `el.value = x`: in a component, on one of the page's objects; a map's
/// fields are made anew, and outside a component there is nothing to set.
#[test]
fn fields_of_the_pages_objects_are_set_in_components() {
    clean("component Picker() {\n    <input type=\"file\" @change={(e) => { e.target.value = \"\" }}/>\n}");
    clean("component Box() {\n    <div @mount={(el) => { el.scrollTop += 10 }}></div>\n}");
    one("component C() {\n    state m = { a: 1 }\n    <button @click={() => { m.a = 2 }}>x</button>\n}", "a map's fields are not set in place: write `m = set(m, \"a\", …)`");
    one("fn f(m: map) {\n    m.a = 2\n}", "cannot set `a`: only the page's objects (an element, an event) have fields that are set, in a component");
    one("fn f(xs: list) {\n    xs[0] = 1\n}", "only variables, and the fields of the page's objects (`el.value = x`), can be assigned to");
}

#[test]
fn key_is_the_lists_and_attributes_are_written_once() {
    one("view Row(key: str, label: str) = <li>{key}: {label}</li>", "<Row> cannot take a parameter named `key`");
    one("component Card(key: str) = <p>{key}</p>", "<Card> cannot take a parameter named `key`");
    one("fn Row(key) = <li>{key}</li>", "<Row> cannot take a parameter named `key`");
    clean("fn lookup(key) = key\nprint(lookup(1))");
    one("view Card(id: int) = <li>{id}</li>\nlet xs = [1]\nprint(<ul>{for x in xs => <Card key={x} id={x} key={x}/>}</ul>)", "attribute `key` is written twice in <Card>");
    one("print(<p class=\"a\" class=\"b\">x</p>)", "attribute `class` is written twice in <p>");
    clean("print(<p.a class=\"b\">x</p>)");
}

#[test]
fn newer_forms_check_clean() {
    clean("let m = { a: 1 }\nlet xs = [1]\nprint([{ ...m, b: 2 }, [...xs, 3], set(xs, 0, 2), m[5]])");
    clean("fn f(x) = x ?? return null\nprint(f(1))");
    clean("let num = 5\nfor str in [\"a\"] { print(str) }\nprint(num + 1)");
    clean("route GET /tz = cookie(\"tz\") ?? \"UTC\"");
    one("component C() = <p>{cookie(\"tz\")}</p>", "`cookie` is not available in the browser");
}

#[test]
fn http_to_a_host_from_a_value_is_worth_a_word() {
    let warn = "the host `http.";
    // The host written in the program, whatever follows it.
    clean("server fn a(id: int) = http.get(`https://api.test/users/${id}`)");
    clean("server fn a(id: int) = http.get(\"https://api.test/users/\" + str(id))");
    clean("let API = env(\"API_URL\") ?? \"https://api.test\"\nserver fn a(id: int) = http.get(`${API}/users/${id}`)");
    clean("let API = env(\"API_URL\") ?? \"https://api.test\"\nserver fn a(id: int) = http.get(API + \"/users/\" + str(id))");
    clean("server fn a(id: int) {\n let url = `https://api.test/u/${id}`\n http.get(url)\n}");
    clean("server fn a(id: int) = http.request(\"GET\", if id > 0 { \"https://a.test/\" } else { \"https://b.test/\" })");
    // From a value: said either way, it is the program's choice.
    one("server fn a(u: str) = http.get(u)", warn);
    one("route GET /p = http.post(request.query.u, { a: 1 })", warn);
    one("server fn a(h: str) = http.get(`https://${h}/x`)", warn);
    one("server fn a(p: str) = http.get(\"https://api.test\" + p)", warn);
    one("server fn a(u: str) {\n let url = u + \"/x\"\n http.get(url)\n}", warn);
    clean("server fn a(u: str) = http.get(u, {}, { public_only: true })");
    clean("server fn a(u: str) = http.request(\"POST\", u, null, {}, { public_only: false })");
    // In the browser the server's network is not where it goes.
    clean("component C() {\n ~u = \"\"\n ~r = null\n <button @click={() => { r = http.get(u) }}>{r}</button>\n}");
}

/// A keyword as a state's name says so, as it does for `fn` and `let`.
#[test]
fn a_keyword_as_a_state_name() {
    one("component C() {\n    state reply = \"\"\n    <p>{reply}</p>\n}", "2 expected a state name, found `reply`, which is a keyword");
    // `state` is still a name of its own.
    clean("let state = 1\nprint(state + 1)");
}

/// SQL names from `sql_name` stay safe in a variable, whatever its argument
/// is made of, and through the program's functions that only return them.
#[test]
fn sql_names_from_helpers_and_templates() {
    clean("fn f(id: int) with db {\n    let table = sql_name(`obj_${id}`)\n    db.query(`select * from ${table}`)\n}");
    clean("fn table_of(id: int) -> str = sql_name(`obj_${id}`)\nfn count_of(id: int) with db = db.one(`select count(*) as n from ${table_of(id)}`)");
    clean("fn tbl(id: int) = sql_name(`obj_${id}`)\nfn u(id: int) = tbl(id)\nfn f(id: int) with db = db.query(`select * from ${u(id)}`)");
    clean("fn tbl(id: int) {\n    if id < 0 { return sql_name(\"none\") }\n    let n = sql_name(`obj_${id}`)\n    n\n}\nfn f(id: int) with db = db.query(`select * from ${tbl(id)}`)");
    // A helper that can return text from a value is not safe.
    one("fn tbl(name: str) = name\nfn f(n: str) with db = db.query(`select * from ${tbl(n)}`)", "SQL injection");
    one("fn tbl(id: int, raw: str) = id > 0 ? sql_name(`obj_${id}`) : raw\nfn f(n: str) with db = db.query(`select * from ${tbl(1, n)}`)", "SQL injection");
    one("fn tbl(raw: str) {\n    if raw == \"\" { return raw }\n    sql_name(\"a\")\n}\nfn f(n: str) with db = db.query(`select * from ${tbl(n)}`)", "SQL injection");
}

/// Markup the browser's HTML parser would not keep as written.
#[test]
fn markup_the_browser_restructures() {
    one("view V() = <p>a <div>b</div></p>", "1 <div> cannot go inside a <p>");
    one("view V() = <p><span>a <ul><li>b</li></ul></span></p>", "<ul> cannot go inside a <p>");
    one("view V(on: bool) = <p>{on && <section>s</section>}</p>", "<section> cannot go inside a <p>");
    one("view V() = <a href=\"/a\">a <a href=\"/b\">b</a></a>", "<a> cannot go inside an <a>");
    one("view V() = <form><form></form></form>", "<form> cannot go inside a <form>");
    // A view or component counts as its outer element.
    one("view Box() = <div>x</div>\nview V() = <p>a <Box/></p>", "<Box>, whose outer element is a <div>, cannot go inside a <p>");
    one("component Live() {\n    state n = 0\n    <div>{n}</div>\n}\nview V() = <p><Live/></p>", "<Live>, whose outer element is a <div>");
    // Fine: inline content in a <p>, a <div> in a <button>, a view that may be a link.
    clean("view Chip() = <span>c</span>\nview V() = <p><b>a</b> <Chip/> <button><div>b</div></button></p>");
    clean("view L(link: bool) = link ? <a href=\"/\">l</a> : <span>l</span>\nview V() = <a href=\"/x\"><L link={false}/></a>");
    clean("view V() = <div><p>a</p><div><section>b</section></div></div>");
}

/// A file that cannot be parsed is reported first, and the names it
/// defines are not also reported as unknown where they are used.
#[test]
fn a_parse_error_comes_before_what_it_causes() {
    let d = std::env::temp_dir().join(format!("quillo-parse-first-{}", std::process::id()));
    std::fs::create_dir_all(d.join("ui")).unwrap();
    std::fs::write(d.join("ui/room.qwe"), "view Row(a) = <li>{a}</li>\n\ncomponent Room(id: int) {\n    state n = 0\n    <div>{n</div>\n}\n").unwrap();
    std::fs::write(d.join("main.qwe"), "use \"ui/room.qwe\"\nroute GET / = <Room id={1}/>\nprint(nowhere)\n").unwrap();
    let a = quillo::check::check_file(&d.join("main.qwe"));
    let msgs: Vec<String> = a.diagnostics.iter().map(|d| format!("{}:{} {}", d.file.rsplit('/').next().unwrap(), d.line, d.message)).collect();
    assert!(msgs[0].starts_with("room.qwe:5 "), "{:#?}", msgs);
    assert!(!msgs.iter().any(|m| m.contains("`Room`")), "{:#?}", msgs);
    assert!(msgs.iter().any(|m| m.contains("unknown name `nowhere`")), "{:#?}", msgs);
    let _ = std::fs::remove_dir_all(&d);
}

/// A route's wildcard starts with `/*`, which is not a comment there.
#[test]
fn a_wildcard_route_is_not_a_comment() {
    clean("route GET /*rest = rest");
    clean("route GET /files/*path = path");
}

