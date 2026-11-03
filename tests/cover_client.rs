//! What the browser compiler refuses or writes for rarer code: `live`'s options, as written,
//! and an empty template.

use quillo::Interp;

fn loads(src: &str) -> Result<String, String> {
    match Interp::new(".").load_source(src, "case.qwe") {
        Ok(v) => Ok(v.repr()),
        Err(e) => Err(format!("{} {}", e.line, e.msg)),
    }
}

fn live_with(opts: &str) -> Result<String, String> {
    loads(&format!("component C() {{\n  state items = live({}, load)\n  <p>{{len(items)}}</p>\n}}\nserver fn load() = [1]\n1", opts))
}

#[test]
fn live_s_every_is_a_positive_time_written_as_it_is() {
    let wrong = "2 <C> (compiled for the browser): `live`'s options are { every: \"1s\" } here, as written: the least time between refreshes, as 500, \"500ms\", \"2s\" or \"1m\"";
    assert_eq!(live_with("{ every: 5 }"), Ok("1".into()));
    assert_eq!(live_with("{ every: \"3m\" }"), Ok("1".into()));
    for opts in ["{ every: \"2h\" }", "{ often: \"3m\" }", "{ every: true }", "{ every: \"xs\" }"] {
        assert_eq!(live_with(opts), Err(wrong.into()), "{}", opts);
    }
}

#[test]
fn an_empty_template_in_a_component_is_an_empty_string() {
    let mut it = Interp::new(".");
    it.load_source("component C() {\n  state s = ``\n  <p>{s}</p>\n}\n", "case.qwe").unwrap();
    let js = quillo::client::bundle(&it).unwrap();
    assert!(js.contains("\"\""), "{}", js);
}

fn bundled(src: &str) -> String {
    let mut it = Interp::new(".");
    it.load_source(src, "case.qwe").unwrap_or_else(|e| panic!("{}\n--- source ---\n{}", e, src));
    quillo::client::bundle(&it).unwrap()
}

#[test]
fn a_component_reads_the_server_s_constants_as_values() {
    let js = bundled("let on = true\nlet none = null\ncomponent C() {\n  state n = 0\n  <p @click={() => { n += 1 }}>{on} {none}</p>\n}\n");
    assert!(js.contains("true") && js.contains("null"), "{}", js);
}

#[test]
fn a_component_s_function_may_return_early() {
    let js = bundled("component C() {\n  state n = 0\n  fn f() {\n    if n > 2 {\n      return\n    }\n    n += 1\n  }\n  <p @click={f}>{n}</p>\n}\n");
    assert!(js.contains("return"), "{}", js);
}

#[test]
fn a_handler_cannot_change_a_server_variable_but_may_read_one() {
    assert_eq!(
        loads("component C() {\n  state n = 0\n  <p @click={() => { m = 1 }}>{n}</p>\n}\nlet m = 0\n"),
        Err("3 <C> (compiled for the browser): `m` is a server variable that the program changes; pass it as a prop or read it with a `server fn`".into())
    );
    bundled("component C() {\n  state n = 0\n  <p @click={() => { n = other + 1 }}>{n}</p>\n}\nlet other = 0\n");
    bundled("fn helper() = http.get(\"/x\")\ncomponent C() {\n  state n = 0\n  <p @click={() => { let r = helper()\n n = 1 }}>{n}</p>\n}\n");
}
