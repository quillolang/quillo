//! The browser compiler's rarer paths: what it refuses, with its words and
//! line, and code that must compute in Node what it computes on the server.

mod support;

use quillo::client::{self, Bundles};
use quillo::Interp;
use std::process::Command;

fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Runs `body` as the body of a function on the server and in Node.
fn both(defs: &str, body: &str) -> (String, String) {
    let src = format!("{}\nfn __t() {{\n{}\n}}\n", defs, body);
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let server = match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(quillo::interp::Ctrl::Return(v)) => v.repr(),
        Err(quillo::interp::Ctrl::Reply(_)) => "REPLY".into(),
    };
    let js = client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let out = Command::new("node").arg("-e").arg(&js).output().expect("node");
    assert!(out.status.success(), "node failed: {}\n{}", String::from_utf8_lossy(&out.stderr), src);
    (server, String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `same`, and the value the server gave.
fn same_is(defs: &str, body: &str, want: &str) {
    if !node_available() {
        return;
    }
    let (s, b) = both(defs, body);
    assert_eq!(s, want, "\n--- defs ---\n{}\n--- body ---\n{}", defs, body);
    assert_eq!(b, want, "\n--- defs ---\n{}\n--- body ---\n{}", defs, body);
}

/// A program that loads, and its bundle.
fn loaded(src: &str) -> Interp {
    let mut it = Interp::new(".");
    it.load_source(src, "case.qwe").unwrap_or_else(|e| panic!("{}\n--- source ---\n{}", e, src));
    it
}

fn bundle(src: &str) -> String {
    client::bundle(&loaded(src)).unwrap_or_else(|e| panic!("{}\n--- source ---\n{}", e, src))
}

/// What loading a program says, with its line: the compile comes with it.
fn refused(src: &str) -> String {
    match Interp::new(".").load_source(src, "case.qwe") {
        Ok(_) => panic!("it loaded\n--- source ---\n{}", src),
        Err(e) => e.to_string(),
    }
}

#[test]
fn what_a_browser_function_cannot_declare() {
    assert_eq!(refused("fn plain() {\n  component Z() = <p></p>\n  1\n}\ncomponent C() = <p>{plain()}</p>"), "case.qwe:2: `plain` (compiled for the browser): `server fn` and `component` must be declared at the top level");
    assert_eq!(refused("fn plain() {\n  route GET / = 1\n}\ncomponent C() = <p>{plain()}</p>"), "case.qwe:2: `plain` (compiled for the browser): routes cannot be declared in browser code");
    assert_eq!(refused("fn plain() {\n  use \"x.qwe\"\n}\ncomponent C() = <p>{plain()}</p>"), "case.qwe:2: `plain` (compiled for the browser): `use` must be at the top level");
    assert_eq!(refused("fn plain() {\n  mount /x = \"x.qwe\"\n}\ncomponent C() = <p>{plain()}</p>"), "case.qwe:2: `plain` (compiled for the browser): `mount` must be at the top level");
    assert_eq!(refused("fn plain() {\n  service x = \"x.qwe\"\n}\ncomponent C() = <p>{plain()}</p>"), "case.qwe:2: `plain` (compiled for the browser): `service` must be at the top level");
    assert_eq!(refused("fn plain() {\n  type Thing = { a: int }\n}\ncomponent C() = <p>{plain()}</p>"), "case.qwe:2: `plain` (compiled for the browser): `type` must be at the top level");
    assert_eq!(refused("component C() {\n  fn inner() {\n    server fn s() = 1\n    s()\n  }\n  <p>{inner()}</p>\n}"), "case.qwe:3: <C> (compiled for the browser): `server fn` and `component` must be declared at the top level");
}

#[test]
fn what_cannot_run_in_a_browser() {
    assert_eq!(refused("fn f() = read_file(\"x\")\ncomponent C() = <p>{f()}</p>"), "case.qwe:1: `f` (compiled for the browser): `read_file` is not available in the browser; call it from a `server fn`");
    assert_eq!(refused("fn f() with db = 1\ncomponent C() = <p>{f()}</p>"), "case.qwe:2: `f` (compiled for the browser): `f` declares `with db`, which the browser cannot do; make it a `server fn`");
    assert_eq!(refused("fn f() = http.nope(\"u\")\ncomponent C() = <p>{f()}</p>"), "case.qwe:1: `f` (compiled for the browser): `http` has no function `nope`");
}

#[test]
fn what_cannot_be_sent_or_made_in_a_browser() {
    assert_eq!(refused("let m = { a: len, b: 1 }\nlet m = { a: upper, b: 2 }\ncomponent C() = <p>{m.b}</p>"), "case.qwe:3: <C> (compiled for the browser): `m` is a function, which cannot be sent to the browser");
    assert_eq!(refused("server fn s() = 1\nlet x = [s(), () => 1]\ncomponent C() = <p>{x}</p>"), "case.qwe:3: <C> (compiled for the browser): `x` waits on the server to be made, so the browser cannot make it");
    assert_eq!(refused("type Rec = { a: int = wait() }\nserver fn wait() = 1\ncomponent C() {\n  <p>{Rec({})}</p>\n}"), "case.qwe:1: <C> (compiled for the browser): the default of `a` in `Rec` waits on the server, which a record made in the browser cannot");
}

#[test]
fn what_a_handler_cannot_assign() {
    assert_eq!(refused("component C(p) {\n  <button @click={() => { p = 2 }}>{p}</button>\n}"), "case.qwe:2: <C> (compiled for the browser): cannot assign to `p`: it follows other values (a prop, a loop's item, a `let` of state); keep it in state: state p = ...");
}

#[test]
fn what_binding_and_live_need() {
    assert_eq!(refused("component C() {\n  <input bind:value=\"x\"/>\n}"), "case.qwe:2: <C> (compiled for the browser): `bind:value` needs a variable: bind:value={name}");
    assert_eq!(refused("component C() {\n  state a = 1\n  <input bind:nope={a}/>\n}"), "case.qwe:3: <C> (compiled for the browser): `bind:nope` is not supported (use bind:value or bind:checked)");
    assert_eq!(refused("let g = 1\ncomponent C() {\n  <input bind:value={g}/>\n}"), "case.qwe:3: <C> (compiled for the browser): `bind:value` needs component state, like `state g`");
    assert_eq!(refused("component C(p) {\n  <input bind:value={p}/>\n}"), "case.qwe:2: <C> (compiled for the browser): `bind:value` needs component state, like `state p`");
    assert_eq!(refused("component C() {\n  state x = live(print)\n  <p>{x}</p>\n}"), "case.qwe:2: <C> (compiled for the browser): `live` needs a server fn: `print` is not one");
    assert_eq!(refused("component C() {\n  let n = 0\n  <p>{waiting(n)}</p>\n}"), "case.qwe:3: <C> (compiled for the browser): `waiting` takes a component's live state by its name (`state n = live(...)`)");
    assert!(bundle("component C() {\n  state n = 0\n  <p>{waiting(n)}</p>\n}").contains("W.waiting("));
}

#[test]
fn a_variable_of_the_components_binds_by_assignment() {
    assert!(bundle("component C() {\n  let a = 1\n  <input bind:value={a}/>\n}").contains("e.target.value"));
}

#[test]
fn a_state_named_live_is_only_a_state() {
    let js = bundle("component C() {\n  state live = 1\n  state x = live(1)\n  <p>{x}</p>\n}");
    assert!(!js.contains("W.live("), "the state was taken for the builtin");
}

#[test]
fn a_program_with_no_elements_compiles_to_nothing_shown() {
    for src in [
        "component C() {\n}",
        "component C() {\n  state x = 1\n}",
        "component C() {\n  let [x, y] = [1, 2]\n}",
        "component C() {\n  fn inner() = 1\n}",
        "view V() {\n}\ncomponent C() = <V/>",
    ] {
        assert!(bundle(src).contains("return null"), "{}", src);
    }
}

#[test]
fn plain_functions_with_the_rarer_forms() {
    same_is("fn f() {\n let x = 2\n let x = 3\n x\n}", "f()", "3");
    same_is("fn f() {\n return\n}", "f()", "null");
    same_is("fn f(a: int?, b: str | null, c: float?, d: bool | int) = [a, b, c, d]", "f(null, null, null, true)", "[null, null, null, true]");
    same_is("fn f(a: int?) = a", "f(\"x\")", "ERROR: argument `a` of `f` must be int?, got str \"x\"");
}

#[test]
fn enumerate_and_ranges_in_a_loop() {
    same_is("", "let out = []\nfor [i, x] in enumerate([\"a\", \"b\"]) { out = out + [`${i}${x}`] }\nout", "[\"0a\", \"1b\"]");
    same_is("", "let out = []\nfor [_, x] in enumerate([\"a\", \"b\"]) { out = out + [x] }\nout", "[\"a\", \"b\"]");
    same_is("", "let out = []\nfor [i, { n }] in enumerate([{ n: 5 }]) { out = out + [i + n] }\nout", "[5]");
    same_is("", "let out = []\nfor [0, x] in enumerate([\"a\"]) { out = out + [x] }\nout", "[\"a\"]");
    same_is("", "let out = []\nfor x in enumerate([\"a\"]) { out = out + [x] }\nout", "[[0, \"a\"]]");
    same_is("", "let out = []\nfor [i, x, y] in enumerate([[1, 2]]) { out = out + [i] }\nout", "ERROR: `for` pattern does not match [0, [1, 2]]");
    same_is("", "let out = []\nfor i in 1..3 { out = out + [i] }\nout", "[1, 2]");
    same_is("", "let out = []\nfor i in range(2, 4) { out = out + [i] }\nout", "[2, 3]");
    same_is("", "let out = []\nfor [a, b] in [[1, 2]] { out = out + [a + b] }\nout", "[3]");
    same_is("", "let out = []\nfor [a, a] in [[1, 1]] { out = out + [a] }\nout", "[1]");
    same_is("", "let out = []\nfor [a, ..rest] in [[1, 2, 3]] { out = out + rest }\nout", "[2, 3]");
    same_is("", "let out = []\nfor [a, ..a] in [[1, 2, 3]] { out = out + [a] }\nout", "[[2, 3]]");
    same_is("", "let out = []\nfor [1, b] in [[1, 2]] { out = out + [b] }\nout", "[2]");
    same_is("", "let out = []\nfor { a } in [{ a: 7 }] { out = out + [a] }\nout", "[7]");
}

#[test]
fn a_function_with_only_an_error_type_is_checked_for_that() {
    same_is("fn f() ! str = err(\"no\")", "is_err(f())", "true");
    same_is("fn f() ! str = 1", "f()", "1");
}

#[test]
fn values_javascript_numbers_cannot_hold_are_written_exactly() {
    same_is("let big = 9007199254740993", "big", "9007199254740993");
    same_is("let price = 12.50m", "price", "12.50m");
    same_is("let xs = [9007199254740993, \"a\", 1]", "xs", "[9007199254740993, \"a\", 1]");
    same_is("let m = { n: 1.5m, s: \"x\" }", "m", "{ n: 1.5m, s: \"x\" }");
    same_is("let m = { \"__proto__\": 9007199254740993, a: true }", "len(keys(m))", "2");
    same_is("let m = { \"__proto__\": 9007199254740993, a: 1 }", "m[\"__proto__\"]", "9007199254740993");
}

#[test]
fn a_key_named_proto_is_a_key_in_the_bundle() {
    same_is("let m = { \"__proto__\": 1 }", "m[\"__proto__\"]", "1");
    same_is("let m = { \"__proto__\": { admin: true } }", "m.admin", "null");
}

#[test]
fn an_empty_template_is_an_empty_string() {
    same_is("", "``", "\"\"");
}

#[test]
fn patterns_with_a_rest_and_records_are_matched_at_run_time() {
    same_is("fn f(x) = match x {\n [{ a }, ..] => a\n _ => 0\n}", "f([{ a: 2 }, 3])", "2");
    same_is("fn f(x) = match x {\n [{ a }, ..more] => more\n _ => 0\n}", "f([{ a: 2 }, 3])", "[3]");
}

#[test]
fn a_public_variable_is_written_into_the_bundle() {
    same_is("", "env(\"PUBLIC_QUILLO_TEST_UNSET\") ?? \"none\"", "\"none\"");
    same_is("", "env(\"PUBLIC_QUILLO_TEST_UNSET\", \"fallback\")", "\"fallback\"");
}

#[test]
fn a_program_of_its_own_named_live_is_not_the_builtin() {
    let js = bundle("fn live(x) = x + 1\ncomponent C() {\n  state s = live(1)\n  <p>{s}</p>\n}");
    assert!(!js.contains("W.live("), "the function was taken for the builtin");
}

#[test]
fn live_needs_a_name_to_follow() {
    assert_eq!(refused("component C() {\n  state b = live(() => 1)\n  <p>{b}</p>\n}"), "case.qwe:2: <C> (compiled for the browser): `live` needs a server fn by name: state items = live(load_items, ...)");
}

#[test]
fn a_component_may_end_with_its_live_state() {
    assert!(bundle("server fn load() = 1\ncomponent C() {\n  state a = live(load)\n}").contains("return null"));
}

#[test]
fn a_method_call_of_something_that_waits_is_refused_in_markup() {
    assert_eq!(
        refused("server fn save(x) = x\ncomponent C() = <p>{1.save()}</p>"),
        "case.qwe:2: <C> (compiled for the browser): `save` waits on the server, so a component can only call it from an event handler (@click={() => { ... }}) or to set state: state x = save(...)"
    );
}

#[test]
fn a_method_that_cannot_run_in_the_browser_is_no_function_there() {
    let it = loaded("fn bad(x) = read_file(x)\nfn __t() = \"x\".bad()\n");
    let js = client::test_script(&it, "__t").unwrap();
    if !node_available() {
        return;
    }
    let out = Command::new("node").arg("-e").arg(&js).output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert_eq!(said.trim(), "ERROR: str has no method `bad`");
}

#[test]
fn a_prop_named_like_a_library_hides_it() {
    let js = bundle("use Chart from \"/assets/chart.js\"\ncomponent C(Chart) = <p>{Chart.x}</p>");
    assert!(!js.contains("js_value("), "the prop was taken for the library");
}

#[test]
fn a_pattern_default_cannot_wait() {
    assert_eq!(
        refused("server fn s() = 1\ncomponent C() {\n  state n = 0\n  <button @click={() => { n = match { b: 1 } {\n    { a = s() } => a\n    _ => 0\n  } }}>{n}</button>\n}"),
        "case.qwe:5: <C> (compiled for the browser): a pattern default cannot wait on the server"
    );
}

#[test]
fn a_handler_with_no_function_is_true() {
    // `quillo check` refuses it; the compiler does not go wrong on it.
    let js = bundle("component C() {\n  <button @click>x</button>\n}");
    assert!(js.rfind("\"click\", 0]").is_some(), "no handler");
    assert!(js.contains("[true]"), "its value is not `true`");
}

#[test]
fn an_error_in_a_used_file_names_it() {
    let d = support::temp_dir("client-used");
    std::fs::write(d.join("lib.qwe"), "\n\nfn f() = read_file(\"x\")\n").unwrap();
    let src = "use \"lib.qwe\"\ncomponent C() = <p>{f()}</p>\n";
    let e = Interp::new(d.to_str().unwrap()).load_source(src, "main.qwe").err().expect("it loaded").to_string();
    assert!(e.ends_with("lib.qwe:3: `f` (compiled for the browser): `read_file` is not available in the browser; call it from a `server fn`"), "{}", e);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn every_library_and_its_links_for_the_whole_program() {
    let it = loaded(
        "use B with preload from \"https://cdn.test/b.js\"\nuse A, A2 with preconnect from \"https://other.test/a.js\"\nuse M with module, preload from \"https://cdn.test/m.js\"\nuse N with module, preconnect from \"https://mod.test/n.js\"\nuse C from \"/assets/c.js\"\n",
    );
    assert_eq!(client::all_libs(&it), ["https://cdn.test/b.js", "https://other.test/a.js", "/assets/c.js"]);
    assert_eq!(
        client::all_hints(&it),
        "<link rel=\"preload\" as=\"script\" href=\"https://cdn.test/b.js\"><link rel=\"preconnect\" href=\"https://other.test\"><link rel=\"modulepreload\" href=\"https://cdn.test/m.js\"><link rel=\"preconnect\" href=\"https://mod.test\" crossorigin>"
    );
}

#[test]
fn a_page_takes_the_runtime_from_its_programs_file() {
    let it = loaded("component A() {\n  state n = 0\n  <button @click={() => { n += 1 }}>{n}</button>\n}\ncomponent B() = <p>b</p>\n");
    let rt = client::runtime_file(&it).unwrap().expect("a runtime file");
    assert!(rt.js.contains("window.__quillo_rt"), "no names handed on");
    assert_eq!(rt.hash, client::hash_text(&rt.js));
    let page = client::bundle_for(&it, &["B".to_string()], Some(&rt)).unwrap();
    assert!(page.split);
    assert_eq!(page.set, ["B"]);
    assert!(page.js.contains("window.__quillo_rt"));
    assert!(page.js.len() < rt.js.len());
    let (whole, also) = client::bundle_and_runtime(&it).unwrap();
    assert_eq!(also.expect("a runtime file").hash, rt.hash);
    assert!(whole.len() > page.js.len());
}

#[test]
fn a_page_with_no_runtime_file_carries_its_own() {
    let it = loaded("component A() = <p>a</p>\n");
    let page = client::bundle_for(&it, &["A".to_string()], None).unwrap();
    assert!(!page.split);
    assert!(!page.js.contains("window.__quillo_rt"));
    assert!(page.js.contains("boot"), "the bundle does not start the page");
}

#[test]
fn a_page_whose_text_seems_to_assign_a_runtime_name_carries_its_own() {
    let it = loaded("component A() = <p>W = 1</p>\n");
    let rt = client::runtime_file(&it).unwrap().unwrap();
    let page = client::bundle_for(&it, &["A".to_string()], Some(&rt)).unwrap();
    assert!(!page.split);
}

#[test]
fn a_pipe_into_a_server_fn_compiles_to_a_call() {
    let js = client::bundle(&loaded("server fn save(x) = x\ncomponent C() = <button @click={() => { 1 | save }}>go</button>")).unwrap();
    assert!(js.contains("W.call("), "the call is gone");
}

#[test]
fn a_pipe_into_a_server_fn_in_markup_is_refused() {
    assert_eq!(
        refused("server fn save(x) = x\ncomponent C() = <p>{1 | save}</p>"),
        "case.qwe:2: <C> (compiled for the browser): `save` waits on the server, so a component can only call it from an event handler (@click={() => { ... }}) or to set state: state x = save(...)"
    );
}

#[test]
fn a_page_needing_more_than_the_file_carries_its_own_runtime() {
    let small = loaded("component A() = <p>a</p>\n");
    let rt = client::runtime_file(&small).unwrap().unwrap();
    let big = loaded("component A() {\n  state n = 0\n  <button @click={() => { n = md(\"x\") }}>{n}</button>\n}\nfn md(s) = markdown(s) + highlight(s, \"rust\")\n");
    let page = client::bundle_for(&big, &["A".to_string()], Some(&rt)).unwrap();
    assert!(!page.split);
    assert!(!page.js.contains("window.__quillo_rt"));
}

#[test]
fn the_kept_page_lists_forget_their_roots_past_a_limit() {
    let page = |set: &str| client::PageBundle { js: format!("// {}", set), set: vec![set.to_string()], split: false, styles: vec![], libs: vec![], hints: String::new() };
    let mut b = Bundles::default();
    let first = b.keep("r0".to_string(), page("A"));
    assert_eq!(b.len(), 1);
    assert_eq!(b.by_hash(&first.hash).unwrap().as_str(), "// A");
    assert_eq!(b.hash_for_roots("r0").unwrap().hash, first.hash);
    assert!(b.hash_for_roots("nobody").is_none());
    for i in 1..=(4 * client::KEPT + 1) {
        assert_eq!(b.keep(format!("r{}", i), page("A")).hash, first.hash);
    }
    assert_eq!(b.len(), 1);
    assert!(b.hash_for_roots("r0").is_none(), "the roots were not forgotten");
    assert!(b.hash_for_roots(&format!("r{}", 4 * client::KEPT + 1)).is_some());
}

#[test]
fn a_float_too_large_for_a_literal_is_the_same_in_the_browser() {
    same_is("", "1e999", "inf");
}

#[test]
fn a_float_too_large_for_a_literal_compiles() {
    assert!(client::test_script(&loaded("fn f() = 1e999\n"), "f").unwrap().contains("Infinity"));
}

#[test]
fn a_plugins_function_stays_on_the_server() {
    let d = support::temp_dir("client-plugin");
    std::fs::write(d.join("kv.qwer"), "pub fn fetch_one(key: &str) -> String { key.to_string() }\n").unwrap();
    let app = d.join("main.qwe");
    std::fs::write(&app, "use \"kv.qwer\"\ncomponent C() = <p>{fetch_one(\"a\")}</p>\nroute GET / = <C/>\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(&app)
        .args(["--port", &support::free_port().to_string()])
        .env("QUILLO_NATIVE_CACHE", std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-cache"))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(said.contains("`fetch_one` is Rust (kv.qwer), which runs on the server, not in <C>; call it from a `server fn`"), "{}", said);
    let _ = std::fs::remove_dir_all(&d);
}
