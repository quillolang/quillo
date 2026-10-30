//! `run_script` at its edges: every kind of statement it refuses, values too
//! large, and what stops a script.

fn run(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n{}", e, src),
    }
}

/// The value of the script `src`, with `vars` as the program's text for them.
fn script(src: &str, vars: &str) -> String {
    run(&format!("run_script({:?}, {})", src, vars))
}

#[track_caller]
fn refused(src: &str, wanted: &str) {
    assert_eq!(script(src, "{}"), format!("err({:?})", wanted), "{}", src);
}

#[test]
fn a_script_cannot_declare_what_belongs_to_a_program() {
    refused("route GET / = 1", "a script cannot define routes");
    refused("mount /notes = \"notes.qwe\"", "a script cannot define routes");
    refused("service billing = \"billing.qwe\"", "a script cannot call services");
    refused("use \"other.qwe\"", "a script cannot `use` files");
    refused("fn f() = 1", "a script cannot define functions (use `(x) => ...`)");
}

#[test]
fn what_is_forbidden_is_found_wherever_it_is() {
    let markup = "a script gives values, not markup";
    refused("let a = <b>x</b>", markup);
    refused("{ a: <b>x</b> }", markup);
    refused("[1, { a: [<b>x</b>] }]", markup);
    refused("`${<b>x</b>}`", markup);
    refused("match 1 {\n 1 => <b>x</b>\n _ => 2\n}", markup);
    refused("match 1 {\n x if <b>x</b> => 1\n _ => 2\n}", markup);
    refused("match <b>x</b> {\n _ => 2\n}", markup);
    refused("for x in [1] {\n <b>x</b>\n}", markup);
    refused("(x = <b>x</b>) => x", markup);
    refused("let m = {}\nm.a = <b>x</b>", markup);
    refused("f(<b>x</b>).a = 1", markup);
    refused("type Thing = { a: str = <b>x</b> }", markup);
    refused("reply 200 \"x\"", "a script cannot reply");
}

#[test]
fn a_maps_fields_are_not_set_in_place() {
    assert_eq!(script("let m = { a: 1 }\nm.b = 2", "{}"), "err(\"line 2: a map's fields are not set in place: write `m = set(m, \\\"b\\\", …)`\")");
}

#[test]
fn match_and_maps_work_in_a_script() {
    assert_eq!(script("match value {\n 1 => \"one\"\n x if x > 5 => \"big\"\n _ => \"small\"\n}", "{ value: 7 }"), "\"big\"");
}

#[test]
fn return_ends_a_script_with_its_value() {
    assert_eq!(script("return 5\n6", "{}"), "5");
    assert_eq!(script("if value > 1 {\n return \"big\"\n}\n\"small\"", "{ value: 2 }"), "\"big\"");
    assert_eq!(script("return\n6", "{}"), "null");
}

#[test]
fn a_helper_that_replies_does_not_reply_for_the_script() {
    assert_eq!(run("fn h() {\n reply 200 \"x\"\n}\nrun_script(\"h()\", { h })"), "err(\"a script cannot reply\")");
}

#[test]
fn a_script_cannot_declare_a_type() {
    assert_eq!(script("type Thing = { a: int }\n1", "{}"), "err(\"line 1: `type` must be at the top level\")");
    let e = quillo::eval_str("run_script(\"type Thing = { a: int }\", {})\nThing({ a: 1 })").err().expect("the program has no such type");
    assert!(e.to_string().contains("unknown name `Thing`"), "{}", e);
}

#[test]
fn a_map_too_large_is_stopped() {
    let out = script("let m = {}\nset(m, key, 1)", "{ key: repeat(\"k\", 1100000) }");
    assert!(out.contains("the script made a value larger than 1 MB"), "{}", out);
}

#[test]
fn a_value_inside_an_error_counts() {
    let out = script("err(text)", "{ text: repeat(\"x\", 1100000) }");
    assert!(out.contains("the script made a value larger than 1 MB"), "{}", out);
    assert_eq!(script("let inner = err(pad)\ninner", "{ pad: \"x\" }"), "err(\"x\")");
}

#[test]
fn nothing_is_counted_for_arguments_that_will_fail_anyway() {
    assert!(script("join(5, \"a\")", "{}").contains("join"));
    assert!(script("replace(5, \"a\", \"b\")", "{}").contains("replace"));
    assert!(script("split(5, \"a\")", "{}").contains("split"));
    assert_eq!(script("split(\"a b\")", "{}"), "[\"a\", \"b\"]");
    assert_eq!(script("join([\"a\", \"b\"], \"-\")", "{}"), "\"a-b\"");
    assert_eq!(script("replace(\"aa\", \"a\", \"b\")", "{}"), "\"bb\"");
}

#[test]
fn a_split_that_would_make_too_much_is_stopped_before_it_runs() {
    let out = script("split(s, \"\")", "{ s: repeat(\"a\", 200000) }");
    assert!(out.contains("larger than 1 MB"), "{}", out);
    let out = script("split(s)", "{ s: repeat(\" \", 200000) }");
    assert!(out.contains("larger than 1 MB"), "{}", out);
}

#[test]
fn dates_work_in_a_script_without_the_clock() {
    assert_eq!(script("date.add_days(\"2026-12-31\", 1)", "{}"), "\"2027-01-01\"");
    assert_eq!(script("date.format(d, \"D MMM YYYY\")", "{ d: \"2026-10-27\" }"), "\"27 Oct 2026\"");
    assert_eq!(script("date.today", "{}"), "null");
    assert!(script("date.today()", "{}").starts_with("err("), "{}", script("date.today()", "{}"));
    assert!(script("date.now()", "{}").starts_with("err("), "{}", script("date.now()", "{}"));
}
