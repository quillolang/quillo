//! The checker's rarer words, each with its exact message and line: what a `let` can never
//! match, letters that look alike, statements whose answer is dropped, effects a function
//! may use, services that are elsewhere, and inputs whose value nothing takes.

mod support;

use quillo::check::{check_file, check_source, check_source_opts, Diagnostic, Options, Severity};
use std::path::{Path, PathBuf};
use support::temp_dir;

fn shown(d: &Diagnostic) -> String {
    let sev = match d.severity {
        Severity::Error => "",
        Severity::Warning => " warning:",
        Severity::Hint => " hint:",
    };
    format!("{}{} {}", d.line, sev, d.message)
}

fn diags_in(src: &str, dir: &Path) -> Vec<String> {
    check_source(src, "t.qwe", dir).diagnostics.iter().map(shown).collect()
}

fn diags(src: &str) -> Vec<String> {
    diags_in(src, Path::new("."))
}

fn says(src: &str, want: &[&str]) {
    assert_eq!(diags(src), want, "\n--- source ---\n{}", src);
}

/// A folder of files for one test, gone when it ends.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str, files: &[(&str, &str)]) -> Folder {
        let d = temp_dir(&format!("cover-check-{}", name));
        for (f, text) in files {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        Folder(d.canonicalize().unwrap())
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const NEVER_MATCHES: &str = "this `let` never matches the value it is given, so it fails every time it runs";

#[test]
fn a_let_of_a_decimal_another_number_or_the_wrong_shape_never_matches() {
    says("let 1.5 = 2.5m\n", &[&format!("1 {}", NEVER_MATCHES)]);
    says("let { a } = [1]\n", &[&format!("1 {}", NEVER_MATCHES)]);
    says("let [a, b] = { a: 1 }\n", &[&format!("1 {}", NEVER_MATCHES)]);
    // The same number either way matches.
    says("let 2.5 = 2.5m\n", &[]);
}

#[test]
fn a_greek_or_cyrillic_letter_in_a_name_is_named_by_its_code_point() {
    says(
        "let total = 1\nlet MAX = 2\nprint(t\u{3bf}tal)\nprint(\u{41c}AX)\n",
        &[
            "3 unknown name `t\u{3bf}tal` (`total` is defined on line 1 with other letters that look the same: this one has U+03BF, a Greek letter, not `o`)",
            "4 unknown name `\u{41c}AX` (`MAX` is defined on line 2 with other letters that look the same: this one has U+041C, a Cyrillic letter, not `M`)",
        ],
    );
}

#[test]
fn arithmetic_on_a_name_alone_is_worked_out_and_dropped() {
    says(
        "let x = 1\nx * 2\nx / 2\nx - 3\nx + 1\n",
        &[
            "2 warning: this is worked out and dropped: to change `x`, `x = x * ...` (or `/`)",
            "3 warning: this is worked out and dropped: to change `x`, `x = x * ...` (or `/`)",
            "4 warning: `x - ...` is worked out and dropped: to change `x`, `x -= ...`",
            "5 warning: `x + ...` is worked out and dropped: to change `x`, `x += ...`",
        ],
    );
}

#[test]
fn react_s_html_for_is_said_to_be_for() {
    says("fn f() = <label htmlFor=\"x\">hi</label>\n", &["1 warning: `htmlFor` is React's name: write `for`"]);
}

#[test]
fn a_type_name_type_of_never_gives_is_said() {
    says(
        "let x = 1\nprint(2)\nlet b = type_of(x) == \"integer\"\nprint(b)\n",
        &["3 warning: `type_of` never gives \"integer\": it gives int, float, dec, str, bool, list, map, fn, html, response, null or error, or a record's type"],
    );
}

#[test]
fn a_match_arm_s_unused_name_is_said_to_be_any_value() {
    says(
        "let v = 3\nlet r = match v {\n  1 => \"a\"\n  other => \"b\"\n}\nprint(r)\n",
        &["4 warning: `other` takes any value and is not used: write `_` for that"],
    );
}

#[test]
fn a_server_fn_or_a_type_inside_a_function_is_refused() {
    says("fn outer() {\n  server fn inner() = 1\n  inner()\n}\nouter()\n", &["2 `server fn` and `component` must be declared at the top level"]);
    says("fn outer() {\n  type Point = { a: int }\n  1\n}\nouter()\n", &["2 `type` must be at the top level"]);
}

#[test]
fn a_typed_parameter_s_default_of_another_type_is_said() {
    says("fn f(a: int = \"x\") = a\nprint(f())\n", &["1 the default of `a` is str, but it must be int"]);
}

#[test]
fn what_a_function_calls_on_a_field_or_pipes_to_a_module_counts_for_its_effects() {
    says(
        "fn helper(xs) {\n  let m = { a: { b: (x) => x } }\n  m.a.b(1)\n  \"https://x.com\" | http.get\n}\nfn f() with db {\n  helper([1])\n}\nf()\n",
        &["7 warning: `helper` may use the http effect, but `f` is declared `with db`"],
    );
}

#[test]
fn a_route_with_db_cannot_call_http() {
    says(
        "routes /a with db {\n  route GET /x {\n    http.get(\"https://example.com\")\n  }\n}\n",
        &["3 `http.get` needs the http effect, but `route GET /a/x` is declared `with db`"],
    );
}

const ELSEWHERE: &str = "1 warning: service billing: billing.qwe is not here, so its calls are not checked; it must run elsewhere (QUILLO_SERVICE_BILLING or --service-url)";

#[test]
fn a_service_that_runs_elsewhere_is_called_over_http() {
    // From a function that may not: its call is HTTP.
    says(
        "service billing = \"billing.qwe\"\nfn f() with db {\n  billing.charge(1)\n}\nf()\n",
        &[ELSEWHERE, "3 `billing.charge` needs the http effect, but `f` is declared `with db`"],
    );
    // From the top level: anything goes.
    says("service billing = \"billing.qwe\"\nlet r = billing.charge(1)\nprint(r)\n", &[ELSEWHERE]);
    // A client secret written in the call options.
    says(
        "service billing = \"billing.qwe\" { auth: { oauth: { client_secret: \"abc\" } } }\n",
        &[ELSEWHERE, "1 warning: service billing: its `client_secret` is written in the source, where every copy of the program has it: read it with env(\"...\")"],
    );
}

#[test]
fn a_service_file_that_does_not_parse_is_an_error_at_the_service_line() {
    let f = Folder::new("bad-service", &[("bad.qwe", "fn (( broken\n")]);
    let d = diags_in("service billing = \"bad.qwe\"\nprint(1)\n", &f.0);
    assert_eq!(d.len(), 1, "{:?}", d);
    assert!(
        d[0].starts_with("1 ") && d[0].ends_with("bad.qwe: line 1: anonymous functions are written `(x) => x * 2` (or `x => x * 2`); `fn` is for named functions"),
        "{:?}",
        d
    );
}

#[test]
fn a_dropped_push_on_a_pipe_says_how_to_keep_it() {
    says("let xs = [1]\nxs | push(2)\nprint(xs)\n", &["2 warning: `push` gives a new value and changes nothing, and it is dropped here: write `xs = xs | push(...)`"]);
}

#[test]
fn a_function_tested_for_truth_is_said_only_by_its_name() {
    says("let ready = () => true\nlet x = ready || 2\nprint(x)\n", &["2 warning: `ready` is a function, which counts as true: to test its answer, `ready()`"]);
    // A field that holds a function, or a lambda, is not named.
    says("let m = { f: () => 1 }\nif m.f { print(1) }\nif (() => 1) { print(2) }\n", &[]);
}

#[test]
fn a_function_called_before_what_it_reads_is_defined_is_said_past_a_bare_return() {
    says("fn f() {\n  print(later)\n  return\n}\nprint(f())\nlet later = 1\n", &["5 `f` reads `later`, which is defined at line 6, after this call"]);
}

#[test]
fn a_class_from_a_block_that_ends_in_a_let_writes_no_class() {
    says("let on = true\nfn f() = <p class={if on { \"a\" } else { let z = 1 }}>x</p>\n", &["2 warning: unused variable `z` (name it `_z` if that is intended)"]);
}

#[test]
fn an_input_whose_value_nothing_takes_says_whose_it_is() {
    says(
        "component C(v) {\n  <input value={v}/>\n}\n",
        &["2 warning: typing here changes nothing: `v` is `C`'s parameter, which `value=` only shows: take what changes with `@input={(e) => ...}`, or add `readonly`"],
    );
    // Through `??`, and a field of the parameter.
    says(
        "component C(p) {\n  <input value={p.name ?? \"\"}/>\n}\n",
        &["2 warning: typing here changes nothing: `p.name` is a field of `C`'s parameter `p`, which `value=` only shows: take what changes with `@input={(e) => ...}`, or add `readonly`"],
    );
    // A state's field.
    says(
        "component C() {\n  state form = { a: \"\" }\n  <textarea value={form.a}/>\n}\n",
        &["3 warning: typing here changes nothing: `form.a` is a field of the state `form`, which `value=` only shows; give it a state of its own, or take what changes with `@input={(e) => ...}`, or add `readonly`"],
    );
    // Nothing can be typed, the type is decided as it runs, or the state itself is shown.
    says("component C(v) {\n  <input value={v} disabled/>\n}\n", &[]);
    says("component C(v, t) {\n  <input type={t} value={v}/>\n}\n", &[]);
    says("component C() {\n  state on = false\n  <input type=\"checkbox\" checked={on}/>\n}\n", &[]);
}

#[test]
fn a_let_in_a_function_that_never_matches_is_said() {
    says("fn f() {\n  let 1 = 2\n}\nf()\n", &[&format!("2 {}", NEVER_MATCHES)]);
}

#[test]
fn a_name_let_twice_at_the_top_level_is_one_variable() {
    says("let x = 1\nlet x = 2\nprint(x)\n", &[]);
}

#[test]
fn a_pipe_into_a_method_of_a_map_is_not_a_dropped_push() {
    says("let m = { push: (a, b) => a }\nlet xs = [1]\nxs | m.push(2)\nprint(xs)\n", &[]);
}

#[test]
fn a_service_key_read_from_the_environment_is_not_said() {
    says("service billing = \"billing.qwe\" { auth: { key: env(\"K\"), oauth: { client_id: \"x\" } } }\n", &[ELSEWHERE]);
}

#[test]
fn a_name_read_before_its_let_in_an_inner_function_says_where_it_is() {
    says("fn outer() {\n  fn inner() {\n    print(qq)\n    let qq = 1\n  }\n  inner()\n}\nouter()\n", &["3 `qq` is used before it is defined (line 4)"]);
    // In a function inside another, read where no `let` is: unknown, said once the outer ends.
    says("fn f() {\n  fn g() {\n    print(tmp)\n  }\n  g()\n}\nf()\n", &["3 unknown name `tmp` (did you mean `t`?)"]);
    // A `let` of that name in a block, or in another function, is not the one read.
    says("fn f() {\n  print(tmp)\n  if true {\n    let tmp = 1\n    print(tmp)\n  }\n}\nf()\n", &["2 unknown name `tmp` (did you mean `t`?)"]);
    says("fn f() {\n  print(tmp)\n}\nf()\nfn g() {\n  let tmp = 1\n  print(tmp)\n}\ng()\n", &["2 unknown name `tmp` (did you mean `t`?)"]);
    // A lambda's read, run after the `let`.
    says("fn f() {\n  let h = () => {\n    print(w)\n  }\n  h()\n  let w = 1\n}\nf()\n", &[]);
}

#[test]
fn classes_from_a_branch_that_ends_in_a_let_are_none() {
    let opts = Options { classes: true, ..Default::default() };
    let src = "let on = true\nfn f() = <p class={if on { \"a\" } else { let z = 1 }}>x</p>\nprint(f())\n";
    let d: Vec<String> = check_source_opts(src, "t.qwe", Path::new("."), opts).diagnostics.iter().map(shown).collect();
    assert_eq!(d, ["2 warning: unused variable `z` (name it `_z` if that is intended)"]);
}

#[test]
fn a_file_that_takes_a_name_as_another_and_calls_the_old_one_is_told_once() {
    let f = Folder::new("old-name", &[("lib/m.qwe", "fn price(x) = x * 2\n"), ("main.qwe", "import lib/m.qwe use price as p\nprint(price(1))\nprint(price(2))\nprint(p(3))\n")]);
    let d: Vec<String> = check_file(&f.0.join("main.qwe")).diagnostics.iter().map(shown).collect();
    assert_eq!(d, ["2 warning: this file takes `price` as `p`: write `p`, or take `price` too"]);
}
