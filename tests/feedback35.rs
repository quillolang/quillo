//! What `{{` and `<script>` mean today (FEEDBACK35.md, Q-428 to Q-431; branch feedback27-35-fixes;
//! Q-427, `{{ ... }}` JavaScript, is not built): a script's text written as it is, a `{` in one
//! told what to write, `style` taking a map, a map shown as a child warned of, and a value from
//! the page in a `let` shown in markup warned of. The browser's side is in tests/browser/edges30.*.

mod support;

use std::path::PathBuf;
use std::process::Command;
use support::send;

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("feedback35-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

fn check(src: &str) -> String {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = dir(&format!("check{}", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)), src);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

#[test]
fn a_scripts_own_text_is_written_as_it_is() {
    let d = dir(
        "script",
        r#"view Shell() = <div><script>console.log("view's"); window.a = 1 < 2 && 3 > 2</script></div>
component C() {
    <div><script>console.log("comp's")</script></div>
}
route GET / = <html><head><script>console.log("page's <!-- not a comment")</script></head><body><Shell/><C/></body></html>
route GET /text = <html><body><p>"quoted" & it's</p></body></html>
"#,
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let page = send(s.port, "GET", "/", &[], "").body;
    assert!(page.contains(r#"<script>console.log("page's <\!-- not a comment")</script>"#), "{}", page);
    assert!(page.contains(r#"<script>console.log("view's"); window.a = 1 < 2 && 3 > 2</script>"#), "{}", page);
    assert!(page.contains(r#"<script>console.log("comp's")</script>"#), "{}", page);
    // Text anywhere else is escaped as before.
    assert!(send(s.port, "GET", "/text", &[], "").body.contains("<p>&quot;quoted&quot; &amp; it&#39;s</p>"));
}

#[test]
fn a_brace_in_a_script_says_what_to_write() {
    let out = check("route GET / = <html><script>const o = {a: 1}</script></html>\n");
    assert!(out.contains("a `{` in a <script> starts a Quillo value: write the code as a string, <script>{raw(CODE)}</script>"), "{}", out);
    // A value in braces is still one.
    assert!(check("let n = 3\nroute GET / = <html><script>window.n = {n}</script></html>\n").contains("ok ("));
}

#[test]
fn style_takes_a_map_of_properties() {
    let d = dir(
        "style",
        r#"route GET / = <html><body><div id="a" style={{ color: "red", font_size: "2rem", marginTop: "3px", "--gap": "4px", hidden_when: null, off: false }}>x</div><p style={"color: blue"}>y</p></body></html>
"#,
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let page = send(s.port, "GET", "/", &[], "").body;
    assert!(page.contains(r#"style="color: red; font-size: 2rem; margin-top: 3px; --gap: 4px""#), "{}", page);
    assert!(page.contains(r#"style="color: blue""#), "{}", page);
}

#[test]
fn a_map_shown_as_a_child_is_warned_of() {
    let out = check("component C() {\n    let x = 5\n    <div><p>{{ x }}</p><p>{{ a: 1 }}</p><p>{x}</p><p>{ {a: 1} == {a: 1} }</p></div>\n}\nroute GET / = <html><C/></html>\n");
    assert!(out.contains("`{{ x }}` shows the map `{ x: ... }`: to show `x`, write `{x}`"), "{}", out);
    assert!(out.contains("a map written as an element's child is shown as its text"), "{}", out);
    assert_eq!(out.matches(": warning:").count(), 2, "{}", out);
    // An attribute's map is a prop, as in JSX.
    assert!(!check("component K(o: map) { <p>{o.a}</p> }\nroute GET / = <html><K o={{ a: 1 }}/></html>\n").contains("warning"));
}

#[test]
fn a_value_from_the_page_in_a_let_shown_in_markup_is_warned_of() {
    let out = check(
        "component W() {\n    let w = js(\"window.innerWidth\")\n    state s = js(\"window.innerWidth\")\n    let f = () => js(\"window.innerWidth\")\n    let unshown = js(\"window.innerWidth\")\n    <div><p>let {w}</p><p>state {s}</p><button @click={() => { s = f() + unshown }}>x</button></div>\n}\nroute GET / = <html><W/></html>\n",
    );
    assert!(out.contains("`w` is a value from the page shown in markup"), "{}", out);
    assert_eq!(out.matches(": warning:").count(), 1, "{}", out);
}

/// Under coverage a test binary that calls nothing of the library is not run (see
/// tests/support): this one does.
#[test]
fn the_library_is_linked() {
    assert!(!quillo::version().is_empty());
}
