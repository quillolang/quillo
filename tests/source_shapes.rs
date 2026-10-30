//! Source text at the corners of the language: templates that never end,
//! markup left open before a declaration, pattern keys, names Quillo has
//! removed, a list of lists as a field's type, and text read as `num` or `date`.

use quillo::check::check_source;
use quillo::eval_str;
use quillo::server::HttpRequest;
use quillo::Interp;
use std::path::Path;

fn gives(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

#[track_caller]
fn refused(src: &str, line: u32, want: &str) {
    match eval_str(src) {
        Ok(v) => panic!("{} gave {}", src, v.repr()),
        Err(e) => assert_eq!((e.line, e.msg.as_str()), (line, want), "{}", src),
    }
}

fn get(it: &mut Interp, url: &str) -> (u16, String) {
    let h = vec![("Host".to_string(), "x".to_string())];
    let r = it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers: h, body: vec![], remote: "127.0.0.1".into() });
    (r.status, String::from_utf8_lossy(&r.body).into_owned())
}

#[test]
fn a_template_string_that_never_ends_is_told_from_where_it_began() {
    refused("let a = 1\nlet x = `abc", 2, "unexpected end of input: unterminated template string");
    refused("`a\nb\nc", 1, "unexpected end of input: unterminated template string");
    refused("`ends in an escape \\", 1, "unexpected end of input: unterminated template string");
}

#[test]
fn markup_left_open_says_which_declaration_it_ran_into() {
    let open = "view P() {\n  <div>\n    hello\n";
    let want = |word: &str| format!("`<div>` is never closed: add `</div>` before `{}` on line 4", word);
    refused(&format!("{}view Other() {{ <p>x</p> }}\n", open), 2, &want("view"));
    refused(&format!("{}component Card() {{ <p>x</p> }}\n", open), 2, &want("component"));
    refused(&format!("{}type Order = \"a\" | \"b\"\n", open), 2, &want("type"));
}

#[test]
fn a_plain_word_in_lower_case_is_not_a_declaration_in_open_markup() {
    refused("view P() {\n  <div>\nview the page\ntype of thing\n", 2, "`<div>` is never closed: add `</div>` at the end of the file");
}

#[test]
fn a_patterns_keys_are_plain_strings() {
    refused("let k = \"a\"\nmatch { a: 1 } {\n  { `${k}`: v } => v,\n  _ => 0,\n}", 3, "pattern keys must be plain strings");
    assert_eq!(gives("match { \"a b\": 1 } {\n  { \"a b\": v } => v,\n  _ => 0,\n}"), "1");
}

#[test]
fn a_removed_name_says_what_to_do_instead() {
    refused("csp()", 1, "unknown name `csp`: Quillo removed it in 0.5.0 (205d7cd); set the headers on the response (respond(status, body, headers)) or at the proxy in front");
    refused("redact(1)", 1, "unknown name `redact`: Quillo removed it in 0.5.0 (205d7cd); leave what must not be logged out of what is printed");
    assert_eq!(quillo::error::removed_note("still_here"), "");
    let a = check_source("audit(\"x\")", "t.qwe", Path::new("."));
    assert!(a.diagnostics[0].message.starts_with("unknown name `audit`: Quillo removed it in 0.5.0"), "{}", a.diagnostics[0].message);
}

#[test]
fn a_program_may_have_an_audit_of_its_own() {
    let a = check_source("fn audit(x: int) -> int = x\naudit(1)\n", "t.qwe", Path::new("."));
    assert!(a.diagnostics.is_empty(), "{:?}", a.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
    assert_eq!(gives("fn audit(x) = x + 1\naudit(1)"), "2");
    let args = check_source("fn audit(x: int) -> int = x\naudit(\"a\")\n", "t.qwe", Path::new("."));
    assert_eq!(args.diagnostics.len(), 1);
    assert_eq!(args.diagnostics[0].message, "argument `x` of `audit` must be int, got str");
}

#[test]
fn assigning_onto_a_name_never_declared_stops() {
    refused("nothere += 1", 1, "unknown name `nothere`");
    refused("nothere -= 1", 1, "unknown name `nothere`");
    assert_eq!(gives("let x = 1\nx += 2\nx"), "3");
}

#[test]
fn records_are_made_inside_a_list_of_lists() {
    let types = "type Cell = { n: float }\ntype Grid = { rows: list<list<Cell>> }\ntype Scores = { rows: list<list<float>> }\n";
    assert_eq!(gives(&format!("{}Grid({{ rows: [[{{ n: 1 }}]] }})", types)), "{ rows: [[{ n: 1.0 }]] }");
    assert_eq!(gives(&format!("{}Scores({{ rows: [[1, 2.5]] }})", types)), "{ rows: [[1.0, 2.5]] }");
    assert_eq!(gives(&format!("{}Grid({{ rows: [[{{ n: \"x\" }}]] }})", types)), "err(\"Grid: `rows[0][0].n` must be float, got str \\\"x\\\"\")");
    // Nothing to make in a list of lists of ints: it stays as it is.
    assert_eq!(gives("type Ints = { rows: list<list<int>> }\nInts({ rows: [[1]] })"), "{ rows: [[1]] }");
}

#[test]
fn a_path_segment_read_as_num_is_an_int_or_a_float() {
    let mut it = Interp::new(&std::env::temp_dir());
    it.load_source("route GET /n/:x(num) = type_of(x) + \" \" + str(x)\n", "app.qwe").unwrap();
    assert_eq!(get(&mut it, "/n/7"), (200, "int 7".to_string()));
    assert_eq!(get(&mut it, "/n/2.5"), (200, "float 2.5".to_string()));
    assert_eq!(get(&mut it, "/n/seven").0, 404);
}

#[test]
fn a_path_segment_read_as_a_date_stays_as_written() {
    let mut it = Interp::new(&std::env::temp_dir());
    it.load_source("route GET /d/:x(date) = x\n", "app.qwe").unwrap();
    assert_eq!(get(&mut it, "/d/2026-02-28"), (200, "2026-02-28".to_string()));
    assert_eq!(get(&mut it, "/d/2026-02-30").0, 404);
    assert_eq!(get(&mut it, "/d/soon").0, 404);
}

#[test]
fn what_the_browser_sends_for_a_decimal_or_a_large_int_may_be_neither() {
    use quillo::interp::from_island_json as read;
    use serde_json::json;
    assert_eq!(read(&json!({ "$dec": "12.50" })).repr(), "12.50m");
    assert_eq!(read(&json!({ "$dec": "twelve" })).repr(), "\"twelve\"");
    assert_eq!(read(&json!({ "$int": "9007199254740993" })).repr(), "9007199254740993");
    assert_eq!(read(&json!({ "$int": "lots" })).repr(), "\"lots\"");
    // Only with text inside, and alone in its object, are they marks at all.
    assert_eq!(read(&json!({ "$int": 5 })).repr(), "{ \"$int\": 5 }");
}
