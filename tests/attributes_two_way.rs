//! What a person changes is two-way by itself, and `on...={...}` is a handler (branch
//! feedback40-43). The browser side is tests/browser.rs's
//! `an_input_held_to_a_state_and_nothing_else_is_two_way`.

use quillo::check::{check_source, Severity};
use std::path::Path;

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new("."))
        .diagnostics
        .iter()
        .map(|d| format!("{}{} {}", d.line, if d.severity == Severity::Warning { " warning:" } else { "" }, d.message))
        .collect()
}

#[test]
fn on_with_a_function_is_a_handler_and_on_with_text_is_html() {
    // An element's `onclick={...}` is `@click`; a view's or component's `onChange` is a parameter.
    let src = r#"view Picker(onChange) = <p>picker</p>
component C() {
    state n = 0
    <div>
        <button onclick={() => { n += 1 }} onMouseOver={() => { n += 2 }}>{n}</button>
        <Picker onChange={() => 1}/>
    </div>
}
route GET / = <html><body><C/><button onclick="alert('static')">x</button></body></html>
"#;
    assert!(diags(src).is_empty(), "{:?}", diags(src));
    let mut it = quillo::Interp::new(env!("CARGO_MANIFEST_DIR"));
    it.load_source(src, "t.qwe").unwrap();
    let html = String::from_utf8_lossy(&it.handle(&quillo::server::HttpRequest {
        method: "GET".into(),
        url: "/".into(),
        headers: vec![("Host".into(), "localhost".into())],
        body: Vec::new(),
        remote: "127.0.0.1".into(),
    }).body).into_owned();
    // The handler is the component's: not an attribute. The text is HTML's, kept as written.
    assert!(html.contains("<button>0</button>"), "{}", html);
    assert!(html.contains(r#"<button onclick="alert(&#39;static&#39;)">x</button>"#), "{}", html);
    // Both spellings for one event: one would be lost.
    assert_eq!(
        diags("component D() {\n    state n = 0\n    <button onclick={() => 1} @click={() => 2}>x</button>\n}\n"),
        ["3 attribute `@click` is written twice in <button> (`onclick={...}` is `@click`)"]
    );
    // Text made from values is still refused, as it would run as code.
    let e = quillo::Interp::new(env!("CARGO_MANIFEST_DIR")).load_source("route GET /a ? { u: str } = <div onclick={`go('${u}')`}>\"x\"</div>", "t.qwe").err().unwrap().to_string();
    assert!(e.contains("takes only text written here"), "{}", e);
    // `on` alone, and a name with other characters, are not events: still text only.
    for name in ["on", "on1x"] {
        let d = diags(&format!("component F() {{\n    state n = 0\n    <div {}={{n}}>x</div>\n}}\n", name));
        assert_eq!(d, [format!("3 `{}` in <div> takes only text written here, as the browser runs it as code", name)]);
    }
}

#[test]
fn every_thing_a_person_changes_can_be_bound() {
    let src = r#"component C() {
    state t = ""
    state b = false
    state o = false
    state v = 0.5
    <div>
        <input bind:value={t}/>
        <textarea bind:value={t}></textarea>
        <select bind:value={t}><option>a</option></select>
        <input type="checkbox" bind:checked={b} bind:indeterminate={o}/>
        <details bind:open={o}><summary>s</summary></details>
        <dialog bind:open={o}>d</dialog>
        <video bind:muted={b} bind:volume={v} bind:currentTime={v} bind:playbackRate={v}></video>
        <audio bind:volume={v}></audio>
        <my-field bind:value={t} bind:checked={b}></my-field>
    </div>
}
route GET / = <C/>
"#;
    assert!(diags(src).is_empty(), "{:?}", diags(src));
    for (bad, tag) in [("<input bind:open={o}/>", "input"), ("<details bind:value={o}></details>", "details"), ("<div bind:volume={o}></div>", "div"), ("<my-field bind:open={o}></my-field>", "my-field")] {
        let d = diags(&format!("component C() {{\n    state o = false\n    {}\n}}\n", bad));
        assert_eq!(d.len(), 1, "{}: {:?}", bad, d);
        assert!(d[0].contains(&format!("is not something a person changes in <{}>", tag)), "{:?}", d);
    }
}

/// The browser code: which attributes became bindings, by the events the element listens to.
fn compiled(src: &str) -> String {
    let mut it = quillo::Interp::new(env!("CARGO_MANIFEST_DIR"));
    it.load_source(src, "t.qwe").unwrap();
    quillo::client::bundle(&it).unwrap_or_default()
}

#[test]
fn an_attribute_held_to_a_state_and_nothing_else_is_two_way() {
    let src = r#"component C() {
    state t = ""
    state b = false
    state o = false
    state v = 0.5
    state qty = 1
    state kind = "text"
    state extra = {}
    let fixed = "x"
    <div>
        <input.a value={t}/>
        <input.b type="checkbox" checked={b} indeterminate={o}/>
        <input.c type="radio" checked={b}/>
        <input.d value={t} @input={(e) => { t = upper(e.target.value) }}/>
        <input.d2 value={t} @change={(e) => { t = e.target.value }}/>
        <input.e value={t} readonly/>
        <input.e2 value={t} readonly={false}/>
        <input.f value={fixed}/>
        <input.g type="file" value={t}/>
        <input.h type="checkbox" value={t}/>
        <input.i type={kind} value={t}/>
        <input.j {...extra} value={t}/>
        <select.k multiple value={t}><option>a</option></select>
        <input.n type="number" value={qty}/>
        <details open={o}><summary>s</summary></details>
        <dialog open={o}>d</dialog>
        <video muted={b} volume={v} currentTime={v} playbackRate={v}></video>
        <video muted={b} @volumechange={() => 1}></video>
    </div>
}
route GET / = <C/>
"#;
    let js = compiled(src);
    assert!(!js.is_empty(), "no bundle");
    let count = |s: &str| js.matches(s).count();
    // Two-way: a, e2 (`readonly={false}`), the number input (as a number), b's tick and half
    // tick, the two opens, the first video's four. One way: a radio, a handler of its own on any
    // event saying it changed, `readonly`, a `let`, a file or checkbox's value, a `type` decided
    // as it runs, a spread, a <select multiple>, the second video.
    assert_eq!(count("e.target.value)"), 2, "{}", js);
    assert_eq!(count("e.target.valueAsNumber)"), 1, "{}", js);
    assert_eq!(count("e.target.checked)"), 1, "{}", js);
    assert_eq!(count("e.target.indeterminate)"), 1, "{}", js);
    assert_eq!(count("e.target.open)"), 2, "{}", js);
    assert_eq!(count("e.target.muted)"), 1, "{}", js);
    assert_eq!(count("e.target.volume)"), 1, "{}", js);
    assert_eq!(count("e.target.currentTime)"), 1, "{}", js);
    assert_eq!(count("e.target.playbackRate)"), 1, "{}", js);
    // `muted` and `volume` listen to one event, in one handler; the second video has its own.
    assert_eq!(count("\"volumechange\""), 2, "{}", js);
}
