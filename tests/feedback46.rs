//! FEEDBACK46.md: an app on the starter (branch feedback40-43). Q-530, a `.qwer` signature's
//! type that a `use` names; Q-531, a style map on every path that writes attributes; Q-532, an
//! input whose value nothing takes.

mod support;

use std::path::PathBuf;
use std::process::Command;
use support::send;

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("feedback46-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

fn check(src: &str) -> String {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = dir(&format!("check{}", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)), src);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

// Q-531: the map was JSON on an element with `class=`, a spread, or a textarea.
#[test]
fn a_style_map_is_properties_whatever_else_the_element_has() {
    let d = dir(
        "style",
        r#"let more = { title: "t", style: { margin_top: "1px" } }
route GET / = <html><body>
    <div id="a" style={{ color: "red" }}>x</div>
    <div class="b" style={{ color: "red" }}>x</div>
    <div style={{ color: "red" }} class="d">x</div>
    <div {...more}>x</div>
    <textarea style={{ color: "red" }} value="t"></textarea>
    <span.dot style={{ left: "1px", top: "2px" }}>x</span>
    <div class="e" style={"color: blue"} data-m={{ a: 1 }}>x</div>
</body></html>
"#,
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let page = send(s.port, "GET", "/", &[], "").body;
    assert!(page.contains(r#"<div id="a" style="color: red">"#), "{}", page);
    assert!(page.contains(r#"<div class="b" style="color: red">"#), "{}", page);
    assert!(page.contains(r#"<div class="d" style="color: red">"#), "{}", page);
    assert!(page.contains(r#"style="margin-top: 1px""#), "{}", page);
    assert!(page.contains(r#"<textarea style="color: red">t</textarea>"#), "{}", page);
    assert!(page.contains(r#"style="left: 1px; top: 2px""#), "{}", page);
    // Text stays text, and a map on another attribute is still its JSON.
    assert!(page.contains(r#"<div class="e" style="color: blue" data-m="{&quot;a&quot;:1}">"#), "{}", page);
    assert!(!page.contains("{&quot;color"), "{}", page);
}

// Q-530: the glue wrote each parameter's type outside the file's module, where its `use`
// lines do not reach: `Value` and `HashMap` named by a `use` did not build.
#[test]
fn a_rust_signature_may_name_what_its_file_uses() {
    let d = dir(
        "glue",
        "use \"auth.qwer\"\nprint(role({ role: \"admin\" }), total({ a: 1, b: 2 }), count([1, \"a\", null]), names({ b: 1, a: 2 }), greet(\"ab\", 2), greet(\"c\", null), head([7, 8]))\n",
    );
    std::fs::write(
        d.join("auth.qwer"),
        r#"use serde_json::Value;
use std::collections::HashMap;

pub fn role(auth: Value) -> String {
    auth.get("role").and_then(|r| r.as_str()).unwrap_or("none").to_string()
}

pub fn total(m: HashMap<String, i64>) -> i64 {
    m.values().sum()
}

pub fn count(xs: &[Value]) -> usize {
    xs.len()
}

pub fn names(m: &HashMap<String, Value>) -> Vec<String> {
    let mut k: Vec<String> = m.keys().cloned().collect();
    k.sort();
    k
}

pub fn greet(name: &str, times: Option<i64>) -> String {
    name.repeat(times.unwrap_or(1) as usize)
}

pub fn head(xs: Vec<Value>) -> Option<Value> {
    xs.into_iter().next()
}
"#,
    )
    .unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .env("QUILLO_NATIVE_CACHE", PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-cache"))
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(out.trim(), r#"admin 3 3 ["a", "b"] abab c 7"#, "{}", String::from_utf8_lossy(&o.stderr));
    // A wrong argument is still said by its parameter's name.
    std::fs::write(d.join("main.qwe"), "use \"auth.qwer\"\nprint(total({ a: \"x\" }))\n").unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .env("QUILLO_NATIVE_CACHE", PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-cache"))
        .output()
        .unwrap();
    let all = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    assert!(all.contains("`total`: argument `m`"), "{}", all);
}

// Q-532: an input showing a parameter or a state's field is one way: typing showed one thing
// while the program kept another, and `check` said nothing.
#[test]
fn an_input_whose_value_nothing_takes_is_warned_of() {
    let out = check(
        r#"view Field(id, v) = <label>{id}<input id={id} value={v}/></label>
component A() {
    state t = ""
    <div><Field id="a" v={t}/><p>{t}</p></div>
}
component B() {
    state f = { name: "" }
    <div><input id="b" value={f.name}/><p>{f.name}</p></div>
}
component D(t, on) {
    <div><textarea value={t}></textarea><input type="checkbox" checked={on}/></div>
}
route GET / = <html><body><A/><B/><D t="x" on={true}/></body></html>
"#,
    );
    assert!(out.contains("main.qwe:1: warning: typing here changes nothing: `v` is `Field`'s parameter, which `value=` only shows: take what changes with `@input={(e) => ...}`, or add `readonly`"), "{}", out);
    assert!(out.contains("main.qwe:8: warning: typing here changes nothing: `f.name` is a field of the state `f`, which `value=` only shows; give it a state of its own, or take what changes with `@input={(e) => ...}`, or add `readonly`"), "{}", out);
    assert!(out.contains("main.qwe:11: warning: typing here changes nothing: `t` is `D`'s parameter"), "{}", out);
    assert!(out.contains("main.qwe:11: warning: ticking this changes nothing: `on` is `D`'s parameter, which `checked=` only shows: take what changes with `@change={(e) => ...}`"), "{}", out);
    assert_eq!(out.matches(": warning:").count(), 4, "{}", out);
    // Something takes it, nothing can be typed, a form sends it, or it is a state: no warning.
    let fine = check(
        r#"component OK(t, open) {
    state s = ""
    state f = { name: "" }
    <form method="post" action="/x">
        <input name="title" value={t}/>
        <input value={t} readonly/>
        <input value={t} @input={(e) => { s = e.value }}/>
        <input value={t} @keydown={(e) => { s = "k" }}/>
        <input value={s}/>
        <input type="hidden" value={t}/>
        <input type="submit" value={t}/>
        <input type={s} value={t}/>
        <input value={f.name} @input={(e) => { f = set(f, "name", e.value) }}/>
        <input value={t} disabled/>
        <details open={open}><summary>more</summary>{s}</details>
        <my-field value={t}></my-field>
    </form>
}
view Form(v) = <form method="post"><input name="q" value={v}/><textarea name="b" value={v}></textarea></form>
route GET / = <html><body><OK t="y" open={false}/><Form v="z"/></body></html>
"#,
    );
    assert!(!fine.contains("warning"), "{}", fine);
}

// The review of Q-532: a lambda's or local fn's parameters, any handler, a handler around the
// input, `readonly` where the browser ignores it, and the shapes first missed.
#[test]
fn the_warning_of_a_value_nothing_takes_knows_its_cases() {
    let fine = check(
        r#"component L(items) {
    <ul>{items.map((it) => <li><input value={it}/></li>)}</ul>
}
component A() {
    state t = ""
    let mk = (x) => <input value={x}/>
    <div>{mk(t)}</div>
}
component N(p) {
    fn row(r) = <input value={r}/>
    <div>{row(p)}</div>
}
component B(v, save) {
    <input value={v} @blur={(e) => save(e.target.value)}/>
}
component D(v, save) {
    <form @input={(e) => save(e.target.value)}><div><input value={v}/></div></form>
}
route GET / = <html><body><L items={["a"]}/><A/><N p="x"/><B v="a" save={(x) => x}/><D v="a" save={(x) => x}/></body></html>
"#,
    );
    assert!(!fine.contains(": warning:"), "{}", fine);
    let out = check(
        r#"component X(v) {
    <input type="checkbox" checked={v} readonly/>
}
component S(v) {
    <select value={v} readonly><option>a</option></select>
}
component U(user) {
    state f = { a: { b: "" } }
    <div><input value={user.name}/><input value={f.a.b}/><input value={user.nick ?? ""}/><input value={user.x} disabled={false}/><input value={user.y} disabled/></div>
}
route GET / = <html><body><X v={true}/><S v="a"/><U user={{ name: "n" }}/></body></html>
"#,
    );
    assert!(out.contains("main.qwe:2: warning: ticking this changes nothing: `v` is `X`'s parameter, which `checked=` only shows: take what changes with `@change={(e) => ...}`, or add `disabled`"), "{}", out);
    assert!(out.contains("main.qwe:5: warning: choosing here changes nothing: `v` is `S`'s parameter, which `value=` only shows: take what changes with `@input={(e) => ...}`, or add `disabled`"), "{}", out);
    assert!(out.contains("main.qwe:9: warning: typing here changes nothing: `user.name` is a field of `U`'s parameter `user`"), "{}", out);
    assert!(out.contains("`f.a.b` is a field of the state `f`"), "{}", out);
    assert!(out.contains("`user.nick` is a field of `U`'s parameter `user`"), "{}", out);
    assert!(out.contains("`user.x` is a field"), "{}", out);
    assert!(!out.contains("`user.y`"), "{}", out);
    assert_eq!(out.matches(": warning:").count(), 6, "{}", out);
}
