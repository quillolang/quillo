//! What a programmer sees when a program does not parse: the message, and
//! the line it names. Then the words a program may still use as its own
//! names, which the parser tells from the statements they can start.

use quillo::{eval_str, parser};

/// The error `src` gives: its line and message.
fn fails(src: &str) -> (u32, String) {
    match eval_str(src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => (e.line, e.msg),
    }
}

fn gives(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

/// Each (source, line, message) refused with just that line and message.
fn refused(cases: &[(&str, u32, &str)]) {
    for (src, line, msg) in cases {
        assert_eq!(fails(src), (*line, msg.to_string()), "{}", src);
    }
}

#[test]
fn tokens_are_named_as_they_were_written() {
    refused(&[
        ("1 2", 1, "expected end of statement, found `2`"),
        ("1 2.5", 1, "expected end of statement, found `2.5`"),
        ("1 3m", 1, "expected end of statement, found `3m`"),
        ("1 \"s\"", 1, "expected end of statement, found a string"),
        ("let x\n1", 1, "expected `=`, found end of line"),
        ("let x", 1, "expected `=`, found end of input"),
        ("\n\nlet x = )", 3, "expected an expression, found `)`"),
        ("let x = }", 1, "expected an expression, found `}`"),
        ("let x = ]", 1, "expected an expression, found `]`"),
        ("let x = :", 1, "expected an expression, found `:`"),
        ("let x = ;", 1, "expected an expression, found `;`"),
        ("let x = .", 1, "expected an expression, found `.`"),
        ("let x = ..", 1, "expected an expression, found `..`"),
        ("let x = +", 1, "expected an expression, found `+`"),
        ("fn f {", 1, "expected `(`, found `{`"),
        ("fn f -", 1, "expected `(`, found `-`"),
        ("fn f <", 1, "expected `(`, found `<`"),
        ("fn f !", 1, "expected `(`, found `!`"),
        ("let x = *", 1, "expected an expression, found `*`"),
        ("let x = %", 1, "expected an expression, found `%`"),
        ("let x = -=", 1, "expected an expression, found `-=`"),
        ("let x = ==", 1, "expected an expression, found `==`"),
        ("let x = !=", 1, "expected an expression, found `!=`"),
        ("let x = <=", 1, "expected an expression, found `<=`"),
        ("let x = >", 1, "expected an expression, found `>`"),
        ("let x = >=", 1, "expected an expression, found `>=`"),
        ("let x = |", 1, "expected an expression, found `|`"),
        ("let x = ||", 1, "expected an expression, found `||`"),
        ("let x = &&", 1, "expected an expression, found `&&`"),
        ("let x = |>", 1, "expected an expression, found `|>`"),
        ("let x = ?", 1, "expected an expression, found `?`"),
        ("let x = ??", 1, "expected an expression, found `??`"),
        ("let x = =>", 1, "expected an expression, found `=>`"),
        ("let x = ->", 1, "expected an expression, found `->`"),
        ("let x = ~", 1, "expected an expression, found `~`"),
        ("let x = < 1", 1, "expected an expression, found `<`"),
        ("let x = <1", 1, "expected an expression, found `<`"),
    ]);
}

#[test]
fn templates_say_what_is_wrong_inside() {
    refused(&[
        ("\n`${}`", 2, "empty `${}` in template"),
        ("`${ 1 `", 1, "unexpected end of input: `${` in a template is never closed"),
        ("`${ \"x\\", 1, "unexpected end of input: `${` in a template is never closed"),
        ("`${1 2}`", 1, "unexpected text inside `{…}` in string"),
    ]);
    assert_eq!(gives("`${ \"a\\\"b\" }`"), "\"a\\\"b\"");
    assert_eq!(gives("len(\"a\\0b\")"), "3");
    assert_eq!(gives("`a${ '}' }b`"), "\"a}b\"");
}

#[test]
fn numbers_that_only_look_like_more() {
    refused(&[("let n = 1e\nn", 1, "expected end of statement, found `e`"), ("1e+", 1, "expected end of statement, found `e`")]);
    assert_eq!(gives("[1e3, 2.5e-1, 6E+2]"), "[1000.0, 0.25, 600.0]");
}

#[test]
fn a_definition_in_the_wrong_place() {
    refused(&[
        ("fn f() {\n  private fn g() = 1\n}", 2, "`private` is written at the top level of a file, before what it names"),
        ("private server fn f() = 1", 1, "a `server fn` cannot be private: it is an address on the server that anyone can call; keep the work in a private `fn` it calls"),
        ("private type Foo = int", 1, "`private type` is not there yet: a type's name is the program's"),
        ("private x", 1, "expected end of statement, found `x`"),
        ("style \"a\"", 1, "`style` belongs in the body of a component or a view"),
        ("view P() {\n  <div/>\n  style \"a\"\n}", 3, "`style` comes before the markup of `P`"),
        ("view P() {\n  fn q() { style \"a\" }\n  <div/>\n}", 2, "`style` belongs in the body of a component or a view"),
        ("view P(a) {\n  style a\n  <div/>\n}", 2, "a style is the same for every `P`, so it cannot read its parameter `a`; set a class or `style=` on an element instead"),
        ("view P(a) {\n  style css`.x { color: ${a}; }`\n  <div/>\n}", 2, "a style is the same for every `P`, so it cannot read its parameter `a`; set a class or `style=` on an element instead"),
        ("fn f(1) = 1", 1, "expected a parameter name, found `1`"),
        ("fn\n", 1, "anonymous functions are written `(x) => x * 2` (or `x => x * 2`); `fn` is for named functions"),
        ("1 = 2", 1, "only variables, and the fields of the page's objects (`el.value = x`), can be assigned to"),
    ]);
}

#[test]
fn components_and_their_state() {
    refused(&[
        ("component foo() = 1", 1, "component names start with a capital letter: `foo`"),
        ("component A() {", 1, "unexpected end of input: missing `}`"),
        ("component A()\n", 2, "expected `=` or `{` to start the body of component `A`"),
        ("component A() {\n  state x\n  <div/>\n}", 2, "expected end of statement, found `x`"),
        ("component A() {\n  state reply = 1\n  <div/>\n}", 2, "expected a state name, found `reply`, which is a keyword: pick another name"),
    ]);
}

#[test]
fn types_say_what_they_cannot_hold() {
    refused(&[
        ("type Foo", 1, "expected end of statement, found `Foo`"),
        ("type USER = { a: int }", 1, "a type is named like `User` or `OrderLine`, with small letters: `USER` reads as a constant"),
        ("fn f(a: 1) = 1", 1, "expected a type, found `1`"),
        ("fn f(a: `x${1}`) = 1", 1, "a string in a type cannot contain `${…}`"),
        ("type Abc = { `x${1}`: int }", 1, "map keys must be plain strings"),
        ("type Abc = {\n  1: int\n}", 2, "expected a field name, found `1`"),
        ("type Abc = { a: int b: str }", 1, "expected `,` or `}` in a map type, found `b`"),
    ]);
    assert_eq!(gives("fn f(a: null) = a\nf(null)"), "null");
    assert_eq!(gives("type Abc = { \"x y\": int }\nfn f(a: Abc) = a[\"x y\"]\nf({ \"x y\": 1 })"), "1");
}

#[test]
fn routes_and_their_paths() {
    refused(&[
        ("route FOO /x = 1", 1, "unknown HTTP method `FOO` (use one of GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS, ANY)"),
        ("route GET x = 1", 1, "expected a path starting with `/` after the method"),
        ("route GET /: = 1", 1, "expected a parameter name after `:` in the path"),
        ("route GET /:id(foo) = 1", 1, "path parameter types are written :name(int), :name(float), :name(dec), :name(num), :name(str) or :name(date)"),
        ("route GET /:id(int = 1", 1, "path parameter types are written :name(int), :name(float), :name(dec), :name(num), :name(str) or :name(date)"),
        ("route GET /x? 1", 1, "expected `{` with query parameters after `?`"),
        ("route GET /x 1", 1, "expected `=` or `{` to start the route body"),
        ("routes /x 1", 1, "expected `{` and the group's routes"),
        ("routes /x | a b", 1, "expected `{` and the group's routes"),
    ]);
}

#[test]
fn service_and_mount_expect_a_file() {
    refused(&[
        ("service /// = \"a.qwe\"", 1, "`service` needs a name or a path: service billing = \"billing.qwe\""),
        ("service /a = \"1-bad.qwe\"", 1, "a service is named like a variable: `1-bad` is not one (name the file so)"),
        ("service foo = \"a.qwe\" { let x = 1\nx }", 1, "after a service's file, a map of how it is called: { timeout: \"2s\", retries: 2 }"),
        ("service foo = 1", 1, "`service` expects a file: service billing = \"billing.qwe\""),
        ("service foo = `a${1}.qwe`", 1, "`service` expects a file: service billing = \"billing.qwe\""),
        ("service /a/ = 1", 1, "`service` expects a file: service billing = \"billing.qwe\""),
        ("service if = 3", 1, "expected end of statement, found `if`"),
        ("mount / = \"a.qwe\"", 1, "`mount` needs a path such as /notes"),
        ("mount /a = 1", 1, "`mount` expects a file: mount /notes = \"notes.qwe\""),
        ("mount /a = `${x}.qwe`", 1, "`mount` expects a file: mount /notes = \"notes.qwe\""),
    ]);
}

#[test]
fn use_says_what_it_takes() {
    refused(&[
        ("use `a.qwe`", 1, "`use` expects a file: use lib/auth.qwe"),
        ("use a from b/", 1, "`use` expects a file: use lib/auth.qwe"),
        ("use a, a from b.qwer", 1, "`a` is taken twice in this `use`"),
        ("use a as b, c as b from b.qwer", 1, "`b` is taken twice in this `use`"),
        ("use __x from \"https://x.example/z.js\"", 1, "`__x`: a name from a script cannot start with `__`"),
        ("use __x from \"lib.js\"", 1, "`__x`: a name from a script cannot start with `__`"),
    ]);
}

#[test]
fn maps_and_patterns_say_what_they_found() {
    refused(&[
        ("let a = 1\n{ ...a b }", 2, "expected `,` or `}` in map, found `b`"),
        ("{ `a${1}`: 1 }", 1, "map keys must be plain strings"),
        ("{ a: 1, 2: 3 }", 1, "expected a map key, found `2`"),
        ("{ a: 1, \"b\" }", 1, "expected `:` after map key \"b\""),
        ("{ a: 1 b: 2 }", 1, "expected `,` or `}` in map, found `b`"),
        ("if true 1", 1, "expected `{` after the `if` condition (or write `cond ? a : b`)"),
        ("for x 1", 1, "expected `in` after the `for` pattern"),
        ("for x in [1] 1", 1, "expected `=>` or `{` after the `for` list"),
        ("let { 1: x } = {}", 1, "expected a field name, found `1`"),
        ("match 1 { -a => 2 }", 1, "expected a number after `-`, found `a`"),
        ("match \"a\" { `x${1}` => 1 }", 1, "string patterns cannot contain `{…}`"),
        ("[..5]", 1, "expected an expression, found `..`"),
        ("{ ..5 }", 1, "expected an expression, found `..`"),
    ]);
}

#[test]
fn markup_says_where_it_went_wrong() {
    refused(&[
        ("<div / >", 1, "expected `>` after `/` in <div>"),
        ("<div a=1/>", 1, "attribute `a` needs a \"string\" or {expression} value"),
        ("<div", 1, "unexpected end of input: unclosed <div>"),
        ("<div\n  a=\"b\"\n", 3, "unexpected end of input: unclosed <div>"),
        ("<div !>", 1, "unexpected `!` in <div> tag"),
        ("<div></div x>", 1, "expected `>` to close </div"),
        ("<div>{ /* c </div>", 1, "expected an expression, found end of input"),
    ]);
}

#[test]
fn an_element_left_open_is_told_at_its_start() {
    refused(&[
        ("<div><p></div>", 1, "`<p>` is never closed: add `</p>` before `</div>` on line 1"),
        ("<div>\n  <p>\n</div>", 2, "`<p>` is never closed: add `</p>` before `</div>` on line 3"),
        ("fn a() {\n  <div>\n  <b> text\n}", 3, "`<b>` is never closed: add `</b>` before the `}` on line 4 (write {\"}\"} for the character)"),
        ("view P() {\n  <div>\nservice billing = \"x\"", 2, "`<div>` is never closed: add `</div>` before `service` on line 3"),
        ("view P() {\n  <div>\n    hello world\n\n\nservice billing = \"x\"", 2, "`<div>` is never closed: add `</div>` before `service` on line 6"),
        ("view P() {\n  <div>\nhello world", 2, "`<div>` is never closed: add `</div>` at the end of the file"),
    ]);
}

#[test]
fn the_fix_for_an_open_element_goes_where_its_end_tag_should() {
    let fix = |src: &str| {
        let Err(e) = eval_str(src) else { panic!("expected an error: {}", src) };
        let f = e.fix.expect("a fix");
        (f.line, f.col, f.text)
    };
    // In the middle of a line.
    assert_eq!(fix("<div><p>x</div>"), (1, 9, "</p>".to_string()));
    // On a line of its own.
    assert_eq!(fix("<div>\n  <p>\n</div>"), (3, 0, "  </p>\n".to_string()));
    // At the end of the file.
    assert_eq!(fix("view P() {\n  <div>"), (2, 7, "\n  </div>".to_string()));
}

#[test]
fn only_text_ending_inside_markup_has_open_tags() {
    assert!(parser::open_tags_at_end("let x = 1\n").is_none());
    assert!(parser::open_tags_at_end("let x = \n").is_none());
    let open: Vec<String> = parser::open_tags_at_end("<div>\n  <p>").unwrap().into_iter().map(|o| o.tag).collect();
    assert_eq!(open, ["div", "p"]);
}

#[test]
fn keywords_of_statements_are_names_elsewhere() {
    assert_eq!(gives("let private = 1\nprivate + 1"), "2");
    assert_eq!(gives("let public = 2\npublic\npublic + 1"), "3");
    assert_eq!(gives("let style = 3\nstyle\nstyle + 1"), "4");
    assert_eq!(gives("let style = 3\nstyle = 4\nstyle"), "4");
    assert_eq!(gives("let server = 1\nserver\nserver + 1"), "2");
    assert_eq!(gives("let type = 1\ntype\ntype = 3\ntype"), "3");
    assert_eq!(gives("let service = 4\nservice\nservice = 5\nservice"), "5");
    assert_eq!(gives("let mount = 1\nmount\nmount + 1"), "2");
    assert_eq!(gives("let routes = 4\n[routes + 1, routes / 2]"), "[5, 2]");
    assert_eq!(gives("let routes = 4\nroutes\nroutes"), "4");
    assert_eq!(gives("let state = { x: 1 }\nstate.x"), "1");
}

#[test]
fn state_is_a_name_in_a_component_too() {
    let src = "component A(state) {\n  state.x\n  state\n  <i>{state.x}</i>\n}\n<A state={{ x: 2 }}/>";
    assert!(gives(src).ends_with("><i>2</i></quillo-island>"));
}

#[test]
fn private_names_of_every_pattern_are_kept_from_other_files() {
    assert_eq!(gives("private let { x: { y }, z } = { x: { y: 1 }, z: 2 }\ny + z"), "3");
    assert_eq!(gives("private let [a, ..b] = [1, 2, 3]\n[a, b]"), "[1, [2, 3]]");
    assert_eq!(gives("private let [a, ..] = [1, 2]\na"), "1");
    assert_eq!(gives("private let [a] as whole = [1]\n[a, whole]"), "[1, [1]]");
    assert_eq!(gives("private let [c] | [c, _] = [1, 2]\nc"), "1");
    assert_eq!(gives("private let 1 = 1\n2"), "2");
}

#[test]
fn field_writes_are_for_the_page_s_objects() {
    let set = "let a = { x: 1 }\nfn f() {\n  a.x = 2\n}\nf()";
    let add = "let a = { x: 1 }\nfn f() {\n  a.x += 2\n}\nf()";
    for src in [set, add] {
        assert_eq!(fails(src), (3, "a map's fields are not set in place: write `m = set(m, \"x\", …)`".to_string()));
    }
}

#[test]
fn an_else_on_its_own_line_goes_on_with_the_if() {
    assert_eq!(gives("let a = if false {\n  1\n}\nelse {\n  2\n}\na"), "2");
}

#[test]
fn small_things_that_parse() {
    assert_eq!(gives("let a = 1;; let b = 2;;\na + b"), "3");
    assert_eq!(gives("(true)"), "true");
    assert_eq!(gives("let a = 2\n(a) * 2"), "4");
    assert_eq!(gives("{ \"0\": \"zero\" }.0"), "\"zero\"");
    assert_eq!(gives("{ 1 }"), "1");
    assert_eq!(gives("match 1.5 { 1.5 => \"x\", _ => \"y\" }"), "\"x\"");
    assert_eq!(gives("match -1 { -1 => \"a\", -1.5 => \"b\", _ => \"c\" }"), "\"a\"");
    assert_eq!(gives("match -1.5 { -1 => \"a\", -1.5 => \"b\", _ => \"c\" }"), "\"b\"");
    assert_eq!(gives("<div#a/>"), "<div id=\"a\"></div>");
    assert_eq!(gives("<div #b .a/>"), "<div id=\"b\" class=\"a\"></div>");
}

#[test]
fn a_style_can_be_a_constant_of_the_file() {
    let scoped = gives("let global = \"i { color: red }\"\nview P() {\n  style global\n  <i/>\n}\nP()");
    assert!(scoped.starts_with("<i data-q-"), "{}", scoped);
    let global = gives("view P() {\n  style global \"i { color: red }\"\n  <i/>\n}\nP()");
    assert!(global.starts_with("<i data-q-"), "{}", global);
}

#[test]
fn braces_in_markup_hold_comments_and_expressions() {
    assert_eq!(gives("<div>{ /* c */ }</div>"), "<div></div>");
    assert_eq!(gives("<div>{\n/* c */\n}</div>"), "<div></div>");
    assert_eq!(gives("<div>{/* c */ 1}</div>"), "<div>1</div>");
}

#[test]
fn choices_of_markup_are_kept_as_branches() {
    assert_eq!(gives("let q = true\n<ul>{q && (q && <li/>)}</ul>"), "<ul><li></li></ul>");
    assert_eq!(gives("let q = true\n<ul>{q && (q ? <li/> : null)}</ul>"), "<ul><li></li></ul>");
    assert_eq!(gives("let q = false\n<ul>{q && (q ? null : <li/>)}</ul>"), "<ul></ul>");
    assert_eq!(gives("let q = true\n<ul>{q && (q ? null : null)}</ul>"), "<ul></ul>");
    assert_eq!(gives("let q = true\n<ul>{if q { let _a = 1 }}</ul>"), "<ul></ul>");
    assert_eq!(gives("let q = true\n<ul>{if q { }}</ul>"), "<ul></ul>");
}
