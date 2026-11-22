//! FEEDBACK79.md, Q-1250, the user's decision: a css`...` written in the program is trusted CSS,
//! which a style's escape lets through wherever it is given (a name, a prop, a spread, a list, a
//! map, a fn's answer, an island's props, a `live` answer, a server fn's answer to the page), and
//! which is the `str` it is everywhere else; what anything makes of it is plain text, and nothing
//! read from outside (a request, a page, a store, a script, an MCP client) is ever trusted. Both
//! renderers agree. Branch feedback72-81.

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
        let d = support::temp_dir(&format!("f7281css-{}-{}", name, N.fetch_add(1, Ordering::SeqCst)));
        std::fs::write(d.join("main.qwe"), src).unwrap();
        Dir(d)
    }

    fn main(&self) -> PathBuf {
        self.0.join("main.qwe")
    }
}

/// The quillo under test: this build's, or the one QUILLO_TEST_BIN names.
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

/// `quillo <cmd> main.qwe` in `d`, under a deadline: whether it succeeded, and what it printed.
fn quillo_in(d: &Path, cmd: &str) -> (bool, String) {
    let mut c = Command::new(quillo()).arg(cmd).arg(d.join("main.qwe")).current_dir(d).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
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
    let status = wait_for(&mut c, Duration::from_secs(120)).unwrap_or_else(|| panic!("quillo {} took too long", cmd));
    (status.success(), t1.join().unwrap() + &t2.join().unwrap())
}

/// `src` passes `quillo check` (warnings allowed), then runs: what it printed.
fn run(src: &str) -> String {
    let d = Dir::new("run", src);
    let (ok, out) = quillo_in(&d.0, "check");
    assert!(ok, "quillo check:\n{}\n{}", out, src);
    let (ok, out) = quillo_in(&d.0, "run");
    assert!(ok, "quillo run:\n{}\n{}", out, src);
    out
}

/// A server for `src` (checked first), with `env` and the files `files` in its folder.
struct Served {
    dir: Dir,
    log: PathBuf,
    port: u16,
    _guard: support::Guard,
}

fn served(name: &str, src: &str, env: &[(&str, &str)], files: &[(&str, &str)]) -> Served {
    let dir = Dir::new(name, src);
    let (ok, out) = quillo_in(&dir.0, "check");
    assert!(ok, "quillo check:\n{}\n{}", out, src);
    for (f, text) in files {
        std::fs::write(dir.0.join(f), text).unwrap();
    }
    let log = dir.0.join("log.txt");
    let mut cmd = Command::new(quillo());
    cmd.arg("run").arg(dir.main()).args(["--port", "0"]).current_dir(&dir.0).stdin(Stdio::null()).stdout(Stdio::null()).stderr(std::fs::File::create(&log).unwrap());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    let port = support::port_in_log(&mut child, &log, Duration::from_secs(30));
    Served { dir, log, port, _guard: support::Guard::new(child) }
}

impl Served {
    fn get(&self, path: &str, headers: &[(&str, &str)]) -> support::Answer {
        support::send(self.port, "GET", path, headers, "")
    }

    fn post(&self, path: &str, headers: &[(&str, &str)], body: &str) -> support::Answer {
        let mut hs = vec![("Sec-Fetch-Site", "same-origin")];
        hs.extend_from_slice(headers);
        support::send(self.port, "POST", path, &hs, body)
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

/// What a visitor sends, and what a style then says of it: one value.
const ONE: &str = r"red\3b  background: blue";

// ---- trusted CSS is the str it is ----------------------------------------------------------

/// Anywhere but a style, css`...` is its text: its type, length, case, comparisons (`==` with the
/// same text is true, as with any `str`), `+`, JSON, a list's text, an index, a loop, a map's key,
/// a pattern, a `str` parameter, markup's text and an attribute's (HTML-escaped), `unique`; an
/// empty one and unicode too.
#[test]
fn trusted_css_is_its_text_everywhere_else() {
    let out = run(r#"let lit = css`color: green; font-size: 20px`
fn f(s: str) -> str = s
print(type_of(lit))
print(len(css`ab`), len(css``), len(css`é😀`))
print(upper(css`é😀 ab`))
print(lit == "color: green; font-size: 20px", css`a` == css`a`, css`a` != "b", css`a` == 1)
print(css`a` < "b", css`b` < css`a`)
print(css`a` + "b", "b" + css`a`, css`a` + css`b`, css`a` + 1)
print([lit, 1])
print(json.stringify({ s: lit }))
print(css`xy`[0], css`xy`[-1])
for ch in css`ab` { print(ch) }
print({ k: 1 }[css`k`])
match css`k` { "k" => print("matched") _ => print("no") }
print(f(lit))
print(len(unique([css`a`, "a", css`a`])))
print(str(lit), `${lit}`)
print(<p title={css`a "b" <c>`}>{css`x <y> & z`}</p>)
print(css`${css`a: 1; b: 2`}; c: 3`)
"#);
    for want in [
        "str\n",
        "2 0 2\n",
        "É😀 AB\n",
        "true true true false\n",
        "true false\n",
        "ab ba ab a1\n",
        "[\"color: green; font-size: 20px\", 1]\n",
        "{\"s\":\"color: green; font-size: 20px\"}\n",
        "x y\n",
        "a\nb\n",
        "1\n",
        "matched\ncolor: green; font-size: 20px\n1\n",
        "color: green; font-size: 20px color: green; font-size: 20px\n",
        "<p title=\"a &quot;b&quot; &lt;c&gt;\">x &lt;y&gt; &amp; z</p>\n",
        // A trusted value inside another css`...` passes as it is.
        "a: 1; b: 2; c: 3\n",
    ] {
        assert!(out.contains(want), "{:?}\n{}", want, out);
    }
}

// ---- where it passes, and what makes it plain ----------------------------------------------

/// Given to a view's style through a name, a top-level `let`, a fn's answer, a spread, a list, a
/// map, `first`, `push`, `set`, another css`...` and a choice, a css`...` keeps its declarations,
/// and the visitor's part of one is one value; a style map takes it as it is.
#[test]
fn trusted_css_passes_through_names_props_lists_maps_and_fns() {
    let out = run(r#"let v = trim(" red; background: blue ")
let c = css`color: ${v}; font-size: 20px`
let lit = css`color: green; font-size: 20px`
view Box(style, label) = <div style={`padding: 8px; ${style}`}>{label}</div>
fn look(x) = css`color: ${x}; font-size: 20px`
let base = { style: lit, label: "spread" }
let held = [lit]
let kept = { s: lit }
let big = true
print(<Box style={lit} label="literal"/>)
print(<Box style={c} label="visitor"/>)
print(<Box style={look(v)} label="fn"/>)
print(<Box {...base}/>)
print(<Box style={held[0]} label="list"/>)
print(<Box style={kept.s} label="map"/>)
print(<Box style={first(held)} label="first"/>)
print(<Box style={push([], lit)[0]} label="push"/>)
print(<Box style={set({}, "k", lit).k} label="set"/>)
print(<Box style={css`margin: 0; ${lit}`} label="nested"/>)
print(<Box style={big ? lit : css`top: 0`} label="choice"/>)
print(<p style={`margin: 0; ${big ? css`a: 1; b: 2` : ""}`}>inchoice</p>)
print(<Box style={v} label="plain"/>)
print(<p style={{ color: css`red; font-weight: bold` }}>stylemap</p>)
print(<p style={{ color: v }}>stylemapplain</p>)
print(<Box style={css``} label="empty"/>)
print(<Box style={css`content: "é😀"; top: 0`} label="unicode"/>)
print(<Box style={css(`color: ${v}; top: 0`)} label="callinstyle"/>)
print(<Box style={`color: ${v}; font-size: 20px`} label="tplprop"/>)
print(<Box style="margin: 0; top: 0" label="litprop"/>)
print(<Box style={"margin: 0; " + v} label="plusprop"/>)
"#);
    for (label, style) in [
        ("literal", "padding: 8px; color: green; font-size: 20px".to_string()),
        ("visitor", format!("padding: 8px; color: {}; font-size: 20px", ONE)),
        ("fn", format!("padding: 8px; color: {}; font-size: 20px", ONE)),
        ("spread", "padding: 8px; color: green; font-size: 20px".into()),
        ("list", "padding: 8px; color: green; font-size: 20px".into()),
        ("map", "padding: 8px; color: green; font-size: 20px".into()),
        ("first", "padding: 8px; color: green; font-size: 20px".into()),
        ("push", "padding: 8px; color: green; font-size: 20px".into()),
        ("set", "padding: 8px; color: green; font-size: 20px".into()),
        ("nested", "padding: 8px; margin: 0; color: green; font-size: 20px".into()),
        ("choice", "padding: 8px; color: green; font-size: 20px".into()),
        ("plain", format!("padding: 8px; {}", ONE)),
        ("empty", "padding: 8px; ".into()),
        ("unicode", "padding: 8px; content: &quot;é😀&quot;; top: 0".into()),
        // An explicit call in a style, whose template's values the walk escaped: css`...`'s form.
        ("callinstyle", format!("padding: 8px; color: {}; top: 0", ONE)),
        // A template or a text written in a view's `style` prop is trusted CSS (the user's
        // decision): its values escaped where it is written, its declarations kept.
        ("tplprop", format!("padding: 8px; color: {}; font-size: 20px", ONE)),
        ("litprop", "padding: 8px; margin: 0; top: 0".into()),
        // Anything else given whole stays text, a visitor's part with it.
        ("plusprop", format!(r"padding: 8px; margin: 0\3b  {}", ONE)),
    ] {
        let want = format!("<div style=\"{}\">{}</div>", style, label);
        assert!(out.contains(&want), "{}\n{}", want, out);
    }
    assert!(out.contains("<p style=\"margin: 0; a: 1; b: 2\">inchoice</p>"), "{}", out);
    assert!(out.contains("<p style=\"color: red; font-weight: bold\">stylemap</p>"), "{}", out);
    assert!(out.contains(&format!("<p style=\"color: {}\">stylemapplain</p>", ONE)), "{}", out);
}

/// `+`, a builtin (`upper`, `trim`, `str`, `join`, `replace`, a pipe), a template around it made
/// apart and an explicit `css(...)` of text make plain text, which a style escapes as one value.
#[test]
fn what_anything_makes_of_trusted_css_is_plain_text() {
    let out = run(r#"let v = trim(" red; background: blue ")
let lit = css`color: green; font-size: 20px`
view Box(style, label) = <div style={`padding: 8px; ${style}`}>{label}</div>
print(<Box style={lit + ""} label="plus"/>)
print(<Box style={"" + lit} label="plus2"/>)
print(<Box style={upper(lit)} label="upper"/>)
print(<Box style={trim(lit)} label="trim"/>)
print(<Box style={str(lit)} label="str"/>)
let around = `${lit}`
print(<Box style={around} label="template"/>)
print(<Box style={join([lit], "")} label="join"/>)
print(<Box style={lit.replace("green", "blue")} label="replace"/>)
print(<Box style={lit | trim} label="pipe"/>)
print(<Box style={css(v + "; font-size: 20px")} label="call"/>)
let made = css
let m = made(`color: ${v}; top: 0`)
print(<Box style={m} label="renamed"/>)
"#);
    let plain = r"padding: 8px; color: green\3b  font-size: 20px";
    for (label, style) in [
        ("plus", plain.to_string()),
        ("plus2", plain.into()),
        ("upper", r"padding: 8px; COLOR: GREEN\3b  FONT-SIZE: 20PX".into()),
        ("trim", plain.into()),
        ("str", plain.into()),
        ("template", plain.into()),
        ("join", plain.into()),
        ("replace", r"padding: 8px; color: blue\3b  font-size: 20px".into()),
        ("pipe", plain.into()),
        ("call", format!(r"padding: 8px; {}\3b  font-size: 20px", ONE)),
        ("renamed", format!(r"padding: 8px; color: {}\3b  top: 0", ONE)),
    ] {
        let want = format!("<div style=\"{}\">{}</div>", style, label);
        assert!(out.contains(&want), "{}\n{}", want, out);
    }
}

// ---- nothing from outside is trusted -------------------------------------------------------

const OUTSIDE: &str = r#"view Box(style) = <div style={`padding: 8px; ${style}`}>x</div>
route GET /q ? { s: str } = <Box style={s}/>
route POST /f { form: { s: str } } = <Box style={s}/>
route POST /j { json: { s: any } } = <Box style={s}/>
route GET /h = <Box style={request.headers["x-s"]}/>
route GET /c = <Box style={request.cookies.s}/>
route GET /p/:s = <Box style={s}/>
route GET /e = <Box style={env("CSS_PROBE")}/>
route GET /file = <Box style={read_file("s.txt")}/>
server fn probe(s) -> str {
    return `${type_of(s)} ${str(<p style={`color: ${s}`}>a</p>)}`
}
server fn back() = { s: css`a: 1; b: 2`, l: [css`c: 3`], p: "e; f" }
"#;

/// A query, a form, a JSON body (its `{"$css": ...}` too), a header, a cookie, a path's part, the
/// environment and a file are text: one value in a style.
#[test]
fn nothing_a_request_brings_is_trusted() {
    let s = served("outside", OUTSIDE, &[("CSS_PROBE", "a: 1; b: 2")], &[("s.txt", "a: 1; b: 2")]);
    let enc = "red%3B%20background%3A%20blue";
    let one = format!("<div style=\"padding: 8px; {}\">x</div>", ONE);
    let cases = [
        ("query", s.get(&format!("/q?s={}", enc), &[])),
        ("form", s.post("/f", &[("content-type", "application/x-www-form-urlencoded")], &format!("s={}", enc))),
        ("json", s.post("/j", &[("content-type", "application/json")], r#"{"s": "red; background: blue"}"#)),
        ("header", s.get("/h", &[("x-s", "red; background: blue")])),
        ("cookie", s.get("/c", &[("cookie", &format!("s={}", enc))])),
        ("path", s.get(&format!("/p/{}", enc), &[])),
    ];
    for (what, a) in cases {
        assert_eq!(a.status, 200, "{}: {}\n{}", what, a.body, s.log());
        assert_eq!(a.body, one, "{}", what);
    }
    // The mark the server writes for trusted CSS, sent in a JSON body: a map, whose text is
    // escaped as any value's.
    let a = s.post("/j", &[("content-type", "application/json")], r#"{"s": {"$css": "red; background: blue"}}"#);
    assert_eq!(a.body, format!("<div style=\"padding: 8px; \\7b  &quot;$css&quot;: &quot;{}&quot; \\7d \">x</div>", ONE), "{}", s.log());
    for path in ["/e", "/file"] {
        assert_eq!(s.get(path, &[]).body, r#"<div style="padding: 8px; a: 1\3b  b: 2">x</div>"#, "{}", path);
    }
}

/// A server fn's arguments from a page: the trusted mark is read as plain text, the `{"$": ...}`
/// form of a map holding it as that map; and what the server answers the page carries its own
/// trusted CSS marked, its plain text as text.
#[test]
fn a_page_cannot_send_trusted_css() {
    let s = served("rpc", OUTSIDE, &[], &[]);
    let rpc = |f: &str, body: &str| s.post(&format!("/_quillo/rpc/{}", f), &[("content-type", "application/json"), ("x-quillo-rpc", "1")], body);
    let a = rpc("probe", r#"[{"$css": "red; background: blue"}]"#);
    assert_eq!(a.body, format!(r#"{{"ok":"str <p style=\"color: {}\">a</p>"}}"#, ONE.replace('\\', "\\\\")), "{}", s.log());
    let a = rpc("probe", r#"[{"$": ["$css", "red; background: blue"]}]"#);
    assert!(a.body.starts_with(r#"{"ok":"map "#) && a.body.contains(r"red\\3b  background: blue"), "{}", a.body);
    let a = rpc("probe", r#"["red; background: blue"]"#);
    assert_eq!(a.body, format!(r#"{{"ok":"str <p style=\"color: {}\">a</p>"}}"#, ONE.replace('\\', "\\\\")));
    // A multipart call, as a page sends one with files.
    let body = "--BB\r\nContent-Disposition: form-data; name=\"args\"\r\n\r\n[{\"$css\": \"red; background: blue\"}]\r\n--BB--\r\n";
    let a = s.post("/_quillo/rpc/probe", &[("content-type", "multipart/form-data; boundary=BB"), ("x-quillo-rpc", "1")], body);
    assert_eq!(a.body, format!(r#"{{"ok":"str <p style=\"color: {}\">a</p>"}}"#, ONE.replace('\\', "\\\\")), "{}", s.log());
    let a = rpc("back", "[]");
    assert_eq!(a.body, r#"{"ok":{"s":{"$css":"a: 1; b: 2"},"l":[{"$css":"c: 3"}],"p":"e; f"}}"#);
}

/// The database and the session keep trusted CSS as text, read back as text, in the request that
/// stored it too.
#[test]
fn stores_keep_trusted_css_as_text() {
    let src = r#"db.open("t.db")
db.exec("create table if not exists t(s text)")
view Box(style) = <div style={`padding: 8px; ${style}`}>x</div>
route GET /db with db = {
    db.exec("delete from t")
    db.exec("insert into t(s) values (?)", css`a: 1; b: 2`)
    db.exec("insert into t(s) values (:s)", { s: css`c: 3; d: 4` })
    db.exec("insert into t(s) values (?)", [css`e: 5; f: 6`][0])
    let rows = db.query("select s from t order by rowid")
    <div>{for r in rows => <Box style={r.s}/>}</div>
}
route writes GET /set = {
    set_session({ s: css`a: 1; b: 2`, n: [css`c: 3; d: 4`] })
    <div><Box style={session().s}/><Box style={session().n[0]}/></div>
}
route GET /get = <div><Box style={session().s}/><Box style={session().n[0]}/></div>
"#;
    let s = served("stores", src, &[], &[]);
    let boxes = |xs: &[&str]| format!("<div>{}</div>", xs.iter().map(|x| format!("<div style=\"padding: 8px; {}\">x</div>", x)).collect::<String>());
    let a = s.get("/db", &[]);
    assert_eq!(a.body, boxes(&[r"a: 1\3b  b: 2", r"c: 3\3b  d: 4", r"e: 5\3b  f: 6"]), "{}", s.log());
    let a = s.get("/set", &[]);
    assert_eq!(a.body, boxes(&[r"a: 1\3b  b: 2", r"c: 3\3b  d: 4"]), "{}", s.log());
    let cookie = a.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie")).map(|(_, v)| v.split(';').next().unwrap_or("").to_string()).collect::<Vec<_>>().join("; ");
    assert!(!cookie.is_empty(), "{:?}", a.headers);
    let a = s.get("/get", &[("cookie", &cookie)]);
    assert_eq!(a.body, boxes(&[r"a: 1\3b  b: 2", r"c: 3\3b  d: 4"]), "{}", s.log());
}

/// A script has no `css`, and trusted CSS it is given, or that a helper of the program's hands
/// it, comes back as text.
#[test]
fn a_script_neither_makes_nor_gives_back_trusted_css() {
    let out = run(r#"view Box(style, label) = <div style={`padding: 8px; ${style}`}>{label}</div>
let lit = css`a: 1; b: 2`
print(is_err(run_script("css`color: red; top: 0`", {})))
print(<Box style={run_script("s", { s: lit })} label="given"/>)
print(<Box style={run_script("[s][0]", { s: lit })} label="list"/>)
print(<Box style={run_script("{ k: s }", { s: lit }).k} label="map"/>)
print(<Box style={run_script("f()", { f: () => lit })} label="helper"/>)
"#);
    assert!(out.starts_with("true\n"), "{}", out);
    for label in ["given", "list", "map", "helper"] {
        let want = format!(r#"<div style="padding: 8px; a: 1\3b  b: 2">{}</div>"#, label);
        assert!(out.contains(&want), "{}\n{}", want, out);
    }
}

/// An MCP tool's arguments are a client's: the trusted mark is a map, and text is text.
#[test]
fn an_mcp_tools_arguments_are_never_trusted() {
    let d = Dir::new(
        "mcp",
        r#"/// What a style makes of it.
server fn mcp probe(s: any) -> str {
    return `${type_of(s)} ${str(<p style={`color: ${s}`}>a</p>)}`
}
"#,
    );
    let (ok, out) = quillo_in(&d.0, "check");
    assert!(ok, "{}", out);
    let meta = serde_json::json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {} });
    let line = |id: i64, s: serde_json::Value| serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": "probe", "arguments": { "s": s }, "_meta": meta } }).to_string();
    let input = d.0.join("in.jsonl");
    std::fs::write(&input, format!("{}\n{}\n", line(1, serde_json::json!({ "$css": "red; background: blue" })), line(2, serde_json::json!("red; background: blue")))).unwrap();
    let child = Command::new(quillo()).arg("mcp").arg(d.main()).current_dir(&d.0).stdin(std::fs::File::open(&input).unwrap()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut g = support::Guard::new(child);
    assert!(wait_for(&mut g.0, Duration::from_secs(60)).is_some(), "quillo mcp did not end");
    let mut text = String::new();
    g.0.stdout.take().unwrap().read_to_string(&mut text).unwrap();
    // A tool's answer is `{"result": ...}` as text.
    let said: Vec<String> = text
        .lines()
        .map(|l| {
            let t = serde_json::from_str::<serde_json::Value>(l).unwrap()["result"]["content"][0]["text"].as_str().unwrap_or("").to_string();
            serde_json::from_str::<serde_json::Value>(&t).ok().and_then(|v| v["result"].as_str().map(str::to_string)).unwrap_or(t)
        })
        .collect();
    assert_eq!(said.len(), 2, "{}", text);
    assert!(said[0].starts_with("map ") && said[0].contains(ONE), "{}", said[0]);
    assert_eq!(said[1], format!("str <p style=\"color: {}\">a</p>", ONE));
}

/// What reads values from outside never makes trusted CSS of the mark: a page's call
/// (`island_marks`), a store's or a service's island JSON (`from_island_json`), plain JSON (a
/// channel's message, an MCP client's arguments); only the server's own kept `live` answer reads
/// it back as trusted CSS. And `Value` is no larger for it.
#[test]
fn the_mark_from_outside_is_read_as_text() {
    use quillo::value::Value;
    let j = serde_json::json!({ "a": { "$css": "x; y" }, "b": [{ "$css": "z" }] });
    let kinds = |v: &Value| match v {
        Value::Map(m) => (m.get("a").map(|x| matches!(x, Value::Css(_))), m.get("b").map(|x| matches!(x, Value::List(l) if matches!(l[0], Value::Css(_))))),
        _ => (None, None),
    };
    let texts = |v: &Value| match v {
        Value::Map(m) => m.get("a").map(|x| x.display()),
        _ => None,
    };
    let page = quillo::value::island_marks(&Value::from_json(&j), None);
    assert_eq!(kinds(&page), (Some(false), Some(false)));
    assert_eq!(texts(&page).as_deref(), Some("x; y"));
    let stored = quillo::interp::from_island_json(&j);
    assert_eq!(kinds(&stored), (Some(false), Some(false)));
    assert_eq!(texts(&stored).as_deref(), Some("x; y"));
    let plain = Value::from_json(&j);
    assert!(matches!(&plain, Value::Map(m) if matches!(m.get("a"), Some(Value::Map(_)))));
    let own = quillo::interp::from_own_island_json(&j);
    assert_eq!(kinds(&own), (Some(true), Some(true)));
    // The island's JSON writes the mark, the island's text the same; plain JSON the text.
    let v = Value::Css("a: 1; b: 2".into());
    assert_eq!(quillo::interp::island_json(&v).to_string(), r#"{"$css":"a: 1; b: 2"}"#);
    assert_eq!(v.island_text(), r#"{"$css":"a: 1; b: 2"}"#);
    assert_eq!(v.to_json().to_string(), r#""a: 1; b: 2""#);
    // As large as before: an Arc<str> beside the others, no more than a builtin's two words.
    assert_eq!(std::mem::size_of::<Value>(), 32);
}

/// A program's own `__css_value` is not the parser's, in the browser as on the server: a
/// visitor's value in css`...` stays escaped.
#[test]
fn a_programs_own_css_value_is_not_the_escape() {
    let src = r#"fn __css_value(x) = x
component C(v) {
    <p id="p" style={css`color: ${v}`}>a</p>
}
route GET / = <html><body><C v="red; background: blue"/></body></html>
"#;
    let s = served("shadow", src, &[], &[]);
    let page = s.get("/", &[]);
    assert!(page.body.contains(&format!("<p id=\"p\" style=\"color: {}\">a</p>", ONE)), "{}", page.body);
    let bundle = page.body.split('"').find(|p| p.starts_with("/_quillo/b/")).expect("a bundle").to_string();
    let js = s.get(&bundle, &[]).body;
    assert!(js.contains("W.csst(B.css((\"color: \" + W.display(B.__css_value($p.v)))))"), "{}", js);
    let _ = &s.dir;
}

/// What the parser puts around a component's `style` written as a template or a text is no
/// expression a program can write (`UnOp::Css`): a call by a made-up name is the program's own
/// function, whose answer is text, in the browser too; css`text`(v) is a text called, refused by
/// `quillo check` (FEEDBACK79.md, Q-1250; branch feedback72-81).
#[test]
fn the_parsers_mark_for_a_style_prop_cannot_be_written() {
    let d = Dir::new("mark", "view Box(style) = <p style={`a: 0; ${style}`}>x</p>\nlet v = \"red; top: 0\"\nprint(<Box style={css`text`(v)}/>)\n");
    let (ok, out) = quillo_in(&d.0, "check");
    assert!(!ok && out.contains("cannot call"), "{}", out);
    let out = run(r#"fn __css_text(x) = x
view Box(style, label) = <p style={`a: 0; ${style}`}>{label}</p>
let v = "red; top: 0"
print(<Box style={__css_text(v)} label="own"/>)
print(<Box style={`${__css_text(v)}`} label="inside"/>)
print(<Box style={`color: ${v}; b: 1`} label="written"/>)
"#);
    for (label, style) in [("own", r"a: 0; red\3b  top: 0"), ("inside", r"a: 0; red\3b  top: 0"), ("written", r"a: 0; color: red\3b  top: 0; b: 1")] {
        let want = format!("<p style=\"{}\">{}</p>", style, label);
        assert!(out.contains(&want), "{}\n{}", want, out);
    }
    let s = served(
        "mark-page",
        r#"fn __css_text(x) = x
component C(style) {
    <p id="p" style={`a: 0; ${style}`}>x</p>
}
component P() {
    <div><C style={__css_text("red; top: 0")}/><C style={`b: ${"1; c: 2"}`}/></div>
}
route GET / = <html><body><P/></body></html>
"#,
        &[],
        &[],
    );
    let page = s.get("/", &[]).body;
    assert!(page.contains(r#"style="a: 0; red\3b  top: 0""#), "{}", page);
    assert!(page.contains(r#"style="a: 0; b: 1; c: 2""#), "{}", page);
    let bundle = page.split('"').find(|p| p.starts_with("/_quillo/b/")).expect("a bundle").to_string();
    let js = s.get(&bundle, &[]).body;
    assert!(js.contains(r#"G["__css_text"]("#) && js.contains("W.csst("), "{}", js);
}

// ---- both renderers --------------------------------------------------------------------------

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

/// The page script around `body`, run once the runtime has started, with `$` and `tick`; what it
/// returns is the result, with the errors the runtime reported and how many components it made
/// again.
fn script(body: &str) -> String {
    format!(
        r#"(async () => {{
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const QW0 = () => window[Symbol.for("quillo")] || {{}};
  for (let i = 0; i < 300 && !QW0().booted; i++) await tick(30);
  const $ = (id) => document.getElementById(id);
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

/// `src` served, and `body` run at `path` in Chrome: what it gave back. None without Chrome.
fn in_page(name: &str, src: &str, path: &str, body: &str) -> Option<serde_json::Value> {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return None;
    };
    let s = served(name, src, &[], &[]);
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let t = s.dir.0.join("t.js");
    std::fs::write(&t, script(body)).unwrap();
    let run = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/run.js");
    let out = Command::new("node")
        .env_remove("QUILLO_PAGE_FOCUS")
        .env_remove("QUILLO_PAGE_CDP")
        .arg(&run)
        .arg(&chrome)
        .arg(format!("http://127.0.0.1:{}{}", s.port, path))
        .arg(s.dir.0.join("profile"))
        .arg("60000")
        .arg(&t)
        .output()
        .expect("node");
    assert!(out.status.success(), "{}: {}\n{}", path, String::from_utf8_lossy(&out.stderr), s.log());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)));
    assert!(v.get("exception").is_none(), "{}", v);
    Some(v)
}

const CARDS: &str = r#"component Card(style, label) {
    state k = 0
    <div class="card" id={label} title={k > 0 ? style : css`none`} style={`padding: 8px; ${style}`} onclick={() => { k = k + 1 }}>{label}</div>
}
fn said(s: str) -> str = s
fn deep() = `${join([css`x`, "y"], css`-`)} | ${sort([css`b`, "a", css`c`])} | ${contains([css`a`], "a")} ${contains(css`abc`, "b")} ${index_of([css`q`, "r"], "r")} ${index_of(css`abc`, "c")} | ${get({ k: css`v` }, css`k`)} ${keys(set({}, css`k`, 1))} | ${json.stringify({ s: [css`a: 1`] })} ${json.parse(css`[1]`)} | ${format_number(1234.5, css`de-DE`)} | ${date.format("2026-11-21", css`DD/MM/YYYY`)} ${date.format("2026-11-21", { pattern: css`YYYY`, locale: css`de-DE` })} ${date.parse(css`21/11/2026`, css`DD/MM/YYYY`)} ${date.valid(css`2026-11-21`)} ${date.from_ms(0, css`UTC`)} | ${split(css`a,b`, css`,`)} ${replace(css`a-b`, css`-`, css`+`)} | ${re.replace(css`a1b2`, css`[0-9]`, css`#`)} ${re.valid(css`[a`)} ${re.test(css`ab`, css`b`)} | ${money.format(money.of(css`12.5`, css`EUR`), css`de-DE`)} ${money.of(css`1.005`, "EUR", css`up`)} ${money.parse(css`12.50`, css`EUR`)} | ${int(css`12`)} ${dec(css`1.5`)} ${float(css`2.5`)} ${normalize(css`é`, css`NFD`) == normalize("é", "NFD")} | ${take(css`abc`, 2)} ${drop(css`abc`, 1)} ${slice(css`abcd`, 1, 3)} ${reverse(css`ab`)} | ${round(2.5m, 0, css`up`)} ${vec.column([{ a: 1 }], css`a`)} | ${type_of(first([css`a: 1`]))} ${day(css`2026-11-21`)} ${match css`2026-11-21` { date => "day" _ => "no" }}`
fn day(x: date) = x
fn text() = `${type_of(css`a`)} ${len(css`ab`)} ${upper(css`ab`)} ${css`a` == "a"} ${css`a` == css`a`} ${css`a` == 1} ${css`a` + "b"} ${css`a` + css`b`} ${css`a` + 1} ${json.stringify({ s: css`a: 1` })} ${[css`x`]} ${css`xy`[1]} ${[{ k: 7 }][0][css`k`]} ${css`y` < "z"} ${len(unique([css`a`, "a"]))} ${said(css`p`)} ${join(for c in css`ab` => c, "-")}`
"#;

/// A component merging its caller's css`...` keeps its declarations on both sides, byte for byte,
/// as the server writes it, once the browser takes it up, made again, and made by the browser
/// alone (css`...` worked out there); a visitor's text through the same prop is one value, and so
/// is a visitor's part of a css`...`; a style map takes it as it is; it is text elsewhere.
#[test]
fn the_browser_writes_trusted_css_as_the_server_does() {
    let src = format!(
        r#"{CARDS}let themes = {{ big: css`color: green; font-size: 20px` }}
component P(v) {{
    state shown = false
    <div>
        <button id="t" onclick={{() => {{ shown = true }}}}>t</button>
        {{shown ? <section id="browser">
            <Card style={{css`color: ${{v}}; font-size: 20px`}} label="b-tagged"/>
            <Card style={{v}} label="b-plain"/>
            <Card style={{css`color: green; font-size: 20px`}} label="b-lit"/>
            <Card style={{upper(css`color: green; font-size: 20px`)}} label="b-upper"/>
            <Card style={{css`margin: 0; ${{css`color: green; font-size: 20px`}}`}} label="b-nested"/>
            <Card style={{themes.big}} label="b-theme"/>
            <Card style={{`color: ${{v}}; font-size: 20px`}} label="b-tpl"/>
            <Card {{...{{ style: "color: green; font-size: 20px", label: "b-spread" }}}}/>
            <p id="b-deep">{{deep()}}</p>
            <p id="b-map" style={{{{ color: css`red; font-weight: bold` }}}}>m</p>
            <p id="b-text">{{text()}}</p>
            <p id="b-page">{{`${{js("Object.isExtensible", css`a`)}} ${{js("Array.from", [1], (x) => css`k`)}} ${{js("encodeURIComponent", [css`c d`])}}`}}</p>
        </section> : null}}
    </div>
}}
route GET / ? {{ v: str = "red; background: blue" }} = <html><body>
    <section id="server">
        <Card style={{css`color: ${{v}}; font-size: 20px`}} label="s-tagged"/>
        <Card style={{v}} label="s-plain"/>
        <Card style={{css`color: green; font-size: 20px`}} label="s-lit"/>
        <Card style={{upper(css`color: green; font-size: 20px`)}} label="s-upper"/>
        <Card style={{css`margin: 0; ${{css`color: green; font-size: 20px`}}`}} label="s-nested"/>
        <Card style={{themes.big}} label="s-theme"/>
        <Card style={{`color: ${{v}}; font-size: 20px`}} label="s-tpl"/>
        <Card {{...{{ style: "color: green; font-size: 20px", label: "s-spread" }}}}/>
        <p id="s-deep">{{deep()}}</p>
        <p id="s-map" style={{{{ color: css`red; font-weight: bold` }}}}>m</p>
        <p id="s-text">{{text()}}</p>
    </section>
    <P v={{v}}/>
</body></html>
"#
    );
    let Some(v) = in_page(
        "both",
        &src,
        "/",
        r#"const doc = new DOMParser().parseFromString(await (await fetch(location.href)).text(), "text/html");
  const names = ["tagged", "plain", "lit", "upper", "nested", "theme", "tpl", "spread"];
  const look = (id) => { const e = $(id), s = getComputedStyle(e); return [e.getAttribute("style"), s.fontSize, s.color, s.backgroundColor]; };
  const sent = names.map((n) => doc.getElementById("s-" + n).getAttribute("style")).concat([doc.getElementById("s-map").getAttribute("style")]);
  const taken = names.map((n) => look("s-" + n)).concat([look("s-map")]);
  for (const n of names) $("s-" + n).click();
  await tick(100);
  const again = names.map((n) => look("s-" + n));
  const titles = names.map((n) => $("s-" + n).getAttribute("title"));
  $("t").click();
  await tick(100);
  const made = names.map((n) => look("b-" + n)).concat([look("b-map")]);
  for (const n of names) $("b-" + n).click();
  await tick(100);
  const btitles = names.map((n) => $("b-" + n).getAttribute("title"));
  return { sent, taken, again, titles, made, btitles, stext: doc.getElementById("s-text").textContent, btext: $("b-text").textContent, page: $("b-page").textContent, sdeep: doc.getElementById("s-deep").textContent, bdeep: $("b-deep").textContent };"#,
    ) else {
        return;
    };
    let none = "rgba(0, 0, 0, 0)";
    let want = serde_json::json!([
        [format!("padding: 8px; color: {}; font-size: 20px", ONE), "20px", "rgb(0, 0, 0)", none],
        [format!("padding: 8px; {}", ONE), "16px", "rgb(0, 0, 0)", none],
        ["padding: 8px; color: green; font-size: 20px", "20px", "rgb(0, 128, 0)", none],
        [r"padding: 8px; COLOR: GREEN\3b  FONT-SIZE: 20PX", "16px", "rgb(0, 0, 0)", none],
        ["padding: 8px; margin: 0; color: green; font-size: 20px", "20px", "rgb(0, 128, 0)", none],
        // A top-level `let`'s, which the bundle carries.
        ["padding: 8px; color: green; font-size: 20px", "20px", "rgb(0, 128, 0)", none],
        // A template written in the `style` prop: trusted, its value escaped (the user's decision).
        [format!("padding: 8px; color: {}; font-size: 20px", ONE), "20px", "rgb(0, 0, 0)", none],
        // A literal map spread at the tag, its `style` written there: trusted as the attribute.
        ["padding: 8px; color: green; font-size: 20px", "20px", "rgb(0, 128, 0)", none],
        ["color: red; font-weight: bold", "16px", "rgb(255, 0, 0)", none],
    ]);
    let styles: Vec<serde_json::Value> = want.as_array().unwrap().iter().map(|x| x[0].clone()).collect();
    assert_eq!(v["sent"], serde_json::Value::Array(styles), "{}", v);
    assert_eq!(v["taken"], want, "{}", v);
    assert_eq!(v["made"], want, "{}", v);
    let cards = serde_json::Value::Array(want.as_array().unwrap()[..8].to_vec());
    assert_eq!(v["again"], cards, "{}", v);
    // Given whole, an attribute's text as it is: trusted or not.
    let titles = serde_json::json!([format!("color: {}; font-size: 20px", ONE), "red; background: blue", "color: green; font-size: 20px", "COLOR: GREEN; FONT-SIZE: 20PX", "margin: 0; color: green; font-size: 20px", "color: green; font-size: 20px", format!("color: {}; font-size: 20px", ONE), "color: green; font-size: 20px".to_string()]);
    assert_eq!(v["titles"], titles, "{}", v);
    assert_eq!(v["btitles"], titles, "{}", v);
    let text = r#"str 2 AB true true false ab ab a1 {"s":"a: 1"} ["x"] y 7 true 1 p a-b"#;
    assert_eq!(v["stext"], text, "{}", v);
    assert_eq!(v["btext"], text, "{}", v);
    // The page's code is given text, not an object: as an argument of `js`, inside a list, and
    // as what a function of the program's gives back to the page.
    assert_eq!(v["page"], r#"false ["k"] c%20d"#, "{}", v);
    // Builtins take trusted CSS as its text, given whole or inside a list or a map (an option),
    // on both sides, as they took css`...` when it was plain text.
    let deep = r#"x-y | ["a", "b", "c"] | true true 1 2 | v ["k"] | {"s":["a: 1"]} [1] | 1.234,5 | 21/11/2026 2026 2026-11-21 true 1970-01-01T00:00:00Z | ["a", "b"] a+b | a#b# false true | 12,50 € { amount: 1.01m, currency: "EUR" } { amount: 12.50m, currency: "EUR" } | 12 1.5 2.5 true | ab bc bc ba | 3 [1] | str 2026-11-21 day"#.replace("12,50 €", "12,50\u{a0}€");
    assert_eq!(v["sdeep"], deep, "{}", v);
    assert_eq!(v["bdeep"], deep, "{}", v);
    assert_eq!(v["remade"], 0, "{}", v);
    assert_eq!(v["errors"], serde_json::json!([]), "{}", v);
}

/// A `live` answer (as the page is made, and from the server's kept answer for the next page) and
/// a server fn's answer reach the page as trusted CSS; a server fn given the page's trusted CSS
/// gets text.
#[test]
fn live_data_and_a_server_fns_answer_carry_trust_to_the_page() {
    let src = format!(
        r#"{CARDS}server fn look() = css`color: green; font-size: 20px`
server fn kind(s) -> str = `${{type_of(s)}} ${{str(<i style={{`x: ${{s}}`}}>a</i>)}}`
component L() {{
    state s = live(look)
    state got = null
    state sent = ""
    <div>
        <Card style={{s ?? ""}} label="live"/>
        <button id="ask" onclick={{() => {{ got = look() }}}}>ask</button>
        <button id="send" onclick={{() => {{ sent = kind(css`a: 1; b: 2`) }}}}>send</button>
        {{got != null ? <Card style={{got}} label="rpc"/> : null}}
        <p id="sent">{{sent}}</p>
    </div>
}}
route GET / = <html><body><L/></body></html>
"#
    );
    let lit = "padding: 8px; color: green; font-size: 20px";
    // The first page makes the live answer, the second takes the server's kept one.
    let s = served("live-kept", &src, &[], &[]);
    for _ in 0..2 {
        let page = s.get("/", &[]).body;
        assert!(page.contains(&format!("id=\"live\" title=\"none\" style=\"{}\"", lit)), "{}\n{}", page, s.log());
        assert!(page.contains(r#"{&quot;$css&quot;:&quot;color: green; font-size: 20px&quot;}"#), "{}", page);
    }
    let body = r#"const st = (id) => $(id) && $(id).getAttribute("style");
  const first = st("live");
  $("ask").click();
  $("send").click();
  for (let i = 0; i < 100 && (!$("rpc") || !$("sent").textContent); i++) await tick(30);
  return { first, rpc: st("rpc"), sent: $("sent").textContent };"#;
    let Some(v) = in_page("live", &src, "/", body) else {
        return;
    };
    assert_eq!(v["first"], lit, "{}", v);
    assert_eq!(v["rpc"], lit, "{}", v);
    assert_eq!(v["sent"], r#"str <i style="x: a: 1\3b  b: 2">a</i>"#, "{}", v);
    assert_eq!(v["errors"], serde_json::json!([]), "{}", v);
}

/// A literal map spread at a component's or view's tag gives its `style` as the attribute does:
/// trusted CSS when written there as a template or a text, a visitor's part one value
/// (FEEDBACK79.md, Q-1250, found in the review of the trusted CSS; branch feedback72-81).
#[test]
fn a_spread_literal_style_at_a_tag_is_trusted_as_the_attribute() {
    let out = run(r#"view Box(style, label) = <p style={`a: 0; ${style}`}>{label}</p>
let v = "red; top: 0"
print(<Box {...{ style: "color: green; b: 1", label: "lit" }}/>)
print(<Box {...{ style: `color: ${v}; b: 1`, label: "tpl" }}/>)
print(<Box {...{ style: v, label: "name" }}/>)
"#);
    for (label, style) in [("lit", "a: 0; color: green; b: 1"), ("tpl", r"a: 0; color: red\3b  top: 0; b: 1"), ("name", r"a: 0; red\3b  top: 0")] {
        let want = format!("<p style=\"{}\">{}</p>", style, label);
        assert!(out.contains(&want), "{}\n{}", want, out);
    }
}

/// A function a script makes runs only while the script does. Given to a helper the program handed
/// in, whole or in a list, a map or an error, the helper may call it back during the script, where
/// it runs as the script's: no effects (a helper's own script and a tag the helper renders it as
/// too, which on 0.8.9 gave it the program's), its steps counted (a callback looping past the
/// budget stops with "too many steps"). Kept past the script, it is not run, an error value,
/// wherever it is called (directly, from a list, a map or an error, by `map`'s quick path for
/// `(x) => x`, as a tag, in `parallel`; a tick: the next test); 0.8.9 ran a kept one later with no
/// limit of steps or time. Builtins the script calls take its functions under its limits; a helper
/// given plain values runs; `parallel` given to a script runs its functions in order under its
/// limits; a css`...` a script's function makes, kept by a helper, is plain text (Q-1274, found
/// reviewing the trusted CSS of FEEDBACK79.md's Q-1250; branch feedback72-81).
///
/// Changed with the user's decision on Q-1274 (a script's function may be passed to the program's
/// helpers and called back while the script runs, as the script's; once it has ended, calling one
/// gives an error value): 45adbcb refused the helper's call itself whenever a script's function
/// was in its arguments ("a script's function stays in the script"), which broke tests/security.rs
/// `scripts_have_no_effects_of_their_own` (a helper calling back the script's `(h) => h`). So
/// `hold(...)` of a function, in a list, a map or an error, now gives the helper's 1, not that
/// refusal; `each([1, 2], (x) => x * 2)` gives [2, 4], not the refusal; the helper keeps 5
/// values, not 1; and each kept one, called after, gives the new error.
#[test]
fn a_scripts_function_stays_in_the_script() {
    let out = run(r#"let holder = []
let kept_css = ""
fn hold(f) {
    holder = push(holder, f)
    return 1
}
fn call_last() = holder[len(holder) - 1]()
fn keep_css(f) {
    kept_css = f()
    return 1
}
fn each(xs, f) = map(xs, f)
fn inner(f) = run_script("g()", { g: f })
fn show(F) = str(<div><F/></div>)
view Box(style) = <p style={{ color: style }}>x</p>
fn w(x) = str(<Box style={x}/>)
print(run_script("let n = 0\nwhile n < 100000 { n = n + 1 }\nn", {}))
print(run_script("hold(() => { let n = 0\nwhile n < 100000 { n = n + 1 }\nreturn n })", { hold: hold }))
print(run_script("hold([(x) => x])", { hold: hold }))
print(run_script("hold({ f: (x) => x })", { hold: hold }))
print(run_script("hold(err((x) => x))", { hold: hold }))
print(run_script("hold(1)", { hold: hold }))
print(run_script("each([1, 2], (x) => x * 2)", { each: each }))
print(run_script("sort_by(map(filter([3, 1, 2], (x) => x > 1), (x) => x * 10), (x) => -x)", {}))
print(run_script("each([1, 2], upper_or)", { each: each, upper_or: (x) => x + 1 }))
print(len(holder))
// Kept past the script: not run, wherever it is called.
print(holder[0]())
print(holder[1][0](3))
print(holder[2].f(3))
print(error(holder[3])(3))
print(map([3], holder[1][0]))
print(parallel([() => holder[0](), () => 0]))
print(show(holder[1][0]))
print(holder[4])
// Called back during the script: runs, as the script's, its steps counted.
print(run_script("hold(() => 7)\ncall_last()", { hold: hold, call_last: call_last }))
print(run_script("hold(() => { let n = 0\nwhile n < 100000 { n = n + 1 }\nreturn n })\ncall_last()", { hold: hold, call_last: call_last }))
print(run_script("hold(() => e(\"HOME\"))\ncall_last()", { hold: hold, call_last: call_last, e: env }))
print(run_script("inner(() => e(\"HOME\"))", { inner: inner, e: env }))
print(run_script("inner(() => { let n = 0\nwhile n < 100000 { n = n + 1 }\nreturn n })", { inner: inner }))
print(run_script("show(() => e(\"HOME\"))", { show: show, e: env }))
print(run_script("show(() => 5)", { show: show }))
print(len(holder))
print(holder[5]())
// What a script's function makes is the script's: css`...` is text, kept as text.
print(run_script("keep_css(() => css`red; background: blue`)", { keep_css: keep_css, css: css }))
print(type_of(kept_css))
print(w(kept_css))
// `parallel` given to a script runs its functions under its limits, and makes it no trusted CSS.
print(run_script("parallel([() => { let n = 0\nwhile n < 100000 { n = n + 1 }\nreturn n }, () => 0])", { parallel: parallel }))
print(run_script("w(parallel([() => css`red; background: blue`, () => 0])[0])", { parallel: parallel, w: w, css: css }))
print(run_script("parallel([() => 1, () => 2])", { parallel: parallel }))
"#);
    let ended = "err(\"a script's function runs only while its script does\")";
    let steps = "err(\"line 2: the script took too many steps\")";
    let env = "err(\"line 1: `env` needs the env effect, which is not allowed here\")";
    let escaped = "<p style=\"color: red\\3b  background: blue\">x</p>";
    let want = [
        steps,
        "1",
        "1",
        "1",
        "1",
        "1",
        "[2, 4]",
        "[30, 20]",
        "[2, 3]",
        "5",
        ended,
        ended,
        ended,
        ended,
        &format!("[{}]", ended),
        &format!("[{}, 0]", ended),
        "<div></div>",
        "1",
        "7",
        steps,
        env,
        env,
        steps,
        env,
        "<div>5</div>",
        "8",
        ended,
        "1",
        "str",
        escaped,
        steps,
        escaped,
        "[1, 2]",
    ]
    .join("\n")
        + "\n";
    // What it printed first; then what it said, the tag's kept function where the program shows
    // it, as an error markup shows nothing.
    assert!(out.starts_with(&want), "{}", out);
    assert!(out[want.len()..].contains("main.qwe:14: an error no statement took, shown as nothing: a script's function runs only while its script does"), "{}", out);
}

/// A function a script made, kept by a helper and called by a tick after the script ended, is not
/// run: an error value; 0.8.9 ran it, 100,000 steps past the script's 20,000
/// (Q-1274, found reviewing the trusted CSS of FEEDBACK79.md's Q-1250; branch feedback72-81).
#[test]
fn a_kept_script_function_called_by_a_tick_is_not_run() {
    let s = served(
        "tick",
        r#"let holder = []
let seen = "none"
fn hold(f) {
    holder = push(holder, f)
    return 1
}
fn tick() {
    if len(holder) > 0 {
        let r = holder[0]()
        seen = is_err(r) ? error(r) : "ran"
    }
}
every(20, tick)
route GET /keep = str(run_script("hold(() => { let n = 0\nwhile n < 100000 { n = n + 1 }\nreturn n })", { hold: hold }))
route GET /seen = seen
"#,
        &[],
        &[],
    );
    assert_eq!(s.get("/seen", &[]).body, "none");
    assert_eq!(s.get("/keep", &[]).body, "1");
    let deadline = Instant::now() + Duration::from_secs(60);
    let seen = loop {
        let b = s.get("/seen", &[]).body;
        if b != "none" || Instant::now() > deadline {
            break b;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(seen, "a script's function runs only while its script does", "{}", s.log());
}

/// Requests take turns on one thread, each with its scripts' runs of its own: one script that
/// starts first and ends first leaves another request's still-running script its run (a list kept
/// for the thread lost it, and `inner(() => 42)` was the run-ended error), and a function one
/// request's script made, still running, does not run in another request's script, which got 7
/// (Q-1274, found reviewing the trusted CSS of FEEDBACK79.md's Q-1250, in its review; branch
/// feedback72-81).
#[test]
fn each_requests_scripts_keep_their_own_runs() {
    let s = served(
        "turns",
        r#"let on = []
let kept = []
fn nap(ms) {
    sleep(ms)
    return 1
}
fn mark(k) {
    on = push(on, k)
    return 1
}
fn inner(f) = run_script("g()", { g: f })
fn hold(f) {
    kept = push(kept, f)
    return 1
}
fn call_kept() = kept[0]()
route GET /on = join(on, ",")
route GET /x = str(run_script("mark(\"x\")\nnap(1500)\n1", { nap: nap, mark: mark }, { timeout: 20000 }))
route GET /y = str(run_script("mark(\"y\")\nnap(3000)\ninner(() => 42)", { nap: nap, mark: mark, inner: inner }, { timeout: 20000 }))
route GET /a = str(run_script("hold(() => 7)\nmark(\"a\")\nnap(3000)\n1", { hold: hold, nap: nap, mark: mark }, { timeout: 20000 }))
route GET /b {
    let r = run_script("call_kept()", { call_kept: call_kept }, { timeout: 20000 })
    return is_err(r) ? error(r) : str(r)
}
"#,
        &[],
        &[],
    );
    let port = s.port;
    let until_on = |k: &str| {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !s.get("/on", &[]).body.split(',').any(|x| x == k) {
            assert!(Instant::now() < deadline, "{} never started:\n{}", k, s.log());
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let ask = |path: &'static str| {
        let order = order.clone();
        std::thread::spawn(move || {
            let body = support::send(port, "GET", path, &[], "").body;
            order.lock().unwrap().push(path);
            body
        })
    };
    // /x's script starts first and ends first, while /y's still runs.
    let x = ask("/x");
    until_on("x");
    let y = ask("/y");
    until_on("y");
    assert_eq!(x.join().unwrap(), "1");
    assert_eq!(y.join().unwrap(), "42", "{}", s.log());
    assert_eq!(*order.lock().unwrap(), ["/x", "/y"]);
    // /a's script keeps a function and is still running when /b's script calls it.
    let a = ask("/a");
    until_on("a");
    assert_eq!(s.get("/b", &[]).body, "a script's function runs only while its script does", "{}", s.log());
    assert!(!order.lock().unwrap().contains(&"/a"), "/a ended before /b asked: {}", s.log());
    assert_eq!(a.join().unwrap(), "1");
}

/// What a script's function gives back to a helper is plain, as what the script hands one is: a
/// helper's trusted CSS a callback hands back (`() => getc()`, through `map`, in a list, or the
/// value the helper gave it) is escaped in the helper's style; the program's own callback still
/// gives it trusted (Q-1274, found reviewing the trusted CSS of FEEDBACK79.md's Q-1250, in its
/// review; branch feedback72-81).
#[test]
fn what_a_scripts_function_gives_a_helper_is_plain() {
    let out = run(r#"let C = css`red; position: fixed; inset: 0`
fn getc() = C
fn show_cb(f) = str(<div style={{ color: f() }}/>)
fn show_val(v) = str(<div style={{ color: v }}/>)
fn show_map(f) = str(<div style={{ color: map([1], (x) => f())[0] }}/>)
fn show_given(f) = str(<div style={{ color: f(C) }}/>)
print(run_script("show_cb(() => getc())", { show_cb: show_cb, getc: getc }))
print(run_script("show_val(getc())", { show_val: show_val, getc: getc }))
print(run_script("show_map(() => getc())", { show_map: show_map, getc: getc }))
print(run_script("show_given((c) => c)", { show_given: show_given }))
print(run_script("show_cb(() => [getc()][0])", { show_cb: show_cb, getc: getc }))
print(run_script("show_cb(() => \"blue\")", { show_cb: show_cb }))
print(show_cb(() => C))
"#);
    let escaped = "<div style=\"color: red\\3b  position: fixed\\3b  inset: 0\"></div>";
    let want = [escaped, escaped, escaped, escaped, escaped, "<div style=\"color: blue\"></div>", "<div style=\"color: red; position: fixed; inset: 0\"></div>"].join("\n") + "\n";
    assert!(out.ends_with(&want), "{}", out);
}

/// A function a script made, which a helper kept in a global a component reads, is never
/// compiled for the browser, where it would run a visitor's script in every page: the program
/// stops at its start, saying so; 0.8.9 wrote `W.add($x_1, 1000)` into the page's bundle. The
/// program's own function there still is (Q-1274, found reviewing the trusted CSS of
/// FEEDBACK79.md's Q-1250, in its review; branch feedback72-81).
#[test]
fn a_kept_script_function_never_reaches_the_browser() {
    let src = |keep: &str| {
        format!(
            r#"let stash = (x) => x
fn keep(f) {{
    stash = f
    return 1
}}
{}
let pick = stash
component Shows() {{
    state n = 1
    <button onclick={{() => n = pick(n)}}>{{n}}</button>
}}
route GET / = <html><head><title>t</title></head><body><Shows/></body></html>
"#,
            keep
        )
    };
    let d = Dir::new("bundle", &src("let made = run_script(\"keep((x) => x + 1000)\", { keep: keep })"));
    let (ok, out) = quillo_in(&d.0, "check");
    assert!(ok, "{}", out);
    let mut c = Command::new(quillo()).arg("run").arg(d.main()).args(["--port", "0"]).current_dir(&d.0).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    let mut se = c.stderr.take().unwrap();
    let t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = se.read_to_string(&mut s);
        s
    });
    let status = wait_for(&mut c, Duration::from_secs(120));
    let said = t.join().unwrap();
    assert!(status.is_some_and(|s| !s.success()), "{}", said);
    assert!(
        said.contains("<Shows> (compiled for the browser): `pick` holds a function a script made (`run_script`), which runs only while its script does and never in the browser"),
        "{}",
        said
    );
    // The program's own function in the same place.
    let s = served("bundle-own", &src("let made = 1"), &[], &[]);
    let page = s.get("/", &[]);
    assert_eq!(page.status, 200, "{}", s.log());
}

/// A server fn's answer the program wrote itself (`respond`, `reply`) is the program's JSON: the
/// page takes no trusted CSS from a mark in it, which data could hold, while the server's own
/// answer of trusted CSS still is (FEEDBACK79.md, Q-1250, found in the review of the trusted CSS;
/// branch feedback72-81).
#[test]
fn a_programs_own_answer_body_carries_no_trust() {
    let src = r#"server fn prefs(raw: str) {
    return respond(200, { ok: json.parse(raw) })
}
server fn told(raw: str) {
    reply 200 { ok: json.parse(raw) }
}
server fn own() = { color: css`green; font-size: 20px` }
server fn nest(raw: str) = respond(200, { ok: json.parse(raw) })
fn pick(x: any) = x
component C() {
    state a = null
    state b = null
    state c = null
    state n1 = null
    state n2 = null
    state n3 = null
    <div>
        <button id="go" onclick={() => { a = prefs("{\"color\": {\"$css\": \"red; background: blue\"}}"); b = told("{\"color\": {\"$css\": \"red; background: blue\"}}"); c = own(); n1 = pick(nest("{\"k\":{\"$css\":{\"inner\":{\"$css\":\"red; background: blue\"}}}}")).k["$css"].inner; n2 = pick(nest("{\"k\":{\"$css\":{\"$css\":\"red; background: blue\"}}}")).k["$css"]; n3 = pick(nest("{\"k\":{\"$\":[\"$css\",{\"$css\":\"red; background: blue\"}]}}")).k["$css"] }}>go</button>
        {n1 != null ? <p id="n1" style={`padding: 1px; color: ${n1}`}>{type_of(n1)}</p> : null}
        {n2 != null ? <p id="n2" style={`padding: 1px; color: ${n2}`}>{type_of(n2)}</p> : null}
        {n3 != null ? <p id="n3" style={`padding: 1px; color: ${n3}`}>{type_of(n3)}</p> : null}
        {a != null ? <p id="a" style={`padding: 1px; color: ${a.color}`}>{type_of(a.color)}</p> : null}
        {b != null ? <p id="b" style={`padding: 1px; color: ${b.color}`}>{type_of(b.color)}</p> : null}
        {c != null ? <p id="c" style={`padding: 1px; color: ${c.color}`}>{type_of(c.color)}</p> : null}
    </div>
}
route GET / = <html><body><C/></body></html>
"#;
    let Some(v) = in_page(
        "own-answer",
        src,
        "/",
        r#"$("go").click();
  for (let i = 0; i < 100 && !($("a") && $("b") && $("c") && $("n1") && $("n2") && $("n3")); i++) await tick(30);
  const st = (id) => [$(id).getAttribute("style"), $(id).textContent, getComputedStyle($(id)).backgroundColor];
  return { a: st("a"), b: st("b"), c: st("c"), n1: st("n1"), n2: st("n2"), n3: st("n3") };"#,
    ) else {
        return;
    };
    let none = "rgba(0, 0, 0, 0)";
    // A mark inside a mark, or in the `{"$": [k, v]}` form, is the program's data too.
    for k in ["a", "b", "n1", "n2", "n3"] {
        let got = &v[k];
        assert_eq!(got[1], "map", "{}", v);
        assert_eq!(got[2], none, "{}", v);
        assert!(got[0].as_str().unwrap().contains(r"\3b "), "{}", v);
    }
    assert_eq!(v["c"], serde_json::json!(["padding: 1px; color: green; font-size: 20px", "str", none]), "{}", v);
    assert_eq!(v["errors"], serde_json::json!([]), "{}", v);
}

/// No program's answer can carry the mark of a server fn's own answer, which tells the page to take
/// trusted CSS from it: `respond`'s headers leave it out in any spelling (so passing on another
/// server fn's answer and its headers works as on 0.8.9), `site_headers` refuses it, an idempotent
/// replay of a program's body has none; the server's own answer, and its replay, keep it. In
/// Chrome each attempt gives the map, one escaped value (FEEDBACK79.md, Q-1250, found in the
/// reviews of the trusted CSS; branch feedback72-81).
#[test]
fn no_program_answer_carries_the_servers_mark() {
    let src = r#"server fn own() = { color: css`green; font-size: 20px` }
server fn body() = respond(200, { ok: { color: { "$css": "red; background: blue" } } })
server fn marked_try() = respond(200, { ok: { color: { "$css": "red; background: blue" } } }, { "X-Quillo-Marked": "1" })
server fn lower_try() = respond(200, { ok: { color: { "$css": "red; background: blue" } } }, { "x-quillo-marked": "1" })
server fn pay() = idempotent(() => respond(200, { ok: { color: { "$css": "red; background: blue" } } }))
server fn pay_own() = idempotent(() => { color: css`green; font-size: 20px` })
route GET /proxy {
    let r = http.post(`http://${request.headers["host"]}/_quillo/rpc/own`, "[]", { "content-type": "application/json", "sec-fetch-site": "same-origin" })
    return respond(r.status, r.body, r.headers)
}
component C() {
    state got = {}
    <div>
        <button id="go" onclick={() => { got = { body: body(), marked: marked_try(), lower: lower_try(), pay: pay(), own: own() } }}>go</button>
        {for k in keys(got) => <p id={k} style={`padding: 1px; color: ${is_err(got[k]) ? "error" : got[k].color}`}>{is_err(got[k]) ? "error" : type_of(got[k].color)}</p>}
    </div>
}
route GET / = <html><body><C/></body></html>
"#;
    {
        let s = served("mark-headers", src, &[], &[]);
        let rpc = |f: &str, key: &str| s.post(&format!("/_quillo/rpc/{}", f), &[("content-type", "application/json"), ("idempotency-key", key)], "[]");
        let mark = |a: &support::Answer| a.header("x-quillo-marked").is_some();
        let a = rpc("own", "k1");
        assert!(mark(&a) && a.status == 200, "{:?}", a.headers);
        let a = rpc("body", "k2");
        assert!(!mark(&a) && a.status == 200, "{:?}", a.headers);
        for f in ["marked_try", "lower_try"] {
            let a = rpc(f, "k3");
            assert!(!mark(&a) && a.status == 200, "{}: {} {:?}", f, a.status, a.headers);
        }
        // Another server fn's answer passed on with its headers: as on 0.8.9, but with no mark.
        let host = format!("127.0.0.1:{}", s.port);
        let a = s.get("/proxy", &[("host", &host)]);
        assert!(!mark(&a) && a.status == 200 && a.body == r#"{"ok":{"color":{"$css":"green; font-size: 20px"}}}"#, "{} {:?} {}", a.status, a.headers, a.body);
        for _ in 0..2 {
            let a = rpc("pay", "k4");
            assert!(!mark(&a) && a.status == 200, "{:?}", a.headers);
            let a = rpc("pay_own", "k5");
            assert!(mark(&a) && a.status == 200, "{:?}", a.headers);
        }
    }
    let d = Dir::new("mark-site", "site_headers({ \"X-Quillo-Marked\": \"1\" })\nroute GET / = \"x\"\n");
    let mut c = Command::new(quillo()).arg("run").arg(d.main()).args(["--port", "0"]).current_dir(&d.0).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    let status = wait_for(&mut c, Duration::from_secs(60)).expect("it stops");
    let mut err = String::new();
    c.stderr.take().unwrap().read_to_string(&mut err).unwrap();
    assert!(!status.success() && err.contains("`site_headers`: `x-quillo-marked` is Quillo's own"), "{}", err);
    let Some(v) = in_page(
        "mark-page",
        src,
        "/",
        r#"$("go").click();
  for (let i = 0; i < 100 && !$("own"); i++) await tick(30);
  const st = (id) => [$(id).getAttribute("style"), $(id).textContent, getComputedStyle($(id)).backgroundColor];
  return { body: st("body"), marked: st("marked"), lower: st("lower"), pay: st("pay"), own: st("own") };"#,
    ) else {
        return;
    };
    let none = "rgba(0, 0, 0, 0)";
    for k in ["body", "pay", "marked", "lower"] {
        assert_eq!(v[k][1], "map", "{}", v);
        assert_eq!(v[k][2], none, "{}", v);
        assert!(v[k][0].as_str().unwrap().contains(r"\3b "), "{}", v);
    }
    assert_eq!(v["own"], serde_json::json!(["padding: 1px; color: green; font-size: 20px", "str", none]), "{}", v);
}

/// A server fn's call that a redirect takes to another origin: whatever that answer says, its mark
/// is not the page's server's, and its trusted CSS mark is data, after a 302 and a 307 alike
/// (FEEDBACK79.md, Q-1250, found in the second review of the trusted CSS; branch feedback72-81).
#[test]
fn a_redirected_answer_carries_no_trust() {
    if chrome().is_none() {
        return;
    }
    // Another origin that answers as the server's own answer would, and lets the page read it.
    let js = r#"const http = require("http");
const h = { "access-control-allow-origin": "*", "access-control-allow-headers": "*", "access-control-allow-methods": "*", "access-control-expose-headers": "x-quillo-marked, x-quillo-answer" };
const s = http.createServer((req, res) => {
  if (req.method === "OPTIONS") { res.writeHead(204, h); return res.end(); }
  res.writeHead(200, { ...h, "content-type": "application/json", "x-quillo-marked": "1", "x-quillo-answer": "1" });
  res.end(JSON.stringify({ ok: { color: { $css: "red; background: blue" } } }));
});
s.listen(0, "127.0.0.1", () => console.log(s.address().port));
setTimeout(() => process.exit(0), 120000);
"#;
    let mut other = Command::new("node").args(["-e", js]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut first = String::new();
    {
        use std::io::BufRead;
        let mut r = std::io::BufReader::new(other.stdout.take().unwrap());
        r.read_line(&mut first).unwrap();
    }
    let _other = support::Guard::new(other);
    let port: u16 = first.trim().parse().unwrap_or_else(|_| panic!("no port: {:?}", first));
    let src = format!(
        r#"server fn go(code: int) {{
    return respond(code, "", {{ location: "http://127.0.0.1:{port}/x" }})
}}
component C() {{
    state a = null
    state b = null
    <div>
        <button id="go" onclick={{() => {{ a = go(302); b = go(307) }}}}>go</button>
        {{a != null ? <p id="a" style={{`padding: 1px; color: ${{is_err(a) ? "error" : a.color}}`}}>{{is_err(a) ? "error" : type_of(a.color)}}</p> : null}}
        {{b != null ? <p id="b" style={{`padding: 1px; color: ${{is_err(b) ? "error" : b.color}}`}}>{{is_err(b) ? "error" : type_of(b.color)}}</p> : null}}
    </div>
}}
route GET / = <html><body><C/></body></html>
"#
    );
    let Some(v) = in_page(
        "redirect",
        &src,
        "/",
        r#"$("go").click();
  for (let i = 0; i < 100 && !($("a") && $("b")); i++) await tick(30);
  const st = (id) => [$(id).getAttribute("style"), $(id).textContent, getComputedStyle($(id)).backgroundColor];
  return { a: st("a"), b: st("b") };"#,
    ) else {
        return;
    };
    for k in ["a", "b"] {
        assert_eq!(v[k][1], "map", "{}", v);
        assert_eq!(v[k][2], "rgba(0, 0, 0, 0)", "{}", v);
        assert!(v[k][0].as_str().unwrap().contains(r"\3b "), "{}", v);
    }
}

// ---- every builtin of the browser's, given trusted CSS -----------------------------------------

/// Each builtin the browser's runtime defines, called with text in the shapes it takes (given
/// whole, in a list, in a map or an option), `«x»` a text: written `"x"`, and css`x`.
const SWEEP: &str = r#"str|str(«ab»)
int|int(«12»)
float|float(«2.5»)
dec|dec(«1.5»)
len|[len(«ab»), len([«a»])]
type_of|type_of(«ab»)
upper|upper(«ab»)
lower|lower(«AB»)
normalize|normalize(«é», «NFD») == normalize("é", "NFD")
trim|trim(« ab »)
filled|[filled(«»), filled(«a»)]
split|split(«a,b», «,»)
join|join([«x», "y"], «-»)
contains|[contains(«abc», «b»), contains([«a»], «a»), contains({ k: 1 }, «k»)]
starts_with|starts_with(«abc», «a»)
ends_with|ends_with(«abc», «c»)
replace|replace(«a-b», «-», «+»)
slice|[slice(«abcd», 1, 3), slice([«a», «b»], 1)]
map|map([«a»], (x) => upper(x))
filter|filter([«a», «b»], (x) => x == «a»)
find|find([«a», «b»], (x) => x == «b»)
any|any([«a»], (x) => x == «a»)
all|all([«a»], (x) => x == «a»)
reduce|reduce([«a», «b»], «», (a, x) => a + x)
count|[count([«a», «b»], (x) => x == «a»), count([«a»])]
group_by|group_by([«a», «b», «a»], (x) => x)
unique|unique([«a», "a", «b»])
flatten|flatten([[«a»], [«b»]])
zip|zip([«a»], [«b»])
enumerate|enumerate([«a»])
first|first([«a», «b»])
last|last([«a», «b»])
keys|keys({ a: «x» })
values|values({ a: «x» })
get|[get({ k: «v» }, «k»), get({ k: 1 }, «j», «d»)]
has|has({ k: 1 }, «k»)
set|[set({}, «k», «v»), set([«a»], 0, «b»)]
remove|[remove({ k: 1, j: 2 }, «k»), remove([«a», «b»], 0)]
merge|merge({ a: «x» }, { b: «y» })
push|push([«a»], «b»)
reverse|[reverse(«ab»), reverse([«a», «b»])]
sort|sort([«b», «a»])
sort_by|sort_by([{ n: «b» }, { n: «a» }], (r) => r.n)
min|min([«b», «a»])
max|max([«b», «a»])
take|[take(«abc», 2), take([«a», «b»], 1)]
drop|[drop(«abc», 1), drop([«a», «b»], 1)]
index_of|[index_of(«abc», «c»), index_of([«q», «r»], «r»)]
repeat|repeat(«ab», 2)
escape|escape(«<b>&»)
raw|raw(«<b>x</b>»)
url_encode|url_encode(«a b&c»)
format_number|format_number(1234.5, «de-DE»)
highlight|highlight(«let x = 1», «js»)
css|css(«a: 1»)
sql|sql(«select 1»)
err|err(«x»)
error|error(err(«x»))
is_err|is_err(«x»)
unwrap|unwrap(«x»)
round|round(2.5m, 0, «up»)
json.parse|json.parse(«[1, "a"]»)
json.stringify|json.stringify({ a: «x», b: [«y»] })
re.valid|re.valid(«a+»)
re.test|re.test(«ab», «b»)
re.find|re.find(«a1», «[0-9]»)
re.find_all|re.find_all(«a1b2», «[0-9]»)
re.replace|re.replace(«a1», «[0-9]», «#»)
re.split|re.split(«a1b», «[0-9]»)
money.of|money.of(«12.5», «EUR»)
money.parse|money.parse(«12.50», «EUR»)
money.from_minor|money.from_minor(1250, «EUR»)
money.to_minor|money.to_minor({ amount: «12.50», currency: «EUR» })
money.add|money.add(money.of("1", "EUR"), { amount: «2.00», currency: «EUR» })
money.sub|money.sub({ amount: «2.00», currency: «EUR» }, money.of("1", "EUR"))
money.mul|money.mul({ amount: «2.00», currency: «EUR» }, 3)
money.neg|money.neg({ amount: «2.00», currency: «EUR» })
money.compare|money.compare({ amount: «2.00», currency: «EUR» }, money.of("1", "EUR"))
money.zero|money.zero(«EUR»)
money.is_zero|money.is_zero({ amount: «0.00», currency: «EUR» })
money.round|money.round({ amount: «1.25», currency: «EUR» }, «up»)
money.minor_units|money.minor_units(«EUR»)
money.allocate|money.allocate({ amount: «10.00», currency: «EUR» }, [1, 2])
money.format|[money.format({ amount: «12.50», currency: «EUR» }, «de-DE»), money.format(money.of("1", "EUR"))]
vec.column|vec.column([{ a: 1 }], «a»)
date.parse|date.parse(«21/11/2026», «DD/MM/YYYY»)
date.valid_zone|date.valid_zone(«Europe/Paris»)
date.valid|[date.valid(«2026-11-21»), date.valid(«2026-11-21», «date»)]
date.parts|date.parts(«2026-11-21»)
date.weekday|date.weekday(«2026-11-21»)
date.is_leap_year|date.is_leap_year(«2024-01-01»)
date.days_in_month|date.days_in_month(«2026-02-01»)
date.add_days|date.add_days(«2026-11-21», 1)
date.add_months|date.add_months(«2026-11-21», 1)
date.add_years|date.add_years(«2026-11-21», 1)
date.days_between|date.days_between(«2026-11-01», «2026-11-21»)
date.months_between|date.months_between(«2026-01-01», «2026-11-21»)
date.start_of_month|date.start_of_month(«2026-11-21»)
date.end_of_month|date.end_of_month(«2026-11-21»)
date.to_zone|date.to_zone(«2026-11-21T10:00:00Z», «Europe/Paris»)
date.compare|date.compare(«2026-11-21», «2026-11-22»)
date.date_in|date.date_in(«2026-11-21T10:00:00Z», «Europe/Paris»)
date.at|date.at(«2026-11-21», «10:30», «Europe/Paris»)
date.ms|date.ms(«2026-11-21T10:00:00Z»)
date.from_ms|date.from_ms(0, «UTC»)
date.format|[date.format(«2026-11-21», «DD/MM/YYYY»), date.format(«2026-11-21», { pattern: «YYYY», locale: «de-DE» }), date.format(«2026-11-21», { style: «long», locale: «de-DE» })]
sum|sum([1, 2.5])
stats.mode|stats.mode([«a», "a", «b»])
stats.multimode|stats.multimode([«a», «b»])
(a money pattern)|match { amount: «1.00», currency: «EUR» } { money => "money" _ => "no" }"#;

/// The browser's builtins the sweep does not call, and why: nothing of theirs reads text, they act
/// on the page, the network or the clock, or they are the parser's own.
const NOT_SWEPT: &[(&str, &str)] = &[
    ("math.", "numbers"),
    ("bit.", "numbers"),
    ("stats.", "numbers"),
    ("vec.", "numbers (but `vec.column`, swept)"),
    ("http.", "the network"),
    ("notification.", "the page's notifications"),
    ("channel.", "a channel"),
    ("abs", "numbers"),
    ("ceil", "numbers"),
    ("floor", "numbers"),
    ("pow", "numbers"),
    ("sqrt", "numbers"),
    ("range", "numbers"),
    ("random", "chance"),
    ("random_int", "chance"),
    ("uuid", "chance"),
    ("now", "the clock"),
    ("date.now", "the clock"),
    ("date.today", "the clock"),
    ("date.of", "numbers"),
    ("print", "the console"),
    ("ask", "the page's dialog"),
    ("notify", "the page's toast"),
    ("navigate", "the page's address"),
    // FEEDBACK84.md, Q-1320, Q-1328 (branch feedback82-85): new builtins; `page_head`'s values are
    // written as the head's text and attributes, escaped, `page_url`'s are the page's address.
    ("page_head", "the page's head"),
    ("page_url", "the page's address"),
    ("url", "the page's address"),
    ("local_path", "the page's address"),
    ("cursor_index", "a field's caret"),
    ("locale", "the page's language"),
    ("t", "a translation table"),
    ("t_html", "a translation table"),
    ("markdown", "asks the server"),
    ("js", "the page's code"),
    ("js_has", "the page's code"),
    ("js_new", "the page's code"),
    ("js_value", "the page's code"),
    ("listen", "the page's events"),
    ("receive", "a stream"),
    ("every", "a timer"),
    ("run_after", "a timer"),
    ("defer", "a timer"),
    ("parallel", "server fns"),
    ("page_updated", "the page"),
    ("waiting", "a live state"),
    ("__concat", "the parser's"),
    ("__css_value", "the parser's"),
    ("__merge", "the parser's"),
    ("__record", "the parser's"),
];

/// The browser's builtins, as runtime.js defines them: `B.name = ...`, and each module's `name:`.
fn browser_builtins() -> Vec<String> {
    let js = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/client/runtime.js")).unwrap();
    let mut mods = Vec::new();
    let mut names = Vec::new();
    for l in js.lines() {
        let Some(rest) = l.strip_prefix("B.") else { continue };
        let name: String = rest.chars().take_while(|c| c.is_ascii_lowercase() || *c == '_').collect();
        let after = rest[name.len()..].trim_start();
        if after.starts_with("= {") {
            mods.push(name);
        } else if after.starts_with('=') && !name.is_empty() {
            names.push(name);
        }
    }
    for m in &mods {
        let at = js.find(&format!("\nB.{} = {{", m)).unwrap();
        let end = at + js[at..].find("\n};").unwrap();
        for l in js[at..end].lines() {
            if let Some(rest) = l.strip_prefix("  ") {
                let k: String = rest.chars().take_while(|c| c.is_ascii_lowercase() || *c == '_').collect();
                if !k.is_empty() && rest[k.len()..].starts_with(':') {
                    names.push(format!("{}.{}", m, k));
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// The sweep's program: `sweep_plain()` and `sweep_css()`, each a line a builtin.
fn sweep_program() -> String {
    let form = |t: &str, css: bool| {
        let mut out = String::new();
        let mut rest = t;
        while let Some(at) = rest.find('«') {
            out.push_str(&rest[..at]);
            let after = &rest[at + '«'.len_utf8()..];
            let end = after.find('»').unwrap();
            let x = &after[..end];
            if css {
                out.push_str(&format!("css`{}`", x));
            } else {
                out.push_str(&format!("\"{}\"", x.replace('"', "\\\"")));
            }
            rest = &after[end + '»'.len_utf8()..];
        }
        out.push_str(rest);
        out
    };
    let rows: Vec<(&str, &str)> = SWEEP.lines().map(|l| l.split_once('|').unwrap()).collect();
    let f = |name: &str, css: bool| {
        let parts: Vec<String> = rows.iter().map(|(n, t)| format!("    `{}: ${{str([{}])}}`", n, form(t, css))).collect();
        format!("fn {}() = join([\n{},\n], \"\\n\")\n", name, parts.join(",\n"))
    };
    f("sweep_plain", false) + &f("sweep_css", true)
}

/// Every builtin of the browser's takes trusted CSS as the text it is, given whole, in a list or in
/// a map: the same answer as for that text written plain, on the server and in the browser, and
/// the two sides the same. A builtin the runtime defines that the sweep does not call, nor says
/// why not, fails here (FEEDBACK79.md, Q-1250, found in the review of the trusted CSS; branch
/// feedback72-81).
#[test]
fn every_browser_builtin_reads_trusted_css_as_text() {
    let swept: Vec<&str> = SWEEP.lines().map(|l| l.split_once('|').unwrap().0).collect();
    let missing: Vec<String> = browser_builtins()
        .into_iter()
        .filter(|n| !swept.contains(&n.as_str()) && !NOT_SWEPT.iter().any(|(k, _)| if k.ends_with('.') { n.starts_with(k) && n != "vec.column" } else { n == k }))
        .collect();
    assert!(missing.is_empty(), "builtins neither swept nor said why not: {:?}", missing);
    let src = format!(
        r#"{}component S() {{
    state shown = false
    <div>
        <button id="t" onclick={{() => {{ shown = true }}}}>t</button>
        {{shown ? <div><pre id="bp">{{sweep_plain()}}</pre><pre id="bc">{{sweep_css()}}</pre></div> : null}}
    </div>
}}
route GET / = <html><body><pre id="sp">{{sweep_plain()}}</pre><pre id="sc">{{sweep_css()}}</pre><S/></body></html>
"#,
        sweep_program()
    );
    let Some(v) = in_page(
        "sweep",
        &src,
        "/",
        r#"const doc = new DOMParser().parseFromString(await (await fetch(location.href)).text(), "text/html");
  $("t").click();
  await tick(150);
  return { sp: doc.getElementById("sp").textContent, sc: doc.getElementById("sc").textContent, bp: $("bp") && $("bp").textContent, bc: $("bc") && $("bc").textContent };"#,
    ) else {
        return;
    };
    // What the browser stopped at, first.
    assert_eq!(v["errors"], serde_json::json!([]), "{}", v);
    let lines = |k: &str| v[k].as_str().unwrap_or("").lines().map(str::to_string).collect::<Vec<_>>();
    let (sp, sc, bp, bc) = (lines("sp"), lines("sc"), lines("bp"), lines("bc"));
    assert_eq!(sp.len(), swept.len(), "{}", v);
    let mut differ = Vec::new();
    for (i, want) in sp.iter().enumerate() {
        for (side, got) in [("server, trusted", &sc), ("browser, plain", &bp), ("browser, trusted", &bc)] {
            if got.get(i) != Some(want) {
                differ.push(format!("{} ({}): {:?}, where the server's plain text gives {:?}", swept[i], side, got.get(i), want));
            }
        }
    }
    assert!(differ.is_empty(), "{}", differ.join("\n"));
}

/// Node, for a compiled script (`quillo::client::test_script`): what it prints, or None without node.
fn in_node(js: &str) -> Option<String> {
    if !Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
        return None;
    }
    let d = support::temp_dir("f7281-css-node");
    std::fs::write(d.join("t.cjs"), js).unwrap();
    let mut c = Command::new("node");
    c.arg(d.join("t.cjs")).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    let st = wait_for(&mut child, Duration::from_secs(30)).expect("node did not end");
    let mut out = String::new();
    child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    assert!(st.success(), "{}", out);
    let _ = std::fs::remove_dir_all(&d);
    Some(out.trim().to_string())
}

/// A count given to t() as css`...` text is a count on both sides: the server read it as its text,
/// the browser took the Css object for no number and chose `other` (found in the docs pass of the
/// trusted CSS; FEEDBACK79.md, Q-1250; branch feedback72-81). Plain text and numbers as before.
#[test]
fn a_css_count_in_t_is_a_count_on_both_sides() {
    let src = "translations({ \"en\": { k: { one: \"one\", other: \"other\" } } })\nfn __t() = [css`1`, css`2`, css` 1 `, \"1\", 1, 2] | map((c) => t(\"k\", { count: c }))\n";
    let mut it = quillo::Interp::new(".");
    it.load_source(src, "case.qwe").unwrap_or_else(|e| panic!("{}", e));
    let f = quillo::value::lookup(&it.globals, "__t").unwrap();
    let server: serde_json::Value = serde_json::from_str(&quillo::value::js_numbers(true, || it.call(&f, vec![], 0)).unwrap_or_else(|_| panic!("__t failed")).repr()).unwrap();
    assert_eq!(server, serde_json::json!(["one", "other", "one", "one", "one", "other"]));
    let script = quillo::client::test_script(&it, "__t").unwrap();
    let i18n = it.site.i18n.clone().unwrap();
    let words = i18n.browser_json(&i18n.best("en").unwrap(), None);
    let Some(out) = in_node(&format!("globalThis.window = {{ [Symbol.for(\"quillo\")]: {{ i18n: {} }} }};\n{}", words, script)) else { return };
    let browser: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{}: {}", e, out));
    assert_eq!(browser, server);
}
