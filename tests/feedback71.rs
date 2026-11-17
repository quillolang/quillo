//! FEEDBACK71.md, the security audit of every entry point (2026-11-16): the fixes made on branch
//! feedback71. Q-968: a path of the site's own whose dot segments hid a second `/` (`/.//evil.com`)
//! was sent as `Location: //evil.com`, another host, through the guide's own
//! `redirect(local_path(next) ?? "/")`; `local_path` let such a path through, on the server and in
//! the browser. Q-969: a `raw(...)` in a `<script>` inside `<svg>` or `<math>` was read by the
//! browser as markup, so a visitor's `<img onerror>` in it ran. Q-979: a spread's `name` (or `id`)
//! that `document` took for one of its own properties stopped every component on the page.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("feedback71-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

/// What `quillo run` prints for a script, under a deadline.
fn run_script(d: &Path) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the script did not end");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let o = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

// ---- Q-968: redirect and local_path ----------------------------------------------------------

const REDIRECTS: &str = r#"route GET /to ? { u: str } = redirect(u)
route GET /go ? { next: str = "/" } = redirect(local_path(next) ?? "/")
route GET /safe ? { u: str } = starts_with(u, "/") && !starts_with(u, "//") ? redirect(u) : "no"
"#;

#[test]
fn a_path_whose_dots_hide_a_second_slash_stays_on_the_site() {
    let d = dir("dots", REDIRECTS);
    for (base, opts) in [("", &[][..]), ("/app", &["--base", "/app"][..])] {
        let s = support::serve_with(&d.join("main.qwe"), opts);
        let to = |route: &str, u: &str| {
            let a = support::send(s.port, "GET", &format!("/{}?{}={}", route, if route == "go" { "next" } else { "u" }, enc(u)), &[], "");
            assert_eq!(a.status, 303, "{} {} under {:?}: {}", route, u, base, a.body);
            a.header("location").unwrap_or_default().to_string()
        };
        for (u, want) in [
            // Each was `//evil.com` (or `///evil.com`), which a browser takes to evil.com.
            ("/.//evil.com", "/evil.com"),
            ("/%2e//evil.com", "/evil.com"),
            ("/%2E//evil.com", "/evil.com"),
            ("/a/..//evil.com", "/evil.com"),
            ("/a/b/../..//evil.com", "/evil.com"),
            ("/..//evil.com", "/evil.com"),
            ("/.%2e//evil.com", "/evil.com"),
            ("/.///evil.com", "/evil.com"),
            ("/./\\evil.com", "/evil.com"),
            ("/.\\/evil.com", "/evil.com"),
            ("/.//evil.com/x?y=//z#//w", "/evil.com/x?y=//z#//w"),
            ("/.//", "/"),
            ("/.//é", "/%C3%A9"),
            // `//` and a host IDNA cannot write once the dots were out: an error value before, a
            // path of the site's own now (found in the second review).
            ("/..//\u{85}", "/%C2%85"),
            // Not at the start, a `//` is the site's own path, as before.
            ("/a/.//b", "/a//b"),
            ("/a//b", "/a//b"),
            ("/./x", "/x"),
            ("/a/../b", "/b"),
        ] {
            assert_eq!(to("to", u), format!("{}{}", base, want), "redirect({:?}) under {:?}", u, base);
            assert_eq!(to("safe", u), format!("{}{}", base, want), "a checked redirect({:?}) under {:?}", u, base);
        }
        // The guide's recipe: each of them is no address of the site's, so `/`.
        for u in ["/.//evil.com", "/%2e//evil.com", "/a/..//evil.com", "/.///evil.com", "/./\\evil.com", "/./%2F/evil.com", "/./%5c/evil.com", "//evil.com"] {
            assert_eq!(to("go", u), format!("{}/", base), "redirect(local_path({:?})) under {:?}", u, base);
        }
        assert_eq!(to("go", "/a/.//b"), format!("{}/a//b", base));
        // What a program wrote with a host is its own: `//evil.com` as given, the base not before it.
        assert_eq!(to("to", "//evil.com"), "//evil.com");
    }
}

#[test]
fn local_path_is_null_for_a_path_that_dots_take_off_the_site() {
    let d = dir("local-path", &format!("for n in {} {{ print(json.stringify(local_path(n))) }}\n", PATHS));
    let want = [
        "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null",
        // Paths of the site's own, as they were given.
        "\"/a/../b\"", "\"/./x\"", "\"/a/.//b\"", "\"/..\"", "\"/.\"", "\"/x?next=/.//evil\"", "\"/x#/.//evil\"", "\"/...//x\"", "\"/.a//x\"", "\"/./é\"",
    ];
    assert_eq!(run_script(&d), want.join("\n") + "\n");
    // Control characters past ASCII (C1, U+0080 to U+009F) are refused, as before; a letter past
    // ASCII at the start is a path's own.
    let d = dir("local-path-c1", "print(json.stringify([local_path(\"/a\u{85}b\"), local_path(\"/é\u{9f}\"), local_path(\"/é/./x\"), local_path(\"/éé\"), local_path(\"/%2Fé\")]))\n");
    assert_eq!(run_script(&d), "[null,null,\"/é/./x\",\"/éé\",null]\n");
}

/// What `local_path` is given in the tests of both sides, as a Quillo list.
const PATHS: &str = r#"["/.//evil.example", "/%2e//evil.example", "/%2E%2e//evil.example", "/a/..//evil.example", "/a/b/../..//evil.example", "/.%2e//evil.example", "/.///evil.example", "/./\\evil.example", "/.\t//evil.example", "/./%2F/evil", "/./%5C/evil", "/.\r//evil.example", "/%2E/%2F/x", " /.//evil.example ", "/%2e.//evil.example", "/a/../%2F/x", "/./%2f/evil", "/.//evil.example?x=1", "/.//", "/a/../b", "/./x", "/a/.//b", "/..", "/.", "/x?next=/.//evil", "/x#/.//evil", "/...//x", "/.a//x", "/./é"]"#;

/// What the server's `local_path` says for each of `PATHS`.
const PATHS_SAID: &str = r#"[null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,"/a/../b","/./x","/a/.//b","/..","/.","/x?next=/.//evil","/x#/.//evil","/...//x","/.a//x","/./é"]"#;

// The browser's `local_path` says what the server's says: a component that runs it on a click.
#[test]
fn in_chrome_local_path_says_what_the_servers_says() {
    let d = dir(
        "local-path-chrome",
        &format!(
            "component Paths() {{\n    state out = \"\"\n    <div><button #go onclick={{() => {{ out = json.stringify(map({}, (n) => local_path(n))) }}}}>go</button><p #out>{{out}}</p></div>\n}}\nroute GET / = <html><head><title>p</title></head><body><Paths/></body></html>\n",
            PATHS
        ),
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const b = document.getElementById("go");
  b.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); b.click(); await tick(300);
  document.body.dataset.result = JSON.stringify({ out: document.getElementById("out").textContent });
})();"#;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), script);
    let said: serde_json::Value = serde_json::from_str(r["out"].as_str().unwrap_or_else(|| panic!("{}", r))).unwrap();
    assert_eq!(said, serde_json::from_str::<serde_json::Value>(PATHS_SAID).unwrap(), "{}", r);
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
    let d = support::temp_dir("feedback71-chrome");
    std::fs::write(d.join("t.js"), script).unwrap();
    let out = Command::new("node").arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(d.join("profile")).arg("30000").arg(d.join("t.js")).output().expect("node");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

// ---- Q-968's follow-up: the browser's `local_path` and C1 controls ------------------------------

// What the server's `local_path` says of control characters past ASCII (C1, U+0080 to U+009F),
// and of letters and a no-break space past ASCII, the browser's says too: it let them through.
#[test]
fn in_chrome_local_path_refuses_c1_controls_as_the_server_does() {
    let paths = "[\"/a\u{85}b\", \"/\u{80}\", \"/x\u{9f}\", \"/é\u{9f}\", \"/\u{7f}\", \"/\u{a0}x\", \"/é/./x\", \"/éé\", \"/%2Fé\", \"/a\u{9e}/b?c=\u{85}\"]";
    let server = run_script(&dir("c1-server", &format!("print(json.stringify(map({}, (n) => local_path(n))))\n", paths)));
    assert_eq!(server, "[null,null,null,null,null,\"/\u{a0}x\",\"/é/./x\",\"/éé\",null,null]\n");
    let d = dir(
        "c1-chrome",
        &format!(
            "component Paths() {{\n    state out = \"\"\n    <div><button #go onclick={{() => {{ out = json.stringify(map({}, (n) => local_path(n))) }}}}>go</button><p #out>{{out}}</p></div>\n}}\nroute GET / = <html><head><title>p</title></head><body><Paths/></body></html>\n",
            paths
        ),
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const b = document.getElementById("go");
  b.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); b.click(); await tick(300);
  document.body.dataset.result = JSON.stringify({ out: document.getElementById("out").textContent });
})();"#;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), script);
    assert_eq!(r["out"].as_str().map(|o| o.to_string() + "\n"), Some(server), "{}", r);
}

// ---- Q-969: raw(...) in a <script> inside <svg> or <math> --------------------------------------

const FOREIGN: &str = r#"view Chart(points: list) = <svg viewBox="0 0 10 10"><script>{raw("window.__v = " + json.stringify(points) + ";")}</script></svg>
view Icon(children) = <svg>{children}</svg>
view Out(children) = <svg><foreignObject>{children}</foreignObject></svg>
view Plain(children) = <g>{children}</g>
view Later(children) {
    let n = 1
    return <svg data-n={n}>{children}</svg>
}
view Kept(children) {
    let s = <svg>{children}</svg>
    return s
}
view Fwd(children) = <Icon>{children}</Icon>
view FwdOut(children) = <Out>{children}</Out>
view Both(children, round: bool) = round ? <svg>{children}</svg> : <svg><g>{children}</g></svg>
view Pick(children, round: bool) = round ? <svg>{children}</svg> : <div>{children}</div>
view PickIf(children, round: bool) {
    if round {
        return <svg>{children}</svg>
    }
    <div>{children}</div>
}
view PickMatch(children, k: str) = match k {
    "svg" => <svg>{children}</svg>
    _ => <div>{children}</div>
}
view Or(children) = <svg>{children ?? ""}</svg>
fn wrap(c) = <svg>{c}</svg>
view ByFn(children) = <div>{wrap(children)}</div>
view Lost(children) {
    let f = (c) => <svg>{c}</svg>
    f(children)
}
view InHtml(children) = <div><p>{children}</p></div>
view Script(q: str) = <script>{raw(q)}</script>
route GET /svg ? { q: str = "" } = <html><body><svg><script>{raw(json.stringify(q))}</script></svg></body></html>
route GET /math ? { q: str = "" } = <html><body><math><script>{raw(json.stringify(q))}</script></math></body></html>
route GET /html ? { q: str = "" } = <html><body><script>{raw(json.stringify(q))}</script></body></html>
route GET /value ? { q: str = "" } = <html><body><svg><script>{q}</script></svg></body></html>
route GET /fo ? { q: str = "" } = <html><body><svg><foreignObject><script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /fo-lower ? { q: str = "" } = <html><body><svg><foreignobject><script>{raw(q)}</script></foreignobject></svg></body></html>
route GET /title ? { q: str = "" } = <html><body><svg><title><script>{raw(q)}</script></title></svg></body></html>
route GET /desc ? { q: str = "" } = <html><body><svg><desc><script>{raw(q)}</script></desc></svg></body></html>
route GET /mi ? { q: str = "" } = <html><body><math><mi><script>{raw(q)}</script></mi></math></body></html>
route GET /mi-case ? { q: str = "" } = <html><body><math><mI><script>{raw(q)}</script></mI></math></body></html>
route GET /mglyph ? { q: str = "" } = <html><body><math><mi><mglyph><script>{raw(q)}</script></mglyph></mi></math></body></html>
route GET /malignmark ? { q: str = "" } = <html><body><math><mtext><malignmark><script>{raw(q)}</script></malignmark></mtext></math></body></html>
route GET /svg-mi ? { q: str = "" } = <html><body><svg><mi><script>{raw(q)}</script></mi></svg></body></html>
route GET /math-fo ? { q: str = "" } = <html><body><math><foreignObject><script>{raw(q)}</script></foreignObject></math></body></html>
route GET /math-svg-fo ? { q: str = "" } = <html><body><math><svg><foreignObject><script>{raw(q)}</script></foreignObject></svg></math></body></html>
route GET /svg-math-mi ? { q: str = "" } = <html><body><svg><math><mi><script>{raw(q)}</script></mi></math></svg></body></html>
route GET /mtext ? { q: str = "" } = <html><body><math><mtext><svg><script>{raw(q)}</script></svg></mtext></math></body></html>
route GET /fomath ? { q: str = "" } = <html><body><svg><foreignObject><math><script>{raw(q)}</script></math></foreignObject></svg></body></html>
route GET /deep ? { q: str = "" } = <html><body><div><p><math><script>{raw(q)}</script></math></p></div></body></html>
route GET /ann ? { q: str = "" } = <html><body><math><annotation-xml encoding="text/html"><script>{raw(q)}</script></annotation-xml></math></body></html>
route GET /ann-xhtml ? { q: str = "" } = <html><body><math><annotation-xml encoding="Application/XHTML+XML"><script>{raw(q)}</script></annotation-xml></math></body></html>
route GET /ann-space ? { q: str = "" } = <html><body><math><annotation-xml encoding=" text/html"><script>{raw(q)}</script></annotation-xml></math></body></html>
route GET /ann-none ? { q: str = "" } = <html><body><math><annotation-xml><script>{raw(q)}</script></annotation-xml></math></body></html>
route GET /ann-svg ? { q: str = "" } = <html><body><math><annotation-xml><svg><script>{raw(q)}</script></svg></annotation-xml></math></body></html>
route GET /ann-dyn ? { q: str = "", e: str = "TEXT/HTML" } = <html><body><math><annotation-xml encoding={e}><script>{raw(q)}</script></annotation-xml></math></body></html>
route GET /ann-spread ? { q: str = "", e: str = "text/html" } = <html><body><math><annotation-xml {...{ encoding: e }}><script>{raw(q)}</script></annotation-xml></math></body></html>
route GET /p ? { q: str = "" } = <html><body><div><svg><p>x</p><script>{raw(q)}</script></svg></div></body></html>
route GET /b-deep ? { q: str = "" } = <html><body><div><svg><g><b>x</b></g><script>{raw(q)}</script></svg></div></body></html>
route GET /img ? { q: str = "" } = <html><body><div><svg><img src="/x.png"/><script>{raw(q)}</script></svg></div></body></html>
route GET /font ? { q: str = "" } = <html><body><div><svg><font color="red">x</font><script>{raw(q)}</script></svg></div></body></html>
route GET /font-plain ? { q: str = "" } = <html><body><svg><font>x</font><script>{raw(q)}</script></svg></body></html>
route GET /font-dyn ? { q: str = "", c: str = "red" } = <html><body><div><svg><font size={c}>x</font><script>{raw(q)}</script></svg></div></body></html>
route GET /pieces ? { q: str = "" } = <html><body><svg><g><text>{q}</text></g><script>{raw(q)}</script></svg></body></html>
route GET /cross ? { q: str = "" } = <html><body><svg><g><foreignObject><svg><g><p></p></g><script>{raw(q)}</script></svg></foreignObject></g></svg></body></html>
route GET /mi-break ? { q: str = "" } = <html><body><math><mi><svg><p></p><script>{raw(q)}</script></svg></mi></math></body></html>
route GET /raw-break ? { q: str = "" } = <html><body><svg><g><foreignObject><svg><g>{raw("<p>x</p>")}</g><script>{raw(q)}</script></svg></foreignObject></g></svg></body></html>
route GET /own ? { q: str = "" } = <html><body><svg><script><![CDATA[ window.__o = {raw(q)}; ]]></script></svg></body></html>
route GET /two ? { q: str = "" } = <html><body><svg><script>var a = {raw(q)}, b = {raw(q)};</script></svg></body></html>
route GET /empty = <html><body><svg><script>{raw("")}</script></svg></body></html>
route GET /chart ? { q: str = "" } = <html><body><Chart points={[q, 1]}/></body></html>
route GET /after ? { q: str = "" } = <html><body><svg><script>{raw(q)}</script></svg><script>{raw(q)}</script></body></html>
route GET /icon ? { q: str = "" } = <html><body><Icon><script>{raw(q)}</script></Icon></body></html>
route GET /later ? { q: str = "" } = <html><body><Later><script>{raw(q)}</script></Later></body></html>
route GET /kept ? { q: str = "" } = <html><body><Kept><script>{raw(q)}</script></Kept></body></html>
route GET /fwd ? { q: str = "" } = <html><body><Fwd><script>{raw(q)}</script></Fwd></body></html>
route GET /fwd-out ? { q: str = "" } = <html><body><FwdOut><script>{raw(q)}</script></FwdOut></body></html>
route GET /out ? { q: str = "" } = <html><body><Out><script>{raw(q)}</script></Out></body></html>
route GET /both ? { q: str = "", r: str = "" } = <html><body><Both round={r == "1"}><script>{raw(q)}</script></Both></body></html>
route GET /pick ? { q: str = "", r: str = "" } = <html><body><Pick round={r == "1"}><script>{raw(q)}</script></Pick></body></html>
route GET /pick-if ? { q: str = "", r: str = "" } = <html><body><PickIf round={r == "1"}><script>{raw(q)}</script></PickIf></body></html>
route GET /pick-match ? { q: str = "", r: str = "" } = <html><body><PickMatch k={r == "1" ? "svg" : "div"}><script>{raw(q)}</script></PickMatch></body></html>
route GET /or ? { q: str = "" } = <html><body><Or><script>{raw(q)}</script></Or></body></html>
route GET /by-fn ? { q: str = "" } = <html><body><ByFn><script>{raw(q)}</script></ByFn></body></html>
route GET /lost ? { q: str = "" } = <html><body><Lost><script>{raw(q)}</script></Lost></body></html>
route GET /in-html ? { q: str = "" } = <html><body><InHtml><script>{raw(q)}</script></InHtml></body></html>
route GET /in-svg-html ? { q: str = "" } = <html><body><svg><InHtml><script>{raw(q)}</script></InHtml></svg></body></html>
route GET /inplain ? { q: str = "" } = <html><body><svg><Plain><script>{raw(q)}</script></Plain></svg></body></html>
route GET /plain ? { q: str = "" } = <html><body><Plain><script>{raw(q)}</script></Plain></body></html>
route GET /cache ? { q: str = "" } = <html><body><Script q={q}/><svg><Script q={q}/></svg><math><Script q={q}/></math><Script q={q}/></body></html>
"#;

/// What a page's body holds before the scripts Quillo adds.
fn body_of(page: &str) -> &str {
    let from = page.find("<body>").map_or(0, |i| i + 6);
    let to = page[from..].find("<script>(()=>").or_else(|| page[from..].find("</body>")).map_or(page.len(), |i| from + i);
    &page[from..to]
}

#[test]
fn raw_in_a_script_the_browser_reads_as_svg_or_mathml_is_text_to_it() {
    let d = dir("foreign", FOREIGN);
    let s = support::serve(&d.join("main.qwe"), &[]);
    // `<b>`, an end tag, a character reference and the end of a CDATA section.
    let q = enc("<b>x</b>&quot;]]>");
    let get = |p: &str| {
        let (path, more) = p.split_once('&').map_or((p, String::new()), |(a, b)| (a, format!("&{}", b)));
        let a = support::send(s.port, "GET", &format!("{}?q={}{}", path, q, more), &[], "");
        assert_eq!(a.status, 200, "{}: {}", p, a.body);
        a.body
    };
    // The same code as in an HTML script, in a CDATA section, its `]]>` split between two.
    let code = r#"<![CDATA[<b>x<\/b>&quot;]]]]><![CDATA[>]]>"#;
    let json = r#"<![CDATA["<b>x<\/b>&quot;]]]]><![CDATA[>"]]>"#;
    // As in an HTML script: no section.
    let html = r#"<b>x<\/b>&quot;]]>"#;
    let cases: Vec<(&str, String)> = vec![
        ("/svg", format!("<svg><script>{json}</script></svg>")),
        ("/math", format!("<math><script>{json}</script></math>")),
        ("/mglyph", format!("<math><mi><mglyph><script>{code}</script></mglyph></mi></math>")),
        ("/malignmark", format!("<math><mtext><malignmark><script>{code}</script></malignmark></mtext></math>")),
        // `mi` is MathML's only inside MathML, `foreignObject` SVG's only inside SVG: an SVG `<math>`
        // and a MathML `<svg>` are their parent's.
        ("/svg-mi", format!("<svg><mi><script>{code}</script></mi></svg>")),
        ("/math-fo", format!("<math><foreignObject><script>{code}</script></foreignObject></math>")),
        ("/math-svg-fo", format!("<math><svg><foreignObject><script>{code}</script></foreignObject></svg></math>")),
        ("/svg-math-mi", format!("<svg><math><mi><script>{code}</script></mi></math></svg>")),
        ("/mtext", format!("<math><mtext><svg><script>{code}</script></svg></mtext></math>")),
        ("/fomath", format!("<svg><foreignObject><math><script>{code}</script></math></foreignObject></svg>")),
        ("/deep", format!("<div><p><math><script>{code}</script></math></p></div>")),
        // An `encoding` that is not HTML's, as the parser compares it; none; an `<svg>` in one.
        ("/ann-space", format!("<math><annotation-xml encoding=\" text/html\"><script>{code}</script></annotation-xml></math>")),
        ("/ann-none", format!("<math><annotation-xml><script>{code}</script></annotation-xml></math>")),
        ("/ann-svg", format!("<math><annotation-xml><svg><script>{code}</script></svg></annotation-xml></math>")),
        ("/ann-dyn&e=text/xml", format!("<math><annotation-xml encoding=\"text/xml\"><script>{code}</script></annotation-xml></math>")),
        ("/ann-spread&e=x", format!("<math><annotation-xml encoding=\"x\"><script>{code}</script></annotation-xml></math>")),
        // A `<font>` without `color`, `face` or `size` is SVG's.
        ("/font-plain", format!("<svg><font>x</font><script>{code}</script></svg>")),
        ("/pieces", format!("<svg><g><text>&lt;b&gt;x&lt;/b&gt;&amp;quot;]]&gt;</text></g><script>{code}</script></svg>")),
        // A `<p>` that ends foreign content inside an integration point: a later `</g>` may close
        // an SVG `<g>` outside it, so where the script is is not known, and it is written as it
        // is safe in both: the browser reads `cross` as SVG, `mi-break` as HTML.
        ("/cross", format!("<svg><g><foreignObject><svg><g><p></p></g><script>{code}</script></svg></foreignObject></g></svg>")),
        ("/mi-break", format!("<math><mi><svg><p></p><script>{code}</script></svg></mi></math>")),
        ("/raw-break", format!("<svg><g><foreignObject><svg><g><p>x</p></g><script>{code}</script></svg></foreignObject></g></svg>")),
        // In a section the program's own text opened: in that one.
        ("/own", r#"<svg><script><![CDATA[ window.__o = <b>x<\/b>&quot;]]]]><![CDATA[>; ]]></script></svg>"#.to_string()),
        ("/two", format!("<svg><script>var a = {code}, b = {code};</script></svg>")),
        ("/chart", r#"<svg viewBox="0 0 10 10"><script><![CDATA[window.__v = ["<b>x<\/b>&quot;]]]]><![CDATA[>",1];]]></script></svg>"#.to_string()),
        ("/after", format!("<svg><script>{code}</script></svg><script>{html}</script>")),
        // A view's children, rendered before it, as where it puts them: through a `let` and a
        // `return`, a view they are passed on to, `??`, a function, and a branch whose arms agree.
        ("/icon", format!("<svg><script>{code}</script></svg>")),
        ("/later", format!("<svg data-n=\"1\"><script>{code}</script></svg>")),
        ("/kept", format!("<svg><script>{code}</script></svg>")),
        ("/fwd", format!("<svg><script>{code}</script></svg>")),
        ("/both&r=1", format!("<svg><script>{code}</script></svg>")),
        ("/both&r=0", format!("<svg><g><script>{code}</script></g></svg>")),
        ("/or", format!("<svg><script>{code}</script></svg>")),
        ("/by-fn", format!("<div><svg><script>{code}</script></svg></div>")),
        ("/inplain", format!("<svg><g><script>{code}</script></g></svg>")),
        // Arms that differ, a lambda, which the walk cannot follow: the safe form, in either arm.
        ("/pick&r=1", format!("<svg><script>{code}</script></svg>")),
        ("/pick&r=0", format!("<div><script>{code}</script></div>")),
        ("/pick-if&r=1", format!("<svg><script>{code}</script></svg>")),
        ("/pick-if&r=0", format!("<div><script>{code}</script></div>")),
        ("/pick-match&r=1", format!("<svg><script>{code}</script></svg>")),
        ("/pick-match&r=0", format!("<div><script>{code}</script></div>")),
        ("/lost", format!("<svg><script>{code}</script></svg>")),
        // A view kept as made in HTML is made again inside `<svg>` and `<math>`.
        ("/cache", format!("<script>{html}</script><svg><script>{code}</script></svg><math><script>{code}</script></math><script>{html}</script>")),
        // Where HTML is read: no section, as before. SVG's integration points in any case, an
        // HTML `annotation-xml`, MathML's text elements, and after a tag that ends foreign content.
        ("/html", r#"<script>"<b>x<\/b>&quot;]]>"</script>"#.to_string()),
        ("/fo", format!("<svg><foreignObject><script>{html}</script></foreignObject></svg>")),
        ("/fo-lower", format!("<svg><foreignobject><script>{html}</script></foreignobject></svg>")),
        ("/title", format!("<svg><title><script>{html}</script></title></svg>")),
        ("/desc", format!("<svg><desc><script>{html}</script></desc></svg>")),
        ("/mi", format!("<math><mi><script>{html}</script></mi></math>")),
        ("/mi-case", format!("<math><mI><script>{html}</script></mI></math>")),
        ("/ann", format!("<math><annotation-xml encoding=\"text/html\"><script>{html}</script></annotation-xml></math>")),
        ("/ann-xhtml", format!("<math><annotation-xml encoding=\"Application/XHTML+XML\"><script>{html}</script></annotation-xml></math>")),
        ("/ann-dyn", format!("<math><annotation-xml encoding=\"TEXT/HTML\"><script>{html}</script></annotation-xml></math>")),
        ("/ann-spread", format!("<math><annotation-xml encoding=\"text/html\"><script>{html}</script></annotation-xml></math>")),
        ("/p", format!("<div><svg><p>x</p><script>{html}</script></svg></div>")),
        ("/b-deep", format!("<div><svg><g><b>x</b></g><script>{html}</script></svg></div>")),
        ("/img", format!("<div><svg><img src=\"/x.png\"><script>{html}</script></svg></div>")),
        ("/font", format!("<div><svg><font color=\"red\">x</font><script>{html}</script></svg></div>")),
        ("/font-dyn", format!("<div><svg><font size=\"red\">x</font><script>{html}</script></svg></div>")),
        ("/fwd-out", format!("<svg><foreignObject><script>{html}</script></foreignObject></svg>")),
        ("/out", format!("<svg><foreignObject><script>{html}</script></foreignObject></svg>")),
        ("/in-html", format!("<div><p><script>{html}</script></p></div>")),
        ("/in-svg-html", format!("<svg><div><p><script>{html}</script></p></div></svg>")),
        ("/plain", format!("<g><script>{html}</script></g>")),
        // A plain value is JSON, `<` and `&` escaped, as before.
        ("/value", r#"<svg><script>"\u003cb>x\u003c/b>\u0026quot;]]>"</script></svg>"#.to_string()),
    ];
    // Twice each: a page's fixed parts, and a view's HTML, are kept after its first render.
    for _ in 0..2 {
        for (route, want) in &cases {
            assert_eq!(body_of(&get(route)), want, "{}", route);
        }
        assert_eq!(body_of(&support::send(s.port, "GET", "/empty", &[], "").body), "<svg><script><![CDATA[]]></script></svg>");
    }
    // Text past ASCII, and `]]>` at both ends and twice over, as given.
    let a = support::send(s.port, "GET", &format!("/svg?q={}", enc("]]>é ✓ ]]]]>]]>")), &[], "").body;
    assert_eq!(body_of(&a), r#"<svg><script><![CDATA["]]]]><![CDATA[>é ✓ ]]]]]]><![CDATA[>]]]]><![CDATA[>"]]></script></svg>"#);
}

#[test]
fn quillo_check_takes_the_programs_of_q969_and_q979() {
    for (name, src) in [("foreign-check", FOREIGN), ("chrome-check", FOREIGN_CHROME), ("clobber-check", SPREADS), ("apart-check", APART), ("honest-check", HONEST), ("leave-check", LEAVE), ("b2-check", B2)] {
        let (ok, said) = check(&dir(name, src));
        assert!(ok, "{}: {}", name, said);
    }
}

/// What `quillo check` says of a program, under a deadline: whether it passed, and its output.
fn check(d: &Path) -> (bool, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("quillo check did not end");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let o = child.wait_with_output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr))
}

const FOREIGN_CHROME: &str = r#"view Icon(children) = <svg>{children}</svg>
view Out(children) = <svg><foreignObject>{children}</foreignObject></svg>
view Kept(children) {
    let s = <svg>{children}</svg>
    return s
}
fn put(name: str, q: str) = raw("window.__" + name + " = " + json.stringify(q) + ";")
route GET / ? { q: str = "" } = <html><head><title>t</title></head><body>
    <svg><script>{put("s", q)}</script></svg>
    <math><script>{put("m", q)}</script></math>
    <script>{put("h", q)}</script>
    <Icon><script>{put("i", q)}</script></Icon>
    <Kept><script>{put("k", q)}</script></Kept>
    <Out><script>{put("o", q)}</script></Out>
    <svg><foreignobject><script>{put("f", q)}</script></foreignobject></svg>
    <svg><title><script>{put("t", q)}</script></title></svg>
    <math><annotation-xml encoding="text/html"><script>{put("a", q)}</script></annotation-xml></math>
    <math><mi><mglyph><script>{put("g", q)}</script></mglyph></mi></math>
    <div><svg><p>p</p><script>{put("p", q)}</script></svg></div>
    <svg><g><foreignObject><svg><g><p></p></g><script>{put("x", q)}</script></svg></foreignObject></g></svg>
    <math><mi><svg><p></p><script>{put("u", q)}</script></svg></mi></math>
    <svg><script>{raw("var a = 1, b = 2; window.__c = a<b && \"&amp;\";")}</script></svg>
</body></html>
"#;

// In Chrome: a visitor's markup in a raw script in an <svg> or <math> makes no element and runs
// nothing, wherever the script stands; each script the browser reads, SVG's or HTML's, runs with
// the value as given. A MathML `<script>` (`m`, `g`) is no script to the browser and runs nothing;
// its text is markup, as SVG's is. One where the renderer cannot tell (`u`: a `<p>` that ended SVG
// in a MathML `<mi>`) is the safe form: in the HTML script the browser reads there, it does not
// run.
#[test]
fn in_chrome_a_raw_script_runs_as_written_where_it_stands_and_lets_no_markup_out() {
    let d = dir("foreign-chrome", FOREIGN_CHROME);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let script = r#"(async () => {
  await new Promise((r) => setTimeout(r, 300));
  const got = {};
  for (const k of ["s", "m", "h", "i", "k", "o", "f", "t", "a", "g", "p", "x", "u", "c"]) got[k] = window["__" + k] ?? null;
  document.body.dataset.result = JSON.stringify({
    pwn: window.__pwn ?? null,
    imgs: document.querySelectorAll("img").length,
    bs: document.querySelectorAll("b").length,
    got,
  });
})();"#;
    for q in [
        "</script><img src=x onerror=window.__pwn=1>&quot;]]><img src=x onerror=window.__pwn=2><b>b</b>",
        "&quot;;window.__pwn=3;//",
        "]]><img src=x onerror=window.__pwn=4>",
        "<![CDATA[<img src=x onerror=window.__pwn=5>",
        "é ✓ <svg onload=window.__pwn=6>",
        "",
    ] {
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/?q={}", s.port, enc(q)), script);
        assert_eq!((r["pwn"].clone(), r["imgs"].as_u64(), r["bs"].as_u64()), (serde_json::Value::Null, Some(0), Some(0)), "{}: {}", q, r);
        for k in ["s", "h", "i", "k", "o", "f", "t", "a", "p", "x"] {
            assert_eq!(r["got"][k].as_str(), Some(q), "window.__{} for {}: {}", k, q, r);
        }
        for k in ["m", "g", "u"] {
            assert_eq!(r["got"][k], serde_json::Value::Null, "window.__{} for {}: {}", k, q, r);
        }
        assert_eq!(r["got"]["c"].as_str(), Some("&amp;"), "{}: {}", q, r);
    }
}

// ---- Q-979: a spread's name or id that the document would take -------------------------------

const SPREADS: &str = r#"component Counter() {
    state n = 0
    <button #inc onclick={() => { n += 1 }}>{n}</button>
}
component Pic(n: str, i: str) {
    state a = { alt: "pic" }
    state k = 0
    <div #box><button #set onclick={() => { k += 1; a = { alt: "pic" + str(k), name: n == "7" ? 7 : n, id: i } }}>set</button><img src="/x.png" {...a}/><object {...a}></object><form {...a}></form></div>
}
route GET /el/:tag ? { a: str = "{}" } {
    let attrs = json.parse(a) ?? {}
    if tag == "img" { return <html><body><img src="/x.png" {...attrs}/></body></html> }
    if tag == "named-img" { return <html><body><img src="/x.png" name="me" {...attrs}/></body></html> }
    if tag == "written" { return <html><body><img name="createElement" id="body" {...attrs}/></body></html> }
    if tag == "image" { return <html><body><image {...attrs}></image></body></html> }
    if tag == "form" { return <html><body><form {...attrs}></form></body></html> }
    if tag == "iframe" { return <html><body><iframe {...attrs}></iframe></body></html> }
    if tag == "embed" { return <html><body><embed {...attrs}/></body></html> }
    if tag == "object" { return <html><body><object {...attrs}></object></body></html> }
    if tag == "input" { return <html><body><input {...attrs}/></body></html> }
    return <html><body><div {...attrs}></div></body></html>
}
route GET /page ? { a: str = "{}" } {
    let attrs = json.parse(a) ?? {}
    return <html><head><title>c3</title></head><body><img src="/x.png" alt="avatar" {...attrs}/><Counter/></body></html>
}
route GET /pic ? { n: str = "", i: str = "" } = <html><head><title>pic</title></head><body><Pic n={n} i={i}/></body></html>
"#;

#[test]
fn a_spread_name_or_id_the_document_would_take_is_dropped() {
    let d = dir("clobber", SPREADS);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |tag: &str, a: &str| body_of(&support::send(s.port, "GET", &format!("/el/{}?a={}", tag, enc(a)), &[], "").body).to_string();
    for (tag, a, want) in [
        ("img", r#"{"name":"createElement"}"#, r#"<img src="/x.png">"#),
        ("img", r#"{"name":"querySelectorAll","alt":"a"}"#, r#"<img alt="a" src="/x.png">"#),
        ("img", r#"{"NAME":"body"}"#, r#"<img src="/x.png">"#),
        ("img", r#"{"name":"toString"}"#, r#"<img src="/x.png">"#),
        ("img", r#"{"name":"__proto__"}"#, r#"<img src="/x.png">"#),
        ("img", r#"{"name":"location"}"#, r#"<img src="/x.png">"#),
        ("img", r#"{"name":"onpointerrawupdate"}"#, r#"<img src="/x.png">"#),
        // An `<img>`'s `id` is the document's only with a `name` beside it.
        ("img", r#"{"id":"body"}"#, r#"<img id="body" src="/x.png">"#),
        ("img", r#"{"name":"me","id":"body"}"#, r#"<img name="me" src="/x.png">"#),
        ("img", r#"{"name":"me","Id":"cookie"}"#, r#"<img name="me" src="/x.png">"#),
        ("img", r#"{"name":7,"id":"body"}"#, r#"<img name="7" src="/x.png">"#),
        ("img", r#"{"name":"createElement","id":"body"}"#, r#"<img id="body" src="/x.png">"#),
        ("img", r#"{"name":"","id":"body"}"#, r#"<img name="" id="body" src="/x.png">"#),
        ("img", r#"{"name":null,"id":"body"}"#, r#"<img id="body" src="/x.png">"#),
        ("named-img", r#"{"id":"body"}"#, r#"<img src="/x.png" name="me">"#),
        ("named-img", r#"{"id":"mine"}"#, r#"<img id="mine" src="/x.png" name="me">"#),
        // `<image>` is read as an `<img>`.
        ("image", r#"{"name":"cookie"}"#, "<image></image>"),
        ("image", r#"{"name":"me","id":"head"}"#, r#"<image name="me"></image>"#),
        ("form", r#"{"name":"forms"}"#, "<form></form>"),
        // A form's `id` is no name of the document's: kept.
        ("form", r#"{"id":"body"}"#, r#"<form id="body"></form>"#),
        ("iframe", r#"{"name":"location","id":"location"}"#, r#"<iframe id="location"></iframe>"#),
        ("embed", r#"{"name":"head","id":"body"}"#, r#"<embed id="body">"#),
        ("object", r#"{"id":"defaultView"}"#, "<object></object>"),
        ("object", r#"{"name":"embeds","id":"forms"}"#, "<object></object>"),
        ("object", r#"{"id":"mine"}"#, r#"<object id="mine"></object>"#),
        // A name that is not one of the document's is kept, and so is any name elsewhere, and what
        // the program writes out.
        ("img", r#"{"name":"avatar","id":"me"}"#, r#"<img name="avatar" id="me" src="/x.png">"#),
        ("img", r#"{"name":"CreateElement"}"#, r#"<img name="CreateElement" src="/x.png">"#),
        ("img", r#"{"name":"createElement2"}"#, r#"<img name="createElement2" src="/x.png">"#),
        ("img", r#"{"name":""}"#, r#"<img name="" src="/x.png">"#),
        ("img", r#"{"name":1}"#, r#"<img name="1" src="/x.png">"#),
        ("img", r#"{"name":"bodé"}"#, r#"<img name="bodé" src="/x.png">"#),
        ("written", r#"{"alt":"x"}"#, r#"<img alt="x" name="createElement" id="body">"#),
        ("input", r#"{"name":"body"}"#, r#"<input name="body">"#),
        ("div", r#"{"name":"createElement","id":"querySelector"}"#, r#"<div name="createElement" id="querySelector"></div>"#),
    ] {
        assert_eq!(get(tag, a), want, "<{}> with {}", tag, a);
    }
}

#[test]
fn in_chrome_a_spread_name_stops_no_component() {
    let d = dir("clobber-chrome", SPREADS);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    // The audit's page: the counter counts, with any name in the spread.
    let count = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const b = document.getElementById("inc");
  // Each click waited for, as long as a slow build takes, not for a fixed time.
  for (let i = 0; i < 2; i++) {
    b.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); b.click();
    for (let t = 0; t < 600 && b.textContent !== String(i + 1); t++) await tick(25);
  }
  document.body.dataset.result = JSON.stringify({ n: b.textContent, own: typeof document.createElement, qsa: typeof document.querySelectorAll });
})();"#;
    for a in [r#"{}"#, r#"{"name":"createElement"}"#, r#"{"name":"querySelectorAll"}"#, r#"{"name":"addEventListener"}"#, r#"{"name":"body"}"#] {
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/page?a={}", s.port, enc(a)), count);
        assert_eq!((r["n"].as_str(), r["own"].as_str(), r["qsa"].as_str()), (Some("2"), Some("function"), Some("function")), "{}: {}", a, r);
    }
    // Set in the browser by a component, twice: the same rule, asked of `document`'s prototypes, so
    // a name the spread set itself the first time (`document.avatar` is then that element) is kept
    // the second time too.
    let set = r##"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const b = document.getElementById("set");
  const seen = [];
  for (let i = 0; i < 2; i++) {
    b.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); b.click();
    for (let t = 0; t < 600 && document.querySelector("#box img").getAttribute("alt") !== "pic" + (i + 1); t++) await tick(25);
    const box = document.getElementById("box");
    seen.push(["img", "object", "form"].map((t) => { const e = box.querySelector(t); return [e.getAttribute("name"), e.getAttribute("id")]; }));
  }
  document.body.dataset.result = JSON.stringify({ seen, alt: document.querySelector("#box img").getAttribute("alt"), qs: typeof document.querySelector, gi: typeof document.getElementById, body: document.body.tagName });
})();"##;
    let n = serde_json::Value::Null;
    let s_ = |v: &str| serde_json::Value::String(v.to_string());
    for (name, id, want) in [
        ("avatar", "body", [[s_("avatar"), n.clone()], [s_("avatar"), n.clone()], [s_("avatar"), s_("body")]]),
        ("querySelector", "body", [[n.clone(), s_("body")], [n.clone(), n.clone()], [n.clone(), s_("body")]]),
        ("", "getElementById", [[s_(""), s_("getElementById")], [s_(""), n.clone()], [s_(""), s_("getElementById")]]),
        ("7", "body", [[s_("7"), n.clone()], [s_("7"), n.clone()], [s_("7"), s_("body")]]),
        ("avatar", "mine", [[s_("avatar"), s_("mine")], [s_("avatar"), s_("mine")], [s_("avatar"), s_("mine")]]),
    ] {
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/pic?n={}&i={}", s.port, enc(name), enc(id)), set);
        let want = serde_json::json!([want, want]);
        assert_eq!(r["seen"], want, "{} {}: {}", name, id, r);
        assert_eq!((r["alt"].as_str(), r["qs"].as_str(), r["gi"].as_str(), r["body"].as_str()), (Some("pic2"), Some("function"), Some("function"), Some("BODY")), "{} {}: {}", name, id, r);
    }
}

// ---- Q-969, found in review: a view kept for another, markup made apart, honest HTML pages -----

const APART: &str = r#"fn put(q: str) = raw("window.__" + "ok = " + json.stringify(q) + ";")
fn put2(name: str, q: str) = raw("window.__" + name + " = " + json.stringify(q) + ";")
view SvgBox(children) = <svg>{children}</svg>
view DivBox(children) = <div>{children}</div>
view Wrap(Box, q: str) = <Box><script>{put(q)}</script></Box>
view Chart(label) = <svg>{label}</svg>
view Fwd(Box, children) = <Box>{children}</Box>
let made = <script>{put2("g", "<b>made</b>")}</script>
route GET /wrap ? { q: str = "", s: str = "" } = <html><head><title>w</title></head><body><Wrap Box={s == "1" ? SvgBox : DivBox} q={q}/></body></html>
route GET /apart ? { q: str = "" } {
    let s = <script>var x = 1; {put2("a", q)}</script>
    return <html><head><title>a</title></head><body><svg>{s}</svg></body></html>
}
route GET /apart-fo ? { q: str = "" } {
    let s = <script>{put2("f", q)}</script>
    return <html><head><title>a</title></head><body><svg><foreignObject>{s}</foreignObject></svg></body></html>
}
route GET /attr ? { q: str = "" } = <html><head><title>a</title></head><body><Chart label={<script>{put2("t", q)}</script>}/></body></html>
route GET /global = <html><head><title>g</title></head><body><svg>{made}</svg></body></html>
route GET /fwd ? { q: str = "", s: str = "" } = <html><head><title>f</title></head><body><Fwd Box={s == "1" ? SvgBox : DivBox}><script>{put2("w", q)}</script></Fwd></body></html>
"#;

#[test]
fn a_view_called_through_a_parameter_has_its_own_answer_in_either_order() {
    let d = dir("kept", APART);
    let q = "<b>x</b>&quot;]]>";
    let svg = r#"<svg><script><![CDATA[window.__ok = "<b>x<\/b>&quot;]]]]><![CDATA[>";]]></script></svg>"#;
    let div = r#"<div><script>window.__ok = "<b>x<\/b>&quot;]]>";</script></div>"#;
    // A server of its own for each order, so that what it keeps is from that order.
    for order in [["0", "1"], ["1", "0"]] {
        let s = support::serve(&d.join("main.qwe"), &[]);
        for _ in 0..2 {
            for v in order {
                let a = support::send(s.port, "GET", &format!("/wrap?s={}&q={}", v, enc(q)), &[], "");
                assert_eq!(body_of(&a.body), if v == "1" { svg } else { div }, "s={} after {:?}", v, order);
            }
        }
    }
}

#[test]
fn markup_made_apart_and_placed_in_svg_is_text_to_the_browser() {
    let d = dir("apart", APART);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let q = enc("<b>x</b>&quot;]]>");
    let get = |p: &str| {
        let (path, more) = p.split_once('&').map_or((p, String::new()), |(a, b)| (a, format!("&{}", b)));
        body_of(&support::send(s.port, "GET", &format!("{}?q={}{}", path, q, more), &[], "").body).to_string()
    };
    // Made for HTML, its text in CDATA sections once it is in `<svg>`; in a `<foreignObject>`,
    // where HTML is read, as it was made. A script with nothing the parser would read as markup
    // is left as it is.
    assert_eq!(get("/apart"), r#"<svg><script><![CDATA[var x = 1; window.__a = "<b>x<\/b>&quot;]]]]><![CDATA[>";]]></script></svg>"#);
    assert_eq!(get("/attr"), r#"<svg><script><![CDATA[window.__t = "<b>x<\/b>&quot;]]]]><![CDATA[>";]]></script></svg>"#);
    assert_eq!(get("/apart-fo"), r#"<svg><foreignObject><script>window.__f = "<b>x<\/b>&quot;]]>";</script></foreignObject></svg>"#);
    assert_eq!(get("/global"), r#"<svg><script><![CDATA[window.__g = "<b>made<\/b>";]]></script></svg>"#);
    // Children forwarded to a view the walk cannot name (a parameter): written as it is safe in
    // either, the CDATA form, and the safe form in `<svg>` as well.
    assert_eq!(get("/fwd&s=1"), r#"<svg><script><![CDATA[window.__w = "<b>x<\/b>&quot;]]]]><![CDATA[>";]]></script></svg>"#);
    assert_eq!(get("/fwd&s=0"), r#"<div><script><![CDATA[window.__w = "<b>x<\/b>&quot;]]]]><![CDATA[>";]]></script></div>"#);
}

// In Chrome: a view called through a parameter, markup made apart, markup given to a view's
// attribute and a global's markup, placed in `<svg>`: no element gets out, and each script runs
// with the value as given.
#[test]
fn in_chrome_markup_made_apart_lets_no_markup_out() {
    let d = dir("apart-chrome", APART);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let script = r#"(async () => {
  await new Promise((r) => setTimeout(r, 300));
  const got = {};
  for (const k of ["ok", "a", "f", "t", "g", "w"]) got[k] = window["__" + k] ?? null;
  document.body.dataset.result = JSON.stringify({ pwn: window.__pwn ?? null, imgs: document.querySelectorAll("img").length, bs: document.querySelectorAll("b").length, got });
})();"#;
    for q in ["</script><img src=x onerror=window.__pwn=1>&quot;]]><img src=x onerror=window.__pwn=2><b>b</b>", "&quot;;window.__pwn=3;//", "<![CDATA[ ]]><img src=x onerror=window.__pwn=4>", "é ✓"] {
        // The `<div>` box first, so that a kept answer would be the wrong one for the `<svg>` box.
        support::send(s.port, "GET", &format!("/wrap?s=0&q={}", enc(q)), &[], "");
        for (path, key) in [("/wrap?s=1", "ok"), ("/wrap?s=0", "ok"), ("/apart?", "a"), ("/apart-fo?", "f"), ("/attr?", "t"), ("/global?", "g"), ("/fwd?s=1", "w")] {
            let sep = if path.ends_with('?') { "" } else { "&" };
            let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}{}{}q={}", s.port, path, sep, enc(q)), script);
            assert_eq!((r["pwn"].clone(), r["imgs"].as_u64(), r["bs"].as_u64()), (serde_json::Value::Null, Some(0), Some(0)), "{} {}: {}", path, q, r);
            let want = if key == "g" { "<b>made</b>" } else { q };
            // A visitor's own `<![CDATA[` in a script made for HTML and placed in `<svg>` is taken
            // for a section there: its markers are gone from the value, and nothing gets out.
            if !q.contains("<![CDATA[") || matches!(key, "f" | "ok") {
                assert_eq!(r["got"][key].as_str(), Some(want), "{} {}: {}", path, q, r);
            }
        }
    }
}

const HONEST: &str = r#"view Lam(children) {
    let f = (x) => <main>{x}</main>
    return f(children)
}
view Grid(children) = <div class="grid">{[children]}</div>
view Twice(children) = <div>{children}<p>{children}</p></div>
view Card(title: str, children) = <section><h2>{title}</h2>{children}</section>
view Lost(children) = <div>{store(children)}</div>
fn store(c) = <main>{c}</main>
view Both(children) = <div>{children}<svg>{children}</svg></div>
view Far(children) = <div>{inner(children)}</div>
fn inner(c) = <Icon>{c}</Icon>
view Icon(children) = <svg>{children}</svg>
route GET /lam = <html><head><title>h</title></head><body><Lam><script>{raw("window.__l = 1 < 2;")}</script></Lam></body></html>
route GET /grid = <html><head><title>h</title></head><body><Grid><script>{raw("window.__g = 1 < 2;")}</script></Grid></body></html>
route GET /twice = <html><head><title>h</title></head><body><Twice><script>{raw("window.__t = (window.__t || 0) + (1 < 2);")}</script></Twice></body></html>
route GET /card = <html><head><title>h</title></head><body><Card title="c"><script>{raw("window.__c = 1 < 2;")}</script></Card></body></html>
route GET /lost = <html><head><title>h</title></head><body><Lost><script>{raw("window.__s = 1 < 2;")}</script></Lost></body></html>
route GET /both = <html><head><title>h</title></head><body><Both><script>{raw("window.__b = 1 < 2;")}</script></Both></body></html>
route GET /far = <html><head><title>h</title></head><body><Far><script>{raw("window.__f = 1 < 2;")}</script></Far></body></html>
"#;

// A view whose body the walk cannot follow, which can reach no `<svg>` or `<math>`: its children
// stay HTML, and their scripts run; where one can be reached (`Both`, `Far` through a function
// and a view), the CDATA form stays.
#[test]
fn children_of_a_view_that_reaches_no_svg_are_html() {
    let d = dir("honest", HONEST);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| body_of(&support::send(s.port, "GET", p, &[], "").body).to_string();
    for _ in 0..2 {
        assert_eq!(get("/lam"), "<main><script>window.__l = 1 < 2;</script></main>");
        assert_eq!(get("/grid"), r#"<div class="grid"><script>window.__g = 1 < 2;</script></div>"#);
        assert_eq!(get("/twice"), "<div><script>window.__t = (window.__t || 0) + (1 < 2);</script><p><script>window.__t = (window.__t || 0) + (1 < 2);</script></p></div>");
        assert_eq!(get("/card"), "<section><h2>c</h2><script>window.__c = 1 < 2;</script></section>");
        assert_eq!(get("/lost"), "<div><main><script>window.__s = 1 < 2;</script></main></div>");
        assert_eq!(get("/both"), "<div><script><![CDATA[window.__b = 1 < 2;]]></script><svg><script><![CDATA[window.__b = 1 < 2;]]></script></svg></div>");
        assert_eq!(get("/far"), "<div><svg><script><![CDATA[window.__f = 1 < 2;]]></script></svg></div>");
    }
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let script = r#"(async () => {
  await new Promise((r) => setTimeout(r, 200));
  document.body.dataset.result = JSON.stringify({ v: ["l", "g", "t", "c", "s", "f"].map((k) => window["__" + k] ?? null) });
})();"#;
    for (path, at, want) in [("/lam", 0, serde_json::json!(true)), ("/grid", 1, serde_json::json!(true)), ("/twice", 2, serde_json::json!(2)), ("/card", 3, serde_json::json!(true)), ("/lost", 4, serde_json::json!(true)), ("/far", 5, serde_json::json!(true))] {
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}{}", s.port, path), script);
        assert_eq!(r["v"][at], want, "{}: {}", path, r);
    }
}

// ---- Q-990: `<sCript>` and `<sTyle>` -----------------------------------------------------------

// A script or a style sheet's tag in another case is refused where the program is read: the
// browser takes `<sCript>` as a script, which the renderer wrote as any element, `raw(x)` as it was.
#[test]
fn a_script_or_style_tag_in_another_case_is_refused() {
    for (name, src, line) in [
        ("case-script", "route GET / ? { q: str = \"\" } = <html><body><sCript>{raw(json.stringify(q))}</sCript></body></html>\n", "<sCript>"),
        ("case-script-svg", "route GET / ? { q: str = \"\" } = <html><body><svg><SCRIPt></SCRIPt></svg></body></html>\n", ""),
        ("case-style", "route GET / = <html><head><sTyle>{\"b { color: red }\"}</sTyle></head><body></body></html>\n", "<sTyle>"),
        ("case-component", "component Comp() {\n    state n = 0\n    <div><sCRIPT>x</sCRIPT><button onclick={() => { n += 1 }}>{n}</button></div>\n}\nroute GET / = <html><body><Comp/></body></html>\n", "<sCRIPT>"),
    ] {
        let (ok, said) = check(&dir(name, src));
        if line.is_empty() {
            // `<SCRIPt>` starts with a capital: a view's name, which there is none of.
            assert!(!ok && !said.contains("in lower case"), "{}: {}", name, said);
            continue;
        }
        assert!(!ok && said.contains(&format!("write {} in lower case", line)), "{}: {}", name, said);
        // `quillo run` refuses it too.
        let out = run_script(&dir(&format!("{}-run", name), src));
        assert!(out.contains("in lower case"), "{}: {}", name, out);
    }
    // The lower-case tags, and a view named `Script`, are as before.
    let (ok, said) = check(&dir("case-fine", "view Script(children) = <div>{children}</div>\nroute GET / = <html><head><style>{\"b { color: red }\"}</style></head><body><Script><script>var a = 1;</script></Script></body></html>\n"));
    assert!(ok, "{}", said);
}

// ---- Q-979, found in review: a form's `action` read past a named input ------------------------

const LEAVE: &str = r#"component Counter() {
    state n = 0
    <button #inc onclick={() => { n += 1; if n > 1000 { navigate("/next") } }}>{n}</button>
}
route GET / = <html><head><title>l</title></head><body><Counter/><form #f action="/next" method="get"><input name="action" value="x"/><button #go>go</button></form></body></html>
route GET /next = <html><head><title>n</title></head><body><p #done>next</p></body></html>
"#;

// The runtime reads where a form goes by its `action` attribute: `f.action` is the input named
// `action` in it. A submit it hears runs that line, and nothing fails. (A page whose code calls
// `navigate` has the runtime's part that hears them.)
#[test]
fn in_chrome_a_form_with_an_input_named_action_is_read_by_its_attribute() {
    let d = dir("leave", LEAVE);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  // Reaching for the counter brings the runtime, which hears the page's submits.
  document.getElementById("inc").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  for (let i = 0; i < 600 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(25);
  const errors = [];
  window.addEventListener("error", (e) => errors.push(String(e.message)));
  const f = document.getElementById("f");
  // Where the runtime takes the form to go, as it makes its address of it.
  const to = [], U = window.URL;
  window.URL = function (a, b) { to.push(String(a)); return new U(a, b); };
  // A submit event the page hears, as the browser's own would be, without leaving the page.
  f.dispatchEvent(new SubmitEvent("submit", { bubbles: true, cancelable: true }));
  window.URL = U;
  await tick(50);
  document.body.dataset.result = JSON.stringify({ booted: !!(window[Symbol.for("quillo")] || {}).booted, errors, to, clobbered: f.action instanceof HTMLInputElement });
})();"#;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), script);
    assert_eq!(r, serde_json::json!({ "booted": true, "errors": [], "to": ["/next"], "clobbered": true }));
}

// ---- Q-969, second review: a view kept from HTML, a `<p>` written by an expression -----------

const B2: &str = r#"fn put(q: str) = raw("window.__ok = " + json.stringify(q) + ";")
view V(s) = <g>{s}</g>
route GET /vc ? { q: str = "", c: str = "1" } {
    let s = <script>{put(q)}</script>
    return <html><head><title>v</title></head><body><div><V s={s}/></div><svg>{c == "1" ? <V s={s}/> : ""}</svg></body></html>
}
route GET /vcfor ? { q: str = "" } {
    let s = <script>{put(q)}</script>
    return <html><head><title>v</title></head><body><div><V s={s}/></div><svg>{for i in [1] => <V s={s}/>}</svg></body></html>
}
route GET /vchtml ? { q: str = "" } {
    let s = <script>{put(q)}</script>
    return <html><head><title>v</title></head><body><div><V s={s}/></div></body></html>
}
route GET /vcsvg ? { q: str = "" } {
    let s = <script>{put(q)}</script>
    return <html><head><title>v</title></head><body><svg>{true ? <V s={s}/> : ""}</svg></body></html>
}
route GET /hide ? { q: str = "", c: str = "1" } = <html><head><title>h</title></head><body><svg><g><foreignObject><math><g>{c == "1" ? <p>x</p> : ""}</g></math><script>{put(q)}</script></foreignObject></g></svg></body></html>
route GET /hidefor ? { q: str = "" } = <html><head><title>h</title></head><body><svg><g><foreignObject><math><g>{for i in [1] => <p>x</p>}</g></math><script>{put(q)}</script></foreignObject></g></svg></body></html>
route GET /hidefont ? { q: str = "", c: str = "1" } = <html><head><title>h</title></head><body><svg><g><foreignObject><math><g>{c == "1" ? <font color="red">x</font> : ""}</g></math><script>{put(q)}</script></foreignObject></g></svg></body></html>
route GET /hidestatic ? { q: str = "" } = <html><head><title>h</title></head><body><svg><g><foreignObject><math><g><p>x</p></g></math><script>{put(q)}</script></foreignObject></g></svg></body></html>
"#;

#[test]
fn a_view_kept_from_html_and_a_p_written_by_an_expression_let_no_markup_out() {
    let d = dir("b2", B2);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let q = "<b>x</b>&quot;]]>";
    let get = |p: &str| {
        let sep = if p.contains('?') { "&" } else { "?" };
        body_of(&support::send(s.port, "GET", &format!("{}{}q={}", p, sep, enc(q)), &[], "").body).to_string()
    };
    let html = r#"<script>window.__ok = "<b>x<\/b>&quot;]]>";</script>"#;
    let svg = r#"<script><![CDATA[window.__ok = "<b>x<\/b>&quot;]]]]><![CDATA[>";]]></script>"#;
    for _ in 0..2 {
        // The view made in HTML first, then in `<svg>` in a branch, a loop, and the next request.
        assert_eq!(get("/vc"), format!("<div><g>{html}</g></div><svg><g>{svg}</g></svg>"));
        assert_eq!(get("/vcfor"), format!("<div><g>{html}</g></div><svg><g>{svg}</g></svg>"));
        assert_eq!(get("/vchtml"), format!("<div><g>{html}</g></div>"));
        assert_eq!(get("/vcsvg"), format!("<svg><g>{svg}</g></svg>"));
        // A `<p>` (or a `<font color>`) an expression writes in MathML in a `<foreignObject>`: what
        // follows is not known (a later `</g>` may close the SVG `<g>` outside), the safe form.
        for (path, mid) in [("/hide", "<p>x</p>"), ("/hidefor", "<p>x</p>"), ("/hidefont", r#"<font color="red">x</font>"#), ("/hidestatic", "<p>x</p>")] {
            assert_eq!(get(path), format!("<svg><g><foreignObject><math><g>{mid}</g></math>{svg}</foreignObject></g></svg>"), "{}", path);
        }
    }
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let script = r#"(async () => {
  await new Promise((r) => setTimeout(r, 300));
  document.body.dataset.result = JSON.stringify({ pwn: window.__pwn ?? null, imgs: document.querySelectorAll("img").length, bs: document.querySelectorAll("b").length, ok: window.__ok ?? null });
})();"#;
    let q = "</script><img src=x onerror=window.__pwn=1>]]><img src=x onerror=window.__pwn=2><![CDATA[<b>x</b>";
    for path in ["/vchtml", "/vcsvg", "/vc", "/vcfor", "/hide", "/hidefor", "/hidefont", "/hidestatic"] {
        let sep = if path.contains('?') { "&" } else { "?" };
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}{}{}q={}", s.port, path, sep, enc(q)), script);
        assert_eq!((r["pwn"].clone(), r["imgs"].as_u64(), r["bs"].as_u64()), (serde_json::Value::Null, Some(0), Some(0)), "{}: {}", path, r);
        // A script the browser reads runs with the value as given (this one holds a visitor's own
        // `<![CDATA[`, whose markers a script made for HTML and placed in `<svg>` loses); one in
        // the safe form where it reads HTML (`/hide`, `<foreignObject>`) does not run.
        if path == "/vchtml" {
            assert_eq!(r["ok"].as_str(), Some(q), "{}: {}", path, r);
        } else if !path.starts_with("/hide") {
            assert!(r["ok"].is_string(), "{}: {}", path, r);
        }
    }
}
