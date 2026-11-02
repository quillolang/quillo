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
    assert_eq!(run("let x = 1\r\nlet y = x + 2\r\ny"), "3");
}

// Q-506: `/a/{id}` was the path `/a/` and a request pattern, and matched nothing.
#[test]
fn a_braced_path_parameter_says_how_one_is_written() {
    assert_eq!(diags("route GET /a/{id} = id\n"), ["1 path parameters are written `:id`, as in `/users/:id`, not `{id}`"]);
    assert_eq!(diags("route GET /a/{id:int} = id\n"), ["1 path parameters are written `:id(int)`, as in `/users/:id(int)`, not `{id:int}`"]);
    assert_eq!(diags("route GET /{x}/b = x\n"), ["1 path parameters are written `:x`, as in `/users/:x`, not `{x}`"]);
    // A request pattern stays one, after a space or written out.
    assert!(diags("route POST / { json: { a: str } } = a\nroute POST /{ json: { b: str } } = b\nroute GET /c/:id(int) = id\n").is_empty());
}

// Q-510: `"Hello ${name}"` is text; said when `name` is one of the program's here.
#[test]
fn a_template_written_in_quotes_is_pointed_out() {
    let d = diags("let name = \"Ada\"\nprint(\"Hello ${name}\")\nprint('Hi ${name}')\nfn f(user) = \"Dear ${user.name}\"\n");
    assert_eq!(d.len(), 3, "{:?}", d);
    assert_eq!(d[0], "2 warning: this string is not a template, so `${name}` stays as text: write it in backquotes, `` `Hello ${name}` ``, or `\\${` to keep the text");
    assert!(d[2].starts_with("4 warning: this string is not a template, so `${user}`"), "{}", d[2]);
    // Not: `\${`, a name the program has not, a builtin's, a real template, a `$` alone.
    assert!(diags("let name = 1\nprint(\"\\${name}\")\nprint(\"${nobody}\")\nprint(\"${print}\")\nprint(`${name}`)\nprint(\"$5 and ${ name }\")\n").is_empty());
    // It stays the text it was.
    assert_eq!(run("let name = 1\n\"a ${name}\""), "\"a ${name}\"");
}

// Q-507: a name imported as a builtin's replaced it everywhere, without a word.
#[test]
fn an_import_named_as_a_builtin_is_warned_of() {
    let dir = std::env::temp_dir().join(format!("quillo-f45-import-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/m.qwe"), "fn price(x) = x * 2\n").unwrap();
    assert_eq!(
        diags_in("import lib/m.qwe use price as print\n", &dir),
        ["1 warning: `print` hides the builtin `print`: taken under that name, `price` is the program's `print` everywhere, so take it under its own name (`use price`) or another"]
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
