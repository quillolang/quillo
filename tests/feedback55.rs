//! FEEDBACK55.md: the sweep of edge cases (branch feedback55). Each test makes the case itself,
//! and the edges either side of it.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("feedback55-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

/// What `quillo check` says of a program, and whether it passed, under a deadline.
fn check(src: &str) -> (bool, String) {
    let d = dir("check", src);
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("check")
        .arg(d.join("main.qwe"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("the check did not end");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let o = child.wait_with_output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr))
}

/// `s` for a URL's query.
fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

/// The start tag of the element with `id` in `html`.
fn tag_of<'a>(html: &'a str, id: &str) -> &'a str {
    let at = html.find(&format!("id=\"{}\"", id)).unwrap_or_else(|| panic!("no #{} in {}", id, html));
    let start = html[..at].rfind('<').unwrap();
    &html[start..start + html[start..].find('>').unwrap() + 1]
}

fn chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium", "/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// `url` in headless Chrome, with `script` run in the page: what it stored in `data-result`.
fn in_chrome(chrome: &Path, url: &str, script: &str) -> serde_json::Value {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let d = support::temp_dir("feedback55-chrome");
    std::fs::write(d.join("t.js"), script).unwrap();
    let out = Command::new("node").arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(d.join("profile")).arg("30000").arg(d.join("t.js")).output().expect("node");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

/// Waits in the page for the runtime to have started.
const BOOTED: &str = "const tick = (ms) => new Promise((r) => setTimeout(r, ms));\n  for (let i = 0; i < 300 && !(window[Symbol.for(\"quillo\")] || {}).booted; i++) await tick(30);\n  const QW = window[Symbol.for(\"quillo\")];";

// Names a spread must not write, in the cases a visitor may send them, and what stays.
const REFUSED: &[&str] = &["data-qi", "data-qd", "data-qt", "data-c", "data-p", "data-s", "data-l", "data-v", "data-e", "data-q-0a1b2c3d", "data-quillo-devtools"];
const SPREAD: &str = r#"{"data-qi": "", "DATA-QI": true, "Data-C": "Card", "data-p": {"children": "<img src=x onerror=window.__pwn=1>"}, "data-s": "{\"open\": true}", "DATA-QD": 0, "data-qt": "0", "DATA-QT": 1, "Data-E": true, "data-L": [1, 2], "data-v": 1.5, "data-q-0a1b2c3d": true, "data-quillo-devtools": "1", "data-qi ": "x", "data-ok": "yes", "data-q": "q", "data-qix": "kept", "data-quality": "high", "data-cp": "x", "data-n": 5, "data-b": false, "data-list": [1, 2], "aria-label": "ünï", "title": ""}"#;

fn spread_kept_only_its_own(tag: &str) {
    let lower = tag.to_ascii_lowercase();
    for n in REFUSED {
        assert!(!lower.contains(&format!(" {}=", n)) && !lower.contains(&format!(" {}>", n)) && !lower.contains(&format!(" {} ", n)), "{} in {}", n, tag);
    }
    for kept in [r#"data-ok="yes""#, r#"data-q="q""#, r#"data-qix="kept""#, r#"data-quality="high""#, r#"data-cp="x""#, r#"data-n="5""#, r#"data-b="false""#, r#"data-list="[1,2]""#, r#"aria-label="ünï""#, r#"title="""#] {
        assert!(tag.contains(kept), "{} not in {}", kept, tag);
    }
}

// Q-700: a spread of visitor JSON wrote `data-qi`, `data-c`, `data-p` and `data-s`: the element
// became a component the visitor chose, and its `children` ran as HTML. Now a spread writes none
// of the names the runtime reads to take an element up, whatever their case or value, on the
// server and in the browser, and every other `data-` name as before.
#[test]
fn a_spread_writes_no_attribute_the_runtime_reads() {
    let d = dir(
        "spread",
        r##"component Card(children) {
    state open = false
    <div.card><button @click={() => { open = !open }}>toggle</button>{open ? children : "closed"}</div>
}
component Profile(m: str) {
    state attrs = {}
    <div><button #load @click={() => { attrs = json.parse(m) ?? {} }}>load</button><p #mine {...attrs}>mine</p><p #theirs {...(json.parse(m) ?? {})}>theirs</p></div>
}
route GET / = <html><head><title>t</title></head><body><Card>hello</Card><div #victim {...(json.parse(request.query.m ?? "{}") ?? {})}>profile</div></body></html>
route GET /in = <html><head><title>t</title></head><body><Card>hello</Card><Profile m={request.query.m ?? "{}"}/></body></html>
"##,
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let b = support::send(s.port, "GET", &format!("/?m={}", enc(SPREAD)), &[], "").body;
    spread_kept_only_its_own(tag_of(&b, "victim"));
    // In a component, as the server makes it.
    let b = support::send(s.port, "GET", &format!("/in?m={}", enc(SPREAD)), &[], "").body;
    spread_kept_only_its_own(tag_of(&b, "theirs"));
    // One key, alone, and an empty map.
    let b = support::send(s.port, "GET", &format!("/?m={}", enc(r#"{"data-qi": ""}"#)), &[], "").body;
    assert_eq!(tag_of(&b, "victim"), r#"<div id="victim">"#);
    let b = support::send(s.port, "GET", "/?m=%7B%7D", &[], "").body;
    assert_eq!(tag_of(&b, "victim"), r#"<div id="victim">"#);
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    // The report's repro: the visitor's state differs from the server's, so the component would
    // be made again, its `children` put in as HTML.
    let attack = r#"{"data-qi": "", "data-c": "Card", "data-p": "{\"children\": \"<img src=x onerror=window.__pwn=1>\"}", "data-s": "{\"open\": true}"}"#;
    let script = format!(
        r#"(async () => {{
  {BOOTED}
  const v = document.getElementById("victim");
  v.dispatchEvent(new PointerEvent("pointerover", {{ bubbles: true }}));
  v.click();
  await tick(400);
  document.body.dataset.result = JSON.stringify({{ pwn: window.__pwn ?? null, raw: document.querySelectorAll("quillo-raw, img").length, attrs: [...v.attributes].map((a) => a.name), text: v.textContent, errors: window.__quilloErrors }});
}})();"#
    );
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/?m={}", s.port, enc(attack)), &script);
    assert_eq!((&r["pwn"], r["raw"].as_u64(), r["text"].as_str()), (&serde_json::Value::Null, Some(0), Some("profile")), "{}", r);
    assert_eq!(r["attrs"], serde_json::json!(["id"]), "{}", r);
    // What the browser makes of the same map, a state set by a click, and the component as it
    // is taken up: the same attributes as the server's.
    let script = format!(
        r#"(async () => {{
  {BOOTED}
  document.getElementById("load").click();
  await tick(300);
  const mine = document.getElementById("mine");
  mine.dispatchEvent(new PointerEvent("pointerover", {{ bubbles: true }}));
  mine.click();
  await tick(300);
  const attrs = (id) => [...document.getElementById(id).attributes].map((a) => a.name + "=" + a.value).join(" ");
  document.body.dataset.result = JSON.stringify({{ pwn: window.__pwn ?? null, raw: document.querySelectorAll("quillo-raw, img").length, mine: attrs("mine"), theirs: attrs("theirs"), remade: QW.remade ?? 0, errors: window.__quilloErrors }});
}})();"#
    );
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/in?m={}", s.port, enc(SPREAD)), &script);
    assert_eq!((&r["pwn"], r["raw"].as_u64(), r["remade"].as_u64()), (&serde_json::Value::Null, Some(0), Some(0)), "{}", r);
    let want = r#"data-ok=yes data-q=q data-qix=kept data-quality=high data-cp=x data-n=5 data-b=false data-list=[1,2] aria-label=ünï title="#;
    assert_eq!(r["mine"].as_str(), Some(format!("id=mine {}", want).as_str()), "{}", r);
    assert_eq!(r["theirs"].as_str(), Some(format!("id=theirs {}", want).as_str()), "{}", r);
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);
}

// Q-700, written out: `data-c={request.query.c}` on an element marked `data-qi` was the same hole.
// The marks of an island, `data-qi`, `data-qd`, `data-qt` and `<quillo-island>`, are the server's
// alone, in any case; the other names mean nothing without them and stay the program's. But on a
// component whose root is its own island (a table's row), the island's attributes are the
// server's: the markup's own are left out on the server, when the browser makes it, and when it
// updates it.
#[test]
fn quillos_island_marks_are_the_servers_alone() {
    for (src, said) in [
        ("route GET / = <div data-qi data-c=\"Card\">x</div>\n", "`data-qi` in <div> is Quillo's own attribute"),
        ("route GET / = <p DATA-QD={request.query.x}>x</p>\n", "`DATA-QD` in <p> is Quillo's own attribute"),
        ("route GET / = <tr Data-Qi=\"\"><td>x</td></tr>\n", "`Data-Qi` in <tr> is Quillo's own attribute"),
        ("route GET / = <template data-qt=\"0\"><p>x</p></template>\n", "`data-qt` in <template> is Quillo's own attribute"),
        ("route GET / = <div DATA-QT>x</div>\n", "`DATA-QT` in <div> is Quillo's own attribute"),
        ("component Box(x: str) {\n    <input bind:data-qi={x}/>\n}\nroute GET / = <Box x=\"1\"/>\n", "`bind:data-qi` in <input>"),
        ("route GET / = <quillo-island data-c=\"Card\">x</quillo-island>\n", "<quillo-island> is Quillo's own element"),
        ("route GET / = <svg><quillo-island/></svg>\n", "<quillo-island> is Quillo's own element"),
        ("route GET / = <Quillo-Island>x</Quillo-Island>\n", "<Quillo-Island> is Quillo's own element"),
    ] {
        let (ok, out) = check(src);
        assert!(!ok && out.contains(said), "{}: {}", src, out);
    }
    let src = r##"component Row(c: str) {
    state n = 1
    state cc = c
    <tr data-c={cc} data-p="{}" data-s="{&quot;n&quot;:5}" data-e data-l="[1]" data-v="1:2" data-x={cc} class="row"><td #cell @click={() => { n += 1; cc = "Other" }}>{n}</td></tr>
}
component Grid() {
    state shown = false
    <div><button #show @click={() => { shown = true }}>show</button><table><tbody #made>{shown ? <Row c="Made"/> : null}</tbody></table></div>
}
route GET / = <html><head><title>t</title></head><body><table><tbody><Row c={request.query.c ?? "Row"}/></tbody></table><Grid/><div #own data-c={request.query.c ?? "Row"} data-p="{}" data-s="{}" data-q="1" data-qix="2" data-quillo-devtools="1" data-e>own</div></body></html>
"##;
    let (ok, out) = check(src);
    assert!(ok, "{}", out);
    let d = dir("marks", src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let b = support::send(s.port, "GET", "/?c=Card", &[], "").body;
    // The island's attributes once each, the server's; the row's others as written.
    let row = r#"<tr data-qi data-c="Row" data-p="{&quot;c&quot;:&quot;Card&quot;}" data-s="{&quot;n&quot;:1,&quot;cc&quot;:&quot;Card&quot;}" class="row" data-x="Card">"#;
    assert!(b.contains(row), "{}", b);
    // Elsewhere they are the program's.
    assert_eq!(tag_of(&b, "own"), r#"<div id="own" data-c="Card" data-p="{}" data-s="{}" data-q="1" data-qix="2" data-quillo-devtools="1" data-e="true">"#);
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    // The row is a component, and the div is not one. The row's update changes its own `data-x`
    // and not the island's `data-c`; the row the browser makes has the island's mark, its name,
    // and its own attributes.
    let script = format!(
        r##"(async () => {{
  {BOOTED}
  const attrs = (el) => [...el.attributes].map((a) => a.name + "=" + a.value).join(" ");
  const tr = document.querySelector("tr");
  document.getElementById("own").click();
  document.getElementById("cell").click();
  await tick(300);
  document.getElementById("show").click();
  await tick(300);
  const made = document.querySelector("#made tr");
  document.body.dataset.result = JSON.stringify({{ cell: document.getElementById("cell").textContent, own: document.getElementById("own").textContent, row: attrs(tr), made: made && attrs(made), remade: QW.remade ?? 0, errors: window.__quilloErrors }});
}})();"##
    );
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/?c=Card", s.port), &script);
    assert_eq!((r["cell"].as_str(), r["own"].as_str(), r["remade"].as_u64()), (Some("2"), Some("own"), Some(0)), "{}", r);
    assert_eq!(r["row"], r#"data-qi= data-c=Row data-p={"c":"Card"} data-s={"n":1,"cc":"Card"} class=row data-x=Other"#, "{}", r);
    assert_eq!(r["made"], "class=row data-x=Made data-qi= data-c=Row", "{}", r);
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);
}

// The server's half of the above, on the root element's text as the server writes it: each of the
// island's names out, in any case and form, and every other attribute as it was.
#[test]
fn an_own_root_keeps_only_its_own_attributes() {
    let strip = |rest: &str| {
        let mut out = String::new();
        quillo::island::root_without_island_attrs(rest, &mut out);
        out
    };
    for (rest, want) in [
        ("><td>a</td></tr>", "><td>a</td></tr>"),
        (" class=\"row\"><td>a</td></tr>", " class=\"row\"><td>a</td></tr>"),
        (" data-c=\"Card\" class=\"row\">x</tr>", " class=\"row\">x</tr>"),
        (" DATA-C=\"Card\" Data-P=\"{}\" data-S=\"{&quot;a&quot;:1}\">x", ">x"),
        (" data-e data-l=\"[1]\" data-v=\"1:2\" data-qd=\"0\" data-qt=\"1\" data-qi>x", ">x"),
        (" data-e=\"true\"\n\tdata-x=\"1\">x", "\n\tdata-x=\"1\">x"),
        // Names like them, and values that name them, stay; so does the text after the tag.
        (" data-cx=\"1\" data-q-0a1b2c3d data-quillo-devtools=\"1\" title=\"data-c=x\">data-c=\"y\"", " data-cx=\"1\" data-q-0a1b2c3d data-quillo-devtools=\"1\" title=\"data-c=x\">data-c=\"y\""),
        (" title=\"ü&gt;\" data-c=\"ü\" lang=\"ünï\">ü", " title=\"ü&gt;\" lang=\"ünï\">ü"),
        (" data-x=a data-c=b>", " data-x=a>"),
        (" data-c=\"never closed", ""),
        ("", ""),
    ] {
        assert_eq!(strip(rest), want, "{:?}", rest);
    }
}

// Q-707: a `css` template in `<svg><style>` (or `<math><style>`) could add rules, as the browser
// decodes character references there and `&#125` needs no `;`. A `${}` value's `&` is CSS's
// `\26 ` now, the same character to CSS; the template's own text, nesting's `&`, stays as
// written, and an HTML `<style>` reads as it did.
#[test]
fn a_css_value_adds_no_rule_in_svg_or_math() {
    let d = dir(
        "css-amp",
        r##"component Sheet(q: str) {
    state c = q
    <div>
        <svg width="10" height="10"><style>{css`circle { fill: ${c} } #inb::after { content: "${c}" }`}</style><circle r="5"/></svg>
        <p #inb>b</p>
        <button #go @click={() => { c = "blue&#125 #inb&#123color:rgb(7,8,9)&#125" }}>go</button>
    </div>
}
route GET /svg = <html><head><title>t</title></head><body><p #p>hi</p><svg width="10" height="10"><style>{css`circle { fill: ${request.query.q ?? "red"} }`}</style><circle r="5"/></svg></body></html>
route GET /math = <html><head><title>t</title></head><body><p #p>hi</p><math><style>{css`mi { color: ${request.query.q ?? "red"} }`}</style><mi>x</mi></math></body></html>
route GET /html = <html><head><title>t</title><style>{css`b { color: ${request.query.q ?? "red"} }`}</style></head><body><p #p>hi</p></body></html>
route GET /amp = <html><head><title>t</title><style>{css`#h::after { content: "${request.query.q ?? ""}" } #h { & b { color: rgb(4, 5, 6) } }`}</style></head><body><p #h><b>b</b></p><svg width="10" height="10"><style>{css`#s::after { content: "${request.query.q ?? ""}" } #s { & b { color: rgb(4, 5, 6) } }`}</style></svg><p #s><b>b</b></p></body></html>
route GET /comp = <html><head><title>t</title></head><body><Sheet q={request.query.q ?? "red"}/></body></html>
"##,
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| support::send(s.port, "GET", p, &[], "").body;
    let attack = enc("red&#125 #p&#123color:rgb(1,2,3)&#125");
    let b = get(&format!("/svg?q={attack}"));
    assert!(b.contains(r"<style>circle { fill: red\26 #125 #p\26 #123color:rgb(1,2,3)\26 #125 }</style>"), "{}", b);
    let b = get(&format!("/math?q={attack}"));
    assert!(b.contains(r"<style>mi { color: red\26 #125 #p\26 #123color:rgb(1,2,3)\26 #125 }</style>"), "{}", b);
    // With its `;` too, which was escaped before; and in HTML, the same.
    let b = get(&format!("/svg?q={}", enc("red&#125; p&#123;color:red&#125;")));
    assert!(b.contains(r"fill: red\26 #125\3b  p\26 #123\3b color:red\26 #125\3b  }"), "{}", b);
    let b = get(&format!("/html?q={attack}"));
    assert!(b.contains(r"<style>b { color: red\26 #125 #p\26 #123color:rgb(1,2,3)\26 #125 }</style>"), "{}", b);
    // One `&`, two, alone, at either end, with other text, and none: only the value's `&` changes.
    for (q, want) in [("A%26B", r"A\26 B"), ("%26", r"\26 "), ("%26%26", r"\26 \26 "), ("%26x", r"\26 x"), ("x%26", r"x\26 "), ("%C3%BC%26%E2%9C%93", r"ü\26 ✓"), ("", ""), ("AB", "AB")] {
        let b = get(&format!("/amp?q={q}"));
        assert!(b.contains(&format!("<style>#h::after {{ content: \"{want}\" }} #h {{ & b {{ color: rgb(4, 5, 6) }} }}</style>")), "{}: {}", q, b);
        assert!(b.contains(&format!("<style>#s::after {{ content: \"{want}\" }} #s {{ & b {{ color: rgb(4, 5, 6) }} }}</style>")), "{}: {}", q, b);
    }
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    let script = r#"(async () => {
  await new Promise((r) => setTimeout(r, 200));
  document.body.dataset.result = JSON.stringify({ color: getComputedStyle(document.getElementById("p")).color });
})();"#;
    for p in ["svg", "math", "html"] {
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/{}?q={}", s.port, p, attack), script);
        assert_eq!(r["color"], "rgb(0, 0, 0)", "{}: {}", p, r);
    }
    // A value's `&` is still `&` to CSS, in a string, and the template's nesting `&` still nests.
    let script = r#"(async () => {
  await new Promise((r) => setTimeout(r, 200));
  const h = document.getElementById("h"), s = document.getElementById("s");
  document.body.dataset.result = JSON.stringify({ h: getComputedStyle(h, "::after").content, s: getComputedStyle(s, "::after").content, hb: getComputedStyle(h.querySelector("b")).color, sb: getComputedStyle(s.querySelector("b")).color });
})();"#;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/amp?q=A%26B", s.port), script);
    assert_eq!(r, serde_json::json!({ "h": "\"A&B\"", "s": "\"A&B\"", "hb": "rgb(4, 5, 6)", "sb": "rgb(4, 5, 6)" }));
    // The browser writes the sheet as the server does (taken up, not made again), and a state's
    // `&` after a click adds no rule either.
    let script = format!(
        r#"(async () => {{
  {BOOTED}
  const before = document.querySelector("svg style").textContent;
  document.getElementById("go").click();
  await tick(300);
  const b = document.getElementById("inb");
  document.body.dataset.result = JSON.stringify({{ remade: QW.remade ?? 0, before, after: document.querySelector("svg style").textContent, color: getComputedStyle(b).color, content: getComputedStyle(b, "::after").content, errors: window.__quilloErrors }});
}})();"#
    );
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/comp?q={}", s.port, enc("red&#125 #inb&#123color:rgb(1,2,3)&#125")), &script);
    assert_eq!(r["remade"], 0, "{}", r);
    assert_eq!(r["before"], r#"circle { fill: red\26 #125 #inb\26 #123color:rgb(1,2,3)\26 #125 } #inb::after { content: "red\26 #125 #inb\26 #123color:rgb(1,2,3)\26 #125" }"#, "{}", r);
    assert_eq!(r["after"], r#"circle { fill: blue\26 #125 #inb\26 #123color:rgb(7,8,9)\26 #125 } #inb::after { content: "blue\26 #125 #inb\26 #123color:rgb(7,8,9)\26 #125" }"#, "{}", r);
    assert_eq!((r["color"].as_str(), r["content"].as_str()), (Some("rgb(0, 0, 0)"), Some("\"blue&#125 #inb&#123color:rgb(7,8,9)&#125\"")), "{}", r);
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);
}

// Q-711: in the browser, `highlight(code, "constructor")` or `"__proto__"` found the language
// map's inherited properties, threw, and showed nothing. Each name is its own now: those, and the
// object's other names, are plain text as on the server; a language in any case still colours.
#[test]
fn highlight_knows_only_its_own_languages_in_the_browser() {
    let d = dir(
        "highlight",
        r##"let NAMES = ["constructor", "__proto__", "CONSTRUCTOR", "__Proto__", "toString", "hasOwnProperty", "valueOf", "", "nope", "PYTHON", "Js", "json", "SQL", "ﬁ", "c"]
let CODE = "let x = 1 // c\n<b> & \"y\""
component Client() {
    state shown = false
    <div id="client" @mount={(el) => { shown = true }}>{for l in (shown ? NAMES : []) => <pre class="hl">{highlight(CODE, l)}</pre>}</div>
}
route GET / = <html><head><title>t</title></head><body>
    <div id="server">{for l in NAMES => <pre class="hl">{highlight(CODE, l)}</pre>}</div>
    <Client/>
</body></html>
"##,
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let b = support::send(s.port, "GET", "/", &[], "").body;
    let plain = "<pre class=\"hl\">let x = <span class=\"hl-n\">1</span> // c\n&lt;b&gt; &amp; &quot;y&quot;</pre>";
    assert_eq!(b.matches(plain).count(), 11, "{}", b);
    assert!(b.contains("<pre class=\"hl\"><span class=\"hl-k\">let</span> x"), "{}", b);
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    let script = r##"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && document.querySelectorAll("#client pre").length === 0; i++) await tick(30);
  const html = (p) => (p.querySelector("quillo-raw") || p).innerHTML;
  const s = [...document.querySelectorAll("#server pre")].map(html), c = [...document.querySelectorAll("#client pre")].map(html);
  document.body.dataset.result = JSON.stringify({ n: c.length, differ: s.map((x, i) => (x === c[i] ? null : [i, x, c[i]])).filter(Boolean), errors: window.__quilloErrors });
})();"##;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), script);
    assert_eq!((r["n"].as_u64(), &r["differ"], &r["errors"]), (Some(15), &serde_json::json!([]), &serde_json::json!([])), "{}", r);
}

/// A component with a builtin's errors in each place they become text, and one the program
/// wrote: `json` with `json.parse`'s, whose message the two sides word differently.
fn errors_program(json: bool) -> String {
    format!(
        r##"type Rec = {{ n: int }}
component Errs(t: str, bad, mine) {{
    state v = t
    <div>
        <p #a>{{str(int(v))}}</p>
        <p #b>{{`could not read: ${{int(v)}}`}}</p>
        <p #c>{{{c}}}</p>
        <p #d>{{str(err("mine"))}}</p>
        <p #e>{{int(v)}}</p>
        <p #f>{{str(err(int(v)))}}</p>
        <p #g>{{str([int(v), {{ k: int(v) }}])}}</p>
        <p #h>{{error(int(v))}}</p>
        <p #i>{{str(Rec(json.parse("{{\"n\": \"x\"}}")))}}</p>
        <p #j>{{json.stringify([int(v), {{ k: int(v), m: err("mine") }}])}}</p>
        <p #k>{{str(bad)}}</p>
        <p #l data-x={{[int(v), err("mine")]}} data-y={{{{ k: int(v) }}}}>l</p>
        <p #m>{{json.stringify(err("mine"))}}</p>
        <p #q>{{str(mine)}}</p>
        <p #n class="c" data-x={{[int(v)]}}>n</p>
        <p #o {{...{{ "data-z": 1 }}}} data-x={{{{ k: int(v), m: err("mine") }}}}>o</p>
        <button #go @click={{() => {{ v = "xyz" }}}}>go</button>
    </div>
}}
route GET / = <html><head><title>t</title></head><body><Errs t="abc" bad={{int("abc")}} mine={{err("mine")}}/></body></html>
"##,
        c = if json { r#"str(json.parse("[1,]"))"# } else { r#"str(err("x"))"# }
    )
}

const ERRS_SCRIPT: &str = r#"(async () => {
  BOOTED
  const texts = () => [..."abcdefghijkmq"].map((id) => document.getElementById(id).textContent).concat([["l", "x"], ["l", "y"], ["n", "x"], ["o", "x"]].map(([id, a]) => document.getElementById(id).dataset[a]));
  const taken = texts();
  document.getElementById("go").click();
  await tick(300);
  document.body.dataset.result = JSON.stringify({ remade: QW.remade ?? 0, resumed: QW.resumed ?? 0, taken, after: texts(), errors: window.__quilloErrors });
})();"#;

/// The texts of the elements ERRS_SCRIPT reads, as the server's page has them.
fn server_texts(html: &str) -> Vec<String> {
    let unescape = |s: &str| s.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&");
    let mut out: Vec<String> = "abcdefghijkmq"
        .chars()
        .map(|id| {
            let t = tag_of(html, &id.to_string());
            let at = html.find(t).unwrap() + t.len();
            unescape(&html[at..at + html[at..].find("</p>").unwrap()])
        })
        .collect();
    // Attributes, on the fast path (`l`) and the one for `class=` (`n`) and spreads (`o`).
    for (id, a) in [("l", "data-x=\""), ("l", "data-y=\""), ("n", "data-x=\""), ("o", "data-x=\"")] {
        let t = tag_of(html, id);
        let at = t.find(a).unwrap() + a.len();
        out.push(unescape(&t[at..at + t[at..].find('"').unwrap()]));
    }
    out
}

// Q-773: in production the server writes a builtin's error as `err("internal error")` (Q-197),
// but the browser, when it made the component, showed the whole text, and made it again for that.
// The browser hides the same errors the server hides, in the same mode: in a template, `str`,
// markup, `json.stringify`, an attribute's JSON and a prop; the program's own `err(...)` and a
// record's are shown, and `error(r)` gives the text on both sides, as Q-197 meant. Under `--watch`
// both show all of it.
#[test]
fn a_builtins_error_reads_the_same_on_both_sides() {
    let hidden = r#"err("internal error")"#;
    let want_shown: Vec<String> = [
        hidden.to_string(),
        format!("could not read: {hidden}"),
        hidden.to_string(),
        r#"err("mine")"#.into(),
        hidden.to_string(),
        format!("err({hidden})"),
        format!("[{hidden}, {{ k: {hidden} }}]"),
        r#"not an int: "abc""#.into(),
        r#"err("Rec: `n` must be int, got str \"x\"")"#.into(),
        r#"[{"error":"internal error"},{"k":{"error":"internal error"},"m":{"error":"mine"}}]"#.into(),
        hidden.to_string(),
        r#"{"error":"mine"}"#.into(),
        r#"err("mine")"#.into(),
        r#"[{"error":"internal error"},{"error":"mine"}]"#.into(),
        r#"{"k":{"error":"internal error"}}"#.into(),
        r#"[{"error":"internal error"}]"#.into(),
        r#"{"k":{"error":"internal error"},"m":{"error":"mine"}}"#.into(),
    ]
    .to_vec();
    let d = dir("errors", &errors_program(true));
    let s = support::serve(&d.join("main.qwe"), &[]);
    let page = support::send(s.port, "GET", "/", &[], "").body;
    let server = server_texts(&page);
    assert_eq!(server, want_shown, "{}", page);
    assert!(!page.contains("not an int: &quot;abc&quot;&quot;") && !page.contains("invalid JSON"), "{}", page);
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    let script = ERRS_SCRIPT.replace("BOOTED", BOOTED);
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), &script);
    assert_eq!((r["remade"].as_u64(), r["resumed"].as_u64()), (Some(0), Some(1)), "{}", r);
    assert_eq!(r["taken"], serde_json::json!(server), "{}", r);
    // Made in the browser after the click, with another value: hidden the same way.
    let after: Vec<String> = want_shown.iter().map(|t| t.replace("abc", "xyz")).collect();
    assert_eq!(r["after"], serde_json::json!(after), "{}", r);
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);

    // Under --watch, all of it on both sides (but json.parse's, worded otherwise in the browser).
    let d = dir("errors-watch", &errors_program(false));
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let page = support::send(s.port, "GET", "/", &[], "").body;
    let server = server_texts(&page);
    let full = r#"err("not an int: \"abc\"")"#;
    assert_eq!(server[0], full, "{}", page);
    assert_eq!(server[1], format!("could not read: {full}"), "{}", page);
    assert_eq!((server[2].as_str(), server[3].as_str()), (r#"err("x")"#, r#"err("mine")"#), "{}", page);
    assert_eq!((server[10].as_str(), server[12].as_str()), (full, r#"err("mine")"#), "{}", page);
    assert_eq!(server[13], r#"[{"error":"not an int: \"abc\""},{"error":"mine"}]"#, "{}", page);
    assert_eq!(server[15], r#"[{"error":"not an int: \"abc\""}]"#, "{}", page);
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), &script);
    assert_eq!((r["remade"].as_u64(), r["resumed"].as_u64()), (Some(0), Some(1)), "{}", r);
    assert_eq!(r["taken"], serde_json::json!(server), "{}", r);
    let after: Vec<String> = server.iter().enumerate().map(|(i, t)| if i == 10 { t.clone() } else { t.replace("abc", "xyz") }).collect();
    assert_eq!(r["after"], serde_json::json!(after), "{}", r);
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);
}

// Q-773, what reaches the page by other ways: a server fn's answer, a channel's message, and the
// page's own JavaScript. A server fn's builtin error comes hidden, its `err("mine")` whole; a
// channel message hides as the page does; an error of the page's JavaScript (a rejected promise,
// a name the page lacks) has no server side and holds nothing of the server's, and is shown.
#[test]
fn what_comes_to_the_page_hides_a_builtins_error_too() {
    let src = r##"server fn mine() = err("mine")
server fn theirs() = int("abc")
server fn deep() = [err("mine"), int("abc")]
channel.on("room", (msg, peer) => {
    channel.send("room", peer, { to: "one", v: [int("abc"), err("mine"), { k: json.parse("[1,]") }] })
    channel.broadcast("room", { to: "all", v: [int("abc"), err("mine"), err(int("x"))] })
})
component Calls(own) {
    state got = []
    state heard = []
    let room = channel.open("room", (m) => { heard = heard + [json.stringify(m)] })
    <div>
        <button #call @click={() => { got = [str(mine()), str(theirs()), str(deep()), str(js("boom", 1)), str(js("nope.x")), str(own)] }}>call</button>
        <button #send @click={() => room.send({ text: "hi" })}>send</button>
        <p #got>{join(got, " | ")}</p>
        <ul #heard>{for t in heard => <li>{t}</li>}</ul>
    </div>
}
route GET / = <html><head><title>t</title></head><body><Calls own={err("mine")}/></body></html>
"##;
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let script = format!(
        r##"(async () => {{
  {BOOTED}
  window.boom = (n) => Promise.reject(new Error("boom " + n));
  document.getElementById("call").click();
  for (let i = 0; i < 100 && !document.getElementById("got").textContent; i++) await tick(50);
  await tick(300);
  document.getElementById("send").click();
  for (let i = 0; i < 100 && document.querySelectorAll("#heard li").length < 2; i++) await tick(50);
  document.body.dataset.result = JSON.stringify({{ got: document.getElementById("got").textContent, heard: [...document.querySelectorAll("#heard li")].map((l) => l.textContent).sort(), remade: QW.remade ?? 0, errors: window.__quilloErrors }});
}})();"##
    );
    let d = dir("comes", src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), &script);
    assert_eq!(
        r["got"],
        r#"err("mine") | err("internal error") | [err("mine"), err("internal error")] | err("js: boom 1") | err("js: nope.x is not defined on this page") | err("mine")"#,
        "{}",
        r
    );
    assert_eq!(
        r["heard"],
        serde_json::json!([
            r#"{"to":"all","v":[{"error":"internal error"},{"error":"mine"},{"error":{"error":"internal error"}}]}"#,
            r#"{"to":"one","v":[{"error":"internal error"},{"error":"mine"},{"k":{"error":"internal error"}}]}"#
        ]),
        "{}",
        r
    );
    assert_eq!((r["remade"].as_u64(), &r["errors"]), (Some(0), &serde_json::json!([])), "{}", r);
    // Under --watch, whole.
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), &script);
    assert!(r["got"].as_str().unwrap().starts_with(r#"err("mine") | err("not an int: \"abc\"") | [err("mine"), err("not an int: \"abc\"")] | "#), "{}", r);
    let heard = r["heard"].to_string();
    assert!(heard.contains(r#"not an int: \\\"abc\\\""#) && heard.contains("invalid JSON") && !heard.contains("internal error"), "{}", r);
}

// Q-773 in a script, which has no visitors: an attribute's JSON shows the error whole, as its
// text and `json.stringify` do, and nothing is logged as hidden. On both paths of attributes.
#[test]
fn a_script_shows_its_errors_in_attributes_too() {
    let d = dir(
        "script-attrs",
        r#"let x = int("abc")
print(<p data-x={[x, err("mine")]} class="c" data-y={{ k: x }}>{str(x)}</p>)
print(<i data-x={[x]}>i</i>)
print(json.stringify([x]))
"#,
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("the script did not end");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let o = child.wait_with_output().unwrap();
    let (out, log) = (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned());
    let e = "{&quot;error&quot;:&quot;not an int: \\&quot;abc\\&quot;&quot;}";
    assert_eq!(
        out,
        format!(
            "<p class=\"c\" data-x=\"[{e},{{&quot;error&quot;:&quot;mine&quot;}}]\" data-y=\"{{&quot;k&quot;:{e}}}\">err(&quot;not an int: \\&quot;abc\\&quot;&quot;)</p>\n<i data-x=\"[{e}]\">i</i>\n[{{\"error\":\"not an int: \\\"abc\\\"\"}}]\n"
        )
    );
    assert!(!log.contains("not an int") && !out.contains("internal error"), "{}", log);
}

// Q-773 for the tests' own harness (`client::test_script`): the browser's half shows errors as
// the interpreter beside it does, whole, and hidden once a server hides them.
#[test]
fn the_browser_half_of_a_test_shows_errors_as_the_interpreter() {
    if Command::new("node").arg("--version").output().map(|o| !o.status.success()).unwrap_or(true) {
        return;
    }
    let both = || {
        let mut it = quillo::Interp::new(".");
        it.load_source("fn __t() = [int(\"abc\"), err(\"mine\"), err(int(\"x\")), json.stringify({ k: int(\"y\") })]\n", "case.qwe").unwrap();
        let server = match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
            Ok(v) => v.repr(),
            _ => panic!("the case failed"),
        };
        let js = quillo::client::test_script(&it, "__t").unwrap();
        let out = support::node(&js).expect("node");
        (server, String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let (s, b) = both();
    assert_eq!(s, r#"[err("not an int: \"abc\""), err("mine"), err(err("not an int: \"x\"")), "{\"k\":{\"error\":\"not an int: \\\"y\\\"\"}}"]"#);
    assert_eq!(b, s);
    quillo::value::HIDING.store(true, std::sync::atomic::Ordering::Relaxed);
    let (s, b) = both();
    quillo::value::HIDING.store(false, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(s, r#"[err("internal error"), err("mine"), err(err("internal error")), "{\"k\":{\"error\":\"internal error\"}}"]"#);
    assert_eq!(b, s);
}

// Q-773 under the debugger without `--watch`: the server hides a builtin's errors, and the
// development bundle, made on a thread of its own as the program loads, says so to the browser
// (it was made before the server had said, and showed them).
#[test]
fn under_the_debugger_the_bundle_hides_as_the_server() {
    use std::io::Write;
    let d = dir("dap", "component Errs() {\n    state v = \"abc\"\n    <p #a>{str(int(v))}</p>\n}\nroute GET / = <html><head><title>t</title></head><body><Errs/></body></html>\n");
    let port = support::free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("dap")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || std::io::copy(&mut stdout, &mut std::io::sink()));
    let mut stdin = child.stdin.take().unwrap();
    let guard = support::Guard::new(child);
    for (seq, (command, args)) in [
        ("initialize", serde_json::json!({ "adapterID": "quillo", "linesStartAt1": true })),
        ("launch", serde_json::json!({ "program": d.join("main.qwe"), "args": ["--port", port.to_string()], "cwd": d })),
        ("configurationDone", serde_json::json!({})),
    ]
    .into_iter()
    .enumerate()
    {
        let body = serde_json::json!({ "seq": seq + 1, "type": "request", "command": command, "arguments": args }).to_string();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !support::listening(port) {
        assert!(std::time::Instant::now() < deadline, "the program did not start under the debugger");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let page = support::send(port, "GET", "/", &[], "").body;
    assert!(page.contains("<p id=\"a\">err(&quot;internal error&quot;)</p>"), "{}", page);
    let loader = support::send(port, "GET", "/_quillo/quillo.js", &[], "").body;
    let at = loader.split("const at = \"").nth(1).and_then(|r| r.split('"').next()).unwrap_or_else(|| panic!("{}", loader));
    let runtime = support::send(port, "GET", &format!("{}quillo.js", at), &[], "").body;
    assert!(runtime.contains("DEV = true;") && runtime.contains("let ERRORS_SHOWN = false;"), "{}", &runtime[..runtime.len().min(300)]);
    assert!(!runtime.contains("ERRORS_SHOWN = true"), "the bundle shows errors the server hides");
    drop(stdin);
    drop(guard);
}
