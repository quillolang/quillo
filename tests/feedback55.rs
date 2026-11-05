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

// ---- Q-701 to Q-705: what the program's files give away, and what it may write -----------------

/// What `quillo run` prints for a script in `d`, under a deadline.
fn run_script(d: &Path, file: &str) -> String {
    run_script_with(d, file, &[])
}

fn run_script_with(d: &Path, file: &str, args: &[&str]) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(args)
        .current_dir(d)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("the script did not end");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let o = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

/// `quillo serve folder` in `d`, until the guard is dropped, and its port.
fn serve_folder(d: &Path, folder: &str) -> (support::Guard, u16) {
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["serve", folder, "--port", &port.to_string()])
        .current_dir(d)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let g = support::Guard::new(child);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !support::listening(port) {
        assert!(std::time::Instant::now() < deadline, "quillo serve did not start");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    (g, port)
}

#[cfg(unix)]
fn link(to: &str, at: &Path) {
    std::os::unix::fs::symlink(to, at).unwrap();
}

/// Each path's status, and that no answer gives the secret away.
fn statuses(port: u16, want: &[(&str, u16)], secret: &str) {
    for (path, status) in want {
        let a = support::send(port, "GET", path, &[], "");
        assert_eq!(a.status, *status, "{}: {}", path, a.body);
        assert!(!a.body.contains(secret), "{} gave {:?} away: {}", path, secret, a.body);
    }
}

// Q-701: `files` judged the name asked for, not where it led: `settings.txt -> ../.env` served
// the settings and `notes.txt -> ../main.qwe` the program. Now what a link leads to is judged as
// a name is, however many links lead there, a second hard link to the settings or the program is
// one of them, and a link to an ordinary file in the folder still serves it. A file kept back is
// a miss, with the folder's page for one.
#[cfg(unix)]
#[test]
fn files_judges_a_link_by_where_it_leads() {
    let d = dir(
        "links",
        "route GET /s/*p = files(\"public\", p)\nroute GET /x/*p = files(\"site\", p)\nroute GET /p/*p = files(\"pub2\", p)\nroute GET /f = file(\"public/docs2/index.html\")\n",
    );
    for f in ["public/docs", "public/docs2", "public/sub", "public/.well-known", "pub2", "conf", ".git"] {
        std::fs::create_dir_all(d.join(f)).unwrap();
    }
    // A folder `--allow-path` names: an index there through a link is not served by `files`, as a
    // link there named itself is not; `file` serves it, named on purpose.
    let outside = support::temp_dir("feedback55-links-outside");
    std::fs::write(outside.join("out.html"), "outside page").unwrap();
    link(&outside.join("out.html").to_string_lossy(), &d.join("public/docs2/index.html"));
    link(&outside.join("out.html").to_string_lossy(), &d.join("public/ext.html"));
    std::fs::write(d.join("pub2/404.html"), "missing page").unwrap();
    link("../.env", &d.join("pub2/settings.txt"));
    for (f, text) in [(".env", "SECRET=1\n"), ("public/ok.txt", "ok"), ("public/sub/index.html", "sub"), ("public/page.html", "page"), ("public/docs/real.html", "real"), ("public/.well-known/acme.txt", "acme"), ("conf/app.json", "{}"), ("conf/.env", "SECRET=2\n"), (".git/config", "[core] SECRET\n"), ("lib.qwe", "fn SECRET() = 1\n"), ("Plug.RS", "// SECRET\n"), ("ünï.txt", "unicode")] {
        std::fs::write(d.join(f), text).unwrap();
    }
    let p = d.join("public");
    for (to, at) in [
        ("../.env", "settings.txt"),
        ("../main.qwe", "notes.txt"),
        ("../.ENV", "upper.txt"),
        ("../LIB.QWE", "lib.txt"),
        ("../Plug.RS", "plug.txt"),
        ("ok.txt", "alias.txt"),
        ("ok.txt", "c1.txt"),
        ("c1.txt", "c2.txt"),
        ("e2.txt", "e1.txt"),
        ("../.env", "e2.txt"),
        ("nowhere.txt", "gone.txt"),
        ("../.git", "repo"),
        ("..", "up"),
        ("../conf", "conf"),
        ("../../.env", "docs/index.html"),
        ("../.env", "about.html"),
        ("../.env", "404.html"),
        ("../ünï.txt", "ünï-link.txt"),
    ] {
        link(to, &p.join(at));
    }
    link("public", &d.join("site"));
    std::fs::hard_link(d.join(".env"), d.join("public/hard-env.txt")).unwrap();
    std::fs::hard_link(d.join("main.qwe"), d.join("public/hard-src.txt")).unwrap();
    let s = support::serve_with(&d.join("main.qwe"), &["--allow-path", &outside.to_string_lossy()]);
    statuses(
        s.port,
        &[
            // Through a link to the settings, the program, a hidden folder, or a name of Quillo's
            // in another case.
            ("/s/settings.txt", 404),
            ("/s/notes.txt", 404),
            ("/s/upper.txt", 404),
            ("/s/lib.txt", 404),
            ("/s/plug.txt", 404),
            ("/s/e1.txt", 404),
            ("/s/repo/config", 404),
            ("/s/repo", 404),
            ("/s/repo/", 404),
            ("/s/up/.env", 404),
            ("/s/up/main.qwe", 404),
            ("/s/conf/.env", 404),
            ("/s/gone.txt", 404),
            ("/s/hard-env.txt", 404),
            ("/s/hard-src.txt", 404),
            ("/s/ext.html", 404),
            ("/s/ext", 404),
            // An index, a name found with its extension and the page of a miss are judged too.
            ("/s/docs/", 404),
            ("/s/docs2/", 404),
            ("/s/about", 404),
            ("/s/missing", 404),
            ("/x/settings.txt", 404),
            // What stays: a link to a file of the folder's, a chain of them, a name of its own.
            ("/s/alias.txt", 200),
            ("/s/c2.txt", 200),
            ("/s/ok.txt", 200),
            ("/s/docs/real.html", 200),
            ("/s/up/public/ok.txt", 200),
            ("/s/conf/app.json", 200),
            ("/s/.well-known/acme.txt", 200),
            ("/s/%C3%BCn%C3%AF-link.txt", 200),
            // A folder that is itself a link serves what it holds, `.well-known` too.
            ("/s/sub/", 200),
            ("/s/page", 200),
            ("/x/ok.txt", 200),
            ("/x/sub/", 200),
            ("/x/.well-known/acme.txt", 200),
        ],
        "SECRET",
    );
    assert_eq!(support::send(s.port, "GET", "/s/c2.txt", &[], "").body, "ok");
    assert_eq!(support::send(s.port, "GET", "/x/sub/", &[], "").body, "sub");
    assert_eq!(support::send(s.port, "GET", "/s/%C3%BCn%C3%AF-link.txt", &[], "").body, "unicode");
    assert_eq!(support::send(s.port, "GET", "/s/missing", &[], "").body, "not found");
    for path in ["/p/settings.txt", "/p/nothing.txt"] {
        let a = support::send(s.port, "GET", path, &[], "");
        assert_eq!((a.status, a.body.as_str()), (404, "missing page"), "{}", path);
    }
    let a = support::send(s.port, "GET", "/f", &[], "");
    assert_eq!((a.status, a.body.as_str()), (200, "outside page"));
    // `quillo serve` is `files`: the same, and the `.env` of the folder it serves is known by
    // identity too.
    let (_g, port) = serve_folder(&d, ".");
    statuses(port, &[("/public/settings.txt", 404), ("/public/notes.txt", 404), ("/public/e1.txt", 404), ("/public/hard-env.txt", 404), ("/public/alias.txt", 200), ("/public/c2.txt", 200)], "SECRET");
}

// Q-702: `files(".", path)` served the SQLite database the program had open, and its `-wal` with
// the password hashes in it, in any case. Now `files` serves no SQLite database, by its first
// bytes: the program's under any name, case, normalization, link or hard link, a tenant's not
// opened since the start, another program's, one no one has open (`file` serves one named on
// purpose); nor its `-wal`, `-shm` or `-journal`. `quillo serve` the same, beside a program that
// runs or is stopped. A name ending as a journal's beside a file that is no database is served.
#[cfg(unix)]
#[test]
fn files_serves_no_sqlite_database() {
    let d = dir(
        "databases",
        r#"db.open("app.db")
db.exec("create table if not exists users (email text, password_hash text)")
db.exec("insert into users values ('alice@example.com', 'HASHVALUE')")
let other = db.connect("données.data")
other.exec("create table if not exists t (x int)")
let tenants = db.connect("t/{tenant}.db")
route GET /late {
    let c = db.connect("late/l.db")
    c.exec("create table if not exists t (x int)")
    "opened"
}
route GET /s/*p = files("public", p)
route GET /r/*path = files(".", path)
route GET /f = file("closed.db")
route POST /into ? { to: str } {
    save_upload(request.files.doc, to) ?? "refused"
}
"#,
    );
    for f in ["public", "late", "t"] {
        std::fs::create_dir_all(d.join(f)).unwrap();
    }
    let sqlite = |name: &str, rows: &str| {
        let c = rusqlite::Connection::open(d.join(name)).unwrap();
        c.execute_batch(&format!("create table x (a text); insert into x values ('{}')", rows)).unwrap();
    };
    std::fs::write(d.join("index.html"), "<p>home</p>").unwrap();
    // Databases no one has open: one of its own, a tenant's, and one with a journal beside it.
    sqlite("closed.db", "HASHVALUE-CLOSED");
    sqlite("t/acme.db", "HASHVALUE-TENANT");
    sqlite("old.db", "HASHVALUE-OLD");
    std::fs::write(d.join("old.db-journal"), b"HASHVALUE").unwrap();
    std::fs::write(d.join("old.db-shm"), b"HASHVALUE").unwrap();
    // Files that only look so: names ending as a journal's beside notes, or alone; a text about
    // the format; and the first bytes of a log and of a journal under names of their own.
    std::fs::write(d.join("public/travel"), "notes").unwrap();
    std::fs::write(d.join("public/travel-journal"), "notes").unwrap();
    std::fs::write(d.join("notes-journal"), "notes").unwrap();
    std::fs::write(d.join("-wal"), "only an ending").unwrap();
    std::fs::write(d.join("about.txt"), "SQLite format 3 is a file format").unwrap();
    std::fs::write(d.join("log.bin"), [0x37, 0x7f, 0x06, 0x82, 0, 0, 0, 0]).unwrap();
    std::fs::write(d.join("j.bin"), [0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7, b'H']).unwrap();
    link("../app.db", &d.join("public/data.bin"));
    let s = support::serve(&d.join("main.qwe"), &[]);
    std::fs::hard_link(d.join("app.db"), d.join("public/backup.bin")).unwrap();
    std::fs::copy(d.join("app.db"), d.join("app.db.bak")).unwrap();
    std::fs::write(d.join("app.dbx"), "x").unwrap();
    assert_eq!(support::send(s.port, "GET", "/late", &[], "").body, "opened");
    let kept = [
        "/r/app.db",
        "/r/APP.DB",
        "/r/App.Db",
        "/r/app.db-wal",
        "/r/APP.DB-WAL",
        "/r/app.db-shm",
        "/r/app.db-Shm",
        "/s/data.bin",
        "/s/backup.bin",
        "/r/app.db.bak",
        "/r/donn%C3%A9es.data",
        "/r/donne%CC%81es.data",
        "/r/late/l.db",
        "/r/late/L.DB-WAL",
        "/r/t/acme.db",
        "/r/t/ACME.DB",
        "/r/closed.db",
        "/r/old.db",
        "/r/old.db-journal",
        "/r/old.db-shm",
        "/r/log.bin",
        "/r/j.bin",
        "/r/main.qwe",
    ];
    statuses(s.port, &kept.iter().map(|p| (*p, 404)).collect::<Vec<_>>(), "HASHVALUE");
    let served = ["/s/travel", "/s/travel-journal", "/r/notes-journal", "/r/-wal", "/r/about.txt", "/r/app.dbx", "/r/index.html"];
    for path in served {
        assert_eq!(support::send(s.port, "GET", path, &[], "").status, 200, "{}", path);
    }
    // A visitor's upload makes no tenant's database, opened or not; a path of another shape is
    // a file like another.
    let upload = |to: &str| {
        let body = "--B\r\nContent-Disposition: form-data; name=\"doc\"; filename=\"x\"\r\nContent-Type: text/plain\r\n\r\nSQLite format 3\r\n--B--\r\n";
        support::send(s.port, "POST", &format!("/into?to={}", enc(to)), &[("Content-Type", "multipart/form-data; boundary=B")], body).body
    };
    for to in ["t/new.db", "t/acme.db", "t/ACME.DB-WAL", "T/Other.Db", "app.db-journal", "late/l.db"] {
        assert_eq!(upload(to), "refused", "{}", to);
    }
    for to in ["t/sub/x.db", "t2/x.db", "t/x.dbx"] {
        assert_eq!(upload(to), to, "{}", to);
    }
    // `file` serves one named on purpose.
    let f = support::send(s.port, "GET", "/f", &[], "");
    assert!(f.status == 200 && f.body.starts_with("SQLite format 3"), "{}", f.status);
    // `quillo serve .` beside the running program, and once it has stopped.
    let quillo_serve = |running: bool| {
        let (_g, port) = serve_folder(&d, ".");
        statuses(port, &[("/app.db", 404), ("/APP.DB", 404), ("/app.db-wal", 404), ("/t/acme.db", 404), ("/closed.db", 404), ("/public/backup.bin", 404), ("/old.db-journal", 404)], "HASHVALUE");
        for path in ["/index.html", "/app.dbx", "/public/travel-journal"] {
            assert_eq!(support::send(port, "GET", path, &[], "").status, 200, "{} (running: {})", path, running);
        }
    };
    quillo_serve(true);
    let mut s = s;
    s.kill();
    quillo_serve(false);
}

// Q-703: `check_program` on a visitor's code read `.env` and `.git/config` for a `use` and said
// their contents back as unknown names. Now a confined check reads only Quillo files, none whose
// name or folder starts with a dot, through a link too; what it may use is used as before.
#[cfg(unix)]
#[test]
fn a_confined_check_reads_only_quillo_files() {
    let d = dir(
        "confined",
        r#"fn c(code) = check_program("probe.qwe", { "probe.qwe": code })
for code in [
    "use \".env\"",
    "use \".ENV\"",
    "use \"site/.git/config\"",
    "use \".git/config\"",
    "use \"notes.txt\"",
    "use \"notes.qwe\"",
    "use \"%2eenv\"",
    "mount /x = \".env\"",
    "service s = \".env\"",
    "mount /x = \"lib/p.qwer\"",
    "use \"../x.qwe\"",
    "use \"/etc/passwd\"",
    "use \"%2e%2e/x.qwe\"",
    "use \"lib.qwe\"\nprint(word())",
    "import lib.qwe use word\nprint(word())",
    "use \"./lib.qwe\"\nprint(word())",
    "use \"ünï.qwe\"\nprint(üw())",
    "import lib/p.qwer use add\nprint(add(1, 2))",
    "",
] {
    print(json.stringify(c(code)))
}
print(check_program(".env"))
print(check_program("notes.txt"))
print(json.stringify(check_program("lib.qwe")))
"#,
    );
    std::fs::create_dir_all(d.join("site/.git")).unwrap();
    std::fs::create_dir_all(d.join("lib")).unwrap();
    for (f, text) in [(".env", "DB_PASSWORD=hunter2-secret\n"), ("site/.git/config", "[core]\n\tgitconfig = 1\n"), ("notes.txt", "SECRET_TEXT = 1\n"), ("lib.qwe", "fn word() = \"w\"\n"), ("ünï.qwe", "fn üw() = 1\n"), ("lib/p.qwer", "pub fn add(a: i64, b: i64) -> i64 { a + b }\n")] {
        std::fs::write(d.join(f), text).unwrap();
    }
    link(".env", &d.join("notes.qwe"));
    let out = run_script(&d, "main.qwe");
    for secret in ["hunter2", "secret", "gitconfig", "SECRET_TEXT", "DB_PASSWORD"] {
        assert!(!out.contains(secret), "{} given away: {}", secret, out);
    }
    let lines: Vec<&str> = out.lines().collect();
    let refused = |path: &str| format!("only Quillo files are read (.qwe, and a plugin's .qwer or .rs for `use`), none whose name or folder starts with a dot: {}", path);
    for (i, path) in [".env", ".ENV", "site/.git/config", ".git/config", "notes.txt", "notes.qwe", "%2eenv", ".env", ".env", "lib/p.qwer"].iter().enumerate() {
        assert!(lines[i].contains(&refused(path)) && lines[i].matches("\"message\"").count() == 1, "{}: {}", path, lines[i]);
    }
    assert!(lines[10].contains("path must stay inside the program's folder: ../x.qwe"), "{}", lines[10]);
    assert!(lines[11].contains("path must stay inside the program's folder: /etc/passwd"), "{}", lines[11]);
    assert!(lines[12].contains("cannot read %2e%2e/x.qwe"), "{}", lines[12]);
    for (i, line) in lines[13..19].iter().enumerate() {
        assert_eq!(*line, "[]", "use {}: {}", i, out);
    }
    assert_eq!(lines[19], format!("err(\"check_program: {}\")", refused(".env")));
    assert_eq!(lines[20], format!("err(\"check_program: {}\")", refused("notes.txt")));
    assert_eq!(lines[21], "[]");
    // The file checked, through a link into a folder `--allow-path` names: by the name it has
    // there.
    let outside = support::temp_dir("feedback55-confined-outside");
    std::fs::write(outside.join("ok.qwe"), "fn ok() = 1\n").unwrap();
    std::fs::write(outside.join(".env"), "DB_PASSWORD=hunter2-secret\n").unwrap();
    link(".env", &outside.join("peek.qwe"));
    link(&outside.to_string_lossy(), &d.join("ext"));
    std::fs::write(d.join("b.qwe"), "print(json.stringify(check_program(\"ext/ok.qwe\")))\nprint(check_program(\"ext/peek.qwe\"))\nprint(json.stringify(check_program(\"ext/gone.qwe\")))\n").unwrap();
    let out = run_script_with(&d, "b.qwe", &["--allow-path", &outside.to_string_lossy()]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "[]", "{}", out);
    assert_eq!(lines[1], format!("err(\"check_program: {}\")", refused("ext/peek.qwe")));
    assert!(lines[2].contains("cannot read file") && !out.contains("hunter2"), "{}", out);
}

// Q-704: the refusal to write `.env` held only for that spelling: `.ENV` was written, and on macOS
// read as the settings at the next start; so `.ENV.LOCAL`, `.ENV.PROD` by `image.convert`, an
// upload named `.ENV`, and `APP.DB-JOURNAL` beside the open database. Now every rule of the
// program's own files compares names as the file system may, and only those names.
#[test]
fn the_programs_own_files_are_refused_in_any_case() {
    let d = dir(
        "own-case",
        r#"print(write_file(".env", "A=1"))
print(write_file(".ENV", "ADMIN_TOKEN=attacker"))
print(write_file(".Env", "A=1"))
print(write_file(".ENV.LOCAL", "A=1"))
print(write_file(".env.", "A=1"))
print(write_file("config/.ENV", "A=1"))
print(image.convert("tiny.png", ".ENV.PROD", { format: "png" }))
print(delete_file(".ENV"))
print(write_file(".envrc", "A=1"))
print(write_file("x.env", "A=1"))
print(write_file(".env-notes", "A=1"))
print(write_file("ünï/.ENV", "A=1"))
db.open("data/app.db")
print(write_file("data/APP.DB-JOURNAL", "x"))
print(write_file("data/App.Db-Wal", "x"))
print(write_file("data/app.db-ſhm", "x"))
print(write_file("data/app.db-journals", "x"))
print(write_file("data/app.db2-journal", "x"))
"#,
    );
    std::fs::create_dir_all(d.join("data")).unwrap();
    image::GrayImage::new(4, 4).save(d.join("tiny.png")).unwrap();
    let out = run_script(&d, "main.qwe");
    let lines: Vec<&str> = out.lines().collect();
    let settings = "it is the program's settings (.env)";
    for (i, name) in [".env", ".ENV", ".Env", ".ENV.LOCAL", ".env.", "config/.ENV"].iter().enumerate() {
        assert_eq!(lines[i], format!("err(\"write_file: will not write {}: {}\")", name, settings), "{}", out);
    }
    assert_eq!(lines[6], format!("err(\"image: will not write .ENV.PROD: {}\")", settings));
    assert_eq!(lines[7], format!("err(\"delete_file: will not delete .ENV: {}\")", settings));
    // Not the settings: `.envrc` is direnv's, `x.env` and `.env-notes` other names.
    assert_eq!(&lines[8..11], &["null", "null", "null"], "{}", out);
    assert_eq!(lines[11], format!("err(\"write_file: will not write ünï/.ENV: {}\")", settings));
    for (i, name) in ["data/APP.DB-JOURNAL", "data/App.Db-Wal", "data/app.db-ſhm"].iter().enumerate() {
        assert_eq!(lines[12 + i], format!("err(\"write_file: will not write {}: it is the database the program has open\")", name), "{}", out);
    }
    assert_eq!(&lines[15..17], &["null", "null"], "{}", out);
    let names: Vec<String> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_lowercase()).collect();
    assert!(!names.iter().any(|n| n == ".env" || n.starts_with(".env.")), "{:?}", names);
    assert!(!d.join("config").exists() && !d.join("ünï").exists());
    // The next start has no settings from it.
    std::fs::write(d.join("b.qwe"), "print(env(\"ADMIN_TOKEN\") ?? \"none\")\n").unwrap();
    assert_eq!(run_script(&d, "b.qwe").trim(), "none");
    // An upload, under the visitor's name or a path, in any case.
    let u = dir(
        "own-upload",
        "route POST /drop {\n    let f = request.files.doc\n    save_upload(f, f.filename) ?? \"refused\"\n}\nroute POST /into ? { to: str } {\n    save_upload(request.files.doc, to) ?? \"refused\"\n}\n",
    );
    let s = support::serve(&u.join("main.qwe"), &[]);
    let upload = |path: &str, name: &str| {
        let body = format!("--B\r\nContent-Disposition: form-data; name=\"doc\"; filename=\"{}\"\r\nContent-Type: text/plain\r\n\r\nADMIN_TOKEN=attacker\r\n--B--\r\n", name);
        support::send(s.port, "POST", path, &[("Content-Type", "multipart/form-data; boundary=B")], &body).body
    };
    for name in [".ENV", ".env", ".Env.Prod", "MAIN.QWE"] {
        assert_eq!(upload("/drop", name), "refused", "{}", name);
    }
    for to in ["MIGRATIONS/002.sql", "Migrations/x.sql", "./.ENV"] {
        assert_eq!(upload(&format!("/into?to={}", enc(to)), "x.txt"), "refused", "{}", to);
    }
    assert_eq!(upload("/drop", "report.txt"), "report.txt");
    assert_eq!(upload("/into?to=migrations2%2Fx.sql", "x.txt"), "migrations2/x.sql");
    assert!(!std::fs::read_dir(&u).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().to_lowercase().starts_with(".env")));
}

// Q-704: a name as Windows takes it, on every system (a stream of `.env`, `app.db.` with its dot),
// and a second hard link to the settings or the program, which writes into them, are refused.
#[cfg(unix)]
#[test]
fn a_name_windows_shortens_and_a_hard_link_are_the_programs_files() {
    let d = dir(
        "own-names",
        r#"db.open("data/app.db")
for p in [".env::$DATA", ".env:x", "data/app.db.", "data/app.db:x", "data/app.db-wal ", "hard.txt", "src.txt", "x:y", "z.db.", "e:v", "notes:x.txt", "data/app.dbs", ":x"] {
    let r = write_file(p, "x")
    print(is_err(r) ? r : "written")
}
"#,
    );
    std::fs::create_dir_all(d.join("data")).unwrap();
    std::fs::write(d.join(".env"), "A=1\n").unwrap();
    std::fs::hard_link(d.join(".env"), d.join("hard.txt")).unwrap();
    std::fs::hard_link(d.join("main.qwe"), d.join("src.txt")).unwrap();
    // Hard links whose names Windows would shorten: judged as the file they are, too.
    rusqlite::Connection::open(d.join("data/app.db")).unwrap().execute_batch("create table t (x int)").unwrap();
    std::fs::hard_link(d.join("data/app.db"), d.join("x:y")).unwrap();
    std::fs::hard_link(d.join("data/app.db"), d.join("z.db.")).unwrap();
    std::fs::hard_link(d.join(".env"), d.join("e:v")).unwrap();
    let out = run_script(&d, "main.qwe");
    let lines: Vec<&str> = out.lines().collect();
    let (settings, open) = ("it is the program's settings (.env)", "it is the database the program has open");
    for (i, (name, why)) in [(".env::$DATA", settings), (".env:x", settings), ("data/app.db.", open), ("data/app.db:x", open), ("data/app.db-wal ", open), ("hard.txt", settings), ("src.txt", "it is one of the program's own files"), ("x:y", open), ("z.db.", open), ("e:v", settings)].iter().enumerate() {
        assert_eq!(lines[i], format!("err(\"write_file: will not write {}: {}\")", name, why), "{}", out);
    }
    assert_eq!(&lines[10..], &["written", "written", "written"], "{}", out);
    assert!(std::fs::read(d.join("data/app.db")).unwrap().starts_with(b"SQLite format 3\0"));
    assert_eq!(std::fs::read_to_string(d.join(".env")).unwrap(), "A=1\n");
    assert!(std::fs::read_to_string(d.join("main.qwe")).unwrap().starts_with("db.open"));
}

// Q-705: `delete_file` refused the open database but deleted the folder holding it. Now a folder
// holding a database the program has open is refused however it is named, and a folder beside it
// is deleted as before.
#[cfg(unix)]
#[test]
fn delete_file_keeps_the_folder_of_an_open_database() {
    let d = dir(
        "db-folder",
        r#"import lib/a.qwe use word
db.open("old/first.db")
db.exec("create table if not exists t (x int)")
db.open("data/app.db")
db.exec("create table if not exists t (x int)")
let other = db.connect("other/b.db")
other.exec("create table if not exists t (x int)")
for p in ["data/app.db", "data", "data/", "data/.", "./data", "DATA", "dlink", "other", "lib", "data/notes.txt", "data2", "dat", "ünï", "old"] {
    print(`${p}: ${is_err(delete_file(p)) ? "refused" : "gone"}`)
}
print(word())
"#,
    );
    for f in ["lib", "data", "other", "data2", "ünï", "old"] {
        std::fs::create_dir_all(d.join(f)).unwrap();
    }
    std::fs::write(d.join("lib/a.qwe"), "fn word() = \"clinic\"\n").unwrap();
    std::fs::write(d.join("data/notes.txt"), "n").unwrap();
    std::fs::write(d.join("data2/x"), "x").unwrap();
    link("data", &d.join("dlink"));
    let out = run_script(&d, "main.qwe");
    // The database `db.open` replaced is closed, and a file again; `db.connect`'s stays open.
    let want = "data/app.db: refused\ndata: refused\ndata/: refused\ndata/.: refused\n./data: refused\nDATA: refused\ndlink: refused\nother: refused\nlib: refused\ndata/notes.txt: gone\ndata2: gone\ndat: gone\nünï: gone\nold: gone\nclinic\n";
    assert_eq!(out, want);
    assert!(d.join("data/app.db").is_file() && d.join("other/b.db").is_file() && d.join("lib/a.qwe").is_file());
    assert!(!d.join("data/notes.txt").exists() && !d.join("data2").exists() && !d.join("ünï").exists() && !d.join("old").exists());
}

// Q-701: what `files` reads or sends of a file is the file it looked at, or one saved in its place
// since that it would serve: a link to the settings swapped in, or a SQLite database, is not read
// in whole, in part, by its first bytes or sent; a file saved anew (a rename over it) is.
#[cfg(unix)]
#[test]
fn a_file_read_is_the_file_looked_at_or_one_served_in_its_place() {
    use quillo::statics;
    use std::io::Read;
    let d = support::temp_dir("feedback55-swap");
    let f = d.join("ok.txt");
    std::fs::write(&f, "ok").unwrap();
    std::fs::write(d.join(".env"), "SECRET=1").unwrap();
    quillo::builtins::protect(&d.join(".env"), "it is the program's settings (.env)");
    let info = statics::info(&f).unwrap();
    assert!(info.id.is_some());
    // Saved anew: the new file is read.
    std::fs::write(d.join("new.txt"), "new").unwrap();
    std::fs::rename(d.join("new.txt"), &f).unwrap();
    assert_eq!(statics::read_range(&f, 0, 2, info.id).unwrap(), b"new");
    assert_eq!(statics::head(&f, info.id).unwrap(), b"new");
    assert_eq!(statics::read_if(&f, &info, |_| true).unwrap().as_deref(), Some(&b"new"[..]));
    let send = |id| {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = std::net::TcpStream::connect(l.local_addr().unwrap()).unwrap();
        client.set_read_timeout(Some(std::time::Duration::from_secs(30))).unwrap();
        let (server, _) = l.accept().unwrap();
        statics::send(server, &quillo::server::plain(200, ""), (f.clone(), 0, 3, id), false);
        let mut out = String::new();
        client.read_to_string(&mut out).unwrap();
        out
    };
    assert!(send(info.id).ends_with("\r\n\r\nnew"));
    // A link to the settings, or a SQLite database, swapped in: not read, nor sent.
    let info = statics::info(&f).unwrap();
    std::fs::remove_file(&f).unwrap();
    link(".env", &f);
    assert!(statics::read_if(&f, &info, |_| true).is_err());
    assert!(statics::head(&f, info.id).is_err());
    assert!(statics::read_range(&f, 0, 1, info.id).is_err());
    assert!(send(info.id).starts_with("HTTP/1.1 404"));
    std::fs::remove_file(&f).unwrap();
    std::fs::write(&f, b"SQLite format 3\0rows").unwrap();
    assert!(statics::read_range(&f, 0, 1, info.id).is_err());
    assert!(send(info.id).starts_with("HTTP/1.1 404"));
    // Without a file to hold to (`file`, named on purpose), what is there is read.
    assert_eq!(statics::read_range(&f, 0, 5, None).unwrap(), b"SQLite");
    let m = statics::mark(&f, 0, 2, info.id);
    assert_eq!(statics::marked(&m), Some((f.clone(), 0, 2, info.id)));
    assert_eq!(statics::marked(&statics::mark(&f, 1, 1, None)), Some((f.clone(), 1, 1, None)));
}

// Q-701: a link into a tool's dot folder inside the folder served is followed, as before: pnpm's
// `node_modules/htmx.org -> .pnpm/...`, a site built into `.vitepress/dist`. A path asked with a
// dot part, a dot file, and a folder of secrets or of Quillo's state are not served that way.
#[cfg(unix)]
#[test]
fn files_follows_a_link_into_a_tools_dot_folder() {
    let d = dir("dot-folders", "route GET /nm/*p = files(\"node_modules\", p)\nroute GET /site/*p = files(\"site\", p)\nroute GET /r/*p = files(\".\", p)\n");
    for f in ["node_modules/.pnpm/htmx.org@1.9.10/node_modules/htmx.org/dist", ".vitepress/dist", ".git", ".quillo/sites", "node_modules/.cache"] {
        std::fs::create_dir_all(d.join(f)).unwrap();
    }
    for (f, text) in [
        ("node_modules/.pnpm/htmx.org@1.9.10/node_modules/htmx.org/dist/htmx.min.js", "htmx"),
        ("node_modules/.pnpm/htmx.org@1.9.10/node_modules/htmx.org/.npmrc", "SECRET token"),
        (".vitepress/dist/index.html", "<p>docs</p>"),
        (".git/config", "[core] SECRET"),
        (".quillo/sites/shop.json", "SECRET cookies"),
        ("node_modules/.cache/x.js", "cached"),
    ] {
        std::fs::write(d.join(f), text).unwrap();
    }
    link(".pnpm/htmx.org@1.9.10/node_modules/htmx.org", &d.join("node_modules/htmx.org"));
    link(".cache", &d.join("node_modules/cache"));
    link(".vitepress/dist", &d.join("site"));
    link(".git", &d.join("docs"));
    link(".quillo", &d.join("state"));
    let s = support::serve(&d.join("main.qwe"), &[]);
    statuses(
        s.port,
        &[
            ("/nm/htmx.org/dist/htmx.min.js", 200),
            ("/nm/cache/x.js", 200),
            ("/site/", 200),
            ("/site/index.html", 200),
            ("/nm/htmx.org/.npmrc", 404),
            ("/nm/.pnpm/htmx.org@1.9.10/node_modules/htmx.org/dist/htmx.min.js", 404),
            ("/r/docs/config", 404),
            ("/r/state/sites/shop.json", 404),
        ],
        "SECRET",
    );
    assert_eq!(support::send(s.port, "GET", "/nm/htmx.org/dist/htmx.min.js", &[], "").body, "htmx");
}

// Q-701: the program's own files are known by number with their path: one saved anew (renamed
// over, as an editor's safe save, `git pull` or a deploy does) leaves its old number to a file
// that is no longer one of them, which is served and written; a `.env` saved anew is known again
// when the program reloads.
#[cfg(unix)]
#[test]
fn a_file_saved_anew_leaves_its_old_number() {
    let d = dir("saved-anew", "import lib.qwe use word\nroute GET /s/*p = files(\"public\", p)\nroute GET /w = is_err(write_file(\"public/copy.txt\", \"x\")) ? \"refused\" : \"written\"\nroute GET / = word()\n");
    std::fs::create_dir_all(d.join("public")).unwrap();
    std::fs::write(d.join("lib.qwe"), "fn word() = \"one\"\n").unwrap();
    std::fs::write(d.join(".env"), "A=1\n").unwrap();
    std::fs::hard_link(d.join("lib.qwe"), d.join("public/copy.txt")).unwrap();
    let s = support::serve_with(&d.join("main.qwe"), &["--reload"]);
    assert_eq!(support::send(s.port, "GET", "/s/copy.txt", &[], "").status, 404);
    assert_eq!(support::send(s.port, "GET", "/w", &[], "").body, "refused");
    // `.env` saved anew, and a hard link to the new one: known once the program has reloaded.
    std::fs::write(d.join("env.tmp"), "A=2\n").unwrap();
    std::fs::rename(d.join("env.tmp"), d.join(".env")).unwrap();
    std::fs::hard_link(d.join(".env"), d.join("public/settings.txt")).unwrap();
    // lib.qwe saved anew: the old number is the copy's alone now.
    std::fs::write(d.join("lib.tmp"), "fn word() = \"two\"\n").unwrap();
    std::fs::rename(d.join("lib.tmp"), d.join("lib.qwe")).unwrap();
    assert_eq!(support::send(s.port, "GET", "/s/copy.txt", &[], "").status, 200);
    assert_eq!(support::send(s.port, "GET", "/w", &[], "").body, "written");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while support::send(s.port, "GET", "/", &[], "").body != "two" {
        assert!(std::time::Instant::now() < deadline, "the program did not reload");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(support::send(s.port, "GET", "/s/settings.txt", &[], "").status, 404);
}

// Q-701: a large file saved anew while it is being sent again and again: every answer is whole,
// of one version or the other, never a 404.
#[cfg(unix)]
#[test]
fn a_large_file_saved_anew_while_sent_is_served() {
    let d = dir("large-swap", "route GET /s/*p = files(\"public\", p)\n");
    std::fs::create_dir_all(d.join("public")).unwrap();
    let size = 2 << 20;
    std::fs::write(d.join("public/big.bin"), vec![b'a'; size]).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saver = {
        let (d, stop) = (d.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut n = 0u8;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let tmp = d.join(format!("public/big.{}.tmp", n % 2));
                std::fs::write(&tmp, vec![if n % 2 == 0 { b'b' } else { b'a' }; size]).unwrap();
                std::fs::rename(&tmp, d.join("public/big.bin")).unwrap();
                n = n.wrapping_add(1);
            }
        })
    };
    for i in 0..150 {
        let a = support::send(s.port, "GET", "/s/big.bin", &[], "");
        assert_eq!((a.status, a.body.len()), (200, size), "request {}", i);
        assert!(a.body.bytes().all(|b| b == b'a') || a.body.bytes().all(|b| b == b'b'), "request {} mixed two files", i);
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    saver.join().unwrap();
}

// The user's decision: `files` serves no SQLite file, and `file` serves one named on purpose, with
// byte ranges, as a database read in the browser over HTTP (sql.js-httpvfs) asks for it.
#[test]
fn file_serves_a_database_named_on_purpose_in_ranges() {
    let d = dir("db-ranges", "route GET /db = file(\"data.db\")\nroute GET /big = file(\"big.db\")\nroute GET /s/*p = files(\".\", p)\n");
    rusqlite::Connection::open(d.join("data.db")).unwrap().execute_batch("create table t (x text); insert into t values ('row')").unwrap();
    let mut big = b"SQLite format 3\0".to_vec();
    big.resize(3 << 20, 7);
    std::fs::write(d.join("big.db"), &big).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    for path in ["/db", "/big"] {
        let a = support::send(s.port, "GET", path, &[("Range", "bytes=0-15")], "");
        assert_eq!((a.status, a.body.as_str()), (206, "SQLite format 3\0"), "{}", path);
        assert!(a.header("content-range").is_some_and(|r| r.starts_with("bytes 0-15/")), "{}", path);
        let a = support::send(s.port, "GET", path, &[("Range", "bytes=16-16")], "");
        assert_eq!((a.status, a.body.len()), (206, 1), "{}", path);
        assert_eq!(support::send(s.port, "GET", path, &[], "").status, 200, "{}", path);
    }
    for path in ["/s/data.db", "/s/big.db"] {
        assert_eq!(support::send(s.port, "GET", path, &[("Range", "bytes=0-15")], "").status, 404, "{}", path);
    }
}
