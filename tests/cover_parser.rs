//! The parser's rarer refusals, each with its exact words and line: `use` and `import` written
//! another way, callers, rest parameters, swapped field types, half a `?:`, handlers that
//! assign, and files named by templates.

use quillo::check::{check_source, Severity};
use quillo::Interp;
use std::path::Path;

/// The first error the checker gives, with its line.
fn error(src: &str) -> String {
    let r = check_source(src, "t.qwe", Path::new("."));
    let d = r.diagnostics.iter().find(|d| d.severity == Severity::Error).unwrap_or_else(|| panic!("no error\n--- source ---\n{}", src));
    format!("{} {}", d.line, d.message)
}

fn errs(src: &str, want: &str) {
    assert_eq!(error(src), want, "\n--- source ---\n{}", src);
}

/// What loading the program says, as `quillo run` would.
fn load_error(src: &str) -> String {
    let mut it = Interp::new(".");
    match it.load_source(src, "case.qwe") {
        Ok(_) => panic!("loaded\n--- source ---\n{}", src),
        Err(e) => format!("{} {}", e.line, e.msg),
    }
}

#[test]
fn a_quoted_use_followed_by_use_says_to_import() {
    errs(
        "use \"lib/a.qwe\" use price\n",
        "1 to take names from a file, `import \"lib/a.qwe\" use ...`; `use \"lib/a.qwe\"` alone takes all its public names",
    );
}

#[test]
fn a_use_with_nothing_after_it_or_a_template_expects_a_file() {
    errs("use\n", "1 `use` expects a file: use lib/auth.qwe");
    errs("use\n\"x.qwe\"\n", "1 `use` expects a file: use lib/auth.qwe");
    errs("let n = 1\nuse `lib/${n}.qwe`\n", "2 `use` expects a file: use lib/auth.qwe");
}

#[test]
fn a_service_or_mount_named_by_a_template_expects_a_file() {
    errs("let n = 1\nservice billing = `b${n}.qwe`\n", "2 `service` expects a file: service billing = \"billing.qwe\"");
    errs("let n = 1\nmount /notes = `n${n}.qwe`\n", "2 `mount` expects a file: mount /notes = \"notes.qwe\"");
}

#[test]
fn build_without_its_value_is_a_name_and_a_string() {
    errs("build \"/x.css\"\n", "1 expected end of statement, found a string");
    errs("let n = 1\nbuild `/x${n}.css` = \"a\"\n", "2 expected end of statement, found a string");
}

#[test]
fn several_names_are_not_assigned_at_once() {
    errs("let a = 1\nlet b = 2\n[a, b] = [b, a]\n", "3 several names are not assigned at once: `let [c, d] = [b, a + b]`, then assign each");
}

#[test]
fn a_caller_is_named_in_plain_text() {
    errs("server fn refund() for [\"\"] = 1\n", "1 a caller is named in plain text: for [\"shop\"]");
    errs("server fn refund() for [5] = 1\n", "1 a caller is named in plain text, like \"shop\", found `5`");
}

#[test]
fn a_component_takes_no_rest_parameter() {
    errs("component C(..rest) {\n  <p/>\n}\n", "1 a function takes no `...rest`: take a list");
}

#[test]
fn a_field_s_type_that_does_not_parse_is_said_as_it_is() {
    errs("type User = { name: (int }\n", "1 expected a type, found `(`");
    errs("type User = { str: (int }\n", "1 expected a type, found `(`");
    errs("type User = { str: nme }\n", "1 `str: nme` has the name and type swapped: write `nme: str`");
}

#[test]
fn half_a_question_mark_choice_says_it_needs_a_colon() {
    let want = "2 `?` needs `:` and a value for when it does not hold: `x ? 1 : 0`, or `x && 1`";
    errs("let x = true\nlet y = x ? 1\n", want);
    errs("let x = true\nlet y = (x) ? 1\n", want);
}

#[test]
fn arguments_side_by_side_need_a_comma() {
    errs("let a = 1\nlet b = 2\nprint(a b)\n", "3 expected `)` after arguments, found `b`");
}

#[test]
fn a_match_arm_s_name_then_a_value_needs_its_arrow() {
    errs("let v = 1\nlet r = match v {\n  x 1 => 2\n}\n", "3 expected `=>` after the match pattern, found `1`");
}

#[test]
fn a_handler_that_assigns_is_not_a_value() {
    errs(
        "component C() {\n  state n = 0\n  <button @click={n = 1}>x</button>\n}\n",
        "3 an assignment is not a value: a handler is a function, `@click={() => { n = ... }}`",
    );
    errs(
        "component C() {\n  state n = 0\n  <button @click={n -= 1}>x</button>\n}\n",
        "3 an assignment is not a value: a handler is a function, `@click={() => { n -= ... }}`",
    );
    errs(
        "component C() {\n  state n = { a: 0 }\n  <button @click={n.a += 1}>x</button>\n}\n",
        "3 an assignment is not a value: a handler is a function, `@click={() => { x += ... }}`",
    );
}

#[test]
fn a_template_with_two_values_in_one_place_is_refused() {
    errs("let n = 1\nprint(`a ${n n} b`)\n", "2 unexpected text inside `{…}` in string");
}

#[test]
fn a_route_parameter_with_an_empty_type_is_not_a_parameter() {
    errs(
        "route GET /a/{id:} = 1\n",
        "1 expected end of statement, found `:`: a map's key is a name or text, and for one in a variable use `set(m, k, v)`",
    );
}

#[test]
fn private_components_and_views_load() {
    let mut it = Interp::new(".");
    it.load_source("private component C() {\n  <p/>\n}\nprivate view V() = <p>v</p>\nlet shown = V()\n", "case.qwe").unwrap();
    assert!(load_error("private type T = { a: int }\n").contains("`private type` is not there yet"));
}

#[test]
fn a_file_style_that_is_global_and_one_that_is_global_alone() {
    let mut it = Interp::new(".");
    it.load_source("style global \"p { color: red }\"\ncomponent C() {\n  <p/>\n}\n", "case.qwe").unwrap();
    // `global` on a line of its own is the style's name, and this file has no components.
    errs("style global\nfn f() = 1\n", "3 a file's `style` styles the components declared in it, and this file has none");
}

#[test]
fn a_nested_routes_group_takes_its_outer_effects() {
    let r = check_source("routes /a with db {\n  routes /b {\n    route GET /x = 1\n  }\n}\n", "t.qwe", Path::new("."));
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
    errs(
        "routes /a with db {\n  routes /b {\n    route GET /x {\n      http.get(\"https://x.com\")\n    }\n  }\n}\n",
        "4 `http.get` needs the http effect, but `route GET /a/b/x` is declared `with db`",
    );
}

#[test]
fn a_view_s_own_style_with_no_element_of_its_own_still_reaches_the_page() {
    use quillo::server::HttpRequest;
    let mut it = Interp::new(".");
    let src = "let v_css = css`#p { color: rgb(4, 5, 6) }`\nview Inner() = <p id=\"p\">inner</p>\nview V() {\n    style v_css\n    <Inner/>\n}\nroute GET / = <html><head></head><body><V/></body></html>\n";
    it.load_source(src, "case.qwe").unwrap();
    let headers = vec![("Host".to_string(), "localhost".to_string())];
    let page = it.handle(&HttpRequest { method: "GET".into(), url: "/".into(), headers, body: Vec::new(), remote: "127.0.0.1".into() }).text();
    assert!(page.contains("<p id=\"p\">inner</p>"), "{}", page);
    assert!(page.contains("/_quillo/s/"), "{}", page);
}

#[test]
fn a_statement_after_a_whole_choice_is_not_said_to_need_a_colon() {
    errs("let a = true\nlet y = a ? 1 : 2 3\n", "2 expected end of statement, found `3`");
    // The `:` further on: the choice is read, and its first value ends too soon.
    errs("let x = true\nlet y = x ? 1 2 : 3\n", "2 `?` needs `:` and a value for when it does not hold: `x ? 1 : 0`, or `x && 1`");
}

#[test]
fn use_after_a_statement_that_does_not_start_with_use_is_found_where_it_is() {
    errs("let a = 1 use\n", "1 expected end of statement, found `use`");
}

#[test]
fn a_list_pattern_with_a_type_expects_its_equals() {
    errs("let [a, b]: int = [1, 2]\n", "1 expected `=`, found `:`");
}

#[test]
fn several_callers_are_listed_with_commas() {
    let r = check_source("server fn refund() for [\"a\", \"b\"] = 1\n", "t.qwe", Path::new("."));
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
}

#[test]
fn a_view_with_a_style_and_other_calls_marks_only_the_style() {
    use quillo::server::HttpRequest;
    let mut it = Interp::new(".");
    let src = "let v_css = css`#q { color: rgb(7, 8, 9) }`\nview Inner() = <p id=\"q\">inner</p>\nview V() {\n    style v_css\n    print(\"made\")\n    <Inner/>\n}\nroute GET / = <html><head></head><body><V/></body></html>\n";
    it.load_source(src, "case.qwe").unwrap();
    let headers = vec![("Host".to_string(), "localhost".to_string())];
    let page = it.handle(&HttpRequest { method: "GET".into(), url: "/".into(), headers, body: Vec::new(), remote: "127.0.0.1".into() }).text();
    assert!(page.contains("<p id=\"q\">inner</p>") && page.contains("/_quillo/s/"), "{}", page);
}
