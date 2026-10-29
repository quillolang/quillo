use quillo::eval_str;

fn ev(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

fn ev_err(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => e.to_string(),
    }
}

#[test]
fn parallel_gives_results_in_order() {
    assert_eq!(ev("let a = 10\nparallel(() => a + 1, () => a * 2, () => \"x\")"), "[11, 20, \"x\"]");
    assert_eq!(ev("parallel(() => parallel(() => 1, () => 2), () => 3)"), "[[1, 2], 3]");
    assert_eq!(ev("parallel(() => err(\"no\"), () => 2)"), "[err(\"no\"), 2]");
    assert_eq!(ev("parallel()"), "[]");
    assert!(ev_err("parallel(() => 1, () => 1 / 0)").contains("division by zero"));
    assert!(ev_err("parallel(() => 1, 2)").contains("argument 2 is int"));
}

#[test]
fn arithmetic_and_precedence() {
    assert_eq!(ev("1 + 2 * 3"), "7");
    assert_eq!(ev("(1 + 2) * 3"), "9");
    // `/` divides exactly, as in JavaScript; whole results stay ints.
    assert_eq!(ev("[7 / 2, 6 / 2, -7 / 2, 7.0 / 2, floor(7 / 2), 1 / 4]"), "[3.5, 3, -3.5, 3.5, 3, 0.25]");
    assert_eq!(ev("-3 + 1"), "-2");
    assert_eq!(ev("10 % 3"), "1");
    assert_eq!(ev("1 < 2 && !(3 > 4)"), "true");
    assert_eq!(ev("!true == false"), "true");
    assert_eq!(ev("0..4"), "[0, 1, 2, 3]");
    assert!(ev_err("1 / 0").contains("division by zero"));
}

#[test]
fn and_or_give_the_deciding_operand() {
    assert_eq!(ev("[null && 1, 0 && 2, 1 || 2, null || \"anonymous\", false || null, true && \"yes\"]"),
        "[null, 2, 1, \"anonymous\", null, \"yes\"]");
    // Errors are falsy, so `||` gives a fallback, like `??`.
    assert_eq!(ev("int(\"x\") || 0"), "0");
    assert_eq!(ev("let calls = 0\nfn hit() { calls += 1\n true }\nfalse && hit()\ntrue || hit()\ncalls"), "0");
}

#[test]
fn strings_and_templates() {
    assert_eq!(ev("let name = \"Ada\"\n`Hi ${name}, ${1 + 1}`"), "\"Hi Ada, 2\"");
    assert_eq!(ev(r#""a" + 1"#), "\"a1\"");
    // Plain strings are plain: braces and `${}` are just text.
    assert_eq!(ev(r#""{braces} and ${x}""#), "\"{braces} and ${x}\"");
    assert_eq!(ev(r#"'single "quotes"'"#), "\"single \\\"quotes\\\"\"");
    assert_eq!(ev(r#"`nested ${ `inner ${1}` } and ${ "q" }`"#), "\"nested inner 1 and q\"");
    assert_eq!(ev(r#"`literal \${x} and \``"#), "\"literal ${x} and `\"");
    assert_eq!(ev("`two\nlines`"), "\"two\\nlines\"");
}

#[test]
fn lists_and_maps() {
    assert_eq!(ev("[1, 2, 3][-1]"), "3");
    assert_eq!(ev("let id = 7\n{ id, name: \"x\" }"), "{ id: 7, name: \"x\" }");
    assert_eq!(ev("{ a: 1 } + { b: 2 }"), "{ a: 1, b: 2 }");
    assert_eq!(ev("{ a: { b: 5 } }.a.b"), "5");
    assert_eq!(ev("{ a: 1 }.missing"), "null");
    assert_eq!(ev("null.anything.at.all"), "null");
    assert_eq!(ev("{ \"content-type\": \"x\" }[\"content-type\"]"), "\"x\"");
}

#[test]
fn functions_defaults_and_types() {
    assert_eq!(ev("fn add(a: int, b: int = 10) -> int = a + b\nadd(1)"), "11");
    assert_eq!(ev("fn add(a, b) { let s = a + b\n s * 2 }\nadd(1, 2)"), "6");
    assert!(ev_err("fn f(a: int) = a\nf(\"x\")").contains("argument `a` of `f` must be int"));
    assert!(ev_err("fn f() -> int = \"no\"\nf()").contains("must return int"));
    assert!(ev_err("fn f(a) = a\nf()").contains("missing argument `a`"));
    assert_eq!(ev("fn f(a: str?) = a ?? \"none\"\nf(null)"), "\"none\"");
    assert_eq!(ev("fn f(a: int | null) = a\nf(null)"), "null");
}

#[test]
fn arrow_functions() {
    assert_eq!(ev("let double = (x) => x * 2\ndouble(4)"), "8");
    assert_eq!(ev("let double = x => x * 2\ndouble(4)"), "8");
    assert_eq!(ev("let add = (a: int, b = 10): int => a + b\n[add(1), add(1, 2)]"), "[11, 3]");
    assert_eq!(ev("let f = () => {\n let x = 2\n x * 3\n}\nf()"), "6");
    assert_eq!(ev("let point = () => { x: 1, y: 2 }\npoint().y"), "2");
    assert!(ev_err("let f = (a: int) => a\nf(\"x\")").contains("must be int"));
    // Arrows inside `for` and match guards still read as arrows where they can.
    assert_eq!(ev("for x in [1, 2].map(y => y * 10) => x + 1"), "[11, 21]");
    assert_eq!(ev("let ok = true\nmatch 1 { n if ok => n + 1 }"), "2");
}

#[test]
fn ternary_and_the_try_operator() {
    assert_eq!(ev("1 > 2 ? \"a\" : \"b\""), "\"b\"");
    assert_eq!(ev("let n = 0\nn > 0 ? \"pos\" : n == 0 ? \"zero\" : \"neg\""), "\"zero\"");
    assert_eq!(ev("let x = null\nx ?? 1 ? \"set\" : \"unset\""), "\"set\"");
    assert_eq!(ev("{ a: true ? 1 : 2, b: 3 }"), "{ a: 1, b: 3 }");
    // A multi-line ternary, JavaScript style.
    assert_eq!(ev("let v = 5\nlet size = v > 3\n    ? \"big\"\n    : \"small\"\nsize"), "\"big\"");
    // `?` with no `:` after it still returns errors early.
    let src = "fn parse(s) = int(s)\nfn plus1(s) = parse(s)? + 1\nfn minus1(s) = parse(s)? - 1\n[plus1(\"4\"), minus1(\"4\"), plus1(\"x\")]";
    assert_eq!(ev(src), "[5, 3, err(\"not an int: \\\"x\\\"\")]");
    assert_eq!(ev("fn f(s) = { v: int(s)?, ok: true }\n[f(\"1\"), f(\"z\")]"), "[{ v: 1, ok: true }, err(\"not an int: \\\"z\\\"\")]");
}

#[test]
fn old_syntax_gets_a_hint() {
    assert!(ev_err("1 and 2").contains("use `&&` instead of `and`"));
    assert!(ev_err("1 or 2").contains("use `||` instead of `or`"));
    assert!(ev_err("not true").contains("use `!x` instead of `not x`"));
    assert!(ev_err("nil").contains("use `null` instead of `nil`"));
    assert!(ev_err("if true then 1 else 2").contains("`c ? a : b`"));
    assert!(ev_err("let f = fn(x) = x").contains("`(x) => x * 2`"));
    assert!(ev_err("fn f(x: int | nil) = x").contains("use `null`"));
    assert!(ev_err("[1] |> len").contains("the pipe is now a single `|`"));
}

#[test]
fn closures_and_recursion() {
    assert_eq!(ev("fn fib(n) = n < 2 ? n : fib(n - 1) + fib(n - 2)\nfib(20)"), "6765");
    assert_eq!(ev("fn adder(n) = (x) => x + n\nlet add5 = adder(5)\nadd5(1)"), "6");
}

#[test]
fn pipelines() {
    assert_eq!(ev("[1, 2, 3] | map((x) => x * 2) | sum"), "12");
    assert_eq!(ev(r#""a,b,c" | split(",") | join("-")"#), "\"a-b-c\"");
    assert_eq!(
        ev("[5, 3, 8]\n    | filter((x) => x > 4)\n    | sort\n    | first"),
        "5"
    );
    assert_eq!(ev("[1, 2, 3]\n    .filter(x => x > 1)\n    .map(x => x * x)"), "[4, 9]");
}

#[test]
fn method_call_syntax() {
    assert_eq!(ev("[3, 1, 2].sort().first()"), "1");
    assert_eq!(ev("\"hey\".upper"), "\"HEY\"");
    assert_eq!(ev("[1, 2, 3].len"), "3");
    assert_eq!(ev("let m = { greet: (n) => `hi ${n}` }\nm.greet(\"bo\")"), "\"hi bo\"");
    assert_eq!(ev("{ a: 1, b: 2 }.keys"), "[\"a\", \"b\"]");
}

#[test]
fn match_patterns() {
    let src = r#"
fn describe(x) = match x {
    0 => "zero"
    int as n if n < 0 => `negative ${n}`
    int => "positive"
    "a" | "b" => "letter"
    [first, ..rest] => `list starting ${first}, ${len(rest)} more`
    { type: "user", name } => `user ${name}`
    { customer.id: id } => `customer ${id}`
    null => "nothing"
    _ => "other"
}
[describe(0), describe(-2), describe(9), describe("b"), describe([1, 2, 3]),
 describe({ type: "user", name: "Ada" }), describe({ customer: { id: 4 } }), describe(null), describe(true)]
"#;
    assert_eq!(
        ev(src),
        r#"["zero", "negative -2", "positive", "letter", "list starting 1, 2 more", "user Ada", "customer 4", "nothing", "other"]"#
    );
    assert!(ev_err("match 1 { 2 => 3 }").contains("no `match` arm"));
}

#[test]
fn let_destructuring() {
    assert_eq!(ev("let [a, b] = [1, 2]\na + b"), "3");
    assert_eq!(ev("let { x, y: why } = { x: 1, y: 2 }\nx + why"), "3");
}

#[test]
fn errors_coalesce_and_try() {
    assert_eq!(ev("null ?? 5"), "5");
    assert_eq!(ev(r#"int("x") ?? 0"#), "0");
    assert_eq!(ev(r#"int("42")"#), "42");
    let src = r#"
fn parse_age(s: str) -> int ! str =
    match int(s) {
        int as n if n >= 0 => n
        _ => err(`bad age: ${s}`)
    }
fn next_year(s) = parse_age(s)? + 1
[next_year("41"), next_year("old")]
"#;
    assert_eq!(ev(src), r#"[42, err("bad age: old")]"#);
    assert!(ev_err("unwrap(err(\"boom\"))").contains("boom"));
}

#[test]
fn control_flow() {
    assert_eq!(ev("if 1 > 2 { \"a\" } else if 2 > 1 { \"b\" } else { \"c\" }"), "\"b\"");
    assert_eq!(ev("let x = if false { 1 }\nx"), "null");
    assert_eq!(ev("for x in [1, 2, 3] => x * x"), "[1, 4, 9]");
    assert_eq!(ev("for [k, v] in { a: 1, b: 2 } => `${k}=${v}`"), "[\"a=1\", \"b=2\"]");
    assert_eq!(ev("fn f(x) { if x > 0 { return \"pos\" }\n \"other\" }\n[f(1), f(0)]"), "[\"pos\", \"other\"]");
}

#[test]
fn assignment() {
    assert_eq!(ev("let n = 1\nn += 2\nn"), "3");
    assert_eq!(ev("let xs = []\nfn add(x) { xs = xs | push(x) }\nadd(1)\nadd(2)\nxs"), "[1, 2]");
    assert!(ev_err("y = 1").contains("never declared"));
}

#[test]
fn html_literals() {
    assert_eq!(ev(r#"<p.note #n-{1 + 1}>a &lt; b</p>"#), r#"<p id="n-2" class="note">a &lt; b</p>"#);
    assert_eq!(
        ev(r#"let x = "<script>"
<div title={x} hidden data-n={1}>{x}<br/><img src="a.png"></div>"#),
        r#"<div title="&lt;script&gt;" hidden data-n="1">&lt;script&gt;<br><img src="a.png"></div>"#
    );
    // Attribute strings are plain text, as in JSX.
    assert_eq!(ev(r#"<a href="/x/{id}">link</a>"#), r#"<a href="/x/{id}">link</a>"#);
    assert_eq!(ev(r#"<ul>{for i in [1, 2] => <li>{i}</li>}</ul>"#), "<ul><li>1</li><li>2</li></ul>");
    assert_eq!(ev(r#"let ok = false
<b disabled={ok} class={ok && "on"}>{ok ? "yes" : null}</b>"#), "<b></b>");
    assert_eq!(ev(r#"<div>{raw("<em>trusted</em>")}</div>"#), "<div><em>trusted</em></div>");
    assert_eq!(ev(r#"<html><body>x</body></html>"#), "<!doctype html><html><body>x</body></html>");
    // Quotes in text are text, as in JSX.
    assert_eq!(ev(r#"<q>"hi"</q>"#), "<q>&quot;hi&quot;</q>");
}

#[test]
fn html_text_follows_jsx_whitespace_rules() {
    let src = r#"
let name = "Ada"
<p>
    Hello, {name}!
    How are
    you?   Fine.
    <b>bold</b> and <i>italic</i>, then {name}'s turn
</p>
"#;
    assert_eq!(
        ev(src),
        "<p>Hello, Ada! How are you?   Fine.<b>bold</b> and <i>italic</i>, then Ada&#39;s turn</p>"
    );
    assert_eq!(ev("<p>Fish &amp; chips &mdash; &#169; &#x263A; &nope;</p>"), "<p>Fish &amp; chips — © ☺ &amp;nope;</p>");
    assert!(ev_err("<p>a } b</p>").contains("unexpected `}`"));
}

#[test]
fn html_multiline_and_nested_conditionals() {
    let src = r##"
let todo = { id: 3, text: "ship", done: true }
<li.todo #todo-{todo.id}>
    {todo.done ? <s>{todo.text}</s> : todo.text}
    <span.meta>#{todo.id}</span>
</li>
"##;
    assert_eq!(
        ev(src),
        r#"<li id="todo-3" class="todo"><s>ship</s><span class="meta">#3</span></li>"#
    );
}

#[test]
fn components() {
    let src = r#"
view Card(title: str, highlight = false, children) =
    <section.card class={highlight && "hl"}>
        <h2>{title}</h2>
        {children}
    </section>
<main>
    <Card title="One">first</Card>
    <Card title="Two" highlight><p>second</p></Card>
</main>
"#;
    assert_eq!(
        ev(src),
        r#"<main><section class="card"><h2>One</h2>first</section><section class="card hl"><h2>Two</h2><p>second</p></section></main>"#
    );
    assert!(ev_err("view A(x) = <b>{x}</b>\n<A y=\"1\"/>").contains("has no parameter `y`"));
    assert!(ev_err("<Missing/>").contains("unknown view <Missing>"));
}

#[test]
fn parse_errors_have_lines() {
    let e = ev_err("let a = 1\nlet b = (2 +\n");
    assert!(e.contains("end of input"), "{}", e);
    let e = ev_err("let a = 1\n<div>x</span>");
    assert!(e.contains("<input>:2:") && e.contains("expected `</div>`"), "{}", e);
    // A missing end tag is told at the element that lacks it (branch
    // editor-end-tags).
    let e = ev_err("let a = 1\nlet b = <ul>\n  <li>one\n</ul>");
    assert!(e.contains("<input>:3:") && e.contains("`<li>` is never closed: add `</li>` before `</ul>` on line 4"), "{}", e);
    let e = ev_err("fn f() {\n  <div>\n    <p>x</p>\n}");
    assert!(e.contains("<input>:2:") && e.contains("`<div>` is never closed: add `</div>` before the `}` on line 4"), "{}", e);
    let e = ev_err("view V() =\n  <main>\n    <p>x</p>\n\nroute GET / = <V/>\n");
    assert!(e.contains("<input>:2:") && e.contains("`<main>` is never closed: add `</main>` before `route` on line 5"), "{}", e);
    // A `use` with no quotes starts a declaration too (branch use-editor).
    for u in ["use lib/auth", "use a, b as c from x.qwer", "use * from lib", "use lib"] {
        let e = ev_err(&format!("view V() =\n  <div>x\n{}\n", u));
        assert!(e.contains("<input>:2:") && e.contains("add `</div>` before `use` on line 3"), "{}: {}", u, e);
    }
    let e = ev_err("fn f(x: number) = x");
    assert!(e.contains("unknown type `number`"), "{}", e);
}

#[test]
fn json_module() {
    assert_eq!(ev(r#"json.parse('{"a": [1, 2.5, null]}')"#), "{ a: [1, 2.5, null] }");
    assert_eq!(ev(r#"json.stringify({ a: 1, b: [true] })"#), r#""{\"a\":1,\"b\":[true]}""#);
    assert_eq!(ev(r#"is_err(json.parse("{"))"#), "true");
    // Floats read as the nearest one, as written in a program (the fuzz
    // target `json` found this one read a step off).
    assert_eq!(ev(r#"json.parse("[1.8446744073709502e19, 18446744073709503230, 0.1, 2.2250738585072011e-308]") == [1.8446744073709502e19, 18446744073709503230.0, 0.1, 2.2250738585072011e-308]"#), "true");
}

#[test]
fn runaway_recursion_is_an_error_not_a_crash() {
    let e = ev_err("fn down(n) = 1 + down(n - 1)\ndown(1)");
    assert!(e.contains("too much recursion") && e.contains("`down`"), "{}", e);
    assert_eq!(ev("fn down(n) = n == 0 ? 0 : 1 + down(n - 1)\ndown(100)"), "100");
}

#[test]
fn spread_and_shorthand_attributes() {
    // On elements: written-out attributes win, classes add up.
    assert_eq!(
        ev(r#"let a = { href: "/x", title: "from map", class: "big", "data-n": 1 }
<a.link {...a} title="written">go</a>"#),
        r#"<a class="link big" href="/x" data-n="1" title="written">go</a>"#
    );
    // `{name}` is short for name={name}.
    assert_eq!(ev("let id = \"main\"\nlet hidden = true\n<div {id} {hidden}></div>"), r#"<div id="main" hidden></div>"#);
    // On views: spread keys fill parameters; extra keys are ignored.
    let src = r#"
view Card(title: str, size: str = "m", children) = <section class={size}><h2>{title}</h2>{children}</section>
let node = { type: "card", props: { title: "Stock", size: "l", unrelated: true } }
[<Card {...node.props}>x</Card>, <Card {...node.props} size="s"/>, <Card {...null} title="t"/>]
"#;
    assert_eq!(
        ev(src),
        r#"[<section class="l"><h2>Stock</h2>x</section>, <section class="s"><h2>Stock</h2></section>, <section class="m"><h2>t</h2></section>]"#
    );
    let label = "x";
    let _ = label;
    assert_eq!(ev("view L(label: str) = <i>{label}</i>\nlet label = \"short\"\n<L {label}/>"), "<i>short</i>");
    // Still errors: a misspelled written-out prop, a missing one, a non-map spread.
    assert!(ev_err("view Card(title: str) = <b>{title}</b>\n<Card {...{ title: \"a\" }} titel=\"b\"/>").contains("has no parameter `titel`"));
    assert!(ev_err("view Card(title: str) = <b>{title}</b>\n<Card {...{ other: 1 }}/>").contains("missing argument `title`"));
    assert!(ev_err("<p {...5}></p>").contains("`{...}` in <p> needs a map, got int"));
    assert!(ev_err("<p {1 + 1}></p>").contains("write {name} (short for name={name}) or {...map}"));
}

#[test]
fn pattern_defaults_are_taken_as_written() {
    assert_eq!(ev("match {} { { limit: int = null } => limit }"), "null");
    assert_eq!(ev("match {} { { limit: int = 20 } => limit }"), "20");
    assert_eq!(ev("match { limit: 5 } { { limit: int = null } => limit }"), "5");
    // A value that is there must still match.
    assert!(ev_err("match { limit: \"x\" } { { limit: int = null } => limit }").contains("no `match` arm"));
}

#[test]
fn style_sheets_are_written_as_is() {
    let src = "let accent = \"#0b57d0\"\nlet css = `.card > h2 { color: ${accent}; }\na[href^='http'] { content: \"&\"; }`\n<style>{css}</style>";
    assert_eq!(ev(src), "<style>.card > h2 { color: #0b57d0; }\na[href^='http'] { content: \"&\"; }</style>");
    // The text cannot close the element and add markup.
    assert_eq!(ev("<style>{\"</style><script>x</script>\"}</style>"), "<style><\\/style><script>x<\\/script></style>");
    // Elsewhere, text is still escaped.
    assert_eq!(ev("<p>{\"a > b\"}</p>"), "<p>a &gt; b</p>");
}

#[test]
fn tagged_templates() {
    assert_eq!(ev("let accent = \"red\"\ncss`a > b { color: ${accent}; }`"), "\"a > b { color: red; }\"");
    assert_eq!(ev("sql`select 1`"), "\"select 1\"");
    // Any function can tag a template.
    assert_eq!(ev("fn shout(s) = upper(s)\nlet n = 2\nshout`${n} apples`"), "\"2 APPLES\"");
    // With a space it is a name followed by a string, as before.
    assert!(ev_err("let x = 1\nx `a`").contains("expected end of statement"));
}

#[test]
fn map_keys_may_be_keywords() {
    assert_eq!(ev("let m = { view: \"grid\", if: 1, for: 2 }\n[m.view, m[\"if\"], m]"), "[\"grid\", 1, { view: \"grid\", if: 1, for: 2 }]");
}

#[test]
fn a_lone_name_in_braces_after_an_arrow_is_a_block() {
    // As in JavaScript: `(a) => { a }` gives `a`, not `{ a: a }`.
    assert_eq!(ev("let h = (a) => { a }\nh(5)"), "5");
    assert_eq!(ev("let h = a => { a }\nh(6)"), "6");
    assert_eq!(ev("fn f(x) = { x }\nf(7)"), "7");
    assert_eq!(ev("match 8 { n => { n } }"), "8");
    // Two names, or a key with a value, are still maps.
    assert_eq!(ev("let p = (x, y) => { x, y }\np(1, 2)"), "{ x: 1, y: 2 }");
    assert_eq!(ev("let p = (x) => { x: x }\np(1)"), "{ x: 1 }");
    assert_eq!(ev("let x = 1\nlet m = { x }\nm"), "{ x: 1 }");
}

#[test]
fn a_line_starting_with_an_operator_continues_the_statement() {
    assert_eq!(ev("let a = 5\nlet b = 2\nlet total = a\n  - b\ntotal"), "3");
    assert_eq!(ev("let total = 5\n  + 2\n  * 3\ntotal"), "11");
    assert_eq!(ev("let same = 1\n  == 1\nsame"), "true");
    // `-b` without a space starts a new statement.
    assert_eq!(ev("fn f(x) {\n let y = x * 2\n -y\n}\nf(1)"), "-2");
}

#[test]
fn record_fields_with_a_union_type_bind_their_name() {
    let src = "fn page_of(q) = match q {\n { page: int | null } => [page]\n _ => \"no\"\n}\n[page_of({ page: 3 }), page_of({ page: null }), page_of({ page: \"x\" }), page_of({})]";
    assert_eq!(ev(src), "[[3], [null], \"no\", \"no\"]");
    assert_eq!(ev("let { n: int | str } = { n: \"a\" }\nn"), "\"a\"");
}

#[test]
fn unicode_escapes() {
    assert_eq!(ev(r#""café""#), "\"café\"");
    assert_eq!(ev(r#"'\u{1F600}'"#), "\"😀\"");
    assert_eq!(ev(r#"let n = 2
`\u{2192} ${n}`"#), "\"→ 2\"");
    assert_eq!(ev(r#"len("\u{41}B")"#), "2");
    assert!(ev_err(r#""\u12""#).contains("bad escape"));
    assert!(ev_err(r#""\u{}""#).contains("bad escape"));
    assert!(ev_err(r#""\u{1234567}""#).contains("bad escape"));
    assert!(ev_err(r#""\u{D800}""#).contains("not a Unicode character"));
}

#[test]
fn while_loops() {
    assert_eq!(ev("let i = 0\nlet s = 0\nwhile i < 5 {\n  s += i\n  i += 1\n}\ns"), "10");
    assert_eq!(ev("let i = 0\nwhile i < 3 { i += 1 }"), "null");
    assert_eq!(ev("fn find(xs, x) {\n  let i = 0\n  while i < len(xs) {\n    if xs[i] == x { return i }\n    i += 1\n  }\n  null\n}\n[find([4, 5, 6], 6), find([], 1)]"), "[2, null]");
    // Only null, false and errors end it, as with `if`.
    assert_eq!(ev("let xs = [1, 2, 3]\nlet n = 0\nwhile first(xs) {\n  xs = drop(xs, 1)\n  n += 1\n}\nn"), "3");
    assert!(ev_err("while true {").contains("end of input"));
    assert!(ev_err("let while = 1").contains("keyword"));
    assert!(ev_err("let i = 0\nwhile i < 2 i += 1").contains("expected `{` after the `while` condition"));
}

#[test]
fn while_loops_in_scripts_count_their_steps() {
    let r = ev(r#"run_script("let i = 0\nwhile true { i += 1 }\ni")"#);
    assert!(r.contains("err(") && r.contains("steps"), "{}", r);
    assert_eq!(ev(r#"run_script("let i = 0\nwhile i < 10 { i += 1 }\ni")"#), "10");
}

#[test]
fn let_and_for_bind_names_that_are_types() {
    assert_eq!(ev("let x = 1\nlet int = x + 1\nlet num = 5\nlet list = [num]\nlet float = 2.5\n[int, x, num + 1, list, float * 2]"), "[2, 1, 6, [5], 5.0]");
    assert_eq!(ev("let out = []\nfor str in [\"a\", \"b\"] { out = push(out, str) }\nout"), "[\"a\", \"b\"]");
    assert_eq!(ev("for num in [1, 2] => num * 10"), "[10, 20]");
    // In `match`, a type's name still tests the type.
    assert_eq!(ev("[match 3 { int => \"an int\", _ => \"no\" }, match \"s\" { int => 1, str => 2 }]"), "[\"an int\", 2]");
}

#[test]
fn spread_in_maps_and_lists() {
    assert_eq!(ev("let m = { a: 1, b: 2 }\n[{ ...m, b: 3 }, { b: 0, ...m }, { ...m }, { ...m, ...{ c: 3 } }, { ...null, a: 1 }]"), "[{ a: 1, b: 3 }, { b: 2, a: 1 }, { a: 1, b: 2 }, { a: 1, b: 2, c: 3 }, { a: 1 }]");
    assert_eq!(ev("let xs = [1, 2]\n[[...xs, 3], [0, ...xs, ...xs], [...xs], [...[]], 0..3]"), "[[1, 2, 3], [0, 1, 2, 1, 2], [1, 2], [], [0, 1, 2]]");
    // A variable named like the builtin spread uses changes nothing.
    assert_eq!(ev("let merge = 5\nlet m = { a: 1 }\n{ ...m, b: merge }"), "{ a: 1, b: 5 }");
    // The error names the spread, not the `+` it was made of (Q-036;
    // branch feedback6-fixes).
    assert!(ev_err("[...1]").contains("`[...x]` spreads a list (or null) into a list, but x is int"));
}

#[test]
fn return_is_an_expression() {
    let defs = "fn f(x) {\n let y = x ?? return \"none\"\n y * 2\n}\nfn g(k) = match k {\n \"a\" => return 1\n _ => 2\n}\nfn h(x) {\n let v = x ?? return\n v\n}\n";
    assert_eq!(ev(&format!("{}[f(2), f(null), g(\"a\"), g(\"b\"), h(null), h(3)]", defs)), "[4, \"none\", 1, 2, null, 3]");
    // From a list made with `for … =>`, it leaves the function.
    assert_eq!(ev("fn f() {\n let xs = for x in [1, 2, 3] => { if x == 2 { return \"left f\" }; x * 10 }\n xs\n}\nf()"), "\"left f\"");
}

#[test]
fn maps_are_indexed_by_their_keys_text_and_lists_take_set() {
    assert_eq!(ev("let m = set({}, 5, \"five\")\nlet g = group_by([{ a: 1 }], (x) => x.a)\n[m[5], m[\"5\"], m[6], get(m, 5), len(g[1])]"), "[\"five\", \"five\", null, \"five\", 1]");
    assert_eq!(ev("let xs = [1, 2, 3]\n[set(xs, 1, 9), set(xs, -1, 0), xs]"), "[[1, 9, 3], [1, 2, 0], [1, 2, 3]]");
    assert!(ev_err("set([1, 2, 3], 3, 0)").contains("index 3 is outside a list of 3"));
    assert!(ev_err("set([1], \"a\", 0)").contains("int"));
}

/// `m = set(m, k, v)` and `reduce(..., (m, x) => set(m, ...))` change the
/// map in place; what still holds it keeps its own.
#[test]
fn maps_built_in_a_loop_are_not_copied_each_time() {
    let start = std::time::Instant::now();
    assert_eq!(ev("fn build(n) {\n let m = {}\n for i in range(n) { m = set(m, str(i), i) }\n m\n}\nlen(build(50000))"), "50000");
    assert_eq!(ev("len(reduce(range(50000), {}, (m, i) => set(m, str(i), true)))"), "50000");
    assert_eq!(ev("fn grow(n) {\n let xs = []\n for i in range(n) { xs = push(xs, i) }\n xs\n}\nlen(grow(50000))"), "50000");
    // Quadratic, these would take tens of seconds.
    assert!(start.elapsed() < std::time::Duration::from_secs(5), "{:?}", start.elapsed());
    // Values stay values: the map another variable holds is not changed.
    assert_eq!(ev("let a = { x: 1 }\nfn f() {\n let m = a\n let b = m\n m = set(m, \"y\", 2)\n [a, b, m]\n}\nf()"), "[{ x: 1 }, { x: 1 }, { x: 1, y: 2 }]");
    assert_eq!(ev("fn f() {\n let m = {}\n for i in range(3) { m = set(m, str(i), len(m)) }\n m\n}\nf()"), "{ \"0\": 0, \"1\": 1, \"2\": 2 }");
    assert_eq!(ev("let outer = { n: 0 }\nlet adds = [1, 2] | map((k) => set(outer, str(k), k))\n[outer, adds]"), "[{ n: 0 }, [{ n: 0, \"1\": 1 }, { n: 0, \"2\": 2 }]]");
}

/// Comments: `/* ... */` anywhere in code, and in markup a line starting
/// with `//` or a `{/* ... */}`. Text with `//` in it otherwise stays text.
#[test]
fn comments_in_code_and_markup() {
    assert_eq!(ev("1 /* one */ + /* two */ 2"), "3");
    // A comment over several lines still ends the statement before it.
    assert_eq!(ev("let a = 1 /* a\nnote */\nlet b = 2\na + b"), "3");
    assert_eq!(
        ev("<div>\n    // a note, naming <Card> and {x}\n    <p>x</p>\n    {/* with } and <b> inside */}\n    {/*\n      over lines\n    */}\n</div>"),
        "<div><p>x</p></div>"
    );
    assert_eq!(ev(r#"<p>see https://example.com // mid-line, so text</p>"#), "<p>see https://example.com // mid-line, so text</p>");
    assert_eq!(ev(r#"<p>{"//"} on purpose</p>"#), "<p>// on purpose</p>");
    assert_eq!(ev("<p>{1 /* inside */ + 1}</p>"), "<p>2</p>");
}

/// A textarea's text between its tags is its value, so it follows state.
#[test]
fn textarea_children_are_its_value() {
    assert_eq!(ev(r#"let t = "a < b"
<textarea>{t}</textarea>"#), "<textarea>a &lt; b</textarea>");
    assert_eq!(ev(r#"let n = 2
<textarea>Item {n}</textarea>"#), "<textarea>Item 2</textarea>");
    assert!(ev_err(r#"<textarea value={"a"}>{"b"}</textarea>"#).contains("not both"));
}

/// Private names run as written: each file's own, locals of the same name included
/// (branch public-private).
#[test]
fn private_names_run_in_their_file() {
    let d = std::env::temp_dir().join(format!("quillo-private-run-{}", std::process::id()));
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::write(d.join("lib/a.qwe"), "private let PREFIX = \"a\"\nprivate fn helper(x: str) = `${PREFIX}:${x}`\nfn from_a(x: str) = helper(x)\nfn shadow(helper: str) = helper\n").unwrap();
    std::fs::write(d.join("lib/b.qwe"), "private fn helper(x: str) = `b:${x}`\nfn from_b(x: str) = helper(x)\n").unwrap();
    std::fs::write(d.join("main.qwe"), "use \"lib/a.qwe\"\nuse \"lib/b.qwe\"\nlet out = [from_a(\"x\"), from_b(\"y\"), shadow(\"mine\")]\n").unwrap();
    let mut it = quillo::interp::Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    let out = it.load_source("out", "probe.qwe").unwrap();
    assert_eq!(out.repr(), "[\"a:x\", \"b:y\", \"mine\"]");
    let _ = std::fs::remove_dir_all(&d);
}
