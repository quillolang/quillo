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
    // Only escaped: a language it does not know colours nothing (Q-834).
    let plain = "<pre class=\"hl\">let x = 1 // c\n&lt;b&gt; &amp; &quot;y&quot;</pre>";
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

// ---- Components in the browser and on the server (slice 8: Q-757 to Q-771) ----------------------

/// `src` served, and `body` run in Chrome on `path` once the runtime has started, with `$(id)` and
/// `tick(ms)`; what it gives back is the result. None without Chrome.
fn in_page(name: &str, src: &str, path: &str, body: &str) -> Option<(serde_json::Value, String)> {
    let (ok, out) = check(src);
    assert!(ok, "{}", out);
    let d = dir(name, src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let html = support::send(s.port, "GET", path, &[], "").body;
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return None;
    };
    let script = format!(
        "(async () => {{\n  {BOOTED}\n  const $ = (id) => document.getElementById(id);\n  const out = await (async () => {{\n{body}\n  }})();\n  out.remade = QW.remade ?? 0;\n  out.errors = window.__quilloErrors;\n  document.body.dataset.result = JSON.stringify(out);\n}})();"
    );
    Some((in_chrome(&chrome, &format!("http://127.0.0.1:{}{}", s.port, path), &script), html))
}

/// Typing `text` into field `id` a key at a time (after emptying it), as a person does.
const TYPE_IN: &str = "const typeIn = async (id, text) => { const el = $(id); el.focus(); el.select(); document.execCommand(\"delete\"); await tick(20); for (const ch of text) { document.execCommand(\"insertText\", false, ch); await tick(20); } };";

// Q-757: a two-way number input wrote the state back as text while it was typed in: `1.0` on the
// way to `1.05` became `1`, and the next key made `15`. A number the field's text already reads as
// stays as typed; another number the program sets is written; blank text reads as no number.
#[test]
fn a_number_field_keeps_what_is_typed() {
    let src = r##"component Price() {
    state amount = 0
    state qty = 1
    <div><input #n type="number" step="0.01" value={amount}/><p #pa>{amount}</p><p #ta>{type_of(amount)}</p>
    <input #q value={qty}/><p #pq>{qty}</p>
    <button #three @click={() => { amount = 3 }}>3</button><button #same @click={() => { amount = 1.5 }}>1.5</button>
    <button #zero @click={() => { qty = 0 }}>0</button></div>
}
route GET / = <html><head><title>t</title></head><body><Price/></body></html>
"##;
    let body = format!(
        r#"{TYPE_IN}
    const out = {{}};
    for (const s of ["1.05", "-0.5", "2.50", "0.001", "1e3", "0", "007", ""]) {{ await typeIn("n", s); await tick(30); out["n" + s] = $("n").value + " " + $("pa").textContent + " " + $("ta").textContent; }}
    await typeIn("n", "1.50"); $("same").click(); await tick(50); out.same = $("n").value + " " + $("pa").textContent;
    $("three").click(); await tick(50); out.three = $("n").value + " " + $("pa").textContent;
    await typeIn("q", "1.0"); await tick(30); out.q = $("q").value + " " + $("pq").textContent;
    await typeIn("q", "  "); await tick(30); $("zero").click(); await tick(50); out.blank = "[" + $("q").value + "] " + $("pq").textContent;
    return out;"#
    );
    let Some((r, _)) = in_page("q757", src, "/", &body) else { return };
    let want = serde_json::json!({
        "n1.05": "1.05 1.05 float", "n-0.5": "-0.5 -0.5 float", "n2.50": "2.50 2.5 float", "n0.001": "0.001 0.001 float",
        "n1e3": "1e3 1000 int", "n0": "0 0 int", "n007": "007 7 int", "n": "  null",
        "same": "1.50 1.5", "three": "3 3", "q": "1.0 1.0", "blank": "[0] 0", "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Q-758: `<select value={x}>` wrote `value` on the select, which browsers ignore, so the page
// showed its first option; made in the browser before its options, it showed none; options that
// came after the value did not take it. The server marks the option it names `selected` (the first
// that does; by its text when it has no `value`), and the browser sets the value again once its
// options are in and whenever they change.
#[test]
fn a_select_shows_the_option_its_value_names() {
    let src = r##"component Pick(start: str) {
    state size = start
    state show = false
    state opts = ["a", "b"]
    state v = "c"
    state w = "q"
    state wopts = ["p", "q"]
    state fopts = ["p", "q", "r"]
    <div><select #s value={size}><option value="s">S</option><option value="m">M</option><option value="m">M again</option></select>
    <button #show @click={() => { show = true }}>show</button>
    {show && <select #made value={size}><option value="s">S</option><option value="m">M</option></select>}
    {show && <select #none value="zz"><option value="x" disabled>X</option><option value="y">Y</option></select>}
    <select #later value={v}>{for o in opts => <option value={o}>{o}</option>}</select>
    <button #addc @click={() => { opts = opts + ["c"] }}>add c</button>
    <select #both value={w}>{for o in wopts => <option value={o}>{o}</option>}</select>
    <button #both-go @click={() => { wopts = ["q", "r"]; w = "r" }}>both</button>
    <select #bound bind:value={size}><option value="s">S</option><option value="m">M</option></select>
    <select #free value="q">{for o in fopts => <option key={o} value={o}>{o}</option>}</select>
    <button #same-opts @click={() => { fopts = fopts | map((o) => o) }}>same</button><button #rev-opts @click={() => { fopts = reverse(fopts) }}>rev</button>
    <button #more-opts @click={() => { fopts = fopts + ["s"] }}>more</button></div>
}
view Shapes() = <div>
    <select #text value={"two words"}><option>one</option><option>
        two   words
    </option></select>
    <select #nil value={null}><option value="a">A</option><option value="">none</option></select>
    <select #kept value="b"><option value="a" selected>A</option><option value="b">B</option></select>
    <select #grouped value="ü"><optgroup label="g"><option value="a">A</option><option value="ü">Ü</option></optgroup></select>
    <select #tricky value="t"><option title=" value=&quot;t&quot;" value="u">U</option><option value="t" class="x">T</option></select>
    <select #plain><option value="a">A</option><option value="b">B</option></select>
    <select #num value={2}>{for i in [1, 2, 3] => <option value={i}>{i}</option>}</select>
    <select #tmpl value={`o${1}`}><option value="o0">0</option><option value="o1">1</option></select>
    <select #brace value="o{1}"><option value="o1">1</option><option value="o{1}">brace</option></select>
</div>
route GET / = <html><head><title>t</title></head><body><Pick start={request.query.s ?? "m"}/><Shapes/></body></html>
"##;
    let (ok, out) = check(src);
    assert!(ok, "{}", out);
    let d = dir("q758", src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let b = support::send(s.port, "GET", "/", &[], "").body;
    for want in [
        r#"<select id="s"><option value="s">S</option><option selected value="m">M</option><option value="m">M again</option></select>"#,
        r#"<select id="later"><option value="a">a</option><option value="b">b</option></select>"#,
        r#"<select id="text"><option>one</option><option selected>two   words</option></select>"#,
        r#"<select id="nil"><option value="a">A</option><option selected value="">none</option></select>"#,
        r#"<select id="kept"><option value="a" selected>A</option><option selected value="b">B</option></select>"#,
        r#"<select id="grouped"><optgroup label="g"><option value="a">A</option><option selected value="ü">Ü</option></optgroup></select>"#,
        r#"<select id="tricky"><option title=" value=&quot;t&quot;" value="u">U</option><option selected class="x" value="t">T</option></select>"#,
        r#"<select id="plain"><option value="a">A</option><option value="b">B</option></select>"#,
        r#"<select id="num"><option value="1">1</option><option selected value="2">2</option><option value="3">3</option></select>"#,
        r#"<select id="tmpl"><option value="o0">0</option><option selected value="o1">1</option></select>"#,
        r#"<select id="bound"><option value="s">S</option><option selected value="m">M</option></select>"#,
        r#"<select id="brace"><option value="o1">1</option><option selected value="o{1}">brace</option></select>"#,
    ] {
        assert!(b.contains(want), "{} not in {}", want, b);
    }
    // The first option, the last, one named by nothing.
    for (q, want) in [("s", r#"<option selected value="s">S</option><option value="m">M</option>"#), ("zz", r#"<option value="s">S</option><option value="m">M</option><option value="m">M again</option>"#)] {
        let b = support::send(s.port, "GET", &format!("/?s={}", q), &[], "").body;
        assert!(b.contains(want), "{}: {}", q, b);
    }
    drop(s);
    let body = r#"const shown = (id) => { const s = $(id); return s ? s.selectedIndex + ":" + s.value : null; };
    const out = { s: shown("s"), later: shown("later"), both: shown("both"), text: shown("text"), nil: shown("nil"), kept: shown("kept"), grouped: shown("grouped"), tricky: shown("tricky"), num: shown("num") };
    $("s").dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    await tick(200);
    $("show").click(); await tick(100);
    out.made = shown("made"); out.none = shown("none");
    // Made here, marked as the server marks them, so that a form's reset comes back to them.
    const marked = (id) => [...$(id).options].map((o) => (o.hasAttribute("selected") ? 1 : 0)).join("");
    out.marked = marked("s") + " " + marked("made") + " " + marked("none");
    $("addc").click(); await tick(100); out.later2 = shown("later");
    $("both-go").click(); await tick(100); out.both2 = shown("both");
    // What a person picked stays while the options only move or stay; options that come show the
    // value again (found in review).
    $("free").value = "r"; $("free").dispatchEvent(new Event("change", { bubbles: true }));
    const free = [];
    for (const id of ["same-opts", "rev-opts", "more-opts"]) { $(id).click(); await tick(80); free.push($("free").value); }
    out.free = free.join(" ");
    return out;"#;
    let Some((r, _)) = in_page("q758b", src, "/", body) else { return };
    let want = serde_json::json!({
        "s": "1:m", "later": "0:a", "both": "1:q", "text": "1:two words", "nil": "1:", "kept": "1:b", "grouped": "1:ü", "tricky": "1:t", "num": "1:2",
        "made": "1:m", "none": "1:y", "marked": "010 01 00", "later2": "2:c", "both2": "1:r", "free": "r r q", "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Q-759: rows keyed inside `? :` or `match` were kept by place, each branch keyed on its own: every
// row was made again when they moved, what was typed and the focus lost. When every branch of a
// row has a `key`, the list is keyed by it; with one branch unkeyed, by place as before.
#[test]
fn rows_keyed_in_a_choice_move_with_their_items() {
    let src = r##"component Rows() {
    state items = [{id: 1, big: true}, {id: 2, big: false}, {id: 3, big: true}]
    <div><ol #ol>{for it in items => it.big ? <li key={it.id} class="big"><input/></li> : <li key={it.id}><input/></li>}</ol>
    <ul #ul>{for it in items => match it.big { true => <li key={it.id} class="big"><input/></li>, _ => <li key={it.id}><input/></li> }}</ul>
    <ul #blk>{for it in items { let k = it.id * 10; if it.big { <li key={k} class="big"><input/></li> } else if it.id == 2 { <li key={k}><input/></li> } else { <li key={k}><input/></li> } }}</ul>
    <ul #half>{for it in items => it.big ? <li key={it.id} class="big"><input/></li> : <li><input/></li>}</ul>
    <ul #one>{for it in take(items, 1) => it.big ? <li key={it.id}><input/></li> : <li key={it.id}><input/></li>}</ul>
    <button #rot @click={() => { items = drop(items, 1) + take(items, 1) }}>rot</button>
    <button #flip @click={() => { items = items | map((x) => x.id == 1 ? {id: 1, big: false} : x) }}>flip</button>
    <button #none @click={() => { items = [] }}>none</button></div>
}
route GET / = <html><head><title>t</title></head><body><Rows/></body></html>
"##;
    let body = r#"const ins = (id) => [...$(id).querySelectorAll("input")];
    const vals = (id) => ins(id).map((x) => x.value).join(",");
    for (const id of ["ol", "ul", "blk", "half", "one"]) ins(id).forEach((x, i) => { x.value = id + i; });
    ins("ol")[1].focus();
    $("rot").click(); await tick(100);
    const out = { ol: vals("ol"), ul: vals("ul"), blk: vals("blk"), half: vals("half"), one: vals("one"), focus: document.activeElement.value };
    $("flip").click(); await tick(100);
    out.flip = vals("ol") + " " + [...$("ol").children].map((li) => li.className).join(",");
    $("none").click(); await tick(100);
    out.none = $("ol").children.length + " " + $("ul").children.length;
    return out;"#;
    let Some((r, _)) = in_page("q759", src, "/", body) else { return };
    let want = serde_json::json!({
        "ol": "ol1,ol2,ol0", "ul": "ul1,ul2,ul0", "blk": "blk1,blk2,blk0", "half": ",,", "one": "", "focus": "ol1",
        "flip": "ol1,ol2, ,big,", "none": "0 0", "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Q-760: a text input or a select bound to an int state made it text: typing a 0 after the 1 gave
// `qty + 1` as "101". It gives the state's own type back: a number for a number state when the text
// is that number as Quillo writes it (null when empty, as a number input), and text otherwise, so
// an option "02" or "10.00" stays chosen (found in review); a text state stays text.
#[test]
fn a_field_bound_to_a_number_state_gives_numbers() {
    let src = r##"component Q() {
    state qty = 1
    state n = 1
    state name = "7"
    state code = "1"
    state month = 1
    state price = 10
    <div><input #q value={qty}/><p #pq>{type_of(qty)} {qty}</p>
    <textarea #ta value={qty}></textarea>
    <select #s value={n}>{for i in [1, 2, 3] => <option value={i}>{i}</option>}<option value="">none</option></select><p #ps>{type_of(n)} {n}</p>
    <input #name value={name}/><p #pname>{type_of(name)} {name}</p>
    <select #code value={code}><option value="1">one</option><option value="2">two</option></select><p #pcode>{type_of(code)} {code}</p>
    <select #month value={month}><option value="01">Jan</option><option value="02">Feb</option></select><p #pmonth>{type_of(month)} {month}</p>
    <select #price value={price}><option value="5">5</option><option value="10.00">10.00</option><option value="1e2">1e2</option><option value=" 7"> 7</option></select><p #pprice>{type_of(price)}</p></div>
}
route GET / = <html><head><title>t</title></head><body><Q/></body></html>
"##;
    let body = format!(
        r#"{TYPE_IN}
    const pick = async (id, v) => {{ $(id).value = v; $(id).dispatchEvent(new Event("input", {{ bubbles: true }})); $(id).dispatchEvent(new Event("change", {{ bubbles: true }})); await tick(40); }};
    const out = {{}};
    for (const s of ["10", "1.5", " 7 ", "1e3", "-3", "", "abc", "0x10", "12", "99999999999999999999", "٣"]) {{ await typeIn("q", s); await tick(30); out["q" + s] = $("pq").textContent; }}
    await typeIn("ta", "42"); await tick(30); out.ta = $("pq").textContent;
    await pick("s", "2"); out.s2 = $("ps").textContent;
    await pick("s", ""); out.s0 = $("ps").textContent;
    await pick("s", "3"); out.s3 = $("ps").textContent;
    await typeIn("name", "12"); await tick(30); out.name = $("pname").textContent;
    await pick("code", "2"); out.code = $("pcode").textContent;
    await pick("month", "02"); out.month = $("month").value + " " + $("pmonth").textContent;
    for (const v of ["10.00", "1e2", " 7", "5"]) {{ await pick("price", v); out["price" + v] = "[" + $("price").value + "] " + $("pprice").textContent; }}
    return out;"#
    );
    let Some((r, _)) = in_page("q760", src, "/", &body) else { return };
    let want = serde_json::json!({
        "q10": "int 10", "q1.5": "float 1.5", "q 7 ": "str  7 ", "q1e3": "str 1e3", "q-3": "int -3", "q": "null ", "qabc": "str abc", "q0x10": "str 0x10",
        "q12": "int 12", "q99999999999999999999": "str 99999999999999999999", "q٣": "str ٣", "ta": "int 42",
        "s2": "int 2", "s0": "null ", "s3": "int 3", "name": "str 12", "code": "str 2", "month": "02 str 02",
        "price10.00": "[10.00] str", "price1e2": "[1e2] str", "price 7": "[ 7] str", "price5": "[5] int", "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Q-761: `<select multiple value={list}>` showed nothing chosen (the server wrote the list as the
// select's `value`), and the browser set the select's value to the list's text, so each change kept
// only the newest option. A list marks each option it names, on both sides.
#[test]
fn a_select_multiple_shows_each_value_of_its_list() {
    let src = r##"component M(start: list) {
    state picked = start
    state show = false
    <div><select #m multiple value={picked} @change={(e) => { picked = js("Array.from", e.target.selectedOptions) | map((o) => o.value) }}><option value="a">A</option><option value="b">B</option><option value="ü">Ü</option></select><p #pm>{join(picked, ",")}</p>
    <button #ac @click={() => { picked = ["a", "ü"] }}>a and ü</button><button #empty @click={() => { picked = [] }}>none</button>
    <button #show @click={() => { show = true }}>show</button>{show && <select #mm multiple value={picked}><option value="a">A</option><option value="b">B</option><option value="ü">Ü</option></select>}</div>
}
view Lists() = <div>
    <select #nums multiple value={[1, 3]}><option value="1">1</option><option value="2">2</option><option value="3">3</option></select>
    <select #one multiple value={["b"]}><option value="a">A</option><option value="b">B</option></select>
    <select #empty0 multiple value={[]}><option value="a">A</option></select>
    <select #single value={["a", "b"]}><option value="a">A</option><option value="b">B</option></select>
</div>
route GET / = <html><head><title>t</title></head><body><M start={json.parse(request.query.p ?? "[\"b\"]") ?? []}/><Lists/></body></html>
"##;
    let (ok, out) = check(src);
    assert!(ok, "{}", out);
    let d = dir("q761", src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let b = support::send(s.port, "GET", "/", &[], "").body;
    for want in [
        r#"<select id="m" multiple><option value="a">A</option><option selected value="b">B</option><option value="ü">Ü</option></select>"#,
        r#"<select id="nums" multiple><option selected value="1">1</option><option value="2">2</option><option selected value="3">3</option></select>"#,
        r#"<select id="one" multiple><option value="a">A</option><option selected value="b">B</option></select>"#,
        r#"<select id="empty0" multiple><option value="a">A</option></select>"#,
        r#"<select id="single"><option selected value="a">A</option><option selected value="b">B</option></select>"#,
    ] {
        assert!(b.contains(want), "{} not in {}", want, b);
    }
    let b = support::send(s.port, "GET", &format!("/?p={}", enc(r#"["a","b","ü"]"#)), &[], "").body;
    assert!(b.contains(r#"<option selected value="a">A</option><option selected value="b">B</option><option selected value="ü">Ü</option>"#), "{}", b);
    drop(s);
    let body = r#"const chosen = (id) => [...$(id).selectedOptions].map((o) => o.value).join(",");
    const out = { m: chosen("m"), nums: chosen("nums"), single: chosen("single") };
    $("m").dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    await tick(200);
    $("m").options[2].selected = true; $("m").dispatchEvent(new Event("change", { bubbles: true })); await tick(80);
    out.added = chosen("m") + " " + $("pm").textContent;
    $("ac").click(); await tick(80); out.ac = chosen("m");
    $("show").click(); await tick(80);
    out.made = chosen("mm") + " " + [...$("mm").options].map((o) => (o.hasAttribute("selected") ? 1 : 0)).join("");
    $("empty").click(); await tick(80); out.empty = chosen("m") + "|" + $("pm").textContent + "|" + chosen("mm");
    return out;"#;
    let Some((r, _)) = in_page("q761b", src, "/", body) else { return };
    let want = serde_json::json!({ "m": "b", "nums": "1,3", "single": "b", "added": "b,ü b,ü", "ac": "a,ü", "made": "a,ü 101", "empty": "||", "remade": 0, "errors": [] });
    assert_eq!(r, want);
}

// Q-762: a component that had left the page followed its parent's new props: what its `@mount`
// returned said `leave 2` for the room that was 1, a `let` made from a prop too. Its props are as
// it last read them once it has gone.
#[test]
fn a_component_that_left_keeps_the_props_it_had() {
    let src = r##"component Room(id: int, name: str, on_log) {
    let label = name + " " + str(id)
    <div #room @mount={() => { on_log("join " + label); () => { on_log("leave " + label + " " + str(id)) } }}>room {id}</div>
}
component App() {
    state id = 1
    state log = []
    <div><button #next @click={() => { id += 1 }}>next</button>
    <Room key={id} id={id} name={"rüm"} on_log={(m) => { log = log + [m] }}/><p #log>{join(log, ", ")}</p></div>
}
route GET / = <html><head><title>t</title></head><body><App/></body></html>
"##;
    let body = r#"await tick(100);
    const out = { first: $("log").textContent };
    $("next").click(); await tick(100);
    out.once = $("log").textContent + " / " + $("room").textContent;
    $("next").click(); await tick(100);
    out.twice = $("log").textContent + " / " + $("room").textContent;
    return out;"#;
    let Some((r, _)) = in_page("q762", src, "/", body) else { return };
    let want = serde_json::json!({
        "first": "join rüm 1",
        "once": "join rüm 1, leave rüm 1 1, join rüm 2 / room 2",
        "twice": "join rüm 1, leave rüm 1 1, join rüm 2, leave rüm 2 2, join rüm 3 / room 3",
        "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Q-763: a component whose root is an SVG element was put in a `<quillo-island>`, which SVG does
// not draw, so it was 0x0 inside `<svg>`, from the server and when made in the browser. Inside an
// `<svg>` its root is its island, whatever it is (a `<rect>`, a `<g>`, an `<a>`, an `<svg>`), on
// both sides; outside one, and in a `<foreignObject>`, it is in its island as before, and a row
// is its own as before.
#[test]
fn a_component_whose_root_is_svgs_draws_inside_svg() {
    let src = r##"component Bar(h: int) {
    state tall = false
    <rect class="bar" width="10" height={tall ? h * 2 : h} fill="teal" @click={() => { tall = true }}/>
}
component Dot() { <circle class="dot" r="5" cx="50" cy="50"/> }
component Group() { <g class="grp"><circle r="3" cx="5" cy="5"/><text x="1" y="9">ü</text></g> }
component Link() { <a href="/x"><rect class="lnk" width="10" height="10"/></a> }
component Inner() { <svg class="inner" width="20" height="20"><rect width="20" height="20"/></svg> }
component Icon() { <svg class="icon" width="8" height="8"><rect width="8" height="8"/></svg> }
component Loose() { <rect class="loose"/> }
component Note() { <span class="note">n</span> }
component Row(t: str) { <tr class="row"><td>{t}</td></tr> }
component Chart() {
    state on = false
    <div><svg #chart width="100" height="100"><Bar h={40}/><Bar h={70}/><Group/><Link/><Inner/><foreignObject width="50" height="20"><Note/></foreignObject>{on && <Dot/>}{on && <Group/>}{on && <Link/>}{on && <Inner/>}</svg>
    <Icon/><Loose/>{on && <Loose/>}<table><tbody><Row t="a"/></tbody></table>
    <button #on @click={() => { on = true }}>on</button></div>
}
route GET / = <html><head><title>t</title></head><body><Chart/></body></html>
"##;
    let body = r#"const size = (el) => { const b = el.getBoundingClientRect(); return Math.round(b.width) + "x" + Math.round(b.height); };
    const drawn = (el) => { const b = el.getBoundingClientRect(); return b.width > 0 && b.height > 0; };
    const svg = (el) => (el.namespaceURI === "http://www.w3.org/2000/svg" ? "svg" : "html") + ":" + el.parentNode.localName;
    const all = (sel, f) => [...document.querySelectorAll(sel)].map(f).join(" ");
    const bars = [...document.querySelectorAll("rect.bar")];
    const out = { bars: bars.map(size).join(" "), grp: all("g.grp", drawn), lnk: all("rect.lnk", size), inner: all("svg.inner", size) };
    bars[0].dispatchEvent(new MouseEvent("click", { bubbles: true })); await tick(100);
    out.tall = size(document.querySelectorAll("rect.bar")[0]);
    $("on").click(); await tick(100);
    const dot = document.querySelector("circle.dot"), grps = document.querySelectorAll("g.grp");
    out.dot = size(dot) + " " + svg(dot) + " " + dot.hasAttribute("data-qi") + " " + dot.dataset.c;
    out.grp2 = grps.length + " " + drawn(grps[1]) + " " + svg(grps[1]) + " " + svg(grps[1].querySelector("text"));
    out.made = all("rect.lnk", size) + " / " + all("svg.inner", size) + " / " + all("svg.inner", svg) + " / " + all("rect.lnk", (r) => svg(r.parentNode));
    out.html = [document.querySelector("svg.icon"), ...document.querySelectorAll("rect.loose"), document.querySelector("span.note")].map((e) => e.parentNode.localName).join(" ");
    out.row = document.querySelector("tr.row").dataset.c;
    return out;"#;
    let Some((r, html)) = in_page("q763", src, "/", body) else { return };
    for want in [
        r#"<svg id="chart" width="100" height="100"><rect data-qi data-c="Bar" data-p="{}" data-s="{&quot;tall&quot;:false}" class="bar" width="10" height="40" fill="teal"></rect>"#,
        r#"<g data-qi data-c="Group" data-p="{}" data-s="{}" class="grp">"#,
        r#"<a data-qi data-c="Link" data-p="{}" data-s="{}" href="/x"><rect class="lnk" width="10" height="10"></rect></a>"#,
        r#"<svg data-qi data-c="Inner" data-p="{}" data-s="{}" class="inner" width="20" height="20">"#,
        r#"<foreignObject width="50" height="20"><quillo-island style="display:contents" data-c="Note" data-p="{}" data-s="{}"><span class="note">"#,
        r#"<quillo-island style="display:contents" data-c="Icon" data-p="{}" data-s="{}"><svg class="icon""#,
        r#"<quillo-island style="display:contents" data-c="Loose" data-p="{}" data-s="{}"><rect class="loose"></rect></quillo-island>"#,
        r#"<tr data-qi data-c="Row" data-p="{}" data-s="{}" class="row">"#,
    ] {
        assert!(html.contains(want), "{} not in {}", want, html);
    }
    let want = serde_json::json!({
        "bars": "10x40 10x70", "grp": "true", "lnk": "10x10", "inner": "20x20", "tall": "10x80", "dot": "10x10 svg:svg true Dot", "grp2": "2 true svg:svg svg:g",
        "made": "10x10 10x10 / 20x20 20x20 / svg:svg svg:svg / svg:svg svg:svg", "html": "quillo-island quillo-island quillo-island quillo-island",
        "row": "Row", "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// The server's half of the above, on a component's HTML: inside an `<svg>` an element alone is
// its own island, whatever its name, nested ones of its name counted; two, text after it, are not;
// outside one only a table's or a select's part is.
#[test]
fn a_root_alone_inside_svg_is_its_own_island() {
    use quillo::island::own_root_in;
    for (html, want) in [
        ("<rect></rect>", Some(5)),
        ("<rect class=\"a\" width=\"10\"></rect>", Some(5)),
        ("<g><g><circle></circle></g><text>ü</text></g>", Some(2)),
        ("<a href=\"#\"><rect></rect></a>", Some(2)),
        ("<svg><svg></svg><rect></rect></svg>", Some(4)),
        ("<textPath>x</textPath>", Some(9)),
        ("<line\n></line>", Some(5)),
        ("<rect></rect><rect></rect>", None),
        ("<g></g>text", None),
        ("<gx></g>", None),
        ("<1x></1x>", None),
        ("text", None),
        ("", None),
    ] {
        assert_eq!(own_root_in(html, true), want, "{}", html);
    }
    for (html, want) in [("<rect></rect>", None), ("<svg></svg>", None), ("<a></a>", None), ("<tr><td>a</td></tr>", Some(3))] {
        assert_eq!(own_root_in(html, false), want, "{}", html);
    }
}

// Q-764: `count = len(items)` in a component runs once, as it starts, while the same in an `if`
// follows `items`; made an effect, cycles went round, a conversion rewrote what was typed and a
// parent's equal new value undid a pick, so it stays as it was (decided in review). `quillo check`
// says so of one that reads a prop or another state, and points to `let`; not of one that reads
// only itself or nothing that changes.
#[test]
fn an_assignment_in_a_component_runs_once_and_check_says_so() {
    let src = r##"component T(base: list) {
    state items = base
    state count = 0
    state count2 = 0
    state n = 0
    state m = 0
    state fixed = 0
    count = len(items)
    if true { count2 = len(items) }
    n += 1
    m = m + 1
    fixed = 7
    <div><p #cc>{count} {count2} {n} {m} {fixed}</p><button #add @click={() => { items = items + ["x"] }}>add</button></div>
}
component Mutual() {
    state a = 0
    state b = 0
    a = b + 1
    b = a + 1
    <div><p #ab>{a} {b}</p><button #mgo @click={() => { a = 10 }}>go</button></div>
}
component Temp() {
    state c = 0
    state f = 32
    f = c * 9 / 5 + 32
    c = (f - 32) * 5 / 9
    <div><input #c type="number" value={c}/><input #f type="number" value={f}/></div>
}
component Child(items: list) {
    state sel = 0
    sel = items[0]
    <div><p #sel>{sel}</p><button #pick @click={() => { sel = 99 }}>pick</button></div>
}
component Draft(initial: str) {
    state draft = ""
    draft = initial
    <div><input #draft value={draft}/><p #dp>{draft}</p></div>
}
component Host() {
    state all = [1, 2, 3]
    state saved = "hello"
    <div><Child items={take(all, 3)}/><button #more @click={() => { all = all + [9] }}>more</button>
    <Draft initial={saved}/><button #same @click={() => { saved = "hello" }}>same</button></div>
}
route GET / = <html><head><title>t</title></head><body><T base={json.parse(request.query.b ?? "[]") ?? []}/><Mutual/><Temp/><Host/></body></html>
"##;
    let (_, said) = check(src);
    let warned: Vec<&str> = said.lines().filter(|l| l.contains("runs once")).collect();
    for (line, name, read, comp) in [(8, "count", "items", "T"), (18, "a", "b", "Mutual"), (19, "b", "a", "Mutual"), (25, "f", "c", "Temp"), (26, "c", "f", "Temp"), (31, "sel", "items", "Child"), (36, "draft", "initial", "Draft")] {
        let want = format!(":{}: warning: `{} = ...` runs once, as <{}> starts: it does not follow `{}` when that changes; for a value that follows it, write `let {} = ...` in place of the state", line, name, comp, read, name);
        assert!(warned.iter().any(|l| l.contains(&want)), "{} not in {}", want, said);
    }
    // Not of `n += 1`, `m = m + 1` (its own state), `fixed = 7`, or the `if` (an effect).
    assert_eq!(warned.len(), 7, "{}", said);
    let body = r#"const out = { start: $("cc").textContent + " | " + $("ab").textContent };
    $("add").click(); await tick(80); out.one = $("cc").textContent;
    $("add").click(); await tick(80); out.two = $("cc").textContent;
    $("mgo").click(); await tick(80); out.go = $("ab").textContent;
    const f0 = $("f").value;
    $("c").focus(); $("c").select(); document.execCommand("delete"); for (const ch of "0.1") { document.execCommand("insertText", false, ch); await tick(20); }
    await tick(50); out.temp = $("c").value + " " + ($("f").value === f0);
    $("pick").click(); await tick(60); $("more").click(); await tick(60); out.sel = $("sel").textContent;
    $("draft").value = "edited"; $("draft").dispatchEvent(new Event("input", { bubbles: true })); await tick(60);
    $("same").click(); await tick(60); out.draft = $("draft").value + " " + $("dp").textContent;
    return out;"#;
    let Some((r, html)) = in_page("q764", src, "/?b=%5B1%2C2%5D", body) else { return };
    // The server's page shows what the browser does: each runs once.
    assert!(html.contains(r#"<p id="cc">2 2 1 1 7</p>"#), "{}", html);
    assert!(html.contains(r#"<p id="ab">1 2</p>"#), "{}", html);
    let want = serde_json::json!({
        "start": "2 2 1 1 7 | 1 2", "one": "2 3 1 1 7", "two": "2 4 1 1 7", "go": "10 2", "temp": "0.1 true", "sel": "99", "draft": "edited edited",
        "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Q-765: a range's `value` set before its dynamic `max` was brought into the default `max` of 100,
// and after `hi = 2000; v = 1500` the field showed 1000. The value is set again when `min`, `max`
// or `step` change; a value past the new limit is brought into it, as the browser does.
#[test]
fn a_range_value_set_before_its_limits_is_kept() {
    let src = r##"component R() {
    state v = 500
    state hi = 1000
    state lo = 0
    state st = 1
    state show = false
    <div><input #r type="range" value={v} min="0" max={hi}/>
    {show && <input #made type="range" value={v} min="0" max={hi}/>}
    {show && <input #low type="range" value={v} min={lo} max="2000" step={st}/>}
    <button #show @click={() => { show = true }}>show</button>
    <button #raise @click={() => { hi = 2000; v = 1500 }}>raise</button>
    <button #over @click={() => { v = 5000 }}>over</button>
    <button #edge @click={() => { v = 2000 }}>edge</button>
    <button #neg @click={() => { lo = -100; v = -50; st = 25 }}>neg</button></div>
}
route GET / = <html><head><title>t</title></head><body><R/></body></html>
"##;
    let body = r#"const vals = () => ["r", "made", "low"].map((id) => ($(id) ? $(id).value : "-")).join(" ");
    const out = { start: vals() };
    $("show").click(); await tick(80); out.shown = vals();
    $("raise").click(); await tick(80); out.raised = vals();
    $("edge").click(); await tick(80); out.edge = vals();
    $("over").click(); await tick(80); out.over = vals();
    $("neg").click(); await tick(80); out.neg = vals();
    return out;"#;
    let Some((r, _)) = in_page("q765", src, "/", body) else { return };
    let want = serde_json::json!({
        "start": "500 - -", "shown": "500 500 500", "raised": "1500 1500 1500", "edge": "2000 2000 2000", "over": "2000 2000 2000", "neg": "0 0 -50",
        "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Q-766: without `Element.moveBefore` (Safari), a focused keyed row that moved lost the focus. The
// focus and what was selected in the field come back after the move; with `moveBefore` they stay.
#[test]
fn a_focused_row_that_moves_keeps_the_focus() {
    let src = r##"component Moves() {
    state rows = ["a", "b", "c"]
    <ul #mv>{for r in rows => <li key={r}><input id={"in-" + r} value={"text " + r} @keydown={(e) => { if e.key == "ArrowUp" { let i = index_of(rows, r); if i > 0 { rows = take(rows, i - 1) + [r, rows[i - 1]] + drop(rows, i + 1) } } }}/></li>}</ul>
}
route GET / = <html><head><title>t</title></head><body><Moves/></body></html>
"##;
    for without in [true, false] {
        let body = format!(
            r##"if ({without}) delete Element.prototype.moveBefore;
    const order = () => [...document.querySelectorAll("#mv input")].map((x) => x.id.slice(3)).join("");
    const up = async () => {{ document.activeElement.dispatchEvent(new KeyboardEvent("keydown", {{ key: "ArrowUp", bubbles: true }})); await tick(80); }};
    const c = $("in-c"); c.focus(); c.setSelectionRange(2, 4, "backward");
    const at = () => document.activeElement.id + " " + order() + " " + c.selectionStart + "-" + c.selectionEnd + " " + c.selectionDirection;
    await up(); const out = {{ one: at() }};
    await up(); out.two = at();
    await up(); out.top = at();
    return out;"##
        );
        let Some((r, _)) = in_page("q766", src, "/", &body) else { return };
        let want = serde_json::json!({ "one": "in-c acb 2-4 backward", "two": "in-c cab 2-4 backward", "top": "in-c cab 2-4 backward", "remade": 0, "errors": [] });
        assert_eq!(r, want, "without moveBefore: {}", without);
    }
}

// Q-767: `e.form` left out the button that sent the form; the browser sends its name and value. It
// has them now; a form sent without one (`requestSubmit()`), and `e.form` of a field's own event,
// have the fields alone.
#[test]
fn a_forms_value_names_the_button_that_sent_it() {
    let src = r##"component Forms() {
    state got = ""
    state typed = ""
    <div><form #f @submit={(e) => { got = json.stringify(e.form) }}><input #title name="title" value="t1" @input={(e) => { typed = json.stringify(e.form) }}/>
    <button #save name="action" value="save">Save</button><button #del name="action" value="délete">Delete</button><button #plain>Plain</button></form>
    <form #other><button #elsewhere name="action" value="other">x</button></form>
    <p #got>{got}</p><p #typed>{typed}</p></div>
}
route GET / = <html><head><title>t</title></head><body><Forms/></body></html>
"##;
    let body = r#"const out = {};
    $("del").click(); await tick(60); out.del = $("got").textContent;
    $("save").click(); await tick(60); out.save = $("got").textContent;
    $("plain").click(); await tick(60); out.plain = $("got").textContent;
    $("f").requestSubmit(); await tick(60); out.none = $("got").textContent;
    $("f").dispatchEvent(new SubmitEvent("submit", { bubbles: true, cancelable: true, submitter: $("elsewhere") })); await tick(60); out.elsewhere = $("got").textContent;
    $("title").dispatchEvent(new Event("input", { bubbles: true })); await tick(60); out.typed = $("typed").textContent;
    return out;"#;
    let Some((r, _)) = in_page("q767", src, "/", body) else { return };
    let want = serde_json::json!({
        "del": r#"{"title":"t1","action":"délete"}"#, "save": r#"{"title":"t1","action":"save"}"#, "plain": r#"{"title":"t1"}"#,
        "none": r#"{"title":"t1"}"#, "elsewhere": r#"{"title":"t1"}"#, "typed": r#"{"title":"t1"}"#, "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Q-768: a form reset emptied the inputs the browser had made (their first value was set as the
// field's value only), while the server's came back to it. The first value is the field's default
// too, as the server writes it; one set later is not.
#[test]
fn a_form_reset_brings_back_the_first_values() {
    let src = r##"component Forms(start: str) {
    state show = false
    state x = start
    <div><form #f1><input #s1 value="default"/><input #s2 value={x}/><button #r1 type="reset">reset</button></form>
    <button #show @click={() => { show = true }}>show</button>
    {show && <form #f2><input #d1 value="default"/><input #d2 value={x}/><input #d3/><input #d4 value={null}/><textarea #ta value="tdéf"></textarea><button #r2 type="reset">reset</button></form>}
    <button #change @click={() => { x = "later" }}>change</button></div>
}
route GET / = <html><head><title>t</title></head><body><Forms start={request.query.s ?? "ünï"}/></body></html>
"##;
    let body = r#"const ids = ["s1", "s2", "d1", "d2", "d3", "d4", "ta"];
    const vals = () => ids.map((id) => ($(id) ? $(id).value : "-")).join("|");
    $("show").click(); await tick(80);
    $("change").click(); await tick(80);
    const out = { shown: vals(), attrs: ids.map((id) => $(id).getAttribute("value")).join("|") };
    for (const id of ids) $(id).value = "typed";
    $("r1").click(); $("r2").click(); await tick(80);
    out.reset = vals();
    return out;"#;
    let Some((r, _)) = in_page("q768", src, "/", body) else { return };
    let want = serde_json::json!({
        "shown": "default|later|default|later|||tdéf", "attrs": "default|ünï|default|ünï|||",
        "reset": "default|ünï|default|ünï|||tdéf", "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Q-769: a `<template>` in a component was taken up through its children, which the browser's
// parser puts in its `.content`: a mismatch on every load, and the one the browser made had its
// rows as children, not content. Its children are its content, from the server and made here.
#[test]
fn a_template_keeps_its_content() {
    let src = r##"component Tpl() {
    state name = "row"
    state show = false
    <div><input/><template #tpl><li class="t">{name}</li></template><template #empty></template><template #txt>just ü</template>
    <button #show @click={() => { show = true }}>show</button><button #rename @click={() => { name = "renamed" }}>rename</button>
    {show && <div><template #made><li class="m">{name}</li><li>two</li></template></div>}</div>
}
route GET / = <html><head><title>t</title></head><body><Tpl/></body></html>
"##;
    let body = r#"const t = (id) => $(id).content.childNodes.length + " " + $(id).childNodes.length + " " + $(id).content.textContent;
    const out = { tpl: t("tpl"), empty: t("empty"), txt: t("txt") };
    $("show").click(); await tick(80);
    out.made = t("made");
    $("rename").click(); await tick(80);
    out.renamed = t("tpl") + " / " + t("made");
    return out;"#;
    let Some((r, html)) = in_page("q769", src, "/", body) else { return };
    assert!(html.contains(r#"<template id="tpl"><li class="t">row</li></template>"#), "{}", html);
    let want = serde_json::json!({
        "tpl": "1 0 row", "empty": "0 0 ", "txt": "1 0 just ü", "made": "2 0 rowtwo", "renamed": "1 0 renamed / 2 0 renamedtwo", "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Q-770: a child that set its parent's state as it started made the server's page show the old
// value above it and the new one below, with the island holding the old. The server renders the
// parent again, the states its markup changed starting as the first render left them, when that
// render's markup changes them no more; one that sets them anew each time (`count += n`) keeps its
// first render, as before. What its own statements set (`n += 1`) is run again from where it was,
// and a select's options are marked once.
#[test]
fn a_child_setting_its_parents_state_as_it_starts_shows_one_value() {
    let src = r##"component Child(on_ready) {
    on_ready(5)
    <span>child</span>
}
component Quiet(on_ready) { <span>quiet</span> }
component Parent() {
    state count = 0
    <div><p #c1>{count}</p><Child on_ready={(n) => { count = n }}/><p #c2>{count}</p></div>
}
component Bump() {
    state count = 0
    <div><p #b1>{count}</p><Child on_ready={(n) => { count += n }}/><p #b2>{count}</p></div>
}
component Calm() {
    state count = 0
    <div><p #q1>{count}</p><Quiet on_ready={(n) => { count = n }}/><p #q2>{count}</p></div>
}
component Inner() {
    state count = 0
    <div><p #i1>{count}</p><Child on_ready={(n) => { count = n }}/><p #i2>{count}</p></div>
}
component Outer() { <section><Inner/></section> }
component Mixed() {
    state n = 0
    state count = 0
    n += 1
    <div><p #x1>{count} {n}</p><Child on_ready={(v) => { count = v }}/><p #x2>{count} {n}</p></div>
}
component Opt(v: str) {
    state label = ""
    label = v + "!"
    <option value={v}>{label}</option>
}
component Pick() {
    state x = "b"
    <div><select #ps value={x}><Opt v="a"/><Opt v="b"/><Opt v="c"/></select></div>
}
route GET / = <html><head><title>t</title></head><body><Parent/><Bump/><Calm/><Outer/><Mixed/><Pick/></body></html>
"##;
    let body = r#"for (const el of document.querySelectorAll("quillo-island")) el.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    await tick(200);
    const two = (a, b) => $(a).textContent + " " + $(b).textContent;
    return { parent: two("c1", "c2"), bump: two("b1", "b2"), calm: two("q1", "q2"), inner: two("i1", "i2"), mixed: two("x1", "x2"), pick: $("ps").value };"#;
    let Some((r, html)) = in_page("q770", src, "/", body) else { return };
    for want in [
        r#"data-c="Parent" data-p="{}" data-s="{&quot;count&quot;:5}"><div><p id="c1">5</p>"#,
        r#"<p id="c2">5</p>"#,
        r#"data-c="Bump" data-p="{}" data-s="{&quot;count&quot;:0}"><div><p id="b1">0</p>"#,
        r#"<p id="b2">5</p>"#,
        r#"data-c="Calm" data-p="{}" data-s="{&quot;count&quot;:0}"><div><p id="q1">0</p>"#,
        r#"data-c="Inner" data-p="{}" data-s="{&quot;count&quot;:5}"><div><p id="i1">5</p>"#,
        r#"data-c="Mixed" data-p="{}" data-s="{&quot;n&quot;:0,&quot;count&quot;:5}"><div><p id="x1">5 1</p>"#,
        r#"<p id="x2">5 1</p>"#,
        r#"<option data-qi data-c="Opt" data-p="{}" data-s="{&quot;label&quot;:&quot;&quot;}" value="a">a!</option><option data-qi data-c="Opt" data-p="{}" data-s="{&quot;label&quot;:&quot;&quot;}" selected value="b">b!</option>"#,
    ] {
        assert!(html.contains(want), "{} not in {}", want, html);
    }
    let want = serde_json::json!({ "parent": "5 5", "bump": "5 5", "calm": "0 0", "inner": "5 5", "mixed": "5 1 5 1", "pick": "b", "remade": 0, "errors": [] });
    assert_eq!(r, want);
}

// The review of Q-770: a component whose own statements set its state (`c = 1`) was rendered again,
// and each in a chain doubled what was below it: 16 deep, the leaf 65,536 times, and a server fn a
// state of it calls as often. Its statements are no reason; and inside one rendered again, none is
// rendered again, so each is rendered at most twice.
#[test]
fn components_setting_their_own_state_render_once() {
    let mut src = String::from("let hits = 0\nserver fn hit(s: str) -> int {\n    hits += 1\n    return hits\n}\ncomponent Leaf() {\n    state v = hit(\"leaf\")\n    <p class=\"leaf\">{v}</p>\n}\n");
    for i in (1..=16).rev() {
        let inner = if i == 16 { "<Leaf/>".to_string() } else { format!("<D{}/>", i + 1) };
        src.push_str(&format!("component D{i}() {{\n    state c = 0\n    c = 1\n    <div class=\"d{i}\">{inner}</div>\n}}\n"));
    }
    // Two that are rendered again, one inside the other.
    src.push_str("component Child(on_ready) {\n    on_ready(5)\n    <Leaf/>\n}\ncomponent In() {\n    state count = 0\n    <div><p>{count}</p><Child on_ready={(n) => { count = n }}/></div>\n}\ncomponent Out() {\n    state count = 0\n    <div><p>{count}</p><Child on_ready={(n) => { count = n }}/><In/></div>\n}\n");
    src.push_str("route GET / = <html><head><title>t</title></head><body><D1/></body></html>\nroute GET /two = <html><head><title>t</title></head><body><Out/></body></html>\nroute GET /hits = <p>{hits}</p>\n");
    let (ok, out) = check(&src);
    assert!(ok, "{}", out);
    let d = dir("q770chain", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let t = std::time::Instant::now();
    let b = support::send(s.port, "GET", "/", &[], "").body;
    assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
    assert!(b.contains(r#"<div class="d16"><quillo-island style="display:contents" data-c="Leaf" data-p="{}" data-s="{&quot;v&quot;:1}"><p class="leaf">1</p>"#), "{}", b);
    assert!(b.contains(r#"data-c="D1" data-p="{}" data-s="{&quot;c&quot;:0}">"#), "{}", b);
    assert_eq!(support::send(s.port, "GET", "/hits", &[], "").body, "<p>1</p>");
    // Out once and again (2 leaves each), In inside each, once more inside Out's first: 7 at most,
    // not 2 x 2 x 2 for each.
    let b = support::send(s.port, "GET", "/two", &[], "").body;
    assert!(b.contains(r#"data-c="Out" data-p="{}" data-s="{&quot;count&quot;:5}"><div><p>5</p>"#), "{}", b);
    let hits: u32 = support::send(s.port, "GET", "/hits", &[], "").body.trim_start_matches("<p>").trim_end_matches("</p>").parse().unwrap();
    assert!(hits - 1 <= 7, "{} renders of the leaves", hits - 1);
}

// Q-771: `autofocus` on markup that appeared later did nothing, as browsers do with what is put in
// after the page loaded; React and Svelte focus it. It is focused once on the page, unless someone
// is typing in another field then; `autofocus={false}` is not, and of rows the first is.
#[test]
fn autofocus_focuses_what_is_made_later() {
    let src = r##"component Focus() {
    state show = false
    state on = false
    state rows = []
    <div><input #other/><button #fs @click={() => { show = true }}>show</button>{show && <div><input #af autofocus/></div>}
    <button #fon @click={() => { on = true }}>on</button>{on && <input #hole autofocus={on}/>}
    <input #never autofocus={false}/>
    <button #rows @click={() => { rows = ["a", "b"] }}>rows</button><ul>{for r in rows => <li key={r}><input id={"row-" + r} autofocus/></li>}</ul></div>
}
route GET / = <html><head><title>t</title></head><body><Focus/></body></html>
"##;
    let body = r#"const at = () => document.activeElement.id || document.activeElement.localName;
    $("other").dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    await tick(200);
    const out = { start: at() };
    $("other").focus();
    $("fs").click(); await tick(80);
    out.typing = at();
    $("other").blur();
    $("fon").click(); await tick(80);
    out.hole = at();
    $("hole").blur();
    $("rows").click(); await tick(80);
    out.rows = at();
    return out;"#;
    let Some((r, html)) = in_page("q771", src, "/", body) else { return };
    assert!(html.contains(r#"<input id="never">"#), "{}", html);
    let want = serde_json::json!({ "start": "body", "typing": "other", "hole": "hole", "rows": "row-a", "remade": 0, "errors": [] });
    assert_eq!(r, want);
}

// ---- slice 9: server and browser (Q-772 to Q-782) ----

/// What `js` prints in Node, under a deadline; None without Node.
fn in_node(js: &str) -> Option<String> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    if !Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
        return None;
    }
    let d = support::temp_dir(&format!("feedback55-node-{}", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::write(d.join("t.cjs"), js).unwrap();
    let mut child = Command::new("node").arg(d.join("t.cjs")).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("node did not end");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "node failed: {}", String::from_utf8_lossy(&o.stderr));
    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Each expression of `cases` on the interpreter (its floats written as a page writes them, as a
/// component's are) and, compiled for the browser, in Node: `[the type of each value in it, it]`,
/// or `ERROR: ...`, the same on both sides and what the case wants (FEEDBACK55.md, Q-775 to
/// Q-782; branch feedback55). Without Node, the server's half.
fn same_on_both(defs: &str, cases: &[(&str, &str)]) {
    same_on_both_after("", defs, cases)
}

/// What a browser whose JSON.parse gives a reviver no text of the numbers does (Safari before
/// 18.4, Firefox before 135): Node's, made so.
const OLD_JSON: &str = "const __parse = JSON.parse;\nJSON.parse = function (t, r) { return r ? __parse(t, function (k, v) { return r.call(this, k, v); }) : __parse(t); };";

/// `same_on_both`, with `prelude` run in Node before the browser's half.
fn same_on_both_after(prelude: &str, defs: &str, cases: &[(&str, &str)]) {
    const TYPES: &str = "fn v(x) = x\nfn tys(x) = if type_of(x) == \"list\" { x | map((y) => tys(y)) } else if type_of(x) == \"map\" { keys(x) | map((k) => [k, tys(x[k])]) } else { type_of(x) }\n";
    let one = |expr: &str, want: &str| {
        let src = format!("{}{}\nfn __t() {{\n  let x = ({})\n  [tys(x), x]\n}}\n", TYPES, defs, expr);
        let (ok, out) = check(&src);
        assert!(ok, "{}\n{}", out, src);
        let mut it = quillo::Interp::new(".");
        it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
        let server = quillo::value::js_numbers(true, || match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
            Ok(v) | Err(quillo::interp::Ctrl::Return(v)) => v.repr(),
            Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
            Err(_) => "left the function".into(),
        });
        assert_eq!(server, want, "server: {}", expr);
        if let Some(browser) = in_node(&format!("{}\n{}", prelude, quillo::client::test_script(&it, "__t").unwrap())) {
            assert_eq!(browser, want, "browser: {}", expr);
        }
    };
    let at = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        let jobs: Vec<_> = (0..6)
            .map(|_| {
                s.spawn(|| loop {
                    let i = at.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((expr, want)) = cases.get(i) else { break };
                    one(expr, want);
                })
            })
            .collect();
        for j in jobs {
            j.join().unwrap();
        }
    });
}

// Q-775: a whole float was a JS number with no fraction, an int to the browser's code: `2.0 / 0`
// stopped with "division by zero", `match` took the int arm, `type_of` said int, an `int`
// parameter and an index took it, and `9007199254740993 * 1.0` stayed exact. A whole float is an
// Fl in the browser, an int a number (a BigInt past 2^53), so every case is the server's: inf
// and NaN for a float divided by zero, int refused for a float parameter and float for an int,
// a record's float field widening an int as the server's does, and an int with no -0 (`-3 * 0`
// is 0, and `1.5 / x` inf). The edges: -0.0, 1e300, inf and NaN as floats, ints either side of
// 2^53, `float("0x10")` and the rest of what Rust reads as no float.
#[test]
fn a_whole_float_is_a_float_in_the_browser_too() {
    let defs = r#"fn fi(x: int) = x
fn ff(x: float) = x
fn fnum(x: num) = x
fn mt(x) = match x { float as f => "float", int as i => "int", _ => "other" }
fn ml(x) = match x { 2 => "two", _ => "other" }
type Pt = { n: int }
type Pf = { n: float }
type Pu = { n: int | float }
type Pl = { xs: list<float> }
"#;
    same_on_both(defs, &[
        (r#"[v(2.0) - v(1.0), v(2.0) * v(3.0), v(2.0) + v(1.0), v(2.0) - v(0.5), v(4.0) * v(0.5), v(2.0) - v(3), v(2.0) * v(3), v(0.5) - v(2.0), v(3) * v(2.0)]"#, r#"[["float", "float", "float", "float", "float", "float", "float", "float", "float"], [1, 6, 3, 1.5, 2, -1, 6, -1.5, 6]]"#),
        (r#"[v(5.0) / v(0), v(1) / v(0.0), v(0.0) / v(0.0), v(-5.0) / v(0), v(5.5) % v(0), v(5.0) % v(0)]"#, r#"[["float", "float", "float", "float", "float", "float"], [inf, inf, NaN, -inf, NaN, NaN]]"#),
        (r#"v(5) / v(0)"#, r#"ERROR: division by zero"#),
        (r#"v(5) % v(0)"#, r#"ERROR: division by zero"#),
        (r#"[mt(v(2)), mt(v(2.0)), mt(v(2.5)), mt(v(-0.0)), mt(v(1e300)), mt(v(math.inf)), mt(v(math.nan)), mt(v(9007199254740993))]"#, r#"[["str", "str", "str", "str", "str", "str", "str", "str"], ["int", "float", "float", "float", "float", "float", "float", "int"]]"#),
        (r#"[ml(v(2)), ml(v(2.0)), ml(v(2.5))]"#, r#"[["str", "str", "str"], ["two", "two", "other"]]"#),
        (r#"[type_of(2.0), type_of(sqrt(4)), type_of(json.parse("1e2")), type_of(json.parse("2.0")), type_of(json.parse("2")), type_of(float(2)), type_of(int(2.0)), type_of(-0.0), type_of(1e300)]"#, r#"[["str", "str", "str", "str", "str", "str", "str", "str", "str"], ["float", "float", "float", "float", "int", "float", "int", "float", "float"]]"#),
        (r#"fi(v(2.0))"#, r#"ERROR: argument `x` of `fi` must be int, got float 2"#),
        (r#"ff(v(2))"#, r#"ERROR: argument `x` of `ff` must be float, got int 2"#),
        (r#"[ff(v(2.0)), ff(v(2.5)), fi(v(2)), fnum(v(2)), fnum(v(2.0)), fnum(v(2.5))]"#, r#"[["float", "float", "int", "int", "float", "float"], [2, 2.5, 2, 2, 2, 2.5]]"#),
        (r#"[Pt(v({ n: 2.0 })), Pt(v({ n: 2 })), Pf(v({ n: 2 })), Pf(v({ n: 2.0 })), Pu(v({ n: 2 })), Pu(v({ n: 2.0 })), Pl(v({ xs: [1, 2.5] }))]"#, r#"[["error", [["n", "int"]], [["n", "float"]], [["n", "float"]], [["n", "int"]], [["n", "float"]], [["xs", ["float", "float"]]]], [err("Pt: `n` must be int, got float 2"), { n: 2 }, { n: 2 }, { n: 2 }, { n: 2 }, { n: 2 }, { xs: [1, 2.5] }]]"#),
        (r#"[1, 2, 3][v(1.0)]"#, r#"ERROR: cannot index list with float"#),
        (r#"slice("abcd", v(1.0), v(3.0))"#, r#"ERROR: `slice` expects an int as argument 2, got float"#),
        (r#"range(v(3.0))"#, r#"ERROR: `range` expects an int as argument 1, got float"#),
        (r#"repeat("a", v(2.0))"#, r#"ERROR: `repeat` expects an int as argument 2, got float"#),
        (r#"take([1, 2, 3], v(2.0))"#, r#"ERROR: `take` expects an int as argument 2, got float"#),
        (r#"round(1.2345, v(2.0))"#, r#"ERROR: `round` expects an int as argument 2, got float"#),
        (r#"[v(9007199254740993) * v(1.0), v(9007199254740993) + v(0.0), v(9007199254740993) / v(1.0)]"#, r#"[["float", "float", "float"], [9007199254740992, 9007199254740992, 9007199254740992]]"#),
        (r#"[1.5 + 0.5, 2.0 * 3, 4.0 - 1, 6.0 / 2, 7.0 % 2, -v(2.0), 3 + 0.0, 0.5 * 4, 6 / 3, 7 / 2, 1e300 * 10]"#, r#"[["float", "float", "float", "float", "float", "float", "float", "float", "int", "float", "float"], [2, 6, 3, 3, 1, -2, 3, 2, 2, 3.5, 10000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000]]"#),
        (r#"[0 * -1, -3 * 0, -4 % 2, -(0), float(0 * -1), 1.5 / (0 * -1), -0.0 * 1, 1.0 / -0.0]"#, r#"[["int", "int", "int", "int", "float", "float", "float", "float"], [0, 0, 0, 0, 0, inf, 0, -inf]]"#),
        (r#"[2.0 == 2, 2 == 2.0, [2.0] == [2], { a: 2.0 } == { a: 2 }, 2.0 == dec("2"), 2.5 == 2, 2.0 != 2, contains([1.0], 1), index_of([1, 2.0], 2)]"#, r#"[["bool", "bool", "bool", "bool", "bool", "bool", "bool", "bool", "int"], [true, true, true, true, true, false, false, true, 1]]"#),
        (r#"[2.0 < 3, 2.0 <= 2, 2 >= 2.0, 1e300 > 9223372036854775807, -0.0 < 0, sort([2.0, 1, 1.5, -0.0, 0])]"#, r#"[["bool", "bool", "bool", "bool", "bool", ["float", "int", "int", "float", "float"]], [true, true, true, true, false, [0, 0, 1, 1.5, 2]]]"#),
        (r#"[sqrt(4), abs(-2.0), abs(-2), floor(2.5), floor(2.0), ceil(-0.5), round(2.5), round(-2.5), round(2.0), round(5, 2), floor(9.1e15), ceil(-9.1e15)]"#, r#"[["float", "float", "int", "int", "int", "int", "int", "int", "int", "float", "float", "float"], [2, 2, 2, 2, 2, 0, 3, -3, 2, 5, 9100000000000000, -9100000000000000]]"#),
        (r#"[float(2), float("2"), float("2.5"), float(" 7 "), float("1e3"), float(".5"), float("5."), float("+1.5"), float("-0"), float("1e400"), float("inf"), float("-Infinity"), float("nan")]"#, r#"[["float", "float", "float", "float", "float", "float", "float", "float", "float", "float", "float", "float", "float"], [2, 2, 2.5, 7, 1000, 0.5, 5, 1.5, 0, inf, inf, -inf, NaN]]"#),
        (r#"[float("0x10"), float("0b11"), float("0o17"), float(" 0x10 "), float("1_000"), float(""), float("."), float("e5"), float("1e")]"#, r#"[["error", "error", "error", "error", "error", "error", "error", "error", "error"], [err("not a number: \"0x10\""), err("not a number: \"0b11\""), err("not a number: \"0o17\""), err("not a number: \" 0x10 \""), err("not a number: \"1_000\""), err("not a number: \"\""), err("not a number: \".\""), err("not a number: \"e5\""), err("not a number: \"1e\"")]]"#),
        (r#"[int(2.0), int(2.9), int(-2.9), int(1e15), int(9007199254740993.0), int(-9.223372036854775808e18), int(9.223372036854775807e18), int(math.inf), int(-0.5)]"#, r#"[["int", "int", "int", "int", "int", "int", "error", "error", "int"], [2, 2, -2, 1000000000000000, 9007199254740992, -9223372036854775808, err("9223372036854776000 is too large for an int"), err("cannot convert float to int"), 0]]"#),
        (r#"[min([2.0, 3]), max([2, 3.0]), min([1, 1.0]), max([1.0, 1]), sum([1.0, 2]), sum([1, 2]), sum([0.5, 0.5]), sum([]), sum([2.0])]"#, r#"[["float", "float", "int", "float", "float", "int", "float", "int", "float"], [2, 3, 1, 1, 3, 3, 1, 0, 2]]"#),
        (r#"[pow(2, 2), pow(2.0, 2), pow(2, 0.5), pow(4, 0.5), pow(2, -1), pow(2, 64.0)]"#, r#"[["int", "float", "float", "float", "float", "float"], [4, 4, 1.4142135623730951, 2, 0.5, 18446744073709552000]]"#),
        (r#"[2.0 + "a", "a" + 2.0, `${1.0}`, str([1.0, 2]), str({ a: 1.0 }), str(-0.0), str(1e21), str(2.5)]"#, r#"[["str", "str", "str", "str", "str", "str", "str", "str"], ["2a", "a2", "1", "[1, 2]", "{ a: 1 }", "0", "1000000000000000000000", "2.5"]]"#),
        (r#"[random() >= 0, type_of(random()), type_of(math.pi), dec(2.0), dec(2.5)]"#, r#"[["bool", "str", "str", "dec", "dec"], [true, "float", "float", 2m, 2.5m]]"#),
        (r#"[money.of(v(2.0), "USD") ?? "refused", money.of(dec("2"), "USD").amount]"#, r#"[["str", "dec"], ["refused", 2.00m]]"#),
        (r#"[unique([1, 1.0, 2]), group_by([1, 1.0], (x) => x), vec.full(2, 1.0), vec.zeros(v(2.0))]"#, r#"[[["int", "int"], [["1", ["int", "float"]]], ["float", "float"], ["int", "int"]], [[1, 2], { "1": [1, 1] }, [1, 1], [0, 0]]]"#),
    ]);
}

// Q-776: an int past 2^53 (a BigInt in the browser) stopped the math functions, `stats` and
// `vec`; `math.factorial(19)`, `math.comb(60, 30)` and `math.prod` overflowed at 2^53; it was never
// equal to the same float; `3037000500 * 3037000500` was a number past 64 bits; `int(-2^63)` was
// refused and `json.parse` rounded the ends of an int's range. Each is exact now, or the server's
// float where it gives one, or the same overflow, at the edges of 2^53 and 2^63 and either side
// of them. `floor`, `ceil` and `round` give an int back as it is on the server too: past 2^53 it
// went through a float (FEEDBACK55.md, Q-776 and Q-789).
#[test]
fn ints_past_2_53_are_exact_in_the_browser() {
    // A loop counts past 2^53 exactly, and across it (FEEDBACK55.md, Q-776; branch feedback55).
    let defs = r#"fn loop_big() {
    let out = []
    for i in range(9007199254740993, 9007199254740995) { out = out + [i + 1] }
    for i in 9007199254740991..9007199254740993 { out = out + [[i, [10, 20][i - 9007199254740991]]] }
    out
}
fn loop_bad() {
    for i in v(1.5)..9007199254740995 { print(i) }
    0
}
"#;
    same_on_both(defs, &[
        (r#"loop_big()"#, r#"[["int", "int", ["int", "int"], ["int", "int"]], [9007199254740994, 9007199254740995, [9007199254740991, 10], [9007199254740992, 20]]]"#),
        (r#"loop_bad()"#, r#"ERROR: `..` needs two ints"#),
        (r#"vec.cumsum(v(["a", 1]))"#, r#"ERROR: `vec.cumsum` expects numbers, got str"#),
        (r#"vec.cumprod(v([dec("1"), 2]))"#, r#"ERROR: `vec.cumprod` expects numbers, got dec (this works on floats: convert a dec with float(x))"#),
        (r#"[v(9007199254740992) == v(9007199254740992.0), v(9007199254740993) == v(9007199254740993), v(9007199254740993) != v(9007199254740992)]"#, r#"[["bool", "bool", "bool"], [true, true, true]]"#),
        (r#"[contains([9007199254740992], 9007199254740992.0), index_of([9007199254740992.0], 9007199254740992), unique([9007199254740992, 9007199254740992.0]), remove([9007199254740992, 1], 9007199254740992.0)]"#, r#"[["bool", "int", ["int"], ["int"]], [true, 0, [9007199254740992], [1]]]"#),
        (r#"match v(9007199254740992) { 9007199254740992.0 => "yes", _ => "no" }"#, r#"["str", "yes"]"#),
        (r#"[sort([9007199254740995, 9007199254740993, 1, -9007199254740993]), min([9007199254740993, 9007199254740994]), max([-9007199254740993, -9007199254740994]), 9007199254740993 < 9007199254740994, 9007199254740993 > 1.5, -9007199254740993 < -1e300]"#, r#"[[["int", "int", "int", "int"], "int", "int", "bool", "bool", "bool"], [[-9007199254740993, 1, 9007199254740993, 9007199254740995], 9007199254740993, -9007199254740993, true, true, false]]"#),
        (r#"[sqrt(9007199254740993), floor(9007199254740993), ceil(9007199254740993), round(9007199254740993), abs(-9007199254740993), math.sqrt(9007199254740993), math.log(9007199254740993)]"#, r#"[["float", "int", "int", "int", "int", "float", "float"], [94906265.62425156, 9007199254740993, 9007199254740993, 9007199254740993, 9007199254740993, 94906265.62425156, 36.7368005696771]]"#),
        (r#"[math.isqrt(9007199254740993), math.gcd(9007199254740993, 3), math.lcm(9007199254740993, 2), math.floor(9007199254740993), math.trunc(1e17), math.ceil(-1e17)]"#, r#"[["int", "int", "int", "int", "int", "int"], [94906265, 3, 18014398509481986, 9007199254740993, 100000000000000000, -100000000000000000]]"#),
        (r#"math.floor(1e19)"#, r#"ERROR: integer overflow"#),
        (r#"[stats.mean([9007199254740993, 1]), stats.median([9007199254740993, 1, 3]), stats.median([9007199254740993]), stats.describe([9007199254740993]).sum]"#, r#"[["float", "int", "float", "float"], [4503599627370496, 3, 9007199254740992, 9007199254740992]]"#),
        (r#"[vec.add([9007199254740993], [0]), vec.mul([9007199254740993], 2), vec.sub(9007199254740993, [1, 2]), vec.cumsum([9007199254740993, 1]), vec.cumsum([9007199254740991, 1, 1]), vec.diff([9007199254740993, 9007199254740995])]"#, r#"[[["int"], ["int"], ["int", "int"], ["int", "int"], ["int", "int", "int"], ["int"]], [[9007199254740993], [18014398509481986], [9007199254740992, 9007199254740991], [9007199254740993, 9007199254740994], [9007199254740991, 9007199254740992, 9007199254740993], [2]]]"#),
        (r#"vec.cumprod([3037000500, 3037000500])"#, r#"ERROR: integer overflow"#),
        (r#"vec.mul([3037000500], 3037000500)"#, r#"ERROR: integer overflow"#),
        (r#"[vec.arange(9007199254740990, 9007199254740993), vec.dot([9007199254740993], [1]), math.sumprod([4294967296], [2147483647])]"#, r#"[[["int", "int"], "int", "int"], [[9007199254740990, 9007199254740991], 9007199254740993, 9223372032559808512]]"#),
        (r#"math.sumprod([4294967296], [4294967296])"#, r#"ERROR: integer overflow"#),
        (r#"[range(9007199254740993, 9007199254740995), 9007199254740993..9007199254740995, len(range(9007199254740990, 9007199254740995))]"#, r#"[[["int", "int"], ["int", "int"], "int"], [[9007199254740993, 9007199254740994], [9007199254740993, 9007199254740994], 5]]"#),
        (r#"[math.factorial(19), math.factorial(20), math.comb(60, 30), math.comb(66, 33), math.perm(20, 19), math.prod([4294967296, 4194304]), math.prod([2, 3]), math.prod([2.0, 3])]"#, r#"[["int", "int", "int", "int", "int", "int", "int", "float"], [121645100408832000, 2432902008176640000, 118264581564861424, 7219428434016265740, 2432902008176640000, 18014398509481984, 6, 6]]"#),
        (r#"math.comb(67, 33)"#, r#"ERROR: integer overflow"#),
        (r#"math.factorial(21)"#, r#"ERROR: integer overflow"#),
        (r#"math.comb(68, 34)"#, r#"ERROR: integer overflow"#),
        (r#"math.perm(21, 21)"#, r#"ERROR: integer overflow"#),
        (r#"math.prod([4294967296, 4294967296])"#, r#"ERROR: integer overflow"#),
        (r#"math.lcm(9223372036854775807, 2)"#, r#"ERROR: integer overflow"#),
        (r#"[v(3037000499) * v(3037000499), v(9007199254740991) + v(1), v(-9007199254740991) - v(1), 9223372036854775807 - 1, -9223372036854775807 - 1]"#, r#"[["int", "int", "int", "int", "int"], [9223372030926249001, 9007199254740992, -9007199254740992, 9223372036854775806, -9223372036854775808]]"#),
        (r#"v(3037000500) * v(3037000500)"#, r#"ERROR: integer overflow"#),
        (r#"9223372036854775807 + 1"#, r#"ERROR: integer overflow"#),
        (r#"[9007199254740993 + 1, 9007199254740993 - 1, 9007199254740993 * 1, 9007199254740993 / 1, 9007199254740993 % 2, -9007199254740993, 9007199254740993 / 2, 9007199254740993 + 0.5]"#, r#"[["int", "int", "int", "int", "int", "int", "float", "float"], [9007199254740994, 9007199254740992, 9007199254740993, 9007199254740993, 1, -9007199254740993, 4503599627370496, 9007199254740992]]"#),
        (r#"[int(-9.223372036854775808e18), json.parse("-9223372036854775808"), json.parse("9223372036854775807"), json.parse("9223372036854775808"), json.parse("-9223372036854775809"), json.parse("[9007199254740993]")]"#, r#"[["int", "int", "int", "float", "float", ["int"]], [-9223372036854775808, -9223372036854775808, 9223372036854775807, 9223372036854776000, -9223372036854776000, [9007199254740993]]]"#),
        (r#"[[1, 2, 3][9007199254740993], "abc"[9007199254740993], { a: 1 }[9007199254740993]]"#, r#"[["null", "null", "null"], [null, null, null]]"#),
        (r#"[bit.and(9007199254740993, 1), bit.shl(1, 62), bit.and(v(3.0), 1), json.stringify([9007199254740993, -9007199254740993])]"#, r#"[["int", "int", "int", "str"], [1, 4611686018427387904, 1, "[9007199254740993,-9007199254740993]"]]"#),
        (r#"math.factorial(v(5.0))"#, r#"["int", 120]"#),
        (r#"[math.gcd(v([12.0, 18])), math.comb(v(5.0), 2), math.ldexp(1.5, v(2.0))]"#, r#"[["int", "int", "float"], [6, 10, 6]]"#),
    ]);
}

// Q-778: the browser ordered text by UTF-16 unit, which puts U+E000 to U+FFFF after the characters
// past U+FFFF; trimmed with JavaScript's set (U+FEFF, not U+0085); and `replace(s, "", x)` put x
// between UTF-16 units only, splitting an emoji. Code point order, Unicode's White_Space and the
// server's empty pattern (before each character and at the end) now, and the patterns that are
// not empty as before.
#[test]
fn text_orders_trims_and_replaces_as_on_the_server() {
    let defs = "";
    same_on_both(defs, &[
        (r#"[sort(["b", "a", "Ａ", "😀", "\ue000", "z", ""]), join(sort(["Ａ", "😀"]), " ")]"#, r#"[[["str", "str", "str", "str", "str", "str", "str"], "str"], [["", "a", "b", "z", "", "Ａ", "😀"], "Ａ 😀"]]"#),
        (r#"["😀" < "\uffff", "\ue000" < "😀", "\uffff" < "😀", "😀" > "\ufffd", max(["😀", "\uffff"]), min(["😀", "\uffff"]), "a" < "ab", "" < "a", "é" > "z"]"#, r#"[["bool", "bool", "bool", "bool", "str", "str", "bool", "bool", "bool"], [false, true, true, true, "😀", "￿", true, true, true]]"#),
        (r#"[sort([[1, "😀"], [1, "\uffff"]]), sort_by(["😀", "ﬁ", "a"], (s) => s)]"#, r#"[[[["int", "str"], ["int", "str"]], ["str", "str", "str"]], [[[1, "￿"], [1, "😀"]], ["a", "ﬁ", "😀"]]]"#),
        (r#"[len(trim("\ufeffname")), len(trim("name\u0085")), len(trim("\u00a0x\u3000")), len(trim("\u200bx")), trim("  x  "), trim(""), trim("\t\n x \r\u2029")]"#, r#"[["int", "int", "int", "int", "str", "str", "str"], [5, 4, 1, 2, "x", "", "x"]]"#),
        (r#"[replace("abc", "", "-"), replace("😀", "", "-"), replace("", "", "-"), replace("é", "", "-"), replace("a😀b", "", ""), replace("aXbX", "X", ""), replace("abc", "b", "")]"#, r#"[["str", "str", "str", "str", "str", "str", "str"], ["-a-b-c-", "-😀-", "-", "-é-", "a😀b", "ab", "ac"]]"#),
    ]);
}

// Q-780: the server refuses to put NaN in order (FEEDBACK49.md, Q-605) and the browser sorted it,
// gave it as `max`, and `NaN < 1` was false. Refused the same way now, in `sort`, `sort_by`, `min`,
// `max`, `<` and the rest, `stats.median`; `vec.argsort` puts it last as the server's keys do, and
// the infinities are ordered as before.
#[test]
fn nan_cannot_be_put_in_order_in_the_browser_either() {
    let defs = "";
    same_on_both(defs, &[
        (r#"sort([math.nan, 1])"#, r#"ERROR: `sort` cannot compare int with float"#),
        (r#"max([math.nan, 1])"#, r#"ERROR: `max` cannot compare int with float"#),
        (r#"min([1, math.nan])"#, r#"ERROR: `min` cannot compare float with int"#),
        (r#"v(math.nan) < 1"#, r#"ERROR: cannot compare float with int"#),
        (r#"v(1) >= v(math.nan)"#, r#"ERROR: cannot compare int with float"#),
        (r#"sort_by([1, 2], (x) => math.nan)"#, r#"ERROR: `sort_by` cannot compare float with float"#),
        (r#"stats.median([math.nan, 1])"#, r#"ERROR: `stats.median`: the data hold NaN at index 0: NaN and infinity cannot be counted or put in order"#),
        (r#"sort([[math.nan], [1]])"#, r#"ERROR: `sort` cannot compare list with list"#),
        (r#"[vec.argsort([math.nan, 1]), vec.argsort([1, math.nan, 0], true), vec.argsort([2, math.nan, math.nan, 1]), math.nan == math.nan, math.nan != math.nan, contains([math.nan], math.nan)]"#, r#"[[["int", "int"], ["int", "int", "int"], ["int", "int", "int", "int"], "bool", "bool", "bool"], [[1, 0], [1, 0, 2], [3, 0, 1, 2], false, true, false]]"#),
        (r#"[sort([math.inf, 1, -math.inf]), max([math.inf, 1]), vec.argmin([math.nan, 3, 1]), vec.argmax([math.nan])]"#, r#"[[["float", "int", "float"], "float", "int", "null"], [[-inf, 1, inf], inf, 2, null]]"#),
    ]);
}

// Q-781 in the browser: a whole float is written `2.0` in JSON as the server writes it (`-0.0`, and
// past 1e21 as JavaScript writes it), and read back as a float on both sides; ints, and an int past
// 2^53, as digits.
#[test]
fn json_writes_a_float_as_the_server_does_on_both_sides() {
    let defs = "";
    same_on_both(defs, &[
        (r#"json.stringify([2.0, 2, 2.5, -0.0, 0.0, 1e21, 1e16, 1e20, 0.0000015, 1e-7, 1.5e-6, -1e21, 1e300, 123e-20])"#, r#"["str", "[2.0,2,2.5,-0.0,0.0,1e+21,10000000000000000.0,100000000000000000000.0,0.0000015,1e-7,0.0000015,-1e+21,1e+300,1.23e-18]"]"#),
        (r#"json.stringify({ a: 2.0, b: [1.0, { c: -2.0 }] }, true)"#, r#"["str", "{\n  \"a\": 2.0,\n  \"b\": [\n    1.0,\n    {\n      \"c\": -2.0\n    }\n  ]\n}"]"#),
        (r#"[json.parse(json.stringify(2.0)), json.parse(json.stringify(-0.0)), json.parse(json.stringify(1e21)), json.parse(json.stringify([1.0, 2]))]"#, r#"[["float", "float", "float", ["float", "int"]], [2, 0, 1000000000000000000000, [1, 2]]]"#),
        (r#"[json.stringify(sqrt(4)), json.stringify(9007199254740993), json.stringify(1.0 / 3), json.stringify(math.inf)]"#, r#"[["str", "str", "str", "str"], ["2.0", "9007199254740993", "0.3333333333333333", "null"]]"#),
    ]);
}

// Q-782: `json.parse` took 1e400 (as inf) and half of a character, which the server refuses;
// `??` missed a stop in a field or an index to its left (`v("abc").foo ?? "none"`); a map's key
// of letters of another script was quoted; `float("0x10")` was 16; with `m`, U+2028 and U+2029
// ended a line for `^` and `$`, and `^` matched between \r and \n; `posixrules` was a zone on the
// server, `US/Pacific-New` and Java's `PST` in the browser. Each as the server has it, or refused
// on both: a zone is a name of the IANA database, its zones and links in any case.
#[test]
fn smaller_differences_between_the_sides_are_gone() {
    let defs = "";
    same_on_both(defs, &[
        // What a Linux tzdata has besides: this machine's zone, and the copies (found in review).
        (r#"[date.now("localtime") ?? "refused", date.now("posix/UTC") ?? "refused", date.now("right/UTC") ?? "refused", date.now("Posix/Europe/Paris") ?? "refused"]"#, r#"[["str", "str", "str", "str"], ["refused", "refused", "refused", "refused"]]"#),
        (r#"[is_err(json.parse("1e400")), is_err(json.parse("-1e400")), json.parse("1e308"), json.parse("1e-400"), is_err(json.parse("\"\\ud83d\"")), is_err(json.parse("\"\\udc00x\"")), json.parse("\"\\ud83d\\ude00\""), is_err(json.parse("{\"\\ud800\": 1}"))]"#, r#"[["bool", "bool", "float", "float", "bool", "bool", "str", "bool"], [true, true, 100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000, 0, true, true, "😀", true]]"#),
        (r#"[v("abc").foo ?? "none", [1, 2][v(1.5)] ?? 10, v({ a: { b: 1 } }).a.b ?? 0, v({ a: 1 }).a.b ?? 7, v([[1]])[0][5] ?? 8, v({}).x ?? 9, v(null).foo ?? "n", v([1])[v("x")] ?? "i", v({ a: 1 }).a ?? 5]"#, r#"[["str", "int", "int", "int", "int", "int", "str", "str", "int"], ["none", 10, 1, 7, 8, 9, "n", "i", 1]]"#),
        (r#"str({ é: 1, Ω: 2, 日本: 3, a²: 4, ǅ: 5, _x: 6, a1: 7, "1a": 8, "a b": 9, "": 10, "x-y": 11, Ⅷ: 12, "٣": 13 })"#, r#"["str", "{ é: 1, Ω: 2, 日本: 3, a²: 4, ǅ: 5, _x: 6, a1: 7, \"1a\": 8, \"a b\": 9, \"\": 10, \"x-y\": 11, Ⅷ: 12, \"٣\": 13 }"]"#),
        (r#"[float("0x10"), float(" 0x10 "), float("0b11"), float("0o17"), float("16"), float(" 16 ")]"#, r#"[["error", "error", "error", "error", "float", "float"], [err("not a number: \"0x10\""), err("not a number: \" 0x10 \""), err("not a number: \"0b11\""), err("not a number: \"0o17\""), 16, 16]]"#),
        (r#"[re.find_all("a\u2028b", "^.", "m") | map((m) => m.text), re.find_all("a\r\nb", "^", "m") | map((m) => m.start), re.find_all("a\r\nb", "$", "m") | map((m) => m.start), re.find_all("a\rb\nc", "^\\w", "m") | map((m) => m.text), re.test("x\u2029y", "x$", "m"), re.test("x\u2029y", "x$"), re.replace("a\nb", "^", ">", "m"), re.find_all("[$^]", "[$^]", "m") | map((m) => m.text), re.test("a\u2028", "a$", "m")]"#, r#"[[["str"], ["int", "int"], ["int", "int"], ["str", "str", "str"], "bool", "bool", "str", ["str", "str"], "bool"], [["a"], [0, 3], [1, 4], ["a", "b", "c"], false, false, ">a\n>b", ["$", "^"], false]]"#),
        (r#"[date.now("posixrules") ?? "refused", date.now("Factory") ?? "refused", date.now("Etc/Unknown") ?? "refused", date.now("US/Pacific-New") ?? "refused", date.now("Canada/East-Saskatchewan") ?? "refused", date.now("SystemV/AST4") ?? "refused", date.now("PST") ?? "refused", date.now("pst") ?? "refused"]"#, r#"[["str", "str", "str", "str", "str", "str", "str", "str"], ["refused", "refused", "refused", "refused", "refused", "refused", "refused", "refused"]]"#),
        (r#"[date.now("Europe/Paris") != null, date.now("europe/paris") != null, date.now("EST") != null, date.now("US/Pacific") != null, date.now("Asia/Calcutta") != null, date.now("UTC") != null]"#, r#"[["bool", "bool", "bool", "bool", "bool", "bool"], [true, true, true, true, true, true]]"#),
    ]);
}

/// `src` served with `opts`, and `body` run in Chrome on `path` once the runtime has started, as
/// `in_page` does, with what the console warned in `warned`. None without Chrome.
fn in_page_with(name: &str, src: &str, opts: &[&str], path: &str, body: &str) -> Option<(serde_json::Value, String)> {
    let (ok, out) = check(src);
    assert!(ok, "{}", out);
    let d = dir(name, src);
    let s = support::serve_with(&d.join("main.qwe"), opts);
    let html = support::send(s.port, "GET", path, &[], "").body;
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return None;
    };
    let script = format!(
        "(async () => {{\n  const warned = [];\n  const warn = console.warn;\n  console.warn = (...a) => {{ warned.push(a.join(\" \")); return warn.apply(console, a); }};\n  {BOOTED}\n  const $ = (id) => document.getElementById(id);\n  const out = await (async () => {{\n{body}\n  }})();\n  out.remade = QW.remade ?? 0;\n  out.errors = window.__quilloErrors;\n  out.warned = warned.filter((w) => w.startsWith(\"quillo:\"));\n  document.body.dataset.result = JSON.stringify(out);\n}})();"
    );
    Some((in_chrome(&chrome, &format!("http://127.0.0.1:{}{}", s.port, path), &script), html))
}

// Q-772: the server wrote a CR and a NUL in text as they are, which the HTML parser makes a newline
// and drops, so the page said otherwise than what the browser computes, and the component was made
// again on every load: two lines typed in a <textarea> and posted come with CRLF. The server writes
// a CR `&#13;` (in an attribute too, as before) and a NUL U+FFFD, the browser shows a NUL as U+FFFD,
// and the page is taken up as written: in a state, a prop, an attribute and a <textarea>, a CR
// alone, one at the end added later, text with neither.
#[test]
fn text_with_a_cr_or_a_nul_is_taken_up_as_the_server_wrote_it() {
    let src = r##"component Note(p: str) {
    state s = "line one\r\nline two"
    state t = "a\u0000b"
    <div><p #a>{s}</p><p #b>{p}</p><p #c>{t}</p><p #d title={s}>x</p><textarea #e>{s}</textarea><p #f>{"a\rb"}</p><p #g>{"plain"}</p><p #h title={t}>y</p><p #i>{"\r"}</p>
    <button #go @click={() => { s = s + "\r"; t = t + "\u0000" }}>go</button></div>
}
route GET / = <html><head><title>t</title></head><body><Note p={"cr\rhere"}/></body></html>
"##;
    let body = r#"const text = (id) => $(id).textContent;
    const out = { a: text("a"), b: text("b"), c: text("c"), d: $("d").getAttribute("title"), e: text("e"), f: text("f"), g: text("g"), h: $("h").getAttribute("title"), i: text("i") };
    $("go").click(); await tick(60);
    out.after = [text("a"), text("c")];
    return out;"#;
    let Some((r, html)) = in_page("q772", src, "/", body) else { return };
    assert!(html.contains("<p id=\"a\">line one&#13;\nline two</p>") && html.contains("<p id=\"c\">a\u{FFFD}b</p>") && html.contains("cr&#13;here"), "{}", html);
    assert!(html.contains("title=\"line one&#13;\nline two\"") && html.contains("<p id=\"g\">plain</p>") && html.contains("<p id=\"i\">&#13;</p>"), "{}", html);
    assert!(!html.contains('\r') && !html.contains('\0'), "{:?}", html);
    let want = serde_json::json!({
        "a": "line one\r\nline two", "b": "cr\rhere", "c": "a\u{FFFD}b", "d": "line one\r\nline two", "e": "line one\r\nline two",
        "f": "a\rb", "g": "plain", "h": "a\u{FFFD}b", "i": "\r", "after": ["line one\r\nline two\r", "a\u{FFFD}b\u{FFFD}"], "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Q-774: an attribute was not compared as a component was taken up: the browser's value silently
// took the server's place, so no difference between the sides was ever seen. Under `--watch` it is
// compared as text is: a page script that changes, removes or adds one the markup sets makes that
// component again (with the markup's value), said with what the page and the browser have; the
// others are taken up as they are, and so is one where a person typed (a field's value is the
// person's) or a script added a class or an attribute the markup does not set. In production the
// browser's value takes its place, as before, and nothing is made again (decided in review: a
// browser extension rewriting a style would make components again on every load).
#[test]
fn an_attribute_that_differs_makes_the_component_again() {
    let src = r##"component Card(label: str) {
    state n = 0
    state gone = null
    state on = true
    <div class="card"><span #lab title={label} data-n={n} aria-busy={on} hidden={gone}>{label}</span><input value={label} readonly/><button class="bump" @click={() => { n += 1 }}>{n}</button></div>
}
component Box() {
    <main><Card label="kept"/><Card label="title"/><Card label="removed"/><Card label="added"/><Card label="typed"/><Card label="classed"/></main>
}
// What a script of the page's does before the page is taken up.
let change = "const c = [...document.querySelectorAll('.card')], s = (i) => c[i].querySelector('span'); s(1).setAttribute('title', 'changed'); s(2).removeAttribute('data-n'); s(3).setAttribute('hidden', ''); c[4].querySelector('input').value = 'typed'; s(5).classList.add('extra'); s(5).setAttribute('data-extra', '1'); window.__before = c.map((x) => x.querySelector('span'));"
route GET / = <html><head><title>t</title></head><body><Box/><script>{raw(change)}</script></body></html>
"##;
    let body = r#"const before = window.__before;
    await tick(100);
    for (const c of document.querySelectorAll(".card")) c.querySelector(".bump").click();
    await tick(150);
    const now = [...document.querySelectorAll(".card")];
    return {
      same: now.map((c, i) => c.querySelector("span") === before[i]),
      titles: now.map((c) => c.querySelector("span").getAttribute("title")),
      n: now.map((c) => c.querySelector("span").getAttribute("data-n")),
      hidden: now.map((c) => c.querySelector("span").hasAttribute("hidden")),
      typed: now[4].querySelector("input").value,
      cls: now[5].querySelector("span").className + " " + now[5].querySelector("span").getAttribute("data-extra"),
    };"#;
    let Some((r, _)) = in_page_with("q774p", src, &[], "/", body) else { return };
    let want = serde_json::json!({
        "same": [true, true, true, true, true, true], "titles": ["kept", "title", "removed", "added", "typed", "classed"],
        "n": ["1", "1", "1", "1", "1", "1"], "hidden": [false, false, false, false, false, false], "typed": "typed", "cls": "extra 1",
        "remade": 0, "errors": [], "warned": [],
    });
    assert_eq!(r, want);
    let Some((r, _)) = in_page_with("q774", src, &["--watch"], "/", body) else { return };
    assert_eq!(r["same"], serde_json::json!([true, false, false, false, true, true]), "{}", r);
    assert_eq!(r["titles"], serde_json::json!(["kept", "title", "removed", "added", "typed", "classed"]), "{}", r);
    assert_eq!(r["n"], serde_json::json!(["1", "1", "1", "1", "1", "1"]), "{}", r);
    assert_eq!(r["hidden"], serde_json::json!([false, false, false, false, false, false]), "{}", r);
    assert_eq!((r["typed"].as_str(), r["cls"].as_str(), r["remade"].as_i64()), (Some("typed"), Some("extra 1"), Some(3)), "{}", r);
    let warned = r["warned"].as_array().unwrap();
    for w in [r#"where it wrote title="changed", the browser makes title="title""#, r#"where it wrote no data-n, the browser makes data-n="0""#, r#"where it wrote hidden="", the browser makes no hidden"#] {
        assert!(warned.iter().any(|x| x.as_str().unwrap().contains(w)), "{} in {:?}", w, warned);
    }
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);
}

// Q-775 across the wire: a whole float in a state, a prop and a server fn's answer was an int once
// the browser had it, and one the page sent a server fn arrived an int. Floats keep their type
// both ways (the server writes `2.0`, the browser reads the text it was written as, and writes
// `2.0` back), so the page is taken up as written, `a / 0` is inf, and the server reads them as
// floats; an int past 2^53, -0.0 and 1e21 too, and ints stay ints.
#[test]
fn floats_cross_to_the_page_and_back_as_floats() {
    let src = r##"server fn kinds(xs) = xs | map((x) => type_of(x))
server fn two() = 2.0
server fn halves() = [1.0, 2.5, -0.0, 9007199254740993, 1e21]
component Floats(p: float, q) {
    state a = 2.0
    state b = [1.0, 2, 2.5]
    state c = 9007199254740993
    state d = { x: 3.0 }
    state got = ""
    state back = ""
    state z = 0
    <div><p #types>{json.stringify([type_of(p), type_of(q), type_of(a), b | map((x) => type_of(x)), type_of(c), type_of(d.x)])}</p>
    <p #vals title={a} data-p={p}>{p} {a} {b} {c} {d.x} {a / z} {json.stringify([p, a, b, c, d])}</p>
    <p #got>{got}</p><p #back>{back}</p>
    <button #go @click={() => { got = json.stringify([type_of(two()), halves() | map((x) => type_of(x)), two() / z]); back = json.stringify(kinds([a, 2, 2.5, -0.0, c, d.x, p])) }}>go</button></div>
}
route GET / = <html><head><title>t</title></head><body><Floats p={2.0} q={1e21}/></body></html>
"##;
    let body = r#"const out = { types: JSON.parse($("types").textContent), vals: $("vals").textContent, title: $("vals").getAttribute("title") };
    $("go").click();
    for (let i = 0; i < 100 && !$("back").textContent; i++) await tick(30);
    out.got = JSON.parse($("got").textContent);
    out.back = JSON.parse($("back").textContent);
    return out;"#;
    let Some((r, html)) = in_page("q775", src, "/", body) else { return };
    assert!(html.contains("<p id=\"vals\" title=\"2\" data-p=\"2\">2 2 122.5 9007199254740993 3 inf [2.0,2.0,[1.0,2,2.5],9007199254740993,{&quot;x&quot;:3.0}]</p>"), "{}", html);
    let want = serde_json::json!({
        "types": ["float", "float", "float", ["float", "int", "float"], "int", "float"],
        "vals": "2 2 122.5 9007199254740993 3 inf [2.0,2.0,[1.0,2,2.5],9007199254740993,{\"x\":3.0}]", "title": "2",
        "got": ["float", ["float", "float", "float", "int", "float"], null],
        "back": ["float", "int", "float", "float", "int", "float", "float"],
        "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Q-779: markup made into text in the browser was "<html>", empty in `json.stringify`; `len` of it
// stopped and two of the same markup were not equal. Its text is its markup on both sides, written
// as the server writes HTML (`&amp;`, `&lt;`, `&quot;`, `&#39;`, a void element without its end, an
// attribute with no value bare), so the page is taken up as written; markup made later too, and
// `raw`'s own text.
#[test]
fn markup_made_into_text_is_its_html_in_the_browser_too() {
    let src = r##"component H() {
    state n = 0
    let h = <b>x</b>
    let a = <a href="/x?a=1&b=2" title={"it's \"q\""}>{"a < b & c"}</a>
    <div><p #s>{str(h)}</p><p #c>{"" + h}</p><p #t>{`${h}!`}</p><p #j>{json.stringify({ h: h })}</p><p #l>{len(h)}</p><p #e>{str(h == <b>x</b>)}</p><p #ne>{str(h == <b>y</b>)}</p>
    <p #a>{str(a)}</p><p #v>{str(<p>one<br>two</p>)}</p><p #i>{str(<input disabled value="v">)}</p><p #r>{str(raw("<i>r</i>"))}</p><p #ls>{str([h, 1])}</p><p #at title={h}>t</p>
    <p #n>{n > 0 ? str(<b class="k">{n}</b>) : ""}</p><button #go @click={() => { n += 1 }}>go</button>
    <p #ea>{str(<img src="x.png" alt="">)}</p><p #et>{str(<p title="">x</p>)}</p><p #vt>{str(<input value="v" type="text">)}</p>
    <p #bt>{str(<input type="checkbox" checked disabled={true} title={""}>)}</p><p #ot>{str(<a title={"t"} href="/h" class="c" id="i">y</a>)}</p>
    <p #nt>{str(<input value={"w"} type="text" title={n > 5 ? "big" : ""}>)}</p></div>
}
route GET / = <html><head><title>t</title></head><body><H/></body></html>
"##;
    let body = r#"const ids = ["s", "c", "t", "j", "l", "e", "ne", "a", "v", "i", "r", "ls", "ea", "et", "vt", "bt", "ot", "nt"];
    const out = {};
    for (const id of ids) out[id] = $(id).textContent;
    out.at = $("at").getAttribute("title");
    $("go").click(); await tick(60);
    out.n = $("n").textContent;
    return out;"#;
    let Some((r, html)) = in_page("q779", src, "/", body) else { return };
    assert!(html.contains("<p id=\"s\">&lt;b&gt;x&lt;/b&gt;</p>"), "{}", html);
    let want = serde_json::json!({
        "s": "<b>x</b>", "c": "<b>x</b>", "t": "<b>x</b>!", "j": "{\"h\":\"<b>x</b>\"}", "l": "8", "e": "true", "ne": "false",
        "a": "<a href=\"/x?a=1&amp;b=2\" title=\"it&#39;s &quot;q&quot;\">a &lt; b &amp; c</a>", "v": "<p>one<br>two</p>", "i": "<input disabled value=\"v\">",
        "r": "<i>r</i>", "ls": "[<b>x</b>, 1]", "at": "<b>x</b>", "n": "<b class=\"k\">1</b>",
        // An attribute of "" is written `=""`, one of `true` bare, in the source's order (found in review).
        "ea": "<img src=\"x.png\" alt=\"\">", "et": "<p title=\"\">x</p>", "vt": "<input value=\"v\" type=\"text\">",
        "bt": "<input type=\"checkbox\" checked disabled title=\"\">", "ot": "<a class=\"c\" title=\"t\" href=\"/h\" id=\"i\">y</a>",
        "nt": "<input value=\"w\" type=\"text\" title=\"\">", "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Review of Q-775 and Q-776, in Chrome: a state, a prop, a server fn's answer, `http.get`'s JSON, a
// `listen`ed stream (read with plain JSON.parse before, on every browser) and `json.parse` keep a
// whole float a float and an int past 2^53 exact, with JSON.parse's text of the numbers and with
// a page where JSON.parse gives none, as older browsers' does: the page is taken up as written.
#[test]
fn numbers_from_the_server_keep_their_type_in_any_browser() {
    let src = r##"fn tys(x) = if type_of(x) == "list" { x | map((y) => tys(y)) } else if type_of(x) == "map" { keys(x) | map((k) => [k, tys(x[k])]) } else { type_of(x) }
server fn answer() = { a: 2.0, n: 9007199254740993, f: [1.0, 2.5, -0.0], i: 7 }
route GET /data = { a: 2.0, n: 9007199254740993 }
route GET /nums = events((send) => { send({ a: 3.0, n: 9007199254740995 }) })
component Nums(p: float) {
    state s = { a: 2.0, n: 9007199254740993, f: [1.0, 2] }
    state got = ""
    state far = ""
    state heard = ""
    listen("/nums", (m) => { heard = json.stringify([tys(m), m.n + 1]) })
    <div><p #s>{json.stringify([tys(s), type_of(p), s.n + 1])}</p><p #got>{got}</p><p #far>{far}</p><p #heard>{heard}</p>
    <p #parsed>{json.stringify(tys(json.parse("[2.0, 9007199254740993, \"1.0\"]")))}</p>
    <button #go @click={() => {
        let a = answer()
        got = json.stringify([tys(a), a.n + 1])
        let r = http.get("/data")
        far = json.stringify([tys(r.json), r.json.n + 1])
    }}>go</button></div>
}
let old_json = "const P = JSON.parse; JSON.parse = function (t, r) { return r ? P(t, function (k, v) { return r.call(this, k, v); }) : P(t); };"
route GET / = <html><head><title>t</title></head><body><Nums p={2.0}/></body></html>
route GET /old = <html><head><title>t</title><script>{raw(old_json)}</script></head><body><Nums p={2.0}/></body></html>
"##;
    let body = r#"let context = false;
    JSON.parse("1", (k, v, c) => { context = c !== undefined; return v; });
    $("go").click();
    for (let i = 0; i < 100 && !($("got").textContent && $("far").textContent && $("heard").textContent); i++) await tick(30);
    return { context, s: $("s").textContent, got: $("got").textContent, far: $("far").textContent, heard: $("heard").textContent, parsed: $("parsed").textContent };"#;
    let same = serde_json::json!({
        "s": r#"[[["a","float"],["n","int"],["f",["float","int"]]],"float",9007199254740994]"#,
        "got": r#"[[["a","float"],["n","int"],["f",["float","float","float"]],["i","int"]],9007199254740994]"#,
        "far": r#"[[["a","float"],["n","int"]],9007199254740994]"#,
        "heard": r#"[[["a","float"],["n","int"]],9007199254740996]"#,
        "parsed": r#"["float","int","str"]"#,
        "remade": 0, "errors": [],
    });
    for (path, context) in [("/", true), ("/old", false)] {
        let Some((mut r, _)) = in_page("numbers-anywhere", src, path, body) else { return };
        assert_eq!(r["context"], serde_json::json!(context), "{}", path);
        r.as_object_mut().unwrap().remove("context");
        assert_eq!(r, same, "{}", path);
    }
}

// Review of Q-775: a whole float reached the page's JavaScript as the runtime's own object:
// `JSON.stringify({ a: 1.0 })` gave {"a":{"f":1}}, `Number.isFinite(2.0)` false, and a chart's
// numbers were objects. It is a number there, inside lists and maps too: given to `js(...)`, to a
// function the page gave, and set on the page's object.
#[test]
fn a_whole_float_is_a_number_to_the_pages_code() {
    let src = r##"component Lib() {
    state out = ""
    <div><p #out>{out}</p><button #go @click={() => {
        let o = js("pagelib.box")
        o.n = 2.0
        let kind = js("pagelib.maker")
        o.m = { x: 2.0 }
        out = json.stringify([js("JSON.stringify", { a: 1.0, b: [2.0, 2.5], c: { d: -0.0 }, e: 3 }), js("Number.isFinite", 2.0), js("pagelib.kind", 3.0), kind(4.0), kind([5.0]), kind({ x: 6.0 }), kind({ x: 7 }), js("pagelib.read")])
    }}>go</button></div>
}
let lib = "window.pagelib = { box: new (class Box {})(), kind: (v) => Array.isArray(v) ? typeof v[0] : v && typeof v === 'object' ? typeof v.x : typeof v, maker: () => window.pagelib.kind, read: () => typeof window.pagelib.box.n + ' ' + window.pagelib.box.n + ' ' + typeof window.pagelib.box.m.x };"
route GET / = <html><head><title>t</title><script>{raw(lib)}</script></head><body><Lib/></body></html>
"##;
    let body = r#"$("go").click(); await tick(80); return { out: JSON.parse($("out").textContent) };"#;
    let Some((r, _)) = in_page("float-to-page", src, "/", body) else { return };
    let want = serde_json::json!({ "out": [r#"{"a":1,"b":[2,2.5],"c":{"d":0},"e":3}"#, true, "number", "number", "number", "number", "number", "number 2 number"], "remade": 0, "errors": [] });
    assert_eq!(r, want);
}

// Review of Q-775: a field held to a float state gave text for a whole number typed (`state amount
// = 1.0`, then `3`: "3", and `amount + 1` "31"), and an int for one the float state held (0.5).
// A float state gets a float, a whole one too, an int state an int (one given 1.5 too), as before.
#[test]
fn a_field_held_to_a_float_state_gives_floats() {
    let src = r##"component Amount() {
    state amount = 1.0
    state rate = 0.5
    state qty = 2
    state sums = ""
    <div><input #a value={amount}/><input #r bind:value={rate}/><input #q value={qty}/>
    <p #p>{type_of(amount)} {type_of(rate)} {type_of(qty)}</p><p #sums>{sums}</p>
    <button #sum @click={() => { sums = `${amount + 1} ${rate * 2} ${qty + 1}` }}>sum</button></div>
}
route GET / = <html><head><title>t</title></head><body><Amount/></body></html>
"##;
    let body = format!(
        r#"{TYPE_IN}
    const out = {{}};
    for (const v of ["3", "2.5", "0"]) {{
      await typeIn("a", v); await typeIn("r", v); await typeIn("q", v); await tick(40);
      $("sum").click(); await tick(40);
      out[v] = [$("p").textContent, $("sums").textContent];
    }}
    return out;"#
    );
    let Some((r, _)) = in_page("float-field", src, "/", &body) else { return };
    let want = serde_json::json!({
        "3": ["float float int", "4 6 4"], "2.5": ["float float float", "3.5 5 3.5"], "0": ["float float int", "1 0 1"],
        "remade": 0, "errors": [],
    });
    assert_eq!(r, want);
}

// Review of slice 9: a test hid errors for the whole process (`value::HIDING`) while it ran Node,
// and the database tests running beside it read `internal error`. A test hides them on its own
// thread now (`value::hiding_here`), for the interpreter and the browser's half alike, and the
// other threads are left as they were.
#[test]
fn errors_hidden_for_one_test_stay_shown_for_the_others() {
    let mut it = quillo::Interp::new(".");
    it.load_source("fn __t() = str(int(\"x\"))\n", "case.qwe").unwrap();
    let call = |it: &mut quillo::Interp| match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.display(),
        _ => panic!("the case failed"),
    };
    let (hidden, script, elsewhere) = quillo::value::hiding_here(true, || {
        let other = std::thread::spawn(quillo::value::hiding).join().unwrap();
        (call(&mut it), quillo::client::test_script(&it, "__t").unwrap(), other)
    });
    assert_eq!((hidden.as_str(), elsewhere), ("err(\"internal error\")", quillo::value::HIDING.load(std::sync::atomic::Ordering::Relaxed)));
    assert!(!script.contains("ERRORS_SHOWN = true"));
    let (shown, script) = quillo::value::hiding_here(false, || (call(&mut it), quillo::client::test_script(&it, "__t").unwrap()));
    assert_eq!(shown, "err(\"not an int: \\\"x\\\"\")");
    assert!(script.contains("ERRORS_SHOWN = true"));
}

// Review of Q-775 and Q-776: a browser whose JSON.parse gives no text of the numbers (Safari before
// 18.4, Firefox before 135) read a whole float as an int and an int past 2^53 with lost digits,
// in island state, server fns' answers, live data, channels, streams, `json.parse` and `http`. The
// numbers whose text says more are marked in the text first (outside strings), under a key no
// text has, and made from their text: the same answers as with it, a string holding a number's
// text left alone, and what is not JSON still refused.
#[test]
fn numbers_are_read_by_their_text_where_json_parse_cannot_say_it() {
    same_on_both_after(OLD_JSON, "", &[
        (r#"[json.parse("9007199254740993") + 1, json.parse("[2.0, 1e2, -0, 2.5, 9007199254740993, -9223372036854775808, 9223372036854775808, 12, \"2.0\", \"a\\\"9007199254740993\"]")]"#, r#"[["int", ["float", "float", "float", "float", "int", "int", "float", "int", "str", "str"]], [9007199254740994, [2, 100, 0, 2.5, 9007199254740993, -9223372036854775808, 9223372036854776000, 12, "2.0", "a\"9007199254740993"]]]"#),
        (r#"json.parse("{\"a\": 1.0, \"b\": [1E2, {\"c\": -0.0}], \"d\": \"x 1.0 \\\" 2.0 y\", \"e\": [0.5, -1.5e3]}")"#, r#"[[["a", "float"], ["b", ["float", [["c", "float"]]]], ["d", "str"], ["e", ["float", "float"]]], { a: 1, b: [100, { c: 0 }], d: "x 1.0 \" 2.0 y", e: [0.5, -1500] }]"#),
        (r#"[is_err(json.parse("01.0")), is_err(json.parse("[1.]")), is_err(json.parse("1e400")), is_err(json.parse("\"\\ud800\"")), json.parse("1e-400"), is_err(json.parse("{\"\\ud800\": 1.0}")), is_err(json.parse("[1.0e400]"))]"#, r#"[["bool", "bool", "bool", "bool", "float", "bool", "bool"], [true, true, true, true, 0, true, true]]"#),
        (r#"[json.parse("{\"\\u0000n\": \"2.0\"}"), json.parse("[]"), json.parse("{}"), json.parse("\"\""), json.parse("0"), json.parse("-0.5")]"#, r#"[[[["\u0000n", "str"]], [], [], "str", "int", "float"], [{ "\u0000n": "2.0" }, [], {}, "", 0, -0.5]]"#),
    ]);
}

// Review of Q-777: a decimal made from a float took Rust's digits on a tie (`{:e}`), the browser
// JavaScript's: `dec(1e15 + 0.3)` was ...0.3m on the server and ...0.2m in the browser, and so were
// `money.of`, `format_number` and `==` with a decimal. The even digit on both, and a value away from
// a tie as before.
#[test]
fn a_decimal_from_a_float_has_the_same_digits_on_both_sides() {
    same_on_both("", &[
        (r#"[dec(1e15 + 0.3), dec(1916218016406818.2), dec(-1916218016406818.2), dec(1e15 + 0.1), dec(0.1), dec(2.0), dec(1 + pow(2, -17)), dec(5e-324)]"#, r#"[["dec", "dec", "dec", "dec", "dec", "dec", "dec", "dec"], [1000000000000000.2m, 1916218016406818.2m, -1916218016406818.2m, 1000000000000000.1m, 0.1m, 2m, 1.0000076293945312m, 0.0000000000000000000000000000m]]"#),
        (r#"[1916218016406818.2 == dec("1916218016406818.2"), 1916218016406818.2 == dec("1916218016406818.3"), dec("0.1") == 0.1, money.of(dec(v(1e15 + 0.3)), "USD").amount, format_number(1916218016406818.2), format_number(1e15 + 0.3, "en-US")]"#, r#"[["bool", "bool", "bool", "dec", "str", "str"], [true, false, true, 1000000000000000.20m, "1,916,218,016,406,818.2", "1,000,000,000,000,000.2"]]"#),
    ]);
}

// Review of Q-772: `escape` wrote a CR and a NUL as they are in the browser and as `&#13;` and
// U+FFFD on the server, so `raw("<p>" + escape(note) + "</p>")` made the component again.
#[test]
fn escape_writes_a_cr_and_a_nul_the_same_on_both_sides() {
    same_on_both("", &[
        (r#"[escape("a\r\nb"), escape("a\rb"), escape("a\u0000b"), escape("<&>\"'"), escape("plain"), escape(""), escape("é😀\r"), escape(2.5)]"#, r#"[["str", "str", "str", "str", "str", "str", "str", "str"], ["a&#13;\nb", "a&#13;b", "a�b", "&lt;&amp;&gt;&quot;&#39;", "plain", "", "é😀&#13;", "2.5"]]"#),
    ]);
}

// Review of Q-776: builtins that take an int refused one past 2^53 in the browser, where the server
// clamps it (`slice`, `take`, `drop`, `round`'s digits, `cursor_index`), refuses it as too large
// (`repeat`, `set`'s index), or counts with it (`range`'s step, `random_int`).
#[test]
fn builtins_that_take_an_int_take_one_past_2_53() {
    same_on_both("", &[
        (r#"[slice("abc", 1, 9007199254740993), slice([1, 2, 3], -9007199254740993), slice("abc", 9007199254740993), take("abc", 9007199254740993), take([1], -9007199254740993), drop([1, 2], 9007199254740993), drop("ab", -9007199254740993), round(1.5, 9007199254740993), round(1.25, -9007199254740993), cursor_index("abc", 9007199254740993)]"#, r#"[["str", ["int", "int", "int"], "str", "str", [], [], "str", "float", "float", "int"], ["bc", [1, 2, 3], "", "abc", [], [], "ab", 1.5, 1, 3]]"#),
        (r#"repeat("a", 9007199254740993)"#, r#"ERROR: `repeat` count is negative or too large"#),
        (r#"repeat("a", -9007199254740993)"#, r#"ERROR: `repeat` count is negative or too large"#),
        (r#"set([1, 2], 9007199254740993, 0)"#, r#"ERROR: `set`: index 9007199254740993 is outside a list of 2"#),
        (r#"set([1, 2], -9007199254740993, 0)"#, r#"ERROR: `set`: index -9007199254740993 is outside a list of 2"#),
        (r#"[set([1, 2], 1, 0), set([1, 2], -1, 0), take("abc", 2), repeat("ab", 2)]"#, r#"[[["int", "int"], ["int", "int"], "str", "str"], [[1, 0], [1, 0], "ab", "abab"]]"#),
        (r#"[range(0, 9007199254740993, 4503599627370497), range(9007199254740993, 9007199254740999, 2), range(9007199254740999, 9007199254740993, -3), range(0, 3, 1), type_of(random_int(9007199254740993, 9007199254740995)), random_int(9007199254740993, 9007199254740993), random_int(-9223372036854775807, -9223372036854775807)]"#, r#"[[["int", "int"], ["int", "int", "int"], ["int", "int"], ["int", "int", "int"], "str", "int", "int"], [[0, 4503599627370497], [9007199254740993, 9007199254740995, 9007199254740997], [9007199254740999, 9007199254740996], [0, 1, 2], "int", 9007199254740993, -9223372036854775807]]"#),
        (r#"random_int(9007199254740995, 9007199254740993)"#, r#"ERROR: `random_int` needs lo <= hi, got 9007199254740995 and 9007199254740993"#),
        (r#"range(9007199254740993, 9007199254740995, 0)"#, r#"ERROR: `range`: a step of 0 never gets anywhere"#),
        (r#"[contains([9007199254740993, 9007199254740994, 9007199254740995], random_int(9007199254740993, 9007199254740995)), random_int(0, 4503599627370496) >= 0]"#, r#"[["bool", "bool"], [true, true]]"#),
    ]);
}

// ---- slice 9, the server's float text (Q-777, Q-781) ----

// Q-777: of two shortest digits as near, Rust took the upper and JavaScript the even one, so the
// server wrote `1916218016406818.3` (it is ...818.25) where the browser wrote `...818.2`. The
// server takes the even one too, in `str`, in a component's text and in JSON: below 2^37 as well
// (1 + 2^-17); a tie whose even digit is the upper, a value next to a tie, a power of two (whose
// lower neighbour is nearer) and a large int (which has no tie) are as before.
#[test]
fn a_float_on_a_tie_takes_the_even_digit_as_javascript() {
    let src = "let xs = [1916218016406818.3, 1e15 + 0.3, -1916218016406818.3, 1.0 + 1.0 / 131072, 1.0 + 3.0 / 131072, 1916218016406818.5, 1916218016406818.75, 1152921504606847232.0, 0.1, 1.0 / 3.0, 1.0 / 16777216]\nprint(join(xs | map((x) => str(x)), \" | \"))\nprint(json.stringify(xs))\n";
    let (ok, out) = check(src);
    assert!(ok, "{}", out);
    let d = dir("q777", src);
    let out = run_script(&d, "main.qwe");
    let mut lines = out.lines();
    assert_eq!(
        lines.next(),
        Some("1916218016406818.2 | 1000000000000000.2 | -1916218016406818.2 | 1.0000076293945312 | 1.0000228881835938 | 1916218016406818.5 | 1916218016406818.8 | 1152921504606847200 | 0.1 | 0.3333333333333333 | 0.00000005960464477539063"),
        "{}",
        out
    );
    assert_eq!(
        lines.next(),
        Some("[1916218016406818.2,1000000000000000.2,-1916218016406818.2,1.0000076293945312,1.0000228881835938,1916218016406818.5,1916218016406818.8,1152921504606847200.0,0.1,0.3333333333333333,5.960464477539063e-8]"),
        "{}",
        out
    );
    // A component's text, which the browser writes too.
    let src = "component Tie() {\n    state x = 1916218016406818.3\n    <p #t>{x} {1.0 + 1.0 / 131072} {-1e15 - 0.3} {1e21} {1916218016406818.5}</p>\n}\nroute GET / = <html><head><title>t</title></head><body><Tie/></body></html>\n";
    let (ok, out) = check(src);
    assert!(ok, "{}", out);
    let d = dir("q777-page", src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let html = support::send(s.port, "GET", "/", &[], "").body;
    assert!(html.contains("<p id=\"t\">1916218016406818.2 1.0000076293945312 -1000000000000000.2 1000000000000000000000 1916218016406818.5</p>"), "{}", html);
}

/// Floats where ties are (odd mantissas of a few bits, at each exponent), both signs, and some of
/// random bits, as Rust writes them back exactly.
fn tie_prone_floats() -> Vec<String> {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut xs = Vec::new();
    for e in -90..90i32 {
        for bits in 1..=40u32 {
            let m = (next() >> (64 - bits)) | 1;
            let x = m as f64 * 2f64.powi(e - bits as i32 + 1);
            xs.push(x);
            xs.push(-x);
        }
    }
    for _ in 0..4000 {
        let x = f64::from_bits(next());
        if x.is_finite() && x != 0.0 {
            xs.push(x);
        }
    }
    xs.iter().map(|x| format!("{:e}", x)).collect()
}

// Q-777 and Q-781 against JavaScript itself: each of 18,000 floats, most where ties are, written
// by `json.stringify` as Node's JSON.stringify writes it (a whole one with `.0`), and by `str` as
// Node's String with no exponent.
#[test]
fn floats_are_written_as_node_writes_them() {
    if support::node(";").map(|o| !o.status.success()).unwrap_or(true) {
        eprintln!("skipping: no node");
        return;
    }
    let xs = tie_prone_floats();
    // Written to a file: this much printed would fill the pipe the test reads at the end.
    let src = "let xs = json.parse(read_file(\"xs.json\"))\nwrite_file(\"out.txt\", join(xs | map((s) => json.stringify(float(s))), \"\\n\") + \"\\n\" + join(xs | map((s) => str(float(s))), \"\\n\"))\nprint(\"done\")\n";
    let (ok, out) = check(src);
    assert!(ok, "{}", out);
    let d = dir("q777-node", src);
    std::fs::write(d.join("xs.json"), serde_json::to_string(&xs).unwrap()).unwrap();
    assert_eq!(run_script(&d, "main.qwe").trim(), "done");
    let server = std::fs::read_to_string(d.join("out.txt")).unwrap();
    let js = format!(
        r#"const xs = {};
const plain = (v) => {{
  const s = String(v), at = s.indexOf("e");
  if (at < 0) return s;
  const neg = s[0] === "-", exp = Number(s.slice(at + 1));
  const [whole, part = ""] = s.slice(neg ? 1 : 0, at).split(".");
  const digits = whole + part, point = whole.length + exp;
  return (neg ? "-" : "") + (point <= 0 ? "0." + "0".repeat(-point) + digits : point >= digits.length ? digits + "0".repeat(point - digits.length) : digits.slice(0, point) + "." + digits.slice(point));
}};
const json = xs.map((s) => {{ const t = JSON.stringify(Number(s)); return /[.e]/.test(t) ? t : t + ".0"; }});
const text = xs.map((s) => {{ const x = Number(s); return Number.isInteger(x) && Math.abs(x) < 1e16 ? x.toFixed(1) : plain(x); }});
console.log(json.concat(text).join("\n"));"#,
        serde_json::to_string(&xs).unwrap()
    );
    let o = support::node(&js).expect("node");
    let node = String::from_utf8_lossy(&o.stdout);
    let (got, want): (Vec<&str>, Vec<&str>) = (server.lines().collect(), node.lines().collect());
    assert_eq!(got.len(), 2 * xs.len(), "{}", &server[..server.len().min(400)]);
    let differ: Vec<String> = (0..got.len()).filter(|&i| got[i] != want[i]).take(5).map(|i| format!("{}: {} / {}", xs[i % xs.len()], got[i], want[i])).collect();
    assert!(differ.is_empty(), "the server / Node: {:?}", differ);
}

// Q-781: `json.stringify` turned to an exponent from 1e16 up and below 1e-5, JavaScript from
// 1e21 and below 1e-6. Each boundary and the float either side, both signs, as JavaScript writes
// them; a whole float keeps its `.0` (so it reads back as a float, -0.0 its sign); inf and NaN are
// null; ints and indented text as before; what is written reads back as the same floats.
#[test]
fn json_stringify_turns_to_an_exponent_where_javascript_does() {
    let src = r#"let xs = [1e16, 9999999999999998.0, 1e20, 1e21, 999999999999999900000.0, 1.0000000000000001e21, 1e-6, 1.5e-6, 9.999999999999997e-7, 0.0000010000000000000002, 1e-7, -1e21, -1.5e-6, -1e-7, 2.0, -0.0, 0.0, 1e300, 5e-324, 1.7976931348623157e308, 123e-20, 0.1, 100000.0]
print(json.stringify(xs))
let back = json.parse(json.stringify(xs))
print(back == xs, unique(back | map(type_of)), str(back[15]), str(back[4]))
print(json.stringify([float("inf"), float("-inf"), float("nan"), []]))
print(json.stringify({ a: 1e16, b: [1.5e-6] }, true))
print(json.stringify([10000000000000000, 9223372036854775807, -1, 0]))
"#;
    let (ok, out) = check(src);
    assert!(ok, "{}", out);
    let d = dir("q781", src);
    let out = run_script(&d, "main.qwe");
    let want = "[10000000000000000.0,9999999999999998.0,100000000000000000000.0,1e+21,999999999999999900000.0,1.0000000000000001e+21,0.000001,0.0000015,9.999999999999997e-7,0.0000010000000000000002,1e-7,-1e+21,-0.0000015,-1e-7,2.0,-0.0,0.0,1e+300,5e-324,1.7976931348623157e+308,1.23e-18,0.1,100000.0]
true [\"float\"] -0.0 999999999999999900000
[null,null,null,[]]
{
  \"a\": 10000000000000000.0,
  \"b\": [
    0.0000015
  ]
}
[10000000000000000,9223372036854775807,-1,0]
";
    assert_eq!(out, want);
}

// ---- end of slice 9 ----

// ---- slice 11: files of a program, plugins, native Rust (Q-807 to Q-822) ----

/// A fresh folder with `files` in it (paths with folders made), for slice 11.
fn s11_dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback55-s11-{}", name));
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

/// `quillo args` run in `cwd`, under a deadline a plugin's first build fits in: whether it
/// passed, and what it printed, stdout then stderr.
fn s11_quillo(cwd: &Path, args: &[&str]) -> (bool, String, String) {
    use std::io::Read;
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Read as it runs: a failed build says a lot, and a full pipe would hold it up.
    let (mut o, mut e) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let out = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = o.read_to_string(&mut s);
        s
    });
    let err = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = e.read_to_string(&mut s);
        s
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("quillo {:?} did not end", args);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    (status.success(), out.join().unwrap(), err.join().unwrap())
}

/// `quillo check file` in `cwd`: whether it passed, what it said (its lines, then its summary),
/// and its lines alone.
fn s11_check(cwd: &Path, file: &str) -> (bool, String, String) {
    let (ok, out, err) = s11_quillo(cwd, &["check", file]);
    (ok, err.clone() + &out, err)
}

/// Whether there is a Rust to build plugins with.
fn s11_cargo() -> bool {
    Command::new("cargo").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// The eight hex digits privacy.rs gives a file by its path: FNV-1a.
fn s11_fnv(key: &str) -> String {
    let mut h: u32 = 0x811c9dc5;
    for b in key.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    format!("{:08x}", h)
}

// Q-807: a private function's tag came from its path as seen from the folder the program was
// started in, so copies started from two folders each ran a `schedule` job of one. Now it is
// its path from the program's folder: the same from any folder, the same as before from the
// program's own, and a file outside that folder by its way from there.
#[test]
fn a_private_names_tag_is_the_same_from_any_folder() {
    let root = s11_dir("q807", &[
        ("prögram-ü/lib/t.qwe", "private fn tick() = 1\nfn name_of() = str(tick)\n"),
        ("prögram-ü/lib/a.qwe", "fn helper() = 1\nfn a_of() = str(helper)\n"),
        ("prögram-ü/lib/b.qwe", "fn helper() = 2\nfn b_of() = str(helper)\n"),
        ("shared/s.qwe", "private fn far() = 1\nfn far_of() = str(far)\n"),
        ("prögram-ü/main.qwe", "private fn mine() = 1\nuse lib/t.qwe\nimport lib/a.qwe use a_of\nimport lib/b.qwe use b_of\nuse ../shared/s.qwe\nprint(name_of(), a_of(), b_of(), far_of(), str(mine))\n"),
    ]);
    let prog = root.join("prögram-ü");
    let want = format!(
        "<fn tick__pv{}> <fn helper__pv{}> <fn helper__pv{}> <fn far__pv{}> <fn mine__pv{}>\n",
        s11_fnv("lib/t.qwe"),
        s11_fnv("lib/a.qwe"),
        s11_fnv("lib/b.qwe"),
        s11_fnv("../shared/s.qwe"),
        s11_fnv("main.qwe")
    );
    let main = prog.join("main.qwe").display().to_string();
    // From its folder, its parent, a folder inside it, and one elsewhere.
    for (cwd, file) in [(prog.clone(), "main.qwe".to_string()), (root.clone(), "prögram-ü/main.qwe".to_string()), (prog.join("lib"), "../main.qwe".to_string()), (std::env::temp_dir(), main.clone())] {
        let (ok, out, err) = s11_quillo(&cwd, &["run", &file]);
        assert!(ok, "{}", err);
        assert_eq!(out, want, "from {}", cwd.display());
    }
}

// Q-807: a service run apart (`--split`) set its own folder as the root, so its tags differed
// from those the app's process gives the same file; it is told the app's folder.
#[test]
fn a_split_service_gives_the_tags_the_app_gives() {
    let d = s11_dir("q807-split", &[
        ("app.qwe", "service tags = \"svc/tags.qwe\"\nroute GET / = tags.name_of()\n"),
        ("svc/tags.qwe", "private fn tick() = 1\nserver fn name_of() -> str = str(tick)\n"),
    ]);
    let want = format!("<fn tick__pv{}>", s11_fnv("svc/tags.qwe"));
    for opts in [&[][..], &["--split"][..]] {
        let s = support::serve_with(&d.join("app.qwe"), opts);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let a = support::send(s.port, "GET", "/", &[], "");
            if a.status == 200 {
                assert_eq!(a.body, want, "{:?}", opts);
                break;
            }
            assert!(std::time::Instant::now() < deadline, "{:?}: {} {}", opts, a.status, a.body);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

// Q-808: a browser name taken from two scripts was the last one's in every file, with no word.
// Now it is an error, in `check` and as the program starts, where the sources differ: in two
// files, or two lines of one.
#[test]
fn a_browser_name_from_two_scripts_is_refused() {
    let a = "client /m1.mjs use helper\ncomponent A() { state s = \"\"\n    <p id=\"a\" @mount={(el) => { s = helper() }}>{s}</p> }\n";
    let b = "client /m2.mjs use helper\ncomponent B() { state s = \"\"\n    <p id=\"b\" @mount={(el) => { s = helper() }}>{s}</p> }\n";
    let main = "import a.qwe use A\nimport b.qwe use B\nroute GET /:f = file(f)\nroute GET / = <html><body><A/><B/></body></html>\n";
    let d = s11_dir("q808", &[("a.qwe", a), ("b.qwe", b), ("main.qwe", main)]);
    let (ok, out, err) = s11_check(&d, "main.qwe");
    assert!(!ok, "{}{}", out, err);
    assert!(out.contains("./b.qwe:1: error: `helper` is taken from /m1.mjs at a.qwe:1, and here from /m2.mjs: a page has one `helper`, so take one of them under another name (`use helper as ...`)"), "{}", out);
    // The program does not start with it either.
    std::fs::write(d.join("one.qwe"), "client /m1.mjs use helper\nclient \"/m2.mjs\" use helper\nprint(\"started\")\n").unwrap();
    let (ok, out, err) = s11_quillo(&d, &["run", "one.qwe"]);
    assert!(!ok && !out.contains("started") && (out.clone() + &err).contains("one.qwe:2: error: `helper` is taken from /m1.mjs at one.qwe:1"), "{}{}", out, err);
    let (ok, out, err) = s11_quillo(&d, &["run", "one.qwe", "--no-check"]);
    assert!(!ok && !out.contains("started"), "{}{}", out, err);
    assert!(err.contains("one.qwe:2: `helper` is taken from /m1.mjs already, and here from /m2.mjs"), "{}", err);
    // One script given the name again, in one file or two: as before.
    std::fs::write(d.join("again.qwe"), "client /m1.mjs use helper\nclient /m1.mjs use helper\nprint(\"started\")\n").unwrap();
    let (ok, out, _) = s11_quillo(&d, &["run", "again.qwe"]);
    assert!(ok && out == "started\n", "{}", out);
    std::fs::write(d.join("b.qwe"), b.replace("/m2.mjs", "/m1.mjs")).unwrap();
    let (ok, out, _) = s11_check(&d, "main.qwe");
    assert!(ok && !out.contains("taken from"), "{}", out);
    // Under another name, two scripts each give theirs.
    std::fs::write(d.join("b.qwe"), b.replace("use helper", "use helper as helper2").replace("s = helper()", "s = helper2()")).unwrap();
    let (ok, out, _) = s11_check(&d, "main.qwe");
    assert!(ok && !out.contains("taken from"), "{}", out);
    // One name for two exports of one module is two sources too.
    std::fs::write(d.join("mod.qwe"), "client module /m.mjs use one as pick\nclient module /m.mjs use two as pick\nprint(1)\n").unwrap();
    let (ok, out, _) = s11_check(&d, "mod.qwe");
    assert!(!ok && out.contains("`pick` is taken from /m.mjs (one)"), "{}", out);
}

// Q-822: a file that takes another whole was given that file's browser names, so their use went
// unwarned; a file's own `client` line is what gives one.
#[test]
fn a_browser_name_a_file_does_not_take_is_warned_of() {
    let a = "client /m1.mjs use helper\nfn from_a() = 1\n";
    let d = s11_dir("q822-browser", &[
        ("a.qwe", a),
        ("d.qwe", "use a.qwe\ncomponent D() { state s = \"\"\n    <p id=\"d\" @mount={(el) => { s = helper() }}>{s}</p> }\n"),
        ("main.qwe", "import d.qwe use D\nroute GET / = <html><body><D/></body></html>\n"),
        ("own.qwe", "use a.qwe\nclient /m1.mjs use helper\ncomponent E() { state s = \"\"\n    <p @mount={(el) => { s = helper() + str(from_a()) }}>{s}</p> }\nroute GET / = <html><body><E/></body></html>\n"),
    ]);
    let (ok, out, _) = s11_check(&d, "main.qwe");
    assert!(ok && out.contains("./d.qwe:3: warning: `helper` is a browser library this file does not take: write its line here too"), "{}", out);
    // Taken by its own line: nothing to say; the file's other names still come with `use`.
    let (ok, out, _) = s11_check(&d, "own.qwe");
    assert!(ok && !out.contains("warning"), "{}", out);
}

// Q-809 and Q-810: NaN and the infinities reached Rust as null, and an error value as a map of
// its message. Now a float crosses as it is, both ways; a `serde_json::Value`, which has no
// NaN, is refused it by the argument's name; and an error given to a Rust function is given
// back, untouched, as the function cannot take it.
#[test]
fn floats_cross_to_rust_as_they_are_and_an_error_is_given_back() {
    if !s11_cargo() {
        return;
    }
    let rust = r#"//! effects: none
use std::collections::HashMap;
pub fn seen(x: Option<f64>) -> String { format!("{:?}", x) }
pub fn plain(x: f64) -> String { format!("{:?}", x) }
pub fn small(x: f32) -> String { format!("{:?}", x) }
pub fn many(xs: Vec<f64>) -> String { format!("{:?}", xs) }
pub fn keyed(m: HashMap<String, f64>) -> String { format!("{:?}", m.get("k")) }
pub fn back(x: f64) -> f64 { x }
pub fn backs() -> Vec<f64> { vec![f64::NAN, f64::INFINITY, -f64::INFINITY, 1.5] }
pub fn dump(v: serde_json::Value) -> String { v.to_string() }
pub fn dumps(v: Vec<serde_json::Value>) -> usize { v.len() }
pub fn store(v: serde_json::Value) -> String { format!("stored {}", v) }
pub fn num(n: i64) -> i64 { n }
pub fn two(a: i64, b: i64) -> i64 { a + b }
"#;
    let main = r#"use n.rs
let nan = 0.0 / 0.0
let inf = 1.0 / 0.0
print(seen(nan), seen(inf), seen(-inf), seen(2.5), seen(null))
print(plain(nan), small(-inf), many([nan, inf, 2.5]), many([]), keyed({ k: -inf }))
let b = back(nan)
print(b == b, back(inf), back(-inf), back(0.1), backs())
print(dump([1, 2.5, "ü"]), dump(null))
let bad = json.parse("{oops")
print(store(bad) == bad, num(bad) == bad, two(1, bad) == bad, store([1, bad]) == bad, store({ k: [bad] }) == bad)
print(store(err("mine")) ?? "given back", dumps([1, err("mine")]) ?? "given back")
fn f(v) = { let x = store(v)?
    `went on with ${x}` }
print(f(bad) == bad, f(1))
"#;
    let d = s11_dir("q809", &[("n.rs", rust), ("main.qwe", main)]);
    let (ok, out, err) = s11_check(&d, "main.qwe");
    assert!(ok, "{}{}", out, err);
    let (ok, out, err) = s11_quillo(&d, &["run", "main.qwe"]);
    assert!(ok, "{}", err);
    assert_eq!(
        out,
        "Some(NaN) Some(inf) Some(-inf) Some(2.5) None\n\
         NaN -inf [NaN, inf, 2.5] [] Some(-inf)\n\
         false inf -inf 0.1 [NaN, inf, -inf, 1.5]\n\
         [1,2.5,\"ü\"] null\n\
         true true true true true\n\
         given back given back\n\
         true went on with stored 1\n"
    );
    // A NaN or an infinity, at the top or inside: refused by the argument's name.
    for (arg, said) in [("0.0 / 0.0", "NaN"), ("[1, -1.0 / 0.0]", "-inf"), ("{ a: { b: 1.0 / 0.0 } }", "inf")] {
        std::fs::write(d.join("bad.qwe"), format!("use n.rs\nprint(dump({}))\n", arg)).unwrap();
        let (ok, _, err) = s11_quillo(&d, &["run", "bad.qwe"]);
        assert!(!ok && err.contains(&format!("bad.qwe:2: `dump`: argument `v` holds {}, which a serde_json::Value cannot: give it as text, or take an f64", said)), "{}", err);
    }
    std::fs::write(d.join("bad.qwe"), "use n.rs\nprint(dumps([1.0 / 0.0]))\n").unwrap();
    let (ok, _, err) = s11_quillo(&d, &["run", "bad.qwe"]);
    assert!(!ok && err.contains("`dumps`: argument `v` holds inf"), "{}", err);
    // A float where an int is wanted is said to be one.
    std::fs::write(d.join("bad.qwe"), "use n.rs\nprint(num(0.0 / 0.0))\n").unwrap();
    let (ok, _, err) = s11_quillo(&d, &["run", "bad.qwe", "--no-check"]);
    assert!(!ok && err.contains("`num`: argument `n` must be int, got float"), "{}", err);
}

// Q-811: `r#` before a letter was read as a raw string that never ended, and every `pub fn`
// after it was lost; raw strings are `r"`, `r#"` and `br#"`.
#[test]
fn a_raw_identifier_hides_no_function() {
    use quillo::native::parse;
    let names = |src: &str| parse(src).exports.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
    assert_eq!(names("pub fn kind(r#type: i64) -> i64 { r#type }\npub fn later_one() -> i64 { 5 }\n"), ["kind", "later_one"]);
    assert_eq!(parse("pub fn kind(r#type: i64) -> i64 { r#type }").exports[0].params[0].0, "type");
    // In a body, a raw identifier, then a raw string with a brace and a `pub fn` in it.
    let src = "pub fn a() -> i64 { let r#in = 1; let s = r\"} pub fn not_a() {\"; let t = r#\"}\"# ; let u = br##\"} pub fn nor_b() {\"##; r#in }\npub fn b() -> i64 { 2 }\npub fn r#loop() -> i64 { 3 }\n";
    assert_eq!(names(src), ["a", "b", "loop"]);
    // `r` alone, `r` before a `#` that starts nothing, an identifier ending in `r`.
    assert_eq!(names("pub fn r(r: i64) -> i64 { r }\npub fn c() -> i64 { let ar = 1; let r = ar; r }\npub fn d() {}\n"), ["r", "c", "d"]);
    // A Quillo word, raw in Rust, is not exported, and says so.
    let p = parse("pub fn r#match() -> i64 { 1 }\npub fn e() {}\n");
    assert_eq!(p.exports.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["e"]);
    assert!(p.problems.iter().any(|(_, m)| m.contains("`match` is not exported: `match` is a word of Quillo's")), "{:?}", p.problems);
    if !s11_cargo() {
        return;
    }
    // Built: called by their names, `loop` too.
    let d = s11_dir("q811", &[("k.rs", &format!("//! effects: none\n{}", src.replace("pub fn r#loop()", "pub fn r#loop(r#ref: i64) -> i64 { r#ref }\npub fn old()"))), ("main.qwe", "use k.rs\nprint(a(), b(), loop(4))\n")]);
    let (ok, out, err) = s11_quillo(&d, &["run", "main.qwe"]);
    assert!(ok && out == "1 2 4\n", "{}{}", out, err);
}

// Q-812: `x.rs` and `x.qwer`, or `a-b.rs` and `a_b.rs`, made libraries of one name, and each
// build deleted the other's. Now each has its own library and lock line, which `--no-build`
// loads; a build lets go of only its own file's earlier libraries of its own glue, so those an
// older Quillo built stay for it; and `--no-build` meeting only those says to run `quillo build`.
#[test]
fn two_rust_files_of_one_stem_keep_a_library_each() {
    if !s11_cargo() {
        return;
    }
    let x_rs = "//! effects: none\npub fn from_rs() -> i64 { 1 }\n";
    let d = s11_dir("q812", &[
        ("x.rs", x_rs),
        ("x.qwer", "//! effects: none\npub fn from_qwer() -> i64 { 2 }\n"),
        ("a-b.rs", "//! effects: none\npub fn ab1() -> i64 { 3 }\n"),
        ("a_b.rs", "//! effects: none\npub fn ab2() -> i64 { 4 }\n"),
        ("main.qwe", "use x.rs\nuse x.qwer\nuse a-b.rs\nuse a_b.rs\nprint(from_rs(), from_qwer(), ab1(), ab2())\n"),
    ]);
    let libs = |d: &Path| {
        let mut v: Vec<String> = std::fs::read_dir(d.join(".quillo/native")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    };
    // What the first glue named this content's library, by the stem alone, with its lock line.
    let (pre, ext) = (std::env::consts::DLL_PREFIX, std::env::consts::DLL_EXTENSION);
    let first = {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        for p in [x_rs.as_bytes(), b"1", std::env::consts::ARCH.as_bytes()] {
            h.update(p);
            h.update([0]);
        }
        let hex: String = h.finalize()[..8].iter().map(|b| format!("{:02x}", b)).collect();
        format!("{}x-{}.{}", pre, hex, ext)
    };
    std::fs::create_dir_all(d.join(".quillo/native")).unwrap();
    std::fs::write(d.join(".quillo/native").join(&first), "built by the first glue").unwrap();
    std::fs::write(d.join(".quillo/plugins.lock"), format!("{} 00\n", first)).unwrap();
    std::fs::write(d.join("only_x.qwe"), "use x.rs\nprint(from_rs())\n").unwrap();
    let (ok, _, err) = s11_quillo(&d, &["run", "only_x.qwe", "--no-build"]);
    assert!(!ok && err.contains("./x.rs was built by an older Quillo, whose libraries this one cannot load, and --no-build says not to build it here: run `quillo build` once"), "{}", err);
    // Another file of that stem, which no Quillo built: not built, as before.
    std::fs::write(d.join("only_qwer.qwe"), "use x.qwer\nprint(from_qwer())\n").unwrap();
    let (ok, _, err) = s11_quillo(&d, &["run", "only_qwer.qwe", "--no-build"]);
    assert!(!ok && err.contains("./x.qwer is not built, and --no-build"), "{}", err);
    let (ok, out, err) = s11_quillo(&d, &["build", "main.qwe"]);
    assert!(ok, "{}{}", out, err);
    let built = libs(&d);
    assert_eq!(built.len(), 5, "{:?}", built);
    // The older one stays, with its lock line.
    assert!(built.contains(&first), "{:?}", built);
    let lock = std::fs::read_to_string(d.join(".quillo/plugins.lock")).unwrap();
    assert_eq!(lock.lines().count(), 5, "{}", lock);
    assert!(lock.contains(&format!("{} 00", first)), "{}", lock);
    let (ok, out, err) = s11_quillo(&d, &["run", "main.qwe", "--no-build"]);
    assert!(ok && out == "1 2 3 4\n", "{}{}", out, err);
    // A change to one builds it alone; the other's library stays, and the file's earlier one goes.
    std::fs::write(d.join("x.rs"), "//! effects: none\npub fn from_rs() -> i64 { 10 }\n").unwrap();
    let (ok, out, err) = s11_quillo(&d, &["run", "main.qwe"]);
    assert!(ok && out == "10 2 3 4\n", "{}{}", out, err);
    assert!(err.contains("building ./x.rs") && !err.contains("x.qwer"), "{}", err);
    let now = libs(&d);
    assert_eq!(now.len(), 5, "{:?}", now);
    assert_eq!(now.iter().filter(|l| !built.contains(l)).count(), 1, "{:?} {:?}", built, now);
    assert!(now.contains(&first), "{:?}", now);
    let (ok, out, err) = s11_quillo(&d, &["run", "main.qwe", "--no-build"]);
    assert!(ok && out == "10 2 3 4\n", "{}{}", out, err);
}

// Q-813: Rust `check` took and the glue could not build. A reference inside another type is
// refused, with the type to take; a `#[cfg(...)]` goes on the glue too, and a function left out
// says so when called.
#[test]
fn rust_the_glue_cannot_build_is_refused_and_cfg_is_kept() {
    use quillo::native::parse;
    let p = parse(
        "pub fn a(name: Option<&str>) -> String { String::new() }\npub fn b(xs: Vec<&str>) -> usize { 0 }\npub fn c(xs: Option<&[i64]>) -> usize { 0 }\npub fn d(m: std::collections::HashMap<String, &str>) -> usize { 0 }\npub fn e(s: &'static str) -> usize { 0 }\n\
         pub fn ok1(s: &str, xs: &[i64], v: &Vec<i64>, o: Option<String>) -> usize { 0 }\npub fn ok2() -> Option<&'static str> { None }\npub fn ok3() -> Vec<&'static str> { vec![] }\n\
         pub fn f(xs: &[&str]) -> usize { 0 }\npub fn g(x: Option<&[u8; 4]>) -> usize { 0 }\npub fn h(m: Option<&std::collections::HashMap<String, &str>>) -> usize { 0 }\n",
    );
    assert_eq!(p.exports.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["ok1", "ok2", "ok3"]);
    let said: Vec<String> = p.problems.iter().map(|(l, m)| format!("{} {}", l, m)).collect();
    for want in [
        "1 `a` is not exported: a parameter of type `Option<&str>` cannot be made from a Quillo value: take `Option<String>`",
        "2 `b` is not exported: a parameter of type `Vec<&str>` cannot be made from a Quillo value: take `Vec<String>`",
        "3 `c` is not exported: a parameter of type `Option<&[i64]>` cannot be made from a Quillo value: take `Option<Vec<i64>>`",
        "4 `d` is not exported: a parameter of type `std::collections::HashMap<String, &str>` cannot be made from a Quillo value: take `std::collections::HashMap<String, String>`",
        "5 `e` is not exported: a parameter of type `&'static str` cannot be made from a Quillo value: take `&str` or `String`",
        "9 `f` is not exported: a parameter of type `&[&str]` cannot be made from a Quillo value: take `Vec<String>`",
        "10 `g` is not exported: a parameter of type `Option<&[u8; 4]>` cannot be made from a Quillo value: take `Option<Vec<u8>>`",
        "11 `h` is not exported: a parameter of type `Option<&std::collections::HashMap<String, &str>>` cannot be made from a Quillo value: take `Option<std::collections::HashMap<String, String>>`",
    ] {
        assert!(said.iter().any(|s| s == want), "{} not in {:?}", want, said);
    }
    // Attributes: each `cfg` of a `pub fn`, and none of another item's or a module's.
    let p = parse("#[cfg(unix)]\n#[inline]\n#[cfg(feature = \"x]\")]\n/// Doc.\npub fn one() {}\n#[cfg(test)]\nfn private() {}\npub fn two() {}\n#[cfg(test)]\nmod tests {\n    pub fn inner() {}\n}\npub fn three() {}\n#[cfg(windows)] pub struct S;\npub fn four() {}\n");
    let cfgs: Vec<(String, Vec<String>)> = p.exports.iter().map(|e| (e.name.clone(), e.cfg.clone())).collect();
    assert_eq!(
        cfgs,
        [("one".to_string(), vec!["unix".to_string(), "feature = \"x]\"".to_string()]), ("two".into(), vec![]), ("three".into(), vec![]), ("four".into(), vec![])]
    );
    assert_eq!(p.exports[0].doc, "Doc.");
    if !s11_cargo() {
        return;
    }
    let (here, there) = if cfg!(target_os = "linux") { ("linux", "macos") } else { ("macos", "linux") };
    let rust = format!(
        "//! effects: none\n#[cfg(target_os = \"{there}\")]\npub fn elsewhere() -> i64 {{ 1 }}\n#[cfg(target_os = \"{here}\")]\n#[cfg(unix)]\npub fn here() -> i64 {{ 2 }}\n#[cfg(not(target_os = \"{there}\"))]\npub fn also_here(n: i64) -> i64 {{ n }}\n"
    );
    let d = s11_dir("q813", &[("c.rs", &rust), ("main.qwe", "use c.rs\nprint(here(), also_here(3))\nprint(elsewhere())\n")]);
    let (ok, out, err) = s11_check(&d, "main.qwe");
    assert!(ok, "{}{}", out, err);
    let (ok, out, err) = s11_quillo(&d, &["run", "main.qwe"]);
    assert!(!ok && out == "2 3\n", "{}{}", out, err);
    assert!(err.contains(&format!("main.qwe:3: `elsewhere`: it is not in this build, as its #[cfg(target_os = \"{}\")] leaves it out", there)), "{}", err);
}

// Q-814: a `//` line, a block comment or `#![...]` before the `//!` header made Quillo miss it;
// the message about effects named a `.qwer` file for a `.rs` one.
#[test]
fn the_header_is_read_after_comments_and_attributes() {
    use quillo::native::parse;
    for top in [
        "// SPDX-License-Identifier: MIT\n",
        "\n\n",
        "#![allow(clippy::all)]\n",
        "#![allow(\n    clippy::all,\n    dead_code\n)]\n",
        "/* A licence,\n   on two lines. */\n",
        "/* one line */ \n// and a comment\n\n#![deny(unsafe_code)]\n",
        "",
    ] {
        let p = parse(&format!("{}//! effects: none\n//! [dependencies]\n//! itoa = \"1\"\npub fn twice(n: i64) -> i64 {{ n * 2 }}\n", top));
        assert_eq!((p.effects, p.deps.trim()), (Some(0), "itoa = \"1\""), "{:?}", top);
        assert_eq!(p.exports[0].name, "twice");
    }
    // What is an item's, or after one, is no header.
    assert_eq!(parse("/// The doc of twice.\n//! effects: none\npub fn twice() {}\n").effects, None);
    assert_eq!(parse("use std::fmt;\n//! effects: none\npub fn twice() {}\n").effects, None);
    assert_eq!(parse("/** doc */\n//! effects: none\n").effects, None);
    if !s11_cargo() {
        return;
    }
    let d = s11_dir("q814", &[
        ("t.rs", "// SPDX-License-Identifier: MIT\n#![allow(dead_code)]\n//! effects: none\n//! [dependencies]\n//! itoa = \"1\"\npub fn twice(n: i64) -> String { itoa::Buffer::new().format(n * 2).to_string() }\n"),
        ("u.rs", "// SPDX-License-Identifier: MIT\npub fn thrice(n: i64) -> i64 { n * 3 }\n"),
        ("v.qwer", "pub fn four(n: i64) -> i64 { n * 4 }\n"),
        ("main.qwe", "use t.rs\nfn calc(n: int) with time = twice(n)\nprint(calc(21))\n"),
        ("bad.qwe", "use u.rs\nuse v.qwer\nfn calc3(n: int) with time = thrice(n)\nfn calc4(n: int) with time = four(n)\n"),
    ]);
    let (ok, out, err) = s11_quillo(&d, &["run", "main.qwe"]);
    assert!(ok && out == "42\n", "{}{}", out, err);
    let (ok, out, _) = s11_check(&d, "bad.qwe");
    assert!(!ok, "{}", out);
    assert!(out.contains("bad.qwe:3: error: `thrice` is Rust whose file does not say what it does") && out.contains("(or `//! effects: none`) at the top of u.rs\n"), "{}", out);
    assert!(out.contains("bad.qwe:4: error: `four` is Rust") && out.contains("at the top of v.qwer\n"), "{}", out);
}

// Q-815: `std::` paths, `-> &'static str` and comments in a signature were refused.
#[test]
fn common_rust_signatures_are_read() {
    use quillo::native::{parse, quillo_type};
    for (rust, q) in [
        ("std::io::Result<String>", "str | error(str)"),
        ("std::vec::Vec<i64>", "list"),
        ("alloc::vec::Vec<std::string::String>", "list"),
        ("std::option::Option<i64>", "int | null"),
        ("core::option::Option<std::option::Option<f64>>", "float | null"),
        ("std::result::Result<i64, String>", "int | error(str)"),
        ("std::collections::HashMap<std::string::String, i64>", "map"),
        ("&'static str", "str"),
        ("Option<&'static str>", "str | null"),
        ("Vec<&'static str>", "list"),
    ] {
        assert_eq!(quillo_type(rust).as_deref(), Ok(q), "{}", rust);
    }
    // Paths of types Quillo has not are still refused, by what was written.
    assert!(quillo_type("std::rc::Rc<i64>").unwrap_err().contains("`Rc<i64>` cannot cross"));
    let p = parse("pub fn price(\n    amount: i64, // in cents; {not a body}\n    rate: f64, /* a share, (of it) */\n) -> f64 { 0.0 }\npub fn add(a: i64 /* left */, b: i64 /* right */) -> i64 { a + b }\npub fn after() -> &'static str { \"x\" }\n");
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    let e: Vec<(String, Vec<(String, String)>)> = p.exports.iter().map(|e| (e.name.clone(), e.params.clone())).collect();
    assert_eq!(e[0].1, [("amount".to_string(), "i64".to_string()), ("rate".to_string(), "f64".to_string())]);
    assert_eq!(e[1].1, [("a".to_string(), "i64".to_string()), ("b".to_string(), "i64".to_string())]);
    assert_eq!((e[2].0.as_str(), p.exports[2].ret.as_deref()), ("after", Some("&'static str")));
    // The line numbers after a signature on several lines are still right.
    assert_eq!(p.exports.iter().map(|e| e.line).collect::<Vec<_>>(), [1, 5, 6]);
    if !s11_cargo() {
        return;
    }
    let rust = "//! effects: none\npub fn read_it(p: &str) -> std::io::Result<String> { if p.is_empty() { Err(std::io::Error::other(\"empty\")) } else { Ok(p.to_string()) } }\npub fn evens(n: i64) -> std::vec::Vec<i64> { (0..n).filter(|x| x % 2 == 0).collect() }\npub fn maybe(n: std::option::Option<i64>) -> std::option::Option<i64> { n.map(|x| x + 1) }\npub fn name() -> &'static str { \"static\" }\npub fn names() -> Vec<&'static str> { vec![\"a\", \"ü\"] }\npub fn price(\n    amount: i64, // in cents\n    rate: f64, /* a share */\n) -> f64 { amount as f64 * rate }\n";
    let d = s11_dir("q815", &[("s.rs", rust), ("main.qwe", "use s.rs\nprint(read_it(\"x\"), read_it(\"\") ?? \"none\", evens(5), maybe(1), maybe(null), name(), names(), price(200, 0.5))\n")]);
    let (ok, out, err) = s11_check(&d, "main.qwe");
    assert!(ok && !out.contains("warning"), "{}{}", out, err);
    let (ok, out, err) = s11_quillo(&d, &["run", "main.qwe"]);
    assert!(ok && out == "x none [0, 2, 4] 2 null static [\"a\", \"ü\"] 100.0\n", "{}{}", out, err);
}

// Q-816: a Rust alias was the program's, so a used file's `import b.rs use who as name` changed
// `name` for every file; and the fix the warning gave did not parse. Now a file's names from
// Rust are its own, and the fix names the export under a name that parses.
#[test]
fn a_rust_alias_is_the_files_own() {
    if !s11_cargo() {
        return;
    }
    let d = s11_dir("q816", &[
        ("a.rs", "//! effects: none\npub fn who() -> String { \"a\".into() }\n"),
        ("b.rs", "//! effects: none\npub fn who() -> String { \"b\".into() }\n"),
        ("imei.rs", "//! effects: none\npub fn imei_valid(s: &str) -> bool { s.len() == 15 }\n"),
        ("other.qwe", "import b.rs use who as name\nfn from_other() = name()\n"),
        ("same.qwe", "import a.rs use who as name\nfn from_same() = name()\n"),
        ("whole.qwe", "use b.rs\nfn from_whole() = who()\n"),
        ("main.qwe", "import a.rs use who as name\nuse other.qwe\nuse same.qwe\nuse whole.qwe\nprint(name(), from_other(), from_same(), from_whole())\n"),
        // A library's own function of a plugin's name, which it never takes, and the other way.
        ("lib/v.qwe", "fn imei_valid(s) = \"lib\"\nfn lib_check(s) = imei_valid(s)\n"),
        ("lib/w.qwe", "import imei.rs use imei_valid\nfn lib2(s) = imei_valid(s)\n"),
        ("libs.qwe", "import imei.rs use imei_valid\nimport lib/v.qwe use lib_check\nprint(imei_valid(\"123456789012345\"), lib_check(\"x\"))\n"),
        ("libs2.qwe", "fn imei_valid(s) = \"main\"\nimport lib/w.qwe use lib2\nprint(imei_valid(\"x\"), lib2(\"123456789012345\"))\n"),
    ]);
    for (file, want) in [("main.qwe", "a b a b\n"), ("libs.qwe", "true lib\n"), ("libs2.qwe", "main true\n")] {
        let (ok, out, err) = s11_check(&d, file);
        assert!(ok && !out.contains("warning"), "{}: {}{}", file, out, err);
        let (ok, out, err) = s11_quillo(&d, &["run", file]);
        assert!(ok && out == want, "{}: {}{}", file, out, err);
    }
    // Two plugins' one name in one file: the warning's fix parses, and works.
    std::fs::write(d.join("both.qwe"), "use a.rs\nuse b.rs\nprint(who())\n").unwrap();
    let (_, out, _) = s11_check(&d, "both.qwe");
    let fix = out.split("take one under another name: ").nth(1).and_then(|r| r.lines().next()).unwrap_or_else(|| panic!("{}", out)).to_string();
    assert_eq!(fix, "import b.rs use who as b_who");
    std::fs::write(d.join("both.qwe"), format!("use a.rs\n{}\nprint(who(), b_who())\n", fix)).unwrap();
    let (ok, out, err) = s11_quillo(&d, &["run", "both.qwe"]);
    assert!(ok && out == "a b\n", "{}{}", out, err);
    // A builtin's name from a file whose name has a `-`: the fix parses too.
    std::fs::write(d.join("lib/my-tools.rs"), "//! effects: none\npub fn len(s: &str) -> i64 { 7 }\n").unwrap();
    std::fs::write(d.join("tools.qwe"), "use lib/my-tools.rs\nprint(len(\"x\"))\n").unwrap();
    let (_, out, _) = s11_check(&d, "tools.qwe");
    let fix = out.split("take it under another name: ").nth(1).and_then(|r| r.lines().next()).unwrap_or_else(|| panic!("{}", out)).to_string();
    assert_eq!(fix, "import lib/my-tools.rs use len as my_tools_len");
    std::fs::write(d.join("tools.qwe"), format!("{}\nprint(my_tools_len(\"x\"), len(\"xy\"))\n", fix)).unwrap();
    let (ok, out, err) = s11_quillo(&d, &["run", "tools.qwe"]);
    assert!(ok && out == "7 2\n", "{}{}", out, err);
    // One function of one file, taken by two files, is not two: a file that takes neither is
    // told to take it from the Rust file, as before, and it runs.
    std::fs::write(d.join("x.qwe"), "import a.rs use who\nfn fx() = who()\n").unwrap();
    std::fs::write(d.join("y.qwe"), "use a.rs\nfn fy() = who()\n").unwrap();
    std::fs::write(d.join("xy.qwe"), "use x.qwe\nuse y.qwe\nprint(who(), fx(), fy())\n").unwrap();
    let (ok, out, _) = s11_check(&d, "xy.qwe");
    assert!(ok && out.contains("xy.qwe:3: warning: `who` comes from a.rs, which this file does not take: add `import a.rs use who`"), "{}", out);
    let (ok, out, err) = s11_quillo(&d, &["run", "xy.qwe"]);
    assert!(ok && out == "a a a\n", "{}{}", out, err);
    // Two functions of that name: told to take one, from the Rust file.
    std::fs::write(d.join("z.qwe"), "import b.rs use who\nfn fz() = who()\n").unwrap();
    std::fs::write(d.join("xz.qwe"), "use x.qwe\nuse z.qwe\nprint(who())\n").unwrap();
    let (ok, out, _) = s11_check(&d, "xz.qwe");
    assert!(!ok && out.contains("xz.qwe:3: error: `who` comes from a.rs and b.rs: take the one you mean: import a.rs use who"), "{}", out);
    std::fs::write(d.join("xz.qwe"), "use x.qwe\nuse z.qwe\nimport a.rs use who\nprint(who(), fx(), fz())\n").unwrap();
    let (ok, out, err) = s11_quillo(&d, &["run", "xz.qwe"]);
    assert!(ok && out == "a a b\n", "{}{}", out, err);
    // A name taken by name wins over the same name of a Rust file taken whole, in either order.
    std::fs::write(d.join("helper.rs"), "//! effects: none\npub fn helper() -> String { \"rust\".into() }\npub fn other() -> i64 { 9 }\n").unwrap();
    std::fs::write(d.join("lib/h.qwe"), "fn helper() = \"lib\"\n").unwrap();
    for (f, src) in [("named1.qwe", "import lib/h.qwe use helper\nuse helper.rs\nprint(helper(), other())\n"), ("named2.qwe", "use helper.rs\nimport lib/h.qwe use helper\nprint(helper(), other())\n")] {
        std::fs::write(d.join(f), src).unwrap();
        let (ok, out, _) = s11_check(&d, f);
        assert!(ok && !out.contains("warning"), "{}: {}", f, out);
        let (ok, out, err) = s11_quillo(&d, &["run", f]);
        assert!(ok && out == "lib 9\n", "{}: {}{}", f, out, err);
    }
    // A Rust file's function taken by name wins over another's of that name taken whole, and
    // one file taken both ways is that file.
    for (f, src, want) in [
        ("forms1.qwe", "import a.rs use who\nuse b.rs\nprint(who())\n", "a\n"),
        ("forms2.qwe", "use b.rs\nimport a.rs use who\nprint(who())\n", "a\n"),
        ("forms3.qwe", "import a.rs use who\nuse a.rs\nprint(who())\n", "a\n"),
    ] {
        std::fs::write(d.join(f), src).unwrap();
        let (ok, out, _) = s11_check(&d, f);
        assert!(ok, "{}: {}", f, out);
        let (ok, out, err) = s11_quillo(&d, &["run", f]);
        assert!(ok && out == want, "{}: {}{}", f, out, err);
    }
    // Both by name: said, as two names taken.
    std::fs::write(d.join("named3.qwe"), "import lib/h.qwe use helper\nimport helper.rs use helper\nprint(helper())\n").unwrap();
    let (ok, out, _) = s11_check(&d, "named3.qwe");
    assert!(!ok && out.contains("`helper` is defined in this file, and taken from lib/h.qwe"), "{}", out);
    assert_eq!(quillo::native::other_name("9lives.qwer", "f"), "_9lives_f");
    assert_eq!(quillo::native::other_name("lib/ünï code.RS", "f"), "ünï_code_f");
}

// Q-817: a main file run through a link looked for its used files beside the link. Now its
// `use`, `import` and `mount` paths count from the folder of the file the link leads to, and
// everything else (the policy, `.env`, data, messages) from the folder given, as before; a folder
// that is a link is the folder given.
#[test]
fn a_main_file_through_a_link_uses_the_files_beside_it() {
    let d = s11_dir("q817", &[
        ("opt/tool/tool.qwe", "use lib/a.qwe\nuse ./lib/b.qwe\nimport lib/c use c\nprint(a(), b(), c())\n"),
        ("opt/tool/lib/a.qwe", "fn a() = 1\n"),
        ("opt/tool/lib/b.qwe", "fn b() = 2\n"),
        ("opt/tool/lib/c.qwe", "fn c() = 3\n"),
        ("opt/tool/app.qwe", "use lib/a.qwe\nprint(a(), env(\"WHO\"), read_file(\"data.txt\"))\ndb.open(\"data.db\")\ndb.exec(\"create table if not exists t (x int)\")\ndb.exec(\"insert into t values (1)\")\nprint(db.query(\"select count(*) as n from t\")[0].n)\n"),
        ("opt/tool/sub/m.qwe", "route GET / = \"m\"\n"),
        ("opt/tool/mounts.qwe", "mount /m = \"sub/m.qwe\"\nroute GET / = \"top\"\n"),
        ("opt/tool/.env", "WHO=real\n"),
        ("opt/tool/data.txt", "real data"),
        ("opt/tool/policed.qwe", "print(env(\"HOME\") ?? \"none\")\n"),
        ("opt/tool/broken.qwe", "use lib/a.qwe\nprint(a(), missing_name)\n"),
        ("site/.env", "WHO=site\n"),
        ("site/data.txt", "site data"),
        ("guarded/quillo.policy", "[effects]\nallow = [\"db\"]\n"),
        ("releases/r1/main.qwe", "use lib/a.qwe\nprint(a(), env(\"WHO\"))\n"),
        ("releases/r1/lib/a.qwe", "fn a() = 7\n"),
        ("releases/r1/.env", "WHO=r1\n"),
    ]);
    let link = |to: &str, at: &str| std::os::unix::fs::symlink(to, d.join(at)).unwrap();
    std::fs::create_dir_all(d.join("bin")).unwrap();
    link("../opt/tool/tool.qwe", "bin/tool.qwe");
    link("tool.qwe", "bin/again.qwe");
    link("../opt/tool/app.qwe", "site/main.qwe");
    link("../opt/tool/mounts.qwe", "site/mounts.qwe");
    link("../opt/tool/broken.qwe", "site/broken.qwe");
    link("../opt/tool/policed.qwe", "guarded/main.qwe");
    link("releases/r1", "current");
    // `use` beside the file the link leads to: through a link, a link to a link, and from the
    // link's own folder.
    for (cwd, args) in [(d.clone(), vec!["run", "bin/tool.qwe"]), (d.join("bin"), vec!["tool.qwe"]), (d.clone(), vec!["run", "bin/again.qwe"])] {
        let (ok, out, err) = s11_quillo(&cwd, &args);
        assert!(ok && out == "1 2 3\n", "{:?}: {}{}", args, out, err);
    }
    let (ok, out, _) = s11_check(&d, "bin/tool.qwe");
    assert!(ok && out.starts_with("bin/tool.qwe: ok (0 routes, 3 functions"), "{}", out);
    // `mount` from the file's folder.
    let s = support::serve(&d.join("site/mounts.qwe"), &[]);
    assert_eq!(support::send(s.port, "GET", "/m/", &[], "").body, "m");
    // The data, `.env` and files beside the link, where the program was put.
    let (ok, out, err) = s11_quillo(&d, &["run", "site/main.qwe"]);
    assert!(ok && out == "1 site site data\n1\n", "{}{}", out, err);
    assert!(d.join("site/data.db").is_file() && !d.join("opt/tool/data.db").exists());
    // The policy beside the link is the program's.
    let (ok, out, err) = s11_quillo(&d, &["run", "guarded/main.qwe"]);
    assert!(!ok && !out.contains('/') && err.contains("guarded/main.qwe:1: error: `env` needs the env effect, which guarded/quillo.policy does not allow"), "{}{}", out, err);
    // Messages name the file as it was given; a used file by where it is.
    let (ok, out, _) = s11_check(&d, "site/broken.qwe");
    assert!(!ok && out.starts_with("site/broken.qwe:2: error: unknown name `missing_name`"), "{}", out);
    // A folder that is a link is the folder given.
    let (ok, out, err) = s11_quillo(&d, &["run", "current/main.qwe"]);
    assert!(ok && out == "7 r1\n", "{}{}", out, err);
    let (ok, out, err) = s11_quillo(&d.join("current"), &["run", "main.qwe"]);
    assert!(ok && out == "7 r1\n", "{}{}", out, err);
    // Where the paths count from.
    assert_eq!(quillo::modules::source_dir(&d.join("bin/tool.qwe")), d.join("opt/tool"));
    assert_eq!(quillo::modules::source_dir(Path::new("main.qwe")), Path::new("."));
    assert_eq!(quillo::modules::source_dir(&d.join("current/main.qwe")), d.join("current"));
}

// Q-818: a `fn` after a top-level `return` in a used file passed `check`, and the program did
// not start. `check` now does as the program does: nothing after it is defined, and its first
// line says it never runs.
#[test]
fn code_after_a_top_level_return_is_said_never_to_run() {
    let d = s11_dir("q818", &[
        ("lib.qwe", "fn early() = 1\nreturn\nfn late() = 2\nroute GET /late = \"late\"\n"),
        ("main.qwe", "use lib.qwe\nprint(early(), late())\n"),
        ("early.qwe", "use lib.qwe\nprint(early())\n"),
        ("end.qwe", "fn early2() = 1\nreturn\n"),
        ("cond.qwe", "fn one_q818() = 1\nif env(\"NOT_SET_Q818\") == \"x\" { return }\nfn two_q818() = 2\n"),
        ("uses.qwe", "use end.qwe\nuse cond.qwe\nprint(early2(), one_q818(), two_q818())\n"),
        ("top.qwe", "print(\"ran\")\nreturn\nfn never() = 1\nprint(never())\n"),
    ]);
    let (ok, out, _) = s11_check(&d, "main.qwe");
    assert!(!ok, "{}", out);
    assert!(out.contains("main.qwe:2: error: unknown name `late`"), "{}", out);
    assert!(out.contains("./lib.qwe:3: warning: this never runs: the `return` at line 2 leaves the file first, so nothing below it is defined"), "{}", out);
    assert_eq!(out.matches("never runs").count(), 1, "{}", out);
    let (ok, _, err) = s11_quillo(&d, &["run", "main.qwe", "--no-check"]);
    assert!(!ok && err.contains("main.qwe:2: unknown name `late`"), "{}", err);
    // What comes before it is there, and a `return` at the end, or in an `if`, says nothing.
    let (ok, out, err) = s11_quillo(&d, &["run", "early.qwe"]);
    assert!(ok && out == "1\n", "{}{}", out, err);
    let (ok, out, _) = s11_check(&d, "uses.qwe");
    assert!(ok && !out.contains("warning"), "{}", out);
    let (ok, out, _) = s11_quillo(&d, &["run", "uses.qwe"]);
    assert!(ok && out == "1 1 2\n", "{}", out);
    // In the main file too.
    let (ok, out, _) = s11_check(&d, "top.qwe");
    assert!(ok && out.contains("top.qwe:3: warning: this never runs: the `return` at line 2"), "{}", out);
    let (ok, out, _) = s11_quillo(&d, &["run", "top.qwe"]);
    assert!(ok && out == "ran\n", "{}", out);
}

// Q-819: Q-523's one message at the import line was only for `import x.qwe use name`. Now a file
// taken whole that is not there, and a Rust one, are said once, with a near file of any kind; a
// path written without its extension is the file there, and one written with it is that file.
#[test]
fn a_missing_file_is_said_once_for_every_form() {
    let d = s11_dir("q819", &[
        ("lib/bx.qwe", "fn bb() = 2\n"),
        ("imaging.rs", "//! effects: none\npub fn img() -> i64 { 3 }\n"),
        ("helpers.qwe", "fn h() = 1\n"),
        ("lib/m.qwe", "fn price() = 4\n"),
        ("whole.qwe", "use lib/b.qwe\nprint(bb(), b())\nfn f() = mistyped_but_not_said()\n"),
        ("rs.qwe", "import imagin.rs use img\nprint(img())\n"),
        ("rs_whole.qwe", "use imagin.rs\nprint(img())\n"),
        ("explicit.qwe", "use helpers.rs\nprint(h())\n"),
        ("explicit2.qwe", "import helpers.qwer use h\nprint(h())\n"),
        ("implied.qwe", "import imaging use img\nimport lib/m use price\nuse helpers\nprint(img(), price(), h())\n"),
        ("other.qwe", "use lib/bx.qwe\nprint(nope)\n"),
        ("two/helpers.qwe", "fn h() = 1\n"),
        ("two/helpers.rs", "//! effects: none\npub fn h() -> i64 { 2 }\n"),
        ("two/m.qwer", "//! effects: none\npub fn m() -> i64 { 2 }\n"),
        ("two/m.qwe", "fn m() = 1\n"),
        ("two/whole.qwe", "use helpers\nprint(h())\n"),
        ("two/named.qwe", "import m use m\nprint(m())\n"),
        ("two/written.qwe", "use helpers.qwe\nimport m.qwe use m\nprint(h(), m())\n"),
    ]);
    let check = |f: &str| s11_check(&d, f);
    let (ok, out, _) = check("whole.qwe");
    assert!(!ok && out == "whole.qwe:1: error: cannot read lib/b.qwe: No such file or directory (os error 2) (did you mean lib/bx.qwe?)\nquillo: 1 error\n", "{}", out);
    for (f, want) in [
        ("rs.qwe", "rs.qwe:1: error: cannot read imagin.rs: No such file or directory (os error 2) (did you mean imaging.rs?)\n"),
        ("rs_whole.qwe", "rs_whole.qwe:1: error: cannot read imagin.rs: No such file or directory (os error 2) (did you mean imaging.rs?)\n"),
        ("explicit.qwe", "explicit.qwe:1: error: cannot read helpers.rs: No such file or directory (os error 2) (did you mean helpers.qwe?)\n"),
        ("explicit2.qwe", "explicit2.qwe:1: error: cannot read helpers.qwer: No such file or directory (os error 2) (did you mean helpers.qwe?)\n"),
    ] {
        let (ok, out, _) = check(f);
        assert!(!ok && out == format!("{}quillo: 1 error\n", want), "{}: {}", f, out);
    }
    // A file there: its names, and a name no file has, as before.
    let (ok, out, _) = check("other.qwe");
    assert!(!ok && out.contains("other.qwe:2: error: unknown name `nope`"), "{}", out);
    let (ok, out, _) = check("implied.qwe");
    assert!(ok, "{}", out);
    // Two files a path without its extension could be: said, at check and as it runs; written
    // with its extension, the one meant.
    let two = d.join("two");
    for (f, want) in [("whole.qwe", "whole.qwe:1: error: `helpers` could be helpers.qwe or helpers.rs: write the one you mean with its extension\n"), ("named.qwe", "named.qwe:1: error: `m` could be m.qwe or m.qwer: write the one you mean with its extension\n")] {
        let (ok, out, _) = s11_check(&two, f);
        assert!(!ok && out == format!("{}quillo: 1 error\n", want), "{}: {}", f, out);
        let (ok, _, err) = s11_quillo(&two, &["run", f, "--no-check"]);
        assert!(!ok && err.contains(&format!("{}:1: `", f)) && err.contains("write the one you mean with its extension"), "{}: {}", f, err);
    }
    let (ok, out, err) = s11_quillo(&two, &["run", "written.qwe"]);
    assert!(ok && out == "1 1\n", "{}{}", out, err);
    if !s11_cargo() {
        return;
    }
    let (ok, out, err) = s11_quillo(&d, &["run", "implied.qwe"]);
    assert!(ok && out == "3 4 1\n", "{}{}", out, err);
    let (ok, _, err) = s11_quillo(&d, &["run", "explicit.qwe"]);
    assert!(!ok && err.contains("helpers.rs"), "{}", err);
}

// Q-820: `use Lib.QWE` became `Lib.QWE.qwe`, and `use "Tools.RS"` read the Rust as Quillo.
#[test]
fn extensions_are_read_in_any_case() {
    use quillo::native::is_rust;
    for (p, rust) in [("a.rs", true), ("a.RS", true), ("a.Rs", true), ("a.QWER", true), ("a.qwer", true), ("a.rsx", false), ("rs", false), ("", false), ("a.qwe", false), ("ä.RS", true)] {
        assert_eq!(is_rust(p), rust, "{}", p);
    }
    let d = s11_dir("q820", &[
        ("Lib.QWE", "fn lib_one() = 1\n"),
        ("Tools.RS", "//! effects: none\npub fn tool_two() -> i64 { 2 }\n"),
        ("Other.Qwer", "//! effects: none\npub fn other_three() -> i64 { 3 }\n"),
        ("main.qwe", "use Lib.QWE\nuse \"Tools.RS\"\nimport Other.Qwer use other_three\nprint(lib_one(), tool_two(), other_three())\n"),
    ]);
    let (ok, out, err) = s11_check(&d, "main.qwe");
    assert!(ok && out.contains("ok (0 routes, 3 functions"), "{}{}", out, err);
    if !s11_cargo() {
        return;
    }
    let (ok, out, err) = s11_quillo(&d, &["run", "main.qwe"]);
    assert!(ok && out == "1 2 3\n", "{}{}", out, err);
    // `quillo build` knows them for Rust too.
    let (ok, out, err) = s11_quillo(&d, &["build", "main.qwe"]);
    assert!(ok && out.matches("built ").count() == 2, "{}{}", out, err);
}

// Q-821: `use Point as Pt` made `Pt(...)` a Point but not a type; `use one as Point` replaced the
// program's type's constructor in that file.
#[test]
fn a_types_alias_is_a_type_and_as_takes_no_types_name() {
    let d = s11_dir("q821", &[
        ("lib.qwe", "type Point = { x: int }\nfn one() = 1\n"),
        ("main.qwe", "import lib.qwe use Point as Pt\nfn norm(p: Pt) -> Pt = p\nfn kind(v) = match v { Pt as p => \"point\", _ => \"other\" }\nfn count_all(ps: list<Pt>) = len(ps)\ntype Pair = { a: Pt, b: Pt }\nprint(norm(Pt({ x: 3 })), kind(Pt({ x: 1 })), kind(3), count_all([Pt({ x: 1 })]), Pair({ a: Pt({ x: 1 }), b: Pt({ x: 2 }) }).b.x)\n"),
        ("wrong.qwe", "import lib.qwe use Point as Pt\nfn norm(p: Pt) = p\nprint(norm(5))\n"),
        ("shadow.qwe", "import lib.qwe use one as Point\nprint(Point({ x: 1 }))\n"),
        ("short.qwe", "import lib.qwe use Point as P\nprint(P({ x: 3 }))\n"),
        ("plain.qwe", "import lib.qwe use Point, one as uno\nimport lib.qwe use Point as Point\nfn norm(p: Point) = p\nprint(norm(Point({ x: 3 })), uno())\n"),
    ]);
    let (ok, out, _) = s11_check(&d, "main.qwe");
    assert!(ok && !out.contains("warning"), "{}", out);
    let (ok, out, err) = s11_quillo(&d, &["run", "main.qwe"]);
    assert!(ok && out == "{ x: 3 } point other 1 2\n", "{}{}", out, err);
    let (ok, out, _) = s11_check(&d, "wrong.qwe");
    assert!(!ok && out.contains("wrong.qwe:3: error: argument `p` of `norm` must be map, got int"), "{}", out);
    let (ok, out, _) = s11_check(&d, "shadow.qwe");
    assert!(!ok && out.contains("shadow.qwe:1: error: `Point` is a type of the program: take `one` from lib.qwe under another name"), "{}", out);
    let (ok, _, err) = s11_quillo(&d, &["run", "shadow.qwe", "--no-check"]);
    assert!(!ok && err.contains("shadow.qwe:1: `Point` is a type of the program"), "{}", err);
    // A name no type can have still makes one, and is told the name it could take.
    let (ok, out, _) = s11_check(&d, "short.qwe");
    assert!(ok && out.contains("short.qwe:1: warning: `P` is `Point` here, but no type is written `P`") && out.contains("take it as `Pt`"), "{}", out);
    let (ok, out, _) = s11_quillo(&d, &["run", "short.qwe"]);
    assert!(ok && out == "{ x: 3 }\n", "{}", out);
    // What an editor is told: the names as written, none the annotations' renaming makes.
    let a = quillo::check::check_file(&d.join("main.qwe"));
    let names: Vec<&str> = a.references.iter().map(|r| r.name.as_str()).chain(a.symbols.iter().map(|s| s.name.as_str())).chain(a.taken.iter().map(|(_, s)| s.name.as_str())).collect();
    assert!(names.contains(&"Pt") && !names.iter().any(|n| n.starts_with("type ")), "{:?}", names);
    let (ok, out, _) = s11_check(&d, "plain.qwe");
    assert!(ok, "{}", out);
    let (ok, out, _) = s11_quillo(&d, &["run", "plain.qwe"]);
    assert!(ok && out == "{ x: 3 } 1\n", "{}", out);
}

// Q-822: the smaller messages.
#[test]
fn smaller_messages_of_plugins_and_imports_are_plain() {
    // `pub fn view()` was exported and could be called by no name.
    let p = quillo::native::parse("pub fn view() -> i64 { 1 }\npub fn views() -> i64 { 2 }\npub fn component() {}\npub fn reply() {}\npub fn viewing() {}\n");
    assert_eq!(p.exports.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["views", "viewing"]);
    assert!(p.problems.iter().any(|(l, m)| *l == 1 && m == "`view` is not exported: `view` is a word of Quillo's, which no call can name, so rename it"), "{:?}", p.problems);
    assert_eq!(p.problems.len(), 3, "{:?}", p.problems);
    // A chain of files: 64 used, then 65, which is said to be a chain, not a cycle.
    let chain = |n: usize| {
        let mut files: Vec<(String, String)> = (1..n).map(|i| (format!("u{}.qwe", i), format!("use u{}.qwe\n", i + 1))).collect();
        files.push((format!("u{}.qwe", n), "fn last() = 1\n".to_string()));
        files.push(("main.qwe".into(), "use u1.qwe\nprint(last())\n".into()));
        files
    };
    for (n, passes) in [(64, true), (65, false)] {
        let files = chain(n);
        let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let d = s11_dir(&format!("q822-chain-{}", n), &refs);
        let (ok, out, _) = s11_check(&d, "main.qwe");
        assert_eq!(ok, passes, "{}: {}", n, out);
        if !passes {
            assert!(out.contains("./u64.qwe:1: error: this `use` is the end of a chain of files more than 64 deep, each used by the one before"), "{}", out);
            assert!(!out.contains("each other"), "{}", out);
        }
    }
    // A used file's repeated `use` is said at the path the program writes.
    let d = s11_dir("q822-paths", &[
        ("lib/a.qwe", "use lib/b.qwe\nuse lib/b.qwe\nfn a() = b()\n"),
        ("lib/b.qwe", "fn b() = 1\n"),
        ("main.qwe", "use lib/a.qwe\nprint(a())\n"),
        ("js1.qwe", "client \"javascript:alert(1)\" use L\n"),
        ("js2.qwe", "client \"data:text/javascript,1\" use L\n"),
        ("js3.qwe", "client \"x\\\".js\" use L\nprint(1)\n"),
        ("js4.qwe", "client \"x\\\".js\" L\n"),
        ("js5.qwe", "client lib/a.qwe use a\n"),
    ]);
    let (ok, out, _) = s11_check(&d, "main.qwe");
    assert!(ok && out.starts_with("./lib/a.qwe:2: warning: lib/b.qwe is taken a second time in this file: once is enough\n"), "{}", out);
    for (f, want) in [
        ("js1.qwe", "js1.qwe:1: error: `client` is for what runs in the browser, a .js or .mjs file or an http(s) address, and `javascript:alert(1)` is neither\n"),
        ("js2.qwe", "js2.qwe:1: error: `client` is for what runs in the browser, a .js or .mjs file or an http(s) address, and `data:text/javascript,1` is neither\n"),
        ("js4.qwe", "js4.qwe:1: error: `client` takes names from a file, and says which after `use`: client \"x\\\".js\" use L\n"),
        ("js5.qwe", "js5.qwe:1: error: `client` is for what runs in the browser, a .js or .mjs file or an address; lib/a.qwe is Quillo: import lib/a.qwe use ...\n"),
    ] {
        let (ok, out, _) = s11_check(&d, f);
        assert!(!ok && out == format!("{}quillo: 1 error\n", want), "{}: {}", f, out);
    }
    let (ok, out, _) = s11_check(&d, "js3.qwe");
    assert!(ok, "{}", out);
    if !s11_cargo() {
        return;
    }
    // The `!` of "does not fit" stays out of the text, in a list and in a list of lists; a
    // number too large for an int names the function.
    let d = s11_dir("q822-native", &[
        ("n.rs", "//! effects: none\npub fn small(xs: Vec<u8>) -> i64 { xs.len() as i64 }\npub fn deeper(xs: Vec<Vec<u8>>) -> i64 { xs.len() as i64 }\npub fn big() -> u64 { u64::MAX }\npub fn fits() -> u64 { i64::MAX as u64 }\n"),
        ("broken.rs", "pub fn none() -> i64 { not_there() }\npub fn also_none() -> i64 { 2 }\n"),
        ("main.qwe", "use n.rs\nprint(small([1, 255]), deeper([[1], [2, 3]]), fits())\n"),
    ]);
    let (ok, out, err) = s11_quillo(&d, &["run", "main.qwe"]);
    assert!(ok && out == "2 2 9223372036854775807\n", "{}{}", out, err);
    for (call, want) in [
        ("small([1, 256])", "`small`: argument `xs` does not fit in u8: 256 (item 1)"),
        ("deeper([[1], [1, -1]])", "`deeper`: argument `xs` does not fit in u8: -1 (item 1) (item 1)"),
        ("small([\"x\"])", "`small`: argument `xs` must be a list of int, got str (item 0)"),
        ("big()", "`big` (n.rs) gave back 18446744073709551615, which is too large for Quillo's int: give it back as text"),
    ] {
        std::fs::write(d.join("bad.qwe"), format!("use n.rs\nprint({})\n", call)).unwrap();
        let (ok, _, err) = s11_quillo(&d, &["run", "bad.qwe"]);
        assert!(!ok && err.contains(&format!("bad.qwe:2: {}\n", want)) && !err.contains('!'), "{}: {}", call, err);
    }
    // A failed build of functions that take nothing shows the file's error, not the glue's.
    std::fs::write(d.join("bad.qwe"), "use broken.rs\nprint(also_none())\n").unwrap();
    let (ok, _, err) = s11_quillo(&d, &["run", "bad.qwe"]);
    assert!(!ok && err.contains("not_there") && !err.contains("unused variable"), "{}", err);
}

// ---- end of slice 11 ----

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
    // Hidden on this test's thread alone: set for the process, the other tests' errors were hidden
    // while it ran (found in review of slice 9).
    let (s, b) = quillo::value::hiding_here(true, both);
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
    // A HEAD is judged too, by the file's first bytes (FEEDBACK55.md, Q-702; branch feedback55).
    let head = |what: (std::path::PathBuf, u64, u64, Option<statics::Hosted>)| {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = std::net::TcpStream::connect(l.local_addr().unwrap()).unwrap();
        client.set_read_timeout(Some(std::time::Duration::from_secs(30))).unwrap();
        let (server, _) = l.accept().unwrap();
        statics::send(server, &quillo::server::plain(200, ""), what, true);
        let mut out = String::new();
        client.read_to_string(&mut out).unwrap();
        out
    };
    let out = head((f.clone(), 0, 20, Some(hosted.clone())));
    assert!(out.starts_with("HTTP/1.1 404") && !out.contains("SQLite"), "{}", out);
    std::fs::remove_file(&f).unwrap();
    std::fs::write(&f, "plain again").unwrap();
    assert!(head((f.clone(), 0, 11, Some(hosted.clone()))).starts_with("HTTP/1.1 200"));
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
    // A file replaced at every try is busy, a 503 with Retry-After (the fifth review); every other
    // answer is one whole file, never two mixed, and most are that.
    let mut whole = 0;
    for i in 0..150 {
        let a = support::send(s.port, "GET", "/s/big.bin", &[], "");
        if a.status == 503 {
            assert_eq!(a.header("retry-after"), Some("1"), "request {}", i);
            continue;
        }
        assert_eq!((a.status, a.body.len()), (200, size), "request {}", i);
        assert!(a.body.bytes().all(|b| b == b'a') || a.body.bytes().all(|b| b == b'b'), "request {} mixed two files", i);
        whole += 1;
    }
    assert!(whole >= 100, "only {} of 150 were served", whole);
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
        // A path of the site's own keeps the `\` of its leading `/` run, so it stays on the site;
        // one that does not start with `/` is read as browsers read it (found in review).
        ("/\\evil.com", "/%5Cevil.com"),
        ("/\\/evil.com", "/%5C/evil.com"),
        ("/\\\\evil.com", "/%5C%5Cevil.com"),
        ("//\\evil.com", "//%5Cevil.com"),
        ("/a\\b\\c", "/a/b/c"),
        ("\\\\evil.com", "//evil.com"),
        // Its dot segments taken out, as browsers would, `%2e` too; a query's and an absolute
        // URL's are let be (RFC 3986 §5.2.4; found in review).
        ("/p\\..\\..\\evil", "/evil"),
        ("/p/%2e%2e/%2E./evil?x=/../y", "/evil?x=/../y"),
        ("/a/./b/../c/", "/a/c/"),
        ("/a/b/..", "/a/"),
        ("/..", "/"),
        ("/a/..b/.c", "/a/..b/.c"),
        ("https://good.com/a/../b", "https://good.com/a/../b"),
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

// Found in review: a check that a redirect stays on the site (`/` first, not `//`) and a path made
// of what the visitor sent stay on the site, with `--base` too, which prefixes a path of the
// site's own: `/\evil.com` sent as `//evil.com` left it, and under `--base` a `..` climbed out.
#[test]
fn a_path_of_the_sites_own_stays_on_it_with_or_without_base() {
    let d = dir(
        "redirect-base",
        "route GET /to ? { u: str } = redirect(u)\nroute GET /lang/:l = redirect(\"/\" + l + \"/home\")\nroute GET /safe ? { u: str } = starts_with(u, \"/\") && !starts_with(u, \"//\") ? redirect(u) : \"no\"\n",
    );
    for (base, opts) in [("", &[][..]), ("/app", &["--base", "/app"][..])] {
        let s = support::serve_with(&d.join("main.qwe"), opts);
        let to = |path: &str| support::send(s.port, "GET", path, &[], "").header("location").unwrap_or_default().to_string();
        for (u, want) in [
            ("/\\evil.com", "/%5Cevil.com"),
            ("/\\/evil.com", "/%5C/evil.com"),
            ("/\\\\evil.com", "/%5C%5Cevil.com"),
            ("/p\\..\\..\\evil", "/evil"),
            ("/../../evil", "/evil"),
            ("/p/%2e%2e/%2e%2e/evil", "/evil"),
            ("/x", "/x"),
        ] {
            assert_eq!(to(&format!("/safe?u={}", enc(u))), format!("{}{}", base, want), "{} under {:?}", u, base);
            assert_eq!(to(&format!("/to?u={}", enc(u))), format!("{}{}", base, want), "{} under {:?}", u, base);
        }
        assert_eq!(to("/lang/%5Cevil.com"), format!("{}/%5Cevil.com/home", base));
        assert_eq!(to("/lang/%5C%5Cevil.com"), format!("{}/%5C%5Cevil.com/home", base));
        // What is not a path of the site's own is as browsers read it, the base not before it.
        assert_eq!(to(&format!("/to?u={}", enc("\\\\evil.com"))), "//evil.com");
        assert_eq!(to(&format!("/to?u={}", enc("https://good.com\\@evil.com"))), "https://good.com/@evil.com");
        assert_eq!(support::send(s.port, "GET", &format!("/safe?u={}", enc("//evil.com")), &[], "").body, "no");
    }
}

// Found in review: a `location` that runs code or carries a page is refused in `respond` and
// `site_headers` as in `redirect` (an app went to it), in any of the forms browsers read.
#[test]
fn no_location_runs_code_or_carries_a_page() {
    let d = dir(
        "location-scripts",
        "route GET /r ? { u: str } = respond(302, null, { location: u })\nroute GET /s ? { u: str } = stream((send) => send(\"x\"), { headers: { Location: u } })\nroute GET /to ? { u: str } = redirect(u)\n",
    );
    let (_g, port, log) = serve_logged(&d, &[]);
    for (i, u) in ["javascript:alert(1)", "JavaScript:alert(1)", " java\tscript:alert(1)", "vbscript:x", "data:text/html,<script>alert(1)</script>", "DATA:x", "\u{1}data:x"].iter().enumerate() {
        for route in ["r", "s"] {
            let why = error_of(port, &log, &format!("/{}?u={}&i={}", route, enc(u), i));
            assert!(why.contains("header `location` is not given a javascript:, vbscript: or data: URL"), "{} {:?}: {}", route, u, why);
        }
        let why = error_of(port, &log, &format!("/to?u={}&i={}", enc(u), i));
        assert!(why.contains("`redirect` is not given a") || why.contains("`redirect` URL cannot contain line breaks, tabs or other control characters"), "{:?}: {}", u, why);
    }
    for u in ["/ok", "https://example.com/data:x", "?data:x", "datax:y"] {
        let a = support::send(port, "GET", &format!("/r?u={}", enc(u)), &[], "");
        assert_eq!((a.status, a.header("location")), (302, Some(u)), "{}", u);
    }
    let d = dir("location-site", "site_headers({ location: env(\"L\") ?? \"/\" })\nprint(\"ok\")\n");
    let ran = script_with_env(&d, "main.qwe", &[("L", "data:x")]);
    assert!(ran.contains("`site_headers`: `location` is not given a javascript:, vbscript: or data: URL, got \"data:x\""), "{}", ran);
    assert!(script_with_env(&d, "main.qwe", &[("L", "/elsewhere")]).contains("ok"));
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

// ---- Q-706, Q-834: the highlighter, on the server (highlight.rs) and in the browser (runtime.js) --

/// A Quillo string literal of `s`.
fn lit(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' | '"' | '$' | '{' | '}' => {
                o.push('\\');
                o.push(c);
            }
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// `want` with `<k>`...`</k>` for `<span class="hl-k">`...`</span>`, and so for each class.
fn spans(want: &str) -> String {
    let mut s = want.to_string();
    for c in ["k", "s", "c", "n", "f", "t", "a"] {
        s = s.replace(&format!("<{}>", c), &format!("<span class=\"hl-{}\">", c)).replace(&format!("</{}>", c), "</span>");
    }
    s
}

fn node_here() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// What `__t` in `src` gives on the server, as `repr` writes it, once the browser's runtime (in
/// Node) has given the same; the server's value too.
fn both_sides(src: &str) -> (String, quillo::value::Value) {
    let mut it = quillo::Interp::new(".");
    it.load_source(src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let v = match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v,
        Err(quillo::interp::Ctrl::Error(e)) => panic!("{}", e.msg),
        Err(_) => panic!("the case did not give a value"),
    };
    let server = v.repr();
    if node_here() {
        let js = quillo::client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}", e));
        let out = support::node(&js).expect("node");
        assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
        let browser = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if browser != server {
            let at = server.chars().zip(browser.chars()).take_while(|(a, b)| a == b).count();
            let around = |t: &str| t.chars().skip(at.saturating_sub(120)).take(240).collect::<String>();
            panic!("the browser differs at character {}:\nserver:  {}\nbrowser: {}", at, around(&server), around(&browser));
        }
    } else {
        eprintln!("skipping the browser half: no Node");
    }
    (server, v)
}

/// `highlight(code, lang)` of each case as the server makes it, once the browser has made the
/// same, token for token.
fn highlighted(cases: &[(&str, &str)]) -> Vec<String> {
    let calls: Vec<String> = cases.iter().map(|(l, c)| format!("str(highlight({}, {}))", lit(c), lit(l))).collect();
    let (_, v) = both_sides(&format!("fn __t() = [{}]\n", calls.join(", ")));
    let quillo::value::Value::List(l) = v else { panic!("not a list") };
    l.iter().map(|x| x.display()).collect()
}

/// Each case highlighted as `want` (written with `spans`), on both sides.
#[track_caller]
fn colours_as(cases: &[(&str, &str, &str)]) {
    let got = highlighted(&cases.iter().map(|(l, c, _)| (*l, *c)).collect::<Vec<_>>());
    for ((l, c, want), g) in cases.iter().zip(got) {
        assert_eq!(g, spans(want), "{} {:?}", l, c);
    }
}

/// The CPU time this thread has used, in seconds: what a timing test measures, so that other
/// tests and other processes on a loaded machine do not change it (found running the suite beside
/// other work).
fn cpu_now() -> f64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes the timespec given.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9
}

/// The best of three runs of `f`, in seconds of this thread's CPU time.
fn best_of_three(f: &dyn Fn()) -> f64 {
    (0..3)
        .map(|_| {
            let t = cpu_now();
            f();
            cpu_now() - t
        })
        .fold(f64::MAX, f64::min)
}

/// Texts of `n` pieces that were quadratic, or could be: the report's (`route GET /*/*...`, read
/// again from the line's start for each `/*`), and each rule Q-834 added that looks along a line.
fn slow_shapes(n: usize) -> Vec<(&'static str, String)> {
    vec![
        ("quillo", format!("route GET {}", "/*".repeat(n))),
        ("quillo", "route GET /*a /*b\n".repeat(n / 8)),
        ("quillo", format!("route GET /a {}", "/*".repeat(n))),
        ("js", format!("x = [{}", "/[".repeat(n))),
        ("js", "(/".repeat(n)),
        ("js", format!("x{}{}", " ".repeat(n), "/ /".repeat(n / 4))),
        ("rust", "'\\".repeat(n)),
        ("python", format!("\"\"\"{}", "a\n".repeat(n))),
        ("sh", "a#".repeat(n)),
        ("rust", "0..".repeat(n)),
    ]
}

// Q-706: `highlight("route GET " + repeat("/*", n), "quillo")` read the line again from its start
// for each `/*`: 5.88 s at n = 64,000, four times as long for each doubling. Each line is read once
// now: 64,000 take milliseconds, and twice the text takes about twice the time, for the report's
// text and for each rule that looks along a line.
#[test]
fn highlighting_takes_time_in_proportion_to_the_text() {
    use quillo::highlight::highlight;
    let report = format!("route GET {}", "/*".repeat(64_000));
    assert!(best_of_three(&|| drop(highlight(&report, "quillo"))) < 0.5, "64,000 `/*` took too long");
    // The path is a path to its end: no comment in it.
    let h = highlight(&report, "quillo");
    assert!(!h.contains("hl-c") && h.ends_with("/*/*"), "{}", &h[..200]);
    // Four times the text against at most six times the time: linear passes with room for a
    // loaded machine, quadratic (sixteen times) does not.
    let (small, large) = (slow_shapes(100_000), slow_shapes(400_000));
    for ((lang, a), (_, b)) in small.iter().zip(large.iter()) {
        let (ta, tb) = (best_of_three(&|| drop(highlight(a, lang))), best_of_three(&|| drop(highlight(b, lang))));
        assert!(tb < 1.5, "{} {:?}...: {:.3} s", lang, &a[..20], tb);
        assert!(tb < 6.0 * ta + 0.02, "{} {:?}...: {:.4} s, then {:.4} s for four times the text", lang, &a[..20], ta, tb);
    }
    // The edges: none, one, and a path after the route's, which is a comment to the end.
    assert_eq!(highlight("route GET ", "quillo"), "<span class=\"hl-k\">route</span> <span class=\"hl-t\">GET</span> ");
    assert_eq!(highlight("route GET /*", "quillo"), "<span class=\"hl-k\">route</span> <span class=\"hl-t\">GET</span> /*");
    assert_eq!(highlight("", "quillo"), "");
    let h = highlight(&format!("route GET /a {}", "/* ".repeat(1000)), "quillo");
    assert_eq!(h.matches("hl-c").count(), 1, "{}", &h[..200]);
}

// Q-706 in the browser: runtime.js's `inRoutePath` had the same loop (2 s at 16,000 `/*`). The
// same texts in Node, timed by the program itself.
#[test]
fn the_browser_highlights_in_proportion_to_the_text() {
    if !node_here() {
        eprintln!("skipping: needs Node");
        return;
    }
    let shape = |n: usize| -> Vec<String> {
        vec![
            format!("took(\"route GET \" + repeat(\"/*\", {}), \"quillo\")", n),
            format!("took(repeat(\"route GET /*a /*b\\n\", {}), \"quillo\")", n / 8),
            format!("took(\"x = [\" + repeat(\"/[\", {}), \"js\")", n),
            format!("took(repeat(\"(/\", {}), \"js\")", n),
            format!("took(\"x\" + repeat(\" \", {}) + repeat(\"/ /\", {}), \"js\")", n, n / 4),
            format!("took(repeat(\"'\\\\\", {}), \"rust\")", n),
            format!("took(\"\\\"\\\"\\\"\" + repeat(\"a\\n\", {}), \"python\")", n),
            format!("took(repeat(\"a#\", {}), \"sh\")", n),
            format!("took(repeat(\"0..\", {}), \"rust\")", n),
        ]
    };
    let src = format!(
        "fn took(code: str, lang: str) -> int {{\n    let best = 1000000\n    for r in [1, 2, 3] {{\n        let t0 = now()\n        let _h = highlight(code, lang)\n        let d = now() - t0\n        if d < best {{\n            best = d\n        }}\n    }}\n    return best\n}}\nfn __t() = [{}, {}, took(\"route GET \" + repeat(\"/*\", 64000), \"quillo\")]\n",
        shape(50_000).join(", "),
        shape(200_000).join(", ")
    );
    let mut it = quillo::Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let js = quillo::client::test_script(&it, "__t").unwrap();
    let out = support::node(&js).expect("node");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let ms: Vec<f64> = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)));
    let n = shape(1).len();
    assert!(ms[2 * n] < 1000.0, "64,000 `/*` took {} ms in the browser", ms[2 * n]);
    for k in 0..n {
        // Four times the text against at most six times the time (quadratic is sixteen).
        assert!(ms[n + k] < 3000.0 && ms[n + k] < 6.0 * ms[k] + 30.0, "shape {}: {} ms, then {} ms for four times the text", k, ms[k], ms[n + k]);
    }
}

// Q-706: what the server and the browser make of a large text is the same, token for token, in
// every language (each sample has the cases Q-834 fixed), and the report's text.
#[test]
fn server_and_browser_highlight_a_large_text_alike() {
    let samples = [
        ("quillo", "route GET /*rest = respond(404, rest) // last\nroutes /*x with db { }\nlet a = f(\"<a>\") /* c */ `t ${1.5}` 0..10\n"),
        ("js", "let r = /\"[/]/g, d = a / b; return /x/.test(s) // c\nconst t = `a\nb` + 'it''s' + 1e3;\n"),
        ("python", "def f(x):\n    \"\"\"Doc for if.\n    and\"\"\"\n    return '''a\nb''' if x else None  # c\n"),
        ("rust", "fn f<'a>(x: &'a str) -> char { let s = \"a\nb\"; for i in 0..n {} '\"' } // c\n"),
        ("sh", "echo it's ${#a[@]} $# 'q' # c\nexport A=1\n"),
        ("sql", "SELECT a, 'it''s' FROM t -- c\nwhere b = 1 /* d */;\n"),
        ("css", ".a:hover { color: #fff; margin: -0.5em .5em !important } /* c */\n"),
        ("html", "<!-- c --><a href=\"/x\" data-k='v'>y &amp; z</a><br/>\n"),
        ("json", "{\"a\": [1, true, null], \"b\": \"c\"}\n"),
        ("nope", "print(1) <b> & 'x'\n"),
    ];
    let calls: Vec<String> = samples.iter().map(|(l, c)| format!("str(highlight(repeat({}, 2000), {}))", lit(c), lit(l))).collect();
    let (server, v) = both_sides(&format!("fn __t() = [{}, str(highlight(\"route GET \" + repeat(\"/*\", 64000), \"quillo\"))]\n", calls.join(", ")));
    assert!(server.len() > 1_000_000, "{}", server.len());
    let quillo::value::Value::List(l) = v else { panic!("not a list") };
    assert!(l[..9].iter().all(|h| h.display().contains("<span")) && !l[9].display().contains("<span"), "{}", &server[..400]);
}

// Q-706: `/_quillo/markdown`, on every server with no route asking for it, took any body
// `--max-body` let through (10 MB), where a 256 KB one held the server 22 s. It takes what a page
// previews as someone writes: a megabyte of JSON, the limit itself, and refuses one byte more.
#[test]
fn the_markdown_endpoint_takes_a_preview_and_no_more() {
    let d = dir("md-endpoint", "route GET / = \"hello\"\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let post = |body: &str| support::send(s.port, "POST", "/_quillo/markdown", &[], body);
    // The report's body: 256 KB of `/*` in a route's path, answered at once.
    let report = serde_json::json!([format!("```quillo\nroute GET {}\n```", "/*".repeat(128_000)), {}]).to_string();
    let t = std::time::Instant::now();
    let a = post(&report);
    assert_eq!(a.status, 200, "{}", &a.body[..200.min(a.body.len())]);
    assert!(t.elapsed().as_secs_f64() < 2.0, "{:?}", t.elapsed());
    assert!(!a.body.contains("hl-c") && a.body.contains("hl-k"), "{}", &a.body[..200]);
    assert_eq!(support::send(s.port, "GET", "/", &[], "").body, "hello");
    // The limit, one byte under and one over, in bytes whatever the characters.
    let of = |text: String| format!("[\"{}\"]", text);
    let limit = quillo::server::MARKDOWN_BODY;
    assert_eq!(limit, 1 << 20);
    assert_eq!(post(&of("a".repeat(limit - 4))).status, 200);
    assert_eq!(post(&of("a".repeat(limit - 5))).status, 200);
    let a = post(&of("a".repeat(limit - 3)));
    assert_eq!(a.status, 413, "{}", a.body);
    assert_eq!(a.body, r#"{"error":"markdown here takes at most 1 MB of JSON: make a longer text's in a route or a server fn"}"#);
    let e = of("é".repeat((limit - 4) / 2));
    assert_eq!(e.len(), limit);
    assert_eq!(post(&e).status, 200);
    assert_eq!(post(&of(format!("{}a", "é".repeat((limit - 4) / 2)))).status, 413);
    // Small texts as before: empty, one character, unicode, with options.
    assert_eq!(post(r#"[""]"#).body, r#"{"ok":""}"#);
    assert_eq!(post(r#"["*a*"]"#).body, r#"{"ok":"<p><em>a</em></p>\n"}"#);
    assert_eq!(post(r#"["ünï *😀*", {"inline": true}]"#).body, r#"{"ok":"ünï <em>😀</em>"}"#);
    // Refused before it is read as JSON.
    assert_eq!(post(&format!("[{}", " ".repeat(limit))).status, 413);
    assert_eq!(post(&format!("[{}", " ".repeat(10))).status, 400);
}

// Q-706: `--max-compute` did not stop a highlight, which is one call: the log said the instance had
// been computing for 5 s, and nothing ended it. A long highlight, a Markdown block's and a text of
// many small blocks now end at the limit, as a loop does, and the instance answers the next
// request at once; what is in time is answered as before.
#[test]
fn max_compute_ends_a_long_highlight() {
    let d = dir(
        "max-compute",
        "route GET / = \"hello\"\nroute GET /h = len(str(highlight(repeat(\"a \", 12000000), \"quillo\")))\nroute GET /small = str(highlight(\"let x = 1\", \"quillo\"))\nroute GET /md = len(str(markdown(\"```quillo\\n\" + repeat(\"a \", 12000000) + \"\\n```\")))\nroute GET /blocks = len(str(markdown(repeat(\"```js\\nlet a = 1\\n```\\n\", 1000000))))\nroute GET /plain = len(str(markdown(repeat(\"```nope\\nx\\n```\\n\", 2000000))))\n",
    );
    assert!(check(&std::fs::read_to_string(d.join("main.qwe")).unwrap()).0);
    let (_g, port, log) = serve_logged(&d, &["--max-compute", "100ms"]);
    for path in ["/h", "/md", "/blocks", "/plain"] {
        let t = std::time::Instant::now();
        assert_eq!(error_of(port, &log, path), format!("main.qwe:{}: this request computed for longer than --max-compute allows", match path { "/h" => 2, "/md" => 4, "/blocks" => 5, _ => 6 }));
        assert!(t.elapsed().as_secs_f64() < 5.0, "{}: {:?}", path, t.elapsed());
        let a = support::send(port, "GET", "/", &[], "");
        assert_eq!((a.status, a.body.as_str()), (200, "hello"));
    }
    let a = support::send(port, "GET", "/small", &[], "");
    assert_eq!((a.status, a.body.as_str()), (200, "<span class=\"hl-k\">let</span> x = <span class=\"hl-n\">1</span>"));
    let a = support::send(port, "POST", "/_quillo/markdown", &[], r#"["```rust,ignore\nfn\n```"]"#);
    assert_eq!(a.body, r#"{"ok":"<pre class=\"hl\"><code class=\"language-rust\"><span class=\"hl-k\">fn</span>\n</code></pre>\n"}"#);
}

// Q-706: a preview at `/_quillo/markdown` past `--max-compute` ends as a route does, a 500 that
// says so, where the endpoint said 400. Under a limit of 1 ms some full-size previews are found
// past it (the watcher looks every 100 ms); the others are made whole.
#[test]
fn a_preview_past_max_compute_ends_as_a_server_error() {
    let d = dir("max-compute-preview", "route GET / = \"hello\"\n");
    let (_g, port, _log) = serve_logged(&d, &["--max-compute", "1ms"]);
    let mut text = String::new();
    while text.len() < 900_000 {
        text.push_str("```js\nlet a = f(1) // c\n```\n");
    }
    let body = serde_json::json!([text]).to_string();
    assert!(body.len() <= quillo::server::MARKDOWN_BODY, "{}", body.len());
    let mut ended = false;
    for _ in 0..200 {
        let a = support::send(port, "POST", "/_quillo/markdown", &[], &body);
        match a.status {
            200 => assert!(a.body.starts_with(r#"{"ok":"<pre class=\"hl\"><code class=\"language-js\"><span class=\"hl-k\">let</span>"#), "{}", &a.body[..200]),
            500 => {
                assert_eq!(a.body, r#"{"error":"this request computed for longer than --max-compute allows"}"#);
                ended = true;
                break;
            }
            s => panic!("{}: {}", s, a.body),
        }
    }
    assert!(ended, "no preview was found past the limit");
    assert_eq!(support::send(port, "GET", "/", &[], "").body, "hello");
}

// Q-834: a Python docstring over lines coloured its inner lines as code, and so did a Rust string
// over lines. Both are strings to their close now, or to the end; Quillo's and JavaScript's `"`
// still end at their line's end.
#[test]
fn a_docstring_and_a_rust_string_over_lines_are_strings() {
    colours_as(&[
        ("py", "def f():\n    \"\"\"Docs for and if.\n    more\n    \"\"\"\n    return 1", "<k>def</k> <f>f</f>():\n    <s>&quot;&quot;&quot;Docs for and if.\n    more\n    &quot;&quot;&quot;</s>\n    <k>return</k> <n>1</n>"),
        ("python", "x = '''a\nfor b'''\ny = \"\" if x else ''", "x = <s>'''a\nfor b'''</s>\ny = <s>&quot;&quot;</s> <k>if</k> x <k>else</k> <s>''</s>"),
        ("py", "\"\"\"never\nfor", "<s>&quot;&quot;&quot;never\nfor</s>"),
        ("py", "\"\"\"a\\\"\"\"b\"\"\" + 'y'", "<s>&quot;&quot;&quot;a\\&quot;&quot;&quot;b&quot;&quot;&quot;</s> + <s>'y'</s>"),
        ("py", "\"\"\"\"\"\"x", "<s>&quot;&quot;&quot;&quot;&quot;&quot;</s>x"),
        ("py", "'''é\n日本😀'''", "<s>'''é\n日本😀'''</s>"),
        ("rust", "let s = \"one\nfor two\";\nlet d = 1;", "<k>let</k> s = <s>&quot;one\nfor two&quot;</s>;\n<k>let</k> d = <n>1</n>;"),
        ("rs", "\"a\\\"\nb\" c", "<s>&quot;a\\&quot;\nb&quot;</s> c"),
        ("rust", "\"open\nfn", "<s>&quot;open\nfn</s>"),
        ("qwe", "let a = \"x\nlet b = 1", "<k>let</k> a = <s>&quot;x\n</s><k>let</k> b = <n>1</n>"),
        ("js", "let a = \"x\nlet b = 1", "<k>let</k> a = <s>&quot;x\n</s><k>let</k> b = <n>1</n>"),
    ]);
}

// Q-834: Rust's `'"'`, JavaScript's `/"/` and the shell's `it's` made the rest of the line a
// string. A Rust character is one (a lifetime is not), a JavaScript regular expression is one
// where a value cannot end (a `/` after one divides), and a `'` in a shell word is no quote.
#[test]
fn a_quote_in_a_character_a_regex_or_a_word_starts_no_string() {
    colours_as(&[
        ("rust", "let c = '\"'; let d = 1;", "<k>let</k> c = <s>'&quot;'</s>; <k>let</k> d = <n>1</n>;"),
        ("rust", "fn f<'a>(x: &'a str) -> &'static str { \"y\" }", "<k>fn</k> f&lt;'a&gt;(x: &amp;'a str) -&gt; &amp;'<k>static</k> str { <s>&quot;y&quot;</s> }"),
        (
            "rust",
            "['\\'', '\\\\', '\\n', '\\u{1F600}', '\\u{10FFFF}', 'é', '日', 'ab']",
            "[<s>'\\''</s>, <s>'\\\\'</s>, <s>'\\n'</s>, <s>'\\u{1F600}'</s>, <s>'\\u{10FFFF}'</s>, <s>'é'</s>, <s>'日'</s>, 'ab']",
        ),
        // Past an escape's length, over a line, or at the end: no character.
        ("rust", "'\\u{10FFFFF}'", "'\\u{<n>10FFFFF</n>}'"),
        ("rust", "'\\\nx'", "'\\\nx'"),
        ("rust", "'\n'", "'\n'"),
        ("rust", "'", "'"),
        ("rust", "' '", "<s>' '</s>"),
        ("js", "let r = /\"/g; let s = \"x\";", "<k>let</k> r = <s>/&quot;/g</s>; <k>let</k> s = <s>&quot;x&quot;</s>;"),
        ("js", "if (/[/\"]/.test(s)) return /a\\/b/i", "<k>if</k> (<s>/[/&quot;]/</s>.<f>test</f>(s)) <k>return</k> <s>/a\\/b/i</s>"),
        ("js", "/re/.test(s); typeof /y/", "<s>/re/</s>.<f>test</f>(s); <k>typeof</k> <s>/y/</s>"),
        ("ts", "let d = a / b / c, e = (a) / 2, f = x[1] / 3, g = this / 4;", "<k>let</k> d = a / b / c, e = (a) / <n>2</n>, f = x[<n>1</n>] / <n>3</n>, g = <k>this</k> / <n>4</n>;"),
        // One that does not close on its line is a `/`.
        ("js", "x = /abc \"s\"\ny = /d/", "x = /abc <s>&quot;s&quot;</s>\ny = <s>/d/</s>"),
        ("js", "x = /é日😀/u", "x = <s>/é日😀/u</s>"),
        ("sh", "echo it's fine 'quoted' # c", "<k>echo</k> it's fine <s>'quoted'</s> <c># c</c>"),
        ("bash", "'a' don't x='b' \"it's\"", "<s>'a'</s> don't x=<s>'b'</s> <s>&quot;it's&quot;</s>"),
        // Python's prefixes are a word's: its quotes are quotes.
        ("py", "f'x' + b'y'", "f<s>'x'</s> + b<s>'y'</s>"),
    ]);
}

// Q-834: the shell's `${#arr[@]}` started a comment. A `#` starts one at a word's start only, as
// the shell reads it; Python's `#` is a comment anywhere.
#[test]
fn the_shells_hash_inside_a_word_starts_no_comment() {
    colours_as(&[
        ("sh", "n=${#arr[@]} $# x # note\n# start\na;# c\na#b", "n=${#arr[@]} $# x <c># note</c>\n<c># start</c>\na;<c># c</c>\na#b"),
        ("sh", "#", "<c>#</c>"),
        ("zsh", "a|#c\n(#d)\nx\t#e", "a|<c>#c</c>\n(<c>#d)</c>\nx\t<c>#e</c>"),
        ("sh", "é#x 日 #y", "é#x 日 <c>#y</c>"),
        ("py", "x#c", "x<c>#c</c>"),
    ]);
}

// Q-834: `0..n` was one number. A range's `..` ends the number before it and starts none; decimals,
// exponents, hex and CSS's `.5em` are as they were.
#[test]
fn a_range_is_two_numbers() {
    colours_as(&[
        ("rust", "for i in 0..n { a[1..=5]; 1.5; 1.5..2.5; ..5; x.. }", "<k>for</k> i <k>in</k> <n>0</n>..n { a[<n>1</n>..=<n>5</n>]; <n>1.5</n>; <n>1.5</n>..<n>2.5</n>; ..<n>5</n>; x.. }"),
        ("qwe", "for i in 0..10 { }", "<k>for</k> i <k>in</k> <n>0</n>..<n>10</n> { }"),
        ("rust", "0..", "<n>0</n>.."),
        ("rust", "0...5", "<n>0</n>...<n>5</n>"),
        ("css", "a { margin: .5em -0.5em }", "<t>a</t> { <a>margin</a>: <n>.5em</n> -<n>0.5em</n> }"),
        ("js", "1e3 + 0x1F + 1_000 + 2.5 + .5", "<n>1e3</n> + <n>0x1F</n> + <n>1_000</n> + <n>2.5</n> + <n>.5</n>"),
    ]);
}

// Q-834: `highlight("print(1)", "nope")` gave `hl-f` and `hl-n` spans, where the guide says a
// language it does not know is escaped text. It is, on both sides; a language it knows, in any
// case, still colours.
#[test]
fn a_language_it_does_not_know_is_only_escaped() {
    colours_as(&[
        ("nope", "print(1)", "print(1)"),
        ("", "f(1) <b>&\"x\" 'y'", "f(1) &lt;b&gt;&amp;&quot;x&quot; 'y'"),
        ("ﬁ", "x(1) // c", "x(1) // c"),
        ("c", "", ""),
        ("text", "é 日本 😀 f(2)", "é 日本 😀 f(2)"),
        ("PYTHON", "print(1)", "<f>print</f>(<n>1</n>)"),
    ]);
    // With no language at all, as with an unknown one.
    let (server, _) = both_sides("fn __t() = [str(highlight(\"f(1) < 2\")), str(highlight(\"f(1) < 2\", null))]\n");
    assert_eq!(server, r#"["f(1) &lt; 2", "f(1) &lt; 2"]"#);
}

// Q-834: a fence's info `rust,ignore` was not read as Rust (nor its class `language-rust`). The
// language is the info's first word, to a space or a comma.
#[test]
fn a_fences_language_is_its_first_word_to_a_comma() {
    let md = |m: &str| {
        let mut it = quillo::Interp::new(".");
        it.load_source(&format!("fn __t() = str(markdown({}))\n", lit(m)), "case.qwe").unwrap();
        it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0).unwrap_or_else(|_| panic!("{}", m)).display()
    };
    let rust = "<pre class=\"hl\"><code class=\"language-rust\"><span class=\"hl-k\">fn</span>\n</code></pre>\n";
    for m in ["```rust,ignore\nfn\n```", "```rust ignore\nfn\n```", "```rust\nfn\n```", "``` rust,no_run\nfn\n```", "```,rust\nfn\n```", "```rust,\nfn\n```", "```rs,should_panic,edition2021\nfn\n```"] {
        let want = if m.contains("rs,") { rust.replace("language-rust", "language-rs") } else { rust.to_string() };
        assert_eq!(md(m), want, "{}", m);
    }
    assert_eq!(md("```nope,x\nf(1)\n```"), "<pre class=\"hl\"><code class=\"language-nope\">f(1)\n</code></pre>\n");
    assert_eq!(md("```\nf(1)\n```"), "<pre><code>f(1)\n</code></pre>\n");
    assert_eq!(md("```日本,rust\nf(1)\n```"), "<pre class=\"hl\"><code class=\"language-\">f(1)\n</code></pre>\n");
}

/// Seconds `markdown(text)` takes, the best of three, on the server.
fn markdown_takes(it: &mut quillo::Interp, f: &str, text: &str) -> f64 {
    let md = quillo::value::lookup(&it.globals, f).unwrap();
    (0..3)
        .map(|_| {
            let t = cpu_now();
            assert!(it.call(&md, vec![quillo::value::Value::str(text)], 0).is_ok(), "{} failed", f);
            cpu_now() - t
        })
        .fold(f64::MAX, f64::min)
}

// Q-706 in Markdown: pulldown-cmark 0.12 was quadratic on mixed emphasis, `*a_*a_...`: a megabyte
// at `/_quillo/markdown`, under its limit, held the server 17.4 s, past `--max-compute 2s`, and a
// request sent after it waited; `markdown()` and `markdown_page()` of a visitor's text in a route,
// up to `--max-body`'s 10 MB, worse. 0.13 is linear: the report's body is answered at once, the
// next request too, and each shape takes about twice as long at twice the size.
#[test]
fn markdown_takes_time_in_proportion_to_the_text() {
    let d = dir("md-linear", "route GET / = \"hello\"\n");
    let (_g, port, _log) = serve_logged(&d, &["--max-compute", "2s"]);
    let report = serde_json::json!(["*a_".repeat(349_521)]).to_string();
    assert!(report.len() <= quillo::server::MARKDOWN_BODY, "{}", report.len());
    let t = std::time::Instant::now();
    let a = support::send(port, "POST", "/_quillo/markdown", &[], &report);
    assert_eq!(a.status, 200, "{}", &a.body[..200.min(a.body.len())]);
    // 0.12 took 17 s; the bound leaves room for a loaded machine.
    assert!(t.elapsed().as_secs_f64() < 8.0, "the report's body took {:?}", t.elapsed());
    assert!(a.body.starts_with(r#"{"ok":"<p>"#), "{}", &a.body[..200]);
    assert_eq!(support::send(port, "GET", "/", &[], "").body, "hello");
    // On the server, markdown() of each shape and markdown_page() of the report's, at 256 KB,
    // 512 KB and 1 MB (0.12: 1.1 s, 4.4 s and 17 s).
    let mut it = quillo::Interp::new(".");
    it.load_source("fn md(t: str) = markdown(t)\nfn page(t: str) = markdown_page(t)\n", "case.qwe").unwrap();
    for (f, p) in [("page", "*a_"), ("md", "*a_"), ("md", "**a_"), ("md", " *_"), ("md", "_a*"), ("md", "*a**"), ("md", "[a]("), ("md", "`a"), ("md", "<a"), ("md", "~~a*"), ("md", "> *a_\n"), ("md", "- *a_\n")] {
        let [t1, t2, t4] = [1usize, 2, 4].map(|k| markdown_takes(&mut it, f, &p.repeat((k << 18) / p.len())));
        assert!(t4 < 2.0, "{} {:?}: 1 MB took {:.3} s", f, p, t4);
        // Four times the text against at most six times the time (0.12 was sixteen).
        assert!(t4 < 6.0 * t1 + 0.03, "{} {:?}: {:.4} s, {:.4} s, {:.4} s", f, p, t1, t2, t4);
    }
    // Ordinary Markdown, as it was: emphasis, links, code, lists, tables, a task.
    let mut md = |t: &str| it.call(&quillo::value::lookup(&it.globals, "md").unwrap(), vec![quillo::value::Value::str(t)], 0).unwrap_or_else(|_| panic!("{}", t)).display();
    assert_eq!(md("*a* _b_ **c** __d__ ***e*** *f_ g_h `i` ~~j~~"), "<p><em>a</em> <em>b</em> <strong>c</strong> <strong>d</strong> <em><strong>e</strong></em> *f_ g_h <code>i</code> <del>j</del></p>\n");
    assert_eq!(md("[a](/x \"t\") ![b](/y) <https://z.example>"), "<p><a href=\"/x\" title=\"t\">a</a> <img src=\"/y\" alt=\"b\" /> <a href=\"https://z.example\">https://z.example</a></p>\n");
    assert_eq!(md("- [x] done\n- [ ] todo\n\n| a | b |\n|---|--:|\n| 1 | 2 |"), "<ul>\n<li><input disabled=\"\" type=\"checkbox\" checked=\"\"/>\ndone</li>\n<li><input disabled=\"\" type=\"checkbox\"/>\ntodo</li>\n</ul>\n<table><thead><tr><th>a</th><th style=\"text-align: right\">b</th></tr></thead><tbody>\n<tr><td>1</td><td style=\"text-align: right\">2</td></tr>\n</tbody></table>\n");
    assert_eq!(md(""), "");
    assert_eq!(md("ünï *😀*"), "<p>ünï <em>😀</em></p>\n");
}

// Q-834: a Rust raw string, read as a `"` string that goes on over lines, coloured the rest of the
// text: `r"C:\temp\"` and `r#"""#`. `r"..."`, `r#"..."#` (to a `"` and as many `#`) and `br"..."`
// are raw: no escapes, over lines, to the end when they do not close. Elsewhere `r` is a word.
#[test]
fn a_rust_raw_string_has_no_escapes() {
    colours_as(&[
        ("rust", "let p = r\"C:\\temp\\\";\nlet x = 1;", "<k>let</k> p = <s>r&quot;C:\\temp\\&quot;</s>;\n<k>let</k> x = <n>1</n>;"),
        ("rust", "let q = r#\"\"\"#; let y = 2;", "<k>let</k> q = <s>r#&quot;&quot;&quot;#</s>; <k>let</k> y = <n>2</n>;"),
        ("rust", "br\"a\\\" b r##\"x\"#y\"## r#type rx\"z\"", "<s>br&quot;a\\&quot;</s> b <s>r##&quot;x&quot;#y&quot;##</s> r#<k>type</k> rx<s>&quot;z&quot;</s>"),
        ("rust", "r##\"a\"#\"## r\"\"", "<s>r##&quot;a&quot;#&quot;##</s> <s>r&quot;&quot;</s>"),
        ("rust", "r\"a\nfn b\" fn", "<s>r&quot;a\nfn b&quot;</s> <k>fn</k>"),
        ("rust", "r#\"a\" fn", "<s>r#&quot;a&quot; fn</s>"),
        ("rs", "r r# br \"r\" b\"x\\\"y\" r\"é日😀\"", "r r# br <s>&quot;r&quot;</s> b<s>&quot;x\\&quot;y&quot;</s> <s>r&quot;é日😀&quot;</s>"),
        ("rust", "r", "r"),
        ("js", "r\"x\" br\"y\"", "r<s>&quot;x&quot;</s> br<s>&quot;y&quot;</s>"),
    ]);
}

// Q-834 (found in review): a word's letters were Rust's `is_alphanumeric` on the server and
// `\p{L}\p{N}` in the browser, so a mark (U+093F in `कि`) or a letter number (`Ⅰ`) ended a word on
// one side only: `echo कि'x'` was a string in the browser alone. Both read Unicode's Alphabetic and
// numbers now.
#[test]
fn a_word_of_marks_or_letter_numbers_is_the_same_word_on_both_sides() {
    colours_as(&[
        ("sh", "echo कि'x' y", "<k>echo</k> कि'x' y"),
        ("sh", "echo ा'x' ॅ'y' a'z'", "<k>echo</k> ा'x' ॅ'y' a'z'"),
        ("js", "Ⅰ(1) f\u{345}(2)", "<f>Ⅰ</f>(<n>1</n>) <f>f\u{345}</f>(<n>2</n>)"),
        ("py", "Ⅻ = 1", "<t>Ⅻ</t> = <n>1</n>"),
        ("rust", "let नमस्ते = 'x';", "<k>let</k> नमस्ते = <s>'x'</s>;"),
        ("js", "e\u{301}(1)", "e\u{301}(<n>1</n>)"),
        ("qwe", "let x² = ½", "<k>let</k> x² = ½"),
    ]);
}

// Q-702: what `files` sends is judged by the bytes it sends: a file whose first bytes are made a
// SQLite database's and back again, in place, as fast as can be, never goes out starting as one,
// whole or in part, small or large (FEEDBACK55.md, Q-702; branch feedback55).
#[cfg(unix)]
#[test]
fn a_database_header_written_in_place_is_never_sent() {
    let d = dir("flip", "route GET /s/*p = files(\"public\", p)\n");
    std::fs::create_dir_all(d.join("public")).unwrap();
    std::fs::write(d.join("public/small.bin"), vec![b's'; 4096]).unwrap();
    std::fs::write(d.join("public/large.bin"), vec![b'l'; 2 << 20]).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flipper = {
        let (d, stop) = (d.clone(), stop.clone());
        std::thread::spawn(move || {
            use std::io::{Seek, Write};
            let mut files: Vec<std::fs::File> = ["small.bin", "large.bin"].iter().map(|n| std::fs::OpenOptions::new().write(true).open(d.join("public").join(n)).unwrap()).collect();
            let mut n = 0u64;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                n += 1;
                for f in files.iter_mut() {
                    f.seek(std::io::SeekFrom::Start(0)).unwrap();
                    f.write_all(if n % 2 == 0 { b"SQLite format 3\0" } else { b"plain text, 16 b" }).unwrap();
                }
            }
        })
    };
    let mut served = 0;
    for i in 0..300 {
        for (p, h) in [("/s/small.bin", vec![]), ("/s/small.bin", vec![("Range", "bytes=0-99")]), ("/s/small.bin", vec![("Range", "bytes=0-4")]), ("/s/large.bin", vec![]), ("/s/large.bin", vec![("Range", "bytes=0-99")]), ("/s/large.bin", vec![("Range", "bytes=0-4")])] {
            let a = support::send(s.port, "GET", p, &h, "");
            if a.status == 200 || a.status == 206 {
                served += 1;
                assert!(!a.body.starts_with("SQLit"), "{} {:?} (request {}) sent a database's first bytes", p, h, i);
            } else {
                assert!(a.status == 404 || a.status == 503, "{} {:?}: {}", p, h, a.status);
            }
        }
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    flipper.join().unwrap();
    assert!(served > 0, "nothing was served while the header changed");
}

// Q-701: a large file being renamed into place again and again, asked for in part, is served
// whole or in part, or a 503 to come back to after a few tries a moment apart: never a 404.
#[cfg(unix)]
#[test]
fn a_large_file_renamed_into_place_is_never_missed() {
    let d = dir("rename-large", "route GET /s/*p = files(\"public\", p)\n");
    std::fs::create_dir_all(d.join("public")).unwrap();
    std::fs::write(d.join("public/big.bin"), vec![b'a'; 3 << 20]).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saver = {
        let (d, stop) = (d.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut n = 0u32;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                n += 1;
                let tmp = d.join(format!("public/big.{}.tmp", n % 2));
                std::fs::write(&tmp, vec![b'a' + (n % 2) as u8; 3 << 20]).unwrap();
                std::fs::rename(&tmp, d.join("public/big.bin")).unwrap();
            }
        })
    };
    for i in 0..200 {
        let a = support::send(s.port, "GET", "/s/big.bin", &[("Range", "bytes=0-2000000")], "");
        assert!(matches!(a.status, 200 | 206 | 503), "request {}: {}", i, a.status);
        if a.status == 503 {
            assert_eq!(a.header("retry-after"), Some("1"));
        } else {
            assert!(a.body.len() == 2_000_001 || a.body.len() == 3 << 20, "request {}: {} bytes", i, a.body.len());
        }
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    saver.join().unwrap();
}

// Q-701, Q-702: a large file `files` judged is not looked into again for a 304 or a HEAD while it
// is the same; written to since (made a database here), it is judged again, and kept back.
#[cfg(unix)]
#[test]
fn a_large_files_judgement_holds_until_it_changes() {
    let d = dir("judged-large", "route GET /s/*p = files(\"public\", p)\n");
    std::fs::create_dir_all(d.join("public")).unwrap();
    std::fs::write(d.join("public/big.bin"), vec![b'b'; 2 << 20]).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |m: &str, h: &[(&str, &str)]| support::send(s.port, m, "/s/big.bin", h, "");
    let tag = get("HEAD", &[]).header("etag").unwrap().to_string();
    for _ in 0..3 {
        assert_eq!(get("GET", &[("If-None-Match", &tag)]).status, 304);
        assert_eq!(get("HEAD", &[]).status, 200);
    }
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).open(d.join("public/big.bin")).unwrap();
        f.write_all(b"SQLite format 3\0").unwrap();
    }
    for (m, h) in [("HEAD", vec![]), ("GET", vec![("If-None-Match", tag.as_str())]), ("GET", vec![]), ("GET", vec![("Range", "bytes=100-199")])] {
        let a = get(m, &h);
        assert_eq!(a.status, 404, "{} {:?}", m, h);
        assert!(!a.body.contains("SQLite"));
    }
}

// Q-701: a file changed so lately that a write in the same tick of its clock would not be seen is
// not kept in memory: within 50 ms of a change where times have nanoseconds, two seconds where they
// are whole seconds (git's rule for a racily clean file), judged by ctime on unix, which no
// program sets; a time ahead of now by more than that is the clock's, and the file is kept: one
// unpacked with an mtime in 2030 is served from memory as another.
#[cfg(unix)]
#[test]
fn a_file_changed_this_moment_is_not_kept() {
    use quillo::statics::{self, racy_time};
    use std::time::{Duration, UNIX_EPOCH};
    let now = UNIX_EPOCH + Duration::new(1_800_000_000, 500_000_000);
    let ms = |n: u64| Duration::from_millis(n);
    // Nanoseconds: 50 ms either side.
    for (t, want) in [(now, true), (now - ms(49), true), (now - ms(50), false), (now - ms(51), false), (now - ms(3600_000), false), (now + ms(49), true), (now + ms(51), false), (now + ms(3600_000), false)] {
        assert_eq!(racy_time(t, now), want, "{:?}", t);
    }
    // Whole seconds: two seconds either side.
    let second = |n: u64| UNIX_EPOCH + Duration::from_secs(n);
    for (t, want) in [(second(1_800_000_000), true), (second(1_799_999_999), true), (second(1_799_999_998), false), (second(1_800_000_002), true), (second(1_800_000_003), false), (second(0), false)] {
        assert_eq!(racy_time(t, now), want, "{:?}", t);
    }
    // A file with its mtime in 2030, not changed since by ctime: kept, so read again from memory
    // after it is gone.
    let d = support::temp_dir("feedback55-racy");
    let f = d.join("future.txt");
    std::fs::write(&f, "from the future").unwrap();
    std::fs::File::options().write(true).open(&f).unwrap().set_modified(second(1_893_456_000)).unwrap();
    std::thread::sleep(ms(120));
    assert!(!statics::racy(&std::fs::metadata(&f).unwrap(), std::time::SystemTime::now()));
    let info = statics::info(&f).unwrap();
    assert_eq!(&*statics::read(&f, &info).unwrap(), b"from the future");
    std::fs::remove_file(&f).unwrap();
    assert_eq!(&*statics::read(&f, &info).unwrap(), b"from the future");
    // Just written: not kept, so gone with the file.
    let g = d.join("fresh.txt");
    std::fs::write(&g, "fresh").unwrap();
    let info = statics::info(&g).unwrap();
    assert_eq!(&*statics::read(&g, &info).unwrap(), b"fresh");
    if statics::racy(&std::fs::metadata(&g).unwrap(), std::time::SystemTime::now()) {
        std::fs::remove_file(&g).unwrap();
        assert!(statics::read(&g, &info).is_err());
    }
}

// Q-701: files of 64 KiB to 1 MiB read side by side are read a few at a time off the lock, each
// as it is; and inside a SQLite transaction, where a request cannot wait elsewhere, on its own
// thread.
#[test]
fn files_read_side_by_side_and_in_a_transaction() {
    let d = dir("side-by-side", "db.open(\"app.db\")\nroute GET /s/*p = files(\"public\", p)\nroute GET /t/*p = db.transaction(() => files(\"public\", p))\n");
    std::fs::create_dir_all(d.join("public")).unwrap();
    for i in 0..48 {
        std::fs::write(d.join(format!("public/f{}.bin", i)), vec![b'a' + (i % 26) as u8; (100 << 10) + i * 1000]).unwrap();
    }
    std::thread::sleep(std::time::Duration::from_millis(60));
    let s = support::serve(&d.join("main.qwe"), &[]);
    let port = s.port;
    let threads: Vec<_> = (0..48)
        .map(|i| {
            std::thread::spawn(move || {
                let a = support::send(port, "GET", &format!("/s/f{}.bin", i), &[], "");
                (a.status, a.body.len(), a.body.bytes().all(|b| b == b'a' + (i % 26) as u8))
            })
        })
        .collect();
    for (i, t) in threads.into_iter().enumerate() {
        assert_eq!(t.join().unwrap(), (200, (100 << 10) + i * 1000, true), "f{}", i);
    }
    let a = support::send(port, "GET", "/t/f7.bin", &[], "");
    assert_eq!((a.status, a.body.len()), (200, (100 << 10) + 7000));
    assert_eq!(support::send(port, "GET", "/t/f8.bin", &[("Range", "bytes=0-9")], "").body, "iiiiiiiiii");
}

// Q-701: the paths `--env-file` and `--tls-key` name are not written, nor deleted, while nothing is
// there, as when something is.
#[cfg(unix)]
#[test]
fn the_named_paths_are_not_written_while_nothing_is_there() {
    let d = dir("named-paths", "route GET /w/*p = is_err(write_file(p, \"x\")) ? \"refused\" : \"written\"\nroute GET /d/*p = is_err(delete_file(p)) ? \"refused\" : \"deleted\"\n");
    std::fs::create_dir_all(d.join("conf")).unwrap();
    std::fs::write(d.join("conf/app.env"), "A=1\n").unwrap();
    std::fs::write(d.join("conf/key.pem"), "-----BEGIN PRIVATE KEY-----\n").unwrap();
    std::fs::write(d.join("conf/cert.pem"), "-----BEGIN CERTIFICATE-----\n").unwrap();
    let (env, cert, key) = (d.join("conf/app.env"), d.join("conf/cert.pem"), d.join("conf/key.pem"));
    let s = support::serve_with(&d.join("main.qwe"), &["--env-file", &env.to_string_lossy(), "--tls-cert", &cert.to_string_lossy(), "--tls-key", &key.to_string_lossy()]);
    let get = |p: &str| support::send(s.port, "GET", p, &[], "").body;
    for p in ["conf/app.env", "conf/key.pem", "conf/APP.ENV"] {
        assert_eq!(get(&format!("/w/{}", p)), "refused", "{}", p);
        assert_eq!(get(&format!("/d/{}", p)), "refused", "{}", p);
    }
    std::fs::remove_file(&env).unwrap();
    std::fs::remove_file(&key).unwrap();
    for p in ["conf/app.env", "conf/key.pem", "./conf/app.env"] {
        assert_eq!(get(&format!("/w/{}", p)), "refused", "{}", p);
    }
    assert!(!env.exists() && !key.exists());
    // Beside them, and the certificate, as before.
    for p in ["conf/app.env2", "conf/key.pem.bak", "conf/other.txt"] {
        assert_eq!(get(&format!("/w/{}", p)), "written", "{}", p);
    }
    assert_eq!(get("/w/conf/cert.pem"), "written");
}

// ---- slice 5: the database layer reads SQL as each database does (Q-708, Q-727, Q-728, Q-729,
// Q-731, Q-738, Q-746..Q-749, Q-755) -------------------------------------------------------

use std::sync::{Arc, Weak};

/// A PostgreSQL server for these tests, shared by those running at once (each with a database of
/// its own); None when it is not installed.
fn pg_server() -> Option<Arc<support::Pg>> {
    static SHARED: Mutex<(Weak<support::Pg>, u32)> = Mutex::new((Weak::new(), 0));
    let mut g = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = g.0.upgrade() {
        return Some(s);
    }
    g.1 += 1;
    let s = Arc::new(support::postgres(&format!("f55db{}", g.1))?);
    g.0 = Arc::downgrade(&s);
    Some(s)
}

/// The same for MariaDB.
fn maria_server() -> Option<Arc<support::Maria>> {
    static SHARED: Mutex<(Weak<support::Maria>, u32)> = Mutex::new((Weak::new(), 0));
    let mut g = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = g.0.upgrade() {
        return Some(s);
    }
    g.1 += 1;
    let s = Arc::new(support::mariadb(&format!("f55db{}", g.1))?);
    g.0 = Arc::downgrade(&s);
    Some(s)
}

/// The servers a test holds while it runs.
#[allow(dead_code)]
struct Servers(Option<Arc<support::Pg>>, Option<Arc<support::Maria>>);

/// Each database a test runs on, fresh for `name`: SQLite (a file), and PostgreSQL and MariaDB
/// where they are installed. As (kind, URL).
fn databases(name: &str) -> (Vec<(&'static str, String)>, Servers) {
    let d = support::temp_dir(&format!("feedback55-db-{}", name));
    let mut out = vec![("sqlite", d.join("app.db").to_string_lossy().into_owned())];
    let pg = pg_server();
    match &pg {
        Some(pg) => out.push(("postgres", pg.database(&format!("f55_{}", name)))),
        None => eprintln!("skipping PostgreSQL: initdb and pg_ctl were not found"),
    }
    let my = maria_server();
    match &my {
        Some(my) => {
            quillo::my::Pool::open_as(&my.url(&format!("f55_{}", name)), true).unwrap();
            out.push(("mariadb", my.url(&format!("f55_{}", name))));
        }
        None => eprintln!("skipping MariaDB: mariadbd and mariadb-install-db were not found"),
    }
    (out, Servers(pg, my))
}

/// A program on `url`, in a folder of its own.
fn on_db(url: &str, more: &str) -> quillo::Interp {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = support::temp_dir(&format!("feedback55-dbp-{}", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let mut it = quillo::Interp::new(&d);
    let src = format!("db.open(\"{}\")\n{}", url, more);
    it.load_source(&src, "app.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    it
}

/// What `body` gives, as written by `repr`; a run-time error as "ERROR: ...".
fn ev(it: &mut quillo::Interp, body: &str) -> String {
    it.load_source(&format!("fn __t() {{\n{}\n}}\n", body), "case.qwe").unwrap_or_else(|x| panic!("{}\n{}", x, body));
    match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr().replace("\\\"", "\""),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    }
}

fn has(got: &str, what: &str, case: &str) {
    assert!(got.contains(what), "{}: wanted {:?} in: {}", case, what, got);
}

// Q-729: placeholders with no values are refused on every database, where SQLite stored nulls.
#[test]
fn placeholders_without_values_are_refused_on_every_database() {
    let (dbs, _held) = databases("novalues");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "db.exec(\"create table ub (name varchar(20), qty int)\")\n");
        for sql in [
            "insert into ub (name, qty) values (?, ?)",
            "insert into ub (name, qty) values (:n, :q)",
            "insert into ub (name, qty) values (?1, ?2)",
            "insert into ub (name, qty) values ('a', ?2)",
        ] {
            has(&ev(&mut it, &format!("db.exec(\"{}\")", sql)), "the query has 2 parameters, got 0", &format!("{} {}", kind, sql));
        }
        // One, in a query too.
        has(&ev(&mut it, "db.query(\"select * from ub where name = ?\")"), "the query has 1 parameter, got 0", kind);
        has(&ev(&mut it, "db.one(\"select * from ub where name = :name\")"), "the query has 1 parameter, got 0", kind);
        assert_eq!(ev(&mut it, "db.query(\"select * from ub\")"), "[]", "{}: nothing stored", kind);
        // A `?` in a string, a name or a comment is none: as before.
        let q = if *kind == "mariadb" { "`n?`" } else { "\\\"n?\\\"" };
        assert_eq!(ev(&mut it, &format!("db.exec(\"insert into ub (name) values ('?') -- ?\")\ndb.query(\"select name as {} from ub /* ? */\") | map((r) => values(r))", q)), "[[\"?\"]]", "{}", kind);
        // As many values as placeholders, and none for none.
        assert_eq!(ev(&mut it, "db.exec(\"insert into ub (name, qty) values (?, ?)\", \"é\", 2).changes"), "1", "{}", kind);
        assert_eq!(ev(&mut it, "db.exec(\"delete from ub\").changes"), "2", "{}", kind);
        has(&ev(&mut it, "db.exec(\"insert into ub (name, qty) values (?, ?)\", \"a\")"), "the query has 2 parameters, got 1", kind);
        has(&ev(&mut it, "db.exec(\"insert into ub (name, qty) values (?, ?)\", \"a\", 1, 2)"), "the query has 2 parameters, got 3", kind);
    }
}

// Q-727: an error given as a value is given back, saying what it was, and a function is refused,
// on every database; nothing is stored.
#[test]
fn an_error_given_as_a_value_is_given_back_on_every_database() {
    let (dbs, _held) = databases("errvalue");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "db.exec(\"create table people (name varchar(40), age int, meta text)\")\n");
        let got = ev(&mut it, "let age = int(\"4O\")\ndb.exec(\"insert into people (name, age) values (?, ?)\", \"ann\", age)");
        has(&got, "value 2 cannot be stored: got error (it failed before this call", kind);
        has(&got, "not an int: \"4O\"", kind);
        // By name, inside a list or a map, and a function.
        has(&ev(&mut it, "db.exec(\"insert into people (name, age) values (:n, :a)\", { n: \"bo\", a: int(\"x\") })"), "value :a cannot be stored: got error (it failed before this call", kind);
        has(&ev(&mut it, "db.exec(\"insert into people (meta) values (?)\", [1, err(\"no\")])"), "value 1 holds an error, which cannot be stored: got error (it failed before this call", kind);
        has(&ev(&mut it, "db.exec(\"insert into people (meta) values (?)\", { a: { b: err(\"deep\") } })"), "deep", kind);
        has(&ev(&mut it, "db.exec(\"insert into people (name) values (?)\", (x) => x)"), "value 1: a fn cannot be stored in the database", kind);
        has(&ev(&mut it, "db.query(\"select * from people where name = ?\", int(\"x\"))"), "value 1 cannot be stored: got error", kind);
        // `?` passes it on.
        has(&ev(&mut it, "db.exec(\"insert into people (name) values (?)\", err(\"mine\"))?\n\"went on\""), "cannot be stored", kind);
        assert_eq!(ev(&mut it, "db.query(\"select * from people\")"), "[]", "{}: nothing stored", kind);
        // A key the SQL does not use may hold anything; every other value is stored.
        assert_eq!(ev(&mut it, "db.exec(\"insert into people (name, age) values (:n, :a)\", { n: \"ünï\", a: 3, later: int(\"x\"), f: (x) => x }).changes"), "1", "{}", kind);
        assert_eq!(ev(&mut it, "db.exec(\"insert into people (name, age, meta) values (?, ?, ?)\", null, 2.0, [1, \"a\", null]).changes"), "1", "{}", kind);
        assert_eq!(ev(&mut it, "len(db.query(\"select * from people\"))"), "2", "{}", kind);
    }
}

// Q-747: a map of values whose keys the SQL does not use is no value where the SQL has no
// placeholder, on every database.
#[test]
fn a_map_where_the_sql_has_no_placeholder_is_no_value() {
    let (dbs, _held) = databases("unusedmap");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "db.exec(\"create table r6 (id int, author varchar(20))\")\ndb.exec(\"insert into r6 values (1, 'ann'), (2, 'bo')\")\n");
        let q = |f: &str| format!("let f = {}\nlet sql = \"select id from r6\" + (if f.author != null {{ \" where author = :author\" }} else {{ \"\" }}) + \" order by id\"\ndb.query(sql, f)", f);
        assert_eq!(ev(&mut it, &q("{ author: null }")), "[{ id: 1 }, { id: 2 }]", "{}", kind);
        assert_eq!(ev(&mut it, &q("{ author: \"bo\" }")), "[{ id: 2 }]", "{}", kind);
        assert_eq!(ev(&mut it, &q("{}")), "[{ id: 1 }, { id: 2 }]", "{}", kind);
        assert_eq!(ev(&mut it, "db.one(\"select count(*) as n from r6\", { unused: 1 }).n"), "2", "{}", kind);
        assert_eq!(ev(&mut it, "db.exec(\"update r6 set author = 'cy' where id = 1\", { x: 1 }).changes"), "1", "{}", kind);
        // A name the SQL uses still needs its value; a list is still one value.
        has(&ev(&mut it, "db.query(\"select id from r6 where author = :author\", { x: 1 })"), "no value for :author", kind);
        has(&ev(&mut it, "db.query(\"select id from r6\", [1])"), "the query has 0 parameters, got 1", kind);
    }
}

// Q-748: one map is the values by name only where the SQL names them: a map or a record as a lone
// `?`'s value is stored, on every database.
#[test]
fn a_map_for_a_lone_placeholder_is_its_value() {
    let (dbs, _held) = databases("lonemap");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "db.exec(\"create table r7 (id int, meta text)\")\ntype Tagged = { tags: list }\n");
        assert_eq!(ev(&mut it, "db.exec(\"insert into r7 (id, meta) values (1, ?)\", { tags: [\"a\"] }).changes"), "1", "{}", kind);
        assert_eq!(ev(&mut it, "db.exec(\"insert into r7 (id, meta) values (2, ?)\", Tagged({ tags: [\"é\"] })).changes"), "1", "{}", kind);
        assert_eq!(ev(&mut it, "db.exec(\"insert into r7 (id, meta) values (3, ?)\", {}).changes"), "1", "{}", kind);
        assert_eq!(
            ev(&mut it, "db.query(\"select meta from r7 order by id\") | map((r) => json.parse(r.meta))"),
            "[{ tags: [\"a\"] }, { tags: [\"é\"] }, {}]",
            "{}",
            kind
        );
        // `?1` is a number, not a name.
        assert_eq!(ev(&mut it, "db.one(\"select ?1 as a\", { k: 1 }).a | json.parse()"), "{ k: 1 }", "{}", kind);
        // By name, as before.
        assert_eq!(ev(&mut it, "db.one(\"select meta from r7 where id = :id\", { id: 1, other: 2 }).meta | json.parse()"), "{ tags: [\"a\"] }", "{}", kind);
        has(&ev(&mut it, "db.one(\"select :a + :b as s\", { a: 1 })"), "no value for :b", kind);
    }
}

// Q-746: a function's, a procedure's or a trigger's body is one statement with the rest, not a
// transaction begun or ended by hand.
#[test]
fn routine_bodies_are_not_transactions_begun_by_hand() {
    let (dbs, _held) = databases("bodies");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "db.exec(\"create table tg (id int, n int)\")\n");
        let bodies: Vec<String> = match *kind {
            "postgres" => vec![
                "create function f55_add(a int, b int) returns int language plpgsql as $$ begin return a + b; end $$".into(),
                "create function f55_sq(a int) returns int language sql begin atomic select case when a > 0 then a * a else 0 end; end".into(),
                "create procedure f55_put(a int) language sql begin atomic insert into tg (id, n) values (a, a); end".into(),
                "do $$ begin perform 1; end $$".into(),
                "create function f55_tg() returns trigger language plpgsql as $f$ begin new.n := new.id * 2; return new; end $f$; create trigger tg_bi before insert on tg for each row execute function f55_tg()".into(),
            ],
            "mariadb" => vec![
                "create function f55_add(a int, b int) returns int deterministic begin declare s int default 0; if a > 0 then set s = a; end if; return s + b; end".into(),
                "create function f55_sq(a int) returns int deterministic return case when a > 0 then a * a else 0 end".into(),
                "create procedure f55_put(a int) begin l: loop insert into tg (id, n) values (a, a); leave l; end loop l; end".into(),
                "begin not atomic select 1; end".into(),
                "create trigger tg_bi before insert on tg for each row begin case when new.id > 0 then set new.n = new.id * 2; else set new.n = 0; end case; end".into(),
            ],
            _ => vec!["create trigger tg_bi after insert on tg begin update tg set n = case when new.id > 0 then new.id * 2 end where id = new.id; end".into()],
        };
        for b in &bodies {
            assert_eq!(ev(&mut it, &format!("is_err(db.exec(\"{}\"))", b)), "false", "{}: {}", kind, b);
        }
        // Inside a transaction too.
        assert_eq!(ev(&mut it, "db.transaction(() => db.exec(\"insert into tg (id) values (5)\").changes)"), "1", "{}", kind);
        assert_eq!(ev(&mut it, "db.one(\"select n from tg where id = 5\").n"), "10", "{}", kind);
        if *kind != "sqlite" {
            assert_eq!(ev(&mut it, "db.one(\"select f55_add(2, 3) as v, f55_sq(4) as w\")"), "{ v: 5, w: 16 }", "{}", kind);
        }
        // A transaction begun or ended by hand is still refused, alone or among others.
        for sql in ["begin", "COMMIT", "start transaction", "rollback", "select 1; commit", "begin; insert into tg (id) values (1)"] {
            has(&ev(&mut it, &format!("db.exec(\"{}\")", sql)), "transaction", &format!("{} {}", kind, sql));
        }
        assert_eq!(ev(&mut it, "db.one(\"select 'begin; end' as v\").v"), "\"begin; end\"", "{}", kind);
    }
}

// Q-749: PostgreSQL's `?` is read as the server reads it: `?||` is a placeholder and `||`, `@?`
// jsonpath's operator, `?1` the first value, block comments nest.
#[test]
fn postgresql_reads_question_marks_as_the_server_does() {
    let (dbs, _held) = databases("qmarks");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "");
        // On MariaDB too, whose driver reads `||` as the others do (slice 6).
        assert_eq!(ev(&mut it, "db.one(\"select 'ab' like ?||'%' as v\", \"a\").v == true || db.one(\"select 'ab' like ?||'%' as v\", \"a\").v == 1"), "true", "{}", kind);
        assert_eq!(ev(&mut it, "db.one(\"select ?2 as a, ?1 as b, ?2 as c\", \"x\", \"ü\")"), "{ a: \"ü\", b: \"x\", c: \"ü\" }", "{}", kind);
        assert_eq!(ev(&mut it, "db.one(\"select /* a ? */ ? as v\", 9).v"), "9", "{}", kind);
        if *kind == "postgres" {
            assert_eq!(ev(&mut it, "db.one(\"select '%'||?||'%' as v\", \"b\").v"), "\"%b%\"", "{}", kind);
            assert_eq!(ev(&mut it, "db.one(\"select '{\\\"a\\\":1}'::jsonb @? '$.a' as v, ? as w\", 1)"), "{ v: true, w: 1 }", "{}", kind);
            assert_eq!(ev(&mut it, "db.one(\"select ?1::int + ?2 as v\", 3, 4).v"), "7", "{}", kind);
            assert_eq!(ev(&mut it, "db.one(\"select /* /* ? */ ? */ ? as v\", 9).v"), "9", "{}", kind);
            // jsonb's own `?`, `?|` and `?&` after a value, as before; a query numbering its own.
            assert_eq!(ev(&mut it, "db.one(\"select '{\\\"a\\\":1}'::jsonb ? 'a' as v, '{\\\"a\\\":1}'::jsonb ?| array['b', ?] as w\", \"a\")"), "{ v: true, w: true }", "{}", kind);
            assert_eq!(ev(&mut it, "db.one(\"select $1::int as v, '{\\\"a\\\":1}'::jsonb ? 'a' as w\", 5)"), "{ v: 5, w: true }", "{}", kind);
            assert_eq!(ev(&mut it, "db.one(\"select arr[1:2] as v from (select array[?, 2, 3] as arr) s\", 1).v"), "[1, 2]", "{}", kind);
        }
    }
}

// Q-708: outside a transaction, a setting made to last the session is refused in every form; inside
// one it is noted, and the connection reset before another request has it.
#[test]
fn a_lasting_setting_is_refused_outside_a_transaction_in_every_form() {
    let Some(pg) = pg_server() else { return };
    let url = pg.database("f55_session") + "?pool_size=1";
    let mut it = on_db(&url, "db.exec(\"create table t (x int)\")\n");
    let refused = "outside `db.transaction` this connection is shared by every request";
    for (sql, values) in [
        ("select set_config('app.user_id', ?, ?)", "\"42\", false"),
        ("select set_config('app.user_id', '42', 'false')", ""),
        ("values (set_config('app.user_id', '42', false))", ""),
        ("update t set x = x where set_config('app.user_id', '42', false) is not null", ""),
        ("select pg_catalog.set_config('app.user_id', :v, :l)", "{ v: \"42\", l: false }"),
        ("select 1; select set_config('app.user_id', '42', false)", ""),
        ("do $$ begin perform set_config('app.user_id', '42', false); end $$", ""),
        ("select set_config('app.user_id', ?, ?)", "\"42\", \"true\""),
        ("select set_config('app.user_id', ?, ?)", "\"42\", null"),
        ("set app.user_id = '42'", ""),
        ("select pg_advisory_lock(?)", "1"),
    ] {
        let call = if values.is_empty() { format!("db.exec(\"{}\")", sql) } else { format!("db.exec(\"{}\", {})", sql, values) };
        has(&ev(&mut it, &call), refused, sql);
        has(&ev(&mut it, &call.replace("db.exec", "db.query")), refused, sql);
    }
    assert_eq!(ev(&mut it, "db.one(\"select current_setting('app.user_id', true) as u\").u ?? \"\""), "\"\"", "nothing leaked");
    // What ends with its transaction is allowed; text that only names it is no call.
    for (sql, values) in [
        ("select set_config('app.user_id', ?, ?)", "\"42\", true"),
        ("select set_config('app.user_id', '42', TRUE)", ""),
        ("select 'set_config(''a'', ''b'', false)' as v", ""),
        ("select pg_advisory_xact_lock(?)", "1"),
        ("set local app.user_id = '42'", ""),
    ] {
        let call = if values.is_empty() { format!("db.exec(\"{}\")", sql) } else { format!("db.exec(\"{}\", {})", sql, values) };
        assert_eq!(ev(&mut it, &format!("is_err({})", call)), "false", "{}", sql);
    }
    // Inside a transaction: noted, read there, and gone after it.
    assert_eq!(
        ev(&mut it, "db.transaction(() => {\n    db.exec(\"select set_config('app.user_id', ?, ?)\", \"7\", false)\n    db.one(\"select current_setting('app.user_id', true) as u\").u\n})"),
        "\"7\""
    );
    assert_eq!(ev(&mut it, "db.transaction(() => db.one(\"select current_setting('app.user_id', true) as u\").u) ?? \"\""), "\"\"", "reset");
}

// Q-731: a value PostgreSQL would take as text, where nothing in the SQL says what it is, is sent
// as what it is: the same answers as SQLite and MariaDB.
#[test]
fn values_with_no_type_in_the_sql_give_the_same_answers_everywhere() {
    let (dbs, _held) = databases("untyped");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "db.exec(\"create table t (id int, name varchar(20), n int)\")\ndb.exec(\"insert into t values (1, '42', 7)\")\n");
        for (body, want) in [
            ("db.one(\"select max(x) as m from (select ? as x union all select ?) s\", 10, 9).m", "10"),
            ("db.one(\"select substring('hello', ?, ?) as s\", 2, 3).s", "\"ell\""),
            ("db.one(\"select ? as v\", 5).v", "5"),
            ("db.one(\"select ? as v\", -2147483649).v", "-2147483649"),
            ("db.one(\"select ? as v\", 2.5).v", "2.5"),
            ("db.one(\"select ? as v\", \"5\").v", "\"5\""),
            // An int past what the column holds is no match; at its edge, one.
            ("db.query(\"select id from t where n = ?\", 5000000000)", "[]"),
            ("db.query(\"select id from t where n = ?\", 2147483648)", "[]"),
            ("db.query(\"select id from t where n = ?\", 7)", "[{ id: 1 }]"),
            // A text column compared with an int, and an int stored in one, as before.
            ("db.query(\"select id from t where name = ?\", 42)", "[{ id: 1 }]"),
            ("db.exec(\"insert into t (id, name) values (2, ?)\", 43).changes", "1"),
            ("db.one(\"select name from t where id = 2\").name", "\"43\""),
        ] {
            assert_eq!(ev(&mut it, body), want, "{}: {}", kind, body);
            // The same in a transaction.
            if !body.contains("insert") {
                assert_eq!(ev(&mut it, &format!("db.transaction(() => {})", body)), want, "{} in a transaction: {}", kind, body);
            }
        }
        let greatest = if *kind == "sqlite" { "max" } else { "greatest" };
        assert_eq!(ev(&mut it, &format!("db.one(\"select {}(?, ?) as g\", 10, 9).g", greatest)), "10", "{}", kind);
        // Mixed: a value with no type beside one compared with a text column.
        assert_eq!(
            ev(&mut it, &format!("db.transaction(() => db.one(\"select {}(?, ?) as g, (select count(*) from t where name = ?) as c\", 10, 9, 42))", greatest)),
            "{ g: 10, c: 1 }",
            "{}",
            kind
        );
    }
}

// Q-728: on MySQL and MariaDB a statement that commits by itself is refused inside a transaction,
// so all or nothing holds there as on PostgreSQL and SQLite.
#[test]
fn a_statement_that_commits_by_itself_is_refused_inside_a_transaction() {
    let (dbs, _held) = databases("implicit");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "db.exec(\"create table rates (cur varchar(3) primary key, v int)\")\ndb.exec(\"insert into rates values ('USD', 1)\")\n");
        let r = ev(&mut it, "db.transaction(() => {\n    db.exec(\"delete from rates where cur = 'USD'\")\n    db.exec(\"truncate table rates\")\n    db.exec(\"insert into rates values (?, ?)\", \"EUR\", 3)?\n    db.exec(\"insert into rates values (?, ?)\", \"EUR\", 4)?\n})");
        if *kind == "mariadb" {
            has(&r, "`db.exec(\"truncate table rates\")` inside a transaction: MySQL and MariaDB commit the transaction around this statement by itself", kind);
        } else if *kind == "postgres" {
            has(&r, "duplicate key", kind);
        }
        // All or nothing: as it was, on each.
        assert_eq!(ev(&mut it, "db.query(\"select cur, v from rates\")"), "[{ cur: \"USD\", v: 1 }]", "{}", kind);
        if *kind != "mariadb" {
            continue;
        }
        for sql in ["create table other (a int)", "CREATE INDEX i ON rates (v)", "drop table rates", "alter table rates add w int", "rename table rates to r2", "lock tables rates write"] {
            has(&ev(&mut it, &format!("db.transaction(() => db.transaction(() => db.exec(\"{}\")))", sql)), "commit the transaction around this statement", sql);
            has(&ev(&mut it, &format!("db.transaction(() => db.query(\"{}\"))", sql)), "commit the transaction around this statement", sql);
        }
        has(&ev(&mut it, "db.exec_many(\"create table x (a int)\", [[]])"), "commit the transaction around this statement", "exec_many");
        // What does not commit, inside; and anything outside.
        assert_eq!(
            ev(&mut it, "db.transaction(() => {\n    db.exec(\"create temporary table tt (a int)\")\n    db.exec(\"insert into tt values (1)\")\n    db.exec(\"drop temporary table tt\")\n    db.one(\"select 'truncate table rates' as v\").v\n})"),
            "\"truncate table rates\""
        );
        assert_eq!(ev(&mut it, "db.exec(\"truncate table rates\").changes"), "0");
        assert_eq!(ev(&mut it, "db.query(\"select * from rates\")"), "[]");
    }
}

// Q-738: `db.transaction` refuses an option it does not have, naming those it has; `quillo check`
// says so before it runs.
#[test]
fn a_transactions_options_are_only_its_own() {
    let mut it = quillo::Interp::new(&support::temp_dir("feedback55-txopts"));
    it.load_source("db.open(\":memory:\")\n", "app.qwe").unwrap();
    for opts in ["{ isolaton: \"serializable\" }", "{ retrys: 5 }", "{ isolation: \"serializable\", Retries: 1 }", "{ \"\": 1 }"] {
        has(&ev(&mut it, &format!("db.transaction({}, () => 1)", opts)), "has no option", opts);
        has(&ev(&mut it, &format!("db.transaction({}, () => 1)", opts)), "(isolation, retries)", opts);
    }
    for opts in ["{}", "{ isolation: null }", "{ retries: 0 }", "{ isolation: \"read_committed\", retries: 20 }"] {
        assert_eq!(ev(&mut it, &format!("db.transaction({}, () => 1)", opts)), "1", "{}", opts);
    }
    // `idempotent` takes its own as before.
    let (ok, out) = check("route POST /pay = idempotent({ max_age: 60, retries: 1, isolation: \"serializable\" }, () => \"paid\")\n");
    assert!(ok, "{}", out);
    let (ok, out) = check("db.open(\":memory:\")\nlet a = db.transaction({ isolaton: \"serializable\", retrys: 5 }, () => 1)\nlet b = db.transaction({ isolation: \"serializable\", retries: 2 }, () => 1)\nlet c = db.transaction({ timeout: 1 }, () => 1)\n");
    assert!(!ok, "{}", out);
    has(&out, "`db.transaction` has no option `isolaton` (did you mean `isolation`?)", "check");
    has(&out, "`db.transaction` has no option `retrys` (did you mean `retries`?)", "check");
    has(&out, "`db.transaction` has no option `timeout` (isolation, retries)", "check");
    assert_eq!(out.matches("has no option").count(), 3, "{}", out);
}

// Q-755 and Q-746 in `quillo check`: a list for PostgreSQL's array is no warning, and a function's
// body is no `begin`.
#[test]
fn check_reads_sql_as_the_databases_do() {
    for (call, warned) in [
        ("db.query(\"select * from t where x = any(?)\", [1, 2])", false),
        ("db.query(\"select * from t where x <> ALL (?)\", [1, 2])", false),
        ("db.exec(\"insert into t values (?::jsonb[])\", [1, 2])", false),
        ("db.exec(\"insert into t values (?::character varying(10)[])\", [\"a\"])", false),
        ("db.one(\"select cast(? as int[]) as v\", [1])", false),
        ("db.query(\"select * from unnest(?)\", [1])", false),
        ("db.exec(\"insert into t values (?)\", [1, 2])", true),
        ("db.exec(\"insert into t values (?::int)\", [1, 2])", true),
        ("db.query(\"select * from t where x = ? and y = any(?)\", [1])", true),
        ("db.exec(\"insert into t values ('?')\", [1])", false),
    ] {
        let (_, out) = check(&format!("db.open(\":memory:\")\nlet r = {}\n", call));
        assert_eq!(out.contains("this list is one parameter"), warned, "{}: {}", call, out);
    }
    for (call, refused) in [
        ("db.exec(\"create function f(a int) returns int language plpgsql as $$ begin return a; end $$\")", false),
        ("db.exec(\"create trigger t after insert on x begin update y set n = 1; end\")", false),
        ("db.exec(\"do $$ begin perform 1; end $$\")", false),
        ("db.exec(\"begin\")", true),
        ("db.query(\"Commit\")", true),
        ("db.one(\"rollback to savepoint a\")", true),
    ] {
        let (_, out) = check(&format!("db.open(\":memory:\")\nlet r = {}\n", call));
        assert_eq!(out.contains("write `db.transaction(() => ...)`"), refused, "{}: {}", call, out);
    }
}

// ---- slice 5's review (FEEDBACK55.md, Q-708, Q-728, Q-729, Q-731, Q-746..Q-749; branch feedback55)

// Q-708: what may change the session where Quillo cannot read it (a function of the program's,
// a `do` block) leaks to no one: outside a transaction it runs on a connection reset after it,
// and every transaction's connection is reset as it ends; the names PostgreSQL reads as
// `set_config`, and temporary tables by their schema, are refused outside one.
#[test]
fn no_setting_reaches_the_next_request_by_any_way() {
    let Some(pg) = pg_server() else { return };
    let url = pg.database("f55_unseen") + "?pool_size=1";
    let mut it = on_db(&url, "db.exec(\"create table t (a int)\")\ndb.exec(\"create function setu(v text) returns text language sql as $$ select set_config('app.user_id', v, false) $$\")\nfn who() = db.one(\"select current_setting('app.user_id', true) as u\").u ?? \"\"\n");
    let refused = "outside `db.transaction` this connection is shared by every request";
    for sql in [
        "select U&\\\"set\\\\005fconfig\\\"('app.user_id', 'u1', false)",
        "do $$ begin execute 'select set_config(''app.user_id'', ''u2'', false)'; end $$",
        "create table pg_temp.x (a int)",
        "select 1 into pg_temp.y",
    ] {
        has(&ev(&mut it, &format!("db.exec(\"{}\")", sql)), refused, sql);
    }
    // Run, and gone after: a function of the program's, outside a transaction and inside one,
    // nested too; a `do` block whose SQL is made as it runs.
    assert_eq!(ev(&mut it, "db.one(\"select setu('u3') as s\").s"), "\"u3\"");
    assert_eq!(ev(&mut it, "who()"), "\"\"", "outside");
    assert_eq!(ev(&mut it, "db.transaction(() => db.transaction(() => db.one(\"select setu(?) as s\", \"u16\").s))"), "\"u16\"");
    assert_eq!(ev(&mut it, "who()"), "\"\"", "inside");
    assert_eq!(ev(&mut it, "db.transaction(() => db.exec(\"create table pg_temp.z (a int)\").changes)"), "0");
    assert_eq!(ev(&mut it, "db.one(\"select count(*) as n from pg_tables where tablename = 'z'\").n"), "0");
    assert_eq!(ev(&mut it, "is_err(db.exec(\"do $$ begin execute 'select setu(' || quote_literal('u5') || ')'; end $$\"))"), "false");
    assert_eq!(ev(&mut it, "who()"), "\"\"", "do");
    // A function made later is known to be the program's at once.
    assert_eq!(ev(&mut it, "db.one(\"select lower(?) as l\", \"X\").l"), "\"x\"");
    assert_eq!(ev(&mut it, "is_err(db.exec(\"create function lower2(v text) returns text language sql as $$ select set_config('app.user_id', v, false) $$\"))"), "false");
    assert_eq!(ev(&mut it, "db.one(\"select lower2('u6') as l\").l"), "\"u6\"");
    assert_eq!(ev(&mut it, "who()"), "\"\"", "made later");
    // What is no function is as before: quoted names as PostgreSQL reads them.
    assert_eq!(ev(&mut it, "is_err(db.one(\"select 'set_config(x, y, false)' as v, \\\"t\\\".a from t\"))"), "false");
}

// Q-731: a statement PostgreSQL does not take with the values' types is tried at most twice,
// in a transaction too, whatever the number of values; the answers stay right.
#[test]
fn values_with_no_type_are_tried_a_bounded_number_of_times() {
    let Some(pg) = pg_server() else { return };
    let url = pg.database("f55_tries");
    let mut it = on_db(&url, "db.exec(\"create table tt (id int, name text)\")\ndb.exec(\"insert into tt values (1, '42')\")\n");
    let refusals = |pg: &support::Pg| pg.log().matches("operator does not exist: text = integer").count();
    for n in [1, 2, 200, 800] {
        let before = refusals(&pg);
        let got = ev(&mut it, &format!("let vals = reduce(range({n}), {{}}, (m, i) => set(m, \"v\" + str(i), i + 42))\ndb.transaction(() => db.query(\"select id from tt where name in (\" + join(range({n}) | map((i) => \":v\" + str(i)), \", \") + \")\", vals))", n = n));
        assert_eq!(got, "[{ id: 1 }]", "{} values", n);
        let tries = refusals(&pg) - before;
        assert!(tries <= 2, "{} values: {} refused tries", n, tries);
    }
    // Two values PostgreSQL takes as ints beside one compared with text: both answers right.
    assert_eq!(ev(&mut it, "db.transaction(() => db.one(\"select greatest(?, ?) as g, (select count(*) from tt where name = ?) as c\", 10, 9, 42))"), "{ g: 10, c: 1 }");
    assert_eq!(ev(&mut it, "db.one(\"select greatest(?, ?) as g\", 10, \"9\").g"), "\"9\"");
}

// Q-749 (review): a name inside PostgreSQL's brackets is a placeholder, unless a value comes
// before its `:` (a slice); SQLite's variables may be digits, and `#name`.
#[test]
fn names_in_brackets_and_sqlites_numbered_names() {
    let (dbs, _held) = databases("brackets");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "db.exec(\"create table t (a int, b int)\")\n");
        if *kind == "postgres" {
            assert_eq!(ev(&mut it, "db.one(\"select array[:a, :b] as v\", { a: 1, b: 2 }).v"), "[1, 2]", "{}", kind);
            assert_eq!(ev(&mut it, "db.one(\"select 'b' = any(array[:a, :b]) as v, 2 = any(array[:c, :d]::int[]) as w\", { a: \"a\", b: \"b\", c: 1, d: 2 })"), "{ v: true, w: true }", "{}", kind);
            assert_eq!(ev(&mut it, "db.one(\"select (array[10, 20, 30])[:lo:2] as v\", { lo: 1 }).v"), "[10, 20]", "{}", kind);
            assert_eq!(ev(&mut it, "db.one(\"select (array[10, 20, 30])[2:3] as v, x[1:1] as w from (select array[5] as x) s\").v"), "[20, 30]", "{}", kind);
        }
        if *kind == "sqlite" {
            has(&ev(&mut it, "db.exec(\"insert into t values (:1, 2)\")"), "the query has 1 parameter, got 0", kind);
            has(&ev(&mut it, "db.exec(\"insert into t values (@1, #b)\")"), "the query has 2 parameters, got 0", kind);
            assert_eq!(ev(&mut it, "db.one(\"select :1 as x, @2 as y, #a as z, $3 as w\", { \"1\": 9, \"2\": 8, a: 7, \"3\": 6 })"), "{ x: 9, y: 8, z: 7, w: 6 }", "{}", kind);
            assert_eq!(ev(&mut it, "db.query(\"select * from t\")"), "[]", "nothing stored");
        }
        // A map of values not bound is not looked at (found in review).
        assert_eq!(ev(&mut it, "db.one(\"select 1 as x\", { cb: (x) => x, e: err(\"no\") }).x"), "1", "{}", kind);
    }
}

// Q-728 (review): on MariaDB, a statement that commits where it cannot be read beforehand (a
// procedure's, `execute immediate`, a `begin not atomic` block) ends the function with the error
// naming it, by the server's own flag; MariaDB's `analyze select`, `for` loops and `/*M! */` are
// read right; migrations take schema changes as before.
#[test]
fn a_hidden_commit_on_mariadb_ends_the_transaction_with_an_error() {
    let Some(my) = maria_server() else { return };
    let url = my.url("f55_hidden");
    quillo::my::Pool::open_as(&url, true).unwrap();
    let mut it = on_db(&url, "db.exec(\"create table if not exists r (a int)\")\ndb.exec(\"create procedure mk() begin create table if not exists made (a int); end\")\n");
    for sql in ["call mk()", "begin not atomic create table if not exists m2 (a int); end", "execute immediate 'create table if not exists m3 (a int)'"] {
        let got = ev(&mut it, &format!("db.exec(\"delete from r\")\ndb.transaction(() => {{\n    db.exec(\"insert into r values (1)\")\n    db.exec(\"{}\")\n    db.exec(\"insert into r values (2)\")\n}})", sql));
        has(&got, &format!("`db.exec(\"{}\")` inside a transaction: MySQL and MariaDB committed the transaction around this statement by itself", sql), sql);
        // What ran after it did not run.
        assert_eq!(ev(&mut it, "db.query(\"select a from r order by a\")"), "[{ a: 1 }]", "{}", sql);
    }
    // Not commits: inside a transaction as before.
    for sql in ["analyze select 1", "insert into r values (9)", "select /*M!100100 1 + */ 1 as v"] {
        assert_eq!(ev(&mut it, &format!("is_err(db.transaction(() => db.query(\"{}\")))", sql)), "false", "{}", sql);
    }
    assert_eq!(ev(&mut it, "db.one(\"select /*M!100100 1 + */ 1 as v\").v"), "2");
    assert_eq!(ev(&mut it, "is_err(db.exec(\"create procedure f55_for() begin for i in 1..3 do insert into r values (i); end for; end\"))"), "false");
    assert_eq!(ev(&mut it, "db.exec(\"delete from r\")\nlet _r = db.transaction(() => db.exec(\"call f55_for()\"))\ndb.query(\"select a from r order by a\")"), "[{ a: 1 }, { a: 2 }, { a: 3 }]");
    // Migrations change the schema as before: MySQL commits them as they run.
    let d = support::temp_dir("feedback55-hidden-migrate");
    std::fs::create_dir_all(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_a.sql"), "create table mig_a (a int);\ninsert into mig_a values (1);\n").unwrap();
    std::fs::write(d.join("migrations/002_b.sql"), "alter table mig_a add b int;\n").unwrap();
    let mut m = quillo::Interp::new(&d);
    m.load_source(&format!("db.open(\"{}\")\n", url), "app.qwe").unwrap();
    assert_eq!(ev(&mut m, "let r = db.migrate()\nis_err(r) ? r : db.query(\"select a, b from mig_a\")"), "[{ a: 1, b: null }]");
}

// ---- slice 6: each driver gives the program the same values on every database (Q-730,
// Q-732..Q-737, Q-739..Q-745, Q-750..Q-754, Q-756) ----------------------------------------------

/// A fresh PostgreSQL database for `name`, and its server, held while the test runs.
fn pg_db(name: &str) -> Option<(Arc<support::Pg>, String)> {
    let Some(pg) = pg_server() else {
        eprintln!("skipping PostgreSQL: initdb and pg_ctl were not found");
        return None;
    };
    let url = pg.database(&format!("f55_{}", name));
    Some((pg, url))
}

/// The same on MariaDB (made: `db.open` makes none).
fn maria_db(name: &str) -> Option<(Arc<support::Maria>, String)> {
    let Some(my) = maria_server() else {
        eprintln!("skipping MariaDB: mariadbd and mariadb-install-db were not found");
        return None;
    };
    let url = my.url(&format!("f55_{}", name));
    Some((my, url))
}

/// `sql` run by psql on `url`, each statement to its end.
fn psql(pg: &support::Pg, url: &str, sql: &[&str]) {
    let mut c = Command::new(pg.bin.join("psql"));
    c.args([url, "-q", "-v", "ON_ERROR_STOP=1"]);
    for s in sql {
        c.args(["-c", s]);
    }
    let out = c.output().unwrap();
    assert!(out.status.success(), "{:?}: {}", sql, String::from_utf8_lossy(&out.stderr));
}

// Q-730: a `bit(n)` value keeps its digits: the cast Quillo adds is to `varbit`, not the bare
// `bit` that is `bit(1)`.
#[test]
fn a_bit_value_keeps_its_length_on_postgresql() {
    let Some((_pg, url)) = pg_db("bits") else { return };
    let mut it = on_db(&url, "db.exec(\"create table perms (id int, flags bit(8), one bit(1), v varbit(4), many bit(2)[])\")\n");
    assert_eq!(ev(&mut it, r#"db.exec("insert into perms (id, flags) values (1, ?::bit(8))", "00000101").changes"#), "1");
    assert_eq!(ev(&mut it, r#"db.exec("insert into perms (id, flags, one, v) values (2, ?, ?, ?)", "11111111", "1", "101").changes"#), "1");
    assert_eq!(ev(&mut it, r#"db.query("select flags, one, v from perms where id < 3 order by id")"#), r#"[{ flags: "00000101", one: null, v: null }, { flags: "11111111", one: "1", v: "101" }]"#);
    assert_eq!(ev(&mut it, r#"db.one("select ?::bit(4) as v", "1010").v"#), r#""1010""#);
    assert_eq!(ev(&mut it, r#"db.one("select ?::bit(1) as v", "1").v"#), r#""1""#);
    // An array of them.
    assert_eq!(ev(&mut it, r#"db.exec("insert into perms (id, many) values (3, ?)", ["01", "10"]).changes"#), "1");
    assert_eq!(ev(&mut it, r#"db.one("select many from perms where id = 3").many"#), r#"["01", "10"]"#);
    // Too many digits, too few, and one that is no digit: as PostgreSQL says.
    has(&ev(&mut it, r#"db.exec("insert into perms (id, flags) values (4, ?)", "000001011")"#), "bit string length 9 does not match type bit(8)", "9 digits");
    has(&ev(&mut it, r#"db.exec("insert into perms (id, flags) values (4, ?)", "0101")"#), "bit string length 4 does not match type bit(8)", "4 digits");
    has(&ev(&mut it, r#"db.exec("insert into perms (id, flags) values (4, ?)", "0102")"#), "is not a valid binary digit", "a 2");
    has(&ev(&mut it, r#"db.exec("insert into perms (id, v) values (4, ?)", "10101")"#), "bit string too long for type bit varying(4)", "varbit");
}

// Q-732: a whole number past an int in json and jsonb is a `dec`, so two of them are two; a value
// serde_json could not read fails only its own cell.
#[test]
fn json_numbers_past_an_int_are_decs_and_a_bad_value_fails_only_its_cell() {
    let Some((_pg, url)) = pg_db("jsonnum") else { return };
    let mut it = on_db(&url, "db.exec(\"create table r4 (id int, p jsonb, q json)\")\n");
    ev(&mut it, r#"db.exec("insert into r4 (id, p) values (1, '{\"n\": 18446744073709551615}'), (2, '{\"n\": 18446744073709551614}'), (3, '{\"n\": 9223372036854775807}'), (4, '{\"n\": 9223372036854775808}'), (5, '{\"n\": -9223372036854775808}'), (6, '{\"n\": -9223372036854775809}'), (7, '{\"n\": 0.1, \"é\": [true, null, \"ü\"]}')")"#);
    assert_eq!(
        ev(&mut it, r#"db.query("select p from r4 order by id") | map((r) => r.p.n)"#),
        "[18446744073709551615m, 18446744073709551614m, 9223372036854775807, 9223372036854775808m, -9223372036854775808, -9223372036854775809m, 0.1]"
    );
    assert_eq!(ev(&mut it, r#"let a = db.query("select p from r4 where id < 3 order by id")
a[0].p.n == a[1].p.n"#), "false");
    assert_eq!(ev(&mut it, r#"db.one("select p from r4 where id = 7").p"#), r#"{ n: 0.1, é: [true, null, "ü"] }"#);
    // Past a dec and a float, a lone surrogate, deep nesting: each its own row, read with the rest.
    ev(&mut it, r#"db.exec("insert into r4 (id, p, q) values (8, '{\"n\": 1e400}', '{\"n\": 1e400}'), (9, null, '\"\\ud800x\"'), (10, null, '1e-400'), (11, (repeat('[', 1000) || '1' || repeat(']', 1000))::jsonb, null), (12, (repeat('[', 1001) || '1' || repeat(']', 1001))::jsonb, null), (13, '[]', '{}')")"#);
    let rows = ev(&mut it, r#"db.query("select id, p, q from r4 where id >= 8 order by id") | map((r) => [r.id, is_err(r.p) ? str(r.p.error) : type_of(r.p), is_err(r.q) ? str(r.q.error) : r.q])"#);
    has(&rows, r#"[8, "db: this JSON could not be read: the number 1000000000000000... (401 characters) is past what a float holds", "db: this JSON could not be read: the number 1e400 is past what a float holds"]"#, "past a float");
    has(&rows, "[9, \"null\", \"\u{fffd}x\"]", "a lone surrogate");
    has(&rows, r#"[10, "null", "db: this JSON could not be read: the number 1e-400 is past what a float holds"]"#, "below a float");
    has(&rows, r#"[11, "list", null]"#, "1,000 deep");
    has(&rows, r#"[12, "db: this JSON could not be read: it nests more than 1000 deep", null]"#, "1,001 deep");
    has(&rows, r#"[13, "list", {}]"#, "empty");
    assert_eq!(ev(&mut it, "len(db.query(\"select p from r4\"))"), "13");
    // Past a dec, a float, as jsonb writes a float's whole number in digits: the program's own
    // floats come back (found in review).
    ev(&mut it, r#"db.exec("insert into r4 (id, p) values (20, ?)", { f: 1.5e30, big: 1e300, top: 1.7976931348623157e308, small: 0.5 })"#);
    assert_eq!(ev(&mut it, r#"let p = db.one("select p from r4 where id = 20").p
[p.f == 1.5e30, p.big == 1e300, p.top == 1.7976931348623157e308, p.small]"#), "[true, true, true, 0.5]");
    assert_eq!(ev(&mut it, r#"let r = db.one("select '1e30'::jsonb as a, '79228162514264337593543950335'::jsonb as b, '79228162514264337593543950336'::jsonb as c")
[r.a == 1e30, r.b, r.c == 79228162514264337593543950336.0]"#), "[true, 79228162514264337593543950335m, true]");
}

// Q-733: JSON text given for a json column is the JSON it holds, as MySQL's JSON takes it, not a
// JSON string; a map is as before.
#[test]
fn json_text_given_for_a_json_column_is_stored_as_json() {
    let (dbs, _held) = databases("jsontext");
    for (kind, url) in &dbs {
        let col = if *kind == "postgres" { "jsonb" } else { "json" };
        let mut it = on_db(url, &format!("db.exec(\"create table items (id int, doc {})\")\n", col));
        let back = |it: &mut quillo::Interp, id: i32| ev(it, &format!("let d = db.one(\"select doc from items where id = {}\").doc\ntype_of(d) == \"str\" ? json.parse(d) : d", id));
        assert_eq!(ev(&mut it, r#"db.exec("insert into items (id, doc) values (1, ?)", json.stringify({ sku: "a1", n: [1, 2] })).changes"#), "1", "{}", kind);
        // jsonb keeps its keys in an order of its own.
        assert_eq!(ev(&mut it, "let d = db.one(\"select doc from items where id = 1\").doc\nlet m = type_of(d) == \"str\" ? json.parse(d) : d\n[m.sku, m.n]"), r#"["a1", [1, 2]]"#, "{}", kind);
        assert_eq!(ev(&mut it, r#"db.exec("insert into items (id, doc) values (2, ?)", { sku: "b2" }).changes"#), "1", "{}", kind);
        assert_eq!(back(&mut it, 2), r#"{ sku: "b2" }"#, "{}", kind);
        assert_eq!(ev(&mut it, r#"db.exec("insert into items (id, doc) values (3, ?)", "[1, \"é\"]").changes"#), "1", "{}", kind);
        assert_eq!(back(&mut it, 3), r#"[1, "é"]"#, "{}", kind);
        assert_eq!(ev(&mut it, r#"db.exec("insert into items (id, doc) values (4, ?)", "\"just text\"").changes"#), "1", "{}", kind);
        // PostgreSQL's jsonb gives the string; the others the JSON text, quotes and all.
        let text = if *kind == "postgres" { r#""just text""# } else { r#"""just text"""# };
        assert_eq!(ev(&mut it, "db.one(\"select doc from items where id = 4\").doc"), text, "{}: a JSON string", kind);
        if *kind == "postgres" {
            assert_eq!(ev(&mut it, r#"db.exec("insert into items (id, doc) values (5, ?::jsonb)", json.stringify({ sku: "c3" })).changes"#), "1");
            assert_eq!(ev(&mut it, r#"db.query("select jsonb_typeof(doc) as t, doc->>'sku' as sku from items where id in (1, 4, 5) order by id")"#), r#"[{ t: "object", sku: "a1" }, { t: "string", sku: null }, { t: "object", sku: "c3" }]"#);
            assert_eq!(ev(&mut it, r#"db.one("select count(*) as n from items where doc @> ?", { sku: "c3" }).n"#), "1");
            // Not a column's value: a JSON string, as MySQL's functions take one (found in review).
            assert_eq!(ev(&mut it, r#"db.one("select ?::json as j", "{}").j"#), r#""{}""#);
            assert_eq!(ev(&mut it, r#"db.one("select count(*) as n from items where doc @> ?", "{\"sku\": \"c3\"}").n"#), "0");
        }
        // Text that is no JSON: refused where the column checks it.
        if *kind != "sqlite" {
            assert_eq!(ev(&mut it, r#"is_err(db.exec("insert into items (id, doc) values (6, ?)", "not json"))"#), "true", "{}", kind);
        }
    }
}

// Q-733, found in review: a str is JSON text only as a json column's value (an insert's values,
// `set col = ?`), as MySQL's JSON column takes it; given to a function it is a JSON string, as
// MySQL's `json_set` takes it.
#[test]
fn a_str_is_json_text_only_as_a_json_columns_value() {
    let (dbs, _held) = databases("jsonargs");
    for (kind, url) in dbs.iter().filter(|(k, _)| *k != "sqlite") {
        let (col, set) = if *kind == "postgres" { ("jsonb", "jsonb_set('{}', '{a}', ?)") } else { ("json", "json_set('{}', '$.a', ?)") };
        let mut it = on_db(url, &format!("db.exec(\"create table ja (id int, j {})\")\n", col));
        let read = |it: &mut quillo::Interp, sql: &str| ev(it, &format!("let v = db.one(\"{}\", \"123\", \"Bob\")\n[v.a, v.b] | map((x) => type_of(x) == \"str\" ? json.parse(x) : x)", sql));
        assert_eq!(read(&mut it, &format!("select {} as a, {} as b", set, set)), r#"[{ a: "123" }, { a: "Bob" }]"#, "{}", kind);
        // A column's value, by insert and by update, a cast after it or not: the JSON it holds.
        assert_eq!(ev(&mut it, "db.exec(\"insert into ja (id, j) values (1, ?), (2, ?)\", \"123\", \"[\\\"é\\\"]\").changes"), "2", "{}", kind);
        assert_eq!(ev(&mut it, "db.exec(\"update ja set j = ?, id = id where id = 2\", \"{\\\"b\\\": true}\").changes"), "1", "{}", kind);
        let back = ev(&mut it, "db.query(\"select j from ja order by id\") | map((r) => type_of(r.j) == \"str\" ? json.parse(r.j) : r.j)");
        assert_eq!(back, "[123, { b: true }]", "{}", kind);
        // A function's argument inside a value is a JSON string there too.
        assert_eq!(ev(&mut it, &format!("db.exec(\"update ja set j = {} where id = 1\", \"Bob\").changes", set)), "1", "{}", kind);
        assert_eq!(ev(&mut it, "let j = db.one(\"select j from ja where id = 1\").j\ntype_of(j) == \"str\" ? json.parse(j) : j"), r#"{ a: "Bob" }"#, "{}", kind);
        if *kind == "postgres" {
            assert_eq!(ev(&mut it, "db.exec(\"insert into ja (id, j) values (3, ?::jsonb) on conflict do nothing\", \"[1]\").changes"), "1");
            assert_eq!(ev(&mut it, "db.one(\"select jsonb_typeof(j) as t from ja where id = 3\").t"), r#""array""#);
        }
    }
}

// Found in review: a whole `dec` inside a JSON value is written as a number, so a number past an
// int read from a JSON column (a dec) goes back as the number it was; a dec with a fraction stays
// a string, as a JSON number with one reads as a float.
#[test]
fn a_whole_dec_inside_json_is_a_number() {
    let (dbs, _held) = databases("jsondec");
    for (kind, url) in &dbs {
        let col = if *kind == "postgres" { "jsonb" } else { "json" };
        let mut it = on_db(url, &format!("db.exec(\"create table jd (id int, j {})\")\n", col));
        assert_eq!(ev(&mut it, "db.exec(\"insert into jd values (1, ?)\", { d: 18446744073709551615m, m: 12.50m, n: [5m, -7m] }).changes"), "1", "{}", kind);
        let j = ev(&mut it, "let j = db.one(\"select j from jd\").j\nlet m = type_of(j) == \"str\" ? json.parse(j) : j\n[m.d, m.m, m.n]");
        if *kind == "postgres" {
            assert_eq!(j, r#"[18446744073709551615m, "12.50", [5, -7]]"#);
            // Written back, it finds its row.
            assert_eq!(ev(&mut it, "let j = db.one(\"select j from jd\").j\ndb.one(\"select count(*) as n from jd where j = ?\", j).n"), "1");
        } else {
            // json.parse reads a number past an int as a float; the text holds the number.
            assert_eq!(ev(&mut it, "let j = db.one(\"select j from jd\").j\ntype_of(j) == \"str\" ? (j | contains(\"18446744073709551615\")) : j"), "true", "{}", kind);
        }
    }
}

// Q-734: a float past `real`'s or `double precision`'s range is refused, as PostgreSQL refuses it
// in text, where it was stored as Infinity or 0.
#[test]
fn a_float_past_its_columns_range_is_refused_on_postgresql() {
    let Some((_pg, url)) = pg_db("floatrange") else { return };
    let mut it = on_db(&url, "db.exec(\"create table r2 (id int, r real, d float8)\")\n");
    for v in ["1e39", "-1e39", "1e-46", "-1e-46", "\"1e39\"", "\"1e-46\""] {
        has(&ev(&mut it, &format!("db.exec(\"insert into r2 (id, r) values (1, ?)\", {})", v)), "is out of range for type real", v);
    }
    for v in ["\"1e400\"", "\"-1e400\"", "\"1e-400\""] {
        has(&ev(&mut it, &format!("db.exec(\"insert into r2 (id, d) values (1, ?)\", {})", v)), "is out of range for type double precision", v);
    }
    has(&ev(&mut it, "db.exec(\"insert into r2 (id, d) values (1, ?)\", \"NaN\")"), "cannot be stored", "NaN as text");
    assert_eq!(ev(&mut it, "len(db.query(\"select * from r2\"))"), "0");
    // The limits, and the forms that go as before.
    for (i, v) in ["3.4e38", "-3.4e38", "1.4e-45", "0.0", "-0.0", "1", "\"2.5\"", "\"0e-400\"", "2.5m"].iter().enumerate() {
        assert_eq!(ev(&mut it, &format!("db.exec(\"insert into r2 (id, r) values ({}, ?)\", {}).changes", i, v)), "1", "{}", v);
    }
    for (i, v) in ["\"1e308\"", "\"4.9e-324\"", "1.7976931348623157e308"].iter().enumerate() {
        assert_eq!(ev(&mut it, &format!("db.exec(\"insert into r2 (id, d) values ({}, ?)\", {}).changes", 20 + i, v)), "1", "{}", v);
    }
    assert_eq!(ev(&mut it, "[db.one(\"select r from r2 where id = 0\").r == 3.4e38, db.one(\"select r from r2 where id = 2\").r > 0.0, db.one(\"select r from r2 where id = 6\").r]"), "[true, true, 2.5]");
}

// Q-735: NaN and the infinities are refused, naming them, on every database (SQLite stored NaN
// as null, MariaDB refused both as "out of range", PostgreSQL kept them); -0.0 is 0.0 on every
// one (SQLite and MariaDB cannot keep it).
#[test]
fn nan_and_the_infinities_are_refused_and_minus_zero_is_zero_on_every_database() {
    let (dbs, _held) = databases("nan");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "db.exec(\"create table m (id int, y double precision not null)\")\n");
        for (v, name) in [("sqrt(-1.0)", "NaN"), ("1.0 / 0.0", "inf"), ("-1.0 / 0.0", "-inf")] {
            has(&ev(&mut it, &format!("db.exec(\"insert into m (id, y) values (1, ?)\", {})", v)), &format!("value 1 is {}, which cannot be stored", name), kind);
        }
        has(&ev(&mut it, "db.exec(\"insert into m (id, y) values (:id, :y)\", { id: 1, y: sqrt(-1.0) })"), "value :y is NaN, which cannot be stored", kind);
        has(&ev(&mut it, "db.exec(\"insert into m (id, y) values (1, ?)\", [1.5, 1.0 / 0.0])"), "value 1 holds inf, which cannot be stored", kind);
        has(&ev(&mut it, "db.query(\"select * from m where y = ?\", sqrt(-1.0))"), "value 1 is NaN", kind);
        assert_eq!(ev(&mut it, "len(db.query(\"select * from m\"))"), "0", "{}: nothing stored", kind);
        // -0.0 is stored as 0.0; the largest and the smallest floats as they are.
        assert_eq!(ev(&mut it, "db.exec(\"insert into m (id, y) values (2, ?)\", -0.0).changes"), "1", "{}", kind);
        assert_eq!(ev(&mut it, "let y = db.one(\"select y from m where id = 2\").y\n[y, 1.0 / y > 0.0]"), "[0.0, true]", "{}", kind);
        for (i, v) in ["1.7976931348623157e308", "-1.7976931348623157e308", "2.2250738585072014e-308", "0.0", "2.5"].iter().enumerate() {
            assert_eq!(ev(&mut it, &format!("db.exec(\"insert into m (id, y) values ({}, ?)\", {}).changes", 10 + i, v)), "1", "{} {}", kind, v);
        }
        assert_eq!(ev(&mut it, "db.one(\"select y from m where id = 10\").y == 1.7976931348623157e308"), "true", "{}", kind);
    }
}

// Q-736: on MySQL, `db.open` of a database that is not there is an error, as on PostgreSQL (the
// user's decision): a typo in a URL made a new, empty database. A tenant's is made by its driver.
#[test]
fn a_mysql_database_that_is_not_there_is_not_made_by_opening_it() {
    let Some(my) = maria_server() else { return };
    let url = my.url_of("f55_typo");
    let d = support::temp_dir("feedback55-typo");
    let mut it = quillo::Interp::new(&d);
    it.load_source("let first = 1\n", "app.qwe").unwrap();
    has(&ev(&mut it, &format!("let r = db.open(\"{}\")\nis_err(r) ? str(r.error) : \"opened\"", url)), "Unknown database 'f55_typo'", "db.open");
    has(&ev(&mut it, &format!("let c = db.connect(\"{}\")\nis_err(c) ? str(c.error) : \"opened\"", url)), "Unknown database 'f55_typo'", "db.connect");
    assert!(quillo::my::Pool::open_as(&url, false).is_err(), "it was made");
    // One that is there opens, as before; and no database named in the URL is none.
    let there = my.url("f55_there");
    assert_eq!(ev(&mut it, &format!("let r = db.open(\"{}\")\ndb.one(\"select database() as d\").d", there)), "\"f55_there\"");
    let mut it = on_db(&my.url(""), "");
    assert_eq!(ev(&mut it, "db.one(\"select database() as d\").d"), "null");
    // A tenant's database is made by the driver when the program names the tenant; asking
    // whether one is there makes none.
    let mut it = on_db(&my.url_of("f55t_{tenant}"), "");
    assert_eq!(ev(&mut it, "db.tenant_exists(\"ghost\")"), "false");
    assert!(quillo::my::Pool::open_as(&my.url_of("f55t_ghost"), false).is_err());
    assert_eq!(ev(&mut it, "new_tenant(\"acme\", () => db.exec(\"create table a (x int)\").changes)"), "0");
    assert!(quillo::my::Pool::open_as(&my.url_of("f55t_acme"), false).is_ok());
    assert_eq!(ev(&mut it, "with_tenant(\"beta\", () => db.one(\"select database() as d\").d)"), "\"f55t_beta\"");
    assert_eq!(ev(&mut it, "db.tenant_exists(\"beta\")"), "true");
}

// Q-737: a URL's `options` are kept with its `schema`, which replaced them.
#[test]
fn a_urls_options_are_kept_with_its_schema() {
    let Some((_pg, url)) = pg_db("options") else { return };
    for (extra, timeout, schema) in [
        ("options=-c%20statement_timeout%3D100&schema=f55s", "100ms", "f55s"),
        ("schema=f55s&options=-c%20statement_timeout%3D200", "200ms", "f55s"),
        ("options=-c%20statement_timeout%3D300", "300ms", "public"),
        ("schema=f55t", "0", "f55t"),
        ("options=&schema=f55u", "0", "f55u"),
    ] {
        let mut it = on_db(&format!("{}?{}", url, extra), "");
        assert_eq!(ev(&mut it, "db.one(\"show statement_timeout\").statement_timeout"), format!("\"{}\"", timeout), "{}", extra);
        assert_eq!(ev(&mut it, "db.one(\"select current_schema() as s\").s"), format!("\"{}\"", schema), "{}", extra);
    }
    let mut it = on_db(&format!("{}?options=-c%20statement_timeout%3D100&schema=f55s", url), "");
    has(&ev(&mut it, "db.one(\"select pg_sleep(0.5) is null as v\")"), "statement timeout", "the server's timeout");
}

// Q-739: `changes` counts the rows a statement matched, on every database: MySQL counted those it
// changed, so "update, else insert" inserted again; an upsert's or a replace's row counts once.
#[test]
fn changes_counts_the_rows_matched_on_every_database() {
    let (dbs, _held) = databases("matched");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "db.exec(\"create table stock (sku varchar(10) primary key, qty int)\")\ndb.exec(\"insert into stock values ('a', 5), ('b', 1)\")\n");
        let c = |it: &mut quillo::Interp, sql: &str| ev(it, &format!("db.exec(\"{}\").changes", sql));
        assert_eq!(c(&mut it, "update stock set qty = 5 where sku = 'a'"), "1", "{}: the value it had", kind);
        assert_eq!(c(&mut it, "update stock set qty = 6 where sku = 'a'"), "1", "{}", kind);
        assert_eq!(c(&mut it, "update stock set qty = qty where sku in ('a', 'b')"), "2", "{}", kind);
        assert_eq!(c(&mut it, "update stock set qty = 1 where sku = 'zz'"), "0", "{}", kind);
        assert_eq!(
            ev(&mut it, "let r = db.exec(\"update stock set qty = 6 where sku = ?\", \"a\")\nr.changes == 0 ? db.exec(\"insert into stock values (?, 6)\", \"a\").changes : \"updated\""),
            "\"updated\"",
            "{}",
            kind
        );
        let up = |rows: &str| match *kind {
            "mariadb" => format!("insert into stock values {} on duplicate key update qty = values(qty)", rows),
            _ => format!("insert into stock values {} on conflict (sku) do update set qty = excluded.qty", rows),
        };
        assert_eq!(c(&mut it, &up("('c', 1)")), "1", "{}: inserted", kind);
        assert_eq!(c(&mut it, &up("('c', 2)")), "1", "{}: updated", kind);
        assert_eq!(c(&mut it, &up("('c', 2)")), "1", "{}: as it was", kind);
        assert_eq!(c(&mut it, &up("('c', 3), ('d', 1)")), "2", "{}: one of each", kind);
        if *kind != "postgres" {
            assert_eq!(c(&mut it, "replace into stock values ('a', 9)"), "1", "{}", kind);
            assert_eq!(c(&mut it, "replace into stock values ('a', 9), ('e', 1)"), "2", "{}", kind);
        }
        assert_eq!(ev(&mut it, "db.one(\"select qty from stock where sku = 'c'\").qty"), "3", "{}", kind);
    }
    // MySQL's own count, where the URL asks for it.
    let Some((_my, url)) = maria_db("matched_own") else { return };
    let mut it = on_db(&format!("{}?client_found_rows=false", url), "db.exec(\"create table s (k int primary key, v int)\")\ndb.exec(\"insert into s values (1, 1)\")\n");
    assert_eq!(ev(&mut it, "db.exec(\"update s set v = 1 where k = 1\").changes"), "0");
}

// Q-740: on MySQL, an id at or past 2^63 is a `dec`, found by itself, where it was negative.
#[test]
fn a_mysql_id_past_2_63_is_a_dec() {
    let Some((_my, url)) = maria_db("bigid") else { return };
    let mut it = on_db(&url, "db.exec(\"create table ub (id bigint unsigned auto_increment primary key, n int)\")\ndb.exec(\"alter table ub auto_increment = 9223372036854775806\")\n");
    for (n, id) in [(1, "9223372036854775806"), (2, "9223372036854775807"), (3, "9223372036854775808m"), (4, "9223372036854775809m")] {
        assert_eq!(ev(&mut it, &format!("let r = db.exec(\"insert into ub (n) values ({})\")\n[r.last_id, db.one(\"select n from ub where id = ?\", r.last_id).n]", n)), format!("[{}, {}]", id, n));
    }
    assert_eq!(ev(&mut it, "db.exec(\"insert into ub (id, n) values (18446744073709551615, 5)\").last_id"), "18446744073709551615m");
    assert_eq!(ev(&mut it, "db.query(\"select id from ub where id > ? order by id\", 9223372036854775807m) | map((r) => r.id)"), "[9223372036854775808m, 9223372036854775809m, 18446744073709551615m]");
}

// Q-741: a MariaDB `BIT(n)` reads as PostgreSQL's `bit(n)`: its n digits, where its bytes were
// passed off as text.
#[test]
fn bit_columns_read_as_their_digits_on_mariadb_as_on_postgresql() {
    let (dbs, _held) = databases("bitcols");
    let ones = "1".repeat(64);
    for (kind, url) in dbs.iter().filter(|(k, _)| *k != "sqlite") {
        let mut it = on_db(url, "db.exec(\"create table bits (k int, b bit(8), b1 bit(1), b64 bit(64))\")\n");
        ev(&mut it, &format!("db.exec(\"insert into bits values (1, b'00000101', b'1', b'{}'), (2, b'11111111', b'0', b'{}1'), (3, b'01000001', null, null)\")", ones, "0".repeat(63)));
        assert_eq!(
            ev(&mut it, "db.query(\"select b, b1, b64 from bits order by k\")"),
            format!(r#"[{{ b: "00000101", b1: "1", b64: "{}" }}, {{ b: "11111111", b1: "0", b64: "{}1" }}, {{ b: "01000001", b1: null, b64: null }}]"#, ones, "0".repeat(63)),
            "{}",
            kind
        );
    }
}

// Q-742: a PostgreSQL type with no reader of its own comes back the same way in every row, by its
// type: hex, where bytes that happened to be UTF-8 were text; `timetz`, `pg_lsn` and `xid8` as
// PostgreSQL writes them.
#[test]
fn postgresql_types_without_a_reader_read_one_way_in_every_row() {
    let Some((_pg, url)) = pg_db("othertypes") else { return };
    let mut it = on_db(&url, "");
    assert_eq!(
        ev(&mut it, "db.one(\"select 'int4'::regtype as t, '00:00:00+00'::timetz as a, '12:00:00+02'::timetz as b, '05:30:15.5-05:30'::timetz as c, '16/B374D848'::pg_lsn as l, '42'::xid8 as x\")"),
        r#"{ t: "\\x00000017", a: "00:00:00+00", b: "12:00:00+02", c: "05:30:15.5-05:30", l: "16/B374D848", x: 42 }"#
    );
    // A range, in two rows: hex both times.
    let r = ev(&mut it, "db.query(\"select r from (values ('[1,5)'::int4range), ('[2,3)'::int4range)) v(r)\") | map((x) => x.r)");
    assert!(r.starts_with(r#"["\\x"#) && r.matches(r#""\\x"#).count() == 2, "{}", r);
    // Text in its kinds stays text.
    assert_eq!(ev(&mut it, "db.one(\"select 'é'::text as a, 'x'::\\\"char\\\" as b, '<a/>'::xml as c, 'n'::name as d\")"), r#"{ a: "é", b: "x", c: "<a/>", d: "n" }"#);
}

// Q-743: a BC date reads as ISO 8601 writes it (`-0043-03-15` for 44 BC, `0000` for 1 BC), and
// that text is written back as the same date.
#[test]
fn bc_dates_read_as_iso_and_are_written_back() {
    let Some((_pg, url)) = pg_db("bcdates") else { return };
    let mut it = on_db(&url, "db.exec(\"create table ds (id int, d date, t timestamp, tz timestamptz)\")\n");
    for (pg, iso) in [("0044-03-15 BC", "-0043-03-15"), ("0001-06-01 BC", "0000-06-01"), ("0002-01-01 BC", "-0001-01-01"), ("0001-01-01", "0001-01-01"), ("4713-11-24 BC", "-4712-11-24"), ("9999-12-31", "9999-12-31")] {
        let got = ev(&mut it, &format!("let d = db.one(\"select '{}'::date as d\").d\n[d, db.one(\"select ?::date = '{}'::date as same\", d).same]", pg, pg));
        assert_eq!(got, format!("[\"{}\", true]", iso), "{}", pg);
    }
    assert_eq!(ev(&mut it, "let t = db.one(\"select '4714-11-24 00:00:00+00 BC'::timestamptz as t\").t\n[t, db.one(\"select ?::timestamptz = '4714-11-24 00:00:00+00 BC'::timestamptz as same\", t).same]"), "[\"-4713-11-24T00:00:00Z\", true]");
    assert_eq!(ev(&mut it, "let t = db.one(\"select '0044-03-15 12:30:00 BC'::timestamp as t\").t\n[t, db.one(\"select ?::timestamp = '0044-03-15 12:30:00 BC'::timestamp as same\", t).same]"), "[\"-0043-03-15T12:30:00\", true]");
    // Into columns, by plain `?`; and as before for what is not BC.
    assert_eq!(ev(&mut it, "db.exec(\"insert into ds values (1, ?, ?, ?)\", \"-0043-03-15\", \"0000-01-02T03:04:05\", \"-0100-06-01T00:00:00Z\").changes"), "1");
    assert_eq!(ev(&mut it, "db.one(\"select d, t, tz, d = '0044-03-15 BC'::date as same from ds where id = 1\")"), "{ d: \"-0043-03-15\", t: \"0000-01-02T03:04:05\", tz: \"-0100-06-01T00:00:00Z\", same: true }");
    assert_eq!(ev(&mut it, "db.exec(\"insert into ds (id, d) values (2, ?), (3, ?)\", \"2026-11-06\", \"infinity\").changes"), "2");
    assert_eq!(ev(&mut it, "db.query(\"select d from ds where id > 1 order by id\") | map((r) => r.d)"), "[\"2026-11-06\", \"infinity\"]");
}

// Q-744: PostgreSQL's `real` and MariaDB's `FLOAT` read back as the shortest float that prints as
// they do: 0.1, not 0.10000000149011612.
#[test]
fn real_and_float_columns_read_back_as_written() {
    let (dbs, _held) = databases("reals");
    for (kind, url) in dbs.iter().filter(|(k, _)| *k != "sqlite") {
        let ty = if *kind == "mariadb" { "float" } else { "real" };
        let mut it = on_db(url, &format!("db.exec(\"create table fl (id int, r {})\")\n", ty));
        for (i, v) in ["0.1", "-0.1", "1.5", "3.4e38", "16777217", "0.3"].iter().enumerate() {
            assert_eq!(ev(&mut it, &format!("db.exec(\"insert into fl values ({}, ?)\", {}).changes", i, v)), "1", "{} {}", kind, v);
        }
        assert_eq!(
            ev(&mut it, "let r = db.query(\"select r from fl order by id\") | map((x) => x.r)\n[r[0], r[1], r[2], r[3] == 3.4e38, r[4], r[5]]"),
            "[0.1, -0.1, 1.5, true, 16777216.0, 0.3]",
            "{}",
            kind
        );
    }
}

// Q-745: after an `alter table`, a statement a connection kept is prepared again, once, where
// every request on it failed ("cached plan must not change result type") until the server
// restarted: on the pool's connections, in a transaction, and on the server's shared ones; a
// change made here, or seen on one connection, is a change for every connection of the pool.
#[test]
fn a_kept_statement_is_prepared_again_after_its_table_changes() {
    let Some((pg, url)) = pg_db("replan") else { return };
    let mut it = on_db(&url, "db.exec(\"create table cp (a int)\")\ndb.exec(\"insert into cp values (1)\")\n");
    let all = "db.query(\"select * from cp\")";
    let in_tx = "db.transaction(() => db.query(\"select * from cp\"))";
    assert_eq!(ev(&mut it, all), "[{ a: 1 }]");
    assert_eq!(ev(&mut it, in_tx), "[{ a: 1 }]");
    assert_eq!(ev(&mut it, "db.query(\"select * from cp where a = ?\", 1)"), "[{ a: 1 }]");
    // Changed here: every connection knows, in a transaction too.
    ev(&mut it, "db.exec(\"alter table cp add column b int default 2\")");
    assert_eq!(ev(&mut it, in_tx), "[{ a: 1, b: 2 }]");
    assert_eq!(ev(&mut it, all), "[{ a: 1, b: 2 }]");
    assert_eq!(ev(&mut it, "db.query(\"select * from cp where a = ?\", 1)"), "[{ a: 1, b: 2 }]");
    assert_eq!(ev(&mut it, "db.exec(\"update cp set b = ? where a = ?\", 3, 1).changes"), "1");
    // A column's type changed, and one dropped, in a script.
    ev(&mut it, "db.exec(\"alter table cp alter column b type text; select 1\")");
    assert_eq!(ev(&mut it, all), "[{ a: 1, b: \"3\" }]");
    ev(&mut it, "db.exec(\"alter table cp drop column b\")");
    assert_eq!(ev(&mut it, "db.query(\"select * from cp where a = ?\", 1)"), "[{ a: 1 }]");
    // Changed elsewhere (another server's migration): tried again once outside a transaction,
    // and the others then know.
    psql(&pg, &url, &["alter table cp add column c int default 4"]);
    assert_eq!(ev(&mut it, all), "[{ a: 1, c: 4 }]");
    assert_eq!(ev(&mut it, in_tx), "[{ a: 1, c: 4 }]");
    // A transaction that met it first: it fails, as the error ended it; the next one is right.
    psql(&pg, &url, &["alter table cp add column d int default 6"]);
    let first = ev(&mut it, in_tx);
    assert!(first == "[{ a: 1, c: 4, d: 6 }]" || first.contains("cached plan must not change result type"), "{}", first);
    assert_eq!(ev(&mut it, in_tx), "[{ a: 1, c: 4, d: 6 }]");
    assert_eq!(ev(&mut it, all), "[{ a: 1, c: 4, d: 6 }]");
    // The server's shared connections, each of which kept the old statement.
    let d = dir("replan-server", &format!("db.open(\"{}\")\nroute GET /all = json.stringify(db.query(\"select * from cp\"))\nroute GET /alter = str(db.exec(\"alter table cp add column e int default 5\").changes)\n", url));
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |path: &str| {
        let a = support::send(s.port, "GET", path, &[], "");
        (a.status, a.body)
    };
    for _ in 0..12 {
        assert_eq!(get("/all"), (200, "[{\"a\":1,\"c\":4,\"d\":6}]".to_string()));
    }
    assert_eq!(get("/alter"), (200, "0".to_string()));
    for i in 0..12 {
        assert_eq!(get("/all"), (200, "[{\"a\":1,\"c\":4,\"d\":6,\"e\":5}]".to_string()), "request {}", i);
    }
    psql(&pg, &url, &["alter table cp drop column e"]);
    for i in 0..12 {
        assert_eq!(get("/all"), (200, "[{\"a\":1,\"c\":4,\"d\":6}]".to_string()), "request {} after another's change", i);
    }
}

// Q-750: a statement that writes where its words do not show it goes to the primary, and a
// replica's refusal of one leaves the replica in use, where it was out for ten seconds. Each
// request is its own (a request that wrote reads from the primary after).
#[test]
fn a_select_that_writes_goes_to_the_primary_and_leaves_the_replica_in_use() {
    let Some(pg) = pg_server() else { return };
    let (primary, replica) = (pg.database("f55_rprimary"), pg.database("f55_rreplica"));
    for (url, who) in [(&primary, "primary"), (&replica, "replica")] {
        psql(&pg, url, &[
            "create table which (v text)",
            &format!("insert into which values ('{}')", who),
            "create sequence rs",
            "create view nv as select nextval('rs') as n",
            &format!("create function f55_mark() returns text language sql as $$ select '{}'::text $$", who),
        ]);
    }
    let name = replica.rsplit('/').next().unwrap().to_string();
    psql(&pg, &primary, &[&format!("alter database {} set default_transaction_read_only = on", name)]);
    let src = format!(
        "db.open(\"{}\", {{ replicas: [\"{}\"] }})\nlet sqls = [\"select nextval('rs') as n, (select v from which) as w\", \"select setval('rs', 10) as n, (select v from which) as w\", \"select v as w, 1 as n from which for update\", \"select pg_catalog.nextval('rs') as n, (select v from which) as w\", \"select f55_mark() as w, 0 as n\", \"select n, (select v from which) as w from nv\", \"select (select v from which) as w, 1 as n from nv\"]\nfn w(i: int) = db.one(sqls[i]).w\nroute GET /read = db.one(\"select v from which\").v\nroute GET /w/:i = w(int(i))\nroute GET /only = str(len(db.query(\"select * from only_here\")))\n",
        primary, replica
    );
    let d = dir("replicas-write", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |path: &str| support::send(s.port, "GET", path, &[], "").body;
    assert_eq!(get("/read"), "replica");
    // Each read the replica refuses or the words show writes: the primary, and the replica stays
    // in use; the last two take the sequence through a view, which the replica refuses.
    for i in 0..7 {
        assert_eq!(get(&format!("/w/{}", i)), "primary", "statement {}", i);
        assert_eq!(get("/read"), "replica", "after statement {}", i);
    }
    // A replica that fails otherwise is left out, as before.
    psql(&pg, &primary, &["create table only_here (x int)"]);
    assert_eq!(get("/only"), "0");
    assert_eq!(get("/read"), "primary");
}

// Q-751: on MySQL, a value past the server's `max_allowed_packet` is refused, naming the limit,
// where it failed as "Broken pipe" or "Packets out of sync"; the other databases take it.
#[test]
fn a_value_past_max_allowed_packet_is_refused_saying_so() {
    let (dbs, _held) = databases("packet");
    for (kind, url) in &dbs {
        let col = if *kind == "mariadb" { "longtext" } else { "text" };
        let mut it = on_db(url, &format!("db.exec(\"create table r11 (v {})\")\n", col));
        let big = ev(&mut it, "db.exec(\"insert into r11 values (?)\", repeat(\"a\", 17000000))");
        if *kind == "mariadb" {
            has(&big, "past the server's max_allowed_packet of 16777216 bytes", kind);
            has(&ev(&mut it, "db.one(\"select ? as v\", repeat(\"é\", 9000000))"), "max_allowed_packet", kind);
            has(&ev(&mut it, &format!("db.exec(\"insert into r11 values ('{}')\")", "b".repeat(17_000_000))), "max_allowed_packet", "text");
        } else {
            has(&big, "{ changes: 1, last_id: ", kind);
        }
        // Below it, as before, on the connection after the refusal too.
        assert_eq!(ev(&mut it, "db.exec(\"insert into r11 values (?)\", repeat(\"a\", 1000000)).changes"), "1", "{}", kind);
        assert_eq!(ev(&mut it, "db.exec(\"insert into r11 values (?)\", \"ü\").changes"), "1", "{}", kind);
    }
}

// Q-752: SQLite's `@name` and `$name` take a map's `name`; on MySQL `@name` is a variable of the
// session, so a map with `name` is refused, saying to write `:name`, where it gave null.
#[test]
fn named_values_and_mysqls_variables() {
    let (dbs, _held) = databases("atnames");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "");
        match *kind {
            "sqlite" => {
                assert_eq!(ev(&mut it, "db.one(\"select @x as v, $y as w, :z as u\", { x: 3, y: \"é\", z: null })"), "{ v: 3, w: \"é\", u: null }");
                assert_eq!(ev(&mut it, "db.one(\"select @x as v\", { \"@x\": 4 })"), "{ v: 4 }");
                has(&ev(&mut it, "db.one(\"select @x as v\", { y: 1 })"), "no value for @x", kind);
            }
            "mariadb" => {
                has(&ev(&mut it, "db.one(\"select @x as v\", { x: 3 })"), "`@x` is a variable of MySQL's session (null unless set), not one of the values: write `:x`", kind);
                has(&ev(&mut it, "db.one(\"select @`x` + :y as v\", { x: 3, y: 1 })"), "`@x` is a variable", kind);
                // Not a value of the map's: the variable, as MySQL has it.
                assert_eq!(ev(&mut it, "db.one(\"select @x as v\", { y: 1 })"), "{ v: null }");
                assert_eq!(ev(&mut it, "db.one(\"select @@sql_mode is not null as v\", { sql_mode: 1 }).v"), "1");
                assert_eq!(ev(&mut it, "db.one(\"select :x as v\", { x: 3 })"), "{ v: 3 }");
            }
            _ => assert_eq!(ev(&mut it, "db.one(\"select :x as v\", { x: 3 })"), "{ v: 3 }"),
        }
    }
}

// Q-753: `changes` and `last_id` mean the same on every database, and SQL with no statement is
// refused.
#[test]
fn changes_and_last_id_mean_the_same_on_every_database() {
    let (dbs, _held) = databases("counts");
    for (kind, url) in &dbs {
        let key = if *kind == "mariadb" { "id int auto_increment primary key" } else if *kind == "postgres" { "id serial primary key" } else { "id integer primary key" };
        let mut it = on_db(url, &format!("db.exec(\"create table t3 ({}, n int)\")\n", key));
        let r = |it: &mut quillo::Interp, sql: &str| ev(it, &format!("let r = db.exec({})\n[r.changes, r.last_id]", sql));
        let one = if *kind == "postgres" { "null" } else { "1" };
        assert_eq!(r(&mut it, "\"insert into t3 (n) values (?)\", 7"), format!("[1, {}]", one), "{}: one row", kind);
        assert_eq!(r(&mut it, "\"insert into t3 (n) values (1), (2), (3)\""), "[3, null]", "{}: several", kind);
        assert_eq!(r(&mut it, "\"update t3 set n = 9 where id = 100\""), "[0, null]", "{}: none", kind);
        assert_eq!(r(&mut it, "\"update t3 set n = 8 where id = 1\""), "[1, null]", "{}: an update", kind);
        assert_eq!(r(&mut it, "\"select * from t3\""), "[0, null]", "{}: a select", kind);
        assert_eq!(r(&mut it, "\"select * from t3 where n > ?\", 0"), "[0, null]", "{}: a select with values", kind);
        assert_eq!(r(&mut it, "\"create table t4 (x int)\""), "[0, null]", "{}: a schema", kind);
        assert_eq!(r(&mut it, "\"insert into t4 values (1); insert into t4 values (2), (3)\""), "[2, null]", "{}: a script's last", kind);
        assert_eq!(r(&mut it, "\"insert into t4 values (4); select 1\""), "[0, null]", "{}: a script ending in a read", kind);
        for sql in ["\"\"", "\"   \"", "\"-- only a note\"", "\";\"", "\"/* */ ;\""] {
            has(&ev(&mut it, &format!("db.exec({})", sql)), "the SQL has no statement in it", &format!("{} {}", kind, sql));
            has(&ev(&mut it, &format!("db.one({})", sql)), "the SQL has no statement in it", &format!("{} {}", kind, sql));
            has(&ev(&mut it, &format!("db.query({})", sql)), "the SQL has no statement in it", &format!("{} {}", kind, sql));
        }
        assert_eq!(ev(&mut it, "db.one(\"select count(*) as n from t3\").n"), "4", "{}", kind);
        if *kind != "mariadb" {
            assert_eq!(ev(&mut it, "db.exec(\"insert into t3 (n) values (?) returning id\", 5).changes"), "1", "{}: returning", kind);
        }
    }
    // SQLite's own `last_insert_rowid()` is what it was, and what the program's SQL reads: a
    // child's parent id (found in review: it was i64::MIN).
    let (dbs, _held) = databases("rowid");
    let mut it = on_db(&dbs[0].1, "db.exec(\"create table t (id integer primary key, n int)\")\ndb.exec(\"insert into t (n) values (1)\")\n");
    assert_eq!(ev(&mut it, "db.exec(\"update t set n = 2\").last_id"), "null");
    assert_eq!(ev(&mut it, "db.one(\"select last_insert_rowid() as i\").i"), "1");
    let mut it = on_db(&dbs[0].1, "db.exec(\"create table parent (id integer primary key, name text)\")\ndb.exec(\"create table child (parent_id int, n text)\")\n");
    assert_eq!(ev(&mut it, "db.exec(\"insert into parent (name) values (?)\", \"p1\")"), "{ changes: 1, last_id: 1 }");
    assert_eq!(ev(&mut it, "db.exec(\"insert into child (parent_id, n) values (last_insert_rowid(), ?)\", \"c1\")"), "{ changes: 1, last_id: 1 }");
    assert_eq!(ev(&mut it, "db.exec(\"update child set n = 'c2'\")\ndb.exec(\"insert into child (parent_id, n) values (last_insert_rowid(), 'c3')\").changes"), "1");
    // The update set no id: the second child's parent is the first child's row id, as SQLite has it.
    assert_eq!(ev(&mut it, "db.query(\"select parent_id, n from child order by rowid\")"), r#"[{ parent_id: 1, n: "c2" }, { parent_id: 1, n: "c3" }]"#);
    // A script follows its last statement, as on the others; the same id put back is none.
    assert_eq!(ev(&mut it, "db.exec(\"insert into parent (name) values ('x'); update parent set name = 'y' where id = 1\")"), "{ changes: 1, last_id: null }");
    assert_eq!(ev(&mut it, "db.exec(\"update parent set name = 'z' where id = 1; insert into parent (name) values ('w')\")"), "{ changes: 1, last_id: 3 }");
    assert_eq!(ev(&mut it, "db.exec(\"insert or replace into parent (id, name) values (9, 'a')\").last_id"), "9");
    assert_eq!(ev(&mut it, "db.exec(\"insert or replace into parent (id, name) values (9, 'b')\").last_id"), "9");
    // An upsert that updated inserted no row.
    assert_eq!(ev(&mut it, "db.exec(\"insert into parent (id, name) values (9, 'c') on conflict (id) do update set name = excluded.name\").last_id"), "null");
    assert_eq!(ev(&mut it, "db.exec(\"insert into parent (id, name) values (10, 'c') on conflict (id) do update set name = excluded.name\").last_id"), "10");
}

// Q-754: the guide's column-list `update` fails on MySQL, and on PostgreSQL with one column;
// written as pairs from the columns, it works on every database.
#[test]
fn an_update_written_as_pairs_from_the_columns_works_on_every_database() {
    let (dbs, _held) = databases("pairs");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "db.exec(\"create table cl (id int, a int, b varchar(10))\")\ndb.exec(\"insert into cl values (1, 0, 'x')\")\n");
        for cols in ["[\"a\", \"b\"]", "[\"a\"]"] {
            let got = ev(&mut it, &format!("let cols = {}\nlet set = cols | map((c) => `${{sql_name(c)}} = ${{sql_param(c)}}`) | join(\", \")\ndb.exec(`update cl set ${{set}} where id = :id`, {{ a: 1, b: \"é\", id: 1 }}).changes", cols));
            assert_eq!(got, "1", "{} {}", kind, cols);
        }
        assert_eq!(ev(&mut it, "db.one(\"select a, b from cl\")"), "{ a: 1, b: \"é\" }", "{}", kind);
    }
}

// Q-756: PostgreSQL arrays keep their bounds, and a jsonb[] takes lists; MySQL's IPv6 hosts,
// unsigned ints past 2^63 and `tinyint(1)` holding more than 0 and 1.
#[test]
fn smaller_differences_of_the_drivers() {
    if let Some((_pg, url)) = pg_db("arrays") {
        let mut it = on_db(&url, "db.exec(\"create table ar (id int, a int[], j jsonb[])\")\n");
        assert_eq!(ev(&mut it, "db.one(\"select '[0:2]={1,2,3}'::int[] as a, '{1,2}'::int[] as b, '[2:3][1:1]={{5},{6}}'::int[] as c, '{}'::int[] as d\")"), "{ a: \"[0:2]={\"1\",\"2\",\"3\"}\", b: [1, 2], c: \"[2:3][1:1]={{\"5\"},{\"6\"}}\", d: [] }");
        assert_eq!(ev(&mut it, "let a = db.one(\"select '[0:2]={1,2,3}'::int[] as a\").a\ndb.exec(\"insert into ar (id, a) values (1, ?)\", a).changes"), "1");
        assert_eq!(ev(&mut it, "db.one(\"select a[0] as z, array_lower(a, 1) as lo from ar where id = 1\")"), "{ z: 1, lo: 0 }");
        assert_eq!(ev(&mut it, "db.exec(\"insert into ar (id, j) values (2, ?)\", [[1, 2], { a: [3] }, \"[4]\", null, \"\\\"é\\\"\"]).changes"), "1");
        assert_eq!(ev(&mut it, "db.one(\"select j from ar where id = 2\").j"), "[[1, 2], { a: [3] }, [4], null, \"é\"]");
    }
    let Some((my, url)) = maria_db("smaller") else { return };
    // An IPv6 host is connected to, not looked up as a name.
    let six = format!("mysql://root@[::1]:{}/f55_smaller", support::free_port());
    let mut it = on_db(&url, "");
    let said = ev(&mut it, &format!("let r = db.connect(\"{}\")\nstr(r.error)", six));
    assert!(!said.contains("lookup") && said.contains("refused"), "{}", said);
    drop(my);
    ev(&mut it, "db.exec(\"create table u (k int, ubi bigint unsigned, t tinyint(1))\")");
    ev(&mut it, "db.exec(\"insert into u values (1, 18446744073709551615, 0), (2, 18446744073709551614, 1), (3, 9223372036854775808, 2), (4, 9223372036854775807, -1), (5, 0, 5)\")");
    assert_eq!(ev(&mut it, "db.query(\"select ubi, t from u order by k\")"), "[{ ubi: 18446744073709551615m, t: false }, { ubi: 18446744073709551614m, t: true }, { ubi: 9223372036854775808m, t: 2 }, { ubi: 9223372036854775807, t: -1 }, { ubi: 0, t: 5 }]");
    // The value read finds its own row alone.
    assert_eq!(ev(&mut it, "let r = db.one(\"select ubi from u where k = 2\")\ndb.query(\"select k from u where ubi = ?\", r.ubi)"), "[{ k: 2 }]");
}

// From slice 5: MariaDB read `||` as `or` under its default sql_mode, so `'%' || ? || '%'` was 0
// or 1 there; its driver adds PIPES_AS_CONCAT to the session before SQL with `||`, until the
// pool resets the connection.
#[test]
fn double_bars_join_text_on_every_database() {
    let (dbs, _held) = databases("pipes");
    for (kind, url) in &dbs {
        let mut it = on_db(url, "db.exec(\"create table pp (name varchar(10))\")\ndb.exec(\"insert into pp values ('ab'), ('cd')\")\n");
        for _ in 0..4 {
            assert_eq!(ev(&mut it, "db.one(\"select '%' || ? || '%' as l, 'a' || null as n\", \"é\")"), "{ l: \"%é%\", n: null }", "{}", kind);
        }
        assert_eq!(ev(&mut it, "db.query(\"select name from pp where name like '%' || ? || '%'\", \"b\")"), "[{ name: \"ab\" }]", "{}", kind);
        if *kind == "mariadb" {
            // The rest of its mode as the server has it; in a transaction; and SQL without `||`
            // on a session the pool reset.
            assert_eq!(ev(&mut it, "db.one(\"select 'x' || 'y' as v, @@sql_mode like '%STRICT_TRANS_TABLES%' as s\")"), "{ v: \"xy\", s: 1 }");
            assert_eq!(ev(&mut it, "db.transaction(() => [db.one(\"select 1 as v\").v, db.one(\"select 'x' || 'y' as v\").v, db.one(\"select 'p' || ? as v\", \"q\").v])"), "[1, \"xy\", \"pq\"]");
            assert_eq!(ev(&mut it, "db.one(\"select 1 or 0 as v\").v"), "1");
        }
    }
}

// ---- slice 6, after review -------------------------------------------------------------------

// Found in review: MariaDB's `||` joins text on every path that runs the program's SQL, a
// migration's (views and all) and `db.exec_many`'s too.
#[test]
fn double_bars_join_text_in_migrations_too() {
    let (dbs, _held) = databases("barsmig");
    for (kind, url) in &dbs {
        let d = support::temp_dir(&format!("feedback55-barsmig-{}", kind));
        std::fs::create_dir_all(d.join("migrations")).unwrap();
        std::fs::write(d.join("migrations/001_a.sql"), "create table mm (v varchar(10));\ninsert into mm values ('a' || 'b');\ncreate view mv as select 'x' || 'y' as c;\n").unwrap();
        let mut it = quillo::Interp::new(&d);
        it.load_source(&format!("db.open(\"{}\")\n", url), "app.qwe").unwrap();
        assert_eq!(ev(&mut it, "db.migrate()"), r#"["001_a.sql"]"#, "{}", kind);
        assert_eq!(ev(&mut it, "[db.one(\"select v from mm\").v, db.one(\"select c from mv\").c]"), r#"["ab", "xy"]"#, "{}", kind);
        assert_eq!(ev(&mut it, "db.exec_many(\"insert into mm (v) values (? || 'z')\", [[\"b\"], [\"é\"]]).changes"), "2", "{}", kind);
        assert_eq!(ev(&mut it, "db.query(\"select v from mm where v like '%z' order by v\") | map((r) => r.v)"), r#"["bz", "éz"]"#, "{}", kind);
    }
}

// Found in review (Q-741): a MariaDB `BIT`'s digits, as Quillo reads them, go back in: stored,
// compared and updated, as PostgreSQL's `bit` takes its digits; a text column's "0101" stays text.
#[test]
fn bit_digits_read_go_back_in_on_mariadb_as_on_postgresql() {
    let (dbs, _held) = databases("bitback");
    for (kind, url) in dbs.iter().filter(|(k, _)| *k != "sqlite") {
        let mut it = on_db(url, "db.exec(\"create table bt (k int, b bit(8), s varchar(10))\")\ndb.exec(\"insert into bt values (1, b'00000101', '0101')\")\n");
        assert_eq!(ev(&mut it, "let b = db.one(\"select b from bt where k = 1\").b\ndb.exec(\"insert into bt (k, b, s) values (?, ?, ?)\", 2, b, \"0101\").changes"), "1", "{}", kind);
        assert_eq!(ev(&mut it, "db.query(\"select k from bt where b = ? order by k\", \"00000101\") | map((r) => r.k)"), "[1, 2]", "{}", kind);
        assert_eq!(ev(&mut it, "db.query(\"select k from bt where s = ? order by k\", \"0101\") | map((r) => r.k)"), "[1, 2]", "{}", kind);
        assert_eq!(ev(&mut it, "db.exec(\"update bt set b = ? where k = ?\", \"11111111\", 2).changes"), "1", "{}", kind);
        assert_eq!(ev(&mut it, "db.query(\"select k, b from bt where b in (?, ?) order by k\", \"00000101\", \"11111111\")"), r#"[{ k: 1, b: "00000101" }, { k: 2, b: "11111111" }]"#, "{}", kind);
        assert_eq!(ev(&mut it, "db.query(\"select k from bt where ? = b\", \"11111111\") | map((r) => r.k)"), "[2]", "{}", kind);
        assert_eq!(ev(&mut it, "db.query(\"select k from bt where b <> ? order by k\", \"11111111\") | map((r) => r.k)"), "[1]", "{}", kind);
        // As each database says of digits that do not fit.
        assert_eq!(ev(&mut it, "is_err(db.exec(\"insert into bt (k, b) values (3, ?)\", \"111111111\"))"), "true", "{}", kind);
    }
}

// Found in review (Q-734): a float a MariaDB `FLOAT` would keep as 0 is refused as out of its
// range, as PostgreSQL's `real` refuses it; one it keeps goes in.
#[test]
fn a_float_a_float_column_would_keep_as_zero_is_refused() {
    let (dbs, _held) = databases("floatzero");
    for (kind, url) in dbs.iter().filter(|(k, _)| *k != "sqlite") {
        let ty = if *kind == "mariadb" { "float" } else { "real" };
        let mut it = on_db(url, &format!("db.exec(\"create table fz (k int, f {}, d double precision)\")\n", ty));
        has(&ev(&mut it, "db.exec(\"insert into fz (k, f) values (1, ?)\", 1e-46)"), "out of range", kind);
        has(&ev(&mut it, "db.exec(\"update fz set f = ? where k = 1\", -1e-50)"), "out of range", kind);
        for v in ["1e-40", "0.0", "1.5", "-0.0"] {
            assert_eq!(ev(&mut it, &format!("db.exec(\"insert into fz (k, f) values (2, ?)\", {}).changes", v)), "1", "{} {}", kind, v);
        }
        assert_eq!(ev(&mut it, "db.exec(\"insert into fz (k, d) values (3, ?)\", 1e-46).changes"), "1", "{}: a double holds it", kind);
    }
}

// Found in review (Q-750): on MySQL, a read that calls the database's own function goes to the
// primary, as on PostgreSQL (looked up once a name); the server's own functions do not.
#[test]
fn a_mysql_read_calling_its_own_function_goes_to_the_primary() {
    let Some(my) = maria_server() else { return };
    let (primary, replica) = (my.url("f55_myprimary"), my.url("f55_myreplica"));
    for (url, who) in [(&primary, "primary"), (&replica, "replica")] {
        let mut it = on_db(url, "");
        ev(&mut it, &format!("db.exec(\"create table which (v varchar(10))\")\ndb.exec(\"insert into which values ('{}')\")\ndb.exec(\"create function f55_mark() returns varchar(10) deterministic return '{}'\")", who, who));
    }
    let src = format!(
        "db.open(\"{}\", {{ replicas: [\"{}\"] }})\nroute GET /read = db.one(\"select v from which\").v\nroute GET /own = db.one(\"select F55_Mark() as w\").w\nroute GET /built = db.one(\"select lower(v) as w from which\").w\n",
        primary, replica
    );
    let d = dir("mysql-own-fns", &src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |path: &str| support::send(s.port, "GET", path, &[], "").body;
    assert_eq!(get("/read"), "replica");
    assert_eq!(get("/own"), "primary");
    assert_eq!(get("/own"), "primary");
    assert_eq!(get("/built"), "replica");
    assert_eq!(get("/read"), "replica");
}

// Found in review (Q-745): a schema's statement lets go only the kept statements on what it
// names; the others keep working, and those on it are prepared again.
#[test]
fn a_schema_change_lets_go_only_the_statements_on_what_it_names() {
    let Some((_pg, url)) = pg_db("replanlocal") else { return };
    let mut it = on_db(&url, "db.exec(\"create table a (x int)\")\ndb.exec(\"create table b (y int)\")\ndb.exec(\"insert into a values (1)\")\ndb.exec(\"insert into b values (2)\")\n");
    for _ in 0..3 {
        assert_eq!(ev(&mut it, "[db.query(\"select * from a\"), db.query(\"select * from b\")]"), "[[{ x: 1 }], [{ y: 2 }]]");
        assert_eq!(ev(&mut it, "db.exec(\"drop table if exists missing_one\").changes"), "0");
    }
    ev(&mut it, "db.exec(\"alter table b add column z int default 3\")");
    assert_eq!(ev(&mut it, "[db.query(\"select * from a\"), db.query(\"select * from b\")]"), "[[{ x: 1 }], [{ y: 2, z: 3 }]]");
    ev(&mut it, "db.exec(\"create or replace view vb as select * from b\")");
    assert_eq!(ev(&mut it, "db.query(\"select * from vb\")"), "[{ y: 2, z: 3 }]");
    ev(&mut it, "db.exec(\"drop view vb\")\ndb.exec(\"create view vb as select y from b\")");
    assert_eq!(ev(&mut it, "db.transaction(() => db.query(\"select * from vb\"))"), "[{ y: 2 }]");
    ev(&mut it, "db.exec(\"alter table \\\"b\\\" rename to c\")");
    assert_eq!(ev(&mut it, "db.query(\"select * from c\")"), "[{ y: 2, z: 3 }]");
}

// Found in review (Q-735): -0 already stored in PostgreSQL reads as 0.0, as every database is
// now given 0.0.
#[test]
fn a_stored_minus_zero_reads_as_zero() {
    let Some((_pg, url)) = pg_db("minuszero") else { return };
    let mut it = on_db(&url, "");
    assert_eq!(ev(&mut it, "let r = db.one(\"select '-0'::float8 as a, '-0'::float4 as b, '[-0.0]'::json as c\")\n[r.a, 1.0 / r.a > 0.0, r.b, 1.0 / r.c[0] > 0.0]"), "[0.0, true, 0.0, true]");
}

// ---- slice 13 ----
// Q-835 to Q-852: the command line, `.env`, the policy and the language server.

/// `quillo args...` in `d`, `input` on its stdin, under a deadline: its status, stdout and stderr.
fn quillo_in(d: &Path, args: &[&str], input: &[u8]) -> (Option<i32>, String, String) {
    use std::io::{Read, Write};
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(d)
        .env("HOME", d)
        .env("QUILLO_REGISTRY", "http://127.0.0.1:9")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let (mut stdin, mut out, mut err) = (child.stdin.take().unwrap(), child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let input = input.to_vec();
    std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let o = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        b
    });
    let e = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        b
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("quillo {:?} did not end", args);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let text = |b: Vec<u8>| String::from_utf8_lossy(&b).into_owned();
    (status.code(), text(o.join().unwrap()), text(e.join().unwrap()))
}

/// A folder of these files (bytes), for one test.
fn files_dir(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback55-{}", name));
    for (f, bytes) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    d
}

/// `quillo run args...` in `d` on a port of its own, its log kept, until the guard is dropped:
/// the guard, the port and the log.
fn run_logged(d: &Path, args: &[&str]) -> (support::Guard, u16, PathBuf) {
    let port = support::free_port();
    let log = d.join(format!("server-{}.log", port));
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .args(args)
        .args(["--port", &port.to_string()])
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

/// Waits, up to 15 s, for `f` to hold; else fails with the log.
fn until_logged(log: &Path, what: &str, mut f: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !f() {
        assert!(std::time::Instant::now() < deadline, "{}: {}", what, std::fs::read_to_string(log).unwrap_or_default());
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn body_of(port: u16, path: &str) -> String {
    support::send(port, "GET", path, &[], "").body
}

// Q-835: a `.env` that was not UTF-8, or could not be read, was skipped without a word, and every
// `env()` was null. Now only a missing one (or a folder of that name) is nothing; any other is
// said with where, and stops the start with 2, as `--env-file` does. A service's own is said too.
#[test]
fn a_dotenv_that_cannot_be_read_is_said_and_stops_the_start() {
    let prog: &[u8] = b"print(env(\"OTHER\"))\n";
    let d = files_dir("dotenv-bad", &[("main.qwe", prog), (".env", b"KEY=s3cr\xe9t\nOTHER=2\n")]);
    let (code, out, err) = quillo_in(&d, &["run", "main.qwe"], b"");
    assert_eq!(code, Some(2), "{}{}", out, err);
    assert!(out.is_empty() && err.contains("cannot read .env: it is not UTF-8 text: byte 0xE9 at line 1, column 9 (save it as UTF-8); to start without it, remove it or give `--env-file"), "{}", err);
    // A UTF-16 file, as Notepad saves one.
    let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("OTHER=2\n".encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
    std::fs::write(d.join(".env"), &utf16).unwrap();
    let (code, _, err) = quillo_in(&d, &["run", "main.qwe"], b"");
    assert_eq!(code, Some(2), "{}", err);
    assert!(err.contains("byte 0xFF at line 1, column 1"), "{}", err);
    // One that cannot be read.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(d.join(".env"), "OTHER=2\n").unwrap();
        std::fs::set_permissions(d.join(".env"), std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(d.join(".env")).is_err() {
            let (code, _, err) = quillo_in(&d, &["run", "main.qwe"], b"");
            assert_eq!(code, Some(2), "{}", err);
            assert!(err.contains("cannot read .env: Permission denied") && err.contains("; make it readable, or, to start without it, remove it or give `--env-file /dev/null`"), "{}", err);
            // The way out it says.
            assert_eq!(quillo_in(&d, &["run", "main.qwe", "--env-file", "/dev/null"], b""), (Some(0), "null\n".into(), String::new()));
        }
        std::fs::set_permissions(d.join(".env"), std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    // A link that leads nowhere: said with where it leads (review of slice 13).
    #[cfg(unix)]
    {
        std::fs::remove_file(d.join(".env")).unwrap();
        std::os::unix::fs::symlink(d.join("gone/prod.env"), d.join(".env")).unwrap();
        let (code, _, err) = quillo_in(&d, &["run", "main.qwe"], b"");
        assert_eq!(code, Some(2), "{}", err);
        assert!(err.contains(&format!("cannot read .env: it is a link to {}, which is not there; to start without it", d.join("gone/prod.env").display())), "{}", err);
        // Named with --env-file: no way out said, it was asked for.
        let (code, _, err) = quillo_in(&d, &["run", "main.qwe", "--env-file", ".env"], b"");
        assert_eq!(code, Some(2), "{}", err);
        assert!(err.contains("which is not there") && !err.contains("to start without it"), "{}", err);
        // Leading to a file: read through the link.
        std::fs::create_dir_all(d.join("gone")).unwrap();
        std::fs::write(d.join("gone/prod.env"), "OTHER=linked\n").unwrap();
        assert_eq!(quillo_in(&d, &["run", "main.qwe"], b""), (Some(0), "linked\n".into(), String::new()));
        std::fs::remove_file(d.join(".env")).unwrap();
    }
    // The edges that read as before: UTF-8 with text past ASCII and a byte-order mark, an empty
    // file, none at all, and a folder named `.env` (a Python virtual environment).
    std::fs::write(d.join(".env"), "\u{FEFF}OTHER=ünï ✓\n").unwrap();
    assert_eq!(quillo_in(&d, &["run", "main.qwe"], b""), (Some(0), "ünï ✓\n".into(), String::new()));
    std::fs::write(d.join(".env"), "").unwrap();
    assert_eq!(quillo_in(&d, &["run", "main.qwe"], b""), (Some(0), "null\n".into(), String::new()));
    std::fs::remove_file(d.join(".env")).unwrap();
    assert_eq!(quillo_in(&d, &["run", "main.qwe"], b""), (Some(0), "null\n".into(), String::new()));
    std::fs::create_dir(d.join(".env")).unwrap();
    assert_eq!(quillo_in(&d, &["run", "main.qwe"], b""), (Some(0), "null\n".into(), String::new()));
    // Named, a folder is no file.
    let (code, _, err) = quillo_in(&d, &["run", "main.qwe", "--env-file", ".env"], b"");
    assert_eq!(code, Some(2), "{}", err);
    assert!(err.contains("cannot read .env"), "{}", err);
}

#[test]
fn a_services_own_dotenv_that_cannot_be_read_stops_its_load() {
    let d = files_dir(
        "dotenv-service",
        &[("app.qwe", b"service billing = \"svc/billing.qwe\"\nroute GET / = billing.who()\n"), ("svc/billing.qwe", b"server fn who() = env(\"B\") ?? \"none\"\n"), ("svc/.env", b"B=\xe9\n")],
    );
    let (code, _, err) = quillo_in(&d, &["run", "app.qwe", "--port", "0"], b"");
    assert_eq!(code, Some(1), "{}", err);
    assert!(err.contains("service `billing`: cannot read ./svc/.env: it is not UTF-8 text: byte 0xE9 at line 1, column 3"), "{}", err);
    // Read, its skipped lines said as the program's are.
    std::fs::write(d.join("svc/.env"), b"B=ok\nC=x\0\n").unwrap();
    let (_g, port, log) = run_logged(&d, &["app.qwe"]);
    assert_eq!(body_of(port, "/"), "ok");
    assert!(std::fs::read_to_string(&log).unwrap().contains("svc/.env:2: a value cannot hold a NUL byte, skipped"));
}

// Q-838: `KEY= # note` set "# note", though the guide says ` #` ends a bare value. Now a value that
// starts with `#` after spaces or a tab is empty; `KEY=#fff` and a quoted `#` keep theirs.
#[test]
fn dotenv_a_note_after_the_spaces_is_no_value() {
    use quillo::dotenv::parse;
    let one = |text: &str| parse(text).into_iter().next().map(|(_, v)| v);
    for (text, want) in [
        ("KEY= # note", ""),
        ("KEY=   # set me", ""),
        ("KEY=\t#x", ""),
        ("KEY= #", ""),
        ("KEY = # spaced key", ""),
        ("KEY=#fff", "#fff"),
        ("KEY=x # note", "x"),
        ("KEY=x#y", "x#y"),
        ("KEY= \"# quoted\"", "# quoted"),
        ("KEY= '# single'", "# single"),
        ("KEY=", ""),
        ("KEY= ünï # n", "ünï"),
    ] {
        assert_eq!(one(text).as_deref(), Some(want), "{:?}", text);
    }
}

// Q-839: an unclosed quote took the rest of the file as its value while saying the line was
// skipped. Now the line is skipped, as said, and the lines after it are read as their own.
#[test]
fn dotenv_an_unclosed_quote_skips_its_line_alone() {
    use quillo::dotenv::{parse, parse_noting};
    let pairs = |t: &str| parse(t).into_iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<_>>();
    assert_eq!(parse_noting("KEY='abc\nOTHER=2\n"), (vec![("OTHER".to_string(), "2".to_string())], vec![(1, "its quote is not closed")]));
    assert_eq!(pairs("KEY=\"abc\nOTHER=2\nLAST=3"), ["OTHER=2", "LAST=3"]);
    // On the last line, alone in the file.
    assert_eq!(parse_noting("A=1\nKEY=\"open"), (vec![("A".to_string(), "1".to_string())], vec![(2, "its quote is not closed")]));
    assert_eq!(pairs("KEY='"), Vec::<String>::new());
    // Closed on a later line: one value over lines, as a PEM key, and closed on the last line.
    assert_eq!(pairs("PEM='-----BEGIN-----\nabc\n-----END-----'\nNEXT=1"), ["PEM=-----BEGIN-----\nabc\n-----END-----", "NEXT=1"]);
    assert_eq!(pairs("Q=\"ünï\n✓\""), ["Q=ünï\n✓"]);
    // A quote inside a bare value is no opening one.
    assert_eq!(pairs("A=it's\nB=2"), ["A=it's", "B=2"]);
}

// Q-840: a NUL in a value panicked as it was set. Now the line is said and skipped, and the others
// are read; a NUL in a quoted value too, and `\0` written in double quotes is the digit.
#[test]
fn dotenv_a_nul_in_a_value_is_said_and_skipped() {
    use quillo::dotenv::parse_noting;
    assert_eq!(parse_noting("KEY=x\0\nB=2\n"), (vec![("B".to_string(), "2".to_string())], vec![(1, "a value cannot hold a NUL byte")]));
    assert_eq!(parse_noting("KEY=\"a\0b\"").1, vec![(1, "a value cannot hold a NUL byte")]);
    assert_eq!(parse_noting("KEY='\0'").1, vec![(1, "a value cannot hold a NUL byte")]);
    assert_eq!(parse_noting("KEY=\"\\0\"").0, vec![("KEY".to_string(), "0".to_string())]);
    assert_eq!(parse_noting("# a note \0\nK\0=1\n"), (vec![], vec![(2, "not KEY=value")]));
    let d = files_dir("dotenv-nul", &[("main.qwe", b"for k in [\"KEY\", \"B\"] { print(k, json.stringify(env(k))) }\n"), (".env", b"KEY=x\0\nB=2\n")]);
    let (code, out, err) = quillo_in(&d, &["run", "main.qwe"], b"");
    assert_eq!((code, out.as_str()), (Some(0), "KEY null\nB \"2\"\n"), "{}", err);
    assert!(err.contains(".env:1: a value cannot hold a NUL byte, skipped") && !err.contains("panicked"), "{}", err);
}

// Q-836: `--watch` started while a used file was broken watched only the main file, and a fixed
// `lib.qwe` never loaded; a reload that failed for a missing import watched the version before's
// files, and making that file loaded nothing. Now every file the failed load read or tried is
// watched, and the folders of those not there. Q-852: each error is said once, and "1 route".
#[test]
fn watch_reloads_when_a_file_the_failed_load_named_is_fixed_or_made() {
    let d = files_dir("watch-broken", &[("main.qwe", b"import lib.qwe use ver\nroute GET / = str(ver())\n"), ("lib.qwe", b"fn ver() = 1 +\n")]);
    let (_g, port, log) = run_logged(&d, &["main.qwe", "--watch"]);
    assert_eq!(support::send(port, "GET", "/", &[], "").status, 500);
    std::fs::write(d.join("lib.qwe"), "fn ver() = 2\n").unwrap();
    until_logged(&log, "the fixed lib.qwe loads", || body_of(port, "/") == "2");
    let said = std::fs::read_to_string(&log).unwrap();
    assert_eq!(said.matches("expected an expression, found end of input").count(), 1, "{}", said);
    assert!(said.contains("quillo: reloaded main.qwe (1 route)\n"), "{}", said);
    // A reload that fails for a file not there: the version before is served, the error said
    // once, and the file, made, loads it.
    std::fs::write(d.join("main.qwe"), "import lib.qwe use ver\nimport b.qwe use bee\nroute GET / = str(ver()) + bee()\n").unwrap();
    until_logged(&log, "the failed reload is said", || std::fs::read_to_string(&log).unwrap().contains("still serving the previous version"));
    let said = std::fs::read_to_string(&log).unwrap();
    assert_eq!(said.matches("cannot read b.qwe").count(), 1, "{}", said);
    assert_eq!(body_of(port, "/"), "2");
    std::fs::write(d.join("b.qwe"), "fn bee() = \"B\"\n").unwrap();
    until_logged(&log, "the file made loads", || body_of(port, "/") == "2B");
    // In a folder not there either, made with it; then two routes.
    std::fs::write(d.join("main.qwe"), "import lib.qwe use ver\nimport sub/c.qwe use sea\nroute GET / = str(ver()) + sea()\nroute GET /x = \"x\"\n").unwrap();
    until_logged(&log, "the second failed reload is said", || std::fs::read_to_string(&log).unwrap().matches("still serving the previous version").count() == 2);
    std::fs::create_dir(d.join("sub")).unwrap();
    std::fs::write(d.join("sub/c.qwe"), "fn sea() = \"C\"\n").unwrap();
    until_logged(&log, "the file in the folder made loads", || body_of(port, "/") == "2C");
    assert!(std::fs::read_to_string(&log).unwrap().contains("quillo: reloaded main.qwe (2 routes)\n"));
    // The main file broken on its own, as before.
    std::fs::write(d.join("main.qwe"), "route GET / = \n").unwrap();
    until_logged(&log, "the third failed reload is said", || std::fs::read_to_string(&log).unwrap().matches("still serving the previous version").count() == 3);
    std::fs::write(d.join("main.qwe"), "route GET / = \"main again\"\n").unwrap();
    until_logged(&log, "the main file fixed loads", || body_of(port, "/") == "main again");
}

#[test]
fn watch_started_on_a_missing_import_loads_once_it_is_made() {
    let d = files_dir("watch-missing", &[("main.qwe", b"import gone.qwe use g\nroute GET / = g()\n")]);
    let (_g, port, log) = run_logged(&d, &["main.qwe", "--watch", "--no-check"]);
    assert_eq!(support::send(port, "GET", "/", &[], "").status, 500);
    std::fs::write(d.join("gone.qwe"), "fn g() = \"here\"\n").unwrap();
    until_logged(&log, "the file made loads", || body_of(port, "/") == "here");
    assert!(!std::fs::read_to_string(&log).unwrap().contains("every 50 ms"), "its folder is watched");
}

// Q-841: `--reload` with `--instances` started no watcher, and nothing said so. Now each instance
// loads a change, the first checking it: a change that does not check reaches none of them.
#[test]
fn reload_with_instances_reloads_every_instance() {
    let d = files_dir("reload-instances", &[("main.qwe", b"route GET / = \"v=1\"\n")]);
    let (_g, port, log) = run_logged(&d, &["main.qwe", "--reload", "--instances", "2"]);
    assert_eq!(body_of(port, "/"), "v=1");
    std::fs::write(d.join("main.qwe"), "route GET / = \"v=2\"\n").unwrap();
    until_logged(&log, "every instance reloads", || (0..12).all(|_| body_of(port, "/") == "v=2"));
    assert_eq!(std::fs::read_to_string(&log).unwrap().matches("reloaded main.qwe").count(), 1, "said once");
    std::fs::write(d.join("main.qwe"), "route GET / = \"v=\" + \n").unwrap();
    until_logged(&log, "the broken change is said", || std::fs::read_to_string(&log).unwrap().contains("still serving the previous version"));
    assert!((0..12).all(|_| body_of(port, "/") == "v=2"));
    // One instance, as before.
    std::fs::write(d.join("main.qwe"), "route GET / = \"v=2\"\n").unwrap();
    let (_g1, port1, log1) = run_logged(&d, &["main.qwe", "--reload", "--instances", "1"]);
    std::fs::write(d.join("main.qwe"), "route GET / = \"v=3\"\n").unwrap();
    until_logged(&log1, "one instance reloads", || body_of(port1, "/") == "v=3");
    let (code, _, err) = quillo_in(&d, &["run", "main.qwe", "--instances", "2", "--watch"], b"");
    assert_eq!(code, Some(2));
    assert!(err.ends_with("quillo: --instances does not go with --watch or --register\n"), "{}", err);
}

// Q-842: `--split --reload` gave only `--watch` to its services, so a change to a service's file
// was never taken. Now `--reload` goes to them too.
#[test]
fn split_reload_takes_a_change_to_a_services_file() {
    let d = files_dir("split-reload", &[("app.qwe", b"service billing = \"billing.qwe\"\nroute GET / = billing.who()\n"), ("billing.qwe", b"server fn who() = \"billing v1\"\n")]);
    let (_g, port, log) = run_logged(&d, &["app.qwe", "--split", "--reload"]);
    assert_eq!(body_of(port, "/"), "billing v1");
    std::fs::write(d.join("billing.qwe"), "server fn who() = \"billing v2 ✓\"\n").unwrap();
    until_logged(&log, "the service reloads", || body_of(port, "/") == "billing v2 ✓");
    assert!(std::fs::read_to_string(&log).unwrap().contains("[billing] quillo: reloaded billing.qwe"));
}

/// `quillo run args...` in `d` with `--port 0`, its log kept: the guard, the app's port (from its
/// own "listening on" line, not a service's) and the log.
fn run_on_any_port(d: &Path, args: &[&str]) -> (support::Guard, u16, PathBuf) {
    let log = d.join("any-port.log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .args(args)
        .args(["--port", "0"])
        .current_dir(d)
        .env("QUILLO_SECRET", "test secret for the tests")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let g = support::Guard::new(child);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let said = std::fs::read_to_string(&log).unwrap_or_default();
        let port = said.lines().filter(|l| l.starts_with("quillo: ")).find_map(|l| l.split("listening on http://127.0.0.1:").nth(1)).and_then(|p| p.split_whitespace().next()?.parse::<u16>().ok());
        if let Some(port) = port.filter(|p| support::listening(*p)) {
            return (g, port, log);
        }
        assert!(std::time::Instant::now() < deadline, "did not start: {}", said);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

// Q-843: `--split --port 0` told every service the app was at port 0, so a service's channel
// relay went nowhere. Now the services are told the port the app was given.
#[test]
fn split_on_any_port_tells_the_services_the_port_given() {
    let d = files_dir("split-port0", &[("app.qwe", b"service billing = \"billing.qwe\"\nroute GET / = billing.who()\n"), ("billing.qwe", b"server fn who() = env(\"QUILLO_APP_URL\") ?? \"none\"\n")]);
    let (_g, port, _log) = run_on_any_port(&d, &["app.qwe", "--split"]);
    assert_ne!(port, 0);
    assert_eq!(body_of(port, "/"), format!("http://127.0.0.1:{}", port));
    // A port named is told as it was.
    let (_g2, named, _) = run_logged(&d, &["app.qwe", "--split"]);
    assert_eq!(body_of(named, "/"), format!("http://127.0.0.1:{}", named));
    // An address that cannot be listened on is said before any service starts.
    let (code, _, err) = quillo_in(&d, &["run", "app.qwe", "--split", "--port", "0", "--host", "203.0.113.1"], b"");
    assert_eq!(code, Some(1), "{}", err);
    assert!(err.starts_with("quillo: cannot listen on 203.0.113.1:0: ") && !err.contains("service billing"), "{}", err);
}

// Q-837: `quillo fix` wrote its import before a file's byte-order mark, and the file no longer
// parsed; the line it added was LF in a CRLF file. Now the mark stays first and the lines added
// end as the file's do.
#[test]
fn fix_writes_after_a_byte_order_mark_with_the_files_line_ends() {
    let main: &[u8] = b"import lib.qwe use ver\nimport ui.qwe use page\nroute GET / = page()\n";
    let d = files_dir("fix-bom", &[("lib.qwe", b"fn ver() = 1\n"), ("main.qwe", main), ("ui.qwe", b"\xef\xbb\xbf// ui\r\nfn page() {\r\n  str(ver())\r\n}")]);
    let (code, out, err) = quillo_in(&d, &["fix", "main.qwe"], b"");
    assert_eq!(code, Some(0), "{}{}", out, err);
    assert_eq!(std::fs::read(d.join("ui.qwe")).unwrap(), b"\xef\xbb\xbf// ui\r\nimport lib.qwe use ver\r\n\r\nfn page() {\r\n  str(ver())\r\n}");
    let (code, out, err) = quillo_in(&d, &["check", "main.qwe"], b"");
    assert_eq!(code, Some(0), "{}{}", out, err);
    assert!(!err.contains("U+FEFF"), "{}", err);
    // A file that starts with an import, after its mark: the new one goes below it.
    std::fs::write(d.join("two.qwe"), "fn two() = 2\n").unwrap();
    std::fs::write(d.join("ui.qwe"), b"\xef\xbb\xbfimport lib.qwe use ver\r\nfn page() {\r\n  str(ver()) + str(two())\r\n}\r\n").unwrap();
    std::fs::write(d.join("main.qwe"), "import lib.qwe use ver\nimport ui.qwe use page\nimport two.qwe use two\nroute GET / = page()\n").unwrap();
    assert_eq!(quillo_in(&d, &["fix", "main.qwe"], b"").0, Some(0));
    assert_eq!(std::fs::read(d.join("ui.qwe")).unwrap(), b"\xef\xbb\xbfimport lib.qwe use ver\r\nimport two.qwe use two\r\nfn page() {\r\n  str(ver()) + str(two())\r\n}\r\n");
    // CRLF without a mark, and a plain LF file, as before.
    std::fs::write(d.join("ui.qwe"), "fn page() {\r\n  str(ver())\r\n}\r\n").unwrap();
    std::fs::write(d.join("main.qwe"), main).unwrap();
    assert_eq!(quillo_in(&d, &["fix", "main.qwe"], b"").0, Some(0));
    assert_eq!(std::fs::read_to_string(d.join("ui.qwe")).unwrap(), "import lib.qwe use ver\r\n\r\nfn page() {\r\n  str(ver())\r\n}\r\n");
    std::fs::write(d.join("ui.qwe"), "// ünï\nfn page() {\n  str(ver())\n}").unwrap();
    assert_eq!(quillo_in(&d, &["fix", "main.qwe"], b"").0, Some(0));
    assert_eq!(std::fs::read_to_string(d.join("ui.qwe")).unwrap(), "// ünï\nimport lib.qwe use ver\n\nfn page() {\n  str(ver())\n}");
}

/// `quillo lsp`, its messages read on a thread of their own so that a test waits on a deadline.
struct Lsp {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    got: std::sync::mpsc::Receiver<serde_json::Value>,
    next: u64,
}

impl Lsp {
    fn start() -> Lsp {
        use std::io::{BufRead, Read};
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("lsp")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        support::stop_with_tests(&child);
        let stdin = child.stdin.take().unwrap();
        let mut out = std::io::BufReader::new(child.stdout.take().unwrap());
        let (tx, got) = std::sync::mpsc::channel();
        std::thread::spawn(move || loop {
            let mut len = 0;
            loop {
                let mut line = String::new();
                if out.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                if line.trim_end().is_empty() {
                    break;
                }
                if let Some((_, v)) = line.split_once(':').filter(|(n, _)| n.eq_ignore_ascii_case("content-length")) {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            if out.read_exact(&mut body).is_err() || tx.send(serde_json::from_slice(&body).unwrap()).is_err() {
                return;
            }
        });
        Lsp { child, stdin, got, next: 1 }
    }

    fn raw(&mut self, bytes: &[u8]) {
        use std::io::Write;
        self.stdin.write_all(bytes).unwrap();
        self.stdin.flush().unwrap();
    }

    fn send(&mut self, msg: serde_json::Value) {
        let body = msg.to_string();
        self.raw(format!("Content-Length: {}\r\n\r\n{}", body.len(), body).as_bytes());
    }

    fn next(&self) -> serde_json::Value {
        self.got.recv_timeout(std::time::Duration::from_secs(20)).expect("the language server answers")
    }

    /// The answer to a request, whole (`result` or `error`), the notifications before it passed over.
    fn ask(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let id = self.next;
        self.next += 1;
        self.send(serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        self.answer(id)
    }

    fn answer(&self, id: u64) -> serde_json::Value {
        loop {
            let m = self.next();
            if m["id"] == serde_json::json!(id) {
                return m;
            }
        }
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) {
        self.send(serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn open(&mut self, uri: &str, text: &str) {
        self.notify("textDocument/didOpen", serde_json::json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } }));
    }

    /// The diagnostics published for each URI until a request asked after them is answered.
    fn published(&mut self) -> std::collections::HashMap<String, Vec<String>> {
        let id = self.next;
        self.next += 1;
        self.send(serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": "textDocument/documentSymbol", "params": { "textDocument": { "uri": "file:///nowhere.qwe" } } }));
        let mut out = std::collections::HashMap::new();
        loop {
            let m = self.next();
            if m["id"] == serde_json::json!(id) {
                return out;
            }
            if m["method"] == "textDocument/publishDiagnostics" {
                let msgs = m["params"]["diagnostics"].as_array().unwrap().iter().map(|d| d["message"].as_str().unwrap().to_string()).collect();
                out.insert(m["params"]["uri"].as_str().unwrap().to_string(), msgs);
            }
        }
    }
}

impl Drop for Lsp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// Q-844: a project opened through a link got no diagnostics: they went to the real path, and
// were of the file on disk, not the editor's text. Now open documents are known by their real
// paths too, and diagnostics go to the URI the editor opened, from what it holds.
#[cfg(unix)]
#[test]
fn lsp_a_project_opened_through_a_link_gets_its_diagnostics() {
    let d = files_dir("lsp-link", &[("prog/main.qwe", b"import lib.qwe use ver\nroute GET / = str(ver())\n"), ("prog/lib.qwe", b"fn ver() = 1\n")]);
    std::os::unix::fs::symlink(d.join("prog"), d.join("proglink")).unwrap();
    let real = d.canonicalize().unwrap();
    let uri = format!("file://{}/proglink/lib.qwe", d.display());
    let mut s = Lsp::start();
    s.ask("initialize", serde_json::json!({ "capabilities": {} }));
    // Unsaved text with an error, the file on disk without.
    s.open(&uri, "fn ver() = 1 +\n");
    let got = s.published();
    assert!(got.get(&uri).is_some_and(|m| m.len() == 1 && m[0].contains("expected an expression")), "{:?}", got);
    assert!(!got.keys().any(|u| u.contains("/prog/lib.qwe")), "nothing under the real path: {:?}", got);
    // Fixed in the editor, though the disk is not: none.
    std::fs::write(d.join("prog/lib.qwe"), "fn ver() = 1 +\n").unwrap();
    s.notify("textDocument/didChange", serde_json::json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": "fn ver() = 2\n" }] }));
    let got = s.published();
    assert_eq!(got.get(&uri), Some(&Vec::new()), "{:?}", got);
    // Opened by its real path, as before: the same, under that URI.
    let real_uri = format!("file://{}/prog/lib.qwe", real.display());
    s.notify("textDocument/didClose", serde_json::json!({ "textDocument": { "uri": uri } }));
    s.open(&real_uri, "fn ver() = 1 +\n");
    let got = s.published();
    assert!(got.get(&real_uri).is_some_and(|m| m.len() == 1 && m[0].contains("expected an expression")), "{:?}", got);
    s.notify("textDocument/didClose", serde_json::json!({ "textDocument": { "uri": real_uri } }));
    // Both files open through the link: what the program says of the main file is left to the
    // main file's own check, not published again under its real path.
    std::fs::write(d.join("prog/lib.qwe"), "fn ver() = 1\n").unwrap();
    let main_uri = format!("file://{}/proglink/main.qwe", d.display());
    s.open(&main_uri, "import lib.qwe use ver\nroute GET / = str(ver())\n");
    s.open(&uri, "fn ver() = 1\n");
    s.published();
    s.notify("textDocument/didChange", serde_json::json!({ "textDocument": { "uri": uri, "version": 4 }, "contentChanges": [{ "text": "fn other() = 2\n" }] }));
    let got = s.published();
    assert_eq!(got.get(&uri), Some(&Vec::new()), "{:?}", got);
    assert!(got.keys().all(|u| u == &uri || u == &main_uri), "{:?}", got);
}

// Review of slice 13 (Q-844): one file open through two links is two documents of one file: what
// is said of it is said under both URIs.
#[cfg(unix)]
#[test]
fn lsp_one_file_open_through_two_links_is_told_under_both() {
    let d = files_dir("lsp-two-links", &[("prog/main.qwe", b"import lib.qwe use ver\nroute GET / = str(ver())\n"), ("prog/lib.qwe", b"fn ver() = 1\n")]);
    for l in ["l1", "l2"] {
        std::os::unix::fs::symlink(d.join("prog"), d.join(l)).unwrap();
    }
    let (u1, u2) = (format!("file://{}/l1/lib.qwe", d.display()), format!("file://{}/l2/lib.qwe", d.display()));
    let mut s = Lsp::start();
    s.ask("initialize", serde_json::json!({ "capabilities": {} }));
    s.open(&u1, "fn ver() = 1\n");
    assert_eq!(s.published().get(&u1), Some(&Vec::new()));
    s.open(&u2, "fn ver() = 1 +\n");
    let got = s.published();
    for u in [&u1, &u2] {
        assert!(got.get(u).is_some_and(|m| m.len() == 1 && m[0].contains("expected an expression")), "{}: {:?}", u, got);
    }
    // Fixed in one: cleared under both.
    s.notify("textDocument/didChange", serde_json::json!({ "textDocument": { "uri": u1, "version": 2 }, "contentChanges": [{ "text": "fn ver() = 2\n" }] }));
    let got = s.published();
    assert_eq!((got.get(&u1), got.get(&u2)), (Some(&Vec::new()), Some(&Vec::new())), "{:?}", got);
    // Closed in one: the other alone.
    s.notify("textDocument/didClose", serde_json::json!({ "textDocument": { "uri": u1 } }));
    s.published();
    s.notify("textDocument/didChange", serde_json::json!({ "textDocument": { "uri": u2, "version": 3 }, "contentChanges": [{ "text": "fn ver() = \n" }] }));
    let got = s.published();
    assert!(got.get(&u2).is_some_and(|m| m.len() == 1) && !got.contains_key(&u1), "{:?}", got);
}

// Q-845: a `content-length` header in another case was no length, and the body was read as
// headers: that request and every one after it went unanswered. Q-852: before `initialize` and
// after `shutdown` requests were answered as usual, and a body that was not JSON got nothing.
#[test]
fn lsp_reads_headers_in_any_case_and_answers_out_of_turn_as_the_protocol_asks() {
    let mut s = Lsp::start();
    let early = s.ask("textDocument/hover", serde_json::json!({}));
    assert_eq!(early["error"]["code"], -32002, "{}", early);
    // A notification then is dropped: the next answer is the request's.
    s.open("file:///early.qwe", "let x = ");
    let body = serde_json::json!({ "jsonrpc": "2.0", "id": 50, "method": "initialize", "params": { "capabilities": {} } }).to_string();
    s.raw(format!("content-length: {}\r\n\r\n{}", body.len(), body).as_bytes());
    let init = s.next();
    assert_eq!((init["id"].clone(), init["result"]["serverInfo"]["name"].clone()), (serde_json::json!(50), serde_json::json!("quillo")), "{}", init);
    for header in ["CONTENT-LENGTH:{}", "Content-length:{}", "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\ncontent-LENGTH :  {}"] {
        let id = s.next;
        s.next += 1;
        let body = serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": "textDocument/documentSymbol", "params": { "textDocument": { "uri": "file:///ünï.qwe" } } }).to_string();
        s.raw(format!("{}\r\n\r\n{}", header.replace("{}", &body.len().to_string()), body).as_bytes());
        assert!(s.answer(id).get("result").is_some(), "{}", header);
    }
    // Not JSON: -32700, and the next request is answered.
    s.raw(b"Content-Length: 5\r\n\r\n{bad}");
    let bad = s.next();
    assert_eq!((bad["error"]["code"].clone(), bad["id"].clone()), (serde_json::json!(-32700), serde_json::Value::Null), "{}", bad);
    assert!(s.ask("textDocument/documentSymbol", serde_json::json!({ "textDocument": { "uri": "file:///a.qwe" } })).get("result").is_some());
    assert_eq!(s.ask("shutdown", serde_json::Value::Null)["result"], serde_json::Value::Null);
    let late = s.ask("textDocument/hover", serde_json::json!({}));
    assert_eq!(late["error"]["code"], -32600, "{}", late);
    s.notify("exit", serde_json::Value::Null);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let status = loop {
        if let Some(st) = s.child.try_wait().unwrap() {
            break st;
        }
        assert!(std::time::Instant::now() < deadline, "exit ends it");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(0));
}

// Q-847: `quillo serve` read its options its own way: `--port=8080` was an unknown option and
// `--with --watch` started "--watch". Now it reads them as `run` does.
#[test]
fn quillo_serve_reads_its_options_as_run_does() {
    let d = files_dir("serve-options", &[("site/index.html", b"<p>home</p>")]);
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["serve", &format!("--port={}", port), "--workers=4", "--log=json", "site"])
        .current_dir(&d)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !support::listening(port) {
        assert!(std::time::Instant::now() < deadline, "quillo serve --port= did not start");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(body_of(port, "/"), "<p>home</p>");
    for (args, said) in [
        (&["serve", "site", "--with", "--watch"][..], "quillo: `--with` needs a value, and `--watch` is an option\n"),
        (&["serve", "site", "--port"][..], "quillo: `--port` needs a value\n"),
        (&["serve", "site", "extra"][..], "quillo: unknown argument `extra`: `serve` takes one folder\n"),
        (&["serve", "--port", "1", "site", "extra"][..], "quillo: unknown argument `extra`: `serve` takes one folder\n"),
        (&["serve", "site", "--watch=1"][..], "quillo: `--watch` takes no value: write `--watch` alone\n"),
        (&["serve", "site", "--port=abc"][..], "quillo: --port is a number from 1 to 65535 (or 0 for any free one), got \"abc\"\n"),
        (&["serve", "nowhere"][..], "quillo: nowhere is not a folder: `quillo serve` serves a folder's files\n"),
    ] {
        assert_eq!(quillo_in(&d, args, b""), (Some(2), String::new(), said.to_string()), "{:?}", args);
    }
}

// Q-848: the REPL fed from a pipe took a line that was not UTF-8 for the end of its input, and
// stopped with 0. Now the line is said, by number, and skipped; the rest runs, and it ends with 1.
#[test]
fn the_repl_says_a_line_that_is_not_utf8_and_goes_on() {
    let d = files_dir("repl-utf8", &[]);
    let (code, out, err) = quillo_in(&d, &["repl"], b"print(\"one\")\nprint(\"\xe9\")\nprint(\"three\")\n");
    assert_eq!((code, out.as_str(), err.as_str()), (Some(1), "one\nthree\n", "error: line 2 is not UTF-8 text (byte 0xE9 at column 8): skipped\n"));
    // The first line, and a last one with no line end.
    let (code, out, err) = quillo_in(&d, &["repl"], b"\xff\nprint(1)\n\xc3");
    assert_eq!((code, out.as_str()), (Some(1), "1\n"), "{}", err);
    assert!(err.contains("line 1 is not UTF-8 text (byte 0xFF at column 1)") && err.contains("line 3 is not UTF-8 text (byte 0xC3 at column 1)"), "{}", err);
    // UTF-8 past ASCII, as before; and with -i the prompts go on after the line, which ends with
    // 0 as its other errors do.
    assert_eq!(quillo_in(&d, &["repl"], "print(\"é ✓\")\n".as_bytes()), (Some(0), "é ✓\n".into(), String::new()));
    let (code, out, err) = quillo_in(&d, &["repl", "-i"], b"print(\"\xe9\")\nprint(2)\n");
    assert_eq!(code, Some(0));
    assert!(out.contains("quillo> 2\n") && err.contains("line 1 is not UTF-8 text"), "{}{}", out, err);
    // Q-852: what `repl` does not take is said, not passed over.
    assert_eq!(quillo_in(&d, &["repl", "hello.qwe"], b""), (Some(2), String::new(), "quillo: unknown argument `hello.qwe`: `repl` takes no file; to run it: quillo run hello.qwe\n".into()));
    assert_eq!(quillo_in(&d, &["repl", "--bogus"], b""), (Some(2), String::new(), "quillo: unknown option `--bogus` for `repl` (it takes -i)\n".into()));
    assert_eq!(quillo_in(&d, &["repl", "-i", "x"], b"").0, Some(2));
}

// Q-849: a policy took limits of 0 that the command line refuses: `max_body = 0` answered every
// POST 413, `max_compute = "0s"` ended every request; and the message named `--max-compute`
// though none was given. Now they are refused as the options are, and the message and the start
// name the policy.
#[test]
fn a_policy_takes_the_limits_the_command_line_takes() {
    let d = files_dir("policy-zero", &[("main.qwe", b"route GET / = \"hi\"\n")]);
    let policy = |text: &str| {
        std::fs::write(d.join("p.toml"), text).unwrap();
        quillo_in(&d, &["check", "main.qwe", "--policy", "p.toml"], b"")
    };
    for (text, said) in [
        ("[limits]\nmax_body = 0\n", "p.toml:2: `max_body` is a size larger than 0"),
        ("[limits]\nmax_body = \"0KB\"\n", "p.toml:2: `max_body` is a size larger than 0"),
        ("[limits]\nmax_compute = \"0s\"\n", "p.toml:2: `max_compute` is a time longer than 0"),
        ("[limits]\nmax_compute = \"0ms\"\n", "p.toml:2: `max_compute` is a time longer than 0"),
        ("[limits]\nmax_compute = \"0\"\n", "p.toml:2: `max_compute` is a time longer than 0"),
        ("[limits]\nmax_compute = \"99999999999999999m\"\n", "p.toml:2: `99999999999999999m` is not a time"),
        ("[limits]\nworkers = 0\n", "p.toml:2: `workers` is a positive number"),
        ("[limits]\nworkers = -1\n", "p.toml:2: `workers` is a positive number"),
    ] {
        let (code, _, err) = policy(text);
        assert_eq!(code, Some(2), "{:?}: {}", text, err);
        assert!(err.contains(said), "{:?}: {}", text, err);
    }
    // The least each takes, and the start says what holds the program.
    let (code, _, err) = policy("[limits]\nmax_body = 1\nmax_compute = \"1ms\"\nworkers = 1\n");
    assert_eq!(code, Some(0), "{}", err);
    assert!(err.contains("quillo: policy p.toml (workers 1; max_compute 1ms; max_body 1 byte)"), "{}", err);
    let (_, _, err) = policy("[limits]\nmax_body = \"2KB\"\n");
    assert!(err.contains("(max_body 2048 bytes)"), "{}", err);
    // Q-852: a byte-order mark is no part of its first line.
    let (code, _, err) = policy("\u{FEFF}[limits]\nworkers = 2\n");
    assert_eq!(code, Some(0), "{}", err);
    assert!(err.contains("(workers 2)"), "{}", err);
}

#[test]
fn a_request_ended_by_the_policys_limit_names_the_policy() {
    // Long enough to take several of the watchdog's 100 ms looks.
    let big = "let x = \"a\" + 1; // note\n".repeat(800_000);
    let d = files_dir("policy-names", &[("main.qwe", b"route GET /h = highlight(read_file(\"big.js\"), \"js\")\nroute GET / = \"hi\"\n"), ("big.js", big.as_bytes()), ("p.toml", b"[limits]\nmax_compute = \"10ms\"\n")]);
    let ended = |args: &[&str]| {
        let (_g, port, log) = run_logged(&d, args);
        assert_eq!(support::send(port, "GET", "/h", &[], "").status, 500);
        assert_eq!(body_of(port, "/"), "hi", "it goes on");
        let mut said = String::new();
        until_logged(&log, "the request's error", || {
            said = std::fs::read_to_string(&log).unwrap();
            said.contains("computed for longer than")
        });
        said
    };
    assert!(ended(&["main.qwe", "--policy", "p.toml"]).contains("this request computed for longer than the policy p.toml allows (max_compute)"));
    assert!(ended(&["main.qwe", "--max-compute", "10ms"]).contains("this request computed for longer than --max-compute allows"));
    // The tighter of the two is the one said.
    assert!(ended(&["main.qwe", "--max-compute", "5ms", "--policy", "p.toml"]).contains("longer than --max-compute allows"));
    assert!(ended(&["main.qwe", "--max-compute", "1m", "--policy", "p.toml"]).contains("longer than the policy p.toml allows"));
}

// Review of slice 13 (Q-849): the stop of a request past the policy's limit is known by its own
// words, as `--max-compute`'s is: `/_quillo/markdown` answers it 500, as a route's, not 400, and
// tells the page no path of the server's (the log has it).
#[test]
fn a_preview_past_the_policys_limit_ends_as_a_server_error_naming_no_path() {
    let d = files_dir("policy-preview", &[("main.qwe", b"route GET / = \"hello\"\n"), ("p.toml", b"[limits]\nmax_compute = \"1ms\"\n")]);
    let (_g, port, log) = run_logged(&d, &["main.qwe", "--policy", "p.toml"]);
    let mut text = String::new();
    while text.len() < 900_000 {
        text.push_str("```js\nlet a = f(1) // c\n```\n");
    }
    let body = serde_json::json!([text]).to_string();
    let mut ended = false;
    for _ in 0..200 {
        let a = support::send(port, "POST", "/_quillo/markdown", &[], &body);
        match a.status {
            200 => {}
            500 => {
                assert_eq!(a.body, r#"{"error":"this request computed for longer than the server's policy allows (max_compute)"}"#);
                ended = true;
                break;
            }
            s => panic!("{}: {}", s, a.body),
        }
    }
    assert!(ended, "no preview was found past the limit");
    until_logged(&log, "the log names the policy", || std::fs::read_to_string(&log).unwrap().contains("this request computed for longer than the policy p.toml allows (max_compute)"));
    assert_eq!(body_of(port, "/"), "hello");
}

// Review of slice 13 (Q-849): an ordinary loop ended by the policy's limit names the policy, and
// `??` takes that stop no more than `--max-compute`'s. Needs the interp.rs patch (within_limit and
// guardable), applied once slice 9 is in: scratchpad/f55/fix/13/q849-interp.patch.
#[test]
fn a_loop_past_the_policys_limit_names_it_and_no_guard_takes_the_stop() {
    let big = "let x = \"a\" + 1; // note\n".repeat(800_000);
    let d = files_dir(
        "policy-loop",
        &[
            ("main.qwe", b"route GET /forever {\n    let i = 0\n    while i >= 0 { i = i + 1 }\n    \"never\"\n}\nroute GET /guarded = highlight(read_file(\"big.js\"), \"js\") ?? \"taken\"\nroute GET /md = markdown(\"```js\\n\" + read_file(\"big.js\") + \"\\n```\") ?? \"taken\"\nroute GET / = \"hi\"\n"),
            ("big.js", big.as_bytes()),
            ("p.toml", b"[limits]\nmax_compute = \"10ms\"\n"),
        ],
    );
    for opts in [&["main.qwe", "--policy", "p.toml"][..], &["main.qwe", "--max-compute", "10ms"]] {
        let (_g, port, log) = run_logged(&d, opts);
        let said = if opts.contains(&"--policy") { "this request computed for longer than the policy p.toml allows (max_compute)" } else { "this request computed for longer than --max-compute allows" };
        for path in ["/forever", "/guarded", "/md"] {
            let a = support::send(port, "GET", path, &[], "");
            assert_eq!(a.status, 500, "{} {:?}: {}", path, opts, a.body);
            until_logged(&log, path, || std::fs::read_to_string(&log).unwrap().lines().any(|l| l.contains(&format!("GET {} ", path)) && l.contains(said)));
        }
        assert_eq!(body_of(port, "/"), "hi");
    }
}

// Q-850: `quillo new` into an empty folder that is there made its folder beside it, and needed
// to write in the parent. Now it builds inside the folder; one that is not there is still made
// beside it.
#[cfg(unix)]
#[test]
fn quillo_new_builds_inside_an_empty_folder_whose_parent_it_cannot_write() {
    use std::os::unix::fs::PermissionsExt;
    let d = files_dir("new-locked", &[]);
    for f in ["locked/empty", "locked/withgit/.git"] {
        std::fs::create_dir_all(d.join(f)).unwrap();
    }
    std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let writable = std::fs::write(d.join("locked/probe"), "").is_ok();
    let (code, out, err) = quillo_in(&d.join("locked/empty"), &["new", ".", "--minimal"], b"");
    let made = |dir: &Path| std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(code, Some(0), "{}{}", out, err);
    assert_eq!(made(&d.join("locked/empty")), [".gitignore", "lib", "main.qwe", "ui"].iter().map(|s| s.to_string()).collect());
    let (code, _, err) = quillo_in(&d.join("locked"), &["new", "withgit", "--minimal"], b"");
    assert_eq!(code, Some(0), "{}", err);
    assert_eq!(made(&d.join("locked/withgit")), [".git", ".gitignore", "lib", "main.qwe", "ui"].iter().map(|s| s.to_string()).collect());
    // One that is not there needs the parent, and says so.
    if !writable {
        let (code, _, err) = quillo_in(&d.join("locked"), &["new", "fresh"], b"");
        assert_eq!(code, Some(1));
        assert!(err.contains("Permission denied") && !d.join("locked/fresh").exists(), "{}", err);
    }
    std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();
    // And one not empty is refused, as before.
    let (code, _, err) = quillo_in(&d.join("locked"), &["new", "empty"], b"");
    assert_eq!(code, Some(1));
    assert!(err.contains("is not empty"), "{}", err);
    let (code, _, _) = quillo_in(&d, &["new", "fresh", "--minimal"], b"");
    assert_eq!(code, Some(0));
    assert!(d.join("fresh/main.qwe").is_file() && !std::fs::read_dir(&d).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("quillo-new")));
    // What a `new` that was stopped left building inside is cleared, when its process is gone;
    // one whose process runs is someone's, and the folder is not empty (review of slice 13).
    let mut ended = Command::new("true").spawn().unwrap();
    let gone = ended.id();
    ended.wait().unwrap();
    std::fs::create_dir_all(d.join(format!("stale/.quillo-new-{}/lib", gone))).unwrap();
    let (code, _, err) = quillo_in(&d.join("stale"), &["new", ".", "--minimal"], b"");
    assert_eq!(code, Some(0), "{}", err);
    assert_eq!(made(&d.join("stale")), [".gitignore", "lib", "main.qwe", "ui"].iter().map(|s| s.to_string()).collect());
    let busy = format!(".quillo-new-{}", std::process::id());
    std::fs::create_dir_all(d.join("busy").join(&busy)).unwrap();
    let (code, _, err) = quillo_in(&d.join("busy"), &["new", ".", "--minimal"], b"");
    assert_eq!(code, Some(1));
    assert!(err.contains(&format!("is not empty: it has {}", busy)), "{}", err);
    std::fs::create_dir_all(d.join("busy/.quillo-new-x")).unwrap();
    assert!(quillo_in(&d.join("busy"), &["new", ".", "--minimal"], b"").2.contains(".quillo-new-x"), "a name of no process is kept");
    // An empty folder it cannot write in says so.
    std::fs::create_dir(d.join("ro")).unwrap();
    std::fs::set_permissions(d.join("ro"), std::fs::Permissions::from_mode(0o555)).unwrap();
    if std::fs::write(d.join("ro/probe"), "").is_err() {
        let (code, _, err) = quillo_in(&d, &["new", "ro"], b"");
        assert_eq!(code, Some(1));
        assert!(err.contains("cannot write in ro, to make the program there: Permission denied"), "{}", err);
    }
    std::fs::set_permissions(d.join("ro"), std::fs::Permissions::from_mode(0o755)).unwrap();
}

const USAGE_START: &str = "Quillo — a small web language\n";

// Q-851: `run -h` without a main.qwe said "no program", `lsp --help` started the server,
// `install -h` printed to stderr, and an option before the command printed the whole usage. Now
// `-h` and `--help` are read first, for every command, to stdout; an option before the command
// is told in one line where the command goes.
#[test]
fn help_is_read_first_for_every_command_and_on_stdout() {
    let d = files_dir("help-first", &[("tool.qwe", b"print(args())\n")]);
    for args in [&["run", "-h"][..], &["fcgi", "--help"], &["lsp", "--help"], &["repl", "--help"], &["dap", "--help"], &["serve", "--help"], &["check", "-h"], &["docs", "--help"], &["classes", "-h"], &["build", "-h"], &["fix", "-h"], &["app", "--help"], &["tool.qwe", "-h"], &["run", "--port", "1", "--help"]] {
        let (code, out, err) = quillo_in(&d, args, b"");
        assert_eq!(code, Some(0), "{:?}: {}", args, err);
        assert!(out.starts_with(USAGE_START) && err.is_empty(), "{:?}: {}{}", args, out, err);
    }
    let (code, out, err) = quillo_in(&d, &["install", "-h"], b"");
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert!(out.starts_with("usage:\n  quillo install <name>[@version]..."), "{}", out);
    let (code, out, _) = quillo_in(&d, &["new", "--help"], b"");
    assert!(code == Some(0) && out.starts_with("usage: quillo new <folder>"), "{}", out);
    // A program's own `--help`, after `--`, is the program's.
    assert_eq!(quillo_in(&d, &["run", "tool.qwe", "--", "--help"], b""), (Some(0), "[\"--help\"]\n".into(), String::new()));
    // An option before the command: one line.
    for (args, said) in [
        (&["--watch", "main.qwe"][..], "quillo: put the command first: `quillo run --watch main.qwe`\n"),
        (&["--port", "0", "srv.qwe"], "quillo: put the command first: `quillo run --port 0 srv.qwe`\n"),
        (&["-w"], "quillo: put the command first: `quillo run -w`\n"),
        (&["--port=0", "my app.qwe"], "quillo: put the command first: `quillo run --port=0 'my app.qwe'`\n"),
        (&["--strict-null", "main.qwe"], "quillo: put the command first: `quillo check --strict-null main.qwe`\n"),
        // A command after the options is the one put first; an option's value is no command.
        (&["--watch", "run", "tool.qwe"], "quillo: put the command first: `quillo run --watch tool.qwe`\n"),
        (&["--policy", "p.toml", "check", "x.qwe"], "quillo: put the command first: `quillo check --policy p.toml x.qwe`\n"),
        (&["--strict-null", "check", "x.qwe"], "quillo: put the command first: `quillo check --strict-null x.qwe`\n"),
        (&["--port=0", "serve", "site"], "quillo: put the command first: `quillo serve --port=0 site`\n"),
        (&["--service", "run", "x.qwe"], "quillo: put the command first: `quillo run --service run x.qwe`\n"),
        (&["--bogus"], "quillo: unknown option `--bogus`; `quillo help` lists the commands and their options\n"),
        (&["--wach"], "quillo: unknown option `--wach` (did you mean `--watch`?); `quillo help` lists the commands and their options\n"),
    ] {
        assert_eq!(quillo_in(&d, args, b""), (Some(2), String::new(), said.to_string()), "{:?}", args);
    }
    // What already worked.
    for args in [&["-h"][..], &["--help"], &["help"]] {
        let (code, out, _) = quillo_in(&d, args, b"");
        assert!(code == Some(0) && out.starts_with(USAGE_START), "{:?}", args);
    }
    assert_eq!(quillo_in(&d, &["--version"], b"").0, Some(0));
}

// Q-852, the smaller ones of the command line.
#[test]
fn misspelt_commands_are_told_every_command_and_help_names_env_file() {
    let d = files_dir("commands-near", &[]);
    for (typed, near) in [("fcg", "fcgi"), ("serv", "serve"), ("classe", "classes"), ("chek", "check"), ("srve", "serve")] {
        let (code, _, err) = quillo_in(&d, &[typed], b"");
        assert_eq!(code, Some(2));
        assert!(err.contains(&format!("unknown command `{}` (did you mean `{}`?)", typed, near)), "{}", err);
    }
    let (_, out, _) = quillo_in(&d, &["help"], b"");
    assert!(out.contains("\n  --env-file F   "), "{}", out);
}

#[test]
fn tls_cert_given_twice_takes_the_last_and_checks_it() {
    let d = files_dir("tls-twice", &[("main.qwe", b"print(\"ran\")\n"), ("cert.pem", b"cert"), ("key.pem", b"key")]);
    let (code, _, err) = quillo_in(&d, &["run", "main.qwe", "--tls-cert", "cert.pem", "--tls-cert", "missing.pem", "--tls-key", "key.pem"], b"");
    assert_eq!((code, err.as_str()), (Some(2), "quillo: --tls-cert missing.pem: there is no such file\n"));
    assert_eq!(quillo_in(&d, &["run", "main.qwe", "--tls-cert", "missing.pem", "--tls-cert", "cert.pem", "--tls-key", "key.pem"], b""), (Some(0), "ran\n".into(), String::new()));
    let (code, _, err) = quillo_in(&d, &["run", "main.qwe", "--tls-cert", "cert.pem", "--tls-key", "key.pem", "--tls-key", "gone.pem"], b"");
    assert_eq!((code, err.as_str()), (Some(2), "quillo: --tls-key gone.pem: there is no such file\n"));
    assert_eq!(quillo_in(&d, &["run", "main.qwe", "--tls-cert", "cert.pem"], b"").2, "quillo: --tls-cert and --tls-key go together\n");
    // The last of each is a pair, or refused as it starts (review of slice 13).
    for n in ["a", "b"] {
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap().self_signed(&key).unwrap();
        std::fs::write(d.join(format!("{}.pem", n)), cert.pem()).unwrap();
        std::fs::write(d.join(format!("{}.key", n)), key.serialize_pem()).unwrap();
    }
    let (code, _, err) = quillo_in(&d, &["run", "main.qwe", "--tls-cert", "a.pem", "--tls-key", "a.key", "--tls-cert", "b.pem"], b"");
    assert_eq!((code, err.as_str()), (Some(2), "quillo: --tls-cert b.pem and --tls-key a.key are no pair: the key is not the certificate's\n"));
    let (code, _, err) = quillo_in(&d, &["run", "main.qwe", "--tls-key", "b.key", "--tls-cert", "a.pem"], b"");
    assert_eq!((code, err.as_str()), (Some(2), "quillo: --tls-cert a.pem and --tls-key b.key are no pair: the key is not the certificate's\n"));
    for (c, k) in [("a.pem", "a.key"), ("b.pem", "b.key")] {
        assert_eq!(quillo_in(&d, &["run", "main.qwe", "--tls-cert", "b.pem", "--tls-cert", c, "--tls-key", k], b""), (Some(0), "ran\n".into(), String::new()), "{}", c);
    }
}

#[test]
fn classes_and_build_say_where_a_file_is_not_utf8() {
    let d = files_dir("classes-utf8", &[("main.qwe", b"route GET / = <p.a>x</p>\n// \xe9\n")]);
    for cmd in ["classes", "build"] {
        let (code, _, err) = quillo_in(&d, &[cmd, "main.qwe"], b"");
        assert_eq!((code, err.as_str()), (Some(1), "quillo: cannot read main.qwe: it is not UTF-8 text: byte 0xE9 at line 2, column 4 (save it as UTF-8)\n"), "{}", cmd);
    }
    std::fs::write(d.join("main.qwe"), b"\xef\xbb\xbfroute GET / = <p.a>\xc3\xbc</p>\n").unwrap();
    assert_eq!(quillo_in(&d, &["classes", "main.qwe"], b""), (Some(0), "a\n".into(), String::new()));
}
