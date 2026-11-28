//! Coverage of feedback72-81 (branch feedback72-81): the lines of src/parser.rs, src/lexer.rs and
//! src/record.rs added since 09e3f19 that no test ran at 85ebb05. A style's walk of its templates
//! (FEEDBACK79.md, Q-1250: a match guard, a block giving a value in `${...}`, a block's bare
//! `return` and a page object's field set, a `reply`), `state` in a component's block (FEEDBACK81.md,
//! Q-1271), HTML's void names holding text in an XML answer (FEEDBACK79.md, Q-1259), `///` after a
//! name (FEEDBACK72.md, Q-1016) and a request's text field where a decimal or text goes (record.rs's
//! `same`). Programs run by this build's `quillo` under a deadline, or in this process; each
//! server is this test's own, stopped by it.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

// ---- helpers ------------------------------------------------------------------------------

/// A folder of its own holding `main.qwe`, removed when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Dir {
    fn new(name: &str, src: &str) -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = support::temp_dir(&format!("cov7281-parse-{}-{}", name, N.fetch_add(1, Ordering::SeqCst)));
        std::fs::write(d.join("main.qwe"), src).unwrap();
        Dir(d)
    }
}

/// The process's end within `within`, or None (it is then killed): a guard against a hang only,
/// far past what a slow debug build takes.
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

/// `quillo <cmd> main.qwe` on `src` in a folder of its own, run there (so messages name
/// `main.qwe:N`): whether it succeeded, and what it printed (stdout, then stderr), read as it goes.
fn quillo_on(cmd: &str, src: &str) -> (bool, String) {
    let d = Dir::new(cmd, src);
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo")).arg(cmd).arg("main.qwe").current_dir(&d.0).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
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
    let status = wait_for(&mut c, Duration::from_secs(600)).unwrap_or_else(|| panic!("quillo {} never ended:\n{}", cmd, src));
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

/// A program loaded in this process, having passed `quillo check`.
fn app(src: &str) -> Interp {
    let (ok, out) = quillo_on("check", src);
    assert!(ok, "quillo check:\n{}\n{}", out, src);
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "main.qwe") {
        panic!("{}", e);
    }
    it
}

/// A request handled in this process.
fn ask(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "localhost:8080".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: h, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
}

fn get(it: &mut Interp, url: &str) -> (u16, String) {
    let r = ask(it, "GET", url, &[], "");
    (r.status, r.text())
}

/// What a visitor sends, and what a style then says of it: one value.
const SENT: &str = "red; background: blue";
const ONE: &str = r"red\3b  background: blue";

// ---- FEEDBACK79.md, Q-1250: every template in a style's expression is style text ---------------

/// A match guard in a style's expression decides as the same guard outside a style does: what
/// decides is no style text, so a guard comparing `${v}` with the visitor's text takes its arm, and
/// that arm's own template is still one value; with no guard taken, the arm's template too.
/// (src/parser.rs `style_values`, a match arm's guard; FEEDBACK79.md Q-1250; coverage of
/// feedback72-81; branch feedback72-81)
///
/// Changed with the fix of what this test found (FEEDBACK79.md, Q-1250, found by the coverage
/// tests of feedback72-81): at 85ebb05 the walk took the guard's template for style text too, so
/// the guard saw the text escaped and the next arm was taken inside the style (`color: ...`) where
/// the same match outside one took the first; now both take the first, as written.
#[test]
fn a_match_guard_in_a_style_decides_as_outside_one() {
    let out = run(r#"let v = trim(" red; background: blue ")
let k = 2
print(<p style={match k {
    n if `${v}` == "red; background: blue" => `top: 0; color: ${v}`
    n if n > 1 => `color: ${v}`
    _ => "left: 0"
}}>guard</p>)
print(<p style={match k {
    n if n > 5 => "top: 0"
    _ => `color: ${v}`
}}>noguard</p>)
let outside = match k {
    n if `${v}` == "red; background: blue" => "first"
    _ => "second"
}
print(outside)
"#);
    for want in [format!("<p style=\"top: 0; color: {}\">guard</p>", ONE), format!("<p style=\"color: {}\">noguard</p>", ONE), "first".to_string()] {
        assert!(out.contains(&want), "{}\n{}", want, out);
    }
    assert!(!out.contains("left: 0") && !out.contains(SENT), "{}", out);
}

/// What decides in a style's expression is no style text, wherever it stands: an `if`'s and a
/// `? :`'s condition, a `while`'s, a `match`'s subject, a comparison's sides inside `&&`, `!`'s, a
/// list or a template compared whole; each decides as it does outside a style (`out:` lines), and
/// the values it chooses between stay one value each. Unicode, and the text on either side of the
/// one compared (one character more, one less), decide as written too. (src/parser.rs
/// `style_values`, a decision; FEEDBACK79.md Q-1250, found by the coverage tests of feedback72-81;
/// branch feedback72-81)
#[test]
fn what_decides_in_a_style_decides_as_outside_one() {
    let out = run(r#"let v = trim(" red; background: blue ")
let u = "rouge; fond: été"
print(<p style={`${v}` == "red; background: blue" ? "top: 0" : `color: ${v}`}>ternary</p>)
print(<p style={if `${u}` == "rouge; fond: été" { `left: 1px; color: ${u}` } else { "x: 1" }}>if</p>)
print(<p style={match `${v}` {
    "red; background: blue" => "top: 1px"
    _ => `color: ${v}`
}}>subject</p>)
print(<p style={(`${v}` == "red; background: blue") && "top: 2px"}>and</p>)
print(<p style={!(`${v}` != "red; background: blue") ? "top: 3px" : "x: 0"}>not</p>)
print(<p style={[`${v}`] == ["red; background: blue"] ? "top: 4px" : "x: 0"}>list</p>)
print(<p style={`${v}` == "red; background: blu" ? "x: 0" : `color: ${v}`}>less</p>)
print(<p style={`${v}` == "red; background: bluee" ? "x: 0" : `color: ${v}`}>more</p>)
print(<p style={{
    let s = ""
    let n = 0
    while `${v}` == "red; background: blue" && n < 2 {
        s = s + `color: ${v};`
        n = n + 1
    }
    s
}}>while</p>)
print(`out: ${`${v}` == "red; background: blue"} ${`${u}` == "rouge; fond: été"} ${[`${v}`] == ["red; background: blue"]}`)
"#);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        [
            "<p style=\"top: 0\">ternary</p>".to_string(),
            "<p style=\"left: 1px; color: rouge\\3b  fond: été\">if</p>".to_string(),
            "<p style=\"top: 1px\">subject</p>".to_string(),
            "<p style=\"top: 2px\">and</p>".to_string(),
            "<p style=\"top: 3px\">not</p>".to_string(),
            "<p style=\"top: 4px\">list</p>".to_string(),
            format!("<p style=\"color: {}\">less</p>", ONE),
            format!("<p style=\"color: {}\">more</p>", ONE),
            format!("<p style=\"color: {};color: {};\">while</p>", ONE, ONE),
            "out: true true true".to_string(),
        ],
        "{}",
        out
    );
}

/// What a decision hands on is still a style's: a call's arguments in a condition or a guard (a
/// function the style's block made keeps what it is given, for a name the style then gives), a
/// block in a condition and a lambda called in a guard (each setting a name the style gives), and
/// a lambda's body in a `match`'s subject; each value stays one value, as the same text given
/// directly is. (src/parser.rs `style_values`, a decision's calls, blocks and lambdas; FEEDBACK79.md
/// Q-1250, found by the coverage tests of feedback72-81; branch feedback72-81)
#[test]
fn what_a_decision_hands_on_in_a_style_is_still_escaped() {
    let out = run(r#"let v = trim(" red; background: blue ")
print(<p style={{
    let s = ""
    let keep = (x) => {
        s = x
        return true
    }
    keep(`color: ${v}`) ? s : ""
}}>call</p>)
print(<p style={{
    let s = ""
    let keep = (x) => {
        s = x
        return true
    }
    match 1 {
        n if keep(`color: ${v}`) => s
        _ => ""
    }
}}>guardcall</p>)
print(<p style={{
    let s = ""
    {
        s = `color: ${v}`
        true
    } ? s : ""
}}>block</p>)
print(<p style={{
    let s = ""
    match 1 {
        n if ((x) => {
            s = `color: ${v}`
            return true
        })(n) => s
        _ => ""
    }
}}>lambda</p>)
print(<p style={{
    let s = ""
    match ((x) => `color: ${x}`)(v) {
        t => t
    }
}}>subjectlambda</p>)
"#);
    for t in ["call", "guardcall", "block", "lambda", "subjectlambda"] {
        let want = format!("<p style=\"color: {}\">{}</p>", ONE, t);
        assert!(out.contains(&want), "{}\n{}", want, out);
    }
    assert!(!out.contains(SENT), "{}", out);
}

/// A block giving a value in a style template's `${...}` is looked into as a branch is, when its
/// value may hold the program's own text: a string with `;` it gives is kept as written, the
/// visitor's value it gives is escaped; a block whose value holds no such text is escaped whole;
/// a block inside a branch likewise. Unicode stays. (src/parser.rs `css_leaves` and `keeps_text`,
/// a block's last expression; FEEDBACK79.md Q-1250; coverage of feedback72-81; branch
/// feedback72-81)
#[test]
fn a_block_giving_a_value_in_a_style_template_is_looked_into() {
    let out = run(r#"let v = trim(" red; background: blue ")
let u = "rouge; fond: été"
let big = true
let small = false
print(<p style={`color: ${{
    let c = v
    big ? "x: 1;" : c
}}`}>keeps</p>)
print(<p style={`color: ${{
    let c = v
    small ? "x: 1;" : c
}}`}>chosen</p>)
print(<p style={`color: ${{
    let c = v
    c
}}`}>whole</p>)
print(<p style={`color: ${small ? "x" : {
    let c = u
    big ? "y: 2;" : c
}}`}>inbranch</p>)
print(<p style={`color: ${big ? {
    let c = u
    small ? "y: 2;" : c
} : "x"}`}>unicode</p>)
"#);
    for want in [
        "<p style=\"color: x: 1;\">keeps</p>".to_string(),
        format!("<p style=\"color: {}\">chosen</p>", ONE),
        format!("<p style=\"color: {}\">whole</p>", ONE),
        "<p style=\"color: y: 2;\">inbranch</p>".to_string(),
        "<p style=\"color: rouge\\3b  fond: été\">unicode</p>".to_string(),
    ] {
        assert!(out.contains(&want), "{}\n{}", want, out);
    }
    assert!(!out.contains(SENT), "{}", out);
}

/// A bare `return` in a style's block (a view leaving early) is passed over by the walk, and the
/// block's template after it is still style text; the view that returned shows nothing, here and
/// in a lambda's body in the style. (src/parser.rs `style_values`, a block's other statements;
/// FEEDBACK79.md Q-1250; coverage of feedback72-81; branch feedback72-81)
#[test]
fn a_bare_return_in_a_style_block_leaves_the_rest_escaped() {
    let out = run(r#"let v = trim(" red; background: blue ")
fn Swatch(c, hide: bool) {
    <p style={{
        if hide {
            return
        }
        `color: ${c}`
    }}>swatch</p>
}
print(Swatch(v, false))
print(Swatch(v, true))
print(<p style={["a", ""].map((x) => {
    if x == "" {
        return ""
    }
    `color: ${v}`
}).join("")}>lambda</p>)
"#);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, [format!("<p style=\"color: {}\">swatch</p>", ONE).as_str(), "null", format!("<p style=\"color: {}\">lambda</p>", ONE).as_str()], "{}", out);
}

/// A page object's field set in a style's block (a handler made there, `el.title = ...`) is
/// style text as the rest: the page's JavaScript escapes the value it sets, as the server escapes
/// the style it writes. (src/parser.rs `style_values`, a block's `SetField`; FEEDBACK79.md
/// Q-1250; coverage of feedback72-81; branch feedback72-81)
#[test]
fn a_field_set_in_a_style_block_is_style_text_in_the_page_too() {
    let mut it = app(r#"component C() {
    state v = "red; background: blue"
    state n = 0
    <div>
        <p id="p" style={{
            let paint = (el) => {
                el.title = `color: ${v}`
            }
            n > 0 ? paint : null
            `color: ${v}`
        }}>a</p>
        <button onclick={() => { n += 1 }}>b</button>
    </div>
}
route GET / = <html><body><C/></body></html>
"#);
    let (status, page) = get(&mut it, "/");
    assert_eq!(status, 200, "{}", page);
    assert!(page.contains(&format!("<p id=\"p\" style=\"color: {}\">a</p>", ONE)), "{}", page);
    let bundle = page.split('"').find(|p| p.starts_with("/_quillo/b/")).unwrap_or_else(|| panic!("no bundle in {}", page)).to_string();
    let (status, js) = get(&mut it, &bundle);
    assert_eq!(status, 200);
    let at = js.find("W.setf(").unwrap_or_else(|| panic!("no field set in the bundle: {}", js));
    let set = &js[at..js[at..].find(';').map_or(js.len(), |e| at + e)];
    assert!(set.contains("\"title\",(\"color: \"+W.display(B.__css_value(") || set.contains("\"title\", (\"color: \" + W.display(B.__css_value("), "{}", set);
}

/// A `reply` in a style's expression sends its body as written, as the same reply before the
/// markup does: it ends the request and never reaches the style. In a `? :`'s branch, in a style
/// template's `${...}`, with unicode and a value one character long, and with no body; a short
/// value is the style's, one value. (src/parser.rs `style_values`, `Expr::Reply`; FEEDBACK79.md
/// Q-1250; coverage of feedback72-81; branch feedback72-81)
///
/// Changed with the fix of what this test found (FEEDBACK79.md, Q-1250, found by the coverage
/// tests of feedback72-81): at 85ebb05 the walk took a reply's body for style text, so `/c` answered
/// `no such colour: red\3b  background: blue`, the visitor's text escaped as CSS in a text answer,
/// where `/plain` answers it as written; now `/c` answers as `/plain` does.
#[test]
fn a_reply_in_a_style_expression_sends_its_body_as_written() {
    let mut it = app(r#"route GET /c = {
    let v = request.query.c ?? "red"
    <p style={len(v) < 5 ? `color: ${v}` : reply 400 `no such colour: ${v}`}>a</p>
}
route GET /e = {
    let v = request.query.c ?? "red"
    <p style={len(v) < 5 ? `color: ${v}` : reply 204}>a</p>
}
route GET /t = {
    let v = request.query.c ?? "red"
    <p style={`color: ${len(v) < 5 ? v : reply 400 `no such colour: ${v}`}`}>a</p>
}
route GET /plain = {
    let v = request.query.c ?? "red"
    if len(v) >= 5 {
        reply 400 `no such colour: ${v}`
    }
    <p style={`color: ${v}`}>a</p>
}
"#);
    for (q, sent) in [("c=red%3B%20background%3A%20blue", SENT), ("c=%C3%A9t%C3%A9%3B%20x%3A%201", "été; x: 1")] {
        for path in ["/c", "/t", "/plain"] {
            assert_eq!(get(&mut it, &format!("{}?{}", path, q)), (400, format!("no such colour: {}", sent)), "{}", path);
        }
        assert_eq!(get(&mut it, &format!("/e?{}", q)), (204, String::new()));
    }
    for (q, style) in [("c=blue", "color: blue"), ("c=%3B", r"color: \3b "), ("c=", "color: ")] {
        for path in ["/c", "/e", "/t", "/plain"] {
            let (status, body) = get(&mut it, &format!("{}?{}", path, q));
            assert_eq!(status, 200, "{}?{}", path, q);
            assert!(body.contains(&format!("<p style=\"{}\">a</p>", style)), "{}?{}: {}", path, q, body);
        }
    }
}

// ---- FEEDBACK81.md, Q-1271: `state` in a component's block -------------------------------------

/// `state` as a program's name in a component's block, followed by `=` or `.`, is the name as
/// before: assigned in a match arm's block, read in an `if`'s; not taken for a declaration.
/// (src/parser.rs `state_in_block`, a `state` not followed by a name; FEEDBACK81.md Q-1271;
/// coverage of feedback72-81; branch feedback72-81)
#[test]
fn state_as_a_name_assigned_in_a_components_block() {
    let out = run(r#"component S(show: bool) {
    let state = { label: "l" }
    let shown = match show {
        true => {
            state = { label: "on" }
            state.label
        }
        _ => ""
    }
    if show {
        state.label
    }
    <p>{shown}|{state.label}</p>
}
print(<S show={true}/>)
print(<S show={false}/>)
"#);
    assert!(out.contains("<p>on|on</p>"), "{}", out);
    assert!(out.contains("<p>|l</p>"), "{}", out);
}

/// `state x = ...` in a component's plain block (a `let`'s value) is told as a block's, with the
/// first value of its kind and the line it is on. (src/parser.rs `state_in_block`, a block that
/// is none of `if`, `for`, `while` or a function; FEEDBACK81.md Q-1271; coverage of feedback72-81;
/// branch feedback72-81)
#[test]
fn state_in_a_plain_block_is_told_as_a_blocks() {
    for (stmt, first) in [
        ("let k = { state n = \"a\" }", "\"\""),
        ("let k = { state xs = [1, 2] }", "[]"),
        ("let k = { state m = { a: 1 } }", "{}"),
        ("let k = { state é = 2 }", "0"),
    ] {
        let name = stmt.split("state ").nth(1).unwrap().split(' ').next().unwrap();
        let src = format!("component S(show: bool) {{\n    let a = 1\n    {}\n    <p>hi {{a}}</p>\n}}\nprint(<S show={{true}}/>)\n", stmt);
        let e = refused(&src);
        let want = format!("main.qwe:3: error: `state` goes at the top of a component's body, not inside a block: declare `state {} = {}` there and set it in the block", name, first);
        assert!(e.contains(&want), "{}\n{}", want, e);
        assert!(!e.contains("newer Quillo"), "{}", e);
    }
}

/// `state x = ...` in a component, in a `match` arm's block (a `let`'s value) is told as a block's
/// that the `match` holds, not as a function's; in a helper fn declared in the component with a
/// body in braces, as a function's, not as a block's; with `=` and braces, and in a lambda, as a
/// function's as before; in an `if` inside an arm, as the `if`'s. Each with the first value of its
/// kind and its line; unicode names too. (src/parser.rs `arrow_body_of`, `parse_fn_rest` and
/// `state_in_block`; FEEDBACK81.md Q-1271, found by the coverage tests of feedback72-81; branch
/// feedback72-81)
#[test]
fn state_in_a_match_arm_or_a_helper_says_where_it_is() {
    let arm = |name: &str, first: &str| format!("error: `state` goes at the top of a component's body, not inside a block: declare `state {} = {}` there and set it in the `match`", name, first);
    let func = |name: &str, first: &str| format!("error: `state` goes at the top of a component's body, not inside a function: declare `state {} = {}` there and set it in the function", name, first);
    for (stmt, line, want) in [
        ("let k = match show {\n        true => { state n = 1 }\n        _ => 0\n    }", 4, arm("n", "0")),
        ("let k = match show {\n        true => {\n            state é = \"a\"\n        }\n        _ => 0\n    }", 5, arm("é", "\"\"")),
        ("fn helper() { state n = [1] }", 3, func("n", "[]")),
        ("fn helper() {\n        let a = 1\n        state m = { a: 1 }\n    }", 5, func("m", "{}")),
        ("fn helper() = { state n = 2.5 }", 3, func("n", "0.0")),
        ("let f = () => { state n = true }", 3, func("n", "false")),
        ("let k = match show {\n        true => {\n            if show { state n = 1 }\n            0\n        }\n        _ => 0\n    }", 5, "error: `state` goes at the top of a component's body, not inside an `if`: declare `state n = 0` there and set it in the `if`".to_string()),
    ] {
        let src = format!("component S(show: bool) {{\n    let a = 1\n    {}\n    <p>hi {{a}}</p>\n}}\nprint(<S show={{true}}/>)\n", stmt);
        let e = refused(&src);
        let want = format!("main.qwe:{}: {}", line, want);
        assert!(e.contains(&want), "{}\n{}\n{}", want, e, src);
    }
    // A `match` arm's block, and a helper's, with no `state` in them run as before.
    let out = run("component S(show: bool) {\n    state n = 1\n    fn helper() { return n + 1 }\n    let k = match show {\n        true => { helper() }\n        _ => 0\n    }\n    <p>{k}</p>\n}\nprint(<S show={true}/>)\n");
    assert!(out.contains("<p>2</p>"), "{}", out);
}

// ---- FEEDBACK79.md, Q-1259: HTML's void names in an XML answer ---------------------------------

/// A feed's `<link>` holding code is read up to its end tag: a string's escaped quote and its
/// brace, a template's escaped backquote, a map's braces inside the code before a `<`, and an end
/// tag with spaces or a tab before its `>`. (src/parser.rs `holds_up_to_end`; FEEDBACK79.md
/// Q-1259; coverage of feedback72-81; branch feedback72-81)
#[test]
fn a_feeds_link_holding_code_is_read_to_its_end_tag() {
    let out = run(r#"let site = { home: "http://example.com/" }
print(<rss version="2.0"><channel><link>{"a\"}<b"}</link></channel></rss>)
print(<rss version="2.0"><channel><link>{`x\`}<`}</link></channel></rss>)
print(<rss version="2.0"><channel><link>{ { n: 1 }.n < 2 ? site.home : "" }</link></channel></rss>)
print(<rss version="2.0"><channel><link>http://example.com/é</link  ></channel></rss>)
print(<rss version="2.0"><channel><link>http://example.com/t</link	></channel></rss>)
"#);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        [
            "<rss version=\"2.0\"><channel><link>a&quot;}&lt;b</link></channel></rss>",
            "<rss version=\"2.0\"><channel><link>x`}&lt;</link></channel></rss>",
            "<rss version=\"2.0\"><channel><link>http://example.com/</link></channel></rss>",
            "<rss version=\"2.0\"><channel><link>http://example.com/é</link></channel></rss>",
            "<rss version=\"2.0\"><channel><link>http://example.com/t</link></channel></rss>",
        ],
        "{}",
        out
    );
}

/// A feed whose `<link>` runs to the end of the file, in text or in code, or whose end tag is not
/// one (`</link x>`): the `<link>` is empty, as before, and what follows is told with its line.
/// (src/parser.rs `holds_up_to_end`, the end of the text and an end tag that is not one;
/// FEEDBACK79.md Q-1259; coverage of feedback72-81; branch feedback72-81)
#[test]
fn a_feeds_link_running_to_the_end_is_told() {
    let e = refused("print(<rss version=\"2.0\"><channel><link>http://example.com/");
    assert!(e.contains("main.qwe:1: error: `<channel>` is never closed: add `</channel>` at the end of the file"), "{}", e);
    let e = refused("let a = 1\nprint(<rss version=\"2.0\"><channel><link>{\"a\"");
    assert!(e.contains("main.qwe:2: error: expected `}`, found end of input"), "{}", e);
    let e = refused("let a = 1\nprint(<rss version=\"2.0\"><channel><link>http://example.com/</link x></link></channel></rss>)\n");
    assert!(e.contains("main.qwe:2: error: `</link>` does not close `<channel>` from line 2: expected `</channel>`"), "{}", e);
}

// ---- FEEDBACK72.md, Q-1016: `pages // 2` ---------------------------------------------------------

/// `///` (or `////`) after a name is no `//` whole-number division: only `// 2` is told, and the
/// program runs as written (the lexer passes over a `///` before reading its text, which, starting
/// with `/`, is neither a number nor a name either). (src/lexer.rs `note_value_comment`, a comment starting `///`; FEEDBACK72.md Q-1016; coverage of
/// feedback72-81; branch feedback72-81)
#[test]
fn a_triple_slash_after_a_name_is_no_division() {
    let src = "let pages = 9\nlet half = pages /// 2\nlet third = pages ///3\nlet q = pages //// 4\nlet d = pages // 2\nprint(half, third, q, d)\n";
    let (ok, out) = quillo_on("check", src);
    assert!(ok, "{}", out);
    let told: Vec<&str> = out.lines().filter(|l| l.contains("starts a comment here")).collect();
    assert_eq!(told, ["main.qwe:5: warning: `//` starts a comment here, so `// 2` is left out: whole-number division is `floor(a / b)`"], "{}", out);
    let (ok, ran) = quillo_on("run", src);
    assert!(ok, "{}", ran);
    assert!(ran.contains("9 9 9 9"), "{}", ran);
}

// ---- record.rs: a request's text where a decimal or a text goes --------------------------------

/// A server fn's map shape with a `dec | str` field (written, or through a type's name): text that
/// is no decimal is kept as the text sent (empty and unicode too), a decimal's text and a number
/// are made decimals, the other field as it was. (src/record.rs `same`, text given back as it
/// was; FEEDBACK75.md follow-ups; coverage of feedback72-81; branch feedback72-81)
#[test]
fn a_text_where_a_decimal_or_a_text_goes_is_kept() {
    let mut it = app(r#"type Amount = dec | str
server fn price(x: { a: dec | str, n: int }) {
    return `${type_of(x.a)} ${x.a} ${x.n}`
}
server fn named(x: { a: Amount }) {
    return `${type_of(x.a)} ${x.a}`
}
route GET / = <p>hi</p>
"#);
    let rpc = |it: &mut Interp, f: &str, args: &str| {
        let r = ask(it, "POST", &format!("/_quillo/rpc/{}", f), &[("Content-Type", "application/json"), ("Sec-Fetch-Site", "same-origin")], args);
        (r.status, r.text())
    };
    for (args, want) in [
        (r#"[{"a":"abc","n":1}]"#, r#"{"ok":"str abc 1"}"#),
        (r#"[{"a":"1.50","n":2}]"#, r#"{"ok":"dec 1.50 2"}"#),
        (r#"[{"a":"été 😀","n":3}]"#, r#"{"ok":"str été 😀 3"}"#),
        (r#"[{"a":"","n":4}]"#, r#"{"ok":"str  4"}"#),
        (r#"[{"a":2,"n":5}]"#, r#"{"ok":"dec 2 5"}"#),
    ] {
        assert_eq!(rpc(&mut it, "price", args), (200, want.to_string()), "{}", args);
    }
    assert_eq!(rpc(&mut it, "named", r#"[{"a":"abc"}]"#), (200, r#"{"ok":"str abc"}"#.to_string()));
    let (status, body) = rpc(&mut it, "price", r#"[{"a":true,"n":1}]"#);
    assert_eq!(status, 400, "{}", body);
}
