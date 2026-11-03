//! FEEDBACK48.md: how markup is written, on the server and in the browser after an update
//! (Q-564 to Q-567, Q-571 and Q-572's newline; branch main): `{true}` and a function as a child
//! show nothing, word attributes (`aria-*`, `data-*`, `draggable`, `spellcheck`,
//! `contenteditable`, `translate`) write their bools as words, a number in a `style` map is in
//! pixels as React has it, `class` takes clsx's maps and lists, a spread's keys follow the
//! written-out rules, and a <pre> whose text starts with a newline keeps it. The parser's and
//! checker's part is in tests/feedback48.rs; the browser's side is tests/browser/f48-render.*.

mod support;

use quillo::Interp;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn run(src: &str) -> String {
    Interp::new(".").load_source(src, "t.qwe").map(|v| v.display()).unwrap_or_else(|e| format!("error: {}", e))
}

/// The markup `m` as the server writes it, after the lines `pre`.
fn page_with(pre: &str, m: &str) -> String {
    run(&format!("{}\nstr({})", pre, m))
}

fn page(m: &str) -> String {
    page_with("", m)
}

// ---- Q-564 and Q-571: what a child shows --------------------------------

#[test]
fn true_false_and_null_children_show_nothing() {
    assert_eq!(page("<p>{true}|{false}|{null}|x</p>"), "<p>|||x</p>");
    assert_eq!(page_with("let admin = true", "<p>{admin} {!admin}</p>"), "<p> </p>");
    // `a || b` with `a` true is `a`, and `a && b` with `a` false is `a`: nothing either way.
    assert_eq!(page_with("let a = true\nlet b = false", "<p>{a || \"no\"}{b && \"no\"}{b || \"yes\"}</p>"), "<p>yes</p>");
    // In a list too, beside what does show.
    assert_eq!(page("<p>{[true, \"a\", false, null, 1]}</p>"), "<p>a1</p>");
    // Text built from a bool is text.
    assert_eq!(page_with("let t = true", "<p>{`is ${t}`}</p>"), "<p>is true</p>");
    // A view that gives a bool.
    assert_eq!(page_with("view V() = true", "<p>[<V/>]</p>"), "<p>[]</p>");
}

#[test]
fn a_function_child_shows_nothing() {
    assert_eq!(page("<div>{(x) => <p>{x}</p>}</div>"), "<div></div>");
    // Among other children: a lambda, a named function, a builtin and a list holding one.
    assert_eq!(
        page_with("let f = (x) => <i>{x}</i>\nfn g() = 1", "<div>a{(x) => <p>{x}</p>}b{f}c{g}d{print}e{[f, \"z\"]}f</div>"),
        "<div>abcdezf</div>"
    );
    // Called, it shows what it gives.
    assert_eq!(page_with("let f = (x) => <i>{x}</i>", "<div>{f(1)}</div>"), "<div><i>1</i></div>");
}

// ---- Q-564: bools in attributes -----------------------------------------

#[test]
fn word_attributes_write_true_and_false() {
    let all = "aria-hidden={t} aria-expanded={f} data-on={f} data-x={t} draggable={t} spellcheck={f} contenteditable={t} translate={f} disabled={f} hidden={t} checked={f} aria-label={null}";
    let want = r#" aria-hidden="true" aria-expanded="false" data-on="false" data-x="true" draggable="true" spellcheck="false" contenteditable="true" translate="no" hidden"#;
    let vars = "let t = true\nlet f = false";
    // The plain path, one with `class=` and one with a spread (each writes attributes its way).
    assert_eq!(page_with(vars, &format!("<div {}>x</div>", all)), format!("<div{}>x</div>", want));
    assert_eq!(page_with(vars, &format!("<div class=\"c\" {}>x</div>", all)), format!("<div class=\"c\"{}>x</div>", want));
    assert_eq!(page_with(vars, &format!("<div {{...{{}}}} {}>x</div>", all)), format!("<div{}>x</div>", want));
    // Written as literals (an element made once), and the other way round.
    assert_eq!(
        page("<div aria-hidden={true} aria-expanded={false} draggable={false} spellcheck={true} translate={true} contenteditable={false} disabled={true}>x</div>"),
        r#"<div aria-hidden="true" aria-expanded="false" draggable="false" spellcheck="true" translate="yes" contenteditable="false" disabled>x</div>"#
    );
}

#[test]
fn word_attribute_edges() {
    // Written bare, a word attribute is true, as in JSX; an HTML boolean one stays bare.
    assert_eq!(page("<div aria-hidden data-open contenteditable disabled>x</div>"), r#"<div aria-hidden="true" data-open="true" contenteditable="true" disabled>x</div>"#);
    // null leaves any attribute out; the case of the name does not matter; `data-` alone and
    // `aria` are not such names.
    assert_eq!(
        page("<div aria-label={null} data-n={null} ARIA-Busy={true} Draggable={false} data-={true} aria={true}>x</div>"),
        r#"<div ARIA-Busy="true" Draggable="false" data- aria>x</div>"#
    );
    // Text and numbers are as they were.
    assert_eq!(page("<div aria-level={2} data-v=\"false\" spellcheck=\"true\">x</div>"), r#"<div aria-level="2" data-v="false" spellcheck="true">x</div>"#);
}

// ---- Q-565: numbers in a style map --------------------------------------

#[test]
fn style_numbers_are_pixels_but_unitless_ones() {
    assert_eq!(
        page("<div style={{ fontSize: 12, width: 100, opacity: 0.5, zIndex: 3, msFlex: 1, \"--gap\": 4 }}>x</div>"),
        r#"<div style="font-size: 12px; width: 100px; opacity: 0.5; z-index: 3; -ms-flex: 1; --gap: 4">x</div>"#
    );
    // 0 stays 0; negative and fractional numbers get the unit; text is as written.
    assert_eq!(
        page("<div style={{ margin: 0, top: -4, left: 1.5, padding: 0.0, width: \"12\", height: \"50%\" }}>x</div>"),
        r#"<div style="margin: 0; top: -4px; left: 1.5px; padding: 0.0; width: 12; height: 50%">x</div>"#
    );
    // React's unitless list, as camelCase or snake_case, and prefixed.
    assert_eq!(
        page("<div style={{ lineHeight: 1.2, line_height: 2, flex: 1, flexGrow: 2, flex_shrink: 0, fontWeight: 700, order: 3, zoom: 2, columnCount: 2, gridRow: 1, gridColumn: 2, orphans: 2, widows: 2, tabSize: 4, fillOpacity: 0.3, strokeOpacity: 1, strokeWidth: 2, animationIterationCount: 3, aspectRatio: 2, WebkitFlex: 1, WebkitLineClamp: 3, MozBoxFlex: 1 }}>x</div>"),
        r#"<div style="line-height: 1.2; line-height: 2; flex: 1; flex-grow: 2; flex-shrink: 0; font-weight: 700; order: 3; zoom: 2; column-count: 2; grid-row: 1; grid-column: 2; orphans: 2; widows: 2; tab-size: 4; fill-opacity: 0.3; stroke-opacity: 1; stroke-width: 2; animation-iteration-count: 3; aspect-ratio: 2; -webkit-flex: 1; -webkit-line-clamp: 3; -moz-box-flex: 1">x</div>"#
    );
    // Prefixed properties that take a length, a decimal, and what is left out.
    assert_eq!(
        page("<div style={{ WebkitBorderRadius: 4, msTransform: \"none\", width: 2.50m, height: 0.0m, gone: null, off: false }}>x</div>"),
        r#"<div style="-webkit-border-radius: 4px; -ms-transform: none; width: 2.50px; height: 0.0">x</div>"#
    );
    // An empty map writes an empty style.
    assert_eq!(page("<div style={{}}>x</div>"), r#"<div style="">x</div>"#);
}

#[test]
fn style_numbers_on_every_path() {
    let m = "{ fontSize: n, opacity: 1 }";
    let want = r#"style="font-size: 14px; opacity: 1""#;
    assert!(page_with("let n = 14", &format!("<p style={{{}}}>x</p>", m)).contains(want));
    assert!(page_with("let n = 14", &format!("<p class=\"c\" style={{{}}}>x</p>", m)).contains(want));
    assert!(page_with("let n = 14", &format!("<p {{...{{ style: {} }}}}>x</p>", m)).contains(want));
}

// ---- Q-566: class maps and lists ----------------------------------------

#[test]
fn class_takes_maps_and_lists() {
    assert_eq!(page("<p class={{ a: true, b: false }}>x</p>"), r#"<p class="a">x</p>"#);
    assert_eq!(page_with("let on = false", "<p class={{ active: on, off: !on }}>x</p>"), r#"<p class="off">x</p>"#);
    assert_eq!(page_with("let on = true", "<p class={[\"a\", on && \"b\"]}>x</p>"), r#"<p class="a b">x</p>"#);
    // Nested lists flattened; null, false, true and "" skipped; a map in a list; numbers as text;
    // a value that is truthy in Quillo (0) counts.
    assert_eq!(page("<p class={[\"a\", [\"b\", [\"c\", null]], false, true, \"\", { d: 0, e: null }, 2]}>x</p>"), r#"<p class="a b c d 2">x</p>"#);
}

#[test]
fn class_edges() {
    // An empty map, an all-false map, an empty list and a list of nothing: no class at all.
    for v in ["{}", "{ a: false, b: null }", "[]", "[null, false, \"\", []]", "true", "false", "null"] {
        assert_eq!(page(&format!("<p class={{{}}}>x</p>", v)), "<p>x</p>", "{}", v);
    }
    // With `.name` and a spread's class, all count, in that order.
    assert_eq!(page("<p.own class={{ a: true }} {...{ class: [\"s\"], title: \"t\" }}>x</p>"), r#"<p class="own a s" title="t">x</p>"#);
    // Text is as it was, spaces and all.
    assert_eq!(page("<p class=\"a  b\">x</p>"), r#"<p class="a  b">x</p>"#);
    assert_eq!(page_with("let c = \"x y\"", "<p class={c}>x</p>"), r#"<p class="x y">x</p>"#);
    // A number is its text, as before.
    assert_eq!(page("<p class={2}>x</p>"), r#"<p class="2">x</p>"#);
}

// ---- Q-567: a spread's keys ---------------------------------------------

#[test]
fn a_spread_follows_the_written_out_rules() {
    let pre = "let props = { onClick: () => 1, title: \"t\", className: \"big\", htmlFor: \"q\", \"aria-pressed\": false, style: { fontSize: 20 }, gone: null, \"@click\": () => 2 }";
    // The handlers are the browser's; className is class, htmlFor is for.
    assert_eq!(
        page_with(pre, "<label {...props}>s</label>"),
        r#"<label class="big" title="t" for="q" aria-pressed="false" style="font-size: 20px">s</label>"#
    );
    // A handler given as text stays out, as before; one given as a function too.
    assert_eq!(page("<a {...{ onclick: \"alert(1)\", onMouseEnter: print, href: \"/x\" }}>s</a>"), r#"<a href="/x">s</a>"#);
    // A spread of null, and of a map of nulls.
    assert_eq!(page("<p {...null} {...{ a: null, b: false, \"data-c\": null }}>s</p>"), "<p>s</p>");
    // Written out wins over the spread, `className` against `class` both counting.
    assert_eq!(page("<p class=\"w\" title=\"mine\" {...{ className: \"s\", title: \"theirs\" }}>s</p>"), r#"<p class="w s" title="mine">s</p>"#);
    // `for` written out wins over a spread's `htmlFor`.
    assert_eq!(page("<label for=\"w\" {...{ htmlFor: \"s\" }}>s</label>"), r#"<label for="w">s</label>"#);
}

#[test]
fn a_spread_capture_handler_is_refused() {
    let out = page("<button {...{ onClickCapture: () => 1 }}>s</button>");
    assert!(out.contains("`onClickCapture` listens before the element's own handlers"), "{}", out);
    assert!(out.contains("write `onClick` (or `@click`)"), "{}", out);
    // Not a function: text, as any other.
    assert_eq!(page("<p {...{ onClickCapture: \"x\" }}>s</p>"), "<p>s</p>");
    // React's own names that end so are events.
    assert_eq!(page("<p {...{ onGotPointerCapture: () => 1 }}>s</p>"), "<p>s</p>");
}

// ---- Q-572: a newline at the start of a <pre> --------------------------

#[test]
fn a_pre_whose_text_starts_with_a_newline_keeps_it() {
    assert_eq!(page("<pre>{\"\\nline\"}</pre>"), "<pre>\n\nline</pre>");
    assert_eq!(page("<listing>{\"\\nline\"}</listing>"), "<listing>\n\nline</listing>");
    assert_eq!(page("<textarea>{\"\\nline\"}</textarea>"), "<textarea>\n\nline</textarea>");
    assert_eq!(page("<textarea value={\"\\nv\"}></textarea>"), "<textarea>\n\nv</textarea>");
    // Without one, as it was; other elements as they were.
    assert_eq!(page("<pre>{\"line\\n\"}</pre>"), "<pre>line\n</pre>");
    assert_eq!(page("<textarea value={\"v\"}></textarea>"), "<textarea>v</textarea>");
    assert_eq!(page("<div>{\"\\nline\"}</div>"), "<div>\nline</div>");
    assert_eq!(page("<pre></pre>"), "<pre></pre>");
    // Inside an element whose tags never change, with attributes from values, and twice
    // (made once, then from what was kept).
    let pre = "let s = \"\\nx\"\nlet c = \"k\"\nview V() = <div><p>a</p><pre>{s}</pre></div>";
    assert_eq!(page_with(pre, "<div><V/><V/></div>"), "<div><div><p>a</p><pre>\n\nx</pre></div><div><p>a</p><pre>\n\nx</pre></div></div>");
    assert_eq!(page_with(pre, "<pre class={c} title={c}>{s}</pre>"), "<pre class=\"k\" title=\"k\">\n\nx</pre>");
    assert_eq!(page_with(pre, "<pre>{s}{s}</pre>"), "<pre>\n\nx\nx</pre>");
}

// ---- the page and the browser -------------------------------------------

fn chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .filter(|_| Command::new("node").args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"]).output().is_ok_and(|o| o.status.success()))
}

fn app() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/f48-render.qwe")
}

#[test]
fn a_components_page_is_written_the_same_way() {
    let s = support::serve(&app(), &[]);
    let page = support::send(s.port, "GET", "/", &[], "").body;
    for want in [
        r#"<p class="active" id="m">"#,
        r#"<p class="a b c d" id="l">"#,
        r#"<p id="e">"#,
        r#"<p id="z">"#,
        r#"style="font-size: 12px; opacity: 0.5; z-index: 0; --gap: 12; -ms-flex: 1; margin: 0; top: -12px; left: 1.5px; line-height: 1.2; flex-grow: 2""#,
        r#"aria-hidden="true" aria-expanded="false" data-on="true" draggable="true" spellcheck="false" contenteditable="true" translate="yes" aria-label="x">a</div>"#,
        r#"<div id="af" aria-hidden="true" aria-expanded="false" data-x="true">af</div>"#,
        r#"<label class="big" title="t" for="i" aria-pressed="true" style="font-size: 12px" data-n="0" id="b">0</label>"#,
        r#"<p id="k">abcdefg</p>"#,
        r#"<span id="kk">xy</span>"#,
        r#"<p id="h">kk</p>"#,
        "<pre id=\"pre\">\n\nline</pre>",
        "<textarea id=\"ta\">\n\nfirst</textarea>",
        r#"<li>1</li><li>2</li>"#,
    ] {
        assert!(page.contains(want), "{}\n{}", want, page);
    }
}

#[test]
fn the_browser_writes_markup_as_the_server_does() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let port = support::free_port();
    let _server = support::Guard::new(
        Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(app())
            .args(["--port", &port.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let profile = std::env::temp_dir().join(format!("quillo-chrome-f48r-{}-{}", std::process::id(), port));
    let out = Command::new("node")
        .arg(dir.join("run.js"))
        .arg(&chrome)
        .arg(format!("http://127.0.0.1:{}/", port))
        .arg(&profile)
        .arg("60000")
        .arg(dir.join("f48-render.js"))
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&profile);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let r: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(r["exception"], serde_json::Value::Null, "{}", r);
    let (b, a, back) = (&r["before"], &r["after"], &r["back"]);
    // Q-566: exactly the classes the map or list gives, before and after an update.
    assert_eq!(b["m"], "active", "{}", r);
    assert_eq!(a["m"], "off", "{}", r);
    assert_eq!(back["m"], "active", "{}", r);
    assert_eq!(b["l"], "a b c d", "{}", r);
    assert_eq!(a["l"], "a d e", "{}", r);
    let mut words: Vec<&str> = back["l"].as_str().unwrap().split(' ').collect();
    words.sort();
    assert_eq!(words, ["a", "b", "c", "d"], "{}", r);
    for s in [b, a, back] {
        assert_eq!(s["e"], serde_json::Value::Null, "{}", r);
        assert_eq!(s["z"], serde_json::Value::Null, "{}", r);
    }
    assert_eq!(b["mixed"], "one two", "{}", r);
    assert_eq!(a["mixed"], "two three", "{}", r);
    assert_eq!(b["sc"], "own y", "{}", r);
    assert_eq!(a["sc"], "own z", "{}", r);
    // Q-565: pixels, which the browser takes.
    assert_eq!(b["fontSize"], "12px", "{}", r);
    assert_eq!(a["fontSize"], "20px", "{}", r);
    let style = a["style"].as_str().unwrap();
    for part in ["font-size: 20px", "opacity: 0.5", "z-index: 0", "--gap: 20", "top: -20px", "left: 1.5px", "line-height: 1.2", "flex-grow: 2"] {
        assert!(style.contains(part), "{}: {}", part, r);
    }
    assert!(back["style"].as_str().unwrap().contains("z-index: 1101"), "{}", r);
    // Q-564: words, both ways.
    let words_on = serde_json::json!({ "aria-hidden": "true", "aria-expanded": "false", "data-on": "true", "draggable": "true", "spellcheck": "false", "contenteditable": "true", "translate": "yes", "hidden": null, "aria-label": "x" });
    let words_off = serde_json::json!({ "aria-hidden": "false", "aria-expanded": "true", "data-on": "false", "draggable": "false", "spellcheck": "true", "contenteditable": "false", "translate": "no", "hidden": "", "aria-label": null });
    assert_eq!(b["a"], words_on, "{}", r);
    assert_eq!(a["a"], words_off, "{}", r);
    assert_eq!(back["a"], words_on, "{}", r);
    assert_eq!(b["props"], serde_json::json!({ "draggable": true, "spellcheck": false, "translate": true, "on": "true" }), "{}", r);
    assert_eq!(a["props"], serde_json::json!({ "draggable": false, "spellcheck": true, "translate": false, "on": "false" }), "{}", r);
    let af = serde_json::json!({ "aria-hidden": "true", "aria-expanded": "false", "data-x": "true", "disabled": null });
    assert_eq!(b["af"], af, "{}", r);
    assert_eq!(a["late"], serde_json::json!({ "aria-hidden": "true", "aria-expanded": "false", "data-x": "true", "disabled": null, "style": "width: 10px; z-index: 2", "cls": "q" }), "{}", r);
    // Q-567: the spread's keys as written out.
    assert_eq!(b["b"]["class"], "big", "{}", r);
    assert_eq!(a["b"]["class"], "small", "{}", r);
    for s in [b, a] {
        for gone in ["classname", "htmlfor", "gone", "onclick"] {
            assert_eq!(s["b"][gone], serde_json::Value::Null, "{}: {}", gone, r);
        }
        assert_eq!(s["b"]["for"], "i", "{}", r);
    }
    assert_eq!(b["b"]["aria-pressed"], "true", "{}", r);
    assert_eq!(a["b"]["aria-pressed"], "false", "{}", r);
    assert_eq!(b["b"]["style"], "font-size: 12px", "{}", r);
    // Its onClick counts; one the spread no longer has does not; the written-out one wins.
    assert_eq!(r["clicked"], "1", "{}", r);
    assert_eq!(r["hpGone"], "1", "{}", r);
    assert_eq!(r["hpBack"], "101", "{}", r);
    assert_eq!(r["written"], "1101", "{}", r);
    // onDoubleClick, onChange on an input (each keystroke), onFocus and onBlur on a wrapper.
    assert_eq!(r["typed"], "hey", "{}", r);
    assert_eq!(r["log"], "dbl;focus;blur;", "{}", r);
    // Q-564 and Q-571: nothing for bools and functions, which are not called.
    for s in [b, a, back] {
        assert_eq!(s["k"], "abcdefg", "{}", r);
        assert_eq!(s["kk"], "xy", "{}", r);
        assert_eq!(s["h"], "kk", "{}", r);
        assert_eq!(s["rows"], "12", "{}", r);
    }
    assert_eq!(b["n"], "0", "{}", r);
    // Q-572: the text's own first newline, taken up and after an update.
    assert_eq!(b["pre"], "\nline", "{}", r);
    assert_eq!(b["pre2"], "\nyes", "{}", r);
    assert_eq!(a["pre2"], "\nno", "{}", r);
    assert_eq!(b["ta"], "\nfirst", "{}", r);
    assert_eq!(a["ta"], "\nsecond", "{}", r);
    assert_eq!(a["latepre"], "\nz", "{}", r);
    // A spread's `…Capture` is refused in the browser too.
    assert_eq!(r["cap"], false, "{}", r);
    let errors = r["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1, "{}", r);
    assert!(errors[0].as_str().unwrap().contains("`onClickCapture` listens before the element's own handlers"), "{}", r);
}

/// `page` of tests/browser in headless Chrome, its script `script`: what the script recorded.
fn in_chrome(chrome: &Path, page: &str, script: &str) -> serde_json::Value {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let port = support::free_port();
    let _server = support::Guard::new(
        Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(dir.join(page)).args(["--port", &port.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let profile = std::env::temp_dir().join(format!("quillo-chrome-f48v-{}-{}", std::process::id(), port));
    let out = Command::new("node").arg(dir.join("run.js")).arg(chrome).arg(format!("http://127.0.0.1:{}/", port)).arg(&profile).arg("60000").arg(dir.join(script)).output().unwrap();
    let _ = std::fs::remove_dir_all(&profile);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

// The review's findings: a spread's `onChange` took the place of `bind:value`'s and
// `bind:checked`'s handler, so neither bound; a function given to an attribute was written
// `<fn <fn>>` by the server and called by the browser as the page loaded; a spread's `CLASS` was
// the server's class and not the browser's.
#[test]
fn a_spread_beside_a_binding_a_function_attribute_and_a_spread_class() {
    // The server's side.
    let s = support::serve(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/f48-review.qwe"), &[]);
    let page = support::send(s.port, "GET", "/", &[], "").body;
    assert!(page.contains(r#"<p id="t">x</p>"#), "{}", page);
    assert!(page.contains(r#"<p class="z y" id="up">u</p>"#), "{}", page);
    let Some(chrome) = chrome() else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let r = in_chrome(&chrome, "f48-review.qwe", "f48-review.js");
    assert_eq!(r["exception"], serde_json::Value::Null, "{}", r);
    assert_eq!(r["remade"], 0, "{}", r);
    // The binding takes what is typed and ticked, and the spread's handler runs after it.
    assert_eq!(r["after"], "hey|true|11", "{}", r);
    // Not called, and nothing written.
    assert_eq!(r["n"], "0", "{}", r);
    assert_eq!(r["t"], r#"<p id="t">x</p>"#, "{}", r);
    assert_eq!(r["up"], "z y", "{}", r);
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);
}
