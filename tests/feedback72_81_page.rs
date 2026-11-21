//! FEEDBACK72.md, FEEDBACK73.md, FEEDBACK79.md, FEEDBACK80.md and FEEDBACK81.md, slice P (pages:
//! rendering and the browser runtime): a value in a `style` template escaped as CSS (Q-1250);
//! HTML's void names as XML's elements in an XML answer (Q-1259); an error value in markup shows
//! nothing and is said once (Q-1272); `state` in an `if` says why (Q-1271); lower case `onchange`
//! is the DOM's `change` (Q-1270, Q-1025); a form's reset sets the states bound to its fields
//! (Q-1269); a scroll alone takes up a component that listens to scrolls (Q-1010).
//! Branch feedback72-81.

mod support;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// A folder of its own, removed when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Dir {
    fn new(name: &str, src: &str) -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = support::temp_dir(&format!("f7281p-{}-{}", name, N.fetch_add(1, Ordering::SeqCst)));
        std::fs::write(d.join("main.qwe"), src).unwrap();
        Dir(d)
    }

    fn main(&self) -> PathBuf {
        self.0.join("main.qwe")
    }
}

/// The quillo under test: this build's, or the one QUILLO_TEST_BIN names (main's, say, to see a
/// test fail there).
fn quillo() -> PathBuf {
    std::env::var_os("QUILLO_TEST_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_quillo")))
}

/// The process's end within `within`, or None (it is then killed).
fn wait_for(c: &mut Child, within: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + within;
    loop {
        if let Some(s) = c.try_wait().unwrap() {
            return Some(s);
        }
        if Instant::now() > deadline {
            let _ = c.kill();
            let _ = c.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `quillo <cmd> main.qwe` on `src`, under a deadline: whether it succeeded, and what it printed
/// (stdout, then stderr).
fn quillo_on(cmd: &str, src: &str) -> (bool, String) {
    let d = Dir::new(cmd, src);
    let mut c = Command::new(quillo()).arg(cmd).arg(d.main()).current_dir(&d.0).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    // Read as it goes, so that a long output cannot fill the pipe and hold the process.
    let (mut so, mut se) = (c.stdout.take().unwrap(), c.stderr.take().unwrap());
    let t1 = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = so.read_to_string(&mut s);
        s
    });
    let t2 = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = se.read_to_string(&mut s);
        s
    });
    let status = wait_for(&mut c, Duration::from_secs(120)).unwrap_or_else(|| panic!("quillo {} took too long:\n{}", cmd, src));
    (status.success(), t1.join().unwrap() + &t2.join().unwrap())
}

/// `src` passes `quillo check` (warnings allowed), then runs: what it printed.
fn run(src: &str) -> String {
    let (ok, out) = quillo_on("check", src);
    assert!(ok, "quillo check:\n{}\n{}", out, src);
    let (ok, out) = quillo_on("run", src);
    assert!(ok, "quillo run:\n{}\n{}", out, src);
    out
}

/// What `quillo check` says of `src`, which it refuses.
fn refused(src: &str) -> String {
    let (ok, out) = quillo_on("check", src);
    assert!(!ok, "quillo check took it:\n{}\n{}", out, src);
    out
}

/// A server for `src` (checked first), its log in a file of the folder.
struct Served {
    _dir: Dir,
    log: PathBuf,
    port: u16,
    _guard: support::Guard,
}

fn served(name: &str, src: &str) -> Served {
    let (ok, out) = quillo_on("check", src);
    assert!(ok, "quillo check:\n{}\n{}", out, src);
    let dir = Dir::new(name, src);
    let log = dir.0.join("log.txt");
    let mut child = Command::new(quillo())
        .arg("run")
        .arg(dir.main())
        .args(["--port", "0"])
        .current_dir(&dir.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let port = support::port_in_log(&mut child, &log, Duration::from_secs(30));
    Served { _dir: dir, log, port, _guard: support::Guard::new(child) }
}

impl Served {
    fn get(&self, path: &str) -> support::Answer {
        support::send(self.port, "GET", path, &[], "")
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// The log once it holds `n` lines with `what` (the server writes it as it goes).
    fn log_with(&self, what: &str, n: usize) -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let log = self.log();
            if log.matches(what).count() >= n || Instant::now() > deadline {
                return log;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }
}

fn chrome() -> Option<PathBuf> {
    let node = Command::new("node").args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"]).output();
    if !node.map(|o| o.status.success()).unwrap_or(false) {
        return None;
    }
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium", "/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// The page script around `body`: once the page has loaded (and, with `boot`, once the runtime has
/// started), `body` runs with `$`, `tick`, `type_in` and `QW`; what it returns is the result, with
/// the errors the runtime reported and how many components it made again.
fn script(body: &str, boot: bool) -> String {
    format!(
        r#"(async () => {{
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const QW0 = () => window[Symbol.for("quillo")] || {{}};
  if ({boot}) for (let i = 0; i < 300 && !QW0().booted; i++) await tick(30);
  const QW = QW0();
  const $ = (id) => document.getElementById(id);
  const type_in = async (id, ...vs) => {{ const el = $(id); for (const v of vs) {{ el.value = v; el.dispatchEvent(new Event("input", {{ bubbles: true }})); await tick(40); }} }};
  let out;
  try {{ out = await (async () => {{
{body}
  }})(); }} catch (e) {{ out = {{ exception: String((e && e.stack) || e) }}; }}
  out.remade = QW0().remade ?? 0;
  out.errors = window.__quilloErrors;
  document.body.dataset.result = JSON.stringify(out);
}})();"#
    )
}

/// `src` served (checked first), and `body` run at `path` in Chrome: what it gave back. None
/// without Chrome.
fn in_page(name: &str, src: &str, path: &str, body: &str, boot: bool) -> Option<serde_json::Value> {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return None;
    };
    let s = served(name, src);
    // One Chrome at a time: several at once overload small machines.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let t = s._dir.0.join("t.js");
    std::fs::write(&t, script(body, boot)).unwrap();
    let run = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/run.js");
    // run.js ends itself 10 s after its own deadline, whatever Chrome does.
    let out = Command::new("node")
        .env_remove("QUILLO_PAGE_FOCUS")
        .env_remove("QUILLO_PAGE_CDP")
        .arg(&run)
        .arg(&chrome)
        .arg(format!("http://127.0.0.1:{}{}", s.port, path))
        .arg(s._dir.0.join("profile"))
        .arg("60000")
        .arg(&t)
        .output()
        .expect("node");
    assert!(out.status.success(), "{}: {}\n{}", path, String::from_utf8_lossy(&out.stderr), s.log());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)));
    assert!(v.get("exception").is_none(), "{}", v);
    Some(v)
}

// ---- Q-1250: a value in a `style` template is a CSS value ----------------------------------

/// What a visitor may send, and what the page then says: one value, its `{`, `}`, `;` and `\` as
/// CSS's escapes, as a style map's are (Q-668); `&` too, as css`...`'s.
#[test]
fn a_visitors_value_in_a_style_template_stays_one_value() {
    let cases: &[(&str, &str)] = &[
        ("red; position: fixed; inset: 0; background: red", r"color: red\3b  position: fixed\3b  inset: 0\3b  background: red"),
        ("red} p { color: blue", r"color: red\7d  p \7b  color: blue"),
        ("a\\", r"color: a\5c "),
        ("\"x\"; top: 0", r"color: &quot;x&quot;\3b  top: 0"),
        ("red /* a */; top: 0", r"color: red /* a */\3b  top: 0"),
        ("url(https://other.example/p.png); top: 0", r"color: url(https://other.example/p.png)\3b  top: 0"),
        ("red\n; top: 0", "color: red\n\\3b  top: 0"),
        ("é—😀; top: 0", r"color: é—😀\3b  top: 0"),
        ("a&b", r"color: a\26 b"),
        ("<b>;", r"color: &lt;b&gt;\3b "),
        (";", r"color: \3b "),
        ("", "color: "),
    ];
    let list = cases.iter().map(|(v, _)| serde_json::to_string(v).unwrap()).collect::<Vec<_>>().join(", ");
    // Each written by the fast way (no `class=`) and by the general one (with `class=`, a spread).
    let src = format!("let vals = [{}]\nfor v in vals {{\n    print(<p style={{`color: ${{v}}`}}>a</p>)\n    print(<p class=\"n\" style={{`color: ${{v}}`}}>a</p>)\n}}\n", list);
    let out = run(&src);
    for (v, want) in cases {
        assert!(out.contains(&format!("<p style=\"{}\">a</p>", want)), "{:?}: {}", v, out);
        assert!(out.contains(&format!("<p class=\"n\" style=\"{}\">a</p>", want)), "{:?} (general): {}", v, out);
    }
}

/// What a style means to write stays as it is: colors, lengths, functions, quoted names, numbers;
/// the template's own text, `;` and all; a whole value (`style={s}`), which cannot be cut; and a
/// style map.
#[test]
fn a_style_template_that_was_right_is_written_as_before() {
    let out = run(r##"let vals = ["#fff", "12px", "rgb(1, 2, 3)", "\"Helvetica Neue\", serif", "calc(100% - 2px)", "var(--gap)", "-1.5em"]
for v in vals {
    print(<p style={`font: ${v}`}>a</p>)
}
let n = 12
let f = 1.5
print(<i style={`width: ${n}px; height: ${f}em; margin: ${-3}px`}/>)
let c = "red"
print(<b style={`a: b; color: ${c}; top: 1px`}>x</b>)
let s = "color: red; top: 0"
print(<b style={s}>x</b>)
print(<b style={{ color: "red; top: 0" }}>x</b>)
print(<b STYLE={`color: ${trim(" red; top: 0")}`}>x</b>)
print(<b title={`a; ${"b; c"}`}>x</b>)
"##);
    for want in [
        r##"<p style="font: #fff">a</p>"##,
        r#"<p style="font: 12px">a</p>"#,
        r#"<p style="font: rgb(1, 2, 3)">a</p>"#,
        r#"<p style="font: &quot;Helvetica Neue&quot;, serif">a</p>"#,
        r#"<p style="font: calc(100% - 2px)">a</p>"#,
        r#"<p style="font: var(--gap)">a</p>"#,
        r#"<p style="font: -1.5em">a</p>"#,
        r#"<i style="width: 12px; height: 1.5em; margin: -3px"></i>"#,
        r#"<b style="a: b; color: red; top: 1px">x</b>"#,
        // A whole value, as before (another check warns of it).
        r#"<b style="color: red; top: 0">x</b>"#,
        r#"<b style="color: red\3b  top: 0">x</b>"#,
        r#"<b STYLE="color: red\3b  top: 0">x</b>"#,
        // Not a style: as written.
        r#"<b title="a; b; c">x</b>"#,
    ] {
        assert!(out.contains(want), "{}\n{}", want, out);
    }
}

/// In the browser, the server's page is taken up as it was written (no component made again),
/// and a value typed in is escaped as on the server: the declaration it carries does nothing.
#[test]
fn a_style_template_in_a_component_is_escaped_in_the_browser_too() {
    let src = r#"component Note() {
    state c = "red; background: blue"
    state shown = false
    <div>
        <p id="p" style={`color: ${c}`}>a</p>
        <input id="i" value={c}/>
        <button id="show" onclick={() => { shown = !shown }}>show</button>
        {shown ? <p id="q" style={`color: ${c}`}>q</p> : null}
    </div>
}
route GET / = <html><body><Note/></body></html>
"#;
    let Some(v) = in_page(
        "style",
        src,
        "/",
        r#"const look = (id) => { const s = getComputedStyle($(id)); return [s.color, s.backgroundColor]; };
  const show = async () => { $("show").click(); await tick(60); };
  const first = [$("p").getAttribute("style"), ...look("p")];
  await type_in("i", "green; background: red");
  const typed = look("p");
  await show();
  const made = [$("q").getAttribute("style"), ...look("q")];
  await type_in("i", "green");
  const plain = [look("p")[0], look("q")[0]];
  await show();
  await type_in("i", "é}{\\;");
  await show();
  return { first, typed, made, plain, odd: $("q").getAttribute("style") };"#,
        true,
    ) else {
        return;
    };
    // Unescaped, the browser's page would show green on red: one declaration, which is no color.
    assert_eq!(v["first"], serde_json::json!([r"color: red\3b  background: blue", "rgb(0, 0, 0)", "rgba(0, 0, 0, 0)"]), "{}", v);
    assert_eq!(v["typed"], serde_json::json!(["rgb(0, 0, 0)", "rgba(0, 0, 0, 0)"]), "{}", v);
    assert_eq!(v["made"], serde_json::json!([r"color: green\3b  background: red", "rgb(0, 0, 0)", "rgba(0, 0, 0, 0)"]), "{}", v);
    assert_eq!(v["plain"], serde_json::json!(["rgb(0, 128, 0)", "rgb(0, 128, 0)"]), "{}", v);
    assert_eq!(v["odd"], r"color: é\7d \7b \5c \3b ", "{}", v);
    assert_eq!(v["remade"], 0, "{}", v);
    assert_eq!(v["errors"], serde_json::json!([]), "{}", v);
}

/// A template a branch gives is escaped as a bare one is: `? :`, `if`, `match`, `??`, `&&`, in
/// parentheses, and by the general way (with `class=`).
#[test]
fn a_style_template_in_a_branch_is_escaped_too() {
    let out = run(r#"let c = trim(" red; top: 0 ")
let dark = true
let k = "a"
print(<p style={dark ? `color: ${c}` : "color: black"}>a</p>)
print(<p style={if dark { `color: ${c}` } else { "x" }}>b</p>)
print(<p style={match k {
    "a" => `color: ${c}`
    _ => ""
}}>c</p>)
print(<p style={null ?? `color: ${c}`}>d</p>)
print(<p style={dark && `color: ${c}`}>e</p>)
print(<p style={(`color: ${c}`)}>f</p>)
print(<p class="n" style={dark ? `color: ${c}` : ""}>g</p>)
print(<p style={dark ? "color: black; top: 0" : `x`}>h</p>)
"#);
    for t in ["a", "b", "c", "d", "e", "f"] {
        assert!(out.contains(&format!("<p style=\"color: red\\3b  top: 0\">{}</p>", t)), "{}: {}", t, out);
    }
    assert!(out.contains("<p class=\"n\" style=\"color: red\\3b  top: 0\">g</p>"), "{}", out);
    // A whole string is the program's, as before.
    assert!(out.contains("<p style=\"color: black; top: 0\">h</p>"), "{}", out);
}

/// What the parser knows the program wrote writes its declarations: a string, a choice of strings
/// (`? :`, `match`, `??`, `||`, `&&` whose other side is a value: it is escaped, the string stays);
/// a name stays one value, a top-level `let` of a string's too (a visitor's value could say the
/// same: decided in review), and a parameter.
#[test]
fn the_programs_own_text_in_a_style_template_writes_its_declarations() {
    let out = run(r#"let base = "display: flex; gap: 1px"
let pad = true ? "padding: 1px;" : ""
let hidden = true
let c = trim(" red; top: 0 ")
fn f(base) = <p style={`${base}; width: 2px`}>shadowed</p>
print(<p style={`${hidden ? "display: none;" : ""} color: red`}>a</p>)
print(<p style={`${base}; width: 2px`}>b</p>)
print(<p style={`${pad} ${"margin: 0;"} color: ${c}`}>c</p>)
print(<p style={`${match hidden {
    true => "x: 1;"
    _ => null
}}`}>d</p>)
print(<p class="n" style={`${base}; width: 2px`}>e</p>)
print(<p style={`${null ?? "x: 1;"}${"" || "y: 2;"} color: red`}>g</p>)
print(<p style={`${hidden && "x: 1;"} color: red`}>h</p>)
print(f("a; b"))
"#);
    for want in [
        r#"<p style="display: none; color: red">a</p>"#,
        r#"<p style="display: flex\3b  gap: 1px; width: 2px">b</p>"#,
        r#"<p style="padding: 1px\3b  margin: 0; color: red\3b  top: 0">c</p>"#,
        r#"<p style="x: 1;">d</p>"#,
        r#"<p class="n" style="display: flex\3b  gap: 1px; width: 2px">e</p>"#,
        r#"<p style="x: 1; color: red">g</p>"#,
        r#"<p style="x: 1; color: red">h</p>"#,
        r#"<p style="a\3b  b; width: 2px">shadowed</p>"#,
    ] {
        assert!(out.contains(want), "{}\n{}", want, out);
    }
}

/// The browser writes the same, byte for byte, in what it makes after the page loaded: the
/// program's text as it is, a name (a top-level `let`'s too) as one value, a template from a branch
/// escaped.
#[test]
fn the_browser_writes_the_programs_style_text_as_the_server_does() {
    let src = r#"let base = "display: flex; gap: 1px"
component C() {
    state hidden = true
    state c = "red; top: 0"
    state shown = false
    <div>
        <p id="a" style={`${hidden ? "display: none;" : ""} color: red`}>a</p>
        <p id="b" style={`${base}; color: ${c}`}>b</p>
        <p id="c" style={hidden ? `color: ${c}` : "color: black"}>c</p>
        <button id="t" onclick={() => { shown = true }}>t</button>
        {shown ? <p id="a2" style={`${hidden ? "display: none;" : ""} color: red`}>a</p> : null}
        {shown ? <p id="b2" style={`${base}; color: ${c}`}>b</p> : null}
        {shown ? <p id="c2" style={hidden ? `color: ${c}` : "color: black"}>c</p> : null}
    </div>
}
route GET / = <html><body><C/></body></html>
"#;
    let Some(v) = in_page(
        "style-text",
        src,
        "/",
        r#"const st = (id) => $(id).getAttribute("style");
  const server = ["a", "b", "c"].map(st);
  $("t").click();
  await tick(100);
  return { server, made: ["a2", "b2", "c2"].map(st) };"#,
        true,
    ) else {
        return;
    };
    let want = serde_json::json!(["display: none; color: red", r"display: flex\3b  gap: 1px; color: red\3b  top: 0", r"color: red\3b  top: 0"]);
    assert_eq!(v["server"], want, "{}", v);
    assert_eq!(v["made"], want, "{}", v);
    assert_eq!(v["remade"], 0, "{}", v);
    assert_eq!(v["errors"], serde_json::json!([]), "{}", v);
}

/// Go's html/template's way: each value put into a template that reaches a style is one CSS value,
/// each piece of text the program wrote stays: a template in a branch keeps its own text, `&&` and
/// `??` keep their strings, a template inside `+`, `trim`, `join` or a block is looked into, a
/// component's `style` prop and a spread's `style` are styles, css`...` passes; a template made in
/// a helper and given whole is the program's value (second review).
#[test]
fn every_value_in_a_style_is_one_value_and_the_programs_text_stays() {
    let out = run(r#"let v = trim(" red; background: blue ")
let n = 20
let big = true
let small = false
let hidden = true
let c = "green"
let extra = "; top: 0"
fn box_style() = `color: ${v}`
view Box(style) = <div style={style}>y</div>
print(<p style={`${big ? `font-size: ${n}px;` : ""} color: ${c}`}>a</p>)
print(<p style={`${big && "font-weight: 700;"} color: red`}>b</p>)
print(<p style={`${small && "font-weight: 700;"} color: red`}>b2</p>)
print(<p style={`${hidden ? "x: 1;" : v}`}>c</p>)
print(<p style={`${hidden ? v : "x: 1;"}`}>c2</p>)
print(<p style={`color: ${v};` + extra}>d</p>)
print(<p style={trim(` color: ${v} `)}>e</p>)
print(<p style={join([`color: ${v}`, "top: 0"], "; ")}>f</p>)
print(<p style={{
    let s = `color: ${v}`
    s
}}>g</p>)
print(<p style={css`color: ${v}; top: 0`}>h</p>)
print(<p style={`${css`margin: 0; padding: ${n}px;`} color: ${v}`}>i</p>)
print(<p style={box_style()}>j</p>)
print(<p style={`${v ?? "black"}`}>k</p>)
print(<p style={`${null ?? "black;"}`}>l</p>)
print(<Box style={`color: ${v}`}/>)
print(<p {...{ style: `color: ${v}` }}>s</p>)
"#);
    for want in [
        r#"<p style="font-size: 20px; color: green">a</p>"#,
        r#"<p style="font-weight: 700; color: red">b</p>"#,
        r#"<p style="false color: red">b2</p>"#,
        r#"<p style="x: 1;">c</p>"#,
        r#"<p style="red\3b  background: blue">c2</p>"#,
        r#"<p style="color: red\3b  background: blue;; top: 0">d</p>"#,
        r#"<p style="color: red\3b  background: blue">e</p>"#,
        r#"<p style="color: red\3b  background: blue; top: 0">f</p>"#,
        r#"<p style="color: red\3b  background: blue">g</p>"#,
        r#"<p style="color: red\3b  background: blue; top: 0">h</p>"#,
        r#"<p style="margin: 0; padding: 20px; color: red\3b  background: blue">i</p>"#,
        // A template made apart and given whole: the program's own value.
        r#"<p style="color: red; background: blue">j</p>"#,
        r#"<p style="red\3b  background: blue">k</p>"#,
        r#"<p style="black;">l</p>"#,
        r#"<div style="color: red\3b  background: blue">y</div>"#,
        r#"<p style="color: red\3b  background: blue">s</p>"#,
    ] {
        assert!(out.contains(want), "{}\n{}", want, out);
    }
}

/// In Chrome, what the server wrote and what the browser makes say the same, byte for byte; the
/// review's font size and color apply again; a visitor's `red; background: blue` through `+`,
/// `trim`, `join`, a spread and a component's prop stays one value, which paints no background.
#[test]
fn the_browser_writes_every_style_as_the_server_does() {
    let src = r#"component Card(style) {
    <div class="card" style={`padding: 8px; ${style}`}>x</div>
}
component P() {
    state v = "red; background: blue"
    state n = 20
    state big = true
    state c = "green"
    state shown = false
    <div>
        <section id="server">
            <p style={`${big ? `font-size: ${n}px;` : ""} color: ${c}`}>a</p>
            <p style={`color: ${v};` + " top: 0"}>d</p>
            <p style={trim(` color: ${v} `)}>e</p>
            <p style={join([`color: ${v}`, "top: 0"], "; ")}>f</p>
            <p {...{ style: `color: ${v}` }}>s</p>
            <Card style={`color: ${v}`}/>
        </section>
        <button id="t" onclick={() => { shown = true }}>t</button>
        {shown ? <section id="browser">
            <p style={`${big ? `font-size: ${n}px;` : ""} color: ${c}`}>a</p>
            <p style={`color: ${v};` + " top: 0"}>d</p>
            <p style={trim(` color: ${v} `)}>e</p>
            <p style={join([`color: ${v}`, "top: 0"], "; ")}>f</p>
            <p {...{ style: `color: ${v}` }}>s</p>
            <Card style={`color: ${v}`}/>
        </section> : null}
    </div>
}
route GET / = <html><body><P/></body></html>
"#;
    let Some(v) = in_page(
        "styles-all",
        src,
        "/",
        r#"const look = (sec) => [...$(sec).querySelectorAll("p, .card")].map((e) => { const s = getComputedStyle(e); return [e.getAttribute("style"), s.fontSize, s.color, s.backgroundColor]; });
  const server = look("server");
  $("t").click();
  await tick(100);
  return { server, browser: look("browser") };"#,
        true,
    ) else {
        return;
    };
    let none = "rgba(0, 0, 0, 0)";
    let one = r"color: red\3b  background: blue";
    let want = serde_json::json!([
        ["font-size: 20px; color: green", "20px", "rgb(0, 128, 0)", none],
        [format!("{}; top: 0", one), "16px", "rgb(0, 0, 0)", none],
        [one, "16px", "rgb(0, 0, 0)", none],
        [format!("{}; top: 0", one), "16px", "rgb(0, 0, 0)", none],
        [one, "16px", "rgb(0, 0, 0)", none],
        // Escaped at the call, where the template written in the `style` prop is trusted CSS since
        // the user's decision, which the component's style lets through: escaped once, where it was
        // escaped twice (`\5c 3b`), still one value (FEEDBACK79.md, Q-1250; branch feedback72-81).
        [r"padding: 8px; color: red\3b  background: blue", "16px", "rgb(0, 0, 0)", none],
    ]);
    assert_eq!(v["server"], want, "{}", v);
    assert_eq!(v["browser"], want, "{}", v);
    assert_eq!(v["remade"], 0, "{}", v);
    assert_eq!(v["errors"], serde_json::json!([]), "{}", v);
}

/// Every template in a style's expression is style text, wherever it stands: a method's receiver,
/// an index, a field, a `?`, a map given to a call, a `for`, a loop that builds the text in a
/// block, a lambda's body; an explicit `css(...)` call is an ordinary call; an error's contents are
/// escaped, markup and a map's keys too, and css`...` in a `<style>` is Q-668's again (third
/// review).
#[test]
fn every_template_in_a_style_is_style_text_and_an_errors_contents_are_escaped() {
    let out = run(r#"let v = trim(" red; background: blue ")
let ds = [{ k: "color", v: v }]
fn f() = `color: ${v}`
print(<p style={[`color: ${v}`, "top: 0"].join("; ")}>listjoin</p>)
print(<p style={`color: ${v}`?}>try</p>)
print(<p style={`color: ${v}`.trim()}>trim</p>)
print(<p style={`color: ${v}`.replace("x", "y")}>replace</p>)
print(<p style={[`color: ${v}`][0]}>index</p>)
print(<p style={{ a: `color: ${v}` }.a}>field</p>)
print(<p style={join(values({ a: `color: ${v}` }), "; ")}>mapcall</p>)
print(<p style={join(for d in ds => `${d.k}: ${d.v}`, "; ")}>for</p>)
print(<p style={ds.map((d) => `${d.k}: ${d.v}`).join("; ")}>lambda</p>)
print(<p style={{
    let s = ""
    for d in ds {
        s = s + `${d.k}: ${d.v};`
    }
    s
}}>loop</p>)
print(<p style={f()}>helper</p>)
print(<p style={css(`color: ${v}`)}>css</p>)
print(<p style={`a: 1; ${css(`color: ${v}`)}`}>innercss</p>)
print(<p style={css`color: ${v}; top: 0`}>tagged</p>)
print(<p style={`color: ${err(<b>{"x); background: blue; y: ("}</b>)}`}>errhtml</p>)
print(<p style={`color: ${err(set({}, "x); background: blue; y: (", 1))}`}>errkey</p>)
print(<p style={`color: ${err(v) ?? "black"}`}>errchoice</p>)
print(<style>{css`.a { color: ${err("x} body { display: none } .y {")} }`}</style>)
"#);
    for want in [
        r#"<p style="color: red\3b  background: blue; top: 0">listjoin</p>"#,
        r#"<p style="color: red\3b  background: blue">trim</p>"#,
        r#"<p style="color: red\3b  background: blue">try</p>"#,
        r#"<p style="color: red\3b  background: blue">replace</p>"#,
        r#"<p style="color: red\3b  background: blue">index</p>"#,
        r#"<p style="color: red\3b  background: blue">field</p>"#,
        r#"<p style="color: red\3b  background: blue">mapcall</p>"#,
        r#"<p style="color: red\3b  background: blue">for</p>"#,
        r#"<p style="color: red\3b  background: blue">lambda</p>"#,
        r#"<p style="color: red\3b  background: blue;">loop</p>"#,
        // A function's text, given whole: the program's own value, as decided.
        r#"<p style="color: red; background: blue">helper</p>"#,
        r#"<p style="color: red\3b  background: blue">css</p>"#,
        // Its values escaped by the walk, the inner call is css`...`'s form, trusted CSS since the
        // user's decision, which the outer value lets through: escaped once, where it was escaped
        // twice (`\5c 3b`), still one value (FEEDBACK79.md, Q-1250; branch feedback72-81).
        r#"<p style="a: 1; color: red\3b  background: blue">innercss</p>"#,
        r#"<p style="color: red\3b  background: blue; top: 0">tagged</p>"#,
        r#"<p style="color: err(&quot;&lt;b&gt;x)\\3b  background: blue\\3b  y: (&lt;/b&gt;&quot;)">errhtml</p>"#,
        r#"<p style="color: err(&quot;\\7b  \&quot;x)\\3b  background: blue\\3b  y: (\&quot;: 1 \\7d &quot;)">errkey</p>"#,
        r#"<p style="color: black">errchoice</p>"#,
        r#"<style>.a { color: err("x\\7d  body \\7b  display: none \\7d  .y \\7b ") }</style>"#,
    ] {
        assert!(out.contains(want), "{}\n{}", want, out);
    }
}

/// A fn declared inside a style's block (or inside a lambda there) and a lambda's parameter
/// defaults are style text too: 10bf35a walked neither, so a visitor's value added a declaration
/// (found verifying 10bf35a; FEEDBACK79.md, Q-1250; branch feedback72-81). The program's own text
/// through the same paths is written as before.
#[test]
fn a_fn_declared_in_a_style_and_a_lambdas_defaults_are_style_text() {
    let out = run(r#"let v = trim(" red; background: blue ")
let ok = "green"
print(<p style={{
    fn mk(x) = `color: ${x}`
    mk(v)
}}>fnblock</p>)
print(<p style={((x = `color: ${v}`) => x)()}>default</p>)
print(<p style={[1].map((n) => {
    fn mk(x) = `color: ${x}`
    mk(v)
}).join("")}>fninlambda</p>)
print(<p style={{
    fn mk(x) = `color: ${x}; top: 0`
    mk(ok)
}}>legit</p>)
"#);
    for want in [
        r#"<p style="color: red\3b  background: blue">fnblock</p>"#,
        r#"<p style="color: red\3b  background: blue">default</p>"#,
        r#"<p style="color: red\3b  background: blue">fninlambda</p>"#,
        r#"<p style="color: green; top: 0">legit</p>"#,
    ] {
        assert!(out.contains(want), "{}\n{}", want, out);
    }
}

/// The browser writes the same as the server, byte for byte, for each of those it can make.
#[test]
fn the_browser_writes_those_styles_as_the_server_does() {
    let src = r#"component C() {
    state v = "red; background: blue"
    state ds = [{ k: "color", v: "red; background: blue" }]
    state shown = false
    <div>
        <section id="server">
            <p style={[`color: ${v}`, "top: 0"].join("; ")}>1</p>
            <p style={`color: ${v}`.trim()}>2</p>
            <p style={join(for d in ds => `${d.k}: ${d.v}`, "; ")}>3</p>
            <p style={ds.map((d) => `${d.k}: ${d.v}`).join("; ")}>4</p>
            <p style={css(`color: ${v}`)}>5</p>
            <p style={`color: ${err(v)}`}>6</p>
            <p style={`color: ${err(v) ?? "black"}`}>7</p>
        </section>
        <button id="t" onclick={() => { shown = true }}>t</button>
        {shown ? <section id="browser">
            <p style={[`color: ${v}`, "top: 0"].join("; ")}>1</p>
            <p style={`color: ${v}`.trim()}>2</p>
            <p style={join(for d in ds => `${d.k}: ${d.v}`, "; ")}>3</p>
            <p style={ds.map((d) => `${d.k}: ${d.v}`).join("; ")}>4</p>
            <p style={css(`color: ${v}`)}>5</p>
            <p style={`color: ${err(v)}`}>6</p>
            <p style={`color: ${err(v) ?? "black"}`}>7</p>
        </section> : null}
    </div>
}
route GET / = <html><body><C/></body></html>
"#;
    let Some(v) = in_page(
        "styles-third",
        src,
        "/",
        r#"const look = (sec) => [...$(sec).querySelectorAll("p")].map((e) => [e.getAttribute("style"), getComputedStyle(e).backgroundColor]);
  const server = look("server");
  $("t").click();
  await tick(100);
  return { server, browser: look("browser") };"#,
        true,
    ) else {
        return;
    };
    let none = "rgba(0, 0, 0, 0)";
    let one = r"color: red\3b  background: blue";
    let want = serde_json::json!([
        [format!("{}; top: 0", one), none],
        [one, none],
        [one, none],
        [one, none],
        [one, none],
        [r#"color: err("red\\3b  background: blue")"#, none],
        ["color: black", none],
    ]);
    assert_eq!(v["server"], want, "{}", v);
    assert_eq!(v["browser"], want, "{}", v);
    assert_eq!(v["remade"], 0, "{}", v);
}

// ---- Q-1259: HTML's void names in an XML answer ---------------------------------------------

/// An Atom feed's `<link href="..."/>` ends as XML has it; an RSS channel's `<link>` holds its
/// text; each of HTML's void names likewise, in every kind of XML root, and in markup a loop
/// makes inside one.
#[test]
fn void_names_in_an_xml_answer_are_xml_elements() {
    let out = run(r#"let url = "http://example.com/a?b=1&c=2"
print(<feed xmlns="http://www.w3.org/2005/Atom"><title>Help</title><link href="http://example.com/help"/></feed>)
print(<rss version="2.0"><channel><title>Help</title><link>http://example.com/</link></channel></rss>)
print(<rss version="2.0"><channel><link>{url}</link><link></link><link/></channel></rss>)
print(<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><url><loc>{url}</loc><img src="a.png"/><br/></url></urlset>)
print(<sitemapindex><sitemap><loc>x</loc><meta name="a">b</meta></sitemap></sitemapindex>)
print(<catalog xmlns="urn:x"><link>one</link><input>two</input><hr/></catalog>)
print(<catalog xmlns:x="urn:x"><link rel="a"/></catalog>)
let entries = [{ t: "é", u: "/1" }, { t: "😀", u: "/2" }]
print(<feed xmlns="http://www.w3.org/2005/Atom">{for e in entries => <entry><title>{e.t}</title><link href={e.u}/></entry>}</feed>)
"#);
    for want in [
        r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Help</title><link href="http://example.com/help"/></feed>"#,
        r#"<rss version="2.0"><channel><title>Help</title><link>http://example.com/</link></channel></rss>"#,
        r#"<rss version="2.0"><channel><link>http://example.com/a?b=1&amp;c=2</link><link/><link/></channel></rss>"#,
        r#"<url><loc>http://example.com/a?b=1&amp;c=2</loc><img src="a.png"/><br/></url></urlset>"#,
        r#"<sitemapindex><sitemap><loc>x</loc><meta name="a">b</meta></sitemap></sitemapindex>"#,
        r#"<catalog xmlns="urn:x"><link>one</link><input>two</input><hr/></catalog>"#,
        r#"<catalog xmlns:x="urn:x"><link rel="a"/></catalog>"#,
        r#"<entry><title>é</title><link href="/1"/></entry><entry><title>😀</title><link href="/2"/></entry></feed>"#,
    ] {
        assert!(out.contains(want), "{}\n{}", want, out);
    }
}

/// A feed served: its type is XML's, and an XML parser reads it.
#[test]
fn a_feed_served_is_read_by_an_xml_parser() {
    let s = served(
        "feed",
        r#"route GET /atom.xml = <feed xmlns="http://www.w3.org/2005/Atom"><title>Help</title><link href="http://example.com/help"/></feed>
route GET /rss.xml = <rss version="2.0"><channel><title>Help</title><link>http://example.com/</link><item><link>http://example.com/1</link></item></channel></rss>
"#,
    );
    for (path, ty) in [("/atom.xml", "application/atom+xml; charset=utf-8"), ("/rss.xml", "application/rss+xml; charset=utf-8")] {
        let a = s.get(path);
        assert_eq!((a.status, a.header("content-type")), (200, Some(ty)), "{}", a.body);
        // Python's ElementTree, where there is one: "mismatched tag" before.
        if let Ok(o) = Command::new("python3").args(["-I", "-c", "import sys, xml.etree.ElementTree as E; E.fromstring(sys.stdin.read()); print('ok')"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().and_then(|mut c| {
            use std::io::Write;
            c.stdin.take().unwrap().write_all(a.body.as_bytes())?;
            c.wait_with_output()
        }) {
            assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "ok", "{}: {}\n{}", path, String::from_utf8_lossy(&o.stderr), a.body);
        }
    }
}

/// HTML pages are written as before, byte for byte: void elements with no end, `<html xmlns>`,
/// an XHTML `<div xmlns>` inside a page, SVG's own rules; and `<link>text</link>` in HTML is still
/// refused, with its fix.
#[test]
fn html_pages_and_svg_are_written_as_before() {
    let src = r#"print(<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="a.css"><meta charset="utf-8"></head><body><img src="a.png"><br><input value="x"/><div xmlns="http://www.w3.org/1999/xhtml"><hr></div><section><p xmlns="urn:x"><wbr></p></section></body></html>)
print(<svg xmlns="http://www.w3.org/2000/svg"><image href="x.png"/></svg>)
print(<div><link rel="a"/><img src="b"></img></div>)
"#;
    let out = run(src);
    let base = std::env::var_os("QUILLO_BASE_BIN");
    for want in [
        r#"<!doctype html><html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="a.css"><meta charset="utf-8"></head><body><img src="a.png"><br><input value="x"><div xmlns="http://www.w3.org/1999/xhtml"><hr></div><section><p xmlns="urn:x"><wbr></p></section></body></html>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="x.png"></image></svg>"#,
        r#"<div><link rel="a"><img src="b"></div>"#,
    ] {
        assert!(out.contains(want), "{}\n{}", want, out);
    }
    // The same bytes as a build before the change, when one is named.
    if let Some(b) = base {
        let d = Dir::new("base", src);
        let o = Command::new(b).arg("run").arg(d.main()).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&o.stdout), out);
    }
    let e = refused("route GET / = <html><body><p><link>http://example.com/</link></p></body></html>\n");
    assert!(e.contains("`<link>` has no end tag: remove `</link>`"), "{}", e);
}

/// Entries a helper or a view makes are XML's too (`<entry>`, `<item>`, `<url>`, `<sitemap>`), and
/// an open `<link ...>` not followed by its text and end is empty, as 0.8.9 had it.
#[test]
fn helpers_make_xml_entries_and_an_open_link_is_empty() {
    let out = run(r#"fn entry(e) = <entry><title>{e.t}</title><link href={e.u}/></entry>
fn item(e) = <item><link>{e.u}</link><guid>{e.u}</guid></item>
view Url(loc) = <url><loc>{loc}</loc></url>
fn map_entry(s) = <sitemap><loc>{s}</loc><link rel="x"/></sitemap>
let es = [{ t: "é", u: "/1" }, { t: "b", u: "/2?x=1&y=2" }]
print(<feed xmlns="http://www.w3.org/2005/Atom">{map(es, entry)}</feed>)
print(<rss version="2.0"><channel>{map(es, item)}</channel></rss>)
print(<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><Url loc="/a"/></urlset>)
print(<sitemapindex>{map_entry("/s.xml")}</sitemapindex>)
print(<feed><link href="x"><entry/></feed>)
print(<feed><link href="y"></link><title>t</title></feed>)
"#);
    for want in [
        r#"<feed xmlns="http://www.w3.org/2005/Atom"><entry><title>é</title><link href="/1"/></entry><entry><title>b</title><link href="/2?x=1&amp;y=2"/></entry></feed>"#,
        r#"<rss version="2.0"><channel><item><link>/1</link><guid>/1</guid></item><item><link>/2?x=1&amp;y=2</link><guid>/2?x=1&amp;y=2</guid></item></channel></rss>"#,
        r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><url><loc>/a</loc></url></urlset>"#,
        r#"<sitemapindex><sitemap><loc>/s.xml</loc><link rel="x"/></sitemap></sitemapindex>"#,
        r#"<feed><link href="x"/><entry></entry></feed>"#,
        r#"<feed><link href="y"/><title>t</title></feed>"#,
    ] {
        assert!(out.contains(want), "{}\n{}", want, out);
    }
}

/// What is in braces is code, whose `<` is no tag: `<link>{1 < 2 ? "a" : "b"}</link>`.
#[test]
fn a_link_holding_code_with_a_less_than_is_read() {
    let out = run("print(<rss><channel><item><link>{1 < 2 ? \"a\" : \"b\"}</link><title>{\"}<\"}</title></item></channel></rss>)\nprint(<feed><link href=\"x\"/><entry><link>{`a}${1 < 2}`}</link></entry></feed>)\n");
    assert!(out.contains("<rss><channel><item><link>a</link><title>}&lt;</title></item></channel></rss>"), "{}", out);
    assert!(out.contains("<feed><link href=\"x\"/><entry><link>a}true</link></entry></feed>"), "{}", out);
}

// ---- Q-1272: an error value in markup shows nothing ------------------------------------------

/// An error shows nothing where it is put, as a child, in a list, as an attribute (left out); a
/// fallback still shows; the log says it once a render, with where, and the component.
#[test]
fn an_error_in_markup_shows_nothing_and_is_said_once() {
    let s = served(
        "errs",
        r#"server fn load_news() -> str ! str = err("503 Service Unavailable")
fn two() = [err("a"), err("b")]
component News() {
    state news = load_news()
    <p id="n">News: {news}</p>
}
route GET / = <html><body><News/></body></html>
route GET /plain = <html><body><p>{err("plain one")}</p><p>{two()}</p><p title={err("t")}>x</p><p>{err("x") ?? "fallback"}</p><p>{[1, err("in a list"), 2]}</p><p>{is_err(err("y")) ? "is one" : ""}</p></body></html>
"#,
    );
    let a = s.get("/plain");
    assert_eq!(a.status, 200);
    assert!(a.body.contains("<body><p></p><p></p><p>x</p><p>fallback</p><p>12</p><p>is one</p></body>"), "{}", a.body);
    assert!(!a.body.contains("err("), "{}", a.body);
    let log = s.log_with("an error no statement took", 1);
    assert_eq!(log.matches("an error no statement took").count(), 1, "{}", log);
    assert!(log.contains("main.qwe:8: an error no statement took, shown as nothing: plain one (give a value for it with `??`, or show something else with `is_err` or `match`)"), "{}", log);
    assert!(log.contains("GET /plain"), "{}", log);
    // Each render says it again.
    s.get("/plain");
    let log = s.log_with("an error no statement took", 2);
    assert_eq!(log.matches("an error no statement took").count(), 2, "{}", log);
    let a = s.get("/");
    assert!(a.body.contains("<p id=\"n\">News: </p>"), "{}", a.body);
    assert!(!a.body.contains(">News: err("), "{}", a.body);
    let log = s.log_with("in <News>", 1);
    assert!(log.contains("main.qwe:5: an error no statement took, made at main.qwe:1, shown in <News> as nothing: 503 Service Unavailable"), "{}", log);
}

/// In the browser: nothing on the page, the same said in the console, the program's error as a
/// state set to one later.
#[test]
fn an_error_in_markup_shows_nothing_in_the_browser() {
    let src = r#"server fn load_news() -> str ! str = err("503 Service Unavailable")
component News() {
    state news = load_news()
    state other = "fine"
    <div>
        <p id="n">News: {news}</p>
        <p id="o" title={other}>{other}</p>
        <button id="b" onclick={() => { other = err("later") }}>b</button>
    </div>
}
route GET / = <html><body><News/></body></html>
"#;
    let Some(v) = in_page(
        "errs-browser",
        src,
        "/",
        r#"const first = [$("n").textContent, $("o").textContent, $("o").getAttribute("title")];
  $("b").click();
  await tick(100);
  return { first, after: [$("o").textContent, $("o").getAttribute("title")], html: document.body.innerHTML.includes("err(") };"#,
        true,
    ) else {
        return;
    };
    assert_eq!(v["first"], serde_json::json!(["News: ", "fine", "fine"]), "{}", v);
    assert_eq!(v["after"], serde_json::json!(["", null]), "{}", v);
    assert_eq!(v["html"], false, "{}", v);
    assert_eq!(v["remade"], 0, "{}", v);
    let errors: Vec<String> = v["errors"].as_array().unwrap().iter().map(|e| e.as_str().unwrap_or("").to_string()).collect();
    assert!(errors.iter().any(|e| e.contains("an error no statement took, shown in <News> as nothing: 503 Service Unavailable")), "{:?}", errors);
    assert!(errors.iter().any(|e| e.contains("shown in <News> as nothing: later")), "{:?}", errors);
    // Once a render: the text and the title of one change say it once.
    assert_eq!(errors.iter().filter(|e| e.contains(": later")).count(), 1, "{:?}", errors);
}

/// Once a request, however many rows a helper makes; in a class list, a style map, a `<style>`,
/// after `??`: nothing, and the log names the line.
#[test]
fn an_error_is_said_once_a_request_and_left_out_everywhere() {
    let src = r#"fn row(x) = <li>{err(`bad ${x}`)}</li>
route GET /rows = <html><body><ul>{map(range(0, 200), row)}</ul></body></html>
route GET /bits = <html><body><p class={["a", err("c")]} style={{ color: err("s"), top: "1px" }}>x</p><style>{err("st")}</style></body></html>
route GET /nn = <html><body><p>{null ?? err("n")}</p></body></html>
"#;
    let s = served("errs-once", src);
    let a = s.get("/rows");
    assert_eq!(a.status, 200);
    assert_eq!(a.body.matches("<li></li>").count(), 200, "{}", a.body);
    s.get("/rows");
    let log = s.log_with("an error no statement took", 2);
    assert_eq!(log.matches("an error no statement took").count(), 2, "{}", log);
    assert!(log.contains("main.qwe:1: an error no statement took, shown as nothing: bad 0"), "{}", log);
    let a = s.get("/bits");
    assert!(a.body.contains("<body><p class=\"a\" style=\"top: 1px\">x</p><style></style></body>"), "{}", a.body);
    assert!(!a.body.contains("err("), "{}", a.body);
    s.get("/nn");
    let log = s.log_with("an error no statement took", 4);
    assert_eq!(log.matches("an error no statement took").count(), 4, "{}", log);
    assert!(log.contains("main.qwe:3: an error no statement took, shown as nothing: c"), "{}", log);
    assert!(log.contains("main.qwe:4: an error no statement took, shown as nothing: n"), "{}", log);
}

/// The browser leaves the same out (a class, a class list's item, a style map's declaration, a
/// list of errors), in what it takes up and in what it makes, and says it with the component.
#[test]
fn the_browser_leaves_errors_out_where_the_server_does() {
    let src = r#"component B() {
    state e = err("nope")
    state on = false
    <div>
        <p id="c" class={e}>c</p>
        <p id="l" class={["a", e]}>l</p>
        <p id="s" style={{ color: e, top: "1px" }}>s</p>
        <p id="n">{null ?? e}</p>
        <ul id="u">{[e, e]}</ul>
        <button id="b" onclick={() => { on = true }}>b</button>
        {on ? <p id="m" class={["x", e]} style={{ color: e, top: "2px" }}>{[e]}</p> : null}
    </div>
}
route GET / = <html><body><B/></body></html>
"#;
    let Some(v) = in_page(
        "errs-attrs",
        src,
        "/",
        r#"const at = (id) => [$(id).getAttribute("class"), $(id).getAttribute("style"), $(id).textContent];
  const taken = ["c", "l", "s", "n", "u"].map(at);
  $("b").click();
  await tick(100);
  return { taken, made: at("m") };"#,
        true,
    ) else {
        return;
    };
    assert_eq!(v["taken"], serde_json::json!([[null, null, "c"], ["a", null, "l"], [null, "top: 1px", "s"], [null, null, ""], [null, null, ""]]), "{}", v);
    assert_eq!(v["made"], serde_json::json!(["x", "top: 2px", ""]), "{}", v);
    assert_eq!(v["remade"], 0, "{}", v);
    let errors: Vec<&str> = v["errors"].as_array().unwrap().iter().filter_map(|e| e.as_str()).collect();
    assert!(!errors.is_empty() && errors.iter().all(|e| e.starts_with("an error no statement took, shown in <B> as nothing: nope")), "{:?}", errors);
}

/// A select given an error chooses nothing, and the log says so, as the browser's console does.
#[test]
fn a_select_given_an_error_is_said() {
    let s = served("errs-select", "route GET / = <html><body><select value={err(\"pick\")}><option>a</option><option>b</option></select></body></html>\n");
    let a = s.get("/");
    assert!(a.body.contains("<select><option>a</option><option>b</option></select>"), "{}", a.body);
    let log = s.log_with("an error no statement took", 1);
    assert!(log.contains("main.qwe:1: an error no statement took, shown as nothing: pick"), "{}", log);
}

// ---- Q-1271: `state` in an `if` ---------------------------------------------------------------

#[test]
fn state_inside_a_block_says_where_it_goes() {
    let cases: &[(&str, &str)] = &[
        ("if show { state extra = \"extra\" }", "`state` goes at the top of a component's body, not inside an `if`: declare `state extra = \"\"` there and set it in the `if`"),
        ("if show { } else { state n = 2 }", "not inside an `if`: declare `state n = 0` there and set it in the `if`"),
        ("if show { if !show { state xs = [1] } }", "not inside an `if`: declare `state xs = []` there and set it in the `if`"),
        ("for i in [1] { state m = { a: i } }", "not inside a `for`: declare `state m = {}` there and set it in the `for`"),
        ("while false { state on = true }", "not inside a `while`: declare `state on = false` there and set it in the `while`"),
        ("let f = () => { state x = 1.5 }", "not inside a function: declare `state x = 0.0` there and set it in the function"),
        ("if show { state when = now() }", "declare `state when = null` there"),
        ("if show { state é = `a${1}` }", "declare `state é = \"\"` there"),
    ];
    for (stmt, want) in cases {
        let e = refused(&format!("component S(show: bool) {{\n    {}\n    <p>hi</p>\n}}\n\nroute GET / = <html><body><S show={{true}}/></body></html>\n", stmt));
        assert!(e.contains(&format!("main.qwe:2: error: {}", want)) || e.contains(want), "{}\n{}", want, e);
        assert!(!e.contains("newer Quillo"), "{}", e);
    }
}

/// `state` as a name of the program's, in a component's block, is a name as before; and a state
/// at the top of the body, set in an `if`, as the message says to write it.
#[test]
fn state_as_a_name_and_at_the_top_still_work() {
    let out = run(r#"component S(show: bool) {
    state extra = ""
    let state = { label: "l" }
    if show { extra = "extra" }
    let shown = show ? state.label : ""
    <p>{extra} {shown}</p>
}
print(<S show={true}/>)
"#);
    assert!(out.contains("extra l"), "{}", out);
    // A `state` word not followed by a name and `=` is told as before.
    let e = refused("component S(show: bool) {\n    if show { state extra }\n    <p>hi</p>\n}\nroute GET / = <html><body><S show={true}/></body></html>\n");
    assert!(e.contains("expected end of statement, found `extra`"), "{}", e);
}

// ---- Q-1270 and Q-1025: lower case `onchange` is the DOM's `change` --------------------------

#[test]
fn only_reacts_names_are_renamed() {
    let twice = |a: &str, b: &str, el: &str| quillo_on("check", &format!("fn f(e) = 1\nprint(<{} {}={{f}} {}={{f}}/>)\n", el, a, b));
    for (a, b, el, said) in [
        ("onchange", "@change", "input", "(`onchange={...}` is `@change`)"),
        ("onchange", "@change", "textarea", "(`onchange={...}` is `@change`)"),
        ("onChange", "@input", "input", "(`onChange={...}` is `@input`)"),
        ("onfocus", "@focus", "div", "(`onfocus={...}` is `@focus`)"),
        ("onblur", "@blur", "div", "(`onblur={...}` is `@blur`)"),
        ("onFocus", "@focusin", "div", "(`onFocus={...}` is `@focusin`)"),
        ("onBlur", "@focusout", "div", "(`onBlur={...}` is `@focusout`)"),
        ("ondoubleclick", "@dblclick", "div", "(`ondoubleclick={...}` is `@dblclick`)"),
    ] {
        let (ok, e) = twice(a, b, el);
        assert!(!ok && e.contains(said), "{} {}: {}", a, b, e);
    }
    // Lower case `onchange` with `@input`, and `onfocus` with `@focusin`, are two events.
    for (a, b, el) in [("onchange", "@input", "input"), ("onfocus", "@focusin", "div"), ("onblur", "@focusout", "div"), ("ONCHANGE", "@input", "input")] {
        let (ok, e) = twice(a, b, el);
        assert!(ok, "{} {}: {}", a, b, e);
    }
    // A `…Capture` names React's form and its event.
    let (_, e) = quillo_on("check", "fn f(e) = 1\nprint(<input onchangecapture={f}/>)\n");
    assert!(e.contains("write `onChange` (or `@input`)"), "{}", e);
    let (_, e) = quillo_on("check", "fn f(e) = 1\nprint(<select onChangeCapture={f}></select>)\n");
    assert!(e.contains("write `onChange` (or `@change`)"), "{}", e);
}

/// Typed into and left, in Chrome: lower case `onchange` once, when the field is left, as `@change`;
/// React's `onChange` and `oninput` at each key; in a spread too; `onfocus` on a wrapper hears no
/// focus of a field in it, React's `onFocus` does.
#[test]
fn lower_case_onchange_waits_for_the_field_to_be_left() {
    let src = r#"component Box() {
    state a = 0
    state b = 0
    state c = 0
    state d = 0
    state e = 0
    state g = 0
    state h = 0
    state i = 0
    <section>
        <input id="a" onchange={() => { a += 1 }}/>
        <input id="b" onChange={() => { b += 1 }}/>
        <input id="c" oninput={() => { c += 1 }}/>
        <input id="d" @change={() => { d += 1 }}/>
        <input id="e" {...{ onchange: () => { e += 1 } }}/>
        <textarea id="t" onchange={() => { i += 1 }}></textarea>
        <div onfocus={() => { g += 1 }}><input id="f1"/></div>
        <div onFocus={() => { h += 1 }}><input id="f2"/></div>
        <p id="out">{a} {b} {c} {d} {e} {g} {h} {i}</p>
    </section>
}
route GET / = <html><body><Box/></body></html>
"#;
    let Some(v) = in_page(
        "onchange",
        src,
        "/",
        r#"for (const id of ["a", "b", "c", "d", "e", "t"]) await type_in(id, "x", "xy", "xyz");
  await tick(100);
  const typed = $("out").textContent;
  for (const id of ["a", "b", "c", "d", "e", "t"]) $(id).dispatchEvent(new Event("change", { bubbles: true }));
  for (const id of ["f1", "f2"]) { $(id).dispatchEvent(new FocusEvent("focus")); $(id).dispatchEvent(new FocusEvent("focusin", { bubbles: true })); }
  await tick(100);
  return { typed, left: $("out").textContent };"#,
        true,
    ) else {
        return;
    };
    assert_eq!(v["typed"], "0 3 3 0 0 0 0 0", "{}", v);
    assert_eq!(v["left"], "1 3 3 1 1 0 1 1", "{}", v);
    assert_eq!(v["errors"], serde_json::json!([]), "{}", v);
}

// ---- Q-1269: a form's reset and the states bound to its fields -------------------------------

const RESET: &str = r#"component Reset() {
    state name = ""
    state on = false
    state pick = "b"
    state note = "n"
    state own = "o"
    state both = ""
    state hits = 0
    <form id="f">
        <input id="name" value={name}/>
        <input id="on" type="checkbox" checked={on}/>
        <select id="pick" value={pick}><option value="a">a</option><option value="b">b</option></select>
        <textarea id="note" value={note}></textarea>
        <input id="own" value={own} oninput={(e) => { own = upper(e.target.value) }}/>
        <input id="both" bind:value={both} oninput={() => { hits += 1 }}/>
        <button id="clear" type="reset">Clear</button>
        <p id="out">Hello, {name}|{on ? "on" : "off"}|{pick}|{note}|{own}|{both}|{hits}</p>
    </form>
}
component Kept() {
    state k = ""
    <form id="h" onreset={(e) => { e.preventDefault() }}><input id="k" value={k}/><button id="clear3" type="reset">Clear</button><p id="ko">{k}</p></form>
}
component Plain() {
    state n = 0
    <form id="g"><input id="x"/><button id="clear2" type="reset" onclick={() => { n += 1 }}>Clear</button><p id="n">{n}</p></form>
}
// Counts the page's `reset` listeners (the runtime's is on `document`).
let COUNT = "window.__resets = 0; const add = document.addEventListener; document.addEventListener = function (t, ...r) { if (t === 'reset') window.__resets++; return add.call(this, t, ...r) };"
route GET / = <html><head><script>{raw(COUNT)}</script></head><body><Reset/><Kept/></body></html>
route GET /plain = <html><head><script>{raw(COUNT)}</script></head><body><Plain/></body></html>
"#;

/// Typed, ticked, picked, then Clear: each bound state is its field's first value again, and the
/// page says so; a field with a handler of its own (one way) keeps its state, a handler beside a
/// `bind:value` is not run; `form.reset()` from a
/// script likewise; one stopped by the program changes nothing. One listener for the page.
#[test]
fn a_forms_reset_sets_the_states_bound_to_its_fields() {
    let Some(v) = in_page(
        "reset",
        RESET,
        "/",
        r#"const out = () => $("out").textContent;
  const fill = async () => {
    await type_in("name", "A", "An", "Ana");
    $("on").click();
    $("pick").value = "a"; for (const t of ["input", "change"]) $("pick").dispatchEvent(new Event(t, { bubbles: true }));
    await type_in("note", "zz");
    await type_in("own", "xy");
    await type_in("both", "q");
    await tick(50);
  };
  await fill();
  const typed = out();
  $("clear").click();
  await tick(100);
  // What the fields show, as the page should say it.
  const fields = () => `Hello, ${$("name").value}|${$("on").checked ? "on" : "off"}|${$("pick").value}|${$("note").value}|XY|${$("both").value}|`;
  const cleared = [out(), fields(), $("name").value];
  await fill();
  $("f").reset();
  await tick(100);
  const scripted = [out(), fields()];
  await type_in("k", "abc");
  $("clear3").click();
  await tick(100);
  return { typed, cleared, scripted, kept: [$("k").value, $("ko").textContent], resets: window.__resets };"#,
        true,
    ) else {
        return;
    };
    assert_eq!(v["typed"], "Hello, Ana|on|a|zz|XY|q|1", "{}", v);
    // The page says what each field shows (the text field empty, as in the report; a box keeps what
    // the runtime last wrote as its attribute, so the form gives it back); a binding's own handler
    // (`oninput` beside `bind:value`) is not run by the reset.
    let said = |w: &serde_json::Value, hits: &str| {
        let (page, fields) = (w[0].as_str().unwrap().to_string(), w[1].as_str().unwrap().to_string());
        assert_eq!(page, fields.clone() + hits, "{}", v);
        assert!(page.starts_with("Hello, |") && page.contains("|n|XY||"), "{}", v);
    };
    said(&v["cleared"], "1");
    assert_eq!(v["cleared"][2], "", "{}", v);
    said(&v["scripted"], "2");
    // A reset the program stopped (`preventDefault`) leaves field and state as they were.
    assert_eq!(v["kept"], serde_json::json!(["abc", "abc"]), "{}", v);
    assert_eq!(v["resets"], 1, "{}", v);
    assert_eq!(v["errors"], serde_json::json!([]), "{}", v);
}

/// A page whose forms bind no field gets no listener, and its reset does what it did.
#[test]
fn a_form_with_no_bound_field_gets_no_listener() {
    let Some(v) = in_page(
        "reset-plain",
        RESET,
        "/plain",
        r#"await type_in("x", "abc");
  $("clear2").click();
  await tick(100);
  return { x: $("x").value, n: $("n").textContent, resets: window.__resets };"#,
        true,
    ) else {
        return;
    };
    assert_eq!(v, serde_json::json!({ "x": "", "n": "1", "resets": 0, "remade": 0, "errors": [] }), "{}", v);
}

// ---- Q-1010: a scroll alone takes up a component that listens to scrolls -----------------------

const SCROLLER: &str = r#"component Scroller() {
    state at = 0
    state n = 0
    <section>
        <div id="box" style="height: 100px; overflow-y: scroll" onscroll={(e) => { at = floor(e.target.scrollTop)
            n += 1 }}>
            <div style="height: 5000px">tall</div>
        </div>
        <p id="at">scrolled to {at}</p><p id="n">{n}</p>
    </section>
}
component Other() {
    state k = 0
    <div id="other" @scroll={() => { k += 1 }} style="height: 50px; overflow-y: scroll"><div style="height: 500px">{k}</div></div>
}
component Clock() {
    state t = 0
    <p onmount={() => { t = 1 }}>{t}</p>
}
route GET / = <html><body><main><h1>A list</h1><Scroller/></main></body></html>
route GET /booted = <html><body><main><h1>A list</h1><Scroller/></main><Clock/></body></html>
route GET /none = <html><body><main><h1>None</h1><Clock/></main></body></html>
route GET /at = <html><body><main><Other/></main></body></html>
"#;

/// Moved by a script, with no pointer, touch, focus or key: the bundle comes on the scroll, the
/// component is taken up, and its handler runs once, on where the box is then (two scrolls before
/// the bundle came are one run).
#[test]
fn a_scroll_alone_takes_up_the_component() {
    let Some(v) = in_page(
        "scroll",
        SCROLLER,
        "/",
        r#"await tick(800);
  const ours = () => performance.getEntriesByType("resource").filter((r) => r.name.includes("/_quillo/")).length;
  const before = ours();
  const box = $("box");
  box.scrollTop = 500;
  await tick(0);
  box.scrollTop = 1200;
  for (let i = 0; i < 100 && $("at").textContent !== "scrolled to 1200"; i++) await tick(30);
  await tick(300);
  return { before, after: ours() > 0, at: $("at").textContent, n: $("n").textContent, top: box.scrollTop };"#,
        false,
    ) else {
        return;
    };
    assert_eq!(v["before"], 0, "{}", v);
    assert_eq!(v["after"], true, "{}", v);
    assert_eq!(v["at"], "scrolled to 1200", "{}", v);
    assert_eq!(v["n"], "1", "{}", v);
    assert_eq!(v["remade"], 0, "{}", v);
}

/// The bundle in already (another component brought it), the component not taken up yet: a scroll
/// takes it up and its handler hears that scroll; `@scroll` as `onscroll`.
#[test]
fn a_scroll_after_the_bundle_came_takes_up_the_component() {
    let Some(v) = in_page(
        "scroll-booted",
        SCROLLER,
        "/booted",
        r#"const box = $("box");
  const taken = !!box.closest("quillo-island").__q;
  box.scrollTop = 700;
  for (let i = 0; i < 100 && $("at").textContent !== "scrolled to 700"; i++) await tick(30);
  return { taken, at: $("at").textContent, n: $("n").textContent };"#,
        true,
    ) else {
        return;
    };
    assert_eq!(v["taken"], false, "{}", v);
    assert_eq!(v["at"], "scrolled to 700", "{}", v);
    assert_eq!(v["n"], "1", "{}", v);
    let Some(v) = in_page(
        "scroll-at",
        SCROLLER,
        "/at",
        r#"await tick(500);
  $("other").scrollTop = 200;
  for (let i = 0; i < 100 && $("other").textContent !== "1"; i++) await tick(30);
  return { k: $("other").textContent };"#,
        false,
    ) else {
        return;
    };
    assert_eq!(v["k"], "1", "{}", v);
}

/// Only a page with such a component carries the scroll part of the loader, and only that island
/// says it listens to scrolls.
#[test]
fn only_a_page_with_a_scroll_handler_listens_to_scrolls() {
    let s = served("scroll-html", SCROLLER);
    let a = s.get("/");
    assert!(a.body.contains("data-sc"), "{}", a.body);
    assert!(a.body.contains("w.sc=1;d.addEventListener(\"scroll\""), "{}", a.body);
    let a = s.get("/none");
    assert!(a.body.contains("<quillo-island"), "{}", a.body);
    assert!(!a.body.contains("data-sc") && !a.body.contains("\"scroll\"") && !a.body.contains("w.sc"), "{}", a.body);
    // A request after one with a scroll island is as the page it is.
    let a = s.get("/");
    assert!(a.body.contains("w.sc=1"), "{}", a.body);
    let a = s.get("/none");
    assert!(!a.body.contains("w.sc"), "{}", a.body);
}
