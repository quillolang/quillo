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
    // A folder of each check's own: tests run at once.
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = dir(&format!("check-{}", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)), src);
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

// Q-701: a file is read as it was looked at: one saved anew in between (renamed over, with
// another length) is looked at again, not read under the other's head; one opened is a file of
// bytes, and a FIFO in its place does not hold the reader. A large file's mark for `files` names
// the folders it judged and the file's tag, and what is sent is the file opened, judged again:
// one put in the place of the judged is sent whole with its own head, and one that leads out of
// the folder, to the settings, or is a database, is a 404.
#[cfg(unix)]
#[test]
fn a_file_is_read_and_sent_as_it_was_looked_at() {
    use quillo::statics;
    use std::io::Read;
    let d = support::temp_dir("feedback55-swap");
    let public = d.join("public");
    std::fs::create_dir_all(&public).unwrap();
    let f = public.join("ok.txt");
    std::fs::write(&f, "ok").unwrap();
    let info = statics::info(&f).unwrap();
    std::fs::write(public.join("new.tmp"), "a longer one").unwrap();
    std::fs::rename(public.join("new.tmp"), &f).unwrap();
    assert!(statics::read(&f, &info).is_err_and(|e| statics::is_changed(&e)));
    let now = statics::info(&f).unwrap();
    assert_eq!(&*statics::read(&f, &now).unwrap(), b"a longer one");
    assert_eq!(statics::read_range(&f, 2, 7).unwrap(), b"longer");
    // A FIFO is no file, and opening it does not wait for a writer.
    let fifo = public.join("pipe");
    let c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let (tx, rx) = std::sync::mpsc::channel();
    let at = fifo.clone();
    std::thread::spawn(move || tx.send(statics::open_regular(&at).is_err()).unwrap());
    assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(5)), Ok(true));
    assert!(statics::info(&fifo).is_err());
    // Where the file opened really is, through a link.
    link("ok.txt", &public.join("alias.txt"));
    let (opened, _) = statics::open_regular(&public.join("alias.txt")).unwrap();
    assert_eq!(statics::path_of(&opened).map(|p| p.canonicalize().unwrap()), Some(f.canonicalize().unwrap()));
    // The mark, and the send.
    let hosted = statics::Hosted { served: public.clone(), program: d.clone(), etag: now.etag.clone(), id: now.id };
    let m = statics::mark(&f, 0, now.len, Some(&hosted));
    assert_eq!(statics::marked(&m), Some((f.clone(), 0, now.len, Some(hosted.clone()))));
    assert_eq!(statics::marked(&statics::mark(&f, 1, 1, None)), Some((f.clone(), 1, 1, None)));
    let send = |what: (std::path::PathBuf, u64, u64, Option<statics::Hosted>)| {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = std::net::TcpStream::connect(l.local_addr().unwrap()).unwrap();
        client.set_read_timeout(Some(std::time::Duration::from_secs(30))).unwrap();
        let (server, _) = l.accept().unwrap();
        let mut head = quillo::server::plain(206, "");
        head.headers.push(("etag".into(), now.etag.clone()));
        head.headers.push(("content-range".into(), "bytes 2-7/12".into()));
        statics::send(server, &head, what, false);
        let mut out = String::new();
        client.read_to_string(&mut out).unwrap();
        out
    };
    let out = send((f.clone(), 2, 6, Some(hosted.clone())));
    assert!(out.starts_with("HTTP/1.1 206") && out.ends_with("\r\n\r\nlonger"), "{}", out);
    // Saved anew, shorter, since the mark: the whole new file, with its own length and tag.
    std::fs::write(public.join("new.tmp"), "short").unwrap();
    std::fs::rename(public.join("new.tmp"), &f).unwrap();
    let out = send((f.clone(), 2, 6, Some(hosted.clone())));
    let fresh = statics::info(&f).unwrap();
    assert!(out.starts_with("HTTP/1.1 200") && out.contains("content-length: 5\r\n") && out.contains(&fresh.etag) && !out.contains("content-range") && out.ends_with("\r\n\r\nshort"), "{}", out);
    // A link swapped in: out of the folder, to the settings, or to a database.
    let outside = support::temp_dir("feedback55-swap-outside");
    std::fs::write(outside.join("id_rsa"), "SECRET key").unwrap();
    std::fs::write(d.join(".env"), "SECRET=1").unwrap();
    std::fs::write(d.join("app.db"), b"SQLite format 3\0rows").unwrap();
    for to in [outside.join("id_rsa"), d.join(".env"), d.join("app.db")] {
        std::fs::remove_file(&f).unwrap();
        link(&to.to_string_lossy(), &f);
        let out = send((f.clone(), 0, 5, Some(hosted.clone())));
        assert!(out.starts_with("HTTP/1.1 404") && !out.contains("SECRET") && !out.contains("rows"), "{}: {}", to.display(), out);
    }
    // `file`, named on purpose, sends what is there.
    assert!(send((f.clone(), 0, 6, None)).ends_with("SQLite"));
}

// Q-701: a link into a build tool's output inside the folder served is followed, as before: pnpm's
// `node_modules/htmx.org -> .pnpm/...`, sites built into `.vitepress/dist`, `.output/public`,
// `.next/static`, `.vercel/output/static`. Every other dot folder may hold secrets and is not
// served through a link: `.vscode/sftp.json`, `.config/git/credentials`, `.git`, `.quillo`'s
// cookies, `.cache`, a tool's server code beside its output; nor a path asked with a dot part.
#[cfg(unix)]
#[test]
fn files_follows_a_link_into_a_tools_output_only() {
    let d = dir("dot-folders", "route GET /nm/*p = files(\"node_modules\", p)\nroute GET /site/*p = files(\"site\", p)\nroute GET /r/*p = files(\".\", p)\n");
    let files = [
        ("node_modules/.pnpm/htmx.org@1.9.10/node_modules/htmx.org/dist/htmx.min.js", "htmx"),
        ("node_modules/.pnpm/htmx.org@1.9.10/node_modules/htmx.org/.npmrc", "SECRET token"),
        ("node_modules/.cache/x.js", "SECRET cached"),
        (".vitepress/dist/index.html", "<p>docs</p>"),
        (".output/public/app.js", "app"),
        (".output/server/index.mjs", "SECRET server"),
        (".next/static/chunk.js", "chunk"),
        (".next/server/page.js", "SECRET server"),
        (".vercel/output/static/v.js", "v"),
        (".vscode/sftp.json", "{\"password\": \"SECRET\"}"),
        (".config/git/credentials", "https://u:SECRET@example.com"),
        (".git/config", "[core] SECRET"),
        (".quillo/sites/shop.json", "SECRET cookies"),
    ];
    for (f, text) in files {
        std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
        std::fs::write(d.join(f), text).unwrap();
    }
    for (to, at) in [
        (".pnpm/htmx.org@1.9.10/node_modules/htmx.org", "node_modules/htmx.org"),
        (".cache", "node_modules/cache"),
        (".vitepress/dist", "site"),
        (".output/public/app.js", "app.js"),
        (".output/server/index.mjs", "server.mjs"),
        (".next/static/chunk.js", "chunk.js"),
        (".next/server/page.js", "page.js"),
        (".vercel/output/static/v.js", "v.js"),
        (".vscode/sftp.json", "sftp.json"),
        (".config/git", "gitconf"),
        (".git", "docs"),
        (".quillo", "state"),
    ] {
        link(to, &d.join(at));
    }
    let s = support::serve(&d.join("main.qwe"), &[]);
    statuses(
        s.port,
        &[
            ("/nm/htmx.org/dist/htmx.min.js", 200),
            ("/site/", 200),
            ("/site/index.html", 200),
            ("/r/app.js", 200),
            ("/r/chunk.js", 200),
            ("/r/v.js", 200),
            ("/nm/htmx.org/.npmrc", 404),
            ("/nm/.pnpm/htmx.org@1.9.10/node_modules/htmx.org/dist/htmx.min.js", 404),
            ("/nm/cache/x.js", 404),
            ("/r/server.mjs", 404),
            ("/r/page.js", 404),
            ("/r/sftp.json", 404),
            ("/r/gitconf/credentials", 404),
            ("/r/docs/config", 404),
            ("/r/state/sites/shop.json", 404),
        ],
        "SECRET",
    );
    assert_eq!(support::send(s.port, "GET", "/nm/htmx.org/dist/htmx.min.js", &[], "").body, "htmx");
}

// Q-701: the program's own files are known by number with when they were made: a second hard link
// to one keeps being one when the file is saved anew (renamed over: an editor's safe save, `git
// pull`, a deploy) and keeps what it held, the old settings above all; a source file's old
// version stops being one when the program has loaded the new. A `.env` saved anew, or made
// later, and the file `--env-file` names, are judged by what is at their paths, with or without
// a reload; and the TLS key is the program's own.
#[cfg(unix)]
#[test]
fn a_file_saved_anew_is_judged_as_it_is_now() {
    let d = dir("saved-anew", "import lib.qwe use word\nroute GET /s/*p = files(\"public\", p)\nroute GET /w = is_err(write_file(\"public/copy.txt\", \"x\")) ? \"refused\" : \"written\"\nroute GET / = word()\n");
    for f in ["public", "conf"] {
        std::fs::create_dir_all(d.join(f)).unwrap();
    }
    std::fs::write(d.join("lib.qwe"), "fn word() = \"one\"\n").unwrap();
    std::fs::write(d.join(".env"), "SECRET=1\n").unwrap();
    std::fs::write(d.join("conf/app.env"), "SECRET=2\n").unwrap();
    std::fs::hard_link(d.join("lib.qwe"), d.join("public/copy.txt")).unwrap();
    std::fs::hard_link(d.join(".env"), d.join("public/old-env.txt")).unwrap();
    let save = |name: &str, text: &str| {
        std::fs::write(d.join("saved.tmp"), text).unwrap();
        std::fs::rename(d.join("saved.tmp"), d.join(name)).unwrap();
    };
    let named = d.join("conf/app.env").to_string_lossy().into_owned();
    let s = support::serve_with(&d.join("main.qwe"), &["--reload", "--env-file", &named]);
    let get = |p: &str| support::send(s.port, "GET", p, &[], "");
    assert_eq!(get("/s/copy.txt").status, 404);
    assert_eq!(get("/w").body, "refused");
    // The settings saved anew: the old file's link is still the settings, and a link to the new
    // one, and to the file `--env-file` names saved anew, are too, before any reload.
    save(".env", "SECRET=3\n");
    save("conf/app.env", "SECRET=4\n");
    std::fs::hard_link(d.join(".env"), d.join("public/new-env.txt")).unwrap();
    std::fs::hard_link(d.join("conf/app.env"), d.join("public/named-env.txt")).unwrap();
    statuses(s.port, &[("/s/old-env.txt", 404), ("/s/new-env.txt", 404), ("/s/named-env.txt", 404)], "SECRET");
    // lib.qwe saved anew: once the program has loaded it, the old version is a file like another.
    save("lib.qwe", "fn word() = \"two\"\n");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while get("/").body != "two" {
        assert!(std::time::Instant::now() < deadline, "the program did not reload");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(get("/s/copy.txt").status, 200);
    assert_eq!(get("/w").body, "written");
    statuses(s.port, &[("/s/old-env.txt", 404), ("/s/new-env.txt", 404), ("/s/named-env.txt", 404)], "SECRET");
    // The TLS key `quillo serve` was given, by its name and by another; the certificate is public.
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap().self_signed(&key).unwrap();
    std::fs::write(d.join("public/localhost.pem"), cert.pem()).unwrap();
    std::fs::write(d.join("public/localhost-key.pem"), key.serialize_pem()).unwrap();
    std::fs::hard_link(d.join("public/localhost-key.pem"), d.join("public/key-copy.txt")).unwrap();
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["serve", "public", "--port", &port.to_string(), "--tls-cert", "public/localhost.pem", "--tls-key", "public/localhost-key.pem"])
        .current_dir(&d)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !support::listening(port) {
        assert!(std::time::Instant::now() < deadline, "quillo serve did not start");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    statuses(port, &[("/localhost-key.pem", 404), ("/key-copy.txt", 404)], "PRIVATE KEY");
    assert!(support::send(port, "GET", "/localhost.pem", &[], "").body.contains("BEGIN CERTIFICATE"));
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

// ---- Q-709 to Q-726: answers, cookies, forms, ranges and conditionals ---------------------------

/// `quillo run main.qwe` in `d` with `opts`, its log kept, until the guard is dropped: the guard,
/// its port and the log.
fn serve_logged(d: &Path, opts: &[&str]) -> (support::Guard, u16, PathBuf) {
    let port = support::free_port();
    let log = d.join("server.log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg("main.qwe")
        .args(["--port", &port.to_string(), "--no-check"])
        .args(opts)
        .current_dir(d)
        .env("QUILLO_SECRET", "test secret for the tests")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let g = support::Guard::new(child);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !support::listening(port) {
        assert!(std::time::Instant::now() < deadline, "did not start: {}", std::fs::read_to_string(&log).unwrap_or_default());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    (g, port, log)
}

/// The error a GET of `path` is answered 500 for, as the log says it.
fn error_of(port: u16, log: &Path, path: &str) -> String {
    let a = support::send(port, "GET", path, &[], "");
    assert_eq!(a.status, 500, "{}: {}", path, a.body);
    let mark = format!("GET {} (rid ", path);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let l = std::fs::read_to_string(log).unwrap_or_default();
        if let Some(line) = l.lines().find(|x| x.contains(&mark)) {
            return line.split_once("): ").map_or(line, |(_, m)| m).to_string();
        }
        assert!(std::time::Instant::now() < deadline, "no error for {} in {}", path, l);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// What a server sends on one connection for `requests`, written at once (the last asking it to
/// close), until it closes it.
fn on_one_connection(port: u16, requests: &str) -> String {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(20))).unwrap();
    s.write_all(requests.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

/// Every value of header `name` in an answer.
fn all_of<'a>(a: &'a support::Answer, name: &str) -> Vec<&'a str> {
    a.headers.iter().filter(|(k, _)| k == name).map(|(_, v)| v.as_str()).collect()
}

/// What `quillo run file` in `d` prints with `env` set, under a deadline.
fn script_with_env(d: &Path, file: &str, env: &[(&str, &str)]) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .current_dir(d)
        .envs(env.iter().copied())
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

const NO_BODY: &str = r#"route GET /s/:n = stream((send) => { sleep(20); send("leftover") }, { status: int(n) })
route GET /e/:n = events((send) => send({ a: 1 }), { status: int(n) })
route GET /r/:n { reply int(n) stream((send) => send("x")) }
route GET /re/:n { reply int(n) events((send) => send(1)) }
route GET /next = "next answer"
"#;

// Q-709: `stream` and `events` with 204, 205 or 304 sent a head that says there is no body, then
// the function's chunks, which the next answer on the connection began with. Now those statuses,
// and a 1xx, are an error for both, given as their option or by `reply`; the statuses either side
// of them stream as before, and the next answer on the connection is whole.
#[test]
fn a_stream_takes_no_status_without_a_body() {
    let d = dir("no-body", NO_BODY);
    let (_g, port, log) = serve_logged(&d, &[]);
    for n in [204, 205, 304] {
        for (kind, name) in [("s", "stream"), ("e", "events"), ("r", "stream"), ("re", "events")] {
            let why = error_of(port, &log, &format!("/{}/{}", kind, n));
            assert!(why.contains(&format!("`{}` cannot answer {}: that status has no body", name, n)), "{}", why);
        }
        // The report's repro: two requests on one connection.
        let out = on_one_connection(port, &format!("GET /s/{} HTTP/1.1\r\nHost: x\r\n\r\nGET /next HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n", n));
        let second = out.find("HTTP/1.1 200 OK").unwrap_or_else(|| panic!("{}", out));
        assert!(out.starts_with("HTTP/1.1 500") && out[..second].ends_with("internal error\n") && !out.contains("leftover"), "{}", out);
        assert!(out.ends_with("next answer"), "{}", out);
    }
    for n in [100, 103, 199, 600] {
        assert_eq!(support::send(port, "GET", &format!("/s/{}", n), &[], "").status, 500, "{}", n);
        assert_eq!(support::send(port, "GET", &format!("/r/{}", n), &[], "").status, 500, "{}", n);
    }
    // Either side, and others with a body: streamed, and the next answer follows the last chunk.
    for n in [200, 203, 206, 303, 305, 404, 599] {
        for kind in ["s", "e", "r"] {
            let out = on_one_connection(port, &format!("GET /{}/{} HTTP/1.1\r\nHost: x\r\n\r\nGET /next HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n", kind, n));
            assert!(out.starts_with(&format!("HTTP/1.1 {} ", n)), "{}", out);
            let second = out.rfind("HTTP/1.1 200 OK").unwrap_or_else(|| panic!("{}", out));
            assert!(second > 0 && out[..second].ends_with("\r\n0\r\n\r\n") && out.ends_with("next answer"), "{}", out);
        }
    }
}

const HEADERS: &str = r#"route GET /nul = respond(200, "x", { "x-content-type-options": null, "x-mine": null, "x-n": 5 })
route GET /me { set_session({ user: "u" }); respond(200, "hi", { "cache-control": null }) }
route GET /two = respond(200, "x", { "Set-Cookie": ["a=1", "b=ü"] })
route GET /one = respond(200, "x", { "set-cookie": ["a=1"] })
route GET /none = respond(200, "x", { "set-cookie": [] })
route GET /list = respond(200, "x", { "x-list": ["a", "b"] })
route GET /map = respond(200, "x", { "x-map": { a: 1 } })
route GET /deep = respond(200, "x", { "set-cookie": ["a=1", ["b=2"]] })
route GET /hole = respond(200, "x", { "set-cookie": ["a=1", null] })
route GET /break = respond(200, "x", { "set-cookie": ["a=1", "b=2\r\nx: y"] })
route GET /s = stream((send) => send("s"), { headers: { "set-cookie": ["a=1", "b=2"], "x-accel-buffering": null, "x-n": 1 } })
route GET /sl = stream((send) => send("s"), { headers: { "x-l": [1] } })
route GET /sh = stream((send) => send("s"), { headers: 5 })
route GET /vary { let _c = cookie("a"); respond(200, "x", { "vary": null }) }
route GET /kept { let _c = cookie("a"); respond(200, "x", { "x-n": 1 }) }
route GET /type = respond(200, "x", { "content-type": null })
route GET /e = events((send) => send(1), { headers: { "cache-control": null } })
route GET /seen = respond(200, "x", { "x-a": null, "x-b": "b" }).headers
route GET /et = events((send) => send(1), { type: 5 })
route GET /es = events((send) => send(1), { type: "text/plain" })
"#;

// Q-710: `respond` sent `null`, a list and a map as text: `x-content-type-options: null`,
// `cache-control: null`, and one `set-cookie: ["a=1", "b=2"]`. Now `null` is no such header, nor
// Quillo's own for it (the user's decision, as `site_headers` has it), while a header the program
// does not name keeps Quillo's; a list is one `set-cookie` an item (none for an empty one) and an
// error for another header, as a map is; `stream`'s and `events`' headers are taken the same way,
// and `events` refuses a type that is no str, as `stream` does.
#[test]
fn an_answers_headers_take_null_and_lists_as_http_has_them() {
    let d = dir("headers", HEADERS);
    let (_g, port, log) = serve_logged(&d, &[]);
    let a = support::send(port, "GET", "/nul", &[], "");
    assert!(all_of(&a, "x-content-type-options").is_empty(), "{:?}", a.headers);
    assert_eq!((a.header("x-mine"), a.header("x-n")), (None, Some("5")));
    let a = support::send(port, "GET", "/me", &[], "");
    assert!(all_of(&a, "cache-control").is_empty(), "{:?}", a.headers);
    assert!(a.header("set-cookie").is_some_and(|c| c.contains("quillo_session=")), "{:?}", a.headers);
    let a = support::send(port, "GET", "/vary", &[], "");
    assert_eq!((all_of(&a, "vary").len(), a.header("cache-control")), (0, Some("private")), "{:?}", a.headers);
    let a = support::send(port, "GET", "/kept", &[], "");
    assert_eq!((a.header("vary"), a.header("cache-control"), a.header("x-content-type-options")), (Some("Cookie"), Some("private"), Some("nosniff")));
    let a = support::send(port, "GET", "/type", &[], "");
    assert_eq!((a.status, a.header("content-type"), a.body.as_str()), (200, None, "x"));
    let a = support::send(port, "GET", "/e", &[], "");
    assert_eq!((a.header("content-type"), a.header("cache-control")), (Some("text/event-stream"), None));
    assert_eq!(support::send(port, "GET", "/seen", &[], "").body, r#"{"x-a":null,"x-b":"b"}"#);
    assert_eq!(support::send(port, "GET", "/es", &[], "").header("content-type"), Some("text/event-stream"));
    assert_eq!(all_of(&support::send(port, "GET", "/two", &[], ""), "set-cookie"), ["a=1", "b=ü"]);
    assert_eq!(all_of(&support::send(port, "GET", "/one", &[], ""), "set-cookie"), ["a=1"]);
    assert!(all_of(&support::send(port, "GET", "/none", &[], ""), "set-cookie").is_empty());
    for (path, want) in [
        ("/list", "`respond` header `x-list` is a str, got list: only `set-cookie` takes a list"),
        ("/map", "`respond` header `x-map` is a str, got map"),
        ("/deep", "`respond` header `set-cookie` takes a list of cookie lines, one a header, got list in it"),
        ("/hole", "got null in it"),
        ("/break", "`respond` header `set-cookie`: names are letters, digits and -, values cannot contain line breaks"),
        ("/sl", "`stream` header `x-l` is a str, got list"),
        ("/sh", "`stream`'s headers are a map, got int"),
        ("/et", "`events`'s type is a str, like \"text/csv\", got int"),
    ] {
        let why = error_of(port, &log, path);
        assert!(why.contains(want), "{}: {}", path, why);
    }
    let a = support::send(port, "GET", "/s", &[], "");
    assert_eq!((a.status, all_of(&a, "set-cookie"), a.header("x-n")), (200, vec!["a=1", "b=2"], Some("1")));
    assert!(all_of(&a, "x-accel-buffering").is_empty(), "{:?}", a.headers);
}

// Q-712: `set_cookie` read only the options it knew, so Express's `sameSite: "Strict"` left the
// cookie `Lax` and `maxAge` made it last the session, and `quillo check` said ok. Now an option it
// does not take is an error, when it runs and in `check` for one written out, naming the snake_case
// option for any spelling of one (`sameSite`, `Max-Age`, `HTTPONLY`) and the options taken for
// others; `delete_cookie` takes them but `max_age`.
#[test]
fn set_cookie_refuses_options_it_does_not_take() {
    let d = dir(
        "cookie-options",
        r#"route GET /all { set_cookie("x", "v", { max_age: 60, path: "/", domain: "example.com", http_only: false, secure: true, same_site: "Strict" }); "ok" }
route GET /empty { set_cookie("x", "v", {}); "ok" }
route GET /nulls { set_cookie("x", "v", { max_age: null, same_site: null }); "ok" }
route GET /del { delete_cookie("x", { path: "/a", domain: "example.com", secure: true, same_site: "Lax", http_only: true }); "ok" }
route GET /zero { delete_cookie("x", { max_age: 0 }); "ok" }
route GET /age/:n { delete_cookie("x", { max_age: int(n) }); "ok" }
"#,
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let cookie = |path: &str| support::send(s.port, "GET", path, &[], "").header("set-cookie").unwrap_or_default().to_string();
    assert_eq!(cookie("/all"), "x=v; Path=/; Domain=example.com; Max-Age=60; Secure; SameSite=Strict");
    assert_eq!(cookie("/empty"), "x=v; Path=/; HttpOnly; Secure; SameSite=Lax");
    assert_eq!(cookie("/nulls"), "x=v; Path=/; HttpOnly; Secure; SameSite=Lax");
    assert_eq!(cookie("/del"), "x=; Path=/a; Domain=example.com; Max-Age=0; HttpOnly; Secure; SameSite=Lax");
    assert_eq!(cookie("/zero"), "x=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax");
    assert_eq!(cookie("/age/0"), "x=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax");
    assert_eq!(support::send(s.port, "GET", "/age/5", &[], "").status, 500);
    let (ok, out) = check("route GET / { set_cookie(\"x\", \"v\", { httpOnly: false, maxAge: 60, sameSite: \"Strict\" }); \"ok\" }\nroute GET /d { delete_cookie(\"x\", { max_age: 5, expires: 1 }); \"ok\" }\nroute GET /k { set_cookie(\"x\", \"v\", { max_age: 5, path: \"/\" }); delete_cookie(\"x\", { path: \"/\", max_age: 0 }); delete_cookie(\"x\", { max_age: null }); delete_cookie(\"x\", { max_age: int(\"5\") }); \"ok\" }\n");
    assert!(!ok, "{}", out);
    for want in [
        "main.qwe:1: error: `set_cookie` has no option `httpOnly`: write it `http_only`",
        "main.qwe:1: error: `set_cookie` has no option `maxAge`: write it `max_age`",
        "main.qwe:1: error: `set_cookie` has no option `sameSite`: write it `same_site`",
        "main.qwe:2: error: `delete_cookie` has no option `max_age` but 0: a deleted cookie's age is 0",
        "main.qwe:2: error: `delete_cookie` has no option `expires`: it takes `path`, `domain`, `http_only`, `secure` and `same_site`",
    ] {
        assert!(out.contains(want), "{} not in {}", want, out);
    }
    assert!(!out.contains("main.qwe:3:"), "{}", out);
    // When it runs, the map made as it runs.
    let d = dir("cookie-options-run", "let o = json.parse(env(\"O\") ?? \"null\")\nset_cookie(\"x\", env(\"V\") ?? \"v\", o)\nprint(\"ok\")\n");
    for (o, want) in [
        (r#"{"sameSite": "Strict"}"#, "`set_cookie` has no option `sameSite`: write it `same_site`"),
        (r#"{"Max-Age": 1}"#, "`set_cookie` has no option `Max-Age`: write it `max_age`"),
        (r#"{"HTTPONLY": true}"#, "`set_cookie` has no option `HTTPONLY`: write it `http_only`"),
        (r#"{"expires": "2027-01-01"}"#, "`set_cookie` has no option `expires`: it takes `max_age`, `path`, `domain`, `http_only`, `secure` and `same_site` (`max_age` is how long it lasts"),
        (r#"{"ünï": 1}"#, "`set_cookie` has no option `ünï`: it takes `max_age`"),
        (r#"{"": 1}"#, "`set_cookie` has no option ``: it takes"),
        (r#"{"same_site": "Strict", "secure": true}"#, "ok"),
        ("{}", "ok"),
    ] {
        let out = script_with_env(&d, "main.qwe", &[("O", o)]);
        assert!(out.contains(want), "{}: {}", o, out);
    }
}

// Q-713: `__Host-` and `__Secure-` cookies went out breaking their prefix's rules, which browsers
// drop: a `__Host-` one with a path, a domain or `secure: false`, a `__Secure-` one with `secure:
// false`, and either without `Secure` under `--watch`. Now those options are an error, and a
// prefixed name (in any case, as browsers read it) is `Secure`, a `__Host-` one at `Path=/`, under
// `--watch` too; other names are as they were.
#[test]
fn a_prefixed_cookie_keeps_its_prefixs_rules() {
    let src = r#"route GET /host { set_cookie("__Host-id", "v"); "ok" }
route GET /host-root { set_cookie("__Host-id", "v", { path: "/" }); "ok" }
route GET /host-lower { set_cookie("__host-id", "v"); "ok" }
route GET /secure { set_cookie("__Secure-t", "v", { path: "/x", domain: "example.com" }); "ok" }
route GET /plain { set_cookie("__Hostx", "v"); "ok" }
route GET /del { delete_cookie("__Host-id"); "ok" }
route GET /c/:n ? { o: str } = { set_cookie(n, "v", json.parse(o)); "ok" }
"#;
    let d = dir("prefixes", src);
    let (_g, port, log) = serve_logged(&d, &[]);
    let cookie = |path: &str| support::send(port, "GET", path, &[], "").header("set-cookie").unwrap_or_default().to_string();
    assert_eq!(cookie("/host"), "__Host-id=v; Path=/; HttpOnly; Secure; SameSite=Lax");
    assert_eq!(cookie("/host-root"), "__Host-id=v; Path=/; HttpOnly; Secure; SameSite=Lax");
    assert_eq!(cookie("/host-lower"), "__host-id=v; Path=/; HttpOnly; Secure; SameSite=Lax");
    assert_eq!(cookie("/secure"), "__Secure-t=v; Path=/x; Domain=example.com; HttpOnly; Secure; SameSite=Lax");
    assert_eq!(cookie("/plain"), "__Hostx=v; Path=/; HttpOnly; Secure; SameSite=Lax");
    assert_eq!(cookie("/del"), "__Host-id=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax");
    for (name, o, want) in [
        ("__Host-a", r#"{"path": "/app"}"#, "a `__Host-` cookie's path is /, as browsers take it no other way, got \"/app\""),
        ("__Host-b", r#"{"path": "//"}"#, "a `__Host-` cookie's path is /"),
        ("__Host-c", r#"{"domain": "example.com"}"#, "a `__Host-` cookie has no domain"),
        ("__Host-d", r#"{"secure": false}"#, "a `__Host-` cookie is `Secure`"),
        ("__HOST-e", r#"{"secure": false}"#, "a `__Host-` cookie is `Secure`"),
        ("__Secure-f", r#"{"secure": false}"#, "a `__Secure-` cookie is `Secure`"),
    ] {
        let why = error_of(port, &log, &format!("/c/{}?o={}", name, enc(o)));
        assert!(why.contains(want), "{} {}: {}", name, o, why);
    }
    // `secure: true` and a null are what it is anyway.
    let a = support::send(port, "GET", &format!("/c/__Secure-g?o={}", enc(r#"{"secure": true, "path": null}"#)), &[], "");
    assert_eq!(a.header("set-cookie"), Some("__Secure-g=v; Path=/; HttpOnly; Secure; SameSite=Lax"));
    // Under --watch, served over plain HTTP: a prefixed name is still `Secure`, which browsers take
    // from localhost; another is not.
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let cookie = |path: &str| support::send(s.port, "GET", path, &[], "").header("set-cookie").unwrap_or_default().to_string();
    assert_eq!(cookie("/host"), "__Host-id=v; Path=/; HttpOnly; Secure; SameSite=Lax");
    assert_eq!(cookie("/secure"), "__Secure-t=v; Path=/x; Domain=example.com; HttpOnly; Secure; SameSite=Lax");
    assert_eq!(cookie("/plain"), "__Hostx=v; Path=/; HttpOnly; SameSite=Lax");
    assert_eq!(cookie("/del"), "__Host-id=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax");
}

// Q-714: a cookie queued twice kept only the last by name alone, so `delete_cookie("theme", {
// path: "/old" })` then `set_cookie("theme", "dark")` sent only the new one, and the old cookie
// stayed. Now one replaces another only at the same name, path and domain (a domain in any case,
// with or without its leading `.`), as browsers keep them.
#[test]
fn a_cookie_replaces_one_only_at_its_name_path_and_domain() {
    let d = dir(
        "cookie-keys",
        r#"route GET /move { delete_cookie("theme", { path: "/old" }); set_cookie("theme", "dark"); "ok" }
route GET /paths { set_cookie("t", "1", { path: "/a" }); set_cookie("t", "2", { path: "/b" }); "ok" }
route GET /again { set_cookie("t", "1", { path: "/a" }); set_cookie("t", "2", { path: "/a" }); "ok" }
route GET /domains { set_cookie("t", "1", { domain: "Example.com" }); set_cookie("t", "2", { domain: ".example.com" }); set_cookie("t", "3", { domain: "a.example.com" }); "ok" }
route GET /names { set_cookie("t", "1"); set_cookie("T", "2"); set_cookie("t", "ü"); "ok" }
route GET /session { set_session({ a: 1 }); set_session({ a: 2 }); "ok" }
"#,
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let cookies = |path: &str| -> Vec<String> { all_of(&support::send(s.port, "GET", path, &[], ""), "set-cookie").iter().map(|c| c.to_string()).collect() };
    assert_eq!(cookies("/move"), ["theme=; Path=/old; Max-Age=0; HttpOnly; SameSite=Lax", "theme=dark; Path=/; HttpOnly; Secure; SameSite=Lax"]);
    assert_eq!(cookies("/paths"), ["t=1; Path=/a; HttpOnly; Secure; SameSite=Lax", "t=2; Path=/b; HttpOnly; Secure; SameSite=Lax"]);
    assert_eq!(cookies("/again"), ["t=2; Path=/a; HttpOnly; Secure; SameSite=Lax"]);
    assert_eq!(cookies("/domains"), ["t=2; Path=/; Domain=.example.com; HttpOnly; Secure; SameSite=Lax", "t=3; Path=/; Domain=a.example.com; HttpOnly; Secure; SameSite=Lax"]);
    assert_eq!(cookies("/names"), ["T=2; Path=/; HttpOnly; Secure; SameSite=Lax", "t=%C3%BC; Path=/; HttpOnly; Secure; SameSite=Lax"]);
    let session = cookies("/session");
    assert_eq!(session.len(), 1, "{:?}", session);
}

fn files_site(name: &str) -> PathBuf {
    let d = dir(
        name,
        "route GET /build/*p = files(\"dist\", p, { cache: \"1y\" })\nroute GET /plain/*p = files(\"dist\", p)\nroute GET /bare/*p = files(\"dist\", p, { not_found: null })\nroute GET /given = files(\"dist\", \"a.txt/\")\n",
    );
    std::fs::create_dir_all(d.join("dist/docs")).unwrap();
    std::fs::write(d.join("dist/404.html"), "<h1>missing</h1>\n").unwrap();
    std::fs::write(d.join("dist/a.txt"), "hello\n").unwrap();
    std::fs::write(d.join("dist/about.html"), "<p>about</p>\n").unwrap();
    std::fs::write(d.join("dist/é.txt"), "accent\n").unwrap();
    std::fs::write(d.join("dist/docs/index.html"), "<p>docs</p>\n").unwrap();
    d
}

// Q-715: `files()` with `cache` sent its 404 page `public, max-age=31536000, immutable`, so a file
// asked for before it was deployed stayed missing in caches for a year. Now the page of a miss is
// `no-cache`, with `cache` or without, and a file found keeps `cache`'s year.
#[test]
fn files_page_of_a_miss_is_not_kept() {
    let d = files_site("miss-cache");
    let s = support::serve(&d.join("main.qwe"), &[]);
    for path in ["/build/app.2.js", "/plain/app.2.js", "/build/x/y.css"] {
        let a = support::send(s.port, "GET", path, &[], "");
        assert_eq!((a.status, a.body.as_str()), (404, "<h1>missing</h1>\n"), "{}", path);
        assert_eq!(all_of(&a, "cache-control"), ["no-cache"], "{}", path);
        assert_eq!((a.header("etag"), a.header("last-modified"), a.header("accept-ranges")), (None, None, None), "{}", path);
    }
    let a = support::send(s.port, "GET", "/build/a.txt", &[], "");
    assert_eq!((a.status, a.header("cache-control")), (200, Some("public, max-age=31536000, immutable")));
    let a = support::send(s.port, "GET", "/bare/app.2.js", &[], "");
    assert_eq!((a.status, a.body.as_str()), (404, "not found"));
}

// Q-721: `files()` served a file at its path with a `/` added, `/s/a.txt/` and `/s/about/`, whose
// relative links then resolved under the wrong folder. The user's decision: a program's URLs are
// its own, so `files()` in a route serves what is asked for as it did, a site that keeps its URLs
// with a `/` at the end and a route declared so too, and a file kept back is the miss one not there
// is, a `/` after it or not. `quillo serve`, whose URLs are its own, moves a file asked for so to
// its path without the `/`, for an hour, only when it would serve it there.
#[test]
fn a_programs_files_serve_a_path_as_its_routes_ask() {
    let d = files_site("file-slash");
    let s = support::serve(&d.join("main.qwe"), &[]);
    for (path, status, body) in [
        ("/build/a.txt/", 200, "hello\n"),
        ("/build/about/", 200, "<p>about</p>\n"),
        ("/build/%C3%A9.txt/", 200, "accent\n"),
        ("/build/a.txt", 200, "hello\n"),
        ("/build/docs/", 200, "<p>docs</p>\n"),
        ("/build/nope/", 404, "<h1>missing</h1>\n"),
        ("/given", 200, "hello\n"),
    ] {
        let a = support::send(s.port, "GET", path, &[], "");
        assert_eq!((a.status, a.body.as_str()), (status, body), "{}", path);
    }
    let a = support::send(s.port, "GET", "/build/docs", &[], "");
    assert_eq!((a.status, a.header("location")), (301, Some("/build/docs/")));
    // A site whose URLs end in `/`: no loop.
    let d = dir("slash-site", "route GET /*p = ends_with(request.path, \"/\") ? files(\"public\", p) : redirect(request.path + \"/\", 301)\n");
    std::fs::create_dir_all(d.join("public")).unwrap();
    std::fs::write(d.join("public/about.html"), "<p>about</p>\n").unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let a = support::send(s.port, "GET", "/about", &[], "");
    assert_eq!((a.status, a.header("location")), (301, Some("/about/")));
    let a = support::send(s.port, "GET", "/about/", &[], "");
    assert_eq!((a.status, a.body.as_str()), (200, "<p>about</p>\n"));
    // A route declared with a `/`, and a database kept back: the same miss as a file not there.
    let d = dir("slash-route", "route GET /about/ = files(\"s\", \"about\")\nroute GET /s/*p = files(\"s\", p)\n");
    std::fs::create_dir_all(d.join("s")).unwrap();
    std::fs::write(d.join("s/about.html"), "<p>s</p>\n").unwrap();
    rusqlite::Connection::open(d.join("s/app.db")).unwrap().execute_batch("create table t (x)").unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    assert_eq!(support::send(s.port, "GET", "/about/", &[], "").body, "<p>s</p>\n");
    let kept: Vec<(u16, String)> = ["/s/app.db/", "/s/nothere.db/", "/s/app.db", "/s/nothere.db"].iter().map(|p| { let a = support::send(s.port, "GET", p, &[], ""); (a.status, a.body) }).collect();
    assert!(kept.iter().all(|k| k == &kept[0]) && kept[0].0 == 404, "{:?}", kept);
}

#[cfg(unix)]
#[test]
fn quillo_serve_moves_a_file_asked_for_as_a_folder() {
    let d = files_site("serve-slash");
    std::fs::write(d.join("dist/.env"), "SECRET=s3cr3t\n").unwrap();
    std::fs::hard_link(d.join("dist/.env"), d.join("dist/notes.txt")).unwrap();
    rusqlite::Connection::open(d.join("dist/app.db")).unwrap().execute_batch("create table t (x)").unwrap();
    let (_g, port) = serve_folder(&d, "dist");
    for (path, to) in [
        ("/a.txt/", "/a.txt"),
        ("/about/?q=1", "/about?q=1"),
        ("/a.txt//", "/a.txt"),
        ("/%C3%A9.txt/", "/%C3%A9.txt"),
        ("/about.html/", "/about.html"),
    ] {
        let a = support::send(port, "GET", path, &[], "");
        assert_eq!((a.status, a.header("location"), a.header("cache-control")), (301, Some(to), Some("max-age=3600")), "{}", path);
    }
    let a = support::send(port, "GET", "/docs", &[], "");
    assert_eq!((a.status, a.header("location")), (301, Some("/docs/")));
    for (path, status, body) in [("/a.txt", 200, "hello\n"), ("/about", 200, "<p>about</p>\n"), ("/docs/", 200, "<p>docs</p>\n")] {
        let a = support::send(port, "GET", path, &[], "");
        assert_eq!((a.status, a.body.as_str()), (status, body), "{}", path);
    }
    // What it would not serve says nothing more with a `/` after it: the miss of a file not there.
    let missing = support::send(port, "GET", "/nothere.db/", &[], "");
    assert_eq!((missing.status, missing.body.as_str()), (404, "<h1>missing</h1>\n"));
    for path in ["/app.db/", "/notes.txt/", "/.env/", "/nothere.txt/", "/app.db", "/notes.txt"] {
        let a = support::send(port, "GET", path, &[], "");
        assert_eq!((a.status, a.body.as_str(), a.header("location")), (404, missing.body.as_str(), None), "{}", path);
    }
}

// Q-722: a miss asked for with a range or a conditional got the plain `not found`: they were
// applied to the 404 page, and its 206, 304 or 412 thrown away. Now the page is sent whole, whatever
// they are; a file found still answers them.
#[test]
fn files_page_of_a_miss_is_whole_whatever_the_request_asks() {
    let d = files_site("miss-whole");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let page_etag = support::send(s.port, "GET", "/plain/404.html", &[], "").header("etag").unwrap().to_string();
    for h in [
        ("Range", "bytes=0-1"),
        ("Range", "bytes=-1"),
        ("Range", "bytes=100-"),
        ("If-None-Match", "*"),
        ("If-None-Match", page_etag.as_str()),
        ("If-Modified-Since", "Thu, 01 Jan 2099 00:00:00 GMT"),
        ("If-Match", "\"other\""),
        ("If-Unmodified-Since", "Thu, 01 Jan 1970 00:00:00 GMT"),
    ] {
        let a = support::send(s.port, "GET", "/plain/nope.txt", &[h], "");
        assert_eq!((a.status, a.body.as_str(), a.header("content-range")), (404, "<h1>missing</h1>\n", None), "{:?}", h);
    }
    let a = support::send(s.port, "GET", "/plain/a.txt", &[("Range", "bytes=0-1")], "");
    assert_eq!((a.status, a.body.as_str()), (206, "he"));
    let etag = support::send(s.port, "GET", "/plain/a.txt", &[], "").header("etag").unwrap().to_string();
    assert_eq!(support::send(s.port, "GET", "/plain/a.txt", &[("If-None-Match", &etag)], "").status, 304);
}

/// A multipart body of `fields` text parts and `files` file parts.
fn multipart(fields: usize, files: usize) -> String {
    let mut b = String::new();
    for i in 0..fields {
        b.push_str(&format!("--XyZ\r\nContent-Disposition: form-data; name=\"f{}\"\r\n\r\nv{}\r\n", i, i));
    }
    for i in 0..files {
        b.push_str(&format!("--XyZ\r\nContent-Disposition: form-data; name=\"up{}\"; filename=\"a{}.txt\"\r\nContent-Type: text/plain\r\n\r\nbody{}\r\n", i, i, i));
    }
    b.push_str("--XyZ--\r\n");
    b
}

const FORM: (&str, &str) = ("Content-Type", "multipart/form-data; boundary=XyZ");

// Q-716: a multipart body of more than 1,000 parts had those past the 1,000th dropped, and the
// answer was a 200. Now it is a 413 before the route runs; 1,000 parts, fields and files together,
// are read whole.
#[test]
fn a_form_of_more_parts_than_are_read_is_too_large() {
    let d = dir("parts", "route POST /up = { fields: len(request.form), files: len(request.files) }\nroute POST /fn = \"ran\"\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    for (fields, files, want) in [
        (1000, 0, Some((1000, 0))),
        (999, 1, Some((999, 1))),
        (995, 5, Some((995, 5))),
        (0, 0, Some((0, 0))),
        (0, 2, Some((0, 2))),
        (1001, 0, None),
        (999, 5, None),
        (1500, 0, None),
    ] {
        let a = support::send(s.port, "POST", "/up", &[FORM], &multipart(fields, files));
        match want {
            Some((f, n)) => {
                let j: serde_json::Value = serde_json::from_str(&a.body).unwrap_or_else(|e| panic!("{}: {}", e, a.body));
                assert_eq!((a.status, j["fields"].as_u64(), j["files"].as_u64()), (200, Some(f), Some(n)), "{} + {}", fields, files);
            }
            None => assert_eq!((a.status, a.body.as_str()), (413, "a form of more than 1000 parts\n"), "{} + {}", fields, files),
        }
    }
    // The route does not run.
    assert_eq!(support::send(s.port, "POST", "/fn", &[FORM], &multipart(1001, 0)).status, 413);
    assert_eq!(support::send(s.port, "POST", "/fn", &[FORM], &multipart(1000, 0)).body, "ran");
}

/// The file the field `f` of a multipart part with `disposition` is, and the form, as `/name` gives.
fn uploaded(port: u16, disposition: &str) -> serde_json::Value {
    let body = format!("--XyZ\r\nContent-Disposition: {}\r\nContent-Type: text/plain\r\n\r\nbytes\r\n--XyZ--\r\n", disposition);
    let a = support::send(port, "POST", "/name", &[FORM], &body);
    assert_eq!(a.status, 200, "{}", a.body);
    serde_json::from_str(&a.body).unwrap()
}

// Q-720: a filename with `\"` in it, which Go's mime/multipart and curl write for a quote, was cut
// at the backslash, and a part with only RFC 5987's `filename*=UTF-8''...` was a field of the form.
// Now `\"` and `\\` are read as the character they stand for, `filename*` is taken when there is no
// `filename` (UTF-8 or ISO-8859-1), and what browsers send reads as before: a Windows path, `%22`.
#[test]
fn a_multipart_filename_is_read_as_other_clients_write_it() {
    let d = dir("filenames", "route POST /name = { files: request.files, form: request.form }\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    for (disposition, name) in [
        (r#"form-data; name="f"; filename="a\"b.txt""#, "a\"b.txt"),
        (r#"form-data; name="f"; filename="\"q\".txt""#, "\"q\".txt"),
        (r#"form-data; name="f"; filename="a\\b.txt""#, "b.txt"),
        (r#"form-data; name="f"; filename*=UTF-8''%E2%82%AC%20rates.txt"#, "€ rates.txt"),
        (r#"form-data; name="f"; filename*=utf-8'en'%C3%BC.txt"#, "ü.txt"),
        (r#"form-data; name="f"; filename*=ISO-8859-1''caf%E9.txt"#, "café.txt"),
        (r#"form-data; name="f"; filename="plain.txt"; filename*=UTF-8''other.txt"#, "plain.txt"),
        (r#"form-data; name="f"; filename="C:\Users\me\x.txt""#, "x.txt"),
        (r#"form-data; name="f"; filename="%22q%22.txt""#, "\"q\".txt"),
        (r#"form-data; name="f"; filename="ünï.txt""#, "ünï.txt"),
        (r#"form-data; name="f"; filename="a\b.txt""#, "b.txt"),
        (r#"form-data; name="f"; filename="dir\""#, ""),
    ] {
        let j = uploaded(s.port, disposition);
        assert_eq!(j["files"]["f"]["filename"].as_str(), Some(name), "{}: {}", disposition, j);
        assert_eq!(j["form"], serde_json::json!({}), "{}", disposition);
    }
    // A field named with a `\` at its end, as a browser sends it, `\"` followed by the next
    // parameter: its quote ends the name.
    let j = uploaded(s.port, r#"form-data; name="f\"; filename="a.txt""#);
    assert_eq!((j["files"]["f\\"]["filename"].as_str(), &j["form"]), (Some("a.txt"), &serde_json::json!({})), "{}", j);
    let j = uploaded(s.port, r#"form-data; name="f\" ;  filename*=UTF-8''%C3%BC.txt"#);
    assert_eq!(j["files"]["f\\"]["filename"].as_str(), Some("ü.txt"), "{}", j);
    // A field is still a field.
    let j = uploaded(s.port, r#"form-data; name="f""#);
    assert_eq!((j["form"]["f"].as_str(), &j["files"]), (Some("bytes"), &serde_json::json!({})));
}

// Q-717: a form whose type was not in lower case read as empty. Now the type is compared by its
// essence in any case, with parameters or without; another type is no form, as before.
#[test]
fn a_forms_type_is_read_in_any_case() {
    let d = dir("form-type", "route POST /form = request.form\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    for (ctype, want) in [
        ("Application/x-www-form-urlencoded", r#"{"a":"1","b":"ü"}"#),
        ("APPLICATION/X-WWW-FORM-URLENCODED; charset=UTF-8", r#"{"a":"1","b":"ü"}"#),
        ("application/x-www-form-urlencoded ;charset=utf-8", r#"{"a":"1","b":"ü"}"#),
        ("application/x-www-form-urlencoded", r#"{"a":"1","b":"ü"}"#),
        ("application/x-www-form-urlencodedx", "{}"),
        ("text/plain", "{}"),
    ] {
        let a = support::send(s.port, "POST", "/form", &[("Content-Type", ctype)], "a=1&b=%C3%BC");
        assert_eq!(a.status, 200, "{}", ctype);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&a.body).unwrap(), serde_json::from_str::<serde_json::Value>(want).unwrap(), "{}", ctype);
    }
}

// Q-718: `Cookie: a=admin; a=root` gave `root`, though a browser sends the cookie with the longer
// path first; `a = 1` gave the name `a ` and the value ` 1`, and `a="q"` kept its quotes. Now the
// first of a name is kept, spaces around `=` are not part of either, and one pair of quotes around
// a value is taken off, for `request.cookies`, `cookie()` and the session's.
#[test]
fn the_cookie_header_is_read_as_browsers_send_it() {
    let d = dir("cookie-header", "route GET /c = request.cookies\nroute GET /one = cookie(\"a\") ?? \"none\"\nroute GET /in { set_session({ u: 7 }); \"in\" }\nroute GET /who = str(session().u)\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    for (header, want) in [
        ("a=admin; a=root", serde_json::json!({ "a": "admin" })),
        ("a = 1 ; b= 2", serde_json::json!({ "a": "1", "b": "2" })),
        ("a=\"q\"", serde_json::json!({ "a": "q" })),
        ("a=\"\"", serde_json::json!({ "a": "" })),
        ("a=\"", serde_json::json!({ "a": "\"" })),
        ("a=\"\"q\"\"", serde_json::json!({ "a": "\"q\"" })),
        ("a=\"caf%C3%A9\"", serde_json::json!({ "a": "café" })),
        ("a=1;b=2;;", serde_json::json!({ "a": "1", "b": "2" })),
        ("a", serde_json::json!({})),
        ("", serde_json::json!({})),
    ] {
        let a = support::send(s.port, "GET", "/c", &[("Cookie", header)], "");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&a.body).unwrap(), want, "{:?}", header);
    }
    assert_eq!(support::send(s.port, "GET", "/one", &[("Cookie", "a=admin; a=root")], "").body, "admin");
    // The session's cookie read the same way: with spaces around `=`, and quoted.
    let line = support::send(s.port, "GET", "/in", &[], "").header("set-cookie").unwrap().to_string();
    let (name, value) = line.split(';').next().unwrap().split_once('=').unwrap();
    for header in [format!("{}={}", name, value), format!("{} = {}", name, value), format!("{}=\"{}\"", name, value)] {
        assert_eq!(support::send(s.port, "GET", "/who", &[("Cookie", &header)], "").body, "7", "{}", header);
    }
}

// Q-719: `redirect()` took any status from 300 to 399, `304 Not Modified` with a `Location` too,
// and wrote `Location` as given, `/a b` and `/a"><script>` too. Now it takes 301, 302, 303, 307 and
// 308, and percent-encodes what a URL cannot hold, leaving a `%XX` already there and what a URL
// holds as they are.
#[test]
fn redirect_takes_redirect_statuses_and_a_url_location_holds() {
    let d = dir("redirects", "route GET /r/:n = redirect(\"/x\", int(n))\nroute GET /d = redirect(\"/x\")\nroute GET /to ? { u: str } = redirect(u)\n");
    let (_g, port, log) = serve_logged(&d, &[]);
    for n in [301, 302, 303, 307, 308] {
        let a = support::send(port, "GET", &format!("/r/{}", n), &[], "");
        assert_eq!((a.status, a.header("location")), (n, Some("/x")));
    }
    assert_eq!(support::send(port, "GET", "/d", &[], "").status, 303);
    for n in [200, 300, 304, 305, 306, 309, 399] {
        let why = error_of(port, &log, &format!("/r/{}", n));
        assert!(why.contains(&format!("`redirect` status is 301, 302, 303, 307 or 308, got {}", n)), "{}", why);
    }
    for (u, want) in [
        ("/a b", "/a%20b"),
        ("/a\"><script>", "/a%22%3E%3Cscript%3E"),
        ("/%41", "/%41"),
        ("/%c3%a9", "/%c3%a9"),
        ("/100%", "/100%25"),
        ("/%zz", "/%25zz"),
        ("/%4", "/%254"),
        ("/é", "/%C3%A9"),
        ("/a\\b", "/a/b"),
        ("/{x}|^`", "/%7Bx%7D%7C%5E%60"),
        ("https://example.com/p?q=a&b=c#f", "https://example.com/p?q=a&b=c#f"),
        ("https://bücher.de/x?q=ü", "https://xn--bcher-kva.de/x?q=%C3%BC"),
        ("https://user:pä@bücher.de:8080/ä b", "https://user:p%C3%A4@xn--bcher-kva.de:8080/%C3%A4%20b"),
        ("//bücher.de/x", "//xn--bcher-kva.de/x"),
        ("https://[::1]:8080/é", "https://[::1]:8080/%C3%A9"),
        ("https://example.com\\é", "https://example.com/%C3%A9"),
        // A `\` as browsers read it in these schemes, before the query: a `/`; after, and in
        // another scheme, a character, encoded.
        ("https://good.com\\@evil.com/", "https://good.com/@evil.com/"),
        ("HTTPS://good.com\\@evil.com/", "HTTPS://good.com/@evil.com/"),
        ("//good.com\\@evil.com", "//good.com/@evil.com"),
        ("/\\evil.com", "//evil.com"),
        ("\\\\evil.com", "//evil.com"),
        ("wss://good.com\\@evil.com", "wss://good.com/@evil.com"),
        ("https://good.com/a\\b?c\\d#e\\f", "https://good.com/a/b?c%5Cd#e%5Cf"),
        ("mailto:a\\b", "mailto:a%5Cb"),
        ("x-app://good.com\\@evil.com", "x-app://good.com%5C@evil.com"),
        ("/x?u=https://bücher.de", "/x?u=https://b%C3%BCcher.de"),
        ("/p?x=[1]&y=@~!$'()*+,;=:", "/p?x=[1]&y=@~!$'()*+,;=:"),
    ] {
        let a = support::send(port, "GET", &format!("/to?u={}", enc(u)), &[], "");
        assert_eq!((a.status, a.header("location")), (303, Some(want)), "{}", u);
    }
    // A host IDNA cannot write is no redirect: an error, where the host went as it was.
    for u in ["https://ü%.com/", "//ü%.com/x"] {
        let a = support::send(port, "GET", &format!("/to?u={}", enc(u)), &[], "");
        assert_eq!((a.status, a.header("location")), (500, None), "{}", u);
    }
    // Tabs and line breaks, which browsers strip, are refused.
    for u in ["https://good.com\t@evil.com", "/\n/evil.com"] {
        assert_eq!(support::send(port, "GET", &format!("/to?u={}", enc(u)), &[], "").status, 500, "{:?}", u);
    }
}

/// The host a `Location` takes a browser to, as WHATWG reads it (a `\` as `/`), for the tests.
fn host_of(location: &str) -> String {
    let rest = location.split_once("://").map_or(location.strip_prefix("//").unwrap_or(""), |(_, r)| r).replace('\\', "/");
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    host.split(':').next().unwrap_or("").to_ascii_lowercase()
}

// Found in review: a program that checks where a redirect goes and then redirects goes where the
// check meant, as a browser reads the URL: `https://good.com\@evil.com/` passes a check for
// good.com, and `%5C` would have sent the browser to evil.com.
#[test]
fn a_checked_redirect_goes_where_the_check_meant() {
    let d = dir("redirect-checked", "route GET /go ? { u: str } = starts_with(u, \"https://good.com\") || starts_with(u, \"//good.com\") ? redirect(u) : \"no\"\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    for u in ["https://good.com\\@evil.com/", "https://good.com\\\\@evil.com", "//good.com\\@evil.com/x", "https://good.com\\.evil.com", "https://good.com/\\@evil.com", "https://good.com"] {
        let a = support::send(s.port, "GET", &format!("/go?u={}", enc(u)), &[], "");
        let to = a.header("location").unwrap_or_else(|| panic!("{}: {}", u, a.status));
        assert_eq!((a.status, host_of(to)), (303, "good.com".to_string()), "{} -> {}", u, to);
    }
    assert_eq!(support::send(s.port, "GET", &format!("/go?u={}", enc("https://evil.com")), &[], "").body, "no");
}

const NAMED_NULL: &str = r#"component Counter() {
    state n = 0
    <button @click={() => { n = n + 1 }}>{n}</button>
}
route GET /plain = <html><body><Counter/></body></html>
route GET /etag = respond(200, <html><body><Counter/></body></html>, { etag: null })
route GET /range = respond(200, <html><body><Counter/></body></html>, { "content-range": null })
route GET /srv = respond(200, "x", { server: null })
route GET /srv2 = respond(200, "x", { server: "mine" })
route GET /pf = respond(200, <html><body><a href="/plain">p</a></body></html>, { "set-cookie": null })
route GET /pf2 = respond(200, <html><body><a href="/plain">p</a></body></html>, { "set-cookie": "a=1" })
route GET /back { let r = respond(200, "x", { "set-cookie": ["a=1", "b=2"], "x-n": null }); respond(r.status, r.body, r.headers) }
route GET /h = respond(200, "x", { "set-cookie": ["a=1", "b=2"], "x-n": null, "x-k": "v" }).headers
route GET /one = respond(200, "x", { "set-cookie": "a=1" }).headers
"#;

// Found in review: a header named `null` holds only its name's place. An `etag` or a
// `content-range` named so made a page "as written", without its component bundle or the
// `--watch` reload script; `server: null` still sent `server: Quillo`; `set-cookie: null` had
// every prefetch refused; and a response's `.headers` kept one `set-cookie` of several, so
// `respond(r.status, r.body, r.headers)` lost the others.
#[test]
fn a_header_named_null_is_no_header_anywhere() {
    let d = dir("named-null", NAMED_NULL);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |path: &str, h: &[(&str, &str)]| support::send(s.port, "GET", path, h, "");
    let plain = get("/plain", &[]).body;
    assert!(plain.contains("</script></body>"), "{}", plain);
    for path in ["/etag", "/range"] {
        let a = get(path, &[]);
        assert_eq!((a.status, a.header("etag"), a.header("content-range"), &a.body), (200, None, None, &plain), "{}", path);
    }
    assert_eq!(get("/srv", &[]).header("server"), None);
    assert_eq!(all_of(&get("/srv2", &[]), "server"), ["mine"]);
    assert_eq!(get("/plain", &[]).header("server"), Some("Quillo"));
    for h in [("Sec-Purpose", "prefetch"), ("x-quillo-prefetch", "1")] {
        assert_eq!(get("/pf", &[h]).status, 200, "{:?}", h);
        assert_eq!(get("/pf2", &[h]).status, 503, "{:?}", h);
    }
    assert_eq!(all_of(&get("/back", &[]), "set-cookie"), ["a=1", "b=2"]);
    assert_eq!(get("/back", &[]).header("x-n"), None);
    assert_eq!(get("/h", &[]).body, r#"{"set-cookie":["a=1","b=2"],"x-n":null,"x-k":"v"}"#);
    assert_eq!(get("/one", &[]).body, r#"{"set-cookie":"a=1"}"#);
    // Under --watch, the reload script goes in as on any page.
    let w = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    for path in ["/plain", "/etag", "/range"] {
        let a = support::send(w.port, "GET", path, &[], "");
        assert!(a.body.contains("/_quillo/events?v="), "{}: {}", path, a.body);
    }
}

// Q-723: a POST with a matching `If-None-Match` got a 304, where RFC 9110 says 412; a range end
// past 2^64 gave the whole file as a 200; `Bytes=0-1` was let go and `bytes=+2-4` taken. Now each is
// as RFC 9110 has it, and what was right stays. `If-Match` and `If-Range` take the file's own
// (weak) tag, the user's decision.
#[test]
fn ranges_and_conditionals_are_as_rfc_9110_has_them() {
    let d = dir("rfc9110", "route GET /f = file(\"a.txt\")\nroute POST /f = file(\"a.txt\")\nroute GET /big = file(\"big.bin\")\n");
    std::fs::write(d.join("a.txt"), "hello\n").unwrap();
    std::fs::write(d.join("big.bin"), vec![b'b'; 2 << 20]).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let first = support::send(s.port, "GET", "/f", &[], "");
    let (etag, modified) = (first.header("etag").unwrap().to_string(), first.header("last-modified").unwrap().to_string());
    assert!(etag.starts_with("W/\""), "{}", etag);
    let strong = etag.trim_start_matches("W/").to_string();
    let status = |method: &str, path: &str, h: &[(&str, &str)]| support::send(s.port, method, path, h, "").status;
    // The user's decision: the file's own tag, with or without its `W/`, passes If-Match and
    // If-Range, as nginx has it, so a resume or a seek gets its part; another tag does not.
    assert_eq!(status("GET", "/f", &[("If-Match", &etag)]), 200);
    assert_eq!(status("GET", "/f", &[("If-Match", &strong)]), 200);
    assert_eq!(status("GET", "/f", &[("If-Match", "*")]), 200);
    assert_eq!(status("GET", "/f", &[("If-Match", &format!("\"x\", {}", etag))]), 200);
    assert_eq!(status("GET", "/f", &[("If-Match", "\"x\", W/\"y\"")]), 412);
    let resumed = support::send(s.port, "GET", "/f", &[("Range", "bytes=2-"), ("If-Range", &etag)], "");
    assert_eq!((resumed.status, resumed.body.as_str()), (206, "llo\n"));
    assert_eq!(status("GET", "/f", &[("Range", "bytes=0-1"), ("If-Range", &strong)]), 206);
    assert_eq!(status("GET", "/f", &[("Range", "bytes=0-1"), ("If-Range", "W/\"other\"")]), 200);
    assert_eq!(status("GET", "/f", &[("Range", "bytes=0-1"), ("If-Range", "*")]), 200);
    assert_eq!(status("GET", "/f", &[("Range", "bytes=0-1"), ("If-Range", &modified)]), 206);
    assert_eq!(status("GET", "/f", &[("Range", "bytes=0-1"), ("If-Range", "Thu, 01 Jan 2099 00:00:00 GMT")]), 200);
    // If-None-Match: a 304 for GET and HEAD, a 412 for another method; If-Modified-Since only for
    // those two.
    assert_eq!(status("GET", "/f", &[("If-None-Match", &etag)]), 304);
    assert_eq!(status("HEAD", "/f", &[("If-None-Match", &etag)]), 304);
    assert_eq!(status("POST", "/f", &[("If-None-Match", &etag)]), 412);
    assert_eq!(status("POST", "/f", &[("If-None-Match", "*")]), 412);
    assert_eq!(status("POST", "/f", &[("If-None-Match", "\"other\"")]), 200);
    assert_eq!(status("POST", "/f", &[("If-Modified-Since", "Thu, 01 Jan 2099 00:00:00 GMT")]), 200);
    assert_eq!(status("GET", "/f", &[("If-Modified-Since", "Thu, 01 Jan 2099 00:00:00 GMT")]), 304);
    for (range, status, part) in [
        ("bytes=0-99999999999999999999", 206, Some("bytes 0-5/6")),
        ("bytes=0-18446744073709551615", 206, Some("bytes 0-5/6")),
        ("bytes=-99999999999999999999", 206, Some("bytes 0-5/6")),
        ("bytes=99999999999999999999-", 416, Some("bytes */6")),
        ("Bytes=0-1", 206, Some("bytes 0-1/6")),
        ("BYTES=1-2", 206, Some("bytes 1-2/6")),
        ("bytes=0-0", 206, Some("bytes 0-0/6")),
        ("bytes=5-5", 206, Some("bytes 5-5/6")),
        ("bytes=6-", 416, Some("bytes */6")),
        ("bytes=+2-4", 200, None),
        ("bytes=2-+4", 200, None),
        ("bytes=-+1", 200, None),
        ("bytes=0x1-2", 200, None),
        ("bytes=", 200, None),
        ("bytes=-", 200, None),
        ("bytés=0-1", 200, None),
        ("bytes=0-1,3-4", 200, None),
    ] {
        let a = support::send(s.port, "GET", "/f", &[("Range", range)], "");
        assert_eq!((a.status, a.header("content-range")), (status, part), "{}", range);
    }
    // A large file's part, resumed with its tag, and its end past 2^64, from disk.
    let big_tag = support::send(s.port, "HEAD", "/big", &[], "").header("etag").unwrap().to_string();
    let a = support::send(s.port, "GET", "/big", &[("Range", "bytes=1048576-"), ("If-Range", &big_tag)], "");
    assert_eq!((a.status, a.body.len()), (206, 1 << 20));
    let a = support::send(s.port, "GET", "/big", &[("Range", "bytes=1-99999999999999999999")], "");
    assert_eq!((a.status, a.header("content-range"), a.body.len()), (206, Some("bytes 1-2097151/2097152"), (2 << 20) - 1));
}

// Q-724: only the gzipped copy of a compressible answer said `Vary: accept-encoding`, so a shared
// cache that saw a request without `Accept-Encoding` first kept the plain copy for everyone. Now
// each answer that would have been gzipped for a client that takes it says so, once; an answer too
// small to be, or of another type, does not.
#[test]
fn a_compressible_answer_varies_by_accept_encoding() {
    let d = dir(
        "vary",
        "route GET /t/:n = repeat(\"x\", int(n))\nroute GET /j = for i in 0..300 => i\nroute GET /png = respond(200, repeat(\"x\", 2000), { \"content-type\": \"image/png\" })\nroute GET /own = respond(200, repeat(\"x\", 2000), { \"vary\": \"Accept-Encoding\" })\nroute GET /star = respond(200, repeat(\"x\", 2000), { \"vary\": \"*\" })\nroute GET /cookie = `${cookie(\"a\") ?? \"\"}${repeat(\"x\", 2000)}`\nroute GET /s/*p = files(\"pub\", p)\n",
    );
    std::fs::create_dir_all(d.join("pub")).unwrap();
    std::fs::write(d.join("pub/app.css"), ".a { color: red; margin: 0 auto; }\n".repeat(60)).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let gz = ("Accept-Encoding", "gzip");
    let varies = |a: &support::Answer| all_of(a, "vary").iter().filter(|v| v.to_ascii_lowercase().contains("accept-encoding")).count();
    for path in ["/t/2000", "/t/1024", "/j", "/s/app.css"] {
        let plain = support::send(s.port, "GET", path, &[], "");
        assert_eq!((plain.status, plain.header("content-encoding"), varies(&plain)), (200, None, 1), "{}", path);
        let zipped = support::send(s.port, "GET", path, &[gz], "");
        assert_eq!((zipped.header("content-encoding"), varies(&zipped)), (Some("gzip"), 1), "{}", path);
    }
    for path in ["/t/1023", "/t/0", "/png"] {
        for h in [&[][..], &[gz][..]] {
            let a = support::send(s.port, "GET", path, h, "");
            assert_eq!((a.status, a.header("content-encoding"), varies(&a)), (200, None, 0), "{} {:?}", path, h);
        }
    }
    let own = support::send(s.port, "GET", "/own", &[], "");
    assert_eq!(all_of(&own, "vary"), ["Accept-Encoding"]);
    assert_eq!(all_of(&support::send(s.port, "GET", "/star", &[], ""), "vary"), ["*"]);
    let personal = support::send(s.port, "GET", "/cookie", &[], "");
    assert!(all_of(&personal, "vary").contains(&"Cookie") && varies(&personal) == 1, "{:?}", personal.headers);
    // A copy not changed since is no answer of a type.
    let etag = support::send(s.port, "GET", "/s/app.css", &[], "").header("etag").unwrap().to_string();
    let a = support::send(s.port, "GET", "/s/app.css", &[("If-None-Match", &etag)], "");
    assert_eq!(a.status, 304);
}

/// A GET of `path` written on a connection of its own, left unread.
fn asked_and_unread(port: u16, path: &str) -> std::net::TcpStream {
    use std::io::Write;
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(20))).unwrap();
    s.write_all(format!("GET {} HTTP/1.1\r\nHost: x\r\n\r\n", path).as_bytes()).unwrap();
    s
}

/// What comes on `s` up to the end of the first head.
fn head_of(s: &mut std::net::TcpStream) -> String {
    use std::io::Read;
    let mut out = Vec::new();
    let mut b = [0u8; 1];
    while !out.ends_with(b"\r\n\r\n") && s.read(&mut b).unwrap_or(0) == 1 {
        out.push(b[0]);
    }
    String::from_utf8_lossy(&out).into_owned()
}

// Q-725: with eight large sends to one client running, a HEAD of a large file, or a 10-byte range
// of one, was refused with a 503, though neither sends much. Now only what is sent from disk takes
// a place: a HEAD sends no body (and its connection carries on), and a range of up to 1 MiB is sent
// from memory; a larger range and the whole file wait their turn, as before.
#[test]
fn a_head_or_a_small_range_takes_no_place_among_the_large_sends() {
    let d = dir("places", "route GET /big = file(\"big.bin\")\nroute GET / = \"hi\"\n");
    let size: u64 = 64 << 20;
    std::fs::File::create(d.join("big.bin")).unwrap().set_len(size).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let mut held: Vec<std::net::TcpStream> = (0..8).map(|_| asked_and_unread(s.port, "/big")).collect();
    for h in held.iter_mut() {
        assert!(head_of(h).starts_with("HTTP/1.1 200"));
    }
    let a = support::send(s.port, "HEAD", "/big", &[], "");
    assert_eq!((a.status, a.header("content-length"), a.body.as_str()), (200, Some(&*size.to_string()), ""));
    let a = support::send(s.port, "HEAD", "/big", &[("Range", "bytes=0-1048576")], "");
    assert_eq!((a.status, a.header("content-length"), a.header("content-range")), (206, Some("1048577"), Some(&*format!("bytes 0-1048576/{}", size))));
    // Two on one connection: the first leaves it open.
    let out = on_one_connection(s.port, "HEAD /big HTTP/1.1\r\nHost: x\r\n\r\nHEAD /big HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    assert_eq!(out.matches("HTTP/1.1 200 OK").count(), 2, "{}", out);
    assert_eq!(out.matches(&format!("content-length: {}\r\n", size)).count(), 2, "{}", out);
    for (range, len) in [("bytes=0-9", 10), ("bytes=-10", 10), ("bytes=0-1048575", 1 << 20)] {
        let a = support::send(s.port, "GET", "/big", &[("Range", range)], "");
        assert_eq!((a.status, a.body.len()), (206, len), "{}", range);
    }
    for range in ["bytes=0-1048576", "bytes=0-"] {
        assert_eq!(support::send(s.port, "GET", "/big", &[("Range", range)], "").status, 503, "{}", range);
    }
    assert_eq!(support::send(s.port, "GET", "/big", &[], "").status, 503);
    assert_eq!(support::send(s.port, "GET", "/", &[], "").body, "hi");
    drop(held);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let mut s = asked_and_unread(s.port, "/big");
        if head_of(&mut s).starts_with("HTTP/1.1 200") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "no place came free");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

// Q-726: `set_cookie` sent a 5,000-byte cookie, which browsers drop; `file()`'s type errors said a
// str was not a str, and a type with a control character went out as an answer with no
// Content-Type, as `stream`'s did; a cookie path of `/a b` was told it "starts with /". Now each
// says what is wrong: a cookie over 3,900 bytes as sent (as `set_session`'s), an empty type or
// download, a control character, and a path's characters.
#[test]
fn cookies_and_types_say_what_is_wrong_with_them() {
    let d = dir(
        "messages",
        "let o = json.parse(env(\"O\") ?? \"null\")\nset_cookie(\"x\", env(\"V\") ?? \"v\", o)\nprint(\"ok\")\n",
    );
    std::fs::write(d.join("f.qwe"), "let t = env(\"T\") ?? \"text/csv\"\nlet n = json.parse(env(\"D\") ?? \"null\")\nprint(file(\"a.txt\", { type: t, download: n }).status)\n").unwrap();
    std::fs::write(d.join("s.qwe"), "let t = json.parse(env(\"T\") ?? \"null\")\nprint(stream((send) => send(\"x\"), { type: t }).status)\n").unwrap();
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    let says = |file: &str, env: &[(&str, &str)], want: &str| {
        let out = script_with_env(&d, file, env);
        assert!(out.contains(want), "{:?}: {}", env, out);
    };
    let x = |n: usize| "x".repeat(n);
    says("main.qwe", &[("V", &x(3898))], "ok");
    says("main.qwe", &[("V", &x(3899))], "`set_cookie`: the cookie `x` is 3901 bytes as sent (its value percent-encoded), more than a browser keeps with its attributes (3900)");
    says("main.qwe", &[("V", &"é".repeat(649))], "ok");
    says("main.qwe", &[("V", &"é".repeat(650))], "the cookie `x` is 3902 bytes as sent");
    says("main.qwe", &[("V", "")], "ok");
    for (path, want) in [
        (r#""/a b""#, "`set_cookie`'s path cannot hold a space, a `;`, a control character or a letter past ASCII (percent-encode them), got \"/a b\""),
        (r#""/a; Domain=x""#, "path cannot hold a space, a `;`"),
        (r#""/é""#, "path cannot hold a space"),
        (r#""/a\tb""#, "path cannot hold a space"),
        (r#""x""#, "`set_cookie`'s path starts with /, got \"x\""),
        (r#""""#, "`set_cookie`'s path starts with /, got \"\""),
        ("5", "`set_cookie`'s path is a str that starts with /, got int"),
        (r#""/a%20b""#, "ok"),
        (r#""/""#, "ok"),
    ] {
        says("main.qwe", &[("O", &format!("{{\"path\": {}}}", path))], want);
    }
    for (t, want) in [
        ("", "`file`'s type is empty: give a content type, like \"text/csv\", or leave it out"),
        ("  ", "`file`'s type is empty"),
        ("text/plain\r\nx: y", "`file`'s type cannot hold line breaks or other control characters, got \"text/plain\\r\\nx: y\""),
        ("text/plain\u{1}", "`file`'s type cannot hold line breaks or other control characters"),
        ("text/plain\u{7f}", "`file`'s type cannot hold line breaks"),
        ("text/plain;\tcharset=utf-8", "200"),
        ("text/csv", "200"),
        ("text/ünï", "200"),
    ] {
        says("f.qwe", &[("T", t)], want);
    }
    says("f.qwe", &[("D", r#""""#)], "`file`'s download is empty: give the name a browser saves the file as, or leave it out");
    says("f.qwe", &[("D", "3")], "`file`'s download is a str, got 3");
    says("f.qwe", &[("D", r#""r.csv""#)], "200");
    for (t, want) in [
        (r#""a\u0001""#, "`stream`'s type cannot hold line breaks or other control characters, got \"a\\u0001\""),
        (r#""""#, "`stream`'s type is empty"),
        ("5", "`stream`'s type is a str, like \"text/csv\", got int"),
        (r#""text/csv""#, "200"),
        ("null", "200"),
    ] {
        says("s.qwe", &[("T", t)], want);
    }
}

// Q-701: a file saved anew with another length is served as it is now, with its own length and
// tag, from memory only as the copy of what was read: a folder switched to another release and
// back serves each release's own bytes; a small file saved anew as a large one is sent from disk.
#[cfg(unix)]
#[test]
fn a_file_saved_anew_is_served_as_it_is_now() {
    let d = dir("releases", "route GET /s/*p = files(\"public\", p)\n");
    for (f, text) in [("releases/a/app.js", "release A".to_string()), ("releases/b/app.js", "release B, which is longer".to_string()), ("grow.txt", "small".to_string())] {
        std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
        std::fs::write(d.join(f), text).unwrap();
    }
    link("releases/a", &d.join("public"));
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| support::send(s.port, "GET", p, &[], "");
    let switch = |to: &str| {
        link(to, &d.join("public.new"));
        std::fs::rename(d.join("public.new"), d.join("public")).unwrap();
    };
    for (release, want) in [("releases/a", "release A"), ("releases/b", "release B, which is longer"), ("releases/a", "release A"), ("releases/b", "release B, which is longer")] {
        switch(release);
        let a = get("/s/app.js");
        assert_eq!((a.status, a.body.as_str()), (200, want), "{}", release);
        assert_eq!(a.header("content-length"), Some(want.len().to_string().as_str()));
    }
    // Small, then saved anew as a large one, then small again.
    std::fs::write(d.join("releases/b/grow.txt"), "small").unwrap();
    assert_eq!(get("/s/grow.txt").body, "small");
    std::fs::write(d.join("big.tmp"), vec![b'g'; 3 << 20]).unwrap();
    std::fs::rename(d.join("big.tmp"), d.join("releases/b/grow.txt")).unwrap();
    let a = get("/s/grow.txt");
    assert_eq!((a.status, a.body.len()), (200, 3 << 20));
    std::fs::write(d.join("small.tmp"), "small again").unwrap();
    std::fs::rename(d.join("small.tmp"), d.join("releases/b/grow.txt")).unwrap();
    assert_eq!(get("/s/grow.txt").body, "small again");
}

// Q-701, Q-702: `files` answers a conditional, a range and a HEAD of a file it serves, small (from
// memory, or read for the first time) or large, as it answers a GET, and of a file it keeps back
// with a 404: a SQLite database, a hard link to the settings, a link to a database's `-shm`
// (judged by the name the link leads to).
#[cfg(unix)]
#[test]
fn files_answers_conditionals_ranges_and_heads_as_it_serves() {
    let d = dir("conditionals", "db.open(\"app.db\")\ndb.exec(\"create table if not exists t (x text)\")\ndb.exec(\"insert into t values ('SECRET row')\")\nroute GET /s/*p = files(\"public\", p)\n");
    std::fs::create_dir_all(d.join("public")).unwrap();
    std::fs::write(d.join(".env"), "SECRET=1\n").unwrap();
    std::fs::write(d.join("public/small.txt"), "0123456789").unwrap();
    std::fs::write(d.join("public/cold.txt"), "abcdefghij").unwrap();
    std::fs::write(d.join("public/big.bin"), vec![b'b'; 2 << 20]).unwrap();
    rusqlite::Connection::open(d.join("public/data.db")).unwrap().execute_batch("create table t (x text); insert into t values ('SECRET')").unwrap();
    std::fs::hard_link(d.join(".env"), d.join("public/env.txt")).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    link("../app.db-shm", &d.join("public/x.txt"));
    let send = |method: &str, p: &str, h: &[(&str, &str)]| support::send(s.port, method, p, h, "");
    // small.txt read once, so in memory; cold.txt and big.bin first asked with a HEAD, which
    // says what the GET then sends.
    let first = send("GET", "/s/small.txt", &[]);
    let tag = first.header("etag").unwrap().to_string();
    for path in ["/s/small.txt", "/s/cold.txt", "/s/big.bin"] {
        let h = send("HEAD", path, &[]);
        let g = send("GET", path, &[]);
        assert_eq!((h.status, h.body.as_str(), g.status), (200, "", 200), "{}", path);
        for k in ["content-length", "etag", "content-type", "last-modified"] {
            assert_eq!(h.header(k), g.header(k), "{}: {}", path, k);
        }
        let tag = g.header("etag").unwrap().to_string();
        let a = send("GET", path, &[("If-None-Match", &tag)]);
        assert_eq!((a.status, a.body.as_str()), (304, ""), "{}", path);
        let a = send("GET", path, &[("Range", "bytes=2-4")]);
        assert_eq!((a.status, a.body.len()), (206, 3), "{}", path);
        assert_eq!(a.header("content-range").map(|r| r.starts_with("bytes 2-4/")), Some(true), "{}", path);
    }
    // A HEAD with gzip taken, of a file never fetched, says the GET's gzip: its length, its
    // encoding and its Vary (FEEDBACK55.md, Q-701; branch feedback55).
    std::fs::write(d.join("public/cold.css"), "body { color: red }\n".repeat(2000)).unwrap();
    let gz = [("Accept-Encoding", "gzip")];
    let h = send("HEAD", "/s/cold.css", &gz);
    let g = send("GET", "/s/cold.css", &gz);
    assert_eq!(g.header("content-encoding"), Some("gzip"));
    for k in ["content-length", "content-encoding", "vary", "etag"] {
        assert_eq!(h.header(k), g.header(k), "{}", k);
    }
    assert_eq!(send("HEAD", "/s/cold.css", &[]).header("content-length"), Some("40000"));
    assert_eq!(send("GET", "/s/small.txt", &[("Range", "bytes=2-4")]).body, "234");
    assert_eq!(send("GET", "/s/cold.txt", &[("Range", "bytes=2-4")]).body, "cde");
    assert_eq!(send("HEAD", "/s/small.txt", &[]).header("content-length"), Some("10"));
    assert_eq!(tag, send("HEAD", "/s/small.txt", &[]).header("etag").unwrap());
    for path in ["/s/data.db", "/s/env.txt", "/s/x.txt"] {
        for (method, h) in [("GET", vec![("If-None-Match", "*")]), ("GET", vec![("Range", "bytes=0-15")]), ("HEAD", vec![]), ("GET", vec![])] {
            let a = send(method, path, &h);
            assert_eq!(a.status, 404, "{} {} {:?}", method, path, h);
            assert!(!a.body.contains("SECRET") && !a.body.contains("SQLite"), "{}", path);
        }
    }
}

// Q-701: package managers' stores right under a `node_modules` are followed through a link as
// pnpm's is (Deno's `.deno`, Bun's `.bun`, Yarn's `.store`); the same names elsewhere are dot
// folders like another.
#[cfg(unix)]
#[test]
fn files_follows_a_link_into_a_package_managers_store() {
    let d = dir("stores", "route GET /nm/*p = files(\"node_modules\", p)\nroute GET /r/*p = files(\".\", p)\n");
    for store in [".pnpm", ".deno", ".bun", ".store"] {
        let f = format!("node_modules/{}/lib@1.0.0/node_modules/lib/index.js", store);
        std::fs::create_dir_all(d.join(&f).parent().unwrap()).unwrap();
        std::fs::write(d.join(&f), store).unwrap();
        link(&format!("{}/lib@1.0.0/node_modules/lib", store), &d.join(format!("node_modules/lib{}", store)));
        // Not under `node_modules`: kept.
        std::fs::create_dir_all(d.join(format!("other/{}", store))).unwrap();
        std::fs::write(d.join(format!("other/{}/x.js", store)), "SECRET").unwrap();
        link(&format!("other/{}/x.js", store), &d.join(format!("x{}.js", store)));
    }
    let s = support::serve(&d.join("main.qwe"), &[]);
    for store in [".pnpm", ".deno", ".bun", ".store"] {
        let a = support::send(s.port, "GET", &format!("/nm/lib{}/index.js", store), &[], "");
        assert_eq!((a.status, a.body.as_str()), (200, store));
        let a = support::send(s.port, "GET", &format!("/r/node_modules/lib{}/index.js", store), &[], "");
        assert_eq!((a.status, a.body.as_str()), (200, store));
        statuses(s.port, &[(&format!("/r/x{}.js", store), 404), (&format!("/nm/{}/lib@1.0.0/node_modules/lib/index.js", store), 404)], "SECRET");
    }
}

// Q-701: the TLS key and the file `--env-file` names, given as links, are known by where they lead
// now: certbot points `live.pem` at a new key at each renewal, and the new key and the old are
// both kept back, under any name.
#[cfg(unix)]
#[test]
fn a_key_given_as_a_link_is_kept_after_a_renewal() {
    let d = support::temp_dir("feedback55-renewal");
    std::fs::create_dir_all(d.join("site/archive")).unwrap();
    let pem = |n: u8| format!("-----BEGIN PRIVATE KEY-----\nKEY{}\n-----END PRIVATE KEY-----\n", n);
    std::fs::write(d.join("site/archive/k1.pem"), pem(1)).unwrap();
    std::fs::write(d.join("site/archive/k2.pem"), pem(2)).unwrap();
    std::fs::write(d.join("site/cert.pem"), "-----BEGIN CERTIFICATE-----\n").unwrap();
    std::fs::write(d.join("site/index.html"), "<p>site</p>").unwrap();
    std::fs::write(d.join("env1"), "SECRET=1\n").unwrap();
    std::fs::write(d.join("env2"), "SECRET=2\n").unwrap();
    link("archive/k1.pem", &d.join("site/live.pem"));
    link("../env1", &d.join("site/app.env"));
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["serve", "site", "--port", &port.to_string(), "--tls-cert", "site/cert.pem", "--tls-key", "site/live.pem", "--env-file", "site/app.env"])
        .current_dir(&d)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !support::listening(port) {
        assert!(std::time::Instant::now() < deadline, "quillo serve did not start");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let kept = ["/live.pem", "/archive/k1.pem", "/app.env"];
    statuses(port, &kept.iter().map(|p| (*p, 404)).collect::<Vec<_>>(), "KEY");
    assert_eq!(support::send(port, "GET", "/index.html", &[], "").status, 200);
    // Renewed: both links lead to new files now.
    let repoint = |to: &str, at: &str| {
        link(to, &d.join("site/next.tmp"));
        std::fs::rename(d.join("site/next.tmp"), d.join(at)).unwrap();
    };
    repoint("archive/k2.pem", "site/live.pem");
    repoint("../env2", "site/app.env");
    let kept = ["/live.pem", "/archive/k2.pem", "/archive/k1.pem", "/app.env"];
    statuses(port, &kept.iter().map(|p| (*p, 404)).collect::<Vec<_>>(), "KEY");
    statuses(port, &[("/app.env", 404)], "SECRET");
    assert!(support::send(port, "GET", "/cert.pem", &[], "").body.contains("CERTIFICATE"));
}

// Q-701: on macOS a folder named through the firmlink `/System/Volumes/Data` is the same folder:
// its files are served, judged in the one form the system names them in.
#[cfg(target_os = "macos")]
#[test]
fn quillo_serve_takes_a_folder_named_through_the_firmlink() {
    let d = support::temp_dir("feedback55-firmlink").canonicalize().unwrap();
    let through = Path::new("/System/Volumes/Data").join(d.strip_prefix("/").unwrap());
    if !through.is_dir() {
        eprintln!("skipping: no {}", through.display());
        return;
    }
    std::fs::create_dir_all(d.join("site")).unwrap();
    std::fs::write(d.join("site/index.html"), "<p>through</p>").unwrap();
    std::fs::write(d.join("site/.env"), "SECRET=1").unwrap();
    let (_g, port) = serve_folder(Path::new("/"), &through.join("site").to_string_lossy());
    let a = support::send(port, "GET", "/index.html", &[], "");
    assert_eq!((a.status, a.body.as_str()), (200, "<p>through</p>"));
    statuses(port, &[("/.env", 404)], "SECRET");
}

// Q-701: a file being written to in place (rewritten, or appended to) is served as it is, never as
// a miss, by `file` and by `files`, whole, in part and for a HEAD; one renamed over again and again
// is served or, while each look finds another file, a 503 to come back to; a large page of a
// miss renamed over stays a 404.
#[cfg(unix)]
#[test]
fn a_file_being_written_is_served_never_missed() {
    let d = dir("busy", "route GET /f = file(\"public/w.txt\")\nroute GET /s/*p = files(\"public\", p)\n");
    std::fs::create_dir_all(d.join("public")).unwrap();
    std::fs::write(d.join("public/w.txt"), vec![b'w'; 20 << 10]).unwrap();
    std::fs::write(d.join("public/grow.bin"), vec![b'g'; 2 << 20]).unwrap();
    std::fs::write(d.join("public/r.txt"), "r").unwrap();
    std::fs::write(d.join("public/404.html"), vec![b'm'; 2 << 20]).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writers: Vec<_> = [0, 1, 2, 3]
        .into_iter()
        .map(|k| {
            let (d, stop) = (d.clone(), stop.clone());
            std::thread::spawn(move || {
                use std::io::Write;
                let mut n = 0u32;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    n += 1;
                    match k {
                        // Over what is there, as an editor that writes in place does.
                        0 => std::fs::OpenOptions::new().write(true).open(d.join("public/w.txt")).unwrap().write_all(&vec![b'a' + (n % 26) as u8; 20 << 10]).unwrap(),
                        1 => {
                            let mut f = std::fs::OpenOptions::new().append(true).open(d.join("public/grow.bin")).unwrap();
                            f.write_all(&[b'g'; 4096]).unwrap();
                        }
                        2 => {
                            std::fs::write(d.join("public/r.tmp"), format!("r{}", n)).unwrap();
                            std::fs::rename(d.join("public/r.tmp"), d.join("public/r.txt")).unwrap();
                        }
                        _ => {
                            std::fs::write(d.join("public/404.tmp"), vec![b'm'; (2 << 20) + (n as usize % 7)]).unwrap();
                            std::fs::rename(d.join("public/404.tmp"), d.join("public/404.html")).unwrap();
                        }
                    }
                }
            })
        })
        .collect();
    let get = |m: &str, p: &str, h: &[(&str, &str)]| support::send(s.port, m, p, h, "");
    for i in 0..150 {
        for (m, p, h, want) in [
            ("GET", "/f", vec![], 200),
            ("GET", "/s/w.txt", vec![], 200),
            ("GET", "/s/w.txt", vec![("Range", "bytes=0-99")], 206),
            ("HEAD", "/s/grow.bin", vec![], 200),
            ("GET", "/s/grow.bin", vec![("Range", "bytes=0-99")], 206),
        ] {
            let a = get(m, p, &h);
            assert_eq!(a.status, want, "{} {} {:?} (request {})", m, p, h, i);
        }
        let a = get("GET", "/s/r.txt", &[]);
        assert!(a.status == 200 && a.body.starts_with('r') || a.status == 503 && a.header("retry-after") == Some("1"), "{}: {}", a.status, a.body);
        let a = get("GET", "/s/missing", &[]);
        assert!(a.status == 404 || a.status == 503, "the page of a miss came back {}", a.status);
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for w in writers {
        w.join().unwrap();
    }
}
