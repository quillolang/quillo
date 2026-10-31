//! Checks and corrections (FEEDBACK16.md, FEEDBACK18.md, FEEDBACK20.md, FEEDBACK21.md; branch
//! feedback20-fixes): what `quillo check` says of markup that goes nowhere and of what a module
//! does not have, and what the browser gives where the server already had the answer.

mod support;

use quillo::check::check_file;
use quillo::client;
use quillo::interp::{Ctrl, Interp};
use std::path::PathBuf;
use std::process::Command;
use support::{send, serve};

fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback21-{}", name));
    for (f, src) in files {
        std::fs::write(d.join(f), src).unwrap();
    }
    d.join("main.qwe")
}

/// Each message of `quillo check`, with its line, and whether it stops the program.
fn said(name: &str, src: &str) -> Vec<String> {
    let f = folder(name, &[("main.qwe", src)]);
    check_file(&f).diagnostics.iter().map(|d| format!("{} {}", d.line, d.message)).collect()
}

/// `body` on the server and in the browser's code, each as `repr` writes it.
fn both(body: &str) -> (String, String) {
    let src = format!("fn __t() {{\n{}\n}}\n", body);
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let f = quillo::value::lookup(&it.globals, "__t").unwrap();
    let server = match it.call(&f, vec![], 0) {
        Ok(v) => v.repr(),
        Err(Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => panic!("{}", src),
    };
    let js = client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let out = Command::new("node").arg("-e").arg(&js).output().expect("node");
    let shown = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (server, shown.trim().to_string())
}

fn node_available() -> bool {
    Command::new("node").arg("--version").output().is_ok()
}

// ---- Q-172: an element that goes nowhere ---------------------------------------------------

const DROPPED: &str = "this element is not on the page: only the last one in a body is; wrap them in one element";

#[test]
fn the_elements_before_the_last_are_said_to_go_nowhere() {
    let d = format!("2 {}", DROPPED);
    assert_eq!(said("pair-component", "component Pair() {\n <p>one</p>\n <p>two</p>\n}\n"), [d.clone()]);
    assert_eq!(said("pair-view", "view Pair() {\n <p>one</p>\n <p>two</p>\n}\n"), [d.clone()]);
    assert_eq!(said("pair-fn", "fn pair() {\n <p>one</p>\n <p>two</p>\n}\n"), [d]);
}

#[test]
fn the_last_element_of_a_body_is_where_it_should_be() {
    assert!(said("one", "component One() {\n let x = 1\n <p>{x}</p>\n}\n").is_empty());
    assert!(said("wrapped", "component Two() {\n <div>\n  <p>one</p>\n  <p>two</p>\n </div>\n}\n").is_empty());
}

// ---- Q-170: a template's `$` in markup -----------------------------------------------------

#[test]
fn a_dollar_before_a_brace_in_markup_is_asked_about() {
    let got = said("dollar", "let name = \"web\"\nroute GET / = <p>\n  ${name}\n</p>\n");
    assert_eq!(got.len(), 1, "{:?}", got);
    assert!(got[0].starts_with("3 the `$` before this `{` is text"), "{:?}", got);
    assert!(got[0].contains("{\"$\"}{name}"), "{:?}", got);
}

#[test]
fn a_dollar_that_is_meant_is_left_alone() {
    assert!(said("price", "let price = 5\nroute GET / = <p>{\"$\"}{price}</p>\n").is_empty());
    assert!(said("plain", "let price = 5\nroute GET / = <p>$ {price} and $5</p>\n").is_empty());
}

#[test]
fn the_page_keeps_the_dollar_the_program_wrote() {
    let f = folder("dollar-page", &[("main.qwe", "let name = \"web\"\nroute GET /a = <p>${name}</p>\nroute GET /b = <p>{\"$\"}{name}</p>\n")]);
    let s = serve(&f, &[]);
    assert_eq!(send(s.port, "GET", "/a", &[], "").body, "<p>$web</p>");
    assert_eq!(send(s.port, "GET", "/b", &[], "").body, "<p>$web</p>");
}

// ---- Q-107: what a module does not have ----------------------------------------------------

#[test]
fn a_module_says_what_it_does_not_have() {
    assert_eq!(said("no-member", "print(json.title)\nprint(db.nothing)\n"), ["1 `json` has no `title`", "2 `db` has no `nothing`"]);
}

#[test]
fn a_shape_binds_its_fields_and_says_so() {
    assert_eq!(
        said("shape", "route POST /notes { json: { title: str } } = json.title\n"),
        ["1 `json` has no `title` (`title` alone is the name here: a shape binds its fields)"]
    );
}

#[test]
fn what_a_module_has_or_a_name_that_hides_it_is_not_a_mistake() {
    assert!(said("members", "print(math.pi + math.e + math.tau + math.inf + math.nan)\nprint([json.parse, db.query])\n").is_empty());
    assert!(said("param", "fn f(json) = json.title\nprint(f({ title: 1 }))\n").is_empty());
    assert!(said("route", "route POST /o { json: { a: str } as json } = json.a\n").is_empty());
}

// ---- Q-126: a service through a helper ----------------------------------------------------

#[test]
fn a_parameter_named_like_a_service_is_no_call_over_http() {
    let f = folder("service-param", &[
        ("billing.qwe", "server fn invoice() = 1\n"),
        ("main.qwe", "service billing = \"billing.qwe\"\nfn load(billing) = billing.invoice()\nfn f() with db = load({ invoice: () => 1 })\n"),
    ]);
    assert!(check_file(&f).diagnostics.is_empty(), "{:?}", check_file(&f).diagnostics);
}

// ---- Q-142: a plugin's fixed-size array ---------------------------------------------------

#[test]
fn a_fixed_size_array_is_named_in_either_direction() {
    let p = quillo::native::parse("pub fn back() -> [u8; 4] { [0; 4] }\npub fn after() -> i64 { 1 }\n");
    assert_eq!(p.exports.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["after"]);
    assert_eq!(p.problems.len(), 1, "{:?}", p.problems);
    assert!(p.problems[0].1.contains("`back` is not exported: the fixed-size array `[u8; 4]`"), "{:?}", p.problems);
}

#[test]
fn a_slice_still_crosses() {
    let p = quillo::native::parse("pub fn sum(xs: &[f64]) -> f64 { xs.iter().sum() }\n");
    assert_eq!((p.exports.len(), p.problems.len()), (1, 0));
}

// ---- Q-138, Q-136: where a statement stands and ends ---------------------------------------

#[test]
fn a_return_that_leaves_an_expression_has_its_line() {
    let got = said("return-line", "fn f(x) -> int {\n let y = x ?? return\n y\n}\n");
    assert!(got.iter().all(|m| m.starts_with("2 ")), "{:?}", got);
}

#[test]
fn a_list_type_ends_a_line_wherever_it_is_written() {
    assert!(said("list-type", "type Ids = list<int>\ntype Names = list<list<str>>?\nlet x = 1\nprint(x)\n").is_empty());
    assert!(said("list-field", "type User = {\n tags: list<str>\n name: str\n}\nlet x = 1\nprint(x)\n").is_empty());
    // A function's body may still start on the next line.
    assert!(said("list-fn", "fn f(xs: list<int>) -> list<int>\n{\n xs\n}\nprint(f([1]))\n").is_empty());
}

// ---- server and browser, one answer ---------------------------------------------------------

fn same(cases: &[(&str, &str)]) {
    if !node_available() {
        return;
    }
    for (body, want) in cases {
        assert_eq!(both(body), (want.to_string(), want.to_string()), "{}", body);
    }
}

#[test]
fn numbers_past_the_ends_are_the_same_in_the_browser() {
    same(&[
        ("is_err(int(1.0e30))", "true"),
        ("is_err(int(-1.0e30))", "true"),
        ("int(9007199254740991.0)", "9007199254740991"),
        ("1e999", "inf"),
        ("-1e999", "-inf"),
        ("`${1e999}`", "\"inf\""),
        ("math.lcm(0, 4)", "0"),
        ("math.lcm(4, 0)", "0"),
    ]);
}

#[test]
fn a_mean_and_a_sum_past_the_floats_are_the_same_in_the_browser() {
    same(&[
        ("stats.mean([1e308, 1e308]) == 1e308", "true"),
        ("stats.mean([1e308, 1e308, -1e308]) == 3.333333333333333e307", "true"),
        ("math.isnan(stats.mean([math.inf, -math.inf]))", "true"),
        ("math.fsum([1e308, -1e308, 1.5])", "1.5"),
    ]);
    if !node_available() {
        return;
    }
    for src in ["math.fsum([1e308, 1e308])", "math.fsum([1e308, 1e308, -1e308])"] {
        let (s, b) = both(src);
        assert!(s.contains("the sum is too large for a float"), "{}", s);
        assert!(b.contains("the sum is too large for a float"), "{}", b);
    }
    let (s, b) = both("math.fsum([math.inf, -math.inf])");
    assert!(s.contains("inf and -inf have no sum"), "{}", s);
    assert!(b.contains("inf and -inf have no sum"), "{}", b);
}

#[test]
fn a_constant_too_large_goes_to_the_browser_as_it_is() {
    if !node_available() {
        return;
    }
    let f = "let big = math.inf\nlet small = -math.inf\nlet none = math.nan\ncomponent C() = <p>{big}{small}{none}</p>\n";
    let js = client::bundle(&{
        let mut it = Interp::new(".");
        it.load_source(f, "case.qwe").unwrap();
        it
    })
    .unwrap();
    assert!(js.contains("Infinity") && js.contains("-Infinity") && js.contains("NaN"), "{}", js);
}

#[test]
fn a_component_writes_an_infinity_as_the_browser_does() {
    let f = folder("inf-page", &[("main.qwe", "component C() = <p>{1e999}</p>\nroute GET / = <html><body><C/></body></html>\n")]);
    let s = serve(&f, &[]);
    assert!(send(s.port, "GET", "/", &[], "").body.contains("<p>inf"), "the page says Infinity");
}

// ---- Q-121: a map in another order, inside what is sent ----------------------------------------

#[test]
fn a_row_whose_keys_moved_is_sent_again() {
    use quillo::livecache::{apply, diff};
    use serde_json::json;
    let (old, new) = (json!([{ "id": 1, "a": 1, "b": 2 }]), json!([{ "id": 1, "b": 2, "a": 1 }]));
    let got = apply(&old, &diff(&old, &new));
    assert_eq!(got.to_string(), new.to_string());
    let (old, new) = (json!({ "m": { "a": 1, "b": 2 } }), json!({ "m": { "b": 2, "a": 1 } }));
    assert_eq!(apply(&old, &diff(&old, &new)).to_string(), new.to_string());
    let same = json!({ "m": { "a": 1 }, "l": [1, 2] });
    assert_eq!(diff(&same, &same), json!({ "$=": 1 }));
}

// ---- Q-112: a private name in each kind of field ------------------------------------------------

#[test]
fn a_field_with_a_default_and_a_private_name_binds_its_key() {
    let f = folder("private-field", &[("main.qwe", "private let q = \"the variable\"\nroute GET /q ? { q: str = null } = q ?? \"nothing\"\n")]);
    let s = serve(&f, &[]);
    assert_eq!(send(s.port, "GET", "/q?q=sent", &[], "").body, "sent");
    assert_eq!(send(s.port, "GET", "/q", &[], "").body, "nothing");
}
