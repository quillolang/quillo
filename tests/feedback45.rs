//! FEEDBACK45.md: what `quillo check` missed or said badly, the lexer and parser part
//! (branch feedback40-43).

use quillo::check::{check_source, Severity};
use quillo::Interp;
use std::path::Path;

fn diags(src: &str) -> Vec<String> {
    diags_in(src, Path::new("."))
}

fn diags_in(src: &str, dir: &Path) -> Vec<String> {
    check_source(src, "t.qwe", dir)
        .diagnostics
        .iter()
        .map(|d| format!("{}{} {}", d.line, if d.severity == Severity::Warning { " warning:" } else { "" }, d.message))
        .collect()
}

fn run(src: &str) -> String {
    Interp::new(".").load_source(src, "t.qwe").map(|v| v.repr()).unwrap_or_else(|e| format!("error: {}", e))
}

// Q-503: an unclosed `/*` was the rest of the file, routes and all, and `check` said ok.
#[test]
fn an_unclosed_comment_is_an_error_at_the_line_it_opens() {
    let said = "`/*` on line 2 is never closed: add `*/` where the comment ends";
    assert_eq!(diags("let a = 1\n/* the totals\nlet b = 2\nroute GET /x = \"x\"\n"), [format!("2 {}", said)]);
    assert_eq!(run("let a = 1\n/* the totals\nprint(a)\n"), format!("error: t.qwe:2: {}", said));
    // At the very end, and on the first line.
    assert_eq!(diags("/*"), ["1 `/*` on line 1 is never closed: add `*/` where the comment ends"]);
    // Closed ones, over lines or not, `/*` in a string or a line comment, and `/**/`.
    assert!(diags("let a = 1 /* x */\n/* one\ntwo */ print(a)\nprint(\"/* no\") // /* no\n/**/print(2)\n").is_empty());
}

// Q-529: a lone CR, as old Mac files end lines, is a line break.
#[test]
fn a_lone_carriage_return_ends_a_line() {
    assert_eq!(run("let x = 1\rlet y = x + 1\ry"), "2");
    // In a string too, as Python's universal newlines read one.
    assert_eq!(run("\"a\rb\""), "\"a\\nb\"");
    assert_eq!(run("let x = 1\r\nlet y = x + 2\r\ny"), "3");
}

// Q-506: `/a/{id}` was the path `/a/` and a request pattern, and matched nothing.
#[test]
fn a_braced_path_parameter_says_how_one_is_written() {
    assert_eq!(diags("route GET /a/{id} = id\n"), ["1 path parameters are written `:id`, as in `/users/:id`, not `{id}`"]);
    assert_eq!(diags("route GET /a/{id:int} = id\n"), ["1 path parameters are written `:id(int)`, as in `/users/:id(int)`, not `{id:int}`"]);
    assert_eq!(diags("route GET /{x}/b = x\n"), ["1 path parameters are written `:x`, as in `/users/:x`, not `{x}`"]);
    // A request pattern stays one, after a space or written out, or a request's field right
    // after the root `/` (found in review).
    assert!(diags("route POST / { json: { a: str } } = a\nroute POST /{ json: { b: str } } = b\nroute GET /c/:id(int) = id\n").is_empty());
    assert!(diags("route POST /{json} = json\nroute GET /{query} = query\n").is_empty());
    assert_eq!(diags("route GET /a/{json} = json\n"), ["1 path parameters are written `:json`, as in `/users/:json`, not `{json}`"]);
}

// Q-510: `"Hello ${name}"` is text; said when `name` is one of the program's here.
#[test]
fn a_template_written_in_quotes_is_pointed_out() {
    let d = diags("let name = \"Ada\"\nprint(\"Hello ${name}\")\nprint('Hi ${name}')\nfn f(user) = \"Dear ${user.name}\"\n");
    assert_eq!(d.len(), 3, "{:?}", d);
    assert_eq!(d[0], "2 warning: this string is not a template, so `${name}` stays as text: a template is written in backquotes, and `\\${` keeps the text");
    assert!(d[2].starts_with("4 warning: this string is not a template, so `${user}`"), "{}", d[2]);
    // Not: `\${`, a name the program has not, a builtin's, a real template, a `$` alone.
    assert!(diags("let name = 1\nprint(\"\\${name}\")\nprint(\"${nobody}\")\nprint(\"${print}\")\nprint(`${name}`)\nprint(\"$5 and ${ name }\")\n").is_empty());
    // It stays the text it was.
    assert_eq!(run("let name = 1\n\"a ${name}\""), "\"a ${name}\"");
    // On a line after its statement's, and in a file the program imports (found in review).
    assert_eq!(diags("let who = 1\nprint(\n  \"hey ${who}\"\n)\n").len(), 1);
    let dir = std::env::temp_dir().join(format!("quillo-f45-dollar-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/w.qwe"), "let name = \"x\"\nfn hi() = \"hi ${name}\"\n").unwrap();
    let d = diags_in("import lib/w.qwe use hi\nprint(hi())\n", &dir);
    assert!(d.iter().any(|w| w.starts_with("2 warning: this string is not a template, so `${name}`")), "{:?}", d);
    let _ = std::fs::remove_dir_all(&dir);
}

// The `${name}` notes are shared by the lexer's clones, not copied: a long file is read in the
// time it was (found in review: 40,000 lines took 11 s).
#[test]
fn many_strings_with_a_dollar_are_read_quickly() {
    let src: String = (0..40_000).map(|i| format!("let s{} = (\"${{zz}}\" + (\"x\"))\n", i)).collect();
    let started = std::time::Instant::now();
    let _ = diags(&src);
    assert!(started.elapsed() < std::time::Duration::from_secs(3), "{:?}", started.elapsed());
}

// Q-507: a name imported as a builtin's replaced it everywhere, without a word.
#[test]
fn an_import_named_as_a_builtin_is_warned_of() {
    let dir = std::env::temp_dir().join(format!("quillo-f45-import-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/m.qwe"), "fn price(x) = x * 2\n").unwrap();
    assert_eq!(
        diags_in("import lib/m.qwe use price as print\n", &dir),
        ["1 warning: `print` hides the builtin `print`: taken under that name, `price` is this file's `print`, so take it under its own name (`use price`) or another"]
    );
    assert!(diags_in("import lib/m.qwe use price as cost\nprint(cost(1))\nimport lib/m.qwe use price\nprint(price(2))\n", &dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// Q-526, Q-516: `let fn`, `let true`, `type if` get the keyword message.
#[test]
fn keywords_are_not_names() {
    for (src, k) in [("let fn = 1", "fn"), ("let true = 1", "true"), ("let false = 1", "false"), ("let null = 1", "null"), ("for null in [1] => 1", "null"), ("type if = int", "if")] {
        assert_eq!(diags(&format!("{}\n", src)), [format!("1 `{}` is a keyword, so it cannot be a name here: pick another name", k)], "{}", src);
    }
    // `type` as a name, then a keyword that is no type's: its own message (found in review).
    assert_eq!(diags("fn f(type) = type in xs\n"), ["1 there is no `in`: `contains(xs, x)`, or `has(m, k)` for a map"]);
    // Names that only start like one, a pattern inside a list, a literal arm of a match.
    assert!(diags("let fn_a = 1\nlet truth = 2\nlet [n, m] = [1, 2]\nlet x = match fn_a { 1 => true, _ => null }\nlet type = 3\n").is_empty());
}

// Q-529: numbers as JavaScript and Rust refuse them, and `--`.
#[test]
fn numbers_are_written_one_way() {
    let between = "a `_` in a number goes between two digits, as in `1_000`";
    for (src, said) in [
        ("print(1__000)", between.to_string()),
        ("print(1_)", between.to_string()),
        ("print(1.5_)", between.to_string()),
        ("print(08)", "a number does not start with `0`: write `8`".to_string()),
        ("print(010)", "a number does not start with `0`: write `10`".to_string()),
        ("print(00)", "a number does not start with `0`: write `0`".to_string()),
        ("print(007.25)", "a number does not start with `0`: write `7.25`".to_string()),
        ("print(00.5)", "a number does not start with `0`: write `0.5`".to_string()),
        ("print(01e3)", "a number does not start with `0`: write `1e3`".to_string()),
        ("print(1.)", "a number does not end with `.`: write `1` or `1.0`".to_string()),
        ("print(--1)", "there is no `--`: to take one away, `n -= 1`; for minus a negative, `-(-n)`".to_string()),
    ] {
        assert_eq!(diags(&format!("{}\n", src)), [format!("1 {}", said)], "{}", src);
    }
    // `let x = 1.` was `1.print(x)` with the next line.
    assert_eq!(diags("let x = 1.\nprint(x)\n"), ["1 a number does not end with `.`: write `1` or `1.0`"]);
    assert_eq!(run("[1_000, 0, 0.5, 10.25, 2.5e-1, 0x_FF, 12.50m, -(-3), 5 - -1, len(0..3)]"), "[1000, 0, 0.5, 10.25, 0.25, 255, 12.50m, 3, 6, 3]");
}

// Q-529: one `,` between a map's entries, as a list's.
#[test]
fn a_map_has_no_empty_entry() {
    let said = "an empty entry in a map: one `,` goes between two entries";
    assert_eq!(diags("let m = { a: 1,, b: 2 }\n"), [format!("1 {}", said)]);
    // A `,` first reads as a block's: refused there.
    assert_eq!(diags("let m = { , a: 1 }\n"), ["1 expected an expression, found `,`"]);
    assert_eq!(run("[{ a: 1, b: 2, }, {}, { a: 1; b: 2 }, {\n  a: 1,\n\n  b: 2,\n}]"), "[{ a: 1, b: 2 }, {}, { a: 1, b: 2 }, { a: 1, b: 2 }]");
}

// Q-521: two elements side by side read as `<`.
#[test]
fn two_elements_side_by_side_are_named() {
    assert_eq!(diags("view V() = <p>a</p><p>b</p>\n"), ["1 two elements side by side: a view gives one, so wrap them in one, like `<div>...</div>`"]);
    assert!(diags("view W() = <div><p>a</p><p>b</p></div>\nlet n = 1 < 2\nlet m = 1 < 2 ? 1 : 2\n").is_empty());
}

// Q-518: an arrow whose body assigns, and a handler written as an assignment.
#[test]
fn an_arrow_may_assign_and_a_handler_must_be_a_function() {
    assert_eq!(run("let n = 0\nlet f = () => n += 1\nlet g = () => n = 5\nlet h = (x) => n -= x\nf()\nf()\nlet a = n\ng()\nh(2)\nlet m = { go: () => n = 9, k: 2 }\n[a, n, f(), m.k, map([1, 2], (x) => x == 1)]"), "[2, 3, null, 2, [true, false]]");
    assert_eq!(run("let n = 0\nlet m = { go: () => n = 9 }\nm.go()\nn"), "9");
    assert!(diags("component C() {\n    state n = 0\n    <button @click={() => n = n + 1}>{n}</button>\n}\n").is_empty());
    assert_eq!(
        diags("component C() {\n    state n = 0\n    <button @click={n += 1}>{n}</button>\n}\n"),
        ["3 an assignment is not a value: a handler is a function, `@click={() => { n += ... }}`"]
    );
    // What cannot be assigned says so, as a statement would.
    assert_eq!(diags("let f = () => 1 = 2\n"), ["1 only variables, and the fields of the page's objects (`el.value = x`), can be assigned to"]);
}

// ---- the checker's part ------------------------------------------------------------------------

fn has(d: &[String], line: u32, part: &str) -> bool {
    d.iter().any(|w| w.starts_with(&format!("{} ", line)) && w.contains(part))
}

// Q-504, Q-516, Q-527: patterns that bind or never match by mistake, and arms that never run.
#[test]
fn patterns_say_what_they_cannot_do() {
    let d = diags("let x = match \"hello\" {\n    intt => \"number\"\n    str => \"text\"\n}\nfn f(v) = match v {\n    Strr => \"s\"\n    _ => \"other\"\n}\ntype User = { name: str }\nfn g(v) = match v {\n    Usr => 1\n    User => 2\n    other => other\n}\nlet y = match 1 {\n    1 => \"a\"\n    1 => \"b\"\n    _ => \"c\"\n}\nlet w = match 1 { \"a\" => 1, _ => 2 }\nlet e = match 1 { }\n");
    assert!(has(&d, 2, "`intt` here is a name, which takes any value: for the type, `int`"), "{:?}", d);
    assert!(has(&d, 3, "this arm never matches: the one on line 2 takes any value"), "{:?}", d);
    assert!(has(&d, 6, "there is no type `Strr`, so this never matches (did you mean `str`?)"), "{:?}", d);
    assert!(has(&d, 11, "there is no type `Usr`, so this never matches (did you mean `User`?)"), "{:?}", d);
    assert!(has(&d, 17, "this arm never matches: an arm before it has the same value"), "{:?}", d);
    assert!(has(&d, 20, "this arm never matches: the value it is given is another"), "{:?}", d);
    assert!(has(&d, 21, "this `match` has no arms"), "{:?}", d);
    // A name that is used, a guard, and `_`: nothing said.
    assert!(diags("fn f(v) = match v {\n    n if n > 1 => n\n    data => len(data)\n}\nfn g(v) = match v { 1 => 1, _ => 2 }\n").is_empty());
    let d = diags("let [a, b] = [1]\nlet { name } = 5\nlet 1 = 2\nlet [c, e] = [1, 2]\nprint(a, b, name, c, e)\n");
    assert_eq!(d.iter().filter(|w| w.contains("this `let` never matches")).count(), 3, "{:?}", d);
    // Route patterns: a name for a type, and a type the program does not have.
    let d = diags("route GET /d ? { page: itn } = `page ${itn}`\nroute POST /a { json: Usr } = json\nroute GET /e ? { page: int, data: str } = page\n");
    assert!(has(&d, 1, "`itn` here is a name") && has(&d, 2, "there is no type `Usr`"), "{:?}", d);
    assert_eq!(d.len(), 2, "{:?}", d);
}

// Q-505: what makes a new value and drops it, and a `while` nothing in it ends.
#[test]
fn dropped_values_and_endless_loops_are_said() {
    let d = diags("let xs = [1]\nxs.push(2)\npush(xs, 2)\nsort(xs)\nlet m = { a: 1 }\nset(m, \"b\", 2)\nlet x = 1\nx == 2\nprint(xs, m)\ncomponent C() {\n    state items = []\n    <button @click={() => { items.push(1) }}>add</button>\n}\nlet i = 0\nwhile i < 3 {\n    i + 1\n}\n");
    for (line, part) in [(2, "write `xs = xs | push(...)`"), (3, "`xs = xs | push(...)`"), (4, "`xs = xs | sort(...)`"), (6, "`m = m | set(...)`"), (8, "`x == ...` compares"), (12, "`items = items | push(...)`"), (15, "nothing in it changes `i`")] {
        assert!(has(&d, line, part), "{} {} {:?}", line, part, d);
    }
    // Not: an assignment, a value kept, `map` for its function, a loop that changes its
    // variable, or one over a page's object that a call changes.
    assert!(diags("fn ok(xs) {\n    let ys = sort(xs)\n    xs.map((v) => print(v))\n    let j = 0\n    while j < 3 { j += 1 }\n    let w = js(\"document.body\")\n    while w.firstChild != null { w.removeChild(w.firstChild) }\n    ys\n}\n").is_empty());
}

// Q-508: one name twice in one place.
#[test]
fn names_declared_twice_are_refused() {
    let d = diags("fn f(a, a) = a\nlet g = (b, b) => b\nfn outer() {\n    fn inner() = 1\n    fn inner() = 2\n    inner()\n}\ncomponent C() {\n    state n = 0\n    state n = 1\n    <p>{n}</p>\n}\nfn k() with db, db = 1\n");
    assert!(has(&d, 1, "parameter `a` is named twice") && has(&d, 2, "parameter `b` is named twice"), "{:?}", d);
    assert!(has(&d, 5, "`inner` is defined twice here, at line 4 and line 5"), "{:?}", d);
    assert!(has(&d, 10, "`n` is defined twice here, at line 9 and line 10"), "{:?}", d);
    assert!(has(&d, 13, "warning: `db` is named twice in this `with`"), "{:?}", d);
    assert_eq!(diags("type User = { name: str, name: int }\n"), ["1 field `name` is named twice in this type"]);
    assert!(diags("fn ok(_, _) = 1\nfn fine() {\n    let x = 1\n    let x = 2\n    x\n}\n").is_empty());
}

// Q-509: misspelt attributes and events, React's names, a view's name in lower case.
#[test]
fn misspelt_markup_is_said() {
    let d = diags("view V() = <div className=\"box\" clas=\"a\"><a hreff=\"/x\">x</a><p @clik={() => 1}>y</p><input placholder=\"n\"/></div>\nview Card() = <div>c</div>\nview W() = <main><card/></main>\n");
    for part in ["`className` is React's name", "`clas` is no attribute the browser knows: did you mean `class`?", "did you mean `href`?", "`@clik` is no event the browser knows: did you mean `@click`?", "did you mean `placeholder`?"] {
        assert!(d.iter().any(|w| w.contains(part)), "{} {:?}", part, d);
    }
    assert!(has(&d, 3, "<card> is no HTML element, and is written to the page as it is: the view is <Card>"), "{:?}", d);
    // Not: data-, aria-, a custom element's, SVG's, a binding, short names, or real elements a view is named like.
    assert!(diags("view Main() = <main><div data-x=\"1\" aria-label=\"a\" role=\"b\"><my-el foo=\"1\"></my-el><svg viewBox=\"0 0 1 1\"><marker refX=\"1\"/><circle cx=\"1\" r=\"1\"/><text>t</text></svg><object data=\"/a.pdf\"></object><button>b</button></div></main>\nview Text() = <p>t</p>\n").is_empty());
}

// Q-511: `request` has the fields the server gives it.
#[test]
fn a_typo_in_requests_fields_is_said() {
    assert_eq!(diags("route GET /e = request.qurey.name ?? \"none\"\n"), ["1 this map has no field `qurey` (did you mean `query`?)"]);
    assert!(diags("route POST /f = [request.method, request.path, request.query, request.query_string, request.params, request.headers, request.cookies, request.body, request.json, request.form, request.files, request.ip, request.id, request.call, request.caller, request.user, request.forwarded]\n").is_empty());
}

// Q-512: a list's items are each what the list takes.
#[test]
fn a_list_with_one_item_of_another_type_is_refused() {
    let d = diags("fn f(a: list<int>) = a\nprint(f([1, \"a\"]))\nprint(f([1, 2]))\nprint(f([]))\nfn g(a: list<num>) = a\nprint(g([1, 2.5]))\nfn h(a: list<int | str>) = a\nprint(h([1, \"a\"]))\n");
    assert_eq!(d, ["2 argument `a` of `f` must be list<int>, but its item 2 is str, not int"]);
    // What is worked out from values not known is let be: `x * 2` is `num | dec` (found in review).
    assert!(diags("fn tot(xs: list<int>) = sum(xs)\nlet data = json.parse(\"[1, 2]\")\nprint(tot(data | map((x) => x * 2)))\nprint(tot([data[0] + 1, 2]))\nfn half(n: int) = [n / 2, n]\nprint(tot(half(4)))\n").is_empty());
}

// Q-513: JavaScript habits that run and mean something else.
#[test]
fn javascript_habits_that_mean_something_else_are_said() {
    let d = diags("print(index_of(\"abc\", \"x\") == -1)\nfn is_ready() = false\nif is_ready { print(\"yes\") }\nprint(is_ready == false)\nroute GET /b/:id = id + 1\n");
    assert!(has(&d, 1, "`index_of` gives null when it is not there, never -1"), "{:?}", d);
    assert!(has(&d, 3, "`is_ready` is a function, which counts as true: to test its answer, `is_ready()`"), "{:?}", d);
    assert!(has(&d, 4, "compares the function, not its answer"), "{:?}", d);
    assert!(has(&d, 5, "`id` is text from the path"), "{:?}", d);
    assert!(diags("fn is_ready() = false\nlet r = is_ready() ? 1 : 2\nprint(index_of(\"a\", \"a\") == null)\nroute GET /c/:id(int) = id + 1\nroute GET /d/:id = `${id}1`\n").is_empty());
}

// Q-514: top-level code calling what reads a global not yet defined.
#[test]
fn a_call_before_what_it_reads_is_defined_is_said() {
    let d = diags("fn f() = later\nprint(f())\nlet later = 1\nlet h = () => hlate\nprint(h())\nlet hlate = 1\n");
    assert_eq!(d, ["2 `f` reads `later`, which is defined at line 3, after this call", "5 `h` reads `hlate`, which is defined at line 6, after this call"]);
    assert!(diags("let early = 2\nfn g(x) {\n    let mine = x\n    map([1], (y) => y + early + mine)\n}\nprint(g(1))\nfn k() = later\nlet later = 1\nprint(k())\nroute GET / = k()\n").is_empty());
}

// Q-515: argument counts of lambdas, by `let` and given to builtins.
#[test]
fn a_lambdas_arguments_are_counted() {
    let d = diags("let f = (a, b) => a + b\nprint(f(1))\nprint(f(1, 2, 3))\nprint(map([1, 2], (a, b) => a + b))\nprint(reduce([1, 2], 0, (x) => x))\nprint(filter([1], () => true))\nprint(sort_by([1], (a, b) => a))\n");
    assert_eq!(d, [
        "2 missing argument `b` in call to `f`",
        "3 `f` takes 2 arguments, got 3",
        "4 `map` passes its function one item, but this one takes 2: for the index, `for [i, x] in enumerate(xs) => ...`, which gives a list too",
        "5 `reduce` passes the total so far and an item: `reduce(xs, 0, (total, x) => total + x)`",
        "6 `filter` passes its function one item, but this one takes 0",
        "7 `sort_by` passes one item, not two to compare: give what to sort by, `sort_by(xs, (x) => x.n)`",
    ]);
    // Defaults may be left out; a reassigned variable is not held to its first lambda.
    assert!(diags("print(map([1, 2], (x, y = 0) => x + y))\nprint(reduce([1, 2], 0, (t, x) => t + x))\nlet k = (x) => x\nk = (x, y) => x\nprint(k(1, 2))\n").is_empty());
}

// What the review found refused or warned of wrongly: closures returned, a local fn's
// parameter, record defaults, branches not taken, a lambda reassigned, `1` against `1.0`,
// renames in route patterns, `onload`, `ismap`, `x-data`, SVG filters, `@changed`, a path's
// parameter shadowed, a loop a closure ends. None of it is said.
#[test]
fn valid_programs_the_review_found_stay_valid() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/programs/f45_valid.qwe")).unwrap();
    assert_eq!(diags(&src), Vec::<String>::new());
}

// ---- the messages' part ------------------------------------------------------------------------

// Q-517: "did you mean" is the same every run, counts a swap as one edit, knows other
// languages' names, and is given for module functions, methods, fields and types.
#[test]
fn suggestions_are_steady_and_near() {
    for (src, want) in [
        ("let x = lne(\"a\")\n", "(did you mean `len`?)"),
        ("pirnt(1)\n", "(did you mean `print`?)"),
        ("let y = None\n", "(did you mean `null`?)"),
        ("print(True)\n", "(did you mean `true`?)"),
        ("print(undefined)\n", "(did you mean `null`?)"),
        ("console.log(1)\n", "(did you mean `print`?)"),
        ("print(parseInt(\"1\"))\n", "(did you mean `int`?)"),
        ("print(json.prase(\"1\"))\n", "(did you mean `json.parse`?)"),
        ("print(math.sqr(4))\n", "(did you mean `math.sqrt`?)"),
        ("let s = \"a\"\nprint(s.toUpperCase())\n", "(did you mean `upper`?)"),
        ("let xs = [1]\nprint(xs.lenght)\n", "`len(x)` is how many it holds"),
    ] {
        for _ in 0..20 {
            let d = diags(src);
            assert!(d.iter().any(|w| w.contains(want)), "{} {:?}", src, d);
        }
    }
}

// Q-519: a parameter's type written another language's way.
#[test]
fn types_written_another_way_say_how_they_are_written() {
    for (src, want) in [
        ("fn f(int: a) = a\n", "`int: a` has the name and type swapped: write `a: int`"),
        ("fn f(a int) = a\n", "a parameter's type follows its name after `:`: `a: int`"),
        ("fn f(int a) = a\n", "a parameter's type follows its name after `:`: `a: int`"),
        ("fn f(a: int[]) = a\n", "a list's item type goes in `<>`: `a: list<int>`"),
        ("fn f(a: list[int]) = a\n", "a list's item type goes in `<>`: `a: list<int>`"),
        ("fn f(a?: int) = a\n", "`?` goes after the type"),
        ("fn f(a: ?int) = a\n", "`?` goes after the type: `a: int?`"),
        ("fn f(a: int): int = a\n", "a function's result type follows `->`"),
        ("fn f(a: integer) = a\n", "unknown type `integer`: did you mean `int`?"),
        ("fn f(a: string) = a\n", "unknown type `string`: did you mean `str`?"),
        ("fn f(a: flaot) = a\n", "unknown type `flaot`: did you mean `float`?"),
        ("fn f(a: Int) = a\n", "there is no type `Int`: the builtin types are written in small letters, `int`"),
        ("let f = (a: string) => a\n", "unknown type `string`: did you mean `str`?"),
        ("let f = (int: a) => a\n", "`int: a` has the name and type swapped: write `a: int`"),
        ("type User = { str: name }\n", "`str: name` has the name and type swapped: write `name: str`"),
        ("let x: int = 1\n", "a `let` takes no type, but its value's"),
        ("component C() {\n    state n: int = 0\n    <p>{n}</p>\n}\n", "a `state` takes no type, but its value's"),
    ] {
        let d = diags(src);
        assert!(d.iter().any(|w| w.contains(want)), "{} {:?}", src, d);
    }
    // Written the right way, or names that are types' too: nothing said.
    assert!(diags("fn f(a: int?, b: list<int> = [], list, str) = a\nlet g = (a: int, b) => a + b\nlet h = (x): int => x\nlet yes = true\nlet t = (yes ? 2 : 3)\ntype Int = { n: int }\nfn k(a: Int) = a\n").is_empty());
}

// Q-520, Q-008: other languages' words and characters say what Quillo writes, with no note
// that the code may be for a newer Quillo.
#[test]
fn other_languages_words_say_what_quillo_writes() {
    for (src, want) in [
        ("const x = 1\n", "there is no `const` or `var`: `let x = 1`"),
        ("var x = 1\n", "there is no `const` or `var`"),
        ("fun add(a) = a\n", "a function is `fn name(x) = ...`"),
        ("retrun 1\n", "`retrun` is no keyword: did you mean `return`?"),
        ("lett x = 1\n", "`lett` is no keyword: did you mean `let`?"),
        ("if true { 1 } elif false { 2 }\n", "after `if`, `else if`"),
        ("switch x { 1 => 2 }\n", "there is no `switch`"),
        ("let x = 1 === 1\n", "there is no `===`"),
        ("let a = 1\na++\n", "there is no `++`: `n += 1`"),
        ("let n = 1\nn *= 2\n", "there is no `*=`: `n = n * 2`"),
        ("print(.5)\n", "a number starts with a digit: `0.5`, not `.5`"),
        ("print(0b101)\n", "there are no binary or octal numbers"),
        ("print(1 & 2)\n", "`&&` is and; for bits, `bit.and(a, b)`"),
        ("print(typeof 1)\n", "`type_of(x)` gives a value's type"),
        ("let x = 1\nif x = 1 { print(1) }\n", "`=` gives a value; to compare, `==`"),
        ("let x = true\nlet y = x ? 1\n", "`?` needs `:` and a value for when it does not hold"),
        ("type user = { name: str }\n", "a type's name starts with a capital letter: `type User = ...`"),
        ("let x = match 1 { case 1 => 2 }\n", "there is no `case`"),
        ("let x = match 1 { 1: 2 }\n", "a `match` arm is a pattern, `=>` and its value"),
        ("let x = match 1 { 1 -> 2 }\n", "a `match` arm is a pattern, `=>` and its value"),
        ("let x = match 1 { n where n > 0 => n }\n", "a guard is written with `if`"),
        ("import { price } from \"lib/m.qwe\"\n", "the file comes first, then the names: `import lib/m.qwe use price`"),
        ("import price from \"lib/m.qwe\"\n", "`import lib/m.qwe use price`"),
        ("from lib/m.qwe import price\n", "`import lib/m.qwe use price`"),
        ("print(“hello”)\n", "a curly quote: write `\"`"),
        ("print(5 – 2)\n", "a dash or a minus sign: write `-`"),
        ("print（1）\n", "a full-width one: write `(`"),
        ("let x = 1 \\\n+ 2\n", "a line goes on by itself after an operator"),
    ] {
        let r = check_source(src, "t.qwe", Path::new("."));
        let d: Vec<String> = r.diagnostics.iter().map(|d| d.message.clone()).collect();
        assert!(d.iter().any(|w| w.contains(want)), "{} {:?}", src, d);
        // A message that says what to write names a rule: no "newer Quillo" note.
        assert!(r.syntax_errors.iter().all(|(e, _)| !quillo::error::may_be_newer_syntax(e)), "{} {:?}", src, r.syntax_errors);
    }
    // What is right stays right: ranges, indexes, `&&`, ternaries, `?` passing an error on, `use`.
    assert!(diags("let r = 1..5\nlet xs = [1, 2]\nprint(xs[0] + 1.5, true && false, true ? 1 : 2)\nfn f(x) = x?\nlet type = 1\nprint(4/2)\n").is_empty());
}

// Q-522: the syntax errors after the first, each from the next statement on; the first first.
#[test]
fn several_syntax_errors_are_said_in_one_run() {
    let lines = |src: &str| check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().map(|d| d.line).collect::<Vec<_>>();
    assert_eq!(lines("let a = (1\nlet b = 2 +\nlet c = [1 2]\nfn f( = 1\nlet ok = 1\n"), [2, 3, 4]);
    assert_eq!(lines("fn f() {\n  let x = (1\n  print(x)\n}\nfn g() = [1 2]\nfn h() = 3\n"), [3, 5]);
    // One error, at the end: said once.
    assert_eq!(lines("let a = 1\nlet b = (\n"), [3]);
    // At most ten.
    let many: String = (0..30).map(|_| "let x = (1\n").collect();
    assert_eq!(lines(&many).len(), 10);
}

// Q-523: an import line's mistake said there, once, by the path written; the right fix for a
// missing `use`; a `use` list does not run on into the next line; another file's line named.
#[test]
fn import_mistakes_are_said_at_the_import_line() {
    let dir = std::env::temp_dir().join(format!("quillo-f45-imports-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/m.qwe"), "fn price(x) = x * 2\nprivate fn secret() = 1\nfn cost(x) = x\n").unwrap();
    let d = |src: &str| diags_in(src, &dir);
    assert_eq!(d("import lib/m.qwe use pricee\nprint(pricee(1))\n"), ["1 lib/m.qwe has no public name `pricee` (did you mean `price`?)"]);
    assert_eq!(d("import lib/m.qwe use secret\nprint(secret())\n"), ["1 `secret` is private to lib/m.qwe: make it `public` there, or take something public"]);
    let missing = d("import lib/mm.qwe use price\nprint(price(1))\n");
    assert_eq!(missing.len(), 1, "{:?}", missing);
    assert!(missing[0].starts_with("1 cannot read lib/mm.qwe") && missing[0].ends_with("(did you mean lib/m.qwe?)"), "{:?}", missing);
    assert_eq!(d("import lib/m.qwe price\n"), ["1 `import` takes names from a file, and says which after `use`: import lib/m.qwe use price"]);
    assert_eq!(d("import lib/m.qwe use *, price\n"), ["1 `*` takes every public name: write `use *` alone, or name each one"]);
    assert_eq!(d("import lib/m.qwe use price,\nprint(price(1))\n"), ["1 this `use` list ends with `,`: drop it, or name what else to take on the next line"]);
    assert_eq!(d("import lib/m.qwe use price,\nlet x = 1\n"), ["1 this `use` list ends with `,`: drop it, or name what else to take on the next line"]);
    assert_eq!(d("print(price(1))\nuse lib/m.qwe\n"), ["1 `price` is used before it is defined (lib/m.qwe:1)"]);
    // The old name after `as`, and a `use` twice: warnings, and the program runs.
    assert_eq!(d("import lib/m.qwe use price as p\nprint(price(1))\nprint(price(2))\n"), ["2 warning: this file takes `price` as `p`: write `p`, or take `price` too"]);
    assert_eq!(d("import lib/m.qwe use price\nimport lib/m.qwe use price\nprint(price(1))\n"), ["2 warning: `price` is taken a second time from lib/m.qwe: drop this one"]);
    assert_eq!(d("use lib/m.qwe\nuse lib/m.qwe\nprint(price(1))\n"), ["2 warning: lib/m.qwe is taken a second time in this file: once is enough"]);
    // What is right stays right: a list over lines, `use *`, both names, a local or parameter of
    // the old name, and a builtin's name taken as another.
    for ok in [
        "import lib/m.qwe use price,\n    cost\nprint(price(cost(1)))\n",
        "import lib/m.qwe use price,\n    cost as c\nprint(price(c(1)))\n",
        "import lib/m.qwe use *\nprint(price(1))\n",
        "import lib/m.qwe use price as p, price\nprint(price(p(1)))\n",
        "import lib/m.qwe use price as p\nfn f(price) = price + 1\nlet price = 3\nprint(f(p(price)))\nprint([1].map((price) => price))\n",
        "import lib/m.qwe use price, cost as p\nprint(price(p(1)))\n",
    ] {
        assert!(d(ok).is_empty(), "{} {:?}", ok, d(ok));
    }
    std::fs::write(dir.join("lib/n.qwe"), "fn len(x) = 0\n").unwrap();
    // (Defining `len` is warned of in lib/n.qwe; `len` here is still not an old name.)
    let builtin = d("import lib/n.qwe use len as size\nprint(len([1]), size(1))\n");
    assert!(builtin.iter().all(|w| !w.contains("takes `len` as")), "{:?}", builtin);
    let _ = std::fs::remove_dir_all(&dir);
}

// Q-524: a name read before its `let` in a function, or after the block that declared it, says
// so and is not called unused; a lookalike letter is named by its code point.
#[test]
fn names_out_of_their_scope_say_where_they_are() {
    assert_eq!(diags("fn f() {\n    print(tmp)\n    let tmp = 1\n}\nf()\n"), ["2 `tmp` is used before it is defined (line 3)"]);
    // From a lambda in the function, before the `let`: it runs after it, so fine, and `soon`
    // is not unused (found in review).
    assert!(diags("fn f() {\n    let g = () => soon\n    let soon = 1\n    print(g())\n}\nf()\n").is_empty());
    assert_eq!(
        diags("if true {\n    let x = 1\n} else {\n    let z = 2\n}\nprint(x, z)\n"),
        [
            "6 `x` is declared inside the `if` (line 2), and is not seen after it: declare it before the `if` (`let x = ...`), and assign it inside",
            "6 `z` is declared inside the `if` (line 4), and is not seen after it: declare it before the `if` (`let z = ...`), and assign it inside",
        ]
    );
    assert_eq!(
        diags("fn f() {\n    for i in [1] {\n        let y = i\n    }\n    return y\n}\nprint(f())\n"),
        ["5 `y` is declared inside the `for` (line 3), and is not seen after it: declare it before the `for` (`let y = ...`), and assign it inside"]
    );
    assert_eq!(
        diags("fn f() {\n    let n = 0\n    while n < 3 {\n        let m = n\n        n += 1\n    }\n    print(m)\n}\nf()\n"),
        ["7 `m` is declared inside the `while` (line 4), and is not seen after it: declare it before the `while` (`let m = ...`), and assign it inside"]
    );
    assert_eq!(
        diags("let а = 1\nprint(a)\n"),
        ["2 unknown name `a` (`а` is defined on line 1 with other letters that look the same: the `а` on line 1 has U+0430, a Cyrillic letter, not `a`)"]
    );
    assert_eq!(
        diags("let a = 1\nprint(а)\n"),
        ["2 unknown name `а` (`a` is defined on line 1 with other letters that look the same: this one has U+0430, a Cyrillic letter, not `a`)"]
    );
    assert_eq!(
        diags("let Οk = 1\nprint(Ok)\n"),
        ["2 unknown name `Ok` (`Οk` is defined on line 1 with other letters that look the same: the `Οk` on line 1 has U+039F, a Greek letter, not `O`)"]
    );
    // Still as they were: a name nothing defines, one in another function, a `let` in a block
    // after the read, a block's own unused `let`, and one read where it is seen.
    assert_eq!(diags("fn f() {\n    print(nope)\n}\nf()\n"), ["2 unknown name `nope` (did you mean `now`?)"]);
    assert_eq!(diags("fn f() {\n    let q = 1\n    print(q)\n}\nfn g() = q\nprint(f(), g())\n"), ["5 unknown name `q`"]);
    let d = diags("fn f() {\n    print(tmp)\n    if true {\n        let tmp = 1\n    }\n}\nf()\n");
    assert!(d[0].starts_with("2 unknown name `tmp`") && d[1] == "4 warning: unused variable `tmp` (name it `_tmp` if that is intended)", "{:?}", d);
    assert_eq!(diags("fn f() {\n  let u = 1\n}\nf()\n"), ["2 warning: unused variable `u` (name it `_u` if that is intended)"]);
    assert!(diags("fn f() {\n    let x = 0\n    if true {\n        let y = 1\n        print(y)\n    }\n    return x\n}\nprint(f())\nlet ü = 1\nprint(ü)\n").is_empty());
    // A lambda reading a `let` above it, called after: as before, fine.
    assert!(diags("fn f() {\n    let n = 2\n    let g = () => n\n    print(g())\n}\nf()\n").is_empty());
}

// Q-525: builtins' parameters by their docs' names, the call shown, one error for `max(1, 2)`,
// the start value of `reduce`, named arguments, `+=` and `-=` held to the first value's type.
#[test]
fn builtin_calls_say_what_they_take() {
    for (src, want) in [
        ("print(map([1, 2]))\n", "1 missing argument `f`, a function, in call to `map`: map(xs, f)"),
        ("let xs = [1]\nprint(reduce(xs, (a, x) => a + x))\n", "2 `reduce` takes `start` before the function: reduce(xs, start, f)"),
        ("print(replace(\"aa\", \"a\"))\n", "1 missing argument `to` in call to `replace`: replace(s, from, to)"),
        ("print(max(1, 2))\n", "1 `max` takes one list: put the items in it, `max([...])`"),
        ("print(sum(1, 2, 3))\n", "1 `sum` takes one list: put the items in it, `sum([...])`"),
        ("print(str())\n", "1 missing argument `x` in call to `str`: str(x)"),
        ("print(upper(\"a\", \"b\"))\n", "1 `upper` takes 1 argument, got 2: upper(s)"),
        ("fn f(a) = a\nprint(f(a: 1))\n", "2 there are no named arguments: give the value alone, `f(...)`, or pass a map with the names, `f({ a: ... })`"),
        ("let n = 1\nn -= \"a\"\n", "2 cannot subtract int and str"),
        ("let xs = [1]\nxs += 2\n", "2 cannot add list and int"),
    ] {
        assert_eq!(diags(src), [want], "{}", src);
    }
    // A program's own function keeps its parameters' names, and no shape.
    assert_eq!(diags("fn f(a, b) = a\nprint(f(1))\n"), ["2 missing argument `b` in call to `f`"]);
    // What `+=` and `-=` may do stays allowed: a float added to an int, text to text, a list to
    // a list, and any once `=` gives the variable another value.
    assert!(diags("let n = 1\nn += 0.5\nlet s = \"\"\ns += \"a\"\nlet xs = []\nxs += [1]\nlet m = 1\nm = \"x\"\nm += \"y\"\nprint(n, s, xs, m, str(1), max([1, 2]), reduce([1], 0, (a, x) => a + x))\n").is_empty());
}

// The review of slice C: what its first version said wrongly.
#[test]
fn the_reviews_cases_for_imports_and_scopes() {
    let dir = std::env::temp_dir().join(format!("quillo-f45-review-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/m.qwe"), "fn price(x) = x * 2\n").unwrap();
    std::fs::write(dir.join("lib/sub.qwe"), "fn helper() = pricee\n").unwrap();
    let d = |src: &str| diags_in(src, &dir);
    // A prop, a view's parameter or a local of the old name is the file's own.
    assert!(d("import lib/m.qwe use price as p\ncomponent Card(price: int) {\n    <div>{price}</div>\n}\nview Home() = <div><Card price={p(3)}/></div>\n").is_empty());
    // A local of the other file's name is what the alias finds: still said.
    assert_eq!(d("import lib/m.qwe use price as p\nroute GET /item/:price {\n    let n = int(price) ?? 0\n    p(n)\n}\n"), ["4 cannot call `price`: it is str, not a function"]);
    // A name another file's import could not take is still said in this one.
    let sub = d("import lib/m.qwe use pricee\nimport lib/sub.qwe use helper\nprint(helper())\n");
    assert!(sub.iter().any(|w| w.contains("unknown name `pricee`")), "{:?}", sub);
    let _ = std::fs::remove_dir_all(&dir);
    // A `let` in a sibling block does not define what the other read; a closure may read a
    // `let` below it.
    assert_eq!(diags("fn a() {\n    if true { print(x) }\n    if false {\n        let x = 1\n        print(x)\n    }\n}\na()\n"), ["2 unknown name `x`"]);
    assert!(diags("fn c() {\n    let g = () => z\n    let z = 1\n    return g()\n}\nprint(c())\n").is_empty());
    // A name of the program's is not a slipped keyword; `len` takes more than lists.
    assert_eq!(diags("fn f(x) = x\nf 2\n"), ["2 expected end of statement, found `2`"]);
    assert_eq!(diags("print(len(\"a\", \"b\"))\n"), ["1 `len` takes 1 argument, got 2: len(x)"]);
    assert_eq!(diags("type Éclair = { a: int }\n"), ["1 a type's name starts with a capital letter from A to Z, as `User`: `Éclair` does not"]);
    assert_eq!(diags("type _x = { a: int }\n"), ["1 a type's name starts with a capital letter, as `User`: `_x` does not"]);
    // A template's line that starts like a statement is its text.
    let lines: Vec<u32> = check_source("let a = 1 +\nlet doc = md`\nExample:\nlet x = 1\n`\nlet b = [1, 2\nprint(doc)\nfn ok() = `the end`\n", "t.qwe", Path::new(".")).diagnostics.iter().map(|d| d.line).collect();
    assert_eq!(lines, [2, 7]);
}
