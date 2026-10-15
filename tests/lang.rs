use wisp::eval_str;

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
fn arithmetic_and_precedence() {
    assert_eq!(ev("1 + 2 * 3"), "7");
    assert_eq!(ev("(1 + 2) * 3"), "9");
    assert_eq!(ev("7 / 2"), "3");
    assert_eq!(ev("7.0 / 2"), "3.5");
    assert_eq!(ev("-3 + 1"), "-2");
    assert_eq!(ev("10 % 3"), "1");
    assert_eq!(ev("1 < 2 and not 3 > 4"), "true");
    assert_eq!(ev("0..4"), "[0, 1, 2, 3]");
    assert!(ev_err("1 / 0").contains("division by zero"));
}

#[test]
fn strings_and_interpolation() {
    assert_eq!(ev("let name = \"Ada\"\n\"Hi {name}, {1 + 1}\""), "\"Hi Ada, 2\"");
    assert_eq!(ev(r#""a" + 1"#), "\"a1\"");
    assert_eq!(ev(r#""literal \{braces\}""#), "\"literal {braces}\"");
    assert_eq!(ev(r#""{ "nested {1}" }""#), "\"nested 1\"");
}

#[test]
fn lists_and_maps() {
    assert_eq!(ev("[1, 2, 3][-1]"), "3");
    assert_eq!(ev("let id = 7\n{ id, name: \"x\" }"), "{ id: 7, name: \"x\" }");
    assert_eq!(ev("{ a: 1 } + { b: 2 }"), "{ a: 1, b: 2 }");
    assert_eq!(ev("{ a: { b: 5 } }.a.b"), "5");
    assert_eq!(ev("{ a: 1 }.missing"), "nil");
    assert_eq!(ev("nil.anything.at.all"), "nil");
    assert_eq!(ev("{ \"content-type\": \"x\" }[\"content-type\"]"), "\"x\"");
}

#[test]
fn functions_defaults_and_types() {
    assert_eq!(ev("fn add(a: int, b: int = 10) -> int = a + b\nadd(1)"), "11");
    assert_eq!(ev("fn add(a, b) { let s = a + b\n s * 2 }\nadd(1, 2)"), "6");
    assert_eq!(ev("let double = fn(x) = x * 2\ndouble(4)"), "8");
    assert!(ev_err("fn f(a: int) = a\nf(\"x\")").contains("argument `a` of `f` must be int"));
    assert!(ev_err("fn f() -> int = \"no\"\nf()").contains("must return int"));
    assert!(ev_err("fn f(a) = a\nf()").contains("missing argument `a`"));
    assert_eq!(ev("fn f(a: str?) = a ?? \"none\"\nf(nil)"), "\"none\"");
}

#[test]
fn closures_and_recursion() {
    assert_eq!(ev("fn fib(n) = if n < 2 then n else fib(n - 1) + fib(n - 2)\nfib(20)"), "6765");
    assert_eq!(ev("fn adder(n) = fn(x) = x + n\nlet add5 = adder(5)\nadd5(1)"), "6");
}

#[test]
fn pipelines() {
    assert_eq!(ev("[1, 2, 3] |> map(fn(x) = x * 2) |> sum"), "12");
    assert_eq!(ev(r#""a,b,c" |> split(",") |> join("-")"#), "\"a-b-c\"");
    assert_eq!(
        ev("[5, 3, 8]\n    |> filter(fn(x) = x > 4)\n    |> sort\n    |> first"),
        "5"
    );
}

#[test]
fn method_call_syntax() {
    assert_eq!(ev("[3, 1, 2].sort().first()"), "1");
    assert_eq!(ev("\"hey\".upper"), "\"HEY\"");
    assert_eq!(ev("[1, 2, 3].len"), "3");
    assert_eq!(ev("let m = { greet: fn(n) = \"hi {n}\" }\nm.greet(\"bo\")"), "\"hi bo\"");
    assert_eq!(ev("{ a: 1, b: 2 }.keys"), "[\"a\", \"b\"]");
}

#[test]
fn match_patterns() {
    let src = r#"
fn describe(x) = match x {
    0 => "zero"
    int as n if n < 0 => "negative {n}"
    int => "positive"
    "a" | "b" => "letter"
    [first, ..rest] => "list starting {first}, {len(rest)} more"
    { type: "user", name } => "user {name}"
    { customer.id: id } => "customer {id}"
    _ => "other"
}
[describe(0), describe(-2), describe(9), describe("b"), describe([1, 2, 3]),
 describe({ type: "user", name: "Ada" }), describe({ customer: { id: 4 } }), describe(nil)]
"#;
    assert_eq!(
        ev(src),
        r#"["zero", "negative -2", "positive", "letter", "list starting 1, 2 more", "user Ada", "customer 4", "other"]"#
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
    assert_eq!(ev("nil ?? 5"), "5");
    assert_eq!(ev(r#"int("x") ?? 0"#), "0");
    assert_eq!(ev(r#"int("42")"#), "42");
    let src = r#"
fn parse_age(s: str) -> int ! str =
    match int(s) {
        int as n if n >= 0 => n
        _ => err("bad age: {s}")
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
    assert_eq!(ev("let x = if false then 1\nx"), "nil");
    assert_eq!(ev("for x in [1, 2, 3] => x * x"), "[1, 4, 9]");
    assert_eq!(ev("for [k, v] in { a: 1, b: 2 } => \"{k}={v}\""), "[\"a=1\", \"b=2\"]");
    assert_eq!(ev("fn f(x) { if x > 0 { return \"pos\" }\n \"other\" }\n[f(1), f(0)]"), "[\"pos\", \"other\"]");
}

#[test]
fn assignment() {
    assert_eq!(ev("let n = 1\nn += 2\nn"), "3");
    assert_eq!(ev("let xs = []\nfn add(x) { xs = xs |> push(x) }\nadd(1)\nadd(2)\nxs"), "[1, 2]");
    assert!(ev_err("y = 1").contains("never declared"));
}

#[test]
fn html_literals() {
    assert_eq!(ev(r#"<p.note #n-{1 + 1}>"a < b"</p>"#), r#"<p id="n-2" class="note">a &lt; b</p>"#);
    assert_eq!(
        ev(r#"let x = "<script>"
<div title={x} hidden data-n="{1}">{x}<br/><img src="a.png"></div>"#),
        r#"<div title="&lt;script&gt;" hidden data-n="1">&lt;script&gt;<br><img src="a.png"></div>"#
    );
    assert_eq!(ev(r#"<ul>{for i in [1, 2] => <li>{i}</li>}</ul>"#), "<ul><li>1</li><li>2</li></ul>");
    assert_eq!(ev(r#"let ok = false
<b disabled={ok} class={if ok then "on"}>{if ok then "yes"}</b>"#), "<b></b>");
    assert_eq!(ev(r#"<div>{raw("<em>trusted</em>")}</div>"#), "<div><em>trusted</em></div>");
    assert_eq!(ev(r#"<html><body>"x"</body></html>"#), "<!doctype html><html><body>x</body></html>");
}

#[test]
fn html_multiline_and_nested_conditionals() {
    let src = r##"
let todo = { id: 3, text: "ship", done: true }
<li.todo #todo-{todo.id}>
    {if todo.done then <s>{todo.text}</s> else todo.text}
    // a comment between children
    <span.meta>"#{todo.id}"</span>
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
    <section.card class={if highlight then "hl"}>
        <h2>{title}</h2>
        {children}
    </section>
<main>
    <Card title="One">"first"</Card>
    <Card title="Two" highlight><p>"second"</p></Card>
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
    let e = ev_err("let a = 1\n<div>\"x\"</span>");
    assert!(e.contains("<input>:2:") && e.contains("expected </div>"), "{}", e);
    let e = ev_err("fn f(x: number) = x");
    assert!(e.contains("unknown type `number`"), "{}", e);
}

#[test]
fn json_module() {
    assert_eq!(ev(r#"json.parse("\{\"a\": [1, 2.5, null]\}")"#), "{ a: [1, 2.5, nil] }");
    assert_eq!(ev(r#"json.stringify({ a: 1, b: [true] })"#), r#""{\"a\":1,\"b\":[true]}""#);
    assert_eq!(ev(r#"is_err(json.parse("\{"))"#), "true");
}
