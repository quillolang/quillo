// FEEDBACK54.md: real hot module replacement under --watch (branch hmr). Q-688 first: what did
// not change keeps its file and its address.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("f54-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

/// The development files a page loads, by path: (address, text).
fn files(port: u16) -> Vec<(String, String, String)> {
    let page = support::send(port, "GET", "/", &[], "").body;
    let b = page.split("/_quillo/b/").nth(1).and_then(|r| r.split('"').next()).unwrap_or_else(|| panic!("{}", page));
    let loader = support::send(port, "GET", &format!("/_quillo/b/{}", b), &[], "").body;
    let at = loader.split("const at = \"").nth(1).and_then(|r| r.split('"').next()).unwrap_or_else(|| panic!("{}", loader));
    let list = loader.split("for (const f of [").nth(1).and_then(|r| r.split(']').next()).unwrap();
    let named: Vec<String> = serde_json::from_str(&format!("[{}]", list)).unwrap();
    named
        .into_iter()
        .map(|f| {
            let url = format!("{}{}", at, f);
            let a = support::send(port, "GET", &url, &[], "");
            assert_eq!(a.status, 200, "{}", url);
            assert_eq!(a.header("cache-control"), Some("public, max-age=31536000, immutable"), "{}", url);
            let path = f.split_once('/').unwrap().1.to_string();
            (path, url, a.body)
        })
        .collect()
}

fn of<'a>(fs: &'a [(String, String, String)], path: &str) -> &'a (String, String, String) {
    fs.iter().find(|f| f.0 == path).unwrap_or_else(|| panic!("no {} in {:?}", path, fs.iter().map(|f| &f.0).collect::<Vec<_>>()))
}

/// Saves `src` and waits for the server to have loaded it (its page says `mark`).
fn save(port: u16, d: &Path, src: &str, mark: &str) {
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    while !support::send(port, "GET", "/", &[], "").body.contains(mark) {
        assert!(Instant::now() < end, "the save was not loaded");
        std::thread::sleep(Duration::from_millis(50));
    }
}

const TWO: &str = "component First() {\n    state a = 0\n    <p.first>{a} one</p>\n}\ncomponent Second() {\n    state b = 1\n    <div.second><b>{b}</b></div>\n}\ncomponent Third() {\n    state c = 2\n    <div.second><b>{c}</b></div>\n}\nroute GET / = <html><body><First/><Second/><Third/></body></html>\n";

// Q-688: blocks were numbered across the program and one hash named every file, so an edit to
// one component (here a block and a variable more, on its own lines) changed every later file
// and every address. The runtime's file, and the other components', now stay as they were.
#[test]
fn an_edit_to_one_component_leaves_the_others_files_and_addresses_alone() {
    let d = dir("one-edit", TWO);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let before = files(s.port);
    assert_eq!(before.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["quillo.js", "main.qwe/First.js", "main.qwe/Second.js", "main.qwe/Third.js", "boot.js"]);
    // Second's and Third's blocks are the same markup: each file declares its own copy, by one name.
    let block = |t: &str| t.lines().find(|l| l.starts_with("const $$b_")).map(|l| l.split(' ').nth(1).unwrap().to_string());
    assert_eq!(block(&of(&before, "main.qwe/Second.js").2), block(&of(&before, "main.qwe/Third.js").2));
    assert!(!of(&before, "quillo.js").2.contains("W.block("), "no block in the runtime's file");
    let edited = TWO.replace("    <p.first>{a} one</p>", "    let z = a + 1\n    <p.first><i>{a}</i>\n        {a > 0 && <b>y</b>} {z} two</p>");
    save(s.port, &d, &edited, "two");
    let after = files(s.port);
    for path in ["quillo.js", "boot.js"] {
        assert_eq!(of(&before, path).1, of(&after, path).1, "{} moved", path);
    }
    // Below the edit, a component's lines moved: its map says so, so its file is new; above it
    // nothing moved. The one that changed has a new address.
    assert_ne!(of(&before, "main.qwe/First.js").1, of(&after, "main.qwe/First.js").1);
    let unchanged = |t: &str| t.split("PLACES[").next().unwrap().to_string();
    for path in ["main.qwe/Second.js", "main.qwe/Third.js"] {
        assert_eq!(unchanged(&of(&before, path).2), unchanged(&of(&after, path).2), "{}'s code changed", path);
    }
}

// Q-688's edges: an edit on the same lines leaves every other file's address as it was; the old
// address of the changed file is still served (a page that has not reloaded yet), and an address
// with the wrong path or hash is not.
#[test]
fn an_edit_on_its_own_lines_keeps_every_other_address() {
    let d = dir("same-lines", TWO);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let before = files(s.port);
    save(s.port, &d, &TWO.replace("{a} one", "{a} uno"), "uno");
    let after = files(s.port);
    for (path, url, _) in &before {
        if path == "main.qwe/First.js" {
            assert_ne!(url, &of(&after, path).1);
            assert_eq!(support::send(s.port, "GET", url, &[], "").status, 200, "the old address is still served");
        } else {
            assert_eq!(url, &of(&after, path).1, "{} moved", path);
        }
    }
    let first = &of(&after, "main.qwe/First.js").1;
    assert_eq!(support::send(s.port, "GET", &first.replace("First.js", "Second.js"), &[], "").status, 404);
    assert_eq!(support::send(s.port, "GET", "/_quillo/src/0000000000000000/main.qwe/First.js", &[], "").status, 404);
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

/// One Chrome test at a time, for all its length: a test waiting on its page's stage while another
/// test's Chrome runs timed out.
fn chrome_turn() -> std::sync::MutexGuard<'static, ()> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TURN.lock().unwrap_or_else(|e| e.into_inner())
}

/// `url` in headless Chrome, with `script` run in the page: what it stored in `data-result`.
fn in_chrome(chrome: &Path, url: &str, script: &str) -> serde_json::Value {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let d = support::temp_dir(&format!("f54-chrome-{}", script.len()));
    std::fs::write(d.join("t.js"), script).unwrap();
    let out = std::process::Command::new("node").arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(d.join("profile")).arg("30000").arg(d.join("t.js")).output().expect("node");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

const COUNTER: &str = "let clicked = false\nroute GET /clicked {\n    clicked = true\n    \"ok\"\n}\nroute GET /was-clicked = clicked\ncomponent Count() {\n    state n = 0\n    <button id=\"b\" onclick={() => { n += 1 }}>{n}</button>\n}\nroute GET / = <html><body><div><Count/></div></body></html>\n";

// Q-686: a save that did not load said nothing on the page, which went on showing the version
// before it. The open page now says so, over itself, and keeps what it holds (no reload); a page
// opened while it stands says so too; the save that loads reloads them, as before.
#[test]
fn a_save_that_does_not_load_is_said_on_the_page_which_keeps_its_state() {
    let d = dir("failed-save", COUNTER);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let version = support::send(s.port, "GET", "/_quillo/events", &[], "").body;
    // A plain request is told too, after the version it has.
    let broken = COUNTER.replace("{n}</button>", "{nope}</button>");
    let Some(chrome) = chrome() else {
        std::fs::write(d.join("main.qwe"), &broken).unwrap();
        let said = support::send(s.port, "GET", &format!("/_quillo/events?v={}&e=0", version), &[], "").body;
        assert!(said.starts_with("err:1:") && said.contains("nope"), "{}", said);
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let port = s.port;
    let page = std::thread::spawn(move || {
        let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const b = document.getElementById("b");
  for (let i = 0; i < 3; i++) { b.click(); await tick(30); }
  window.__kept = 1;
  await fetch("/clicked");
  let box = null;
  for (let i = 0; i < 600 && !box; i++) { await tick(30); box = document.querySelector("quillo-error"); }
  const text = box ? box.shadowRoot.querySelector("pre").textContent : null;
  document.body.dataset.result = JSON.stringify({ text, n: document.getElementById("b").textContent, kept: window.__kept === 1, errors: window.__quilloErrors });
})();"#;
        in_chrome(&chrome, &format!("http://127.0.0.1:{}/", port), script)
    });
    let end = Instant::now() + Duration::from_secs(30);
    while support::send(s.port, "GET", "/was-clicked", &[], "").body != "true" {
        assert!(Instant::now() < end && !page.is_finished(), "the page did not click");
        std::thread::sleep(Duration::from_millis(30));
    }
    std::fs::write(d.join("main.qwe"), &broken).unwrap();
    let r = page.join().unwrap();
    assert!(r["text"].as_str().is_some_and(|t| t.contains("nope")), "{}", r);
    assert_eq!((r["n"].as_str(), r["kept"].as_bool()), (Some("3"), Some(true)), "the page was reloaded: {}", r);
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);
    // Opened while the save stands: the version before, and the overlay.
    let chrome = self::chrome().unwrap();
    let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  let box = null;
  for (let i = 0; i < 300 && !box; i++) { await tick(30); box = document.querySelector("quillo-error"); }
  document.body.dataset.result = JSON.stringify({ shown: !!box, n: document.getElementById("b").textContent });
})();"#;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), script);
    assert_eq!((r["shown"].as_bool(), r["n"].as_str()), (Some(true), Some("0")), "{}", r);
    // The save that loads: pages are told the new version (and reload).
    std::fs::write(d.join("main.qwe"), COUNTER.replace("{n}</button>", "{n}!</button>")).unwrap();
    let said = support::send(s.port, "GET", &format!("/_quillo/events?v={}&e=1", version), &[], "").body;
    assert!(said.parse::<u64>().is_ok() && said != version, "{}", said);
    assert!(support::send(s.port, "GET", "/", &[], "").body.contains("0!</button>"));
}

/// `quillo run` with no `QUILLO_SECRET`, on a port of its own: the server, and what it said
/// until it listened.
struct Bare(std::process::Child);

impl Drop for Bare {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn bare(d: &Path, port: u16, opts: &[&str]) -> (Bare, String) {
    use std::io::BufRead;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", &port.to_string()])
        .args(opts)
        .env_remove("QUILLO_SECRET")
        .current_dir(d)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let err = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let mut said = String::new();
    let end = Instant::now() + Duration::from_secs(20);
    while !said.contains("listening on") {
        assert!(Instant::now() < end, "the server did not start: {}", said);
        if let Ok(l) = rx.recv_timeout(Duration::from_millis(50)) {
            said.push_str(&l);
            said.push('\n');
        }
    }
    (Bare(child), said)
}

const VISITS: &str = "route GET / {\n    let n = (session().n ?? 0) + 1\n    set_session({ n })\n    str(n)\n}\n";

/// GET / with `cookie`: the body, and the cookie the answer set (or the one given).
fn visit(port: u16, cookie: &str) -> (String, String) {
    let a = support::send(port, "GET", "/", &[("cookie", cookie)], "");
    let set = a.headers.iter().find(|(k, _)| k == "set-cookie").map(|(_, v)| v.split(';').next().unwrap().to_string());
    (a.body, set.unwrap_or_else(|| cookie.to_string()))
}

// Q-689: with no QUILLO_SECRET each start made a random key, so a restart under --watch (a new
// binary, a changed .env) signed the developer out. Under --watch the key is now kept in
// .quillo/.quillo-secret, its owner's alone, and said once; a session goes on across a restart.
// Without --watch, a random key as before, and no file; with QUILLO_SECRET, that.
#[test]
fn a_restart_under_watch_keeps_the_session() {
    let d = dir("restart-session", VISITS);
    let port = support::free_port();
    let (s, said) = bare(&d, port, &["--watch"]);
    assert!(said.contains("sessions are signed with .quillo/.quillo-secret"), "{}", said);
    let (one, cookie) = visit(port, "");
    let (two, cookie) = visit(port, &cookie);
    assert_eq!((one.as_str(), two.as_str()), ("1", "2"));
    drop(s);
    let key = d.join(".quillo/.quillo-secret");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&key).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let (_s, _) = bare(&d, port, &["--watch"]);
    assert_eq!(visit(port, &cookie).0, "3", "signed out by the restart");
    drop(_s);

    // Without --watch: no file made, none used.
    let d2 = dir("restart-plain", VISITS);
    let (s, said) = bare(&d2, port, &[]);
    assert!(!said.contains(".quillo-secret"), "{}", said);
    let (_, cookie) = visit(port, "");
    drop(s);
    assert!(!d2.join(".quillo/.quillo-secret").exists());
    let (_s, _) = bare(&d2, port, &[]);
    assert_eq!(visit(port, &cookie).0, "1");
}

/// What a reloaded page says, from its own script: the first page leaves what it saw in
/// `sessionStorage`, and the page that replaces it writes it out.
// Once the runtime is up: the reload kept the page's state (A.6), which it puts back then.
const AFTER: &str = "if (sessionStorage.f54) (async () => { for (let i = 0; i < 300 && !(window[Symbol.for(\"quillo\")] || {}).booted; i++) await new Promise((r) => setTimeout(r, 30)); document.body.dataset.result = JSON.stringify({ reloaded: true, first: JSON.parse(sessionStorage.f54), n: document.getElementById(\"b\").textContent, hmr: JSON.parse(sessionStorage.f54hmr || \"[]\") }); })();\n";

fn hot_program(other: &str, label: &str) -> String {
    format!("let stage = 0\nroute GET /stage/:n(int) {{\n    stage = n\n    \"ok\"\n}}\nroute GET /stage = stage\nroute GET /other = \"{}\"\nroute GET /after.js = file(\"after.js\")\ncomponent Count() {{\n    state n = 0\n    <button id=\"b\" onclick={{() => {{ n += 1 }}}}>{}{{n}}</button>\n}}\nroute GET / = <html><body><div><Count/></div><script src=\"/after.js\"></script></body></html>\n", other, label)
}

fn stage(port: u16, n: u64, page: &std::thread::JoinHandle<serde_json::Value>) {
    let end = Instant::now() + Duration::from_secs(30);
    while support::send(port, "GET", "/stage", &[], "").body != n.to_string() {
        assert!(Instant::now() < end && !page.is_finished(), "the page did not reach stage {}", n);
        std::thread::sleep(Duration::from_millis(30));
    }
}

// Q-685, step 1: every save reloaded every open page, and the developer lost what it held. A
// page told of a new version now asks for itself again: when its HTML and its code are as they
// were (here a route it never calls changed), it stays as it is; when its code changed, it
// reloads, as before. --watch=reload reloads it at every change.
#[test]
fn a_change_the_page_does_not_use_leaves_it_as_it_is() {
    let d = dir("hot-unused", &hot_program("a", "n="));
    std::fs::write(d.join("after.js"), AFTER).unwrap();
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let page = support::send(s.port, "GET", "/", &[], "").body;
    assert!(page.contains("/*quillo-dev*/") && page.contains("HOT=1"), "{}", page);
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let port = s.port;
    let page = std::thread::spawn(move || {
        let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const b = document.getElementById("b");
  for (let i = 0; i < 3; i++) { b.click(); await tick(30); }
  window.__kept = 1;
  // What the page decided, as tools hear it (`quillo:hmr`), kept across the reload.
  const said = [];
  document.addEventListener("quillo:hmr", (e) => { const d = JSON.parse(e.detail); said.push([d.what, d.why]); sessionStorage.f54hmr = JSON.stringify(said); });
  await fetch("/stage/1");
  // The save to /other lands; the page asks for itself and stays.
  for (let i = 0; i < 300 && (await (await fetch("/other")).text()) !== "b"; i++) await tick(30);
  await tick(1500);
  sessionStorage.f54 = JSON.stringify({ kept: window.__kept === 1, n: b.textContent, errors: window.__quilloErrors });
  await fetch("/stage/2");
})();"#;
        in_chrome(&chrome, &format!("http://127.0.0.1:{}/", port), script)
    });
    stage(s.port, 1, &page);
    std::fs::write(d.join("main.qwe"), hot_program("b", "n=")).unwrap();
    stage(s.port, 2, &page);
    // The page's own HTML, outside its islands, reloads it (changed with A.2: a component's label
    // is swapped in place now).
    std::fs::write(d.join("main.qwe"), hot_program("b", "n=").replace("<script src=", "<p>more</p><script src=")).unwrap();
    let r = page.join().unwrap();
    assert_eq!(r["first"], serde_json::json!({ "kept": true, "n": "n=3", "errors": [] }), "{}", r);
    assert_eq!((r["reloaded"].as_bool(), r["n"].as_str()), (Some(true), Some("n=3")), "{}", r);
    assert_eq!(r["hmr"], serde_json::json!([["kept", ""], ["reload", "html"]]), "{}", r);
}

// Q-685's edges, on the server's side: the page's print leaves out its bundle's address (which
// moves with its code) and nothing else; --watch=reload turns the page's check off.
#[test]
fn a_pages_print_follows_its_html_and_watch_reload_turns_it_off() {
    let d = dir("hot-print", &hot_program("a", "n="));
    std::fs::write(d.join("after.js"), AFTER).unwrap();
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    // The print as the dev script holds it, `let v='N',h='...'` (a let since A.3: a morphed page
    // takes the new one; the test changed for that).
    let print = |port: u16| {
        let p = support::send(port, "GET", "/", &[], "").body;
        p.split(",h='").nth(1).unwrap().split('\'').next().unwrap().to_string()
    };
    let first = print(s.port);
    assert_eq!(print(s.port), first, "the same page, the same print");
    save(s.port, &d, &hot_program("b", "n="), "n=");
    assert_eq!(print(s.port), first, "a route the page does not call");
    // A component's markup is its code's (compared by the bundle's ids, A.2); what is outside
    // the islands is the page's (changed with A.2: the edit was a component's label).
    save(s.port, &d, &hot_program("b", "count "), "count ");
    assert_eq!(print(s.port), first, "a component's markup is not the page's print");
    save(s.port, &d, &hot_program("b", "count ").replace("<script src=", "<p>more</p><script src="), "<p>more</p>");
    assert_ne!(print(s.port), first, "the page's own HTML changed");
    drop(s);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch=reload"]);
    assert!(support::send(s.port, "GET", "/", &[], "").body.contains("HOT=0"));
    drop(s);
    // Live data's version moves with any change of data (here a publish that changes nothing the
    // page shows): the page's live states follow it, so its print does not.
    let d = dir("hot-live", "server fn load() = 1\ncomponent C() {\n    state x = live(load)\n    <p>{x}</p>\n}\nroute GET /bump {\n    publish(\"elsewhere\")\n    \"ok\"\n}\nroute GET / = <html><body><div><C/></div></body></html>\n");
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let before = support::send(s.port, "GET", "/", &[], "").body;
    assert!(before.contains(" data-v=\""), "{}", before);
    support::send(s.port, "GET", "/bump", &[], "");
    let after = support::send(s.port, "GET", "/", &[], "").body;
    let v = |p: &str| p.split(" data-v=\"").nth(1).unwrap().split('"').next().unwrap().to_string();
    assert_ne!(v(&before), v(&after), "the version moved");
    assert_eq!(print(s.port), before.split(",h='").nth(1).unwrap().split('\'').next().unwrap(), "the print did not");
}

const TASKS: &str = "let tasks = [\"first\"]\nserver fn list() = tasks\nroute POST /add {\n    tasks = tasks + [\"more\"]\n    publish(\"var:tasks\")\n    \"ok\"\n}\nroute GET /other = \"OTHER\"\ncomponent List() {\n    state items = live(list)\n    <ul id=\"l\">{for t in items => <li>{t}</li>}</ul>\n}\nroute GET / = <html><body><div><List/></div></body></html>\n";

/// GET / as a page made with version `was` asks after a save: the response's print, and the
/// print of what that version renders now.
fn asked(port: u16, was: &str) -> (String, Option<String>) {
    let a = support::send(port, "GET", "/", &[("x-quillo-hot", was)], "");
    let print = a.body.split(",h='").nth(1).unwrap().split('\'').next().unwrap().to_string();
    (print, a.header("x-quillo-was").map(String::from))
}

// Q-685, found by Quillo DevTools' tests: a page whose data changed since it was made (a live
// list grown longer) was told its HTML changed at the next save, whatever the save, and reloaded.
// The server keeps the version a save replaced and renders it too, by the same request, on the
// data as it is now: the same print, unless the save changed the page. A page more than one save
// behind compares its own, as before.
#[test]
fn a_save_is_judged_on_the_data_as_it_is_now() {
    let d = dir("same-data", TASKS);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let v1 = support::send(s.port, "GET", "/_quillo/events", &[], "").body;
    support::send(s.port, "POST", "/add", &[], "");
    std::fs::write(d.join("main.qwe"), TASKS.replace("\"OTHER\"", "\"OTHER 2\"")).unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    while support::send(s.port, "GET", "/other", &[], "").body != "OTHER 2" {
        assert!(Instant::now() < end, "the save was not loaded");
        std::thread::sleep(Duration::from_millis(50));
    }
    let (now, was) = asked(s.port, &v1);
    assert_eq!(was.as_deref(), Some(now.as_str()), "a save to a route the page does not call");
    let v2 = support::send(s.port, "GET", "/_quillo/events", &[], "").body;
    assert_ne!(v1, v2);
    // Outside the islands (changed with A.2: the edit was inside the component's list).
    save(s.port, &d, &TASKS.replace("\"OTHER\"", "\"OTHER 2\"").replace("</div></body>", "</div><p>after</p></body>"), "<p>after</p>");
    let (now, was) = asked(s.port, &v2);
    assert!(was.is_some() && was.as_deref() != Some(now.as_str()), "the page's markup changed");
    assert_eq!(asked(s.port, &v1).1, None, "two saves behind: no print of a version not kept");
    assert_eq!(asked(s.port, "x").1, None);
    drop(s);
    let s = support::serve(&d.join("main.qwe"), &[]);
    assert_eq!(support::send(s.port, "GET", "/", &[("x-quillo-hot", "1")], "").header("x-quillo-was"), None, "--watch only");
}

// The same in Chrome: the list followed the data (live), then a save to another route leaves the
// page as it is.
#[test]
fn a_page_whose_data_changed_is_kept_at_a_save_it_does_not_use() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let src = format!("{}let ready = false\nroute GET /stage-ready {{\n    ready = true\n    \"ok\"\n}}\nroute GET /is-ready = ready\n", TASKS);
    let d = dir("same-data-chrome", &src);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let port = s.port;
    let page = std::thread::spawn(move || {
        let script = r##"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const said = [];
  document.addEventListener("quillo:hmr", (e) => { const d = JSON.parse(e.detail); said.push([d.what, d.why]); });
  window.__kept = 1;
  await fetch("/add", { method: "POST" });
  for (let i = 0; i < 300 && document.querySelectorAll("#l li").length < 2; i++) await tick(30);
  const items = document.querySelectorAll("#l li").length;
  await fetch("/stage-ready");
  for (let i = 0; i < 400 && !said.length; i++) await tick(30);
  document.body.dataset.result = JSON.stringify({ items, said, kept: window.__kept === 1 });
})();"##;
        in_chrome(&chrome, &format!("http://127.0.0.1:{}/", port), script)
    });
    let end = Instant::now() + Duration::from_secs(30);
    while support::send(s.port, "GET", "/is-ready", &[], "").body != "true" {
        assert!(Instant::now() < end && !page.is_finished(), "the page did not follow the data");
        std::thread::sleep(Duration::from_millis(30));
    }
    std::fs::write(d.join("main.qwe"), src.replace("\"OTHER\"", "\"OTHER 2\"")).unwrap();
    let r = page.join().unwrap();
    assert_eq!(r, serde_json::json!({ "items": 2, "said": [["kept", ""]], "kept": true }));
}

fn kept_program(n_init: &str, label: &str) -> String {
    format!("let stage = 0\nroute GET /stage/:n(int) {{\n    stage = n\n    \"ok\"\n}}\nroute GET /stage = stage\nroute GET /after.js = file(\"after.js\")\ncomponent Count() {{\n    state n = {}\n    state note = \"\"\n    <div><button id=\"b\" onclick={{() => {{ n += 1; note = \"clicked\" }}}}>{}{{n}}</button><span id=\"note\">{{note}}</span></div>\n}}\nroute GET / = <html><body><div><Count/></div><input id=\"free\"/><details id=\"d\"><summary>more</summary>inside</details><script src=\"/after.js\"></script></body></html>\n", n_init, label)
}

const KEPT_AFTER: &str = "if (sessionStorage.f54k) { const t = (ms) => new Promise((r) => setTimeout(r, ms)); (async () => { for (let i = 0; i < 300 && !(window[Symbol.for(\"quillo\")] || {}).booted; i++) await t(30); await t(100); const QW = window[Symbol.for(\"quillo\")]; document.body.dataset.result = JSON.stringify({ b: document.getElementById(\"b\").textContent, note: document.getElementById(\"note\").textContent, free: document.getElementById(\"free\").value, open: document.getElementById(\"d\").open, remade: QW.remade ?? 0, left: Object.keys(sessionStorage).filter((k) => k.startsWith(\"quillo-keep:\")).length, again: (() => { QW.keep(); const key = Object.keys(sessionStorage).find((k) => k.startsWith(\"quillo-keep:\")); const k = JSON.parse(sessionStorage.getItem(key)); sessionStorage.removeItem(key); return { islands: k.islands.filter(Boolean).length, fields: k.fields.length, open: k.open.length }; })(), errors: window.__quilloErrors }); })(); }\n";

const KEPT_SCRIPT: &str = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  document.getElementById("b").dispatchEvent(new Event("focusin", { bubbles: true }));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const b = document.getElementById("b");
  for (let i = 0; i < 3; i++) { b.click(); await tick(30); }
  const free = document.getElementById("free");
  free.value = "hello";
  document.getElementById("d").open = true;
  sessionStorage.f54k = "1";
  await fetch("/stage/1");
})();"#;

// FEEDBACK54.md, A.6: where a save makes the page reload, it keeps what it held: an island's
// states (while their type is the same), a field typed into that nothing follows, an open
// <details>; read once, so nothing is left behind. A state whose type changed starts again.
#[test]
fn a_reload_keeps_what_the_page_held() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    // A save that changes the program's own script, as a reload needs (changed with A.2 and A.3:
    // a component's label is swapped in place, and the page's own HTML morphed).
    let outside = |p: String| p.replace("<script src=\"/after.js\">", "<script src=\"/after.js?2\">");
    for (to, want_b, want_note) in [(outside(kept_program("0", "n=")), "n=3", "clicked"), (outside(kept_program("\"zero\"", "n=")), "n=zero", "clicked")] {
        let d = dir("kept", &kept_program("0", "n="));
        std::fs::write(d.join("after.js"), KEPT_AFTER).unwrap();
        let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
        let port = s.port;
        let page = std::thread::spawn(move || in_chrome(&chrome_or_skip(), &format!("http://127.0.0.1:{}/", port), KEPT_SCRIPT));
        let end = Instant::now() + Duration::from_secs(30);
        while support::send(s.port, "GET", "/stage", &[], "").body != "1" {
            assert!(Instant::now() < end && !page.is_finished(), "the page did not get ready");
            std::thread::sleep(Duration::from_millis(30));
        }
        std::fs::write(d.join("main.qwe"), &to).unwrap();
        let r = page.join().unwrap();
        assert_eq!(
            r,
            // `again`: what the page as it came back would keep, asked of it here (what ran before a
            // reload is lost to the runtime's coverage).
            serde_json::json!({ "b": want_b, "note": want_note, "free": "hello", "open": true, "remade": 0, "left": 0, "again": { "islands": 1, "fields": 1, "open": 1 }, "errors": [] }),
            "to {}",
            to
        );
    }
    let _ = chrome;
}

fn chrome_or_skip() -> PathBuf {
    chrome().unwrap()
}

fn swap_program(n_init: &str, label: &str) -> String {
    format!("let stage = 0\nroute GET /stage/:n(int) {{\n    stage = n\n    \"ok\"\n}}\nroute GET /stage = stage\ncomponent Count() {{\n    state n = {}\n    state note = \"\"\n    <div><button id=\"b\" onclick={{() => {{ n += 1; note = \"clicked\" }}}}>{}{{n}}</button><span id=\"note\">{{note}}</span></div>\n}}\nroute GET / = <html><body><div><Count/></div><p id=\"outside\">outside</p></body></html>\n", n_init, label)
}

const SWAP_SCRIPT: &str = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const b = () => document.getElementById("b");
  b().dispatchEvent(new Event("focusin", { bubbles: true }));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const said = [];
  document.addEventListener("quillo:hmr", (e) => { const d = JSON.parse(e.detail); said.push(d.what); });
  for (let i = 0; i < 3; i++) { b().click(); await tick(30); }
  window.__kept = 1;
  await fetch("/stage/1");
  for (let i = 0; i < 400 && !b().textContent.startsWith("count "); i++) await tick(30);
  const first = { b: b().textContent, note: document.getElementById("note").textContent };
  b().click(); await tick(50);
  const clicked = b().textContent;
  await fetch("/stage/2");
  for (let i = 0; i < 400 && !b().textContent.includes("zero"); i++) await tick(30);
  const QW = window[Symbol.for("quillo")];
  document.body.dataset.result = JSON.stringify({ first, clicked, second: b().textContent, said, kept: window.__kept === 1, remade: QW.remade ?? 0, errors: window.__quilloErrors });
})();"#;

// FEEDBACK54.md, A.2: a save that changes a component's code swaps it into the open page in
// place: no reload (what the page's own script set stays), its states kept while their kind is
// the same, its handlers the new code's; a state whose kind changed starts again.
#[test]
fn a_changed_component_is_swapped_in_place() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = dir("swap", &swap_program("0", "n="));
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let port = s.port;
    let page = std::thread::spawn(move || in_chrome(&chrome, &format!("http://127.0.0.1:{}/", port), SWAP_SCRIPT));
    let wait = |n: &str| {
        let end = Instant::now() + Duration::from_secs(30);
        while support::send(port, "GET", "/stage", &[], "").body != n {
            assert!(Instant::now() < end, "stage {}", n);
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    wait("1");
    std::fs::write(d.join("main.qwe"), swap_program("0", "count ")).unwrap();
    wait("2");
    std::fs::write(d.join("main.qwe"), swap_program("\"zero\"", "count ")).unwrap();
    let r = page.join().unwrap();
    assert_eq!(
        r,
        serde_json::json!({ "first": { "b": "count 3", "note": "clicked" }, "clicked": "count 4", "second": "count zero", "said": ["swapped", "swapped"], "kept": true, "remade": 0, "errors": [] })
    );
}

fn nested_program(label: &str) -> String {
    format!("let stage = 0\nroute GET /stage/:n(int) {{\n    stage = n\n    \"ok\"\n}}\nroute GET /stage = stage\ncomponent Inner(name: str) {{\n    state m = 0\n    <button class=\"in\" onclick={{() => {{ m += 1 }}}}>{{name}} {{m}}</button>\n}}\ncomponent Outer() {{\n    state open = false\n    <div><p id=\"o\">{}{{str(open)}}</p><button id=\"t\" onclick={{() => {{ open = !open }}}}>t</button><Inner name=\"a\"/><Inner name=\"b\"/></div>\n}}\nroute GET / = <html><body><div><Outer/></div></body></html>\n", label)
}

const NESTED_SCRIPT: &str = r##"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  $("#t").dispatchEvent(new Event("focusin", { bubbles: true }));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const said = [];
  document.addEventListener("quillo:hmr", (e) => said.push(JSON.parse(e.detail).what));
  $("#t").click(); await tick(30);
  const [a, b] = document.querySelectorAll(".in");
  a.click(); await tick(30); b.click(); await tick(30); b.click(); await tick(30);
  await fetch("/stage/1");
  for (let i = 0; i < 400 && !$("#o").textContent.startsWith("open: "); i++) await tick(30);
  const after = [...document.querySelectorAll(".in")].map((x) => x.textContent);
  document.querySelectorAll(".in")[0].click(); await tick(50);
  const QW = window[Symbol.for("quillo")];
  document.body.dataset.result = JSON.stringify({ o: $("#o").textContent, after, clicked: document.querySelectorAll(".in")[0].textContent, said, remade: QW.remade ?? 0, errors: window.__quilloErrors });
})();"##;

// A.2's edge: the components inside a swapped one keep their states too, each its own, by
// component and order, and their handlers still work.
#[test]
fn components_inside_a_swapped_one_keep_their_states() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = dir("swap-nested", &nested_program("open="));
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let port = s.port;
    let page = std::thread::spawn(move || in_chrome(&chrome, &format!("http://127.0.0.1:{}/", port), NESTED_SCRIPT));
    let end = Instant::now() + Duration::from_secs(30);
    while support::send(s.port, "GET", "/stage", &[], "").body != "1" {
        assert!(Instant::now() < end && !page.is_finished(), "the page did not get ready");
        std::thread::sleep(Duration::from_millis(30));
    }
    std::fs::write(d.join("main.qwe"), nested_program("open: ")).unwrap();
    let r = page.join().unwrap();
    assert_eq!(r, serde_json::json!({ "o": "open: true", "after": ["a 1", "b 2"], "clicked": "a 2", "said": ["swapped"], "remade": 0, "errors": [] }));
}

const DATA: &str = "let notes = []\nlet title = \"Shop\"\nlet count = 0\nlet helper = null\nroute POST /add {\n    notes = notes + [\"x\"]\n    count += 1\n    helper = (y) => y\n    \"ok\"\n}\nroute GET /n = `${len(notes)} ${title} ${count}`\n";

/// `quillo run --watch` with its log in a file: the server, and where its log is.
fn logged(d: &Path, port: u16) -> (Bare, PathBuf) {
    let log = d.join("server.log");
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", &port.to_string(), "--watch"])
        .env("QUILLO_SECRET", "test secret for the tests")
        .current_dir(d)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < end, "the server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    (Bare(child), log)
}

/// Saves `src` and waits for the reload's line, the `n`th: that line.
fn reloaded(d: &Path, log: &Path, src: &str, n: usize) -> String {
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if let Some(l) = text.lines().filter(|l| l.starts_with("quillo: reloaded ")).nth(n - 1) {
            return l.to_string();
        }
        assert!(Instant::now() < end, "no reload {}: {}", n, text);
        std::thread::sleep(Duration::from_millis(50));
    }
}

// Q-687, Part C: a save started the server's data again (top-level lets emptied, a :memory:
// database too) and said nothing. Under --watch a let the program changes keeps its value across a
// save while its kind is the same, and the reload's line says so; one whose kind changed, or that
// holds a function, starts again, said too; a let the program never changes is code and follows
// the edit; a :memory: database starts again, said.
#[test]
fn a_save_keeps_the_servers_data_and_says_so() {
    let d = dir("data", DATA);
    let port = support::free_port();
    let (_s, log) = logged(&d, port);
    support::send(port, "POST", "/add", &[], "");
    support::send(port, "POST", "/add", &[], "");
    assert_eq!(support::send(port, "GET", "/n", &[], "").body, "2 Shop 2");
    let line = reloaded(&d, &log, &DATA.replace("\"Shop\"", "\"My shop\""), 1);
    assert_eq!(support::send(port, "GET", "/n", &[], "").body, "2 My shop 2", "{}", line);
    assert!(line.contains("; kept notes, count") && line.contains("started again: helper (it holds fn)"), "{}", line);
    let line = reloaded(&d, &log, &DATA.replace("\"Shop\"", "\"My shop\"").replace("let count = 0", "let count = \"\""), 2);
    assert!(line.contains("kept notes") && line.contains("count (its type changed: int to str)"), "{}", line);
    assert_eq!(support::send(port, "GET", "/n", &[], "").body, "2 My shop ", "{}", line);
    let line = reloaded(&d, &log, &format!("db.open(\":memory:\")\n{}", DATA), 3);
    assert!(line.contains(":memory: database starts again empty"), "{}", line);
    // Not under --reload (production): nothing carried, nothing said.
    drop(_s);
    let d = dir("data-reload", DATA);
    let port = support::free_port();
    let log = d.join("server.log");
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).args(["--port", &port.to_string(), "--reload"]).env("QUILLO_SECRET", "test secret for the tests").current_dir(&d).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::fs::File::create(&log).unwrap()).spawn().unwrap();
    let _s = Bare(child);
    let end = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(20));
    }
    support::send(port, "POST", "/add", &[], "");
    let line = reloaded(&d, &log, &DATA.replace("\"Shop\"", "\"My shop\""), 1);
    assert!(!line.contains("kept"), "{}", line);
    assert_eq!(support::send(port, "GET", "/n", &[], "").body, "0 My shop 0");
}

fn morph_program(title: &str, label: &str, more: &str) -> String {
    // `Quiet`, never reached for, is not taken up: the page gives it its new HTML whole; the
    // stylesheet follows the title, so the page's sheets are swapped.
    format!("let stage = 0\nroute GET /stage/:n(int) {{\n    stage = n\n    \"ok\"\n}}\nroute GET /stage = stage\ncomponent Count(label: str) {{\n    state n = 0\n    <button id=\"b\" onclick={{() => {{ n += 1 }}}}>{{label}} {{n}}</button>\n}}\ncomponent Quiet(label: str) {{\n    state m = 0\n    <i id=\"q\" onclick={{() => {{ m += 1 }}}}>{{label}}</i>\n}}\nroute GET / = <html><head><title>{}</title><link rel=\"stylesheet\" href=\"/{}.css\"/></head><body><h1 id=\"h\" class=\"{}\">{}</h1><div><Count label=\"{}\"/></div><section><Quiet label=\"{}\"/></section>{}</body></html>\n", title, title.to_lowercase(), title, title, label, title, more)
}

const MORPH_SCRIPT: &str = r##"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  $("#b").dispatchEvent(new Event("focusin", { bubbles: true }));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const said = [];
  document.addEventListener("quillo:hmr", (e) => { const d = JSON.parse(e.detail); said.push(d.what); });
  for (let i = 0; i < 3; i++) { $("#b").click(); await tick(30); }
  window.__kept = 1;
  await fetch("/stage/1");
  for (let i = 0; i < 400 && $("#h").textContent !== "Two"; i++) await tick(30);
  const first = { h: $("#h").textContent, cls: $("#h").className, title: document.title, b: $("#b").textContent, q: $("#q").textContent, sheet: [...document.querySelectorAll("link[rel=stylesheet]")].map((l) => l.getAttribute("href")) };
  await fetch("/stage/2");
  for (let i = 0; i < 400 && !$("#b").textContent.startsWith("clicks"); i++) await tick(30);
  const second = { b: $("#b").textContent };
  await fetch("/stage/3");
  for (let i = 0; i < 400 && !$("#foot"); i++) await tick(30);
  const QW = window[Symbol.for("quillo")];
  document.body.dataset.result = JSON.stringify({ first, second, foot: ($("#foot") || {}).textContent, said, kept: window.__kept === 1, remade: QW.remade ?? 0, errors: window.__quilloErrors });
})();"##;

// FEEDBACK54.md, A.3: a save that changes the page's own HTML outside its components (a heading's
// text and class, the title) no longer reloads it: the page morphs onto the new HTML, its
// components kept as they are; a component whose props the page changed is made again with them,
// its states kept; a node added at the end is added. The page says "morphed".
#[test]
fn the_pages_own_html_is_morphed_in_place() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = dir("morph", &morph_program("One", "n=", ""));
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let port = s.port;
    let page = std::thread::spawn(move || in_chrome(&chrome, &format!("http://127.0.0.1:{}/", port), MORPH_SCRIPT));
    let wait = |n: &str| {
        let end = Instant::now() + Duration::from_secs(30);
        while support::send(port, "GET", "/stage", &[], "").body != n {
            assert!(Instant::now() < end, "stage {}", n);
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    wait("1");
    std::fs::write(d.join("main.qwe"), morph_program("Two", "n=", "")).unwrap();
    wait("2");
    std::fs::write(d.join("main.qwe"), morph_program("Two", "clicks", "")).unwrap();
    wait("3");
    std::fs::write(d.join("main.qwe"), morph_program("Two", "clicks", "<p id=\"foot\">foot</p>")).unwrap();
    let r = page.join().unwrap();
    assert_eq!(
        r,
        serde_json::json!({ "first": { "h": "Two", "cls": "Two", "title": "Two", "b": "n= 3", "q": "Two", "sheet": ["/two.css"] }, "second": { "b": "clicks 3" }, "foot": "foot", "said": ["morphed", "morphed", "morphed"], "kept": true, "remade": 0, "errors": [] })
    );
}

// Q-689, found in review: the key file was made through a link planted where it goes, and a short
// one there before was rewritten and kept its mode. A link is refused (the server says so and
// signs with a random key), a short file is replaced by one its owner alone can read, and a key
// others could read is made its owner's, said.
#[cfg(unix)]
#[test]
fn the_session_key_file_is_made_safely() {
    use std::os::unix::fs::PermissionsExt;
    let d = dir("key-safe", VISITS);
    std::fs::create_dir_all(d.join(".quillo")).unwrap();
    std::os::unix::fs::symlink(d.join("elsewhere.txt"), d.join(".quillo/.quillo-secret")).unwrap();
    let port = support::free_port();
    let (s, said) = bare(&d, port, &["--watch"]);
    drop(s);
    assert!(said.contains("is a link") && !d.join("elsewhere.txt").exists(), "{}", said);
    std::fs::remove_file(d.join(".quillo/.quillo-secret")).unwrap();
    std::fs::write(d.join(".quillo/.quillo-secret"), "short").unwrap();
    std::fs::set_permissions(d.join(".quillo/.quillo-secret"), std::fs::Permissions::from_mode(0o644)).unwrap();
    let (s, _) = bare(&d, port, &["--watch"]);
    drop(s);
    let key = d.join(".quillo/.quillo-secret");
    assert_eq!(std::fs::metadata(&key).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(std::fs::read_to_string(&key).unwrap().len() >= 32);
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
    let (s, said) = bare(&d, port, &["--watch"]);
    drop(s);
    assert!(said.contains("could be read by others"), "{}", said);
    assert_eq!(std::fs::metadata(&key).unwrap().permissions().mode() & 0o777, 0o600);
}

// Found in review: the version a save replaced rendered the page for real, so a GET that writes
// wrote twice at each save, once for each open page. It renders as a prefetch does: one that
// would change something gives no print (the page compares its own), and only a GET is rendered.
#[test]
fn the_old_versions_render_changes_nothing() {
    let src = "let hits = 0\nroute GET / {\n    hits += 1\n    <html><body><p>{hits}</p></body></html>\n}\nroute GET /calm = <html><body><p>calm</p></body></html>\nroute GET /hits = hits\nroute GET /other = \"a\"\n";
    let d = dir("old-render", src);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let v1 = support::send(s.port, "GET", "/_quillo/events", &[], "").body;
    std::fs::write(d.join("main.qwe"), src.replace("\"a\"", "\"b\"")).unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    while support::send(s.port, "GET", "/other", &[], "").body != "b" {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(50));
    }
    // The hits are the server's data, kept across the save (Part C): read them before and after.
    let before: i64 = support::send(s.port, "GET", "/hits", &[], "").body.parse().unwrap();
    let a = support::send(s.port, "GET", "/", &[("x-quillo-hot", &v1)], "");
    assert_eq!(a.header("x-quillo-was"), None, "a render that writes gives no print");
    let after: i64 = support::send(s.port, "GET", "/hits", &[], "").body.parse().unwrap();
    assert_eq!(after, before + 1, "the old version wrote too");
    assert!(support::send(s.port, "GET", "/calm", &[("x-quillo-hot", &v1)], "").header("x-quillo-was").is_some());
    assert_eq!(support::send(s.port, "POST", "/calm", &[("x-quillo-hot", &v1)], "").header("x-quillo-was"), None);
}

// Found in review: the old version's render (for the page's print) started its own page states
// over the request's, and the new version's deferred parts, rendered after it, found none: the
// page stayed on its placeholder, with an error in the log.
#[test]
fn the_old_render_leaves_the_requests_page_states_alone() {
    let src = "state me = \"init\"\nfn mine() {\n    return me\n}\nserver fn slow() {\n    sleep(50)\n    return mine()\n}\ncomponent Page() {\n    state s = defer(slow())\n    <div>{s == null ? <p>still waiting</p> : <p id=\"d\">{s}</p>}</div>\n}\nroute GET /other = \"a\"\nroute GET /d/:who {\n    me = who\n    <html><body><Page/></body></html>\n}\n";
    let d = dir("old-render-states", src);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch", "--no-check"]);
    let v1 = support::send(s.port, "GET", "/_quillo/events", &[], "").body;
    std::fs::write(d.join("main.qwe"), src.replace("\"a\"", "\"b\"")).unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    while support::send(s.port, "GET", "/other", &[], "").body != "b" {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(50));
    }
    let a = support::send(s.port, "GET", "/d/carol", &[("x-quillo-hot", &v1)], "");
    assert!(a.body.contains("<p id=\"d\">carol</p>"), "{}", a.body);
}

/// A second of silence, as a WAV file, for an <audio> to play: 8,000 samples of 8 bits.
fn silence() -> String {
    let mut w = Vec::new();
    let n: u32 = 8000;
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + n).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&8000u32.to_le_bytes());
    w.extend_from_slice(&8000u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&8u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&n.to_le_bytes());
    w.extend(std::iter::repeat(128u8).take(n as usize));
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in w.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let v = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for k in 0..4 {
            out.push(if k <= c.len() { A[(v >> (18 - 6 * k) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

fn dom_program(label: &str) -> String {
    format!("let stage = 0\nroute GET /stage/:n(int) {{\n    stage = n\n    \"ok\"\n}}\nroute GET /stage = stage\ncomponent Form() {{\n    state n = 0\n    state bound = \"b\"\n    <div>\n        <button id=\"b\" onclick={{() => {{ n += 1 }}}}>{}{{n}}</button>\n        <input id=\"free\"/>\n        <input id=\"bound\" bind:value={{bound}}/>\n        <input id=\"tick\" type=\"checkbox\"/>\n        <details id=\"d\"><summary>more</summary>inside</details>\n        <div id=\"box\" style=\"height: 40px; overflow: auto\"><p style=\"height: 400px\">tall</p></div>\n        <audio id=\"au\" muted src=\"data:audio/wav;base64,{}\"></audio>\n        <dialog id=\"dlg\">a dialog</dialog>\n    </div>\n}}\nroute GET / = <html><body><div><Form/></div></body></html>\n", label, silence())
}

const DOM_SCRIPT: &str = r##"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  $("#b").dispatchEvent(new Event("focusin", { bubbles: true }));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const said = [];
  document.addEventListener("quillo:hmr", (e) => said.push(JSON.parse(e.detail).what));
  $("#b").click(); $("#b").click(); await tick(30);
  $("#bound").value = "typed"; $("#bound").dispatchEvent(new Event("input", { bubbles: true }));
  $("#tick").checked = true;
  $("#d").open = true;
  $("#box").scrollTop = 120;
  const au = $("#au");
  for (let i = 0; i < 200 && au.readyState < 1; i++) await tick(20);
  au.currentTime = 0.5;
  $("#dlg").show();
  const free = $("#free");
  free.value = "hello world"; free.focus(); free.setSelectionRange(2, 7);
  await fetch("/stage/1");
  for (let i = 0; i < 400 && !$("#b").textContent.startsWith("count "); i++) await tick(30);
  const f = $("#free"), au2 = $("#au");
  for (let i = 0; i < 200 && au2.readyState < 1; i++) await tick(20);
  document.body.dataset.result = JSON.stringify({ b: $("#b").textContent, free: f.value, focused: document.activeElement === f, sel: [f.selectionStart, f.selectionEnd], bound: $("#bound").value, tick: $("#tick").checked, open: $("#d").open, scroll: $("#box").scrollTop, dialog: $("#dlg").open, time: au2.currentTime, newAudio: au2 !== au, said, errors: window.__quilloErrors });
})();"##;

// FEEDBACK54.md, A.4: a component swapped in place is made again, and its DOM with it, so what the
// page held that no state does went: what was typed in a field nothing follows, a checkbox ticked,
// an open <details> and <dialog>, a box's scroll, an <audio>'s time, the focus and its selection.
// They are put back after the
// swap, where the place in the component still holds the same kind of element; a bound field
// keeps its state's value.
#[test]
fn a_swap_keeps_what_the_dom_held() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = dir("swap-dom", &dom_program("n="));
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let port = s.port;
    let page = std::thread::spawn(move || in_chrome(&chrome, &format!("http://127.0.0.1:{}/", port), DOM_SCRIPT));
    let end = Instant::now() + Duration::from_secs(30);
    while support::send(s.port, "GET", "/stage", &[], "").body != "1" {
        assert!(Instant::now() < end && !page.is_finished(), "the page did not get ready");
        std::thread::sleep(Duration::from_millis(30));
    }
    std::fs::write(d.join("main.qwe"), dom_program("count ")).unwrap();
    let r = page.join().unwrap();
    assert_eq!(
        r,
        serde_json::json!({ "b": "count 2", "free": "hello world", "focused": true, "sel": [2, 7], "bound": "typed", "tick": true, "open": true, "scroll": 120, "dialog": true, "time": 0.5, "newAudio": true, "said": ["swapped"], "errors": [] })
    );
}
