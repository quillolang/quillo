// FEEDBACK67.md: PWA built into the language (branch pwa). Slice 1: Q-959, `ask` and `notify`
// written where the code runs on the server are refused by `quillo check`, and say what reaches
// someone who is not on the page; Q-957, a page a service worker answers from its cache stops
// reloading under --watch, and says why.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("f67-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

/// `quillo check main.qwe` in `d`, with a deadline: whether it passed, and its error lines.
fn check(d: &Path) -> (bool, Vec<String>) {
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("check")
        .arg("main.qwe")
        .current_dir(d)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > end {
            let _ = child.kill();
            panic!("quillo check did not end");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let o = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    (o.status.success(), text.lines().filter(|l| l.contains(": error: ")).map(String::from).collect())
}

/// What `quillo check` and the run say of `ask` and `notify` on the server.
const IN_THE_BROWSER: &str = "`ask` and `notify` speak to someone in the browser, on a page that is open: call them from a component (an event handler); to reach someone who is not on the page, `notification.send(user, { title, body, url })` sends to their devices, and `publish` updates the pages that are open";

fn refused(line: u32, name: &str, place: &str) -> String {
    format!("main.qwe:{}: error: `{}` {} runs on the server; {}", line, name, place, IN_THE_BROWSER)
}

// The report's two forms: `notify` in a route, and in a `server fn` a component calls. Both passed
// the checker and failed every call with a 500.
const ROUTE: &str = "route POST /orders/:id/ship ? { user: str } {\n    notify(`Order ${id} has shipped`, { title: \"Shop\", to: user })\n    { ok: true }\n}\n";
const SERVER_FN: &str = "server fn ship(id: int) -> map {\n    notify(\"Order shipped\")\n    { ok: true }\n}\ncomponent C() { <button onclick={() => ship(1)}>Ship</button> }\nroute GET / = <html><body><C/></body></html>\n";

#[test]
fn q959_the_reports_two_forms_are_refused_with_what_to_do() {
    assert_eq!(check(&dir("route", ROUTE)), (false, vec![refused(2, "notify", "in a route")]));
    assert_eq!(check(&dir("server-fn", SERVER_FN)), (false, vec![refused(2, "notify", "in a `server fn`")]));
}

// Where the side is sure: a route's own code and a function written in it, a `server fn` and the
// functions inside it, what a top-level `every` (a tick) or `schedule` (a job) is given, the top
// level and a page state's initializer (the server runs both as the program starts or for each
// request). `ask` as `notify`; piped, as a method, unicode. One error a call: given too many, that
// is what is said.
#[test]
fn q959_where_the_code_runs_on_the_server_it_is_refused() {
    let src = r#"every(5000, () => notify("tick"))
schedule("5m", () => { ask("job?") })
notify("top")
state shown = notify("page")
server fn save(xs: list) {
    xs.map((x) => notify(x))
    "x" | notify()
    "y".notify()
    notify("a", { title: "T" }, 3)
    return 1
}
route GET /r {
    let f = () => ask("Envoyé ? 日本 ✓")
    f()
    <html><body><p title={notify("attr")}>x</p></body></html>
}
route GET /c = <html><body><C/></body></html>
component C() { <button onclick={() => save([1])}>x</button> }
"#;
    let (ok, got) = check(&dir("sides", src));
    assert!(!ok);
    assert_eq!(
        got,
        [
            refused(1, "notify", "in a tick"),
            refused(2, "ask", "in a job"),
            refused(3, "notify", "at the top level"),
            refused(4, "notify", "at the top level"),
            refused(6, "notify", "in a `server fn`"),
            refused(7, "notify", "in a `server fn`"),
            refused(8, "notify", "in a `server fn`"),
            "main.qwe:9: error: `notify` takes 2 arguments, got 3: notify(message, options)".to_string(),
            refused(13, "ask", "in a route"),
            refused(15, "notify", "in a route"),
        ]
    );
}

// What must keep working: a component's handler, a handler written in a route's markup (onclick
// and @click), a view's, a component calling a plain helper that calls `notify` (the helper may run
// either side: not guessed), a lambda a top-level `let` holds, one a page state holds, a function
// given to a view, `every` in a component (the page's clock), a `notify` of the program's own,
// `ask` handed on as a value, and a tool's `ask`, which is said of the tool, once.
#[test]
fn q959_where_the_code_may_run_in_the_browser_nothing_is_said() {
    let src = r#"fn tell(m: str) { notify(m) }
let later = () => notify("later")
state on_save = () => notify("saved")
view Card(go: fn) = <button onclick={() => { go(); notify("view") }}>v</button>
component C() {
    state n = 0
    if n > 3 { every(1000, () => { notify("Big!") }) }
    <div><button onclick={() => { notify("comp"); tell("helper"); n += 1 }}>x</button><Card go={() => ask("sure?")}/></div>
}
fn call1(f, a) = f(a)
route GET /h = <html><body><button onclick={() => notify("handler")} @dblclick={() => ask("sure?")}>Go</button></body></html>
route GET /v = call1(ask, "value")
route GET / = <html><body><C/></body></html>
"#;
    assert_eq!(check(&dir("browser", src)), (true, vec![]));
    let own = "fn notify(who: str, text: str) = `${who}: ${text}`\nroute GET / = notify(\"ana\", \"shipped\")\n";
    assert_eq!(check(&dir("own", own)), (true, vec![]));
    let tool = "/// T.\nserver fn mcp quiz() with time = ask(\"Q?\")\n/// N.\nserver fn mcp n() with time {\n    notify(\"x\")\n    return 1\n}\n";
    let (ok, got) = check(&dir("tool", tool));
    assert!(!ok);
    assert_eq!(got.len(), 2, "{:?}", got);
    assert!(got[0].starts_with("main.qwe:2: error: in a tool, `ask` is the page's dialog"), "{:?}", got);
    assert_eq!(got[1], refused(5, "notify", "in a `server fn`"));
}

/// `quillo run` of `d` on `port` with its log in a file: the server, and the log's path.
struct Logged(std::process::Child, PathBuf);

impl Drop for Logged {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn logged(d: &Path, opts: &[&str]) -> (Logged, u16) {
    let port = support::free_port();
    let log = d.join("server.log");
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", &port.to_string()])
        .args(opts)
        .env("QUILLO_SECRET", "test secret for the tests")
        .current_dir(d)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let s = Logged(child, log);
    let end = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < end, "the server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    (s, port)
}

/// The log, once it holds `want`.
fn log_with(s: &Logged, want: &str) -> String {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let text = std::fs::read_to_string(&s.1).unwrap_or_default();
        if text.contains(want) || Instant::now() > end {
            return text;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

// Run without the check, as before: a 500, and the log says what to do instead; under --base too
// (the prefix a proxy takes off), and from a `server fn` a page calls.
#[test]
fn q959_a_run_without_the_check_says_what_to_do_as_it_fails() {
    for opts in [&["--no-check"][..], &["--no-check", "--base", "/shop"][..]] {
        let d = dir("run", ROUTE);
        let (s, port) = logged(&d, opts);
        assert_eq!(support::send(port, "POST", "/orders/7/ship?user=ana", &[], "").status, 500);
        let log = log_with(&s, IN_THE_BROWSER);
        assert!(log.contains(&format!("main.qwe:2: {}", IN_THE_BROWSER)), "{:?}: {}", opts, log);
    }
    let d = dir("run-fn", SERVER_FN);
    let (s, port) = logged(&d, &["--no-check"]);
    let a = support::send(port, "POST", "/_quillo/rpc/ship", &[("content-type", "application/json"), ("origin", "http://localhost")], "[1]");
    assert_eq!(a.status, 500, "{}", a.body);
    assert!(log_with(&s, IN_THE_BROWSER).contains(IN_THE_BROWSER));
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

/// One Chrome at a time, for all of a case: its page waits on the server's stage meanwhile.
fn chrome_turn() -> std::sync::MutexGuard<'static, ()> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TURN.lock().unwrap_or_else(|e| e.into_inner())
}

/// `url` in headless Chrome (tests/browser/run.js): what the page stored in `data-result`.
fn in_chrome(chrome: &Path, url: &str, script: &str) -> serde_json::Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let d = support::temp_dir(&format!("f67-chrome-{}", script.len()));
    std::fs::write(d.join("t.js"), script).unwrap();
    let out = std::process::Command::new("node").arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(d.join("profile")).arg("40000").arg(d.join("t.js")).output().expect("node");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

/// The repro's page, at `path`, with its script: a stage the test waits on, and the worker.
fn sw_program(path: &str, h1: &str) -> String {
    format!("let stage = 0\nroute GET /stage/:n(int) {{\n    stage = n\n    \"ok\"\n}}\nroute GET /stage = stage\nroute GET /sw.js = file(\"sw.js\")\nroute GET /t.js = file(\"t.js\")\nroute GET {} = <html><body><h1>{}</h1><script src=\"/t.js\"></script></body></html>\n", path, h1)
}

/// The tutorial worker of the report: the page kept at install, and every answer from the cache.
const CACHE_FIRST: &str = r#"self.addEventListener("install", (e) => e.waitUntil(caches.open("v1").then((c) => c.add("/"))));
self.addEventListener("activate", (e) => e.waitUntil(clients.claim()));
self.addEventListener("fetch", (e) => e.respondWith(caches.match(e.request).then((hit) => hit || fetch(e.request))));
"#;

/// A worker that goes to the network first, and to its cache only when that fails.
const NETWORK_FIRST: &str = r#"self.addEventListener("install", (e) => e.waitUntil(caches.open("v1").then((c) => c.add("/"))));
self.addEventListener("activate", (e) => e.waitUntil(clients.claim()));
self.addEventListener("fetch", (e) => e.respondWith(fetch(e.request).catch(() => caches.match(e.request))));
"#;

/// Run on each load of the page (kept in sessionStorage across them): the first registers the
/// worker (`@WORKER@`), waits for it to control the page and reloads, so that the page comes from
/// the worker; the second asks for the save; 4 s after, the page loaded last says how many loads
/// came after the save, what it shows, the overlay, the console's warnings and what the dev
/// script told tools (`quillo:hmr`).
const PAGE_SCRIPT: &str = r#"(async () => {
  const S = sessionStorage, tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const warned = JSON.parse(S.f67w || "[]"), warn = console.warn;
  console.warn = (...a) => { warned.push(a.join(" ")); S.f67w = JSON.stringify(warned); warn.apply(console, a); };
  document.addEventListener("quillo:hmr", (e) => { const d = JSON.parse(e.detail); const h = JSON.parse(S.f67h || "[]"); h.push([d.what, d.why]); S.f67h = JSON.stringify(h); });
  const n = +(S.f67n || 0) + 1;
  S.f67n = n;
  const sw = navigator.serviceWorker;
  if (!S.f67stage) {
    S.f67stage = "1";
    if (@WORKER@) {
      await sw.register("/sw.js");
      await sw.ready;
      for (let i = 0; i < 300 && !sw.controller; i++) await tick(20);
    }
    location.reload();
    return;
  }
  if (S.f67stage === "1") {
    S.f67stage = "2";
    S.f67base = n;
    S.f67h = "[]";
    S.f67at = Date.now();
    await fetch("/stage/1");
  }
  while (Date.now() < +S.f67at + 4000) await tick(50);
  const box = document.querySelector("quillo-error");
  document.body.dataset.result = JSON.stringify({
    loads: n - +S.f67base,
    h1: document.querySelector("h1").textContent,
    controlled: !!(sw && sw.controller),
    said: box ? box.shadowRoot.querySelector("pre").textContent : null,
    warned,
    hmr: JSON.parse(S.f67h),
    left: Object.keys(S).filter((k) => k.startsWith("quillo-reload:")).length,
  });
})();
"#;

/// One case: the page at `path` (as the URL writes it, `shown`) with `worker` under `opts`, a save
/// that changes its <h1>, and what the page said 4 s after.
fn sw_case(chrome: &Path, name: &str, path: &str, shown: &str, worker: Option<&str>, opts: &[&str]) -> serde_json::Value {
    let d = dir(name, &sw_program(path, "One"));
    std::fs::write(d.join("sw.js"), worker.unwrap_or("")).unwrap();
    std::fs::write(d.join("t.js"), PAGE_SCRIPT.replace("@WORKER@", if worker.is_some() { "true" } else { "false" })).unwrap();
    let s = support::serve_with(&d.join("main.qwe"), opts);
    let port = s.port;
    let (chrome, url) = (chrome.to_path_buf(), format!("http://127.0.0.1:{}{}", port, shown));
    let page = std::thread::spawn(move || in_chrome(&chrome, &url, "void 0"));
    let end = Instant::now() + Duration::from_secs(30);
    while support::send(port, "GET", "/stage", &[], "").body != "1" {
        assert!(Instant::now() < end && !page.is_finished(), "{}: the page did not reach its stage", name);
        std::thread::sleep(Duration::from_millis(30));
    }
    std::fs::write(d.join("main.qwe"), sw_program(path, "Two")).unwrap();
    page.join().unwrap()
}

/// What the page says when its worker's cache answered it.
const CACHED: &str = "This page came from a service worker's cache (`navigator.serviceWorker.controller`), so `--watch` cannot refresh it: unregister the worker (DevTools > Application > Service workers), or have it go to the server first for pages";

// Q-957: the report's page, its tutorial worker answering everything from its cache. Under --watch
// the page asked for itself, got the old page from the cache, and stayed as it was, saying
// nothing; under --watch=reload it reloaded without end (314 loads in 4 s). Now one reload, which
// brings the cached page back older than the version it reloaded for: no more, and the overlay and
// the console say why. Nothing is left in sessionStorage.
#[test]
fn q957_a_page_a_cache_first_worker_answers_stops_and_says_why() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    for (opts, why) in [(&["--watch"][..], "cache"), (&["--watch=reload"][..], "off")] {
        let r = sw_case(&chrome, "cache-first", "/", "/", Some(CACHE_FIRST), opts);
        assert_eq!(
            r,
            serde_json::json!({ "loads": 1, "h1": "One", "controlled": true, "said": CACHED, "warned": [CACHED], "hmr": [["reload", why], ["cached", why]], "left": 0 }),
            "{:?}",
            opts
        );
    }
}

// What must keep working: a worker that goes to the network first, and no worker (at a unicode
// path, which the kept version is named by): one reload, and the change shown.
#[test]
fn q957_a_network_first_worker_or_none_reloads_once_and_shows_the_change() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    for (name, path, shown, worker) in [("network-first", "/", "/", Some(NETWORK_FIRST)), ("no-worker", "/café", "/caf%C3%A9", None)] {
        let r = sw_case(&chrome, name, path, shown, worker, &["--watch"]);
        assert_eq!(
            r,
            serde_json::json!({ "loads": 1, "h1": "Two", "controlled": worker.is_some(), "said": null, "warned": [], "hmr": [["reload", "html"]], "left": 0 }),
            "{}",
            name
        );
    }
}
