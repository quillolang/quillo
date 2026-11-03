//! The interpreter's rarer turns, each by the value or exact error it gives: null compared,
//! scripts that run with a budget of steps (so without the fast paths), loops long enough
//! for machine code, fields of maps named like functions, and markup rendered on the server.

mod support;

use quillo::server::HttpRequest;
use quillo::Interp;

/// A request to the program, in this process: its status and text.
fn send(it: &mut Interp, method: &str, url: &str, body: &str) -> (u16, String) {
    let headers = vec![
        ("Host".to_string(), "localhost".to_string()),
        ("content-type".to_string(), "application/json".to_string()),
        ("sec-fetch-site".to_string(), "same-origin".to_string()),
    ];
    let r = it.handle(&HttpRequest { method: method.into(), url: url.into(), headers, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() });
    (r.status, r.text())
}

fn value(src: &str) -> String {
    match Interp::new(".").load_source(src, "case.qwe") {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

fn error(src: &str) -> String {
    match Interp::new(".").load_source(src, "case.qwe") {
        Ok(v) => panic!("it gave {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => format!("{} {}", e.line, e.msg),
    }
}

#[test]
fn null_compared_with_a_number_either_way_says_what_null_is() {
    let hint = "what is not found is null, not -1, from `index_of`, `find` and `get` too, so ask `x == null` first";
    assert_eq!(error("1 < null"), format!("1 cannot compare int with null: {}", hint));
    assert_eq!(error("null < 1"), format!("1 cannot compare null with int: {}", hint));
}

#[test]
fn a_script_adds_and_takes_away_in_place() {
    assert_eq!(value("run_script(\"let x = 1\\nx += 2\\nx -= 1\\nx\", {})"), "2");
}

#[test]
fn a_script_s_while_loop_declares_in_its_body() {
    assert_eq!(value("run_script(\"let i = 0\\nlet t = 0\\nwhile i < 3 {\\n  let y = i\\n  t += y\\n  i += 1\\n}\\nt\", {})"), "3");
}

#[test]
fn a_script_calls_lambdas_and_reads_fields_through_them() {
    assert_eq!(value("run_script(\"map([1, 2], (x) => x + 1)\", {})"), "[2, 3]");
    assert_eq!(value("run_script(\"sort_by([{n: 2}, {n: 1}], (x) => x.n)\", {})"), "[{ n: 1 }, { n: 2 }]");
    assert_eq!(value("run_script(\"let s = 0\\nfor x in [1, 2] { s += x }\\ns\", {})"), "3");
}

#[test]
fn a_long_while_loop_that_declares_goes_on_in_machine_code() {
    assert_eq!(value("let i = 0\nlet t = 0\nwhile i < 5000 {\n  let y = i\n  t += y\n  i += 1\n}\nt"), "12497500");
    // One that fails there fails as it would have.
    assert_eq!(error("let i = 0\nwhile i < 5000 {\n  let y = 10 / (100 - i)\n  i += 1\n}\ni"), "3 division by zero");
    // And after it has gone on in machine code.
    assert_eq!(error("let i = 0\nwhile i < 5000 {\n  let y = 10 / (1000 - i)\n  i += 1\n}\ni"), "3 division by zero");
}

#[test]
fn a_function_with_a_default_called_often_gives_the_same_answers() {
    assert_eq!(value("fn f(a, b = 2) = a + b\nlet t = 0\nfor i in range(3000) {\n  t += f(i)\n}\nt"), "4504500");
}

#[test]
fn a_map_s_field_named_like_a_function_is_null() {
    assert_eq!(value("fn b() = 1\n{ a: 1 }.b"), "null");
}

#[test]
fn a_map_set_from_its_own_field_is_updated() {
    assert_eq!(value("let m = { a: 1 }\nm = set(m, \"a\", get(m, \"a\") + 1)\nm"), "{ a: 2 }");
    assert_eq!(value("let m = { a: [1] }\nm = set(m, \"a\", [1, 2])\nm"), "{ a: [1, 2] }");
    assert_eq!(value("let xs = [1, 2]\nxs = push(xs, len(xs))\nxs"), "[1, 2, 2]");
}

#[test]
fn an_index_into_a_map_or_a_number() {
    assert_eq!(value("let m = { a: 1 }\nm[0]"), "null");
    assert_eq!(error("let n = 5\nn[0]"), "2 cannot index int with int");
}

#[test]
fn a_for_with_an_arrow_body_runs_it_and_drops_its_value() {
    assert_eq!(value("let xs = [1, 2]\nfor x in xs => x * 2\nxs"), "[1, 2]");
    assert_eq!(error("for x in [1, 0] => 1 / x\n1"), "1 division by zero");
}

#[test]
fn markup_on_the_server_drops_handlers_and_keys_and_keeps_a_script_s_elements() {
    assert_eq!(value("<div @click={(e) => 1} key=\"k\">x</div>"), "<div>x</div>");
    assert_eq!(value("<script><b>x</b></script>"), "<script><b>x</b></script>");
}

#[test]
fn a_map_set_from_a_call_that_is_not_its_own_read_is_made_the_long_way() {
    assert_eq!(value("fn foo(x) = [x]\nlet m = {}\nm = set(m, \"k\", push(foo(1), 2))\nm"), "{ k: [1, 2] }");
    // Keys and items that are lists, maps or functions.
    assert_eq!(error("let m = { k: [1] }\nm = set(m, [1], push(m[[1]], 2))\nm"), "2 cannot index map with list");
    assert_eq!(error("let m = { k: [1] }\nm = set(m, { a: 1 }, push(m[{ a: 1 }], 2))\nm"), "2 cannot index map with map");
    assert_eq!(value("let m = { k: [1] }\nm = set(m, \"k\", push(m[\"k\"], [3]))\nm"), "{ k: [1, [3]] }");
    assert_eq!(value("let m = { k: [1] }\nm = set(m, \"k\", push(m[\"k\"], { a: 1 }))\nm"), "{ k: [1, { a: 1 }] }");
    assert_eq!(value("let m = { k: [1] }\nm = set(m, \"k\", push(m[\"k\"], (x) => x))\nlen(m.k)"), "2");
    // What to start from that is not a constant, or more of them than `get` takes.
    assert_eq!(value("let m = { a: { b: 1 } }\nm = set(m, \"a\", set(get(m, \"a\", {}, 1), \"b\", 2))\nm"), "{ a: { b: 2 } }");
    assert_eq!(value("let d = {}\nlet m = { a: { b: 1 } }\nm = set(m, \"a\", set(get(m, \"a\", d), \"b\", 2))\nm"), "{ a: { b: 2 } }");
    assert_eq!(value("let xs = [[1], [2]]\nxs = set(xs, 0, set(xs[0], 0, 5))\nxs"), "[[5], [2]]");
}

#[test]
fn merge_remove_and_a_long_set_on_a_variable_give_the_new_value() {
    assert_eq!(value("let m = { a: 1 }\nm = merge(m, { b: 2 })\nm"), "{ a: 1, b: 2 }");
    assert_eq!(value("let m = { a: 1, b: 2 }\nm = remove(m, \"a\")\nm"), "{ b: 2 }");
    assert_eq!(value("let m = { a: 1 }\nm = set(m, \"a\", 2, 3)\nm"), "{ a: 2 }");
}

#[test]
fn an_assignment_from_a_call_of_nothing_or_a_pipe_into_a_name() {
    assert_eq!(value("fn foo() = 1\nlet x = 0\nx = foo()\nx"), "1");
    assert_eq!(value("let s = \"a\"\ns = s | upper\ns"), "\"A\"");
    assert_eq!(value("let o = { f: (s, n) => s }\nlet s = \"a\"\ns = s | o.f(1)\ns"), "\"a\"");
}

#[test]
fn a_map_read_with_a_text_index_and_a_missing_field_through_a_lambda() {
    assert_eq!(value("let m = { a: 1 }\nm[\"a\"]"), "1");
    assert_eq!(value("map([{ a: 1 }], (x) => x.b)"), "[null]");
    assert_eq!(value("fn f(m) = m.b\nf({ a: 1 })"), "null");
}

#[test]
fn a_global_function_assigned_another_is_called_as_the_new_one() {
    assert_eq!(value("let g = () => 1\ng = () => 2\ng()"), "2");
}

#[test]
fn a_view_s_handlers_and_key_are_not_in_its_html() {
    assert_eq!(value("view V(n) = <div @click={(e) => n} key={n}>{n}</div>\nstr(V(1))"), "\"<div>1</div>\"");
}

#[test]
fn a_post_route_adds_to_a_local_where_writes_are_noted() {
    let mut it = Interp::new(".");
    it.load_source("route POST /n {\n  let x = 1\n  x += 2\n  x -= 1\n  x\n}\n", "case.qwe").unwrap();
    assert_eq!(send(&mut it, "POST", "/n", ""), (200, "2".to_string()));
}

#[test]
fn a_server_fn_called_again_and_one_that_reads_a_field_answer_the_same() {
    let mut it = Interp::new(".");
    it.load_source("server fn add(a, b) = a + b\nserver fn name(x) = x.name\nview W(n) -> html = <b key={n}>{n}</b>\nroute GET /w = <html><body><W n={1}/></body></html>\n", "case.qwe").unwrap();
    for _ in 0..3 {
        assert_eq!(send(&mut it, "POST", "/_quillo/rpc/add", "[1, 2]"), (200, "{\"ok\":3}".to_string()));
    }
    assert_eq!(send(&mut it, "POST", "/_quillo/rpc/name", "[{\"name\": \"a\"}]"), (200, "{\"ok\":\"a\"}".to_string()));
    assert_eq!(send(&mut it, "POST", "/_quillo/rpc/name", "[{\"x\": \"a\"}]"), (200, "{\"ok\":null}".to_string()));
    // A view with a declared result is checked, and rendered without its key.
    let (status, page) = send(&mut it, "GET", "/w", "");
    assert_eq!(status, 200);
    assert!(page.starts_with("<!doctype html><html><body><b>1</b>"), "{}", page);
}

#[test]
fn a_reply_at_the_top_of_a_used_file_is_outside_a_route() {
    let d = support::temp_dir("cover-interp-use-reply");
    std::fs::write(d.join("lib.qwe"), "reply 404\n").unwrap();
    let mut it = Interp::new(d.to_str().unwrap());
    let e = match it.load_source("use lib.qwe\n1\n", "case.qwe") {
        Ok(v) => panic!("it gave {}", v.repr()),
        Err(e) => e,
    };
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!((e.line, e.msg.as_str()), (0, "`reply` used outside of a route"));
}

#[test]
fn an_index_into_a_parameter_that_holds_a_map_or_a_number() {
    assert_eq!(value("fn f(m) = m[0]\nf({ a: 1 })"), "null");
    assert_eq!(value("fn f(m) = m[\"a\"]\nf({ a: 1 })"), "1");
    assert_eq!(error("fn f(n) = n[0]\nf(5)"), "1 cannot index int with int");
}

#[test]
fn a_long_loop_with_no_declaration_fails_in_machine_code_as_it_would_have() {
    assert_eq!(error("let i = 0\nlet t = 0\nwhile i < 5000 {\n  t = t + 10 / (1000 - i)\n  i += 1\n}\nt"), "4 division by zero");
    assert_eq!(error("fn f() {\n  let i = 0\n  let t = 0\n  while i < 5000 {\n    t = t + 10 / (1000 - i)\n    i += 1\n  }\n  t\n}\nf()"), "5 division by zero");
}
