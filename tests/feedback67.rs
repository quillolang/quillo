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

/// The system picks the port (`--port 0`) and the log says which: a port picked here could be
/// taken by another process before the server bound it (a shown bug in the test, as feedback31's
/// and prefetching's on CI; branch security).
fn logged(d: &Path, opts: &[&str]) -> (Logged, u16) {
    let mut s = start_logged(d, "0", opts);
    let port = support::port_in_log(&mut s.0, &s.1, Duration::from_secs(20));
    (s, port)
}

/// `logged`, on `port`: a server started again where the one before was. The port must be that
/// one, so it is given; the listener is checked to be this server, as another process could have
/// taken the port between the two (branch security).
fn logged_at(d: &Path, port: u16, opts: &[&str]) -> Logged {
    let s = start_logged(d, &port.to_string(), opts);
    let end = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < end, "the server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(support::listener_is(port, s.0.id()), "another process listens on {}", port);
    s
}

fn start_logged(d: &Path, port: &str, opts: &[&str]) -> Logged {
    let log = d.join("server.log");
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", port])
        .args(opts)
        .env("QUILLO_SECRET", "test secret for the tests")
        .current_dir(d)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    Logged(child, log)
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

// ---- B.1: installable from app.toml (slice 2) --------------------------------------------------

/// A program's folder with these files.
fn app_dir(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let d = support::temp_dir(&format!("f67-app-{}", name));
    for (f, bytes) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    d
}

/// An answer's status, headers (lower case) and body as bytes: an icon is no text.
fn fetch(port: u16, method: &str, path: &str, headers: &[(&str, &str)]) -> (u16, Vec<(String, String)>, Vec<u8>) {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut head = format!("{} {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n", method, path);
    for (k, v) in headers {
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let end = out.windows(4).position(|w| w == b"\r\n\r\n").map_or(out.len(), |i| i + 4);
    let text = String::from_utf8_lossy(&out[..end]).into_owned();
    let mut lines = text.lines();
    let status = lines.next().and_then(|l| l.split(' ').nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
    let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    (status, headers, out[end..].to_vec())
}

fn header<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
    h.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn page(port: u16, path: &str) -> String {
    let (status, _, body) = fetch(port, "GET", path, &[]);
    assert_eq!(status, 200, "{}", path);
    String::from_utf8(body).unwrap()
}

fn manifest(port: u16) -> serde_json::Value {
    let (status, h, body) = fetch(port, "GET", "/manifest.webmanifest", &[]);
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    assert_eq!(header(&h, "content-type"), Some("application/manifest+json"));
    serde_json::from_slice(&body).unwrap()
}

/// An image file, as PNG (or another format), each pixel from `f`.
fn image_file(w: u32, h: u32, format: image::ImageFormat, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(w, h, |x, y| image::Rgba(f(x, y))));
    let img = if format == image::ImageFormat::Jpeg { image::DynamicImage::ImageRgb8(img.to_rgb8()) } else { img };
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), format).unwrap();
    out
}

const RED: [u8; 4] = [255, 0, 0, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

/// The icon at `src` (the manifest's, the base taken off as a proxy would), decoded: a PNG.
fn icon_at(port: u16, src: &str) -> image::DynamicImage {
    let (status, h, body) = fetch(port, "GET", src, &[]);
    assert_eq!(status, 200, "{}", src);
    assert_eq!(header(&h, "content-type"), Some("image/png"));
    image::load_from_memory_with_format(&body, image::ImageFormat::Png).unwrap()
}

/// The manifest's icons, by purpose and size, each decoded.
fn icons(port: u16, base: &str) -> Vec<(String, u32, image::DynamicImage)> {
    let m = manifest(port);
    m["icons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            let src = i["src"].as_str().unwrap();
            let rest = src.strip_prefix(base).unwrap();
            let hash = rest.strip_prefix("/_quillo/i/").and_then(|r| r.strip_suffix(".png")).unwrap();
            assert!(hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()), "{}", src);
            assert_eq!(i["type"], "image/png");
            let size: u32 = i["sizes"].as_str().unwrap().split('x').next().unwrap().parse().unwrap();
            (i["purpose"].as_str().unwrap().to_string(), size, icon_at(port, rest))
        })
        .collect()
}

fn near(got: image::Rgba<u8>, want: [u8; 4]) -> bool {
    got.0.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 3)
}

/// A process with a deadline: its output, killed past `secs`.
fn within(mut c: std::process::Command, secs: u64) -> std::process::Output {
    let mut child = c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    let end = Instant::now() + Duration::from_secs(secs);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > end {
            let _ = child.kill();
            panic!("{:?} did not end", c);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}

/// `quillo check` of `main` in `d`: whether it passed, and what it said of app.toml.
fn check_app(d: &Path, main: &str) -> (bool, Vec<String>) {
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(["check", main]).current_dir(d);
    let o = within(c, 30);
    let text = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    (o.status.success(), text.lines().filter(|l| l.contains("app.toml")).map(String::from).collect())
}

/// Pages of every kind the tags go on, or not.
const PAGES: &str = r#"route GET / = <html><head><title>Shop</title></head><body><h1>Corner Shop</h1></body></html>
route GET /bare = <html><body><h1>Bare</h1></body></html>
route GET /header = <html><body><header>Top</header></body></html>
route GET /own = <html><head><link rel="manifest" href="/m.json"></head><body><h1>Own</h1></body></html>
route GET /theme = <html><head><meta name="theme-color" content="red"></head><body><h1>Theme</h1></body></html>
route GET /part = <p>a part</p>
route GET /static/*p = files("public", p)
route GET /apple = <html><head><meta charset="utf-8"><link rel="apple-touch-icon" href="/mine.png"></head><body>x</body></html>
route GET /open = respond(200, "<html><head><link data-rel=\"manifest\" rel=stylesheet href=s.css><title>Open</title><body>x</body></html>", { "content-type": "text/html; charset=utf-8" })
route GET /upper = respond(200, "<HTML><HEAD><TITLE>Up</TITLE></HEAD><BODY>x</BODY></HTML>", { "content-type": "text/html" })
"#;

const STATIC_PAGE: &[u8] = b"<!doctype html><html><head><title>As written</title></head><body>file</body></html>";

/// What each page is with no app.toml, and with one (`tags` where they go); under `base`, the
/// page's own root-relative link carries it, as every link does.
fn pages_as(tags: &str, theme_less: &str, apple_less: &str, base: &str) -> Vec<(&'static str, String)> {
    vec![
        ("/apple", format!("<!doctype html><html><head><meta charset=\"utf-8\"><link rel=\"apple-touch-icon\" href=\"{}/mine.png\">{}</head><body>x</body></html>", base, apple_less)),
        ("/open", format!("<html><head>{}<link data-rel=\"manifest\" rel=stylesheet href=s.css><title>Open</title><body>x</body></html>", tags)),
        ("/upper", format!("<HTML><HEAD><TITLE>Up</TITLE>{}</HEAD><BODY>x</BODY></HTML>", tags)),
        ("/", format!("<!doctype html><html><head><title>Shop</title>{}</head><body><h1>Corner Shop</h1></body></html>", tags)),
        ("/bare", format!("<!doctype html><html>{}<body><h1>Bare</h1></body></html>", tags)),
        ("/header", format!("<!doctype html><html>{}<body><header>Top</header></body></html>", tags)),
        ("/own", format!("<!doctype html><html><head><link rel=\"manifest\" href=\"{}/m.json\"></head><body><h1>Own</h1></body></html>", base)),
        ("/theme", format!("<!doctype html><html><head><meta name=\"theme-color\" content=\"red\">{}</head><body><h1>Theme</h1></body></html>", theme_less)),
        ("/part", "<p>a part</p>".into()),
        ("/static/page.html", String::from_utf8(STATIC_PAGE.to_vec()).unwrap()),
    ]
}

// B.1: the three tags, once, in the head (or where the browser starts one: after <html>, not in a
// <header>); none on a page with its own manifest, ours but the theme colour where it has one; none
// on a part of a page, nor on what files() sends as written. Under --base the addresses carry it.
// The page grows by 151 bytes (161 under --base /shop), and its HEAD says so.
#[test]
fn b1_the_tags_go_in_the_head_once_and_only_there() {
    let d = app_dir("tags", &[("main.qwe", PAGES.as_bytes()), ("public/page.html", STATIC_PAGE), ("app.toml", b"name = \"Corner Shop\"\ntheme_color = \"#2e7d32\"\n")]);
    for (opts, base) in [(&[][..], ""), (&["--base", "/shop"][..], "/shop")] {
        let s = support::serve_with(&d.join("main.qwe"), opts);
        let (manifest_tag, apple_tag) = (format!("<link rel=\"manifest\" href=\"{}/manifest.webmanifest\">", base), format!("<link rel=\"apple-touch-icon\" href=\"{}/apple-touch-icon.png\">", base));
        let tags = format!("{}<meta name=\"theme-color\" content=\"#2e7d32\">{}", manifest_tag, apple_tag);
        assert_eq!(tags.len(), 151 + 2 * base.len());
        let theme = "<meta name=\"theme-color\" content=\"#2e7d32\">";
        for (path, want) in pages_as(&tags, &format!("{}{}", manifest_tag, apple_tag), &format!("{}{}", manifest_tag, theme), base) {
            assert_eq!(page(s.port, path), want, "{} {:?}", path, opts);
        }
        // A HEAD has no body, and the GET's length: the tags count in it (review; FEEDBACK31.md,
        // Q-393's rule).
        for path in ["/", "/bare", "/upper"] {
            let (status, h, body) = fetch(s.port, "HEAD", path, &[]);
            assert_eq!((status, body.len()), (200, 0));
            assert_eq!(header(&h, "content-length").and_then(|l| l.parse().ok()), Some(page(s.port, path).len()), "{} {:?}", path, opts);
        }
    }
}

// What must keep working: a program without app.toml is served byte for byte as before, and has no
// manifest, Apple icon or icons.
#[test]
fn b1_without_app_toml_nothing_changes() {
    let d = app_dir("none", &[("main.qwe", PAGES.as_bytes()), ("public/page.html", STATIC_PAGE)]);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    for (path, want) in pages_as("", "", "", "") {
        assert_eq!(page(s.port, path), want, "{}", path);
    }
    for path in ["/manifest.webmanifest", "/apple-touch-icon.png", "/_quillo/i/0123456789abcdef.png"] {
        assert_eq!(fetch(s.port, "GET", path, &[]).0, 404, "{}", path);
    }
}

/// The page that asks Chrome for its own manifest checks, through run.js (`data-cdp`).
const CHROME_CHECKS: &str = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  document.body.setAttribute("data-cdp-out", "");
  document.body.setAttribute("data-cdp", JSON.stringify([["Page.getAppManifest", {}], ["Page.getInstallabilityErrors", {}]]));
  for (let i = 0; i < 1000 && document.body.hasAttribute("data-cdp"); i++) await tick(20);
  const [m, inst] = JSON.parse(document.body.getAttribute("data-cdp-out"));
  document.body.dataset.result = JSON.stringify({ url: new URL(m.url).pathname, errors: m.errors, name: JSON.parse(m.data || "{}").name, installability: inst.installabilityErrors });
})();"#;

// B.1: Chrome's own checks find the app installable, with no service worker: no manifest errors and
// no installability errors, from a page with a head and from one without (the guide's form).
#[test]
fn b1_chrome_finds_the_app_installable_with_no_worker() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = app_dir("chrome", &[("main.qwe", PAGES.as_bytes()), ("app.toml", b"name = \"Quillo Corner Shop\"\ntheme_color = \"#2e7d32\"\n")]);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    for path in ["/", "/bare"] {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
        let t = support::temp_dir(&format!("f67-chrome-app{}", path.len()));
        std::fs::write(t.join("t.js"), CHROME_CHECKS).unwrap();
        let mut c = std::process::Command::new("node");
        c.arg(root.join("browser/run.js")).arg(&chrome).arg(format!("http://127.0.0.1:{}{}", s.port, path)).arg(t.join("profile")).arg("40000").arg(t.join("t.js")).env("QUILLO_PAGE_CDP", "1");
        let out = within(c, 70);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let r: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(r, serde_json::json!({ "url": "/manifest.webmanifest", "errors": [], "name": "Quillo Corner Shop", "installability": [] }), "{}", path);
    }
}

/// An app.toml with every key the web reads and every key the phones do.
const FULL: &str = r##"# The app around this program, for the phone builds and the web.
name = "Corner Shop"                  # under the icon
id = "org.example.shop"               # the phone's bundle id, never the manifest's
version = "1.2.0"
build = 7
icon = "icon.png"
main = "main.qwe"
site = "https://shop.example.com"
env.API_URL = "https://api.example.com"   # public, as everything in the app is
description = 'Groceries in ten minutes, # and all.'
theme_color = "#2e7d32"
background_color = "White"
display = "fullscreen"
start_url = "/app?from=pwa"
"##;

// B.1: the manifest says what app.toml says, the phone keys left out (`id` is the phone's bundle
// id: the manifest's is the start URL); under --base, start_url, scope, id and the icons carry it;
// with only a name, the defaults; with an empty file, Quillo's name. Revalidated (no-cache, an ETag
// and a 304); a HEAD has no body. Under --instances 2, every instance gives the same manifest and
// icons.
#[test]
fn b1_the_manifest_says_what_app_toml_says() {
    let icon = image_file(512, 512, image::ImageFormat::Png, |_, _| RED);
    let d = app_dir("manifest", &[("main.qwe", PAGES.as_bytes()), ("icon.png", &icon), ("app.toml", FULL.as_bytes())]);
    for (opts, base) in [(&[][..], ""), (&["--base", "/shop"][..], "/shop")] {
        let s = support::serve_with(&d.join("main.qwe"), opts);
        let mut m = manifest(s.port);
        let icons = m.as_object_mut().unwrap().remove("icons").unwrap();
        let start = format!("{}/app?from=pwa", base);
        assert_eq!(
            m,
            serde_json::json!({ "name": "Corner Shop", "short_name": "Corner Shop", "description": "Groceries in ten minutes, # and all.", "start_url": start, "scope": format!("{}/", base), "id": format!("{}/", base), "display": "fullscreen", "theme_color": "#2e7d32", "background_color": "White" })
        );
        let kinds: Vec<(&str, &str)> = icons.as_array().unwrap().iter().map(|i| (i["purpose"].as_str().unwrap(), i["sizes"].as_str().unwrap())).collect();
        assert_eq!(kinds, [("any", "192x192"), ("any", "512x512"), ("maskable", "512x512")]);
        assert!(icons.as_array().unwrap().iter().all(|i| i["src"].as_str().unwrap().starts_with(&format!("{}/_quillo/i/", base))), "{}", icons);
        // Revalidated: an ETag, and a 304 for it.
        let (_, h, _) = fetch(s.port, "GET", "/manifest.webmanifest", &[]);
        assert_eq!(header(&h, "cache-control"), Some("no-cache"));
        let etag = header(&h, "etag").unwrap().to_string();
        assert_eq!(fetch(s.port, "GET", "/manifest.webmanifest", &[("If-None-Match", &etag)]).0, 304);
        let (status, _, body) = fetch(s.port, "HEAD", "/manifest.webmanifest", &[]);
        assert_eq!((status, body.len()), (200, 0));
        // Only GET and HEAD; and only the icons it names.
        assert_eq!(fetch(s.port, "POST", "/manifest.webmanifest", &[]).0, 404);
        assert_eq!(fetch(s.port, "GET", "/_quillo/i/0123456789abcdef.png", &[]).0, 404);
    }
    for (text, name, short) in [("name = \"Corner Shop\"\n", "Corner Shop", "Corner Shop"), ("", "Quillo", "Quillo"), ("name = \"Café Crème Brûlée\"\n", "Café Crème Brûlée", "Café Crème")] {
        std::fs::write(d.join("app.toml"), text).unwrap();
        let s = support::serve_with(&d.join("main.qwe"), &[]);
        let mut m = manifest(s.port);
        m.as_object_mut().unwrap().remove("icons");
        assert_eq!(m, serde_json::json!({ "name": name, "short_name": short, "start_url": "/", "scope": "/", "id": "/", "display": "standalone" }), "{:?}", text);
    }
    std::fs::write(d.join("app.toml"), FULL).unwrap();
    // On a port of its own: the line it listens on says how many instances after the port.
    let (_s, port) = logged(&d, &["--instances", "2"]);
    let first = fetch(port, "GET", "/manifest.webmanifest", &[]).2;
    let src = serde_json::from_slice::<serde_json::Value>(&first).unwrap()["icons"][1]["src"].as_str().unwrap().to_string();
    let png = fetch(port, "GET", &src, &[]).2;
    for _ in 0..8 {
        assert_eq!(fetch(port, "GET", "/manifest.webmanifest", &[]).2, first);
        assert_eq!(fetch(port, "GET", &src, &[]).2, png);
    }
}

// B.1: each icon at its size and kind, as PNG: "any" as the image is (its clear parts kept, a
// rectangle centred on a clear square); maskable at 80%, centred on background_color (white when
// none); Apple's whole, on the background, with no alpha channel. From a PNG, a JPEG, and Quillo's
// own when none is named. Kept for a year at addresses named by what they are made of, which change
// with the background; Apple's at /apple-touch-icon.png, revalidated.
#[test]
fn b1_the_icons_are_made_to_size_and_kind() {
    let blue = [0, 0, 255, 255];
    let white = [255, 255, 255, 255];
    let red = image_file(600, 600, image::ImageFormat::Png, |_, _| RED);
    let half = image_file(512, 512, image::ImageFormat::Png, |x, _| if x < 256 { CLEAR } else { RED });
    let wide = image_file(800, 400, image::ImageFormat::Png, |_, _| RED);
    let jpeg = image_file(512, 512, image::ImageFormat::Jpeg, |_, _| [0, 128, 0, 255]);
    let d = app_dir("icons", &[("main.qwe", PAGES.as_bytes()), ("red.png", &red), ("half.png", &half), ("wide.png", &wide), ("green.jpg", &jpeg)]);
    let px = |img: &image::DynamicImage, x: u32, y: u32| img.to_rgba8().get_pixel(x, y).to_owned();
    let mut seen = Vec::new();
    for (toml, base) in [("icon = \"red.png\"\nbackground_color = \"#00f\"\n", "/shop"), ("icon = \"red.png\"\n", ""), ("icon = \"half.png\"\nbackground_color = \"blue\"\n", ""), ("icon = \"half.png\"\n", ""), ("icon = \"wide.png\"\n", ""), ("icon = \"green.jpg\"\n", ""), ("", "")] {
        std::fs::write(d.join("app.toml"), toml).unwrap();
        let opts: &[&str] = if base.is_empty() { &[] } else { &["--base", base] };
        let s = support::serve_with(&d.join("main.qwe"), opts);
        let got = icons(s.port, base);
        let (_, h, body) = fetch(s.port, "GET", "/apple-touch-icon.png", &[]);
        let apple = image::load_from_memory_with_format(&body, image::ImageFormat::Png).unwrap();
        assert_eq!(header(&h, "cache-control"), Some("no-cache"));
        let etag = header(&h, "etag").unwrap().to_string();
        assert_eq!(fetch(s.port, "GET", "/apple-touch-icon.png", &[("If-None-Match", &etag)]).0, 304);
        let src = manifest(s.port)["icons"][0]["src"].as_str().unwrap().strip_prefix(base).unwrap().to_string();
        let (_, h, _) = fetch(s.port, "GET", &src, &[]);
        assert_eq!(header(&h, "cache-control"), Some("public, max-age=31536000, immutable"));
        seen.push(src);
        let sizes: Vec<(&str, u32, u32, u32)> = got.iter().map(|(p, n, img)| (p.as_str(), *n, img.width(), img.height())).collect();
        assert_eq!(sizes, [("any", 192, 192, 192), ("any", 512, 512, 512), ("maskable", 512, 512, 512)], "{}", toml);
        assert_eq!((apple.width(), apple.height(), apple.color()), (180, 180, image::ColorType::Rgb8), "{}", toml);
        let maskable = &got[2].2;
        assert_eq!(maskable.color(), image::ColorType::Rgb8, "{}", toml);
        let bg = if toml.contains("background_color") { blue } else { white };
        // The safe zone: 409 of 512, from 51.
        assert!(near(px(maskable, 45, 256), bg) && near(px(maskable, 256, 45), bg) && near(px(maskable, 466, 256), bg), "{}", toml);
        match toml.split('"').nth(1).unwrap_or("") {
            "red.png" => {
                assert_eq!(got[1].2.color(), image::ColorType::Rgb8, "an icon with no clear part has no alpha channel");
                assert!(near(px(&got[0].2, 0, 0), RED) && near(px(&got[1].2, 511, 511), RED));
                assert!(near(px(maskable, 58, 256), RED) && near(px(maskable, 256, 256), RED));
                assert!(near(px(&apple, 0, 0), RED) && near(px(&apple, 179, 90), RED));
            }
            "half.png" => {
                assert_eq!(got[1].2.color(), image::ColorType::Rgba8);
                assert!(near(px(&got[1].2, 10, 256), CLEAR) && near(px(&got[1].2, 500, 256), RED));
                assert!(near(px(maskable, 100, 256), bg) && near(px(maskable, 450, 256), RED));
                assert!(near(px(&apple, 10, 90), bg) && near(px(&apple, 170, 90), RED));
            }
            "wide.png" => {
                assert!(near(px(&got[1].2, 256, 10), CLEAR) && near(px(&got[1].2, 256, 256), RED) && near(px(&got[1].2, 0, 256), RED));
                assert!(near(px(&apple, 90, 5), white) && near(px(&apple, 90, 90), RED));
            }
            "green.jpg" => {
                let [r, g, b, _] = px(&got[0].2, 96, 96).0;
                assert!(r < 12 && (116..140).contains(&g) && b < 12, "{:?}", (r, g, b));
            }
            _ => assert_eq!(got[1].2.width(), 512, "Quillo's own"),
        }
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 7, "an address for each image and background: {:?}", seen);
}

const HEAD_PAGE: &str = "route GET / = <html><head><title>Shop</title></head><body>x</body></html>\n";

// B.1: a route of the program's own at /manifest.webmanifest wins: Quillo answers nothing there and
// adds no tags (its Apple icon still answered); one at /apple-touch-icon.png: that one is the
// program's, and the page gets no Apple link. A route that takes any path (review: the guide's last
// `route ANY /*rest` for a 404 page, `files("public", path)`, `/:page`) is tried first: a real
// answer of its wins (a 2xx that is not HTML, a real public/manifest.webmanifest included, or a
// 3xx), and anything else is Quillo's; its pages get the tags. One whose type the path does not fit (`:id(int)`) does not take
// it. A guarded group at / keeps its pages, and the app's two still answer: a browser asks for them
// without cookies, and they hold nothing of anyone's.
#[test]
fn b1_a_route_of_the_programs_own_wins() {
    let toml: &[u8] = b"name = \"Shop\"\ntheme_color = \"teal\"\n";
    let d = app_dir("own-manifest", &[("main.qwe", format!("{}route GET /manifest.webmanifest = {{ name: \"mine\" }}\n", HEAD_PAGE).as_bytes()), ("app.toml", toml)]);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    assert_eq!(fetch(s.port, "GET", "/manifest.webmanifest", &[]).2, b"{\"name\":\"mine\"}");
    assert_eq!(page(s.port, "/"), "<!doctype html><html><head><title>Shop</title></head><body>x</body></html>");
    assert_eq!(fetch(s.port, "GET", "/apple-touch-icon.png", &[]).0, 200);

    let d = app_dir("own-apple", &[("main.qwe", format!("{}route GET /apple-touch-icon.png = \"mine\"\n", HEAD_PAGE).as_bytes()), ("app.toml", toml)]);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    assert_eq!(fetch(s.port, "GET", "/apple-touch-icon.png", &[]).2, b"mine");
    assert_eq!(page(s.port, "/"), "<!doctype html><html><head><title>Shop</title><link rel=\"manifest\" href=\"/manifest.webmanifest\"><meta name=\"theme-color\" content=\"teal\"></head><body>x</body></html>");
    assert_eq!(manifest(s.port)["name"], "Shop");

    let tagged = "<!doctype html><html><head><title>Shop</title><link rel=\"manifest\" href=\"/manifest.webmanifest\"><meta name=\"theme-color\" content=\"teal\"><link rel=\"apple-touch-icon\" href=\"/apple-touch-icon.png\"></head><body>x</body></html>";
    let d = app_dir("own-param", &[("main.qwe", format!("{}route GET /:page = page\n", HEAD_PAGE).as_bytes()), ("app.toml", toml)]);
    let (s, port) = logged(&d, &[]);
    assert_eq!(fetch(port, "GET", "/manifest.webmanifest", &[]).2, b"manifest.webmanifest");
    assert_eq!(page(port, "/"), tagged);
    assert!(!log_with(&s, "listening").contains("app.toml"), "nothing said at the start");

    // The guide's 404 page: a last `route ANY /*rest`. Quillo's two answer, HEAD too.
    let d = app_dir("own-rest", &[("main.qwe", format!("{}route ANY /*rest = respond(404, `Nothing at /${{rest}}`)\n", HEAD_PAGE).as_bytes()), ("app.toml", toml)]);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    assert_eq!(manifest(s.port)["name"], "Shop");
    assert_eq!(fetch(s.port, "HEAD", "/manifest.webmanifest", &[]).0, 200);
    assert_eq!(icon_at(s.port, "/apple-touch-icon.png").width(), 180);
    assert_eq!(fetch(s.port, "GET", "/elsewhere", &[]).2, b"Nothing at /elsewhere");
    assert_eq!(page(s.port, "/"), tagged);

    // files() of a folder: a manifest the program has is its own; Apple's icon, which it has not,
    // Quillo's.
    let own_manifest = br#"{"name":"From public"}"#;
    let d = app_dir("own-files", &[("main.qwe", format!("{}route GET /*path = files(\"public\", path)\n", HEAD_PAGE).as_bytes()), ("public/manifest.webmanifest", own_manifest), ("app.toml", toml)]);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    assert_eq!(fetch(s.port, "GET", "/manifest.webmanifest", &[]).2, own_manifest);
    assert_eq!(icon_at(s.port, "/apple-touch-icon.png").width(), 180);
    assert_eq!(page(s.port, "/"), tagged);

    // Review: a route that takes any path keeps the two only with a real answer, a 2xx that is not
    // HTML or a 3xx. The guide's front end (`file("dist/index.html")`, HTML for every path), a
    // guarded group's catch-all (401 to the browser's cookieless fetch) and one that fails (500)
    // give Quillo's; their own paths answer as before. A redirect is the program's.
    let spa_page: &[u8] = b"<!doctype html><html><head><title>SPA</title></head><body><div id=\"app\"></div></body></html>";
    let guarded_rest = "fn signed_in(r) = session()?.user != null ? r : respond(401, \"sign in\")\nroutes / | signed_in {\n    route ANY /*rest = \"members\"\n}\n";
    for (name, main, path, want) in [
        ("spa", "route GET /*rest = file(\"dist/index.html\")\n", "/products/42", (200, spa_page)),
        ("guarded-rest", guarded_rest, "/products/42", (401, &b"sign in"[..])),
        ("failing", "route ANY /*rest = respond(500, \"broken\")\n", "/products/42", (500, &b"broken"[..])),
    ] {
        let d = app_dir(&format!("own-{}", name), &[("main.qwe", main.as_bytes()), ("dist/index.html", spa_page), ("app.toml", toml)]);
        let s = support::serve_with(&d.join("main.qwe"), &[]);
        assert_eq!(manifest(s.port)["name"], "Shop", "{}", name);
        assert_eq!(icon_at(s.port, "/apple-touch-icon.png").width(), 180, "{}", name);
        let (status, _, body) = fetch(s.port, "GET", path, &[]);
        assert_eq!((status, &body[..]), want, "{}", name);
    }
    let d = app_dir("own-redirect", &[("main.qwe", b"route GET /*rest = redirect(\"/home\")\n"), ("app.toml", toml)]);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    for path in ["/manifest.webmanifest", "/apple-touch-icon.png"] {
        let (status, h, _) = fetch(s.port, "GET", path, &[]);
        assert!((300..400).contains(&status) && header(&h, "location") == Some("/home"), "{} {}", path, status);
    }

    let d = app_dir("own-int", &[("main.qwe", format!("{}route GET /:id(int) = id\n", HEAD_PAGE).as_bytes()), ("app.toml", toml)]);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    assert_eq!(manifest(s.port)["name"], "Shop");
    assert!(page(s.port, "/").contains("<link rel=\"manifest\""));

    let guarded = "fn signed_in(r) = session()?.user != null ? r : respond(401, \"sign in\")\nroutes / | signed_in {\n    route GET / = <html><head></head><body>x</body></html>\n}\n";
    let d = app_dir("own-guarded", &[("main.qwe", guarded.as_bytes()), ("app.toml", toml)]);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    assert_eq!(manifest(s.port)["name"], "Shop");
    assert_eq!(fetch(s.port, "GET", "/apple-touch-icon.png", &[]).0, 200);
    for path in ["/", "/elsewhere"] {
        assert_eq!(fetch(s.port, "GET", path, &[]).2, b"sign in", "{}", path);
    }
}

// B.1: the short name, when none is given: the name cut at a word to at most 12 characters, never
// mid-word; the name whole when it fits or its first word is longer. The limit and one either side,
// one long word, unicode (characters, not bytes), spaces around.
#[test]
fn b1_the_short_name_is_cut_at_a_word() {
    for (name, want) in [
        ("Corner Shop", "Corner Shop"),
        ("Twelve chars", "Twelve chars"),
        ("Thirteen char", "Thirteen"),
        ("Abcdefghijkl mno", "Abcdefghijkl"),
        ("Abcdefghijklm nop", "Abcdefghijklm nop"),
        ("Quillo Corner Shop", "Quillo"),
        ("Supercalifragilistic", "Supercalifragilistic"),
        ("Café Crème Brûlée", "Café Crème"),
        ("日本の食料品店オンラインストア", "日本の食料品店オンラインストア"),
        ("日本の 食料品店オンラインストア", "日本の"),
        ("  Padded  Name Is Long ", "Padded  Name"),
        ("A", "A"),
        ("", ""),
    ] {
        assert_eq!(quillo::pwa::short_name(name), want, "{:?}", name);
    }
}

// B.1: app.toml is read as the phone builds read it, by one reader: strings with TOML's escapes
// (`#` in one is no comment), literal strings, bare numbers, booleans and words, comments, blank
// lines, a byte-order mark and dotted keys; a key given twice is read at its first, as the phone
// builds' `sed | head -1` reads it. What cannot be read is said at its line.
#[test]
fn b1_one_reader_reads_app_toml() {
    let a = quillo::pwa::AppToml::parse("\u{FEFF}# a comment\n\nname = \"Shop #1\"   # kept\nbuild = 3\nok = true\nicon = 'a b.png'\nenv.API_URL = \"https://x\"\n\"quoted\".k = word # c\nu = \"\\u00e9\\U0001F600\\x\\t\"\n");
    let got: Vec<(&str, &str, u32)> = a.entries.iter().map(|e| (e.key.as_str(), e.value.as_str(), e.line)).collect();
    assert_eq!(got, [("name", "Shop #1", 3), ("build", "3", 4), ("ok", "true", 5), ("icon", "a b.png", 6), ("env.API_URL", "https://x", 7), ("quoted.k", "word", 8), ("u", "é😀x\t", 9)]);
    assert!(a.problems.is_empty(), "{:?}", a.problems);
    let a = quillo::pwa::AppToml::parse("name = \"open\nname = \"again\"\n[env]\njust words\nbad key = 1\nx = \"a\" b\ny = 'lit\nz =\n");
    assert_eq!(a.get("name").map(|e| e.value.as_str()), Some("open"));
    // Each at its line, a warning where the phone builds read the file all the same.
    let lines: Vec<(u32, bool)> = a.problems.iter().map(|p| (p.line, p.warning)).collect();
    assert_eq!(lines, [(1, false), (2, true), (3, true), (4, false), (5, false), (6, false), (7, false), (8, true)], "{:?}", a.problems.iter().map(|p| &p.message).collect::<Vec<_>>());
    assert!(quillo::pwa::AppToml::parse("").entries.is_empty());
    let a = quillo::pwa::AppToml::parse("v = \"a\\rb\\bc\\fd\\uZZZZ\\\"\"\n");
    assert_eq!(a.get("v").map(|e| e.value.as_str()), Some("a\rb\u{8}c\u{c}duZZZZ\""));
    // CSS colours: hex of 3, 4, 6 and 8 digits (laid on white when partly clear), names in any case.
    assert_eq!(quillo::pwa::colour("#2e7d32"), Some([0x2e, 0x7d, 0x32]));
    assert_eq!(quillo::pwa::colour("#fff"), Some([255, 255, 255]));
    assert_eq!(quillo::pwa::colour("#0008"), Some([0x77, 0x77, 0x77]));
    assert_eq!(quillo::pwa::colour("#00000000"), Some([255, 255, 255]));
    assert_eq!(quillo::pwa::colour(" Teal "), Some([0, 0x80, 0x80]));
    assert_eq!(quillo::pwa::colour("rebeccapurple"), Some([0x66, 0x33, 0x99]));
    assert_eq!(quillo::pwa::colour("transparent"), Some([255, 255, 255]));
    for bad in ["#12", "#12345", "#1234567", "#ggg", "tealish", "rgb(0,0,0)", ""] {
        assert_eq!(quillo::pwa::colour(bad), None, "{:?}", bad);
    }
}

// B.1: `quillo check` reads app.toml and says its mistakes before the program runs, each with the
// file and its line: errors stop it, warnings do not. The name's limit (30) and one either side,
// empty; the icon missing, a folder, not an image, a GIF, under 512 (a warning); display browser (a
// warning) and one that is none; colours; start_url; and what cannot be read. A file that is a
// folder or not UTF-8. `quillo run` stops on the errors, and names the file as it was given.
#[test]
fn b1_check_says_what_is_wrong_in_app_toml_at_its_line() {
    let small = image_file(256, 256, image::ImageFormat::Png, |_, _| RED);
    let big = image_file(512, 512, image::ImageFormat::Jpeg, |_, _| RED);
    let gif = image_file(600, 600, image::ImageFormat::Gif, |_, _| RED);
    // Review: decoded whole, within the run's bounds: a header that claims 20000 × 20000, and a
    // file cut short in its pixels, which only a decode finds; one past 4096 a side, a warning.
    let bomb = png_header_only(20_000, 20_000);
    let wavy = image_file(600, 600, image::ImageFormat::Png, |x, y| [(x * 7 % 256) as u8, (y * 13 % 256) as u8, ((x ^ y) % 256) as u8, 255]);
    let huge = image_file(4100, 600, image::ImageFormat::Png, |_, _| RED);
    let d = app_dir("check", &[("main.qwe", HEAD_PAGE.as_bytes()), ("small.png", &small), ("big.jpg", &big), ("anim.gif", &gif), ("notes.png", b"not an image"), ("cut.png", &small[..20]), ("folder.png/x", b""), ("bomb.png", &bomb), ("short.png", &wavy[..wavy.len() * 3 / 5]), ("huge.png", &huge)]);
    let bad = "name = \"A name that is far too long for a phone\"\nicon = \"small.png\"\ndisplay = \"browser\"\ntheme_color = \"green-ish\"\nbackground_color = \"#12345\"\nstart_url = \"app\"\nshort_name = \"x\nname = \"again\"\n[env]\ndisplay2 = \"x\" y\nlonely\n";
    std::fs::write(d.join("app.toml"), bad).unwrap();
    assert_eq!(
        check_app(&d, "main.qwe"),
        (
            false,
            vec![
                "app.toml:1: error: name must be 1 to 30 characters, as the phone builds take it: this one has 39".to_string(),
                "app.toml:2: warning: icon \"small.png\" is 256×256: Chrome wants 512×512 to offer to install the app, so it is enlarged; give one of 512×512 or more".into(),
                "app.toml:3: warning: display = \"browser\" opens the app in a tab: browsers do not offer to install it (standalone is the default)".into(),
                "app.toml:4: error: theme_color \"green-ish\" is not a CSS colour: #rgb, #rrggbb, #rrggbbaa or a name such as teal".into(),
                "app.toml:5: error: background_color \"#12345\" is not a CSS colour: #rgb, #rrggbb, #rrggbbaa or a name such as teal".into(),
                "app.toml:6: error: start_url is a path of the program's, starting with / (like /app), not \"app\"".into(),
                "app.toml:7: error: `short_name`: a string never closed (a \" missing at the end of the line)".into(),
                "app.toml:8: warning: `name` is given twice (line 1 first): line 1 is the one read, by the phone builds too".into(),
                "app.toml:9: warning: `[env]`: app.toml is flat, one `key = value` a line; the keys after it are read as if the line were not there, by the phone builds and Quillo alike (env.API_URL = \"...\" for env)".into(),
                "app.toml:10: error: `display2`: text after the value (a comment starts with #)".into(),
                "app.toml:10: warning: `display2` is read by no one: `display`?".into(),
                "app.toml:11: error: `lonely` is not `key = value`".into(),
            ]
        )
    );
    // From the folder above: the file named as the program is.
    let (ok, said) = check_app(d.parent().unwrap(), &format!("{}/main.qwe", d.file_name().unwrap().to_string_lossy()));
    assert!(!ok && said[0].starts_with(&format!("{}/app.toml:1: error: name must be", d.file_name().unwrap().to_string_lossy())), "{:?}", said);
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(["run", "main.qwe", "--port", "0"]).current_dir(&d);
    let o = within(c, 30);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("app.toml:4: error: theme_color \"green-ish\""), "{}", String::from_utf8_lossy(&o.stderr));
    // Warnings alone pass.
    std::fs::write(d.join("app.toml"), "icon = \"small.png\"\ndisplay = \"browser\"\n").unwrap();
    let (ok, said) = check_app(&d, "main.qwe");
    assert!(ok && said.len() == 2 && said.iter().all(|l| l.contains(": warning: ")), "{:?}", said);
    for (toml, want) in [
        ("name = \"123456789012345678901234567890\"\n", None),
        ("name = \"Ça-va 日本語 ✓ 123456789012345678\"\n", None),
        ("name = \"1234567890123456789012345678901\"\n", Some("app.toml:1: error: name must be 1 to 30 characters, as the phone builds take it: this one has 31")),
        // What the phone builds take is a warning: an empty name (Quillo then, on the phone too), no
        // value, a [table] line, a key given twice; and a key read by no one.
        ("name = \"\"\n", Some("app.toml:1: warning: name is empty: the app is called Quillo, as the phone builds call it")),
        ("name =\n", Some("app.toml:1: warning: `name` has no value after `=`: as if it were not given")),
        ("[android]\nname = \"Shop\"\n", Some("app.toml:1: warning: `[android]`: app.toml is flat, one `key = value` a line; the keys after it are read as if the line were not there, by the phone builds and Quillo alike (env.API_URL = \"...\" for env)")),
        ("env.API = \"a\"\nenv.API = \"b\"\n", Some("app.toml:2: warning: `env.API` is given twice (line 1 first): the phone's env() takes the last of them")),
        ("theme-color = \"teal\"\n", Some("app.toml:1: warning: `theme-color` is read by no one: `theme_color`?")),
        ("theme_colour = \"teal\"\n", Some("app.toml:1: warning: `theme_colour` is read by no one: `theme_color`?")),
        ("Name = \"Shop\"\n", Some("app.toml:1: warning: `Name` is read by no one: `name`?")),
        ("colour = \"teal\"\n", Some("app.toml:1: warning: `colour` is read by no one: app.toml takes name, icon, short_name, theme_color, background_color, display, start_url and description, and the phone builds' id, version, build, main, site and env.NAME")),
        ("icon = \"nowhere/a.png\"\n", Some("app.toml:1: error: icon \"nowhere/a.png\": no such file in the program's folder")),
        ("icon = \"bomb.png\"\n", Some("app.toml:1: error: icon \"bomb.png\" cannot be read as an image: Image size exceeds limit")),
        ("icon = \"short.png\"\n", Some("app.toml:1: error: icon \"short.png\" cannot be read as an image: unexpected end of file")),
        ("icon = \"huge.png\"\n", Some("app.toml:1: warning: icon \"huge.png\" is 4100×600: it is decoded whole, 9 MB, to make the icons; 1024×1024 is plenty")),
        ("icon = \"gone.png\"\n", Some("app.toml:1: error: icon \"gone.png\": no such file in the program's folder")),
        ("icon = \"folder.png\"\n", Some("app.toml:1: error: icon \"folder.png\": no such file in the program's folder")),
        ("icon = \"notes.png\"\n", Some("app.toml:1: error: icon \"notes.png\" is not a PNG, JPEG or WebP image")),
        ("icon = \"anim.gif\"\n", Some("app.toml:1: error: icon \"anim.gif\" is not a PNG, JPEG or WebP image")),
        ("icon = \"cut.png\"\n", Some("app.toml:1: error: icon \"cut.png\" cannot be read as an image: unexpected end of file")),
        ("icon = \"big.jpg\"\n", None),
        ("display = \"window\"\n", Some("app.toml:1: error: display is standalone, fullscreen, minimal-ui or browser, not \"window\"")),
        ("display = \"minimal-ui\"\ntheme_color = \"#0f08\"\nbackground_color = \"NAVY\"\nstart_url = \"/?from=app\"\n", None),
        ("start_url = \"//evil.example\"\n", Some("app.toml:1: error: start_url is a path of the program's, starting with / (like /app), not \"//evil.example\"")),
        (FULL.replace("icon = \"icon.png\"\n", "").as_str(), None),
    ] {
        std::fs::write(d.join("app.toml"), toml).unwrap();
        let (ok, said) = check_app(&d, "main.qwe");
        assert_eq!((ok, said), (!want.is_some_and(|w| w.contains(": error: ")), want.map(String::from).into_iter().collect::<Vec<_>>()), "{:?}", toml);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(d.join("locked.png"), &small).unwrap();
        std::fs::set_permissions(d.join("locked.png"), std::fs::Permissions::from_mode(0o000)).unwrap();
        // Not as root, who reads it anyway.
        if std::fs::read(d.join("locked.png")).is_err() {
            std::fs::write(d.join("app.toml"), "icon = \"locked.png\"\n").unwrap();
            let (ok, said) = check_app(&d, "main.qwe");
            assert!(!ok && said.len() == 1 && said[0].starts_with("app.toml:1: error: icon \"locked.png\" cannot be read: Permission denied"), "{:?}", said);
        }
    }
    std::fs::write(d.join("app.toml"), b"name = \"\xff\"\n").unwrap();
    assert_eq!(check_app(&d, "main.qwe"), (false, vec!["app.toml: error: cannot read it: not UTF-8 text".to_string()]));
    std::fs::remove_file(d.join("app.toml")).unwrap();
    std::fs::create_dir(d.join("app.toml")).unwrap();
    let (ok, said) = check_app(&d, "main.qwe");
    assert!(!ok && said.len() == 1 && said[0].starts_with("app.toml: error: cannot read it: "), "{:?}", said);
}

// B.1: run without the check, what app.toml gets wrong is left out, and the start says what of the
// icon: a missing one, or one that is no image, gives Quillo's; one that says it is a PNG and is not
// one is found when made, and gives Quillo's too. A colour that is none and a display that is none
// are left out; an empty name is Quillo's. An app.toml that is a folder: said, and nothing served.
#[test]
fn b1_run_unchecked_what_is_wrong_is_left_out_and_said() {
    let mut broken = image_file(512, 512, image::ImageFormat::Png, |_, _| RED);
    let n = broken.len();
    broken[n - 60..n - 12].fill(7);
    let d = app_dir("unchecked", &[("main.qwe", HEAD_PAGE.as_bytes()), ("notes.png", b"not an image"), ("broken.png", &broken)]);
    for (icon, said) in [("gone.png", "gone.png: No such file or directory (os error 2): Quillo's icon is used"), ("notes.png", "notes.png is not a PNG, JPEG or WebP image: Quillo's icon is used"), ("broken.png", "quillo: app.toml: icon: ")] {
        std::fs::write(d.join("app.toml"), format!("name = \"\"\nicon = \"{}\"\ntheme_color = \"green-ish\"\nbackground_color = \"#12345\"\ndisplay = \"window\"\nstart_url = \"app\"\n", icon)).unwrap();
        let (s, port) = logged(&d, &["--no-check"]);
        let mut m = manifest(port);
        let src = m["icons"][1]["src"].as_str().unwrap().to_string();
        m.as_object_mut().unwrap().remove("icons");
        assert_eq!(m, serde_json::json!({ "name": "Quillo", "short_name": "Quillo", "start_url": "/", "scope": "/", "id": "/", "display": "standalone" }), "{}", icon);
        assert_eq!(page(port, "/"), "<!doctype html><html><head><title>Shop</title><link rel=\"manifest\" href=\"/manifest.webmanifest\"><link rel=\"apple-touch-icon\" href=\"/apple-touch-icon.png\"></head><body>x</body></html>");
        assert_eq!(icon_at(port, &src).width(), 512);
        assert!(log_with(&s, said).contains(said), "{}: {}", icon, log_with(&s, said));
    }
    std::fs::remove_file(d.join("app.toml")).unwrap();
    std::fs::create_dir(d.join("app.toml")).unwrap();
    let (s, port) = logged(&d, &["--no-check"]);
    assert_eq!(fetch(port, "GET", "/manifest.webmanifest", &[]).0, 404);
    assert!(log_with(&s, "app.toml: ").contains("app.toml: Is a directory"), "{}", log_with(&s, "app.toml: "));
}

// B.1: under --watch, app.toml is one of the program's files: made, changed or taken away, the
// program loads again with it; and so is the icon it names.
#[test]
fn b1_watch_reloads_on_an_app_toml_change() {
    let one = image_file(512, 512, image::ImageFormat::Png, |_, _| RED);
    let d = app_dir("watch", &[("main.qwe", HEAD_PAGE.as_bytes()), ("a.png", &one)]);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let until = |what: &str, f: &dyn Fn() -> bool| {
        let end = Instant::now() + Duration::from_secs(15);
        while !f() {
            assert!(Instant::now() < end, "{}", what);
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let name = || {
        let (status, _, body) = fetch(s.port, "GET", "/manifest.webmanifest", &[]);
        (status == 200).then(|| serde_json::from_slice::<serde_json::Value>(&body).unwrap()["name"].as_str().unwrap_or("").to_string())
    };
    assert_eq!(name(), None);
    assert!(!page(s.port, "/").contains("rel=\"manifest\""));
    std::fs::write(d.join("app.toml"), "name = \"One\"\n").unwrap();
    until("made", &|| name().as_deref() == Some("One"));
    assert!(page(s.port, "/").contains("<link rel=\"manifest\" href=\"/manifest.webmanifest\">"));
    std::fs::write(d.join("app.toml"), "name = \"Two\"\nicon = \"a.png\"\n").unwrap();
    until("changed", &|| name().as_deref() == Some("Two"));
    let src = || manifest(s.port)["icons"][0]["src"].as_str().unwrap().to_string();
    let before = src();
    std::fs::write(d.join("a.png"), image_file(512, 512, image::ImageFormat::Png, |_, _| [0, 0, 255, 255])).unwrap();
    until("the icon changed", &|| src() != before);
    assert!(near(icon_at(s.port, &src()).to_rgba8().get_pixel(96, 96).to_owned(), [0, 0, 255, 255]));
    // More icons than are kept (16), each made and served as asked.
    for bg in ["red", "green", "navy", "teal", "olive"] {
        let was = src();
        std::fs::write(d.join("app.toml"), format!("name = \"Two\"\nicon = \"a.png\"\nbackground_color = \"{}\"\n", bg)).unwrap();
        until("the background changed", &|| src() != was);
        for i in manifest(s.port)["icons"].as_array().unwrap() {
            assert_eq!(icon_at(s.port, i["src"].as_str().unwrap()).width(), if i["sizes"] == "192x192" { 192 } else { 512 });
        }
        assert_eq!(fetch(s.port, "GET", "/apple-touch-icon.png", &[]).0, 200);
    }
    std::fs::remove_file(d.join("app.toml")).unwrap();
    until("taken away", &|| name().is_none());
    assert!(!page(s.port, "/").contains("rel=\"manifest\""));
}

// B.1: the phone builds' keys and env.X do not disturb the web's reader, and mobile/app-config.sh
// (sourced with `prog` set, as ios/build.sh does) still reads its keys from a file that has the
// web's: the same name and icon; `short_name`, the colours and `start_url` unread there. `env.X` is
// what the phone's `env()` gives, by the same reader.
#[test]
fn b1_the_phone_builds_still_read_their_keys() {
    let icon = image_file(512, 512, image::ImageFormat::Png, |_, _| RED);
    let d = app_dir("phone", &[("main.qwe", HEAD_PAGE.as_bytes()), ("icon.png", &icon), ("app.toml", format!("short_name = \"Shop\"\n{}", FULL).as_bytes())]);
    let root = env!("CARGO_MANIFEST_DIR");
    let script = format!("prog='{}'; . '{}/mobile/app-config.sh'; echo \"$APP_NAME|$APP_ID|$APP_VERSION|$APP_BUILD|$APP_ICON|$APP_MAIN|$APP_SITE\"", d.display(), root);
    let mut c = std::process::Command::new("sh");
    c.args(["-c", &script, &format!("{}/mobile/ios/build.sh", root)]);
    let o = within(c, 30);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(String::from_utf8_lossy(&o.stdout), format!("Corner Shop|org.example.shop|1.2.0|7|{}/icon.png|main.qwe|https://shop.example.com\n", d.display()));
    let env = quillo::mobile::app_env(&d.join("app.toml"));
    assert_eq!(env.into_iter().collect::<Vec<_>>(), [("API_URL".to_string(), "https://api.example.com".to_string())]);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    let m = manifest(s.port);
    assert_eq!((m["name"].as_str(), m["short_name"].as_str(), m["id"].as_str()), (Some("Corner Shop"), Some("Shop"), Some("/")));
    for phone in ["version", "build", "main", "site", "env", "API_URL", "org.example.shop"] {
        assert!(!m.to_string().contains(phone), "{}: {}", phone, m);
    }
}

// ---- Slices 1 and 2, the review's findings ------------------------------------------------------

/// Runs the --watch dev script (`script`, as a page carries it) in Node's vm, a page load at a time,
/// with a service worker's cache played by `pages` (the version each load carries) and `fetched`
/// (what a fetch of itself gets): the review's harness, each case's loads, warnings, `quillo:hmr`
/// events and what was left in sessionStorage.
const HARNESS: &str = r#"
const vm = require("vm");
const served = @SCRIPT@;
const tmpl = served.replace(/let v='\d+',h='[0-9a-f]+'/, "let v='@V@',h='@H@'").replace(/HOT=[01]/, "HOT=@HOT@");
if (tmpl === served) throw new Error("not the dev script: " + served.slice(0, 80));
async function run({ pages, fetched, server, hot, controller = true }) {
  const store = new Map();
  const sessionStorage = { getItem: (k) => (store.has(k) ? store.get(k) : null), setItem: (k, v) => store.set(k, String(v)), removeItem: (k) => store.delete(k) };
  let loads = 0;
  const warned = [], events = [];
  while (loads < 20) {
    const ver = pages[Math.min(loads, pages.length - 1)];
    loads++;
    let reloaded = false;
    const pending = [];
    const ctx = {
      sessionStorage, console: { warn: (m) => warned.push(m), error: () => {} },
      navigator: { serviceWorker: { controller: controller ? {} : null } },
      location: { pathname: "/", search: "", href: "http://x/", origin: "http://x", reload: () => { reloaded = true; } },
      document: { documentElement: { hasAttribute: () => false, appendChild() {} }, createElement: () => ({ attachShadow: () => ({ set innerHTML(_) {}, querySelector: () => ({}) }), shadowRoot: { querySelector: () => ({}) }, remove() {} }), querySelector: () => null, querySelectorAll: () => [], dispatchEvent: (e) => events.push(JSON.parse(e.detail)) },
      CustomEvent: class { constructor(n, o) { this.detail = o.detail; } },
      Event: class {}, setTimeout: () => {}, Symbol, JSON, Object, URL, Promise, Date, String,
      WebSocket: class { constructor() { pending.push(() => { this.onopen && this.onopen(); this.onmessage && this.onmessage({ data: String(server) }); }); } },
      fetch: async () => ({ ok: true, headers: { get: () => null }, text: async () => `<script>/*quillo-dev*/(()=>{let v='${fetched}',h='abc'` }),
    };
    ctx.window = ctx;
    vm.createContext(ctx);
    vm.runInContext(tmpl.replace("@V@", String(ver)).replace("@H@", "abc").replace("@HOT@", hot ? "1" : "0"), ctx);
    for (const f of pending) f();
    for (let i = 0; i < 20; i++) await new Promise((r) => setImmediate(r));
    if (!reloaded) break;
  }
  return { loads, warned: warned.length, events: events.map((e) => e.what + ":" + e.why + ":" + e.version), left: store.size };
}
(async () => {
  const cases = @CASES@;
  const out = [];
  for (const c of cases) out.push(await run(c));
  console.log(JSON.stringify(out));
})();
"#;

// Review, Q-957: the dev script took versions to only rise, and a restart counts from 1 again. A
// page a cache-first worker kept at version 7, the server started again at 1: --watch=reload
// reloaded without end, and --watch said nothing and let saves go by. Now by equality: the version
// a reload is from is kept with the one it is for, and a page that comes back with the version it
// reloaded from, a worker controlling it, came from a cache: one reload, then it stops and says why.
// The review's cases and its harness, on the script a page carries; and what must keep working: a
// reload that brings the new version (a save, or a restart), and HMR keeping the page.
#[test]
fn q957_review_versions_are_compared_by_equality_a_restart_included() {
    let d = dir("harness", "route GET / = <html><body>x</body></html>\n");
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let html = page(s.port, "/");
    let at = html.find("<script>/*quillo-dev*/").expect("the dev script") + "<script>".len();
    let script = &html[at..at + html[at..].find("</script>").unwrap()];
    let cases = serde_json::json!([
        { "pages": [1], "fetched": 1, "server": 2, "hot": false },
        { "pages": [1], "fetched": 1, "server": 2, "hot": true },
        { "pages": [7], "fetched": 7, "server": 1, "hot": false },
        { "pages": [7], "fetched": 7, "server": 1, "hot": true },
        { "pages": [7], "fetched": 7, "server": 2, "hot": false },
        { "pages": [1, 2], "fetched": 2, "server": 2, "hot": false },
        { "pages": [7, 1], "fetched": 1, "server": 1, "hot": false },
        { "pages": [1], "fetched": 2, "server": 2, "hot": true },
        { "pages": [7], "fetched": 1, "server": 1, "hot": true },
    ]);
    let js = HARNESS.replace("@SCRIPT@", &serde_json::to_string(script).unwrap()).replace("@CASES@", &cases.to_string());
    let file = d.join("harness.cjs");
    std::fs::write(&file, js).unwrap();
    let mut c = std::process::Command::new("node");
    c.arg(&file);
    let o = within(c, 60);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let got: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let cached = |why: &str, v: &str| serde_json::json!({ "loads": 2, "warned": 1, "events": [format!("reload:{}:{}", why, v), format!("cached:{}:{}", why, v)], "left": 0 });
    assert_eq!(
        got,
        serde_json::json!([
            cached("off", "2"),
            cached("cache", "2"),
            cached("off", "1"),
            cached("cache", "1"),
            cached("off", "2"),
            { "loads": 2, "warned": 0, "events": ["reload:off:2"], "left": 0 },
            { "loads": 2, "warned": 0, "events": ["reload:off:1"], "left": 0 },
            { "loads": 1, "warned": 0, "events": ["kept::2"], "left": 0 },
            { "loads": 1, "warned": 0, "events": ["kept::1"], "left": 0 },
        ])
    );
}

// Review, Q-957 in Chrome: the report's page under its cache-first worker, kept at version 3, and the
// server started again (version 1) on the same port: one reload, then the page stops and says why,
// under --watch and --watch=reload.
#[test]
fn q957_review_a_server_started_again_under_a_worker_stops_and_says_why() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    for (opts, why) in [(&["--watch"][..], "cache"), (&["--watch=reload"][..], "off")] {
        let d = dir(&format!("restart{}", why), &sw_program("/", "One"));
        std::fs::write(d.join("sw.js"), CACHE_FIRST).unwrap();
        std::fs::write(d.join("t.js"), PAGE_SCRIPT.replace("@WORKER@", "true")).unwrap();
        let (first, port) = logged(&d, opts);
        let mut first = Some(first);
        // Two saves: the page the worker keeps is version 3.
        let version = || {
            let html = page(port, "/");
            html.split("let v='").nth(1).and_then(|r| r.split('\'').next()).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0)
        };
        for n in 1..=2 {
            std::fs::write(d.join("main.qwe"), format!("{}// save {}\n", sw_program("/", "One"), n)).unwrap();
            let end = Instant::now() + Duration::from_secs(15);
            while version() != n + 1 {
                assert!(Instant::now() < end, "save {} did not load", n);
                std::thread::sleep(Duration::from_millis(30));
            }
        }
        let (chrome, url) = (chrome.clone(), format!("http://127.0.0.1:{}/", port));
        let page_run = std::thread::spawn(move || in_chrome(&chrome, &url, "void 0"));
        let end = Instant::now() + Duration::from_secs(30);
        while support::send(port, "GET", "/stage", &[], "").body != "1" {
            assert!(Instant::now() < end && !page_run.is_finished(), "the page did not reach its stage");
            std::thread::sleep(Duration::from_millis(30));
        }
        drop(first.take());
        let _again = logged_at(&d, port, opts);
        assert_eq!(version(), 1, "started again at 1");
        let r = page_run.join().unwrap();
        assert_eq!(
            r,
            serde_json::json!({ "loads": 1, "h1": "One", "controlled": true, "said": CACHED, "warned": [CACHED], "hmr": [["reload", why], ["cached", why]], "left": 0 }),
            "{:?}",
            opts
        );
    }
}

/// A PNG of `w` × `h` that says so in its header and holds next to nothing: what a decoder must
/// refuse before it makes room for the pixels.
fn png_header_only(w: u32, h: u32) -> Vec<u8> {
    let chunk = |kind: &[u8], data: &[u8]| {
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(data);
        let mut c = (data.len() as u32).to_be_bytes().to_vec();
        c.extend_from_slice(kind);
        c.extend_from_slice(data);
        c.extend_from_slice(&crc.sum().to_be_bytes());
        c
    };
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(chunk(b"IHDR", &ihdr));
    png.extend(chunk(b"IDAT", &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]));
    png.extend(chunk(b"IEND", b""));
    png
}


// Review: many first requests for an app's icons at once, a 4000 × 4000 source: one decode makes the
// four, and the requests that come meanwhile wait for it. Sixteen decodes took 1.8 GB that was never
// given back, and any visitor can ask: the addresses are in the manifest. Each answer is the icon.
#[test]
fn b1_review_many_first_requests_for_the_icons_make_them_once() {
    let big = image_file(4000, 4000, image::ImageFormat::Png, |x, y| [(x % 256) as u8, (y % 256) as u8, 90, 255]);
    let d = app_dir("one-decode", &[("main.qwe", HEAD_PAGE.as_bytes()), ("big.png", &big), ("app.toml", b"name = \"Big\"\nicon = \"big.png\"\n")]);
    let s = support::serve_with(&d.join("main.qwe"), &[]);
    let mut srcs: Vec<(String, u32)> = manifest(s.port)["icons"].as_array().unwrap().iter().map(|i| (i["src"].as_str().unwrap().to_string(), i["sizes"].as_str().unwrap().split('x').next().unwrap().parse().unwrap())).collect();
    srcs.push(("/apple-touch-icon.png".into(), 180));
    let port = s.port;
    let asks: Vec<_> = (0..16)
        .map(|i| {
            let (src, size) = srcs[i % srcs.len()].clone();
            std::thread::spawn(move || (icon_at(port, &src).width(), size))
        })
        .collect();
    for a in asks {
        let (got, want) = a.join().unwrap();
        assert_eq!(got, want);
    }
    let rss = std::process::Command::new("ps").args(["-o", "rss=", "-p", &s.pid().to_string()]).output().unwrap();
    let kb: u64 = String::from_utf8_lossy(&rss.stdout).trim().parse().unwrap();
    assert!(kb < 400 * 1024, "{} MB", kb / 1024);
}

// Review: the phone builds read their keys as Quillo reads them, so the app and the site show one
// name: each of the review's files, and more, through mobile/app-config.sh's `app_value` (sourced as
// ios/build.sh does) and through Quillo's reader, key by key. Before, the shell gave `"Shop` for
// "Shop #1", kept the quotes before trailing spaces or a CR, gave `'A` for 'A#B' and kept the
// backslash of "Tom\"s".
#[test]
fn b1_review_the_phone_builds_and_quillo_read_app_toml_alike() {
    let icon = image_file(512, 512, image::ImageFormat::Png, |_, _| RED);
    let d = app_dir("readers", &[("main.qwe", HEAD_PAGE.as_bytes()), ("icon.png", &icon), ("app.toml", FULL.as_bytes())]);
    let files = [
        "name = \"Shop #1\"\n",
        "name = \"Corner Shop\"   \n",
        "name = \"Corner Shop\"\r\nid = \"org.example.crlf\"\r\nbuild = 3\r\n",
        "name = 'A#B'\n",
        "name = \"Tom\\\"s\"\n",
        "name = \"Back\\\\slash\"\n",
        "name = \"Caf\\u00e9 \\U0001F600 \\uZZZZ \\uD800\"\n",
        "name = \"a\\tb\"\n",
        "name = Bare words   # a comment\nversion = 1.2.3\n",
        "name = \"x\" # a comment with \"quotes\" and 'more'\n",
        "\u{FEFF}name = \"Marked\"\n",
        "name = \"\"\n",
        "name =\n",
        "name = \"First\"\nname = \"Second\"\n",
        "[android]\nname = \"Under a table\"\n",
        "\"name\" = \"Quoted key\"\nsite = 'https://shop.example.com'\n",
        "name = \"never closed\n",
        "name = 'never closed\n",
        "  name  =  \"Spaced\"  \nmain = \"apps/phone.qwe\"\n",
        "short_name = \"S\"\nnames = \"no\"\nname = \"N\"\nicon = \"art/i.png\"\n",
        "# name = \"commented\"\nname = \"日本の店\"\n",
        FULL,
    ];
    let keys = ["name", "id", "version", "build", "icon", "main", "site"];
    let root = env!("CARGO_MANIFEST_DIR");
    for (n, text) in files.iter().enumerate() {
        let case = d.join(format!("case{}.toml", n));
        std::fs::write(&case, text).unwrap();
        let script = format!(
            "prog='{}'; . '{}/mobile/app-config.sh'; APP_CONFIG='{}'; for k in {}; do printf '%s\\037' \"$(app_value $k)\"; done",
            d.display(),
            root,
            case.display(),
            keys.join(" ")
        );
        let mut c = std::process::Command::new("sh");
        c.args(["-c", &script, &format!("{}/mobile/ios/build.sh", root)]);
        let o = within(c, 30);
        assert!(o.status.success(), "{:?}: {}", text, String::from_utf8_lossy(&o.stderr));
        let shell: Vec<String> = String::from_utf8(o.stdout).unwrap().split('\u{1f}').map(String::from).collect();
        let app = quillo::pwa::AppToml::parse(text);
        let ours: Vec<String> = keys.iter().map(|k| app.get(k).map_or(String::new(), |e| e.value.clone())).collect();
        assert_eq!(&shell[..keys.len()], &ours[..], "{:?}", text);
    }
    // The name the app gets is the manifest's: "Shop #1" on both, Quillo for an empty one.
    for (text, name) in [("name = \"Shop #1\"\n", "Shop #1"), ("name = \"\"\n", "Quillo")] {
        std::fs::write(d.join("app.toml"), format!("{}id = \"org.example.shop\"\nicon = \"icon.png\"\n", text)).unwrap();
        let script = format!("prog='{}'; . '{}/mobile/app-config.sh'; printf '%s' \"$APP_NAME\"", d.display(), root);
        let mut c = std::process::Command::new("sh");
        c.args(["-c", &script, &format!("{}/mobile/ios/build.sh", root)]);
        let o = within(c, 30);
        assert_eq!(String::from_utf8_lossy(&o.stdout), name);
        let s = support::serve_with(&d.join("main.qwe"), &[]);
        assert_eq!(manifest(s.port)["name"], name);
    }
}

// Review: the phone's env() is as it was: an `env.NAME` given twice is its last, as a `.env`'s is,
// and `env.X =` is "" (the reader of slice 2 gave the first, and nothing); the escapes and the
// byte-order mark the one reader adds stay. `quillo check` warns of the twice-given one.
#[test]
fn b1_review_the_phones_env_keeps_the_last_and_the_empty() {
    let d = app_dir("env-last", &[("app.toml", "\u{FEFF}env.A = \"1\"\nenv.A = \"2\"\nenv.B =\nenv.C = \"\\u00e9\"\nenv.D = # nothing\n".as_bytes())]);
    let mut got: Vec<(String, String)> = quillo::mobile::app_env(&d.join("app.toml")).into_iter().collect();
    got.sort();
    let want: Vec<(String, String)> = [("A", "2"), ("B", ""), ("C", "é"), ("D", "")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    assert_eq!(got, want);
}

// Review: an icon named in a folder that is not there: the watcher could not watch it and looked for
// changes every 50 ms instead; now that path is left out, and the program still loads again on a
// change. `quillo check` says the icon is missing (b1_check_says_what_is_wrong_in_app_toml_at_its_line).
#[test]
fn b1_review_an_icon_in_a_missing_folder_leaves_the_watcher_as_it_was() {
    let d = app_dir("watch-missing", &[("main.qwe", HEAD_PAGE.as_bytes()), ("app.toml", b"name = \"One\"\nicon = \"nowhere/a.png\"\n")]);
    let (s, port) = logged(&d, &["--watch", "--no-check"]);
    assert_eq!(manifest(port)["name"], "One");
    std::fs::write(d.join("app.toml"), "name = \"Two\"\nicon = \"nowhere/a.png\"\n").unwrap();
    let end = Instant::now() + Duration::from_secs(15);
    while manifest(port)["name"] != "Two" {
        assert!(Instant::now() < end, "app.toml's change did not load");
        std::thread::sleep(Duration::from_millis(50));
    }
    let log = log_with(&s, "reloaded");
    assert!(!log.contains("every 50 ms"), "{}", log);
}

// ---- B.2: Quillo's service worker, for routes tagged `offline` (slice 3) ------------------------

/// A server the page tells where it is (`fetch(STAGE + "/stage/<n>")`, no CORS needed), which
/// stays up while the program's is stopped and started again.
const STAGES: &str = "let stage = 0\nroute GET /stage/:n(int) {\n    stage = n\n    \"ok\"\n}\nroute GET /stage = stage\n";

/// `in_chrome` with a deadline for pages that wait on the server's stages, and the stage
/// server's address in the script (`@STAGE@`).
fn in_chrome_staged(chrome: &Path, url: &str, script: &str, stages: u16) -> serde_json::Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let script = script.replace("@STAGE@", &format!("http://127.0.0.1:{}", stages));
    let d = support::temp_dir(&format!("f67-sw-{}", script.len()));
    std::fs::write(d.join("t.js"), &script).unwrap();
    let mut c = std::process::Command::new("node");
    c.arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(d.join("profile")).arg("60000").arg(d.join("t.js"));
    let out = within(c, 90);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

/// Waits until the page says it reached `n` (or finished, which fails the test with its answer).
fn stage_reached(stages: u16, n: u32, page: &std::thread::JoinHandle<serde_json::Value>) {
    let end = Instant::now() + Duration::from_secs(60);
    while support::send(stages, "GET", "/stage", &[], "").body != n.to_string() {
        assert!(Instant::now() < end && !page.is_finished(), "the page did not reach stage {}", n);
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// What the scripts share: frames loaded through the worker, the caches' contents, and waits
/// on the server (`down`, `up`).
const SW_KIT: &str = r#"
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const sw = navigator.serviceWorker;
  const stage = (n) => fetch("@STAGE@/stage/" + n, { mode: "no-cors" });
  const frame = (path) => new Promise((ok) => {
    const f = document.createElement("iframe");
    f.onload = () => ok(f);
    f.src = path;
    document.body.appendChild(f);
  });
  // A page as the worker gives it: its heading, `data-kept`, and whether it is the offline page.
  const look = async (path) => {
    const f = await frame(path), d = f.contentDocument, h = d.documentElement;
    const kept = h.getAttribute("data-kept");
    return { h1: (d.querySelector("h1") || {}).textContent || null, kept: kept === null ? null : /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/.test(kept) ? "iso" : kept, offline: h.hasAttribute("data-quillo-offline"), title: d.title, frame: f };
  };
  const shown = (r) => { const o = Object.assign({}, r); delete o.frame; return o; };
  // Each cache, its name's version as V and its session as S, with the paths it keeps (hashes as H);
  // not the list of the sessions dropped, nor the mark that the version's pages are all kept.
  const snap = async () => {
    const out = {};
    for (const n of await caches.keys()) {
      // Opened only if it is still there: opening one the worker just dropped makes it again,
      // empty, and it was seen on every look after (found in the third review's runs).
      if (n.endsWith(":gone") || !(await caches.has(n))) continue;
      const k = (await (await caches.open(n)).keys()).map((r) => decodeURI(new URL(r.url).pathname).replace(/\/[0-9a-f]{16}\./, "/H.")).filter((p) => !p.endsWith("/_quillo/sw-state")).sort();
      if (k.length) out[n.replace(/:[0-9a-f]{16}/, ":V").replace(/:V:.+$/, ":V:S")] = k;
    }
    return out;
  };
  // What the worker keeps of a page it answered is written after it gives the page (its
  // `waitUntil`), which on CI's debug build came after the frame's load, and a look then read the
  // cache without the page: read once it has held for 100 ms, 2 s at most (a shown bug in the
  // tests, run 44022675220; branch pwa).
  const kept = async () => {
    let was = JSON.stringify(await snap());
    for (let i = 0; i < 20; i++) {
      await tick(100);
      const now = JSON.stringify(await snap());
      if (now === was) break;
      was = now;
    }
    return JSON.parse(was);
  };
  // The sessions the worker dropped, by name.
  const dropped = async (base = "") => { const r = await (await caches.open("quillo:" + base + ":gone")).match(base + "/_quillo/sw-dropped"); return r ? await r.json() : []; };
  const tagNow = () => (/(?:^|;\s*)quillo_tag=([^;]*)/.exec(document.cookie) || [])[1] || "-";
  const down = async () => { for (let i = 0; i < 600; i++) { try { await fetch("/ping", { cache: "no-store" }); } catch (_) { return true; } await tick(25); } return false; };
  const up = async (want) => { for (let i = 0; i < 600; i++) { try { const r = await fetch("/ping", { cache: "no-store" }); if ((await r.text()) === want) return true; } catch (_) {} await tick(25); } return false; };
  // `look`, once the page in the frame is as `ok` wants it (a gate that loads it again).
  const lookUntil = async (path, ok) => {
    const f = await frame(path);
    for (let i = 0; i < 200; i++) { try { if (f.contentDocument.readyState === "complete" && ok(f.contentDocument)) break; } catch (_) {} await tick(25); }
    const d = f.contentDocument, h = d.documentElement, k = h.getAttribute("data-kept");
    return { h1: (d.querySelector("h1") || {}).textContent || null, kept: k === null ? null : /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/.test(k) ? "iso" : k, offline: h.hasAttribute("data-quillo-offline"), title: d.title, session: h.getAttribute("data-kept-session"), hidden: h.hasAttribute("hidden") };
  };
  const controlled = async () => { await sw.ready; for (let i = 0; i < 300 && !sw.controller; i++) await tick(20); return !!sw.controller; };
"#;

/// The shop: offline routes with and without parameters, one counting its loads, one with a
/// component, one that calls a server fn; a page that is not kept; app.toml's name, in unicode.
const SHOP: &str = r#"let loads = 0
server fn add(n: int) -> int = n + 1
component Counter() {
    state n = 0
    <button id="inc" onclick={() => { n += 1 }}>clicked {n}</button>
}
component Adder() {
    state got = ""
    <button id="call" onclick={() => {
        let r = add(1)
        got = is_err(r) ? "unreachable" : str(r)
    }}>add {got}</button>
}
route offline GET / = <html><head><title>Home</title></head><body><h1>Home</h1><Adder/></body></html>
route offline GET /menu {
    loads += 1
    <html><body><h1>Menu {loads}</h1></body></html>
}
route offline GET /never = <html><body><h1>Never</h1><Counter/></body></html>
route offline GET /item/:id = <html><body><h1>Item {id}</h1></body></html>
route GET /plain = <html><body><h1>Plain</h1></body></html>
route GET /ping = "pong"
"#;

const SHOP_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {};
  out.controlled = await controlled();
  const reg = await sw.getRegistration();
  out.worker = new URL(sw.controller.scriptURL).pathname;
  out.scope = new URL(reg.scope).pathname;
  // Online: the network first, each load a new page.
  out.menu = [(await look("/menu")).h1, (await look("/menu")).h1];
  const call = async () => {
    const b = document.getElementById("call"), was = b.textContent;
    b.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    b.click();
    for (let i = 0; i < 200 && b.textContent === was; i++) await tick(25);
    return b.textContent;
  };
  out.call = await call();
  out.ready = (await fetch("/_quillo/ready")).status;
  out.kept = await kept();
  await stage(1);
  out.down = await down();
  // The server is stopped.
  out.offline = {};
  for (const p of ["/menu", "/", "/never", "/item/5", "/plain", "/caf%C3%A9"]) out.offline[p] = shown(await look(p));
  const never = (await look("/never")).frame.contentDocument;
  const inc = never.getElementById("inc");
  inc.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  inc.click();
  for (let i = 0; i < 200 && inc.textContent !== "clicked 1"; i++) await tick(25);
  out.component = inc.textContent;
  out.call_offline = await call();
  const failed = async (p) => { try { await fetch(p); return false; } catch (_) { return true; } };
  out.untouched = { ready: await failed("/_quillo/ready"), ping: await failed("/ping") };
  out.kept_after = await kept();
  out.errors = window.__quilloErrors;
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// B.2: the worker registers from an offline route's page, at the program's scope, and controls
// it (`/menu`, which counts its loads, is not kept on install: the install asks as a prefetch, and
// Quillo refuses a prefetch that would change something; second review). Online, a page comes from the network each time and a server fn is answered by the server;
// what is kept is the offline routes with no parameter (kept on install, opened or not) and the
// files their pages name, nothing of /_quillo/rpc, ready or another GET. With the server stopped:
// the page opened shows its last copy, marked `data-kept`; one never opened opens too, its
// component working from the kept files; a page not kept, or with a parameter never opened, is
// the offline page with app.toml's name (unicode, escaped); a server fn says it cannot be reached
// and /_quillo/ready and other GETs fail as they would with no worker.
#[test]
fn b2_offline_pages_are_kept_and_shown_with_the_server_stopped() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = app_dir("sw-shop", &[("main.qwe", SHOP.as_bytes()), ("app.toml", "name = \"Corner <Shop> ✓\"\n".as_bytes())]);
    assert!(check_app(&d, "main.qwe").0);
    let stages = support::serve(&dir("sw-stages", STAGES).join("main.qwe"), &[]);
    let (s, port) = logged(&d, &[]);
    let (c, url, sp) = (chrome.clone(), format!("http://127.0.0.1:{}/", port), stages.port);
    let page = std::thread::spawn(move || in_chrome_staged(&c, &url, &SHOP_SCRIPT.replace("@KIT@", SW_KIT), sp));
    stage_reached(stages.port, 1, &page);
    let log = std::fs::read_to_string(&s.1).unwrap_or_default();
    drop(s);
    let r = page.join().unwrap();
    let name = "Corner <Shop> ✓";
    let offline_page = serde_json::json!({ "h1": name, "kept": null, "offline": true, "title": name });
    let files = serde_json::json!(["/", "/_quillo/b/H.js", "/_quillo/b/H.js", "/_quillo/r/H.js", "/menu", "/never"]);
    assert_eq!(
        r,
        serde_json::json!({
            "controlled": true, "worker": "/_quillo/sw.js", "scope": "/",
            "menu": ["Menu 1", "Menu 2"], "call": "add 2", "ready": 200,
            "kept": { "quillo::V": files },
            "down": true,
            "offline": {
                "/menu": { "h1": "Menu 2", "kept": "iso", "offline": false, "title": "" },
                "/": { "h1": "Home", "kept": "iso", "offline": false, "title": "Home" },
                "/never": { "h1": "Never", "kept": "iso", "offline": false, "title": "" },
                "/item/5": offline_page, "/plain": offline_page, "/caf%C3%A9": offline_page,
            },
            "component": "clicked 1", "call_offline": "add unreachable",
            "untouched": { "ready": true, "ping": true },
            "kept_after": { "quillo::V": files },
            "errors": [],
        }),
        "{}",
        log
    );
}

/// A visitor's page kept for offline, and the two ways of signing out: a form the browser sends,
/// answered with a redirect, and a server fn.
const SIGNED: &str = r#"server fn sign_out() {
    end_session()
}
component Out() {
    <button id="out" onclick={() => { sign_out() }}>out</button>
}
route GET /in/:who {
    set_session({ user: who })
    redirect("/")
}
route POST /logout {
    end_session()
    redirect("/")
}
route GET /bye {
    end_session()
    <html><body><h1>Bye</h1></body></html>
}
route offline GET / = <html><body><h1>Home</h1><Out/><form id="f" method="post" action="/logout"><button>Sign out</button></form></body></html>
route offline GET /mine = <html><body><h1>Mine {session().user ?? "nobody"}</h1></body></html>
route GET /ping = "pong"
"#;

const SIGNED_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {};
  out.controlled = await controlled();
  const until = async (f) => { for (let i = 0; i < 200; i++) { const v = await f(); if (v) return v; await tick(25); } return null; };
  const privates = async () => Object.keys(await kept()).filter((k) => k.endsWith(":S")).length;
  const signIn = async (who) => { await look("/in/" + who); return (await look("/mine")).h1; };
  out.at_install = await kept();
  out.mine = await signIn("ana");
  out.signed_in = await kept();
  // A form the browser sends, answered by a redirect: the page after it says the session is none.
  const home = (await look("/")).frame;
  await new Promise((ok) => { home.onload = ok; home.contentDocument.getElementById("f").submit(); });
  out.after_form = await until(async () => (await privates()) === 0 && (await kept()));
  out.again = await signIn("bob");
  out.privates = (await until(async () => (await privates()) === 1)) ? 1 : 0;
  // A server fn: the runtime tells the worker. Its bundle loaded first, which a loaded machine
  // took seconds for (the click waits for it, and a 5 s wait ran out in a full run).
  const b = document.getElementById("out");
  b.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  for (let i = 0; i < 400 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(25);
  b.click();
  out.after_fn = null;
  for (let i = 0; i < 400 && !out.after_fn; i++) { if ((await privates()) === 0) out.after_fn = await kept(); else await tick(25); }
  // A page that signs out in its own answer.
  await signIn("cy");
  await look("/bye");
  out.after_page = await until(async () => (await privates()) === 0 && (await kept()));
  // Signed in again, the server stopped: the visitor's page is kept for them.
  out.last = await signIn("dee");
  await stage(1);
  out.down = await down();
  out.offline = shown(await look("/mine"));
  out.errors = window.__quilloErrors;
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// B.2: a page that reads the visitor is kept apart, by the session it was made in, and dropped
// when the session ends or changes: by a form the browser sends (its redirect's page says the
// session left), by a server fn (the runtime tells the worker), by a page that ends it, and by
// signing in as another. Public pages stay. With the server stopped, the page kept for the one
// signed in is theirs.
#[test]
fn b2_a_visitors_page_is_dropped_when_the_session_ends_or_changes() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = dir("sw-signed", SIGNED);
    assert!(check(&d).0, "{:?}", check(&d));
    let stages = support::serve(&dir("sw-stages2", STAGES).join("main.qwe"), &[]);
    let (s, port) = logged(&d, &[]);
    let (c, url, sp) = (chrome.clone(), format!("http://127.0.0.1:{}/", port), stages.port);
    let page = std::thread::spawn(move || in_chrome_staged(&c, &url, &SIGNED_SCRIPT.replace("@KIT@", SW_KIT), sp));
    stage_reached(stages.port, 1, &page);
    let log = std::fs::read_to_string(&s.1).unwrap_or_default();
    drop(s);
    let r = page.join().unwrap();
    let public = serde_json::json!(["/", "/_quillo/b/H.js", "/_quillo/r/H.js"]);
    let with = |user_page: bool| if user_page { serde_json::json!({ "quillo::V": public, "quillo::V:S": ["/mine"] }) } else { serde_json::json!({ "quillo::V": public }) };
    assert_eq!(
        r,
        serde_json::json!({
            "controlled": true,
            "at_install": with(true), "mine": "Mine ana", "signed_in": with(true),
            "after_form": with(false), "again": "Mine bob", "privates": 1, "after_fn": with(false), "after_page": with(false),
            "last": "Mine dee", "down": true,
            "offline": { "h1": "Mine dee", "kept": "iso", "offline": false, "title": "" },
            "errors": [],
        }),
        "{}",
        log
    );
}

/// A policy's hash of `text`, as `'sha256-...'`.
fn policy_hash(text: &str) -> String {
    use sha2::Digest;
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let data = sha2::Sha256::digest(text.as_bytes());
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    format!("'sha256-{}'", out)
}

/// What registers the worker, at the base given, and tells it the session the browser holds.
fn register_script(base: &str) -> String {
    format!("<script>(n=>{{if(!n)return;const u=\"{0}/_quillo/sw.js\";n.register(u,{{scope:\"{0}/\"}});const c=n.controller,m=/(?:^|;\\s*)quillo_tag=([^;]*)/.exec(document.cookie);c&&c.scriptURL===new URL(u,location).href&&c.postMessage({{quillo:\"session\",who:m?m[1]:\"-\"}})}})(navigator.serviceWorker)</script>", base)
}

/// Under the guide's content policy, with nothing of its own on the page but the register script;
/// its stages its own, as the policy lets the page reach no other server.
const GUARDED: &str = "let stage = 0\nroute GET /stage/:n(int) {\n    stage = n\n    \"ok\"\n}\nroute GET /stage = stage\nsite_headers(\"/\", { \"content-security-policy\": \"default-src 'self'\" })\nroute offline GET / = <html><head><title>p</title></head><body><h1>Home</h1></body></html>\nroute offline GET /menu = <html><body><h1>Menu</h1></body></html>\nroute GET /ping = \"pong\"\n";

const WATCHED_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {}, refused = [];
  document.addEventListener("securitypolicyviolation", (e) => refused.push(e.violatedDirective));
  out.controlled = await controlled();
  // The worker keeps "/" as it installs with a prefetch, which Quillo refuses (503, tried again on
  // a later navigation) while the page's own requests run: on CI's debug build the page's bundle
  // was still coming, and one run read the cache with "/menu" alone. If "/" is not kept within a
  // second it is opened once, as a visitor would, which keeps it (a shown bug in the test; branch
  // pwa).
  const home0 = async () => ((await kept())["quillo::V"] || []).includes("/");
  for (let i = 0; i < 40 && !(await home0()); i++) await tick(25);
  if (!(await home0())) { (await look("/")).frame.remove(); for (let i = 0; i < 200 && !(await home0()); i++) await tick(25); }
  out.kept = await kept();
  const reg = await sw.getRegistration();
  await stage(1);
  out.up = await up("pong again");
  // Under --watch: the worker there takes itself away, and what Quillo's kept.
  await reg.update().catch((e) => { out.update = String(e); });
  for (let i = 0; i < 200 && (await sw.getRegistrations()).length; i++) await tick(25);
  out.registrations = (await sw.getRegistrations()).length;
  out.caches = await caches.keys();
  const home = (await look("/")).frame;
  out.registers = [...home.contentDocument.scripts].some((s) => s.textContent.includes("/_quillo/sw.js"));
  out.frame_controlled = !!home.contentWindow.navigator.serviceWorker.controller;
  out.refused = refused;
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// B.2: the register script passes the page's policy, hashed as Quillo's other scripts are, and
// the worker registers under `default-src 'self'`. Under --watch no page registers it, and
// `/_quillo/sw.js` is a worker that takes itself away with what Quillo's kept: a browser that
// registered the worker in a run without --watch has none after the next check, and no cache.
#[test]
fn b2_the_policy_lets_the_register_script_through_and_watch_takes_the_worker_away() {
    let d = dir("sw-guarded", GUARDED);
    let (s, port) = logged(&d, &[]);
    let (status, h, body) = fetch(port, "GET", "/", &[]);
    let body = String::from_utf8(body).unwrap();
    assert_eq!(status, 200);
    let js = register_script("");
    assert!(body.contains(&format!("{}</body>", js)), "{}", body);
    let inner = js.trim_start_matches("<script>").trim_end_matches("</script>");
    assert_eq!(header(&h, "content-security-policy"), Some(format!("default-src 'self'; script-src 'self' {}", policy_hash(inner)).as_str()));
    drop(s);
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    // Started again under --watch, and (the review) without --watch with its offline routes gone:
    // either way the worker there takes itself away.
    for (opts, src) in [(&["--watch"][..], GUARDED.to_string()), (&[][..], GUARDED.replace("route offline", "route"))] {
        std::fs::write(d.join("main.qwe"), GUARDED).unwrap();
        let s = logged_at(&d, port, &[]);
        let (c, url) = (chrome.clone(), format!("http://127.0.0.1:{}/", port));
        let page = std::thread::spawn(move || in_chrome_staged(&c, &url, &WATCHED_SCRIPT.replace("@KIT@", SW_KIT), port));
        stage_reached(port, 1, &page);
        drop(s);
        std::fs::write(d.join("main.qwe"), src.replace("\"pong\"", "\"pong again\"")).unwrap();
        let s = logged_at(&d, port, opts);
        // No register script and no marks; the worker is the one that goes.
        let (_, h, body) = fetch(port, "GET", "/", &[]);
        let body = String::from_utf8(body).unwrap();
        assert!(!body.contains("/_quillo/sw.js") && header(&h, "x-quillo-offline").is_none() && header(&h, "x-quillo-session").is_none(), "{:?} {}", h, body);
        let (status, h, worker) = fetch(port, "GET", "/_quillo/sw.js", &[]);
        assert_eq!((status, header(&h, "service-worker-allowed")), (200, Some("/")));
        assert!(String::from_utf8_lossy(&worker).contains("self.registration.unregister()"));
        let r = page.join().unwrap();
        assert_eq!(
            r,
            serde_json::json!({
                "controlled": true, "kept": { "quillo::V": ["/", "/menu"] }, "up": true,
                "registrations": 0, "caches": [], "registers": false, "frame_controlled": false, "refused": [],
            }),
            "{:?}: {}",
            opts,
            std::fs::read_to_string(&s.1).unwrap_or_default()
        );
    }
}

/// A program in two versions: the second has another offline route and another heading.
fn versioned(n: u32) -> String {
    let extra = if n > 1 { "route offline GET /extra = <html><body><h1>Extra</h1></body></html>\n" } else { "" };
    format!("route offline GET / = <html><body><h1>Version {0}</h1></body></html>\nroute offline GET /menu = <html><body><h1>Menu</h1></body></html>\n{1}route GET /ping = \"pong {0}\"\n", n, extra)
}

const VERSION_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {};
  out.controlled = await controlled();
  const reg = await sw.getRegistration(), first = reg.active;
  let changes = 0;
  sw.addEventListener("controllerchange", () => changes++);
  const before = await caches.keys();
  out.before = await kept();
  await stage(1);
  out.up = await up("pong 2");
  await reg.update();
  for (let i = 0; i < 400 && (await caches.keys()).includes(before[0]); i++) await tick(25);
  for (let i = 0; i < 200 && !changes; i++) await tick(25);
  out.after = await kept();
  out.old_gone = !(await caches.keys()).includes(before[0]);
  out.new_worker = reg.active !== first && sw.controller.scriptURL === first.scriptURL;
  out.changes = changes;
  out.home = (await look("/")).h1;
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// B.2: the program edited and started again on its port is a new worker (its version names the
// program and its offline routes): it is installed at the next check, keeps the new routes, takes
// over the page, and drops what the one before kept.
#[test]
fn b2_a_new_version_installs_a_new_worker_that_drops_the_old_caches() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = dir("sw-version", &versioned(1));
    let stages = support::serve(&dir("sw-stages4", STAGES).join("main.qwe"), &[]);
    let (s, port) = logged(&d, &[]);
    let (_, h1, _) = fetch(port, "GET", "/_quillo/sw.js", &[]);
    let (c, url, sp) = (chrome.clone(), format!("http://127.0.0.1:{}/", port), stages.port);
    let page = std::thread::spawn(move || in_chrome_staged(&c, &url, &VERSION_SCRIPT.replace("@KIT@", SW_KIT), sp));
    stage_reached(stages.port, 1, &page);
    drop(s);
    std::fs::write(d.join("main.qwe"), versioned(2)).unwrap();
    let s = logged_at(&d, port, &[]);
    let (_, h2, _) = fetch(port, "GET", "/_quillo/sw.js", &[]);
    assert_ne!(header(&h1, "etag"), header(&h2, "etag"));
    let r = page.join().unwrap();
    assert_eq!(
        r,
        serde_json::json!({
            "controlled": true, "before": { "quillo::V": ["/", "/menu"] }, "up": true,
            "after": { "quillo::V": ["/", "/extra", "/menu"] }, "old_gone": true, "new_worker": true, "changes": 1, "home": "Version 2",
        }),
        "{}",
        std::fs::read_to_string(&s.1).unwrap_or_default()
    );
}

/// A proxy that serves the program under `base`, the prefix taken off as `--base` expects; with
/// the program stopped it closes the connection, as a network gone does.
const PROXY: &str = r#"const http = require("http");
const [listen, upstream, base, mode] = [+process.argv[1], +process.argv[2], process.argv[3], process.argv[4]];
http.createServer((req, res) => {
  if (req.url !== base && !req.url.startsWith(base + "/")) { res.writeHead(404); return res.end("not here"); }
  const p = http.request({ host: "127.0.0.1", port: upstream, path: req.url.slice(base.length) || "/", method: req.method, headers: req.headers }, (u) => { res.writeHead(u.statusCode, u.headers); u.pipe(res); });
  p.on("error", () => { if (mode !== "502") return req.socket.destroy(); res.writeHead(502, { "content-type": "text/html" }); res.end("<html><body><p id=bad>bad gateway</p></body></html>"); });
  req.pipe(p);
}).listen(listen, "127.0.0.1", function () { console.log(this.address().port); });
"#;

const BASED: &str = r#"component Counter() {
    state n = 0
    <button id="inc" onclick={() => { n += 1 }}>clicked {n}</button>
}
route offline GET / = <html><body><h1>Home</h1></body></html>
route offline GET /menu = <html><body><h1>Menu</h1><Counter/></body></html>
route GET /ping = "pong"
"#;

const BASED_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {};
  out.controlled = await controlled();
  const reg = await sw.getRegistration();
  out.worker = new URL(sw.controller.scriptURL).pathname;
  out.scope = new URL(reg.scope).pathname;
  out.kept = await kept();
  await stage(1);
  for (let i = 0; i < 600; i++) { try { await fetch("/shop/ping", { cache: "no-store" }); } catch (_) { out.down = true; break; } await tick(25); }
  const menu = await look("/shop/menu");
  const inc = menu.frame.contentDocument.getElementById("inc");
  inc.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  inc.click();
  for (let i = 0; i < 200 && inc.textContent !== "clicked 1"; i++) await tick(25);
  out.menu = shown(menu);
  out.component = inc.textContent;
  out.home = shown(await look("/shop/"));
  out.gone = shown(await look("/shop/nothing"));
  out.errors = window.__quilloErrors;
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// B.2 under --base: the worker is at the prefix's /_quillo/sw.js, its scope the prefix, allowed by
// its header; its pages, its caches and the files it keeps carry the prefix; with the program
// stopped, a page opened by a proxy that serves it at the prefix comes from the worker, with its
// component working.
#[test]
fn b2_under_base_the_worker_and_its_pages_carry_the_prefix() {
    let d = dir("sw-base", BASED);
    let (s, port) = logged(&d, &["--base", "/shop"]);
    let (status, h, worker) = fetch(port, "GET", "/_quillo/sw.js", &[]);
    let worker = String::from_utf8(worker).unwrap();
    assert_eq!((status, header(&h, "service-worker-allowed")), (200, Some("/shop/")));
    assert!(worker.contains("const BASE = \"/shop\", V = \"") && worker.contains("PAGES = [\"/shop/\",\"/shop/menu\"]"), "{}", &worker[..worker.len().min(2000)]);
    assert!(page(port, "/menu").contains(&register_script("/shop")));
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let stages = support::serve(&dir("sw-stages5", STAGES).join("main.qwe"), &[]);
    let (_proxy, listen) = proxy(port, "/shop", "");
    let (c, url, sp) = (chrome.clone(), format!("http://127.0.0.1:{}/shop/", listen), stages.port);
    let page = std::thread::spawn(move || in_chrome_staged(&c, &url, &BASED_SCRIPT.replace("@KIT@", SW_KIT), sp));
    stage_reached(stages.port, 1, &page);
    let log = std::fs::read_to_string(&s.1).unwrap_or_default();
    drop(s);
    let r = page.join().unwrap();
    assert_eq!(
        r,
        serde_json::json!({
            "controlled": true, "worker": "/shop/_quillo/sw.js", "scope": "/shop/",
            "kept": { "quillo:/shop:V": ["/shop/", "/shop/_quillo/b/H.js", "/shop/_quillo/r/H.js", "/shop/menu"] },
            "down": true,
            "menu": { "h1": "Menu", "kept": "iso", "offline": false, "title": "" }, "component": "clicked 1",
            "home": { "h1": "Home", "kept": "iso", "offline": false, "title": "" },
            "gone": { "h1": "You are offline", "kept": null, "offline": true, "title": "Offline" },
            "errors": [],
        }),
        "{}",
        log
    );
}

const LIVE: &str = r#"let n = 0
server fn count() -> int = n
component Count() {
    state v = live(count)
    <p id="v">count {v}</p>
}
route GET / = <html><body><Count/></body></html>
"#;

/// The sockets the runtime makes after the page's first, counted as they are made and closed;
/// with the server stopped, the live loop waits longer after each that does not open: after the
/// second, 2 s or more.
const ONLINE_SCRIPT: &str = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 200 && document.getElementById("v").textContent !== "count 0"; i++) await tick(25);
  // The live loop is the bundle's, which comes when the island is reached for and took seconds on
  // CI's debug build: the server was stopped before it came, and no socket was ever made (tried 0,
  // at_once 0, in every CI run since this test was written). Reached for, and booted, before the
  // sockets are counted, as a fast machine always had it (a shown bug in the test; branch pwa).
  document.getElementById("v").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  for (let i = 0; i < 400 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(25);
  const W = WebSocket;
  let made = 0, closed = 0;
  window.WebSocket = function (u, p) { const s = p === undefined ? new W(u) : new W(u, p); made++; s.addEventListener("close", () => closed++); return s; };
  window.WebSocket.prototype = W.prototype;
  await fetch("@STAGE@/stage/1", { mode: "no-cors" });
  for (let i = 0; i < 600 && closed < 2; i++) await tick(25);
  const out = { tried: closed };
  await tick(300);
  out.waiting = made - closed;
  const before = made;
  dispatchEvent(new Event("online"));
  await tick(300);
  out.at_once = made - before;
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// B.2: back online, as the browser says, the live socket is opened again at once, where it waited
// out its backoff (2 s or more after its second try that did not open, up to half a minute).
#[test]
fn b2_back_online_the_live_socket_is_opened_again_at_once() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = dir("sw-online", LIVE);
    let stages = support::serve(&dir("sw-stages6", STAGES).join("main.qwe"), &[]);
    let (s, port) = logged(&d, &[]);
    let (c, url, sp) = (chrome.clone(), format!("http://127.0.0.1:{}/", port), stages.port);
    let page = std::thread::spawn(move || in_chrome_staged(&c, &url, ONLINE_SCRIPT, sp));
    stage_reached(stages.port, 1, &page);
    drop(s);
    assert_eq!(page.join().unwrap(), serde_json::json!({ "tried": 2, "waiting": 0, "at_once": 1 }));
}

/// Every kind of answer of a program with offline routes.
const MARKED: &str = r#"server fn sign_out() {
    end_session()
}
server fn same() -> int = 1
server fn who() -> str = session().user ?? "nobody"
route GET /in/:who {
    set_session({ user: who })
    redirect("/")
}
route GET /cart {
    set_session({ user: session().user, cart: [1, 2, 3] })
    "ok"
}
route offline GET / = <html><body><h1>Home</h1></body></html>
route offline GET /item/:id = <html><body><h1>Item {id}</h1></body></html>
route offline GET /café = <html><body><h1>Café</h1></body></html>
route offline GET /mine = <html><body><h1>{session().user ?? "nobody"}</h1></body></html>
route offline GET /nulled = respond(200, <html><body>{session().user ?? "x"}</body></html>, { "cache-control": null })
route offline GET /theme = <html><body>{cookie("theme") ?? "light"}</body></html>
route offline GET /agent = <html><body>{request.headers["user-agent"] ?? ""}</body></html>
route offline GET /order = <html><body><form method="post" action="/buy"><input type="hidden" name="_idempotency_key" value="k1"></form></body></html>
route offline GET /data = { a: 1 }
route offline GET /missing = respond(404, <html><body>gone</body></html>)
route offline GET /static = file("page.html")
route GET /plain = <html><body><h1>Plain</h1></body></html>
route GET /signed = <html><body>{session().user ?? "nobody"}</body></html>
routes offline /docs {
    route GET /a = <html><body>a</body></html>
    route POST /b = <html><body>b</body></html>
}
fn signed(request) = session().user == null ? redirect("/") : request
fn teamed(request) = request.headers["x-team"] == null ? redirect("/") : request
routes offline /s | signed {
    route GET /f = <html><body>{session().user}</body></html>
}
routes offline /t | teamed {
    route GET /e = <html><body>e</body></html>
}
"#;

/// An answer's status, its marks for the worker (`x-quillo-offline`, `x-quillo-session`), the
/// `quillo_tag` cookie it sets, and how often it holds the register script.
type Marks = (u16, Option<String>, Option<String>, Option<String>, usize);

fn marks_of(port: u16, method: &str, path: &str, cookie: Option<&str>) -> Marks {
    let hs: Vec<(&str, &str)> = cookie.map(|c| ("cookie", c)).into_iter().collect();
    let (status, h, body) = fetch(port, method, path, &hs);
    let tag = h.iter().find(|(k, v)| k == "set-cookie" && v.starts_with("quillo_tag=")).map(|(_, v)| v.clone());
    (status, header(&h, "x-quillo-offline").map(str::to_string), header(&h, "x-quillo-session").map(str::to_string), tag, String::from_utf8_lossy(&body).matches(&register_script("")).count())
}

/// Signs in at `/in/<who>`: the session cookie as a request sends it, and the answer's tag.
fn sign_in(port: u16, who: &str) -> (String, String, Vec<(String, String)>) {
    let (status, h, _) = fetch(port, "GET", &format!("/in/{}", who), &[]);
    assert_eq!(status, 303);
    let cookie = h.iter().find(|(k, v)| k == "set-cookie" && v.contains("quillo_session=")).map(|(_, v)| v.split(';').next().unwrap().to_string()).unwrap();
    (cookie, header(&h, "x-quillo-session").unwrap_or("").to_string(), h)
}

// B.2 and its review, as the server answers: an offline route's 200 page registers the worker, a
// page with a parameter too, once each. What is kept is decided by what the request read, not by
// `cache-control`: no one's (`1`); the visitor's through `session()` alone (`private`, by the
// session's name); not at all when it read them otherwise (a cookie, a header), held a form key
// (a kept form would send its order again), or could reach a shared cache (`cache-control: null`
// leaves no name on it). Not a page that is not tagged, not JSON, not a 404, not a POST of an
// offline group; a file as it is on disk is marked and has nothing added; a HEAD has the GET's
// marks. The session's name goes only on a page of the visitor's and on an answer that sets or
// ends the session (a redirect, a server fn's), never on a page anyone may be given.
#[test]
fn b2_the_marks_and_the_register_script_go_where_they_belong() {
    let d = app_dir("sw-marked", &[("main.qwe", MARKED.as_bytes()), ("page.html", b"<!doctype html><html><body>file</body></html>")]);
    assert!(check(&d).0, "{:?}", check(&d));
    let (_s, port) = logged(&d, &[]);
    assert_eq!(register_script("").len(), 280);
    // No session cookie at all: every page says no one (`-`), which any cache may give anyone.
    let m = |method: &str, path: &str| marks_of(port, method, path, None);
    let (one, none) = (|| Some("1".to_string()), || Some("-".to_string()));
    assert_eq!(m("GET", "/"), (200, one(), none(), None, 1));
    assert_eq!(m("HEAD", "/"), (200, one(), none(), None, 0));
    assert_eq!(m("GET", "/item/5"), (200, one(), none(), None, 1));
    assert_eq!(m("GET", "/caf%C3%A9"), (200, one(), none(), None, 1));
    assert_eq!(m("GET", "/docs/a"), (200, one(), none(), None, 1));
    // The visitor's page, no session: the one of no session's, `-`.
    assert_eq!(m("GET", "/mine"), (200, Some("private".into()), none(), None, 1));
    for path in ["/nulled", "/theme", "/agent", "/order"] {
        assert_eq!(m("GET", path), (200, None, none(), None, 1), "{}", path);
    }
    // Only a GET's answer says no one: another site's form, sent here, carries no Lax cookie
    // (third review).
    assert_eq!(m("POST", "/docs/b"), (200, None, None, None, 0));
    assert_eq!(m("GET", "/plain"), (200, None, none(), None, 0));
    assert_eq!(m("GET", "/data"), (200, None, None, None, 0));
    assert_eq!(m("GET", "/missing"), (404, None, none(), None, 0));
    assert_eq!(m("GET", "/nowhere"), (404, None, None, None, 0));
    let (_, h, body) = fetch(port, "GET", "/static", &[]);
    assert_eq!((header(&h, "x-quillo-offline"), String::from_utf8(body).unwrap()), (Some("1"), "<!doctype html><html><body>file</body></html>".to_string()));
    // Signed in: the session's pages carry its name; one anyone may be given does not.
    let (ana, tag, _) = sign_in(port, "ana");
    let with = |path: &str, c: &str| marks_of(port, "GET", path, Some(c));
    let ana_tagged = format!("{}; quillo_tag={}", ana, tag);
    assert_eq!(with("/mine", &ana_tagged), (200, Some("private".into()), Some(tag.clone()), None, 1));
    assert_eq!(with("/signed", &ana_tagged), (200, None, Some(tag.clone()), None, 0));
    for path in ["/", "/plain", "/theme", "/agent", "/nulled"] {
        assert_eq!(with(path, &ana_tagged).2, None, "{}", path);
    }
    // Behind a guard that reads the session and hands the request back: kept for the session;
    // behind one that reads a header: not kept.
    assert_eq!(with("/s/f", &ana_tagged), (200, Some("private".into()), Some(tag.clone()), None, 1));
    let (status, h, _) = fetch(port, "GET", "/t/e", &[("cookie", &ana_tagged), ("x-team", "a")]);
    assert_eq!((status, header(&h, "x-quillo-offline")), (200, None));
    // A server fn: one that sets or ends the session says the name it leaves; one that reads it, or
    // nothing, says none.
    let rpc = |name: &str, cookie: &str| {
        let a = support::send(port, "POST", &format!("/_quillo/rpc/{}", name), &[("cookie", cookie), ("origin", "http://localhost")], "[]");
        (a.status, a.header("x-quillo-session").map(str::to_string))
    };
    assert_eq!(rpc("same", &ana), (200, None));
    assert_eq!(rpc("who", &ana), (200, None));
    assert_eq!(rpc("sign_out", &ana), (200, Some("-".into())));
}

// B.2 review: the session's name is a digest of its hidden id and (second review) of all it holds:
// a cart written into it is another name; two sign-ins of one browser as one person keep it;
// another person is another name. `quillo_tag`, the cookie that names it, comes with the session cookie: readable
// by the page (no HttpOnly), its path the program's base, `SameSite=Lax`, `Secure` as the session
// cookie is, living as long; taken away with it. A page of the session's that came without it (a
// session from before the program's offline routes, a cookie lost) sets it again, once.
#[test]
fn b2_the_session_is_named_by_its_hidden_id_and_its_cookie() {
    for (base, path) in [(&[][..], "/"), (&["--base", "/shop"][..], "/shop")] {
        let d = app_dir("sw-tag", &[("main.qwe", MARKED.as_bytes()), ("page.html", b"x")]);
        let (_s, port) = logged(&d, base);
        let (ana, tag, h) = sign_in(port, "ana");
        let line = h.iter().find(|(k, v)| k == "set-cookie" && v.starts_with("quillo_tag=")).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(line, format!("quillo_tag={}; Path={}; Max-Age=2592000; Secure; SameSite=Lax", tag, path));
        assert!(h.iter().any(|(k, v)| k == "set-cookie" && v.contains("quillo_session=") && v.contains("HttpOnly")));
        assert!(tag.len() == 16 && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'), "{}", tag);
        // A cart in the session: another name (second review: any change to the session is one,
        // so that the person's kept pages go, and are kept again on the next visits).
        let (_, h, _) = fetch(port, "GET", "/cart", &[("cookie", &ana)]);
        let cart_tag = header(&h, "x-quillo-session").unwrap().to_string();
        assert_ne!(cart_tag, tag);
        let cart = h.iter().find(|(k, v)| k == "set-cookie" && v.contains("quillo_session=")).map(|(_, v)| v.split(';').next().unwrap().to_string()).unwrap();
        assert_eq!(marks_of(port, "GET", "/mine", Some(&format!("{}; quillo_tag={}", cart, cart_tag))).2, Some(cart_tag.clone()));
        // Signed in again a moment later in the same browser, as the same person: the same name.
        let (_, h, _) = fetch(port, "GET", "/in/ana", &[("cookie", &ana)]);
        assert_eq!(header(&h, "x-quillo-session"), Some(tag.as_str()));
        // Another person, and the same one again on another browser: other names.
        let (_, bob, _) = sign_in(port, "bob");
        let (_, ana2, _) = sign_in(port, "ana");
        assert!(bob != tag && ana2 != tag && bob != ana2, "{} {} {}", tag, bob, ana2);
        // The page of the session's without the cookie sets it again for the session's life; with
        // it, or another's, as the server reads it.
        let (_, _, _, again, _) = marks_of(port, "GET", "/mine", Some(&ana));
        let again = again.unwrap();
        let age: u64 = again.split("Max-Age=").nth(1).unwrap().split(';').next().unwrap().parse().unwrap();
        assert!(again.starts_with(&format!("quillo_tag={}; Path={}; Max-Age=", tag, path)) && (2591990..=2592000).contains(&age) && !again.contains("HttpOnly"), "{}", again);
        assert_eq!(marks_of(port, "GET", "/mine", Some(&format!("{}; quillo_tag={}", ana, tag))).3, None);
        let wrong = marks_of(port, "GET", "/mine", Some(&format!("{}; quillo_tag=someone", ana))).3.unwrap();
        assert!(wrong.starts_with(&format!("quillo_tag={};", tag)), "{}", wrong);
        // No session, a stale cookie: taken away.
        let gone = marks_of(port, "GET", "/mine", Some("quillo_tag=someone")).3.unwrap();
        assert!(gone.starts_with(&format!("quillo_tag=; Path={}; Max-Age=0", path)), "{}", gone);
        // Ended: both go.
        let a = support::send(port, "POST", "/_quillo/rpc/sign_out", &[("cookie", &ana), ("origin", "http://localhost")], "[]");
        let lines: Vec<&str> = a.headers.iter().filter(|(k, _)| k == "set-cookie").map(|(_, v)| v.as_str()).collect();
        assert!(lines.iter().any(|l| l.starts_with(&format!("quillo_tag=; Path={}; Max-Age=0", path))), "{:?}", lines);
    }
    // A program without offline routes sets no such cookie.
    let d = app_dir("sw-tag-none", &[("main.qwe", MARKED.replace("route offline", "route").replace("routes offline", "routes").as_bytes()), ("page.html", b"x")]);
    let (_s, port) = logged(&d, &[]);
    let (_, _, h) = sign_in(port, "ana");
    assert!(!h.iter().any(|(k, v)| k == "set-cookie" && v.starts_with("quillo_tag=")), "{:?}", h);
}

/// `fetch` with a body.
fn fetch_body(port: u16, path: &str) -> (u16, Vec<(String, String)>, String) {
    let (s, h, b) = fetch(port, "GET", path, &[]);
    (s, h, String::from_utf8_lossy(&b).into_owned())
}

// B.2: `/_quillo/sw.js` is JavaScript, revalidated each time, allowed the program's base as its
// scope, with a validator (a 304 for it); a POST is not it. It names the offline routes with no
// parameter, once each, in order, unicode as written, and the offline page with app.toml's name
// made safe. Its version is one for the program: the same on every instance and after a restart,
// another when a file the program uses changes.
#[test]
fn b2_the_worker_is_served_with_its_version_one_per_program() {
    let d = app_dir("sw-served", &[("main.qwe", MARKED.as_bytes()), ("page.html", b"x"), ("app.toml", "name = \"A <b> & \\\"c\\\" ✓\"\n".as_bytes())]);
    let (s, port) = logged(&d, &["--instances", "2"]);
    let (status, h, worker) = fetch_body(port, "/_quillo/sw.js");
    assert_eq!(status, 200);
    assert_eq!(header(&h, "content-type"), Some("text/javascript; charset=utf-8"));
    assert_eq!(header(&h, "cache-control"), Some("no-cache"));
    assert_eq!(header(&h, "service-worker-allowed"), Some("/"));
    let etag = header(&h, "etag").unwrap().to_string();
    assert!(worker.contains(&format!("V = {}", etag)), "{}", etag);
    assert!(worker.contains("PAGES = [\"/\",\"/café\",\"/mine\",\"/nulled\",\"/theme\",\"/agent\",\"/order\",\"/data\",\"/missing\",\"/static\",\"/docs/a\",\"/s/f\",\"/t/e\"]"), "{}", &worker[..2500.min(worker.len())]);
    // The gate, as the server names it in a page's policy.
    assert!(worker.contains(&format!("GATE = {}", serde_json::Value::String(quillo::pwa::gate().into()))));
    assert!(worker.contains("<title>A &lt;b&gt; &amp; &quot;c&quot; ✓</title>") && worker.contains("<h1>A &lt;b&gt; &amp; &quot;c&quot; ✓</h1>"), "{}", worker);
    let (status, _, body) = fetch(port, "GET", "/_quillo/sw.js", &[("if-none-match", &etag)]);
    assert_eq!((status, body.len()), (304, 0));
    assert_eq!(fetch(port, "POST", "/_quillo/sw.js", &[]).0, 404);
    for _ in 0..8 {
        assert_eq!(fetch_body(port, "/_quillo/sw.js").2, worker, "one worker on every instance");
    }
    drop(s);
    let s = logged_at(&d, port, &[]);
    assert_eq!(fetch_body(port, "/_quillo/sw.js").2, worker, "the same after a restart");
    drop(s);
    std::fs::write(d.join("main.qwe"), format!("use \"lib.qwe\"\n{}", MARKED)).unwrap();
    std::fs::write(d.join("lib.qwe"), "fn one() = 1\n").unwrap();
    let s = logged_at(&d, port, &[]);
    let with_lib = fetch_body(port, "/_quillo/sw.js").2;
    assert_ne!(with_lib, worker);
    drop(s);
    std::fs::write(d.join("lib.qwe"), "fn one() = 2\n").unwrap();
    let _s = logged_at(&d, port, &[]);
    assert_ne!(fetch_body(port, "/_quillo/sw.js").2, with_lib, "a file it uses changed");
}

// B.2 and its review: a program with no offline route is as it was, an app.toml or not: no marks
// on any answer, no register script, no `quillo_tag`. Its `/_quillo/sw.js`, which a browser asks
// for only if it registered Quillo's worker in an earlier run, is the worker that takes itself
// away with what Quillo's kept (by the spec a 404 there would leave the old worker answering from
// its cache); so under --watch, offline routes or not, where no page is marked either.
#[test]
fn b2_without_an_offline_route_nothing_changes() {
    let src = MARKED.replace("route offline", "route").replace("routes offline", "routes");
    for app in [false, true] {
        let mut files: Vec<(&str, &[u8])> = vec![("main.qwe", src.as_bytes()), ("page.html", b"x")];
        if app {
            files.push(("app.toml", b"name = \"Shop\"\n"));
        }
        let d = app_dir(&format!("sw-none-{}", app), &files);
        for opts in [&[][..], &["--watch"][..]] {
            let (s, port) = logged(&d, opts);
            let (status, h, worker) = fetch_body(port, "/_quillo/sw.js");
            assert!(status == 200 && worker.contains("self.registration.unregister()") && worker.contains("const MINE = \"quillo:\" + \"\" + \":\""), "{:?}: {}", opts, worker);
            assert_eq!(header(&h, "service-worker-allowed"), Some("/"));
            let (ana, _, signed) = sign_in(port, "ana");
            assert!(!signed.iter().any(|(k, v)| k.starts_with("x-quillo-offline") || k.starts_with("x-quillo-session") || v.starts_with("quillo_tag=")), "{:?}", signed);
            for (method, path) in [("GET", "/"), ("HEAD", "/"), ("GET", "/mine"), ("GET", "/plain"), ("GET", "/nowhere"), ("POST", "/docs/b")] {
                let (_, h, body) = fetch(port, method, path, &[("cookie", &ana)]);
                assert!(!h.iter().any(|(k, v)| k.starts_with("x-quillo-offline") || k.starts_with("x-quillo-session") || v.starts_with("quillo_tag=")), "{:?} {} {}: {:?}", opts, method, path, h);
                assert!(!String::from_utf8_lossy(&body).contains("/_quillo/sw.js"), "{} {}", method, path);
            }
            drop(s);
        }
    }
}

// B.2, the tag as written: after `route`, before the method (`route offline GET /x`, with patterns
// and `with` after the path as ever), and after `routes`, before the group's path, where each GET
// route of the group and of the groups inside it is kept, not its others. `offline` stays a name
// everywhere else. Refused, each said for what it is: the tag on a route that is not a GET, a
// group with no GET, and the tag where the report wrote it (`route GET /menu offline`) or after
// the method.
#[test]
fn b2_the_tag_is_parsed_and_checked() {
    let ok = [
        "let offline = true\nfn f(offline: bool) = !offline\nroute GET /x = f(offline) ? \"y\" : \"n\"\n",
        "route offline GET /x with db = db.query(\"select 1\")\n",
        "route offline GET /x/:id(int) ? { page: int = 1 } = <p>{id} {page}</p>\n",
        "routes offline /docs {\n    routes /inner {\n        route GET /a = \"a\"\n    }\n    route DELETE /b = \"b\"\n}\n",
        "routes /docs {\n    routes offline /inner {\n        route GET /a = \"a\"\n    }\n    route offline GET /b = \"b\"\n}\n",
        "let routes = 2\nlet offline = 1\nroute GET / = str(routes / offline)\n",
    ];
    for src in ok {
        assert_eq!(check(&dir("tag-ok", src)), (true, vec![]), "{}", src);
    }
    let refused = [
        ("route offline POST /x = \"x\"\n", "main.qwe:1: error: `offline` keeps a page for when there is no network, and a page is a GET: `route offline POST ...` cannot be kept (`route offline GET /...` for the page that shows it)"),
        ("route offline ANY /x = \"x\"\n", "main.qwe:1: error: `offline` keeps a page for when there is no network, and a page is a GET: `route offline ANY ...` cannot be kept (`route offline GET /...` for the page that shows it)"),
        ("route GET /menu offline = \"x\"\n", "main.qwe:1: error: `offline` comes after `route`, before the method: `route offline GET /menu ...`"),
        ("route GET offline /menu = \"x\"\n", "main.qwe:1: error: `offline` comes after `route`, before the method: `route offline GET /...`"),
        ("route offline /menu = \"x\"\n", "main.qwe:1: error: `offline` comes before the method, and a page kept for offline is a GET: `route offline GET /menu = ...`"),
        ("routes offline /api {\n    route POST /a = \"a\"\n}\n", "main.qwe:1: error: this `routes offline` group has no GET route: `offline` keeps pages, which are GETs, and its other routes are sent again, never kept"),
        ("routes /api {\n    routes offline /in {\n        route PUT /a = \"a\"\n    }\n    route GET /b = \"b\"\n}\n", "main.qwe:2: error: this `routes offline` group has no GET route: `offline` keeps pages, which are GETs, and its other routes are sent again, never kept"),
        // Markup left open runs into the next declaration, a tagged route as a plain one.
        ("route GET /a = <div>\n    <p>x</p>\n\nroute offline GET /b = \"b\"\n", "main.qwe:1: error: `<div>` is never closed: add `</div>` before `route` on line 4"),
    ];
    for (src, said) in refused {
        assert_eq!(check(&dir("tag-refused", src)), (false, vec![said.to_string()]), "{}", src);
    }
    // Which routes are tagged, as the server reads them: each GET of an offline group, at any depth.
    let d = dir("tag-which", "routes offline /docs {\n    routes /inner {\n        route GET /a = <html><body>a</body></html>\n    }\n    route DELETE /b = <html><body>b</body></html>\n}\nroutes /other {\n    route GET /c = <html><body>c</body></html>\n}\n");
    let (_s, port) = logged(&d, &[]);
    let tagged = |m: &str, p: &str| header(&fetch(port, m, p, &[]).1, "x-quillo-offline").is_some();
    assert_eq!((tagged("GET", "/docs/inner/a"), tagged("DELETE", "/docs/b"), tagged("GET", "/other/c")), (true, false, false));
}

// B.2 in Quillo's highlighter (code on pages, the guide): the tag is a keyword after `route` and
// `routes`, and the path after it is still a path (its `/*` no comment); a name `offline` is none.
#[test]
fn b2_the_highlighter_knows_the_tag() {
    let v = quillo::eval_str("highlight(\"route offline GET /docs/*rest = 1\\nroutes offline /api {\\n}\\nlet offline = f(offline)\\nroute GET /x = offline\", \"quillo\")").unwrap();
    assert_eq!(
        v.display(),
        "<span class=\"hl-k\">route</span> <span class=\"hl-k\">offline</span> <span class=\"hl-t\">GET</span> /docs/*rest = <span class=\"hl-n\">1</span>\n<span class=\"hl-k\">routes</span> <span class=\"hl-k\">offline</span> /api {\n}\n<span class=\"hl-k\">let</span> offline = <span class=\"hl-f\">f</span>(offline)\n<span class=\"hl-k\">route</span> <span class=\"hl-t\">GET</span> /x = offline"
    );
}

// B.2 in the editor (`quillo lsp`): the tag is offered after `route ` and `routes `, first, and not
// elsewhere; hovering it says what it does, and a name `offline` is no tag.
#[test]
fn b2_the_editor_offers_and_explains_the_tag() {
    use serde_json::{json, Value};
    use std::io::{BufRead, Read, Write};
    let d = support::temp_dir("f67-lsp").canonicalize().unwrap();
    let text = "route offline GET /menu = 1\nroutes offline /docs {\n}\nlet offline = 2\nroute \nroutes \nlet \nroute GET \n";
    std::fs::write(d.join("main.qwe"), text).unwrap();
    let uri = format!("file://{}", d.join("main.qwe").display());
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo")).arg("lsp").current_dir(&d).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().unwrap();
    let mut g = support::Guard::new(child);
    let mut out = std::io::BufReader::new(g.0.stdout.take().unwrap());
    let mut stdin = g.0.stdin.take().unwrap();
    let mut next = 0;
    let end = Instant::now() + Duration::from_secs(30);
    let mut ask = |method: &str, params: Value, notify: bool| -> Value {
        next += 1;
        let msg = if notify { json!({ "jsonrpc": "2.0", "method": method, "params": params }) } else { json!({ "jsonrpc": "2.0", "id": next, "method": method, "params": params }) };
        let body = msg.to_string();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
        if notify {
            return Value::Null;
        }
        loop {
            assert!(Instant::now() < end, "the language server did not answer");
            let mut len = 0;
            loop {
                let mut line = String::new();
                assert!(out.read_line(&mut line).unwrap() > 0, "the language server ended");
                if line.trim_end().is_empty() {
                    break;
                }
                if let Some(v) = line.trim_end().strip_prefix("Content-Length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut buf = vec![0; len];
            out.read_exact(&mut buf).unwrap();
            let m: Value = serde_json::from_slice(&buf).unwrap();
            if m.get("id") == Some(&json!(next)) {
                return m["result"].clone();
            }
        }
    };
    ask("initialize", json!({ "capabilities": {} }), false);
    ask("initialized", json!({}), true);
    ask("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } }), true);
    let mut at = |method: &str, line: usize, character: usize| ask(method, json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }), false);
    let tag = at("textDocument/hover", 0, 8)["contents"]["value"].as_str().unwrap_or("").to_string();
    assert!(tag.starts_with("`route offline GET /menu = ...`") && tag.contains("Only a GET can be kept."), "{}", tag);
    assert_eq!(at("textDocument/hover", 1, 9)["contents"]["value"].as_str().unwrap_or(""), tag);
    assert!(!at("textDocument/hover", 3, 6).to_string().contains("Only a GET can be kept"));
    for (line, col, want) in [(4, 6, true), (5, 7, true), (6, 4, false), (7, 10, false)] {
        let items = at("textDocument/completion", line, col);
        let tag = items.as_array().unwrap().iter().position(|i| i["label"] == "offline" && i["kind"] == 14);
        assert_eq!(tag.is_some(), want, "line {}: {}", line, items);
        assert!(tag.map_or(true, |t| t == 0), "not first: {}", items);
    }
}

// B.2: never in the phone shells (nor `quillo app`, which runs as "app" the same way): service
// workers do not run there, and the program is on the device. No marks, no script, no worker.
#[test]
fn b2_never_in_the_phone_shells() {
    quillo::platform::set_runtime("mobile");
    let d = dir("sw-mobile", "route offline GET / = <html><body><h1>Home</h1></body></html>\n");
    let mut it = quillo::Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    quillo::pwa::attach(&mut it, &d);
    let get = |it: &mut quillo::Interp, url: &str| it.handle(&quillo::server::HttpRequest { method: "GET".into(), url: url.into(), headers: vec![], body: vec![], remote: "127.0.0.1".into() });
    let page = get(&mut it, "/");
    assert_eq!(page.status, 200);
    assert!(!page.headers.iter().any(|(k, _)| k.starts_with("x-quillo-offline") || k.starts_with("x-quillo-session")), "{:?}", page.headers);
    assert!(!page.text().contains("/_quillo/sw.js"), "{}", page.text());
    assert_eq!(get(&mut it, "/_quillo/sw.js").status, 404);
}

/// A session's pages under a policy that refuses inline scripts but the server's hashes: its
/// stages on the stage server, which the policy lets the page reach.
fn gated(stages: u16) -> String {
    format!(
        "site_headers(\"/\", {{ \"content-security-policy\": \"default-src 'self'; connect-src 'self' http://127.0.0.1:{}\" }})\nroute GET /in/:who {{\n    set_session({{ user: who }})\n    redirect(\"/\")\n}}\nroute offline GET / = <html><head><title>Home</title></head><body><h1>Home</h1></body></html>\nroute offline GET /mine = <html><head><title>Mine</title></head><body><h1>Mine {{session().user ?? \"nobody\"}}</h1></body></html>\nroute offline GET /bare = <html><body><h1>Bare {{session().user ?? \"nobody\"}}</h1></body></html>\nroute GET /ping = \"pong\"\n",
        stages
    )
}

const GATE_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {}, refused = [];
  document.addEventListener("securitypolicyviolation", (e) => refused.push(e.violatedDirective));
  const secure = "; Path=/; Secure; SameSite=Lax";
  out.controlled = await controlled();
  await look("/in/ana");
  const ana = tagNow();
  out.ana = (await look("/mine")).h1;
  await look("/bare");
  out.kept = await kept();
  await stage(1);
  out.down = await down();
  // Offline, the browser holding the session the pages were kept for: shown, past the gate.
  out.mine = await lookUntil("/mine", (d) => !d.documentElement.hasAttribute("hidden"));
  out.bare = await lookUntil("/bare", (d) => !d.documentElement.hasAttribute("hidden"));
  out.same = out.mine.session === ana && out.bare.session === ana;
  // Signed out where the worker did not see it (the cookie ended): the offline page, and the
  // session's pages dropped.
  document.cookie = "quillo_tag=; Max-Age=0" + secure;
  out.ended = await lookUntil("/mine", (d) => d.documentElement.hasAttribute("data-quillo-offline"));
  out.after_end = await kept();
  out.dropped = (await dropped()).map((t) => t === ana ? "ana" : t);
  await stage(2);
  out.up = await up("pong");
  // The server still takes ana's cookie, and sets her `quillo_tag` again (the page's register
  // script then tells the worker she is back: which comes first is the browser's, so what it
  // keeps here is not looked at; b2_review2_a_late_answer_of_a_dropped_session_is_not_kept).
  out.late = (await look("/mine")).h1;
  // Another person's tag: the offline page.
  await look("/in/bob");
  const bob = tagNow();
  out.bob = (await look("/mine")).h1;
  await stage(3);
  out.down2 = await down();
  document.cookie = "quillo_tag=someone" + secure;
  out.other = await lookUntil("/mine", (d) => d.documentElement.hasAttribute("data-quillo-offline"));
  out.after_other = await kept();
  out.dropped2 = (await dropped()).includes(bob);
  out.refused = refused;
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// B.2 review, the gate: a page kept for a session is given back hidden, the gate first in its
// head (passing the page's policy, which the server named it in), and shown only when the
// browser's `quillo_tag` names that session. A session that ended without the worker seeing it
// (the cookie gone) or another person's: the offline page instead, and that session's pages
// dropped; an answer of a session dropped that comes later is not kept again.
#[test]
fn b2_a_sessions_kept_page_is_shown_only_to_that_session() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let stages = support::serve(&dir("sw-stages7", STAGES).join("main.qwe"), &[]);
    let d = dir("sw-gated", &gated(stages.port));
    let (s, port) = logged(&d, &[]);
    // The policy of a session's page names the gate, which the page does not hold yet.
    let (_, h, _) = fetch(port, "GET", "/mine", &[]);
    assert!(header(&h, "content-security-policy").unwrap().contains(&policy_hash(quillo::pwa::gate())), "{:?}", h);
    let (_, h, _) = fetch(port, "GET", "/", &[]);
    assert!(!header(&h, "content-security-policy").unwrap().contains(&policy_hash(quillo::pwa::gate())), "{:?}", h);
    let (c, url, sp) = (chrome.clone(), format!("http://127.0.0.1:{}/", port), stages.port);
    let page = std::thread::spawn(move || in_chrome_staged(&c, &url, &GATE_SCRIPT.replace("@KIT@", SW_KIT), sp));
    stage_reached(stages.port, 1, &page);
    drop(s);
    stage_reached(stages.port, 2, &page);
    let s = logged_at(&d, port, &[]);
    stage_reached(stages.port, 3, &page);
    let log = std::fs::read_to_string(&s.1).unwrap_or_default();
    drop(s);
    let r = page.join().unwrap();
    let offline = serde_json::json!({ "h1": "You are offline", "kept": null, "offline": true, "title": "Offline", "session": null, "hidden": false });
    let public = serde_json::json!({ "quillo::V": ["/"] });
    assert_eq!(
        r,
        serde_json::json!({
            "controlled": true, "ana": "Mine ana",
            "kept": { "quillo::V": ["/"], "quillo::V:S": ["/bare", "/mine"] }, "down": true,
            "mine": { "h1": "Mine ana", "kept": "iso", "offline": false, "title": "Mine", "session": r["mine"]["session"], "hidden": false },
            "bare": { "h1": "Bare ana", "kept": "iso", "offline": false, "title": "", "session": r["mine"]["session"], "hidden": false },
            "same": true,
            "ended": offline, "after_end": public, "dropped": ["ana"],
            "up": true, "late": "Mine ana",
            "bob": "Mine bob", "down2": true,
            "other": offline, "after_other": public, "dropped2": true,
            "refused": [],
        }),
        "{}",
        log
    );
}

/// Two versions: the second drops `/old`, and has a page that fails (503) until `/fix`.
fn flaky(n: u32) -> String {
    if n == 1 {
        "route offline GET / = <html><body><h1>One</h1></body></html>\nroute offline GET /old = <html><body><h1>Old</h1></body></html>\nroute offline GET /menu = <html><body><h1>Menu 1</h1></body></html>\nroute GET /ping = \"pong 1\"\n".into()
    } else {
        "let down = true\nroute GET /fix {\n    down = false\n    \"ok\"\n}\nroute offline GET / = <html><body><h1>Two</h1></body></html>\nroute offline GET /menu = <html><body><h1>Menu 2</h1></body></html>\nroute offline GET /flaky = down ? respond(503, \"not now\") : <html><body><h1>Flaky</h1></body></html>\nroute GET /ping = \"pong 2\"\n".into()
    }
}

const FLAKY_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {};
  out.controlled = await controlled();
  const reg = await sw.getRegistration();
  // The caches by version: each one's pages, this worker's version last.
  const versions = async () => { const out = []; for (const n of await caches.keys()) if (!n.endsWith(":gone")) out.push((await (await caches.open(n)).keys()).map((r) => new URL(r.url).pathname).filter((p) => !p.endsWith("sw-state")).sort().join(" ")); return out.sort(); };
  out.before = await versions();
  let changes = 0;
  sw.addEventListener("controllerchange", () => changes++);
  await stage(1);
  out.up = await up("pong 2");
  await reg.update();
  for (let i = 0; i < 200 && !changes; i++) await tick(25);
  out.both = await versions();
  await stage(2);
  out.down = await down();
  // The new version could not keep /flaky: the old copies still serve.
  out.old = shown(await look("/old"));
  out.menu = (await look("/menu")).h1;
  await stage(3);
  out.up2 = await up("pong 2");
  await fetch("/fix");
  // Tried again on a navigation, 10 s after the last try at most: then the old version goes.
  for (let i = 0; i < 30 && (await versions()).length > 1; i++) { await look("/"); await tick(1000); }
  out.after = await versions();
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// B.2 review: a new version whose pages could not all be kept as it installed (one answered 503)
// takes over but leaves the old version's copies, which still serve offline; it tries again on a
// later navigation, and once all are kept the old version's caches go.
#[test]
fn b2_a_failed_update_keeps_the_old_pages_until_the_new_ones_are_kept() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = dir("sw-flaky", &flaky(1));
    let stages = support::serve(&dir("sw-stages8", STAGES).join("main.qwe"), &[]);
    let (s, port) = logged(&d, &[]);
    let (c, url, sp) = (chrome.clone(), format!("http://127.0.0.1:{}/", port), stages.port);
    let page = std::thread::spawn(move || in_chrome_staged(&c, &url, &FLAKY_SCRIPT.replace("@KIT@", SW_KIT), sp));
    stage_reached(stages.port, 1, &page);
    drop(s);
    std::fs::write(d.join("main.qwe"), flaky(2)).unwrap();
    let s = logged_at(&d, port, &[]);
    stage_reached(stages.port, 2, &page);
    drop(s);
    stage_reached(stages.port, 3, &page);
    let s = logged_at(&d, port, &[]);
    let r = page.join().unwrap();
    assert_eq!(
        r,
        serde_json::json!({
            "controlled": true, "before": ["/ /menu /old"], "up": true,
            "both": ["/ /menu", "/ /menu /old"], "down": true,
            "old": { "h1": "Old", "kept": "iso", "offline": false, "title": "" }, "menu": "Menu 2",
            "up2": true, "after": ["/ /flaky /menu"],
        }),
        "{}",
        std::fs::read_to_string(&s.1).unwrap_or_default()
    );
}

const BOUNDED: &str = "route GET /in/:who {\n    set_session({ user: who })\n    redirect(\"/\")\n}\nroute offline GET / = <html><body><h1>Home</h1></body></html>\nroute offline GET /item/:n = <html><body><h1>Item {n}</h1></body></html>\nroute offline GET /mine/:n = <html><body><h1>{session().user} {n}</h1></body></html>\n";

const BOUNDED_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {};
  out.controlled = await controlled();
  await look("/in/ana");
  const f = document.createElement("iframe");
  document.body.appendChild(f);
  const go = (p) => new Promise((ok) => { f.onload = ok; f.src = p; });
  for (let n = 1; n <= 101; n++) await go("/item/" + n);
  for (let n = 1; n <= 51; n++) await go("/mine/" + n);
  await tick(500);
  const k = await kept(), pub = k["quillo::V"], mine = k["quillo::V:S"];
  out.public = [pub.length, pub.includes("/"), pub.includes("/item/1"), pub.includes("/item/2"), pub.includes("/item/101")];
  out.session = [mine.length, mine.includes("/mine/1"), mine.includes("/mine/2"), mine.includes("/mine/51")];
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// B.2 review: what is kept is bounded, the oldest dropped first: 100 pages of no one's (here the
// page kept on install and the first item go), and 50 of a session.
#[test]
fn b2_the_kept_pages_are_bounded_the_oldest_dropped_first() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = dir("sw-bounded", BOUNDED);
    let (_s, port) = logged(&d, &[]);
    let r = in_chrome_staged(&chrome, &format!("http://127.0.0.1:{}/", port), &BOUNDED_SCRIPT.replace("@KIT@", SW_KIT), port);
    assert_eq!(r, serde_json::json!({ "controlled": true, "public": [100, false, false, true, true], "session": [50, false, true, true] }));
}

/// The proxy in front of a server on `port`, at `base`, in `mode` ("" closes the connection when
/// the server is down, "502" answers so).
/// It listens on port 0 and prints the port it was given (branch security).
fn proxy(port: u16, base: &str, mode: &str) -> (support::Guard, u16) {
    let child = std::process::Command::new("node").arg("-e").arg(PROXY).arg("0").arg(port.to_string()).arg(base).arg(mode).stdout(std::process::Stdio::piped()).spawn().unwrap();
    let mut g = support::Guard::new(child);
    let listen = support::port_on_stdout(&mut g.0, "the proxy", Duration::from_secs(20));
    (g, listen)
}

const PROXIED_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {};
  out.controlled = await controlled();
  out.kept = await kept();
  await stage(1);
  for (let i = 0; i < 600; i++) { const r = await fetch("/ping", { cache: "no-store" }); if (r.status === 502) { out.down = true; break; } await tick(25); }
  out.menu = shown(await look("/menu"));
  const plain = (await look("/plain")).frame.contentDocument;
  out.plain = (plain.getElementById("bad") || {}).textContent || null;
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// B.2 review: an app down behind a proxy that answers 502 (or 503, 504) for it: a navigation to a
// kept page gets its copy; one to a page not kept gets the proxy's answer.
#[test]
fn b2_a_proxys_502_for_an_app_that_is_down_gives_the_kept_copy() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = dir("sw-502", "route offline GET / = <html><body><h1>Home</h1></body></html>\nroute offline GET /menu = <html><body><h1>Menu</h1></body></html>\nroute GET /plain = <html><body><h1>Plain</h1></body></html>\nroute GET /ping = \"pong\"\n");
    let stages = support::serve(&dir("sw-stages9", STAGES).join("main.qwe"), &[]);
    let (s, port) = logged(&d, &[]);
    let (_proxy, listen) = proxy(port, "", "502");
    let (c, url, sp) = (chrome.clone(), format!("http://127.0.0.1:{}/", listen), stages.port);
    let page = std::thread::spawn(move || in_chrome_staged(&c, &url, &PROXIED_SCRIPT.replace("@KIT@", SW_KIT), sp));
    stage_reached(stages.port, 1, &page);
    drop(s);
    assert_eq!(
        page.join().unwrap(),
        serde_json::json!({ "controlled": true, "kept": { "quillo::V": ["/", "/menu"] }, "down": true, "menu": { "h1": "Menu", "kept": "iso", "offline": false, "title": "" }, "plain": "bad gateway" })
    );
}

/// A link pointed at is prefetched (Chrome's speculation rules, as Quillo writes them), and the page
/// it leads to says how it came.
fn prefetched(tag: &str) -> String {
    format!(
        "let loads = 0\nlet REPORT = \"addEventListener('load', () => {{ const n = performance.getEntriesByType('navigation')[0]; document.body.setAttribute('data-result', JSON.stringify({{ h1: document.querySelector('h1').textContent, delivery: n.deliveryType, kept: document.documentElement.getAttribute('data-kept'), controlled: !!navigator.serviceWorker.controller }})) }})\"\nroute {0}GET / = <html><body><a id=\"slow\" href=\"/slow\">slow</a><a id=\"menu\" href=\"/menu\">menu</a></body></html>\nroute GET /slow {{\n    sleep(600)\n    <html><body><h1>Slow</h1><script>{{raw(REPORT)}}</script></body></html>\n}}\nroute {0}GET /menu {{\n    loads += 1\n    set_cookie(\"seen\", str(loads))\n    <html><body><h1>Menu {{loads}}</h1><script>{{raw(REPORT)}}</script></body></html>\n}}\n",
        tag
    )
}

/// Waits for the worker where there is one; keeps `/menu` once (opened in a frame); points at the
/// link `@LINK@` and follows it.
const POINT_SCRIPT: &str = r#"(async () => {
  const sw = navigator.serviceWorker, tick = (ms) => new Promise((r) => setTimeout(r, ms));
  if ([...document.scripts].some((s) => s.textContent.includes("/_quillo/sw.js"))) { await sw.ready; for (let i = 0; i < 200 && !sw.controller; i++) await tick(20); }
  const f = document.createElement("iframe"); f.src = "/menu"; document.body.appendChild(f); await new Promise((r) => (f.onload = r)); f.remove();
  await tick(300);
  const a = document.getElementById("@LINK@");
  a.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  await tick(1500);
  a.click();
})();"#;

// B.2 review: speculation-rules prefetch works on the pages the worker controls as without one: the
// page pointed at comes from the prefetch, made once. A prefetch Quillo refuses with a 503 (its page
// sets a cookie) is not answered with the kept copy: followed, the link gives the page itself.
#[test]
fn b2_prefetch_works_under_the_worker() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    // `/menu` counts its loads (and sets a cookie): the worker's install, a prefetch, does not run it.
    for (tag, controlled, menu) in [("offline ", true, "Menu 2"), ("", false, "Menu 2")] {
        for (link, want) in [("slow", serde_json::json!({ "h1": "Slow", "delivery": "navigational-prefetch", "kept": null, "controlled": controlled })), ("menu", serde_json::json!({ "h1": menu, "delivery": "", "kept": null, "controlled": controlled }))] {
            let d = dir("sw-prefetch", &prefetched(tag));
            let (s, port) = logged(&d, &[]);
            let r = in_chrome_staged(&chrome, &format!("http://127.0.0.1:{}/", port), &POINT_SCRIPT.replace("@LINK@", link), port);
            let log = std::fs::read_to_string(&s.1).unwrap_or_default();
            assert_eq!(r, want, "{:?} {}: {}", tag, link, log);
            if link == "slow" {
                assert_eq!(log.matches("GET /slow").count(), 1, "{}", log);
            } else {
                assert!(log.contains("GET /menu -> 503") && log.contains("prefetch"), "{}", log);
            }
        }
    }
}

/// The served worker in Node, its Cache API and `fetch` made up: how often a navigation's answer
/// is copied, for each kind of answer.
const CLONES: &str = r#"const vm = require("vm");
const src = require("fs").readFileSync(process.argv[2], "utf8");
const listeners = {}, store = new Map();
const key = (r) => (typeof r === "string" ? new URL(r, "http://h/").href : r.url);
const cache = (name) => ({
  async match(r) { const m = store.get(name), u = key(r); return m.has(u) ? m.get(u).clone() : undefined; },
  async put(r, res) { store.get(name).set(key(r), res); },
  async delete(r) { return store.get(name).delete(key(r)); },
  async keys() { return [...store.get(name).keys()].map((url) => ({ url })); },
});
const caches = { async open(n) { if (!store.has(n)) store.set(n, new Map()); return cache(n); }, async has(n) { return store.has(n); }, async keys() { return [...store.keys()]; }, async delete(n) { return store.delete(n); } };
let clones = 0, next = null;
const answer = (headers) => { const res = new Response("<html><body>x</body></html>", { status: 200, headers }); const c = res.clone.bind(res); res.clone = () => { clones++; return c(); }; return res; };
const ctx = { location: new URL("http://h/_quillo/sw.js"), caches, Response, Headers, URL, console, setTimeout, Promise };
// The pages kept on install are fetched by path: answered apart, and not counted.
ctx.fetch = async (r) => (typeof r === "string" ? new Response("", { status: 404 }) : next());
ctx.self = { addEventListener: (t, f) => (listeners[t] = f), skipWaiting() {}, registration: {}, clients: { claim: async () => {} } };
vm.createContext(ctx);
vm.runInContext(src, ctx);
const navigate = async (path, headers) => {
  next = () => answer(headers);
  const waits = [];
  let responded;
  listeners.fetch({ request: { url: "http://h" + path, method: "GET", mode: "navigate", headers: new Headers() }, preloadResponse: Promise.resolve(undefined), respondWith: (p) => (responded = p), waitUntil: (p) => waits.push(p) });
  await responded;
  await Promise.all(waits);
};
(async () => {
  const out = {};
  for (const [name, path, headers] of [
    ["page", "/plain", { "content-type": "text/html" }],
    ["theirs", "/mine", { "content-type": "text/html", "x-quillo-session": "abc" }],
    ["download", "/big.zip", { "content-type": "application/zip" }],
    ["events", "/feed", { "content-type": "text/event-stream" }],
    ["kept", "/menu", { "content-type": "text/html", "x-quillo-offline": "1" }],
  ]) {
    clones = 0;
    await navigate(path, headers);
    out[name] = clones;
  }
  console.log(JSON.stringify(out));
})();
"#;

// B.2 review: a navigation's answer is copied only when it is a page to keep, as its headers say:
// a page, a download or an event stream the worker does not keep is not held twice.
#[test]
fn b2_only_a_page_to_keep_is_copied() {
    let d = dir("sw-clones", "route offline GET / = <html><body>x</body></html>\n");
    let (_s, port) = logged(&d, &[]);
    std::fs::write(d.join("sw.js"), fetch_body(port, "/_quillo/sw.js").2).unwrap();
    std::fs::write(d.join("clones.js"), CLONES).unwrap();
    let mut c = std::process::Command::new("node");
    c.arg(d.join("clones.js")).arg(d.join("sw.js"));
    let o = within(c, 30);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let r: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&o.stdout)));
    assert_eq!(r, serde_json::json!({ "page": 0, "theirs": 0, "download": 0, "events": 0, "kept": 1 }));
}

// B.2 review: `quillo check` says where an offline route reads who the visitor is other than
// through `session()` (a cookie there or in a function it calls, a header, its group's middleware
// that reads the request, a server fn that does): its page is not kept. The session alone, a
// middleware that only hands the request back, a route of no one's: nothing said. And an offline
// route in a mounted program, whose pages the main program's worker does not keep.
#[test]
fn b2_check_says_what_will_not_be_kept() {
    let src = r#"fn theme() = cookie("theme") ?? "light"
server fn agent() -> str = request.headers["user-agent"] ?? ""
fn reads(request) {
    if request.headers["x-team"] == null {
        return redirect("/")
    }
    request
}
fn signed(request) = session().user == null ? redirect("/") : request
route offline GET /a = <html><body>{cookie("x") ?? ""}</body></html>
route offline GET /b = <html><body>{request.headers["user-agent"] ?? ""}</body></html>
route offline GET /c = <html><body>{theme()}</body></html>
route offline GET /d = <html><body>{agent()}</body></html>
routes offline /t | reads {
    route GET /e = <html><body>e</body></html>
}
routes offline /s | signed {
    route GET /f = <html><body>{session().user}</body></html>
}
route offline GET /g = <html><body>{session().user ?? "nobody"}</body></html>
route offline GET /h ? { page: int = 1 } = <html><body>{page}</body></html>
mount /blog = "blog.qwe"
"#;
    let d = dir("sw-check", src);
    std::fs::write(d.join("blog.qwe"), "route GET / = \"blog\"\nroute offline GET /post = <html><body>post</body></html>\n").unwrap();
    let o = within({
        let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"));
        c.args(["check", "main.qwe"]).current_dir(&d);
        c
    }, 30);
    let text = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    let said: Vec<&str> = text.lines().filter(|l| l.contains(": warning: ")).collect();
    let not_kept = |line: u32, route: &str| format!("main.qwe:{}: warning: `route offline {}` reads who the visitor is other than through `session()` (a cookie, a header, the request's own fields of theirs): its page is not kept for offline, and the offline page is shown in its place; read who they are with `session()`", line, route);
    assert_eq!(
        said,
        vec![
            not_kept(10, "GET /a").as_str(),
            not_kept(11, "GET /b").as_str(),
            not_kept(12, "GET /c").as_str(),
            not_kept(13, "GET /d").as_str(),
            not_kept(15, "GET /t/e").as_str(),
            "./blog.qwe:2: warning: `route offline GET /post` is in a program mounted at `/blog`, whose pages Quillo's service worker does not keep: put the route in the main program",
        ],
        "{}",
        text
    );
}

// ---- B.2, second review ------------------------------------------------------------------------

/// `script` in Node, with `makeBrowser` from tests/browser/sw-harness.mjs (the reviewer's emulation
/// of a browser around the served worker: Cache Storage, a cookie jar, opaque redirects, the
/// network switched off, the gate as sw-gate.js runs it) and `O`, the server's address: what it
/// prints, as JSON.
fn harness(port: u16, script: &str) -> serde_json::Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/sw-harness.mjs");
    let d = support::temp_dir(&format!("f67-harness-{}", script.len()));
    let text = format!("import {{ makeBrowser }} from {:?};\nconst O = \"http://127.0.0.1:{}\";\n{}", root.display().to_string(), port, script);
    std::fs::write(d.join("t.mjs"), text).unwrap();
    let mut c = std::process::Command::new("node");
    c.arg(d.join("t.mjs"));
    let o = within(c, 60);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&o.stdout)))
}

/// The second review's program: sessions keyed every way, a person changed every way, `parallel`,
/// a waiting part, pages that read a cookie, a header, the visitor's address, a route whose GET
/// writes, a redirect, a page that stops being one, and marks the program writes itself.
const SECOND: &str = r#"server fn sign_out() {
    end_session()
}
server fn slow_who() = session().user ?? "nobody"
server fn whoami() -> str = session().user ?? "nobody"
route GET /in/:who {
    set_session({ user: who })
    redirect("/")
}
route GET /uid/:who {
    set_session({ uid: who })
    redirect("/")
}
route GET /name/:who {
    set_session({ name: who })
    redirect("/")
}
route GET /gh/:t {
    set_session({ github_token: t })
    redirect("/")
}
route GET /login-start {
    set_session({ state: "s1", nonce: "n1" }, { max_age: "10m" })
    redirect("/")
}
route GET /soft-out {
    set_session({ user: null })
    redirect("/")
}
route GET /anon-cart {
    set_session({ cart: [1] })
    redirect("/")
}
route GET /cart/:n {
    set_session({ user: session().user, cart: [n] })
    redirect("/")
}
route GET /par-in {
    let [a, b] = parallel(() => set_session({ user: "zed" }), () => 1)
    "ok"
}
route GET /out {
    end_session()
    redirect("https://example.com/")
}
component Slow() {
    state w = defer(slow_who())
    <p>Slow {w ?? "..."}</p>
}
component Lw() {
    state w = live(whoami)
    <p>Live {w}</p>
}
let hidden = false
route GET /hide {
    hidden = true
    "ok"
}
route offline GET / = <html><body><h1>Home</h1></body></html>
route offline GET /mine = <html><body><h1>Mine {session().user ?? session().uid ?? session().name ?? session().github_token ?? "nobody"}</h1></body></html>
route offline GET /par {
    let [u, x] = parallel(() => session().user ?? "nobody", () => 1)
    <html><body><h1>Par {u}</h1></body></html>
}
route offline GET /parc {
    let [u, x] = parallel(() => cookie("token") ?? "none", () => 1)
    <html><body><h1>Parc {u}</h1></body></html>
}
route offline GET /def = <html><body><h1>Def</h1><Slow/></body></html>
route offline GET /lw = <html><body><h1>Lw</h1><Lw/></body></html>
route offline GET /theme = <html><body><h1>Theme {cookie("theme") ?? "light"}</h1></body></html>
route offline GET /ip = <html><body><h1>Ip {request.ip}</h1></body></html>
route offline GET /nulled = respond(200, <html><body><h1>Nulled {session().user ?? "x"}</h1></body></html>, { "cache-control": null })
route offline GET /menu = hidden ? respond(404, <html><body><h1>Gone</h1></body></html>) : <html><body><h1>Menu</h1></body></html>
route offline GET /read-all {
    write_file("marked-read.txt", "all notifications marked read by a GET")
    <html><body><h1>Read all</h1></body></html>
}
route offline GET /go = redirect("/")
route offline GET /forged = respond(200, <html><body><h1>Forged {session().user ?? "nobody"}</h1></body></html>, { "x-quillo-offline": "1", "x-quillo-session": "abc" })
route GET /plain = <html><body><h1>Plain</h1></body></html>
route GET /plain-forged = respond(200, <html><body><h1>{session().user ?? "nobody"}</h1></body></html>, { "x-quillo-offline": "1" })
route GET /ping = "pong"
"#;

/// A request's answer: its marks for the worker and the `quillo_tag` it sets (or "cleared").
fn second_marks(port: u16, path: &str, cookie: &str, extra: &[(&str, &str)]) -> (Option<String>, Option<String>, Option<String>) {
    let mut hs: Vec<(&str, &str)> = vec![];
    if !cookie.is_empty() {
        hs.push(("cookie", cookie));
    }
    hs.extend_from_slice(extra);
    let (_, h, _) = fetch(port, "GET", path, &hs);
    let tag = h.iter().find(|(k, v)| k == "set-cookie" && v.starts_with("quillo_tag=")).map(|(_, v)| {
        let value = v.split(';').next().unwrap().trim_start_matches("quillo_tag=").to_string();
        if value.is_empty() { "cleared".to_string() } else { value }
    });
    (header(&h, "x-quillo-offline").map(str::to_string), header(&h, "x-quillo-session").map(str::to_string), tag)
}

/// Follows a sign-in or a change of the session at `path` with `cookie`: the cookie after it, and
/// the name the answer gives.
fn session_step(port: u16, path: &str, cookie: &str) -> (String, String) {
    let hs: Vec<(&str, &str)> = if cookie.is_empty() { vec![] } else { vec![("cookie", cookie)] };
    let (_, h, _) = fetch(port, "GET", path, &hs);
    let next = h.iter().find(|(k, v)| k == "set-cookie" && v.contains("quillo_session=")).map(|(_, v)| v.split(';').next().unwrap().to_string()).unwrap_or_else(|| cookie.to_string());
    (next, header(&h, "x-quillo-session").unwrap_or("").to_string())
}

// Second review, 1: the session's name is a digest of its hidden id and all it holds, so that a
// change of person, which keeps the hidden id where either side has no `user`, is another name in
// each of the review's cases: a sign-in in two steps (as plugins/oidc.qwe does), `{ user: null }`,
// an anonymous cart and then a sign-in, and sessions keyed `uid`, `name` or `github_token`. Any
// write to the session is another name too (a cart: the person's kept pages are dropped, and kept
// again on the next visits); the same session written again as it was keeps it.
#[test]
fn b2_review2_the_name_changes_with_anything_in_the_session() {
    let d = dir("r2-names", SECOND);
    let (_s, port) = logged(&d, &["--no-check"]);
    let changed = |first: &str, then: &str| {
        let (c, a) = session_step(port, first, "");
        let (_, b) = session_step(port, then, &c);
        assert!(a.len() == 16 && b.len() == 16 && a != b, "{} then {}: {} {}", first, then, a, b);
    };
    changed("/login-start", "/in/bob");
    changed("/in/ana", "/soft-out");
    changed("/soft-out", "/in/bob");
    changed("/anon-cart", "/in/ana");
    changed("/uid/alice", "/uid/bob");
    changed("/name/alice", "/name/bob");
    changed("/gh/t1", "/gh/t2");
    changed("/in/ana", "/cart/1");
    let (c, a) = session_step(port, "/in/ana", "");
    assert_eq!(session_step(port, "/in/ana", &c).1, a, "the same session written again");
    // As set, as read: the page names it as the sign-in did.
    assert_eq!(second_marks(port, "/mine", &c, &[]).1, Some(a));
}

const PERSON_SCRIPT: &str = r#"
const out = {};
for (const [name, first, change] of [["soft sign-out", "/in/ana", ["/soft-out", "/in/bob"]], ["two steps", "/in/ana", ["/login-start", "/in/bob"]], ["uid", "/uid/alice", ["/uid/bob"]], ["anonymous cart", "/anon-cart", ["/in/bob"]]]) {
  const b = await makeBrowser(O);
  await b.install();
  await b.go(first);
  const kept = (await b.go("/mine")).h1;
  for (const p of change) await b.go(p);
  b.online = false;
  out[name] = { kept, offline: await b.go("/mine") };
}
console.log(JSON.stringify(out));
"#;

// Second review, 1, as the worker sees it: a page kept for one person is never shown to the next,
// whatever changed the person.
#[test]
fn b2_review2_a_kept_page_never_reaches_the_next_person() {
    let d = dir("r2-person", SECOND);
    let (_s, port) = logged(&d, &["--no-check"]);
    let r = harness(port, PERSON_SCRIPT);
    let gone = serde_json::json!({ "status": 503, "h1": "You are offline", "kept": false, "session": null, "offlinePage": true });
    assert_eq!(
        r,
        serde_json::json!({
            "soft sign-out": { "kept": "Mine ana", "offline": gone }, "two steps": { "kept": "Mine ana", "offline": gone },
            "uid": { "kept": "Mine alice", "offline": gone }, "anonymous cart": { "kept": "Mine nobody", "offline": gone },
        })
    );
}

// Second review, 2: what a `parallel` function read of the session, and a session it set, come
// back to the page: kept for that session, named as the cookie names it. A function that read a
// cookie: not kept. A waiting part (`defer`) is made after the page's headers, or for a prefetch
// after the request (where it sees no session at all): what it reads is not known, so the page is
// not kept either way, and says no name, nor takes Ana's cookie away. A `live` value is made with
// the page, which then is the session's.
#[test]
fn b2_review2_parallel_waiting_parts_and_live() {
    let d = dir("r2-parallel", SECOND);
    let (_s, port) = logged(&d, &["--no-check"]);
    let (ana, tag) = session_step(port, "/in/ana", "");
    let mine = format!("{}; quillo_tag={}", ana, tag);
    let private = Some("private".to_string());
    assert_eq!(second_marks(port, "/par", &mine, &[]), (private.clone(), Some(tag.clone()), None));
    assert_eq!(second_marks(port, "/parc", &mine, &[]).0, None);
    assert_eq!(second_marks(port, "/def", &mine, &[]).0, None);
    assert_eq!(second_marks(port, "/def", &mine, &[("x-quillo-prefetch", "1")]), (None, None, None));
    assert_eq!(second_marks(port, "/lw", &mine, &[]), (private, Some(tag.clone()), None));
    // A session set in `parallel`: the answer names it as its cookie does.
    let (_, h, _) = fetch(port, "GET", "/par-in", &[("cookie", &mine)]);
    let set = h.iter().find(|(k, v)| k == "set-cookie" && v.starts_with("quillo_tag=")).map(|(_, v)| v.split(';').next().unwrap().trim_start_matches("quillo_tag=").to_string());
    assert!(set.is_some() && header(&h, "x-quillo-session") == set.as_deref() && set.as_deref() != Some(tag.as_str()), "{:?}", h);
}

const OUT_SCRIPT: &str = r#"
const b = await makeBrowser(O);
await b.install();
await b.go("/in/ana");
await b.go("/mine");
const before = Object.keys(await b.dump()).filter((k) => /:V:/.test(k) && !k.endsWith(":-"));
// Signed out by a redirect off the site: the worker sees nothing of it.
const out = await b.go("/out");
const still = Object.keys(await b.dump()).filter((k) => /:V:/.test(k) && !k.endsWith(":-"));
// The next page of the site, any page, says no one is signed in.
await b.go("/plain");
const after = Object.keys(await b.dump()).filter((k) => /:V:/.test(k) && !k.endsWith(":-"));
console.log(JSON.stringify({ before: before.length, out: out.status, still: still.length, after: after.length }));
"#;

// Second review, 4: a page whose request came with no session cookie at all says no one is signed
// in (`x-quillo-session: -`), which any cache may give anyone; the worker then drops every session's
// kept pages. A sign-out the worker could not see (a redirect off the site) is caught at the next
// page of the site. A page whose request had a session and that read none says nothing.
#[test]
fn b2_review2_a_page_of_no_one_drops_the_sessions_kept_pages() {
    let d = dir("r2-out", SECOND);
    let (_s, port) = logged(&d, &["--no-check"]);
    assert_eq!(second_marks(port, "/plain", "", &[]), (None, Some("-".into()), None));
    assert_eq!(second_marks(port, "/", "", &[]), (Some("1".into()), Some("-".into()), None));
    let (ana, tag) = session_step(port, "/in/ana", "");
    assert_eq!(second_marks(port, "/plain", &format!("{}; quillo_tag={}", ana, tag), &[]), (None, None, None));
    assert_eq!(second_marks(port, "/", &ana, &[]), (Some("1".into()), None, None));
    assert_eq!(harness(port, OUT_SCRIPT), serde_json::json!({ "before": 1, "out": "not handled", "still": 1, "after": 0 }));
}

const OTHER_READS_SCRIPT: &str = r#"
const b = await makeBrowser(O);
await b.install();
await b.go("/in/ana");
await b.go("/mine");
const tag = b.jar.get("quillo_tag");
for (const p of ["/theme", "/ip", "/nulled", "/parc", "/plain"]) await b.go(p);
const same = b.jar.get("quillo_tag") === tag;
b.online = false;
console.log(JSON.stringify({ same, offline: await b.go("/mine"), gated: b.gated || 0 }));
"#;

// Second review, 5: `quillo_tag` is set or taken away from what is known of the session alone: a
// page that read a cookie, a header, the visitor's address or nothing at all, or whose
// `cache-control` was taken away, leaves Ana's cookie and her kept pages as they are. It is taken
// away where the request has no valid session; set again where it has one that `session()` read.
#[test]
fn b2_review2_pages_that_read_other_things_leave_the_name_alone() {
    let d = dir("r2-other", SECOND);
    let (_s, port) = logged(&d, &["--no-check"]);
    let (ana, tag) = session_step(port, "/in/ana", "");
    let mine = format!("{}; quillo_tag={}", ana, tag);
    for p in ["/theme", "/ip", "/nulled", "/parc", "/plain", "/"] {
        assert_eq!(second_marks(port, p, &mine, &[]).2, None, "{}", p);
    }
    assert_eq!(second_marks(port, "/mine", &ana, &[]).2, Some(tag.clone()), "set again where `session()` read it");
    assert_eq!(second_marks(port, "/plain", &format!("quillo_tag={}", tag), &[]).2, Some("cleared".into()), "no session cookie: taken away");
    assert_eq!(second_marks(port, "/mine", &format!("quillo_session=forged; __Host-quillo_session=forged.x; quillo_tag={}", tag), &[]).2, Some("cleared".into()), "no valid session");
    let gone = harness(port, OTHER_READS_SCRIPT);
    assert_eq!(gone, serde_json::json!({ "same": true, "offline": { "status": 200, "h1": "Mine ana", "kept": true, "session": gone["offline"]["session"], "offlinePage": false }, "gated": 0 }));
    // And a locale rule, which reads the request.
    let d = dir("r2-locale", "translations({ en: { hi: \"Hi\" }, fr: { hi: \"Salut\" } })\nlocale_from((request) => request.query.lang ?? \"en\")\nroute GET /in/:who {\n    set_session({ user: who })\n    redirect(\"/\")\n}\nroute offline GET / = <html><body><h1>{t(\"hi\")}</h1></body></html>\nroute offline GET /mine = <html><body><h1>{session().user}</h1></body></html>\n");
    let (_s2, port) = logged(&d, &[]);
    let (ana, tag) = session_step(port, "/in/ana", "");
    assert_eq!(second_marks(port, "/?lang=fr", &format!("{}; quillo_tag={}", ana, tag), &[]).2, None);
    assert_eq!(harness(port, "const b = await makeBrowser(O); await b.install(); await b.go(\"/in/ana\"); await b.go(\"/mine\"); const t = b.jar.get(\"quillo_tag\"); await b.go(\"/?lang=fr\"); b.online = false; console.log(JSON.stringify({ same: b.jar.get(\"quillo_tag\") === t, offline: (await b.go(\"/mine\")).h1 }));"), serde_json::json!({ "same": true, "offline": "ana" }));
}

/// A version whose install keeps what it can: a GET that writes, redirects here and off the site,
/// and a page that fails (503) for good.
const PRECACHED: &str = "let down = true\nroute GET /fix {\n    down = false\n    \"ok\"\n}\nroute offline GET / = <html><body><h1>Two</h1></body></html>\nroute offline GET /read-all {\n    write_file(\"marked-read.txt\", \"read by a GET\")\n    <html><body><h1>Read all</h1></body></html>\n}\nroute offline GET /here = redirect(\"/\")\nroute offline GET /away = redirect(\"https://example.com/\")\nroute offline GET /flaky = down ? respond(503, \"not now\") : <html><body><h1>Flaky</h1></body></html>\nroute GET /ping = \"pong\"\n";

const TRIES_SCRIPT: &str = r#"
const b = await makeBrowser(O);
// The caches by name (both versions' are "quillo::V" in `dump`).
const names = () => [...b.store.keys()].filter((k) => !k.endsWith(":gone"));
await b.install();
const one = names();
await fetch(O + "/stage-done");
// The new version, installed: its pages tried, the old version's kept until they are.
// The server is being started again: a refused connection is a try that did not come yet (under
// load the restart took longer than the first fetch).
for (let i = 0; i < 400; i++) { try { const r = await fetch(O + "/ping"); if ((await r.text()) === "pong") break; } catch (_) {} await new Promise((r) => setTimeout(r, 50)); }
const T0 = Date.now();
b.time = T0;
await b.update();
const out = { v1: one.length, after_install: names().length, old_kept: names().includes(one[0]) };
// Tried again on navigations: not before 10 s, then 30, 90 and 270 s after each; five in all.
const at = [5, 11, 20, 42, 133, 404, 2000];
out.versions = [];
for (const s of at) {
  b.time = T0 + s * 1000;
  await b.go("/");
  out.versions.push(names().length);
}
out.old_gone = !names().includes(one[0]);
out.kept = [...b.store.get(names()[0]).keys()].map((u) => new URL(u).pathname).filter((p) => !p.startsWith("/_quillo/"));
console.log(JSON.stringify(out));
"#;

// Second review, 6 and 7: the worker's install asks for its pages as a prefetch, which Quillo
// answers changing nothing: a GET that would write is refused (its 503 says so), not run, also
// under --no-prefetch; a redirect, here or off the site, and a refusal are done with for the
// version, never asked again. A page that does not come (a 503 of a proxy's or the program's) is
// asked again on navigations, 10 s, 30, 90 and 270 s after each try, five tries in all; after them
// the version counts as kept, and the old version's caches go.
#[test]
fn b2_review2_the_precache_changes_nothing_and_gives_up_after_five_tries() {
    for opts in [&[][..], &["--no-prefetch"][..]] {
        let d = dir("r2-precache", "route offline GET / = <html><body><h1>One</h1></body></html>\nroute offline GET /old = <html><body><h1>Old</h1></body></html>\nlet stage = 0\nroute GET /stage-done {\n    stage = 1\n    \"ok\"\n}\nroute GET /stage = stage\nroute GET /ping = \"pong 1\"\n");
        let (s, port) = logged(&d, opts);
        let script = TRIES_SCRIPT.to_string();
        let page = std::thread::spawn(move || harness(port, &script));
        let end = Instant::now() + Duration::from_secs(30);
        while support::send(port, "GET", "/stage", &[], "").body != "1" {
            assert!(Instant::now() < end && !page.is_finished(), "the harness did not reach its stage");
            std::thread::sleep(Duration::from_millis(30));
        }
        drop(s);
        std::fs::write(d.join("main.qwe"), PRECACHED).unwrap();
        let s = logged_at(&d, port, opts);
        let r = page.join().unwrap();
        let log = std::fs::read_to_string(&s.1).unwrap_or_default();
        assert_eq!(r, serde_json::json!({ "v1": 1, "after_install": 2, "old_kept": true, "versions": [2, 2, 2, 2, 2, 1, 1], "old_gone": true, "kept": ["/"] }), "{:?}: {}", opts, log);
        assert!(!d.join("marked-read.txt").exists(), "{:?}: the GET wrote", opts);
        let count = |p: &str| log.lines().filter(|l| l.starts_with(&format!("GET {} ", p))).count();
        assert_eq!((count("/read-all"), count("/here"), count("/away"), count("/flaky")), (1, 1, 1, 5), "{:?}: {}", opts, log);
        assert!(log.contains("GET /read-all -> 503"), "{}", log);
    }
}

/// Pages a busy Quillo refuses to prefetch as the worker installs (`browser.busy`): twice, then
/// served; refused for longer than the try waits; refused for every page, which share the 5 s; and
/// a 503 with no mark, a proxy's, which waits for the next try as before.
const BUSY_SCRIPT: &str = r#"
const has = async (b, p) => ((await b.dump())["quillo::V"] || []).includes(p);
const b = await makeBrowser(O);
b.busy = { path: "/", times: 2 };
await b.install();
const twice = { asked: b.busy.asked, kept: await has(b, "/") };
const c = await makeBrowser(O);
c.busy = { path: "/menu", times: 99 };
const t0 = Date.now();
await c.install();
const longer = { asked: c.busy.asked, kept: await has(c, "/menu"), other: await has(c, "/"), within: Date.now() - t0 < 6000 };
const e = await makeBrowser(O);
e.busy = { path: "", times: 99 };
const t1 = Date.now();
await e.install();
const every = { asked: e.busy.asked, within: Date.now() - t1 < 7000 };
const p = await makeBrowser(O);
p.busy = { path: "/", times: 1, plain: true };
await p.install();
const plain = { asked: p.busy.asked, kept: await has(p, "/"), other: await has(p, "/menu") };
console.log(JSON.stringify({ twice, longer, every, plain }));
"#;

// Found on CI (a debug build, where the page's own requests still ran as the worker installed):
// a page Quillo is too busy to prefetch (Q-179's 503, marked `busy`) is asked again within the
// install's try after 0.25, 0.5, 1 and 2 s, 5 s for the whole try; it had waited for a navigation
// 10 s on, and under steady load was never kept. A 503 without the mark waits for the next try.
#[test]
fn b2_a_page_quillo_is_too_busy_to_prefetch_is_asked_again_as_the_worker_installs() {
    let d = dir("sw-busy", "route offline GET / = <html><body><h1>Home</h1></body></html>\nroute offline GET /menu = <html><body><h1>Menu</h1></body></html>\n");
    let (_s, port) = logged(&d, &[]);
    let r = harness(port, BUSY_SCRIPT);
    // "/menu" five times (four waits, 3.75 s), then "/" with the 1.25 s left: 0.25 and 0.5, and
    // asked no more at 1 s (three in all).
    assert_eq!(
        r,
        serde_json::json!({
            "twice": { "asked": 3, "kept": true },
            "longer": { "asked": 5, "kept": false, "other": true, "within": true },
            "every": { "asked": 8, "within": true },
            "plain": { "asked": 1, "kept": false, "other": true },
        })
    );
}

const BUSY_SLOW: &str = r#"
fn work() {
    let t = 0
    for i in range(0, 300000) {
        t = t + len(str(i))
    }
    return t
}
route offline GET / {
    let n = work()
    return <html><body><h1>made {n}</h1></body></html>
}
route GET /no = respond(503, "not now")
"#;

// Found on CI: Q-179's 503, for a prefetch while others wait, says so to Quillo's worker
// (`x-quillo-prefetch: busy`); a route's own 503 does not, and a page made is not marked.
#[test]
fn b2_a_busy_servers_refusal_of_a_prefetch_is_marked_busy() {
    let d = dir("busy-mark", BUSY_SLOW);
    let (_s, port) = logged(&d, &[]);
    let guesses: Vec<_> = (0..24)
        .map(|_| std::thread::spawn(move || {
            let a = support::send(port, "GET", "/", &[("x-quillo-prefetch", "1")], "");
            (a.status, a.header("x-quillo-prefetch").map(str::to_string))
        }))
        .collect();
    let answers: Vec<(u16, Option<String>)> = guesses.into_iter().map(|g| g.join().unwrap()).collect();
    assert!(answers.iter().any(|(s, _)| *s == 503), "{:?}", answers);
    for (status, mark) in &answers {
        match status {
            503 => assert_eq!(mark.as_deref(), Some("busy"), "{:?}", answers),
            200 => assert_eq!(mark.as_deref(), None, "{:?}", answers),
            _ => panic!("{:?}", answers),
        }
    }
    let own = support::send(port, "GET", "/no", &[], "");
    assert_eq!((own.status, own.header("x-quillo-prefetch")), (503, None));
}

const FORGET_SCRIPT: &str = r#"
const b = await makeBrowser(O);
await b.install();
await b.go("/in/ana");
const forged = await b.go("/forged");
await b.go("/menu");
const before = (await b.dump())["quillo::V"].includes("/menu");
await fetch(O + "/hide");
const now = await b.go("/menu");
const after = (await b.dump())["quillo::V"].includes("/menu");
b.online = false;
console.log(JSON.stringify({ forged: forged.h1, before, now: now.h1, after, offline: (await b.go("/menu")).offlinePage, forged_offline: (await b.go("/forged")).session !== null }));
"#;

// Second review, from the suspicions: the marks a program's own answer carries are taken off, so
// that only Quillo says what its worker keeps (a page of Ana's marked `1` would have been kept for
// everyone): an offline route's page is marked as what it read, and a page of no offline route
// not at all. An address answered without the mark (a 404 now) loses the copy kept of it.
#[test]
fn b2_review2_the_programs_own_marks_are_not_taken_and_an_unmarked_answer_drops_the_copy() {
    let d = dir("r2-forget", SECOND);
    let (_s, port) = logged(&d, &["--no-check"]);
    let (ana, tag) = session_step(port, "/in/ana", "");
    let mine = format!("{}; quillo_tag={}", ana, tag);
    assert_eq!(second_marks(port, "/forged", &mine, &[]), (Some("private".into()), Some(tag.clone()), None));
    assert_eq!(second_marks(port, "/plain-forged", &mine, &[]), (None, Some(tag.clone()), None));
    assert_eq!(second_marks(port, "/plain-forged", "", &[]), (None, Some("-".into()), None));
    assert_eq!(harness(port, FORGET_SCRIPT), serde_json::json!({ "forged": "Forged ana", "before": true, "now": "Gone", "after": false, "offline": true, "forged_offline": true }));
}

/// A session's pages in every shape the review found: a conditional comment before the real
/// `<html>`, a fragment, a page with no head, `<html>` in a script's string before the real one,
/// and a comment before the doctype.
const SHAPES: &str = r#"route GET /in/:who {
    set_session({ user: who })
    redirect("/")
}
route offline GET / = <html><body><h1>Home</h1></body></html>
route offline GET /cond = respond(200, "<!doctype html><!--[if IE 9]><html class=\"ie9\"><![endif]--><!--[if gt IE 9]><!--><html lang=\"fr\" class=\"modern\"><!--<![endif]--><head><title>Cond</title></head><body><h1>Cond " + (session().user ?? "nobody") + "</h1></body></html>", { "content-type": "text/html; charset=utf-8" })
route offline GET /frag = <div><h1>Frag {session().user ?? "nobody"}</h1></div>
route offline GET /nohead = <html lang="de"><body><h1>Nohead {session().user ?? "nobody"}</h1></body></html>
route offline GET /str = respond(200, "<!doctype html><script>window.s = \"<html data-x=1><head>\"</script><html lang=\"es\"><head><title>Str</title></head><body><h1>Str " + (session().user ?? "nobody") + "</h1></body></html>", { "content-type": "text/html; charset=utf-8" })
route offline GET /note = respond(200, "<!-- a note --><!doctype html><html lang=\"it\"><body><h1>Note " + (session().user ?? "nobody") + "</h1></body></html>", { "content-type": "text/html; charset=utf-8" })
route offline GET /c1 = respond(200, "<!--><html lang=\"pt\"><body><h1>C1 " + (session().user ?? "nobody") + "</h1></body></html>", { "content-type": "text/html; charset=utf-8" })
route offline GET /c2 = respond(200, "<!---><!doctype html><html lang=\"nl\"><body><h1>C2 " + (session().user ?? "nobody") + "</h1></body></html>", { "content-type": "text/html; charset=utf-8" })
route offline GET /c3 = respond(200, "<!-- a --!><!doctype html><html lang=\"sv\"><body><h1>C3 " + (session().user ?? "nobody") + "</h1></body></html>", { "content-type": "text/html; charset=utf-8" })
route offline GET /c4 = respond(200, "<!-- never closed <html><body><h1>C4 " + (session().user ?? "nobody") + "</h1>", { "content-type": "text/html; charset=utf-8" })
route GET /ping = "pong"
"#;

const SHAPES_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {};
  out.controlled = await controlled();
  await look("/in/ana");
  const paths = ["/cond", "/frag", "/nohead", "/str", "/note", "/c1", "/c2", "/c3", "/c4"];
  for (const p of paths) await look(p);
  await stage(1);
  out.down = await down();
  const parse = async (p, gated) => {
    const f = await frame(p);
    for (let i = 0; i < 200 && (f.contentDocument.readyState !== "complete" || (gated && f.contentDocument.documentElement.hasAttribute("hidden"))); i++) await tick(25);
    const d = f.contentDocument, h = d.documentElement, first = d.head && d.head.firstElementChild;
    return {
      h1: (d.querySelector("h1") || {}).textContent || null, lang: h.getAttribute("lang"), cls: h.getAttribute("class"), x: h.getAttribute("data-x"),
      kept: /^\d{4}-/.test(h.getAttribute("data-kept") || ""), session: h.hasAttribute("data-kept-session"), hidden: h.hasAttribute("hidden"),
      gate_first: !!first && first.tagName === "SCRIPT" && first.textContent.includes("data-kept-session"), s: f.contentWindow.s === undefined ? null : f.contentWindow.s,
      mode: d.compatMode, title: d.title,
    };
  };
  for (const p of paths) out[p] = await parse(p, true);
  out["/"] = await parse("/", false);
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// Second review, 3: the worker puts its marks in an `<html>` start tag of its own before all of
// the page but its doctype and comments, and for a session's page a head with the gate, and
// changes nothing later; the browser merges the page's own `<html>` attributes into it and leaves
// out its second head. In Chrome, with the server stopped, each shape is parsed so: the gate first
// in the head, run before the page, the page's own `lang` and `class` on the root (the conditional
// comment's not), a script's string as written, the doctype's standards mode kept.
#[test]
fn b2_review2_the_marks_go_before_the_page_which_the_browser_merges() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = dir("r2-shapes", SHAPES);
    let stages = support::serve(&dir("r2-stages1", STAGES).join("main.qwe"), &[]);
    let (s, port) = logged(&d, &["--no-check"]);
    let (c, url, sp) = (chrome.clone(), format!("http://127.0.0.1:{}/", port), stages.port);
    let page = std::thread::spawn(move || in_chrome_staged(&c, &url, &SHAPES_SCRIPT.replace("@KIT@", SW_KIT), sp));
    stage_reached(stages.port, 1, &page);
    drop(s);
    let r = page.join().unwrap();
    let shown = |h1: &str, lang: Option<&str>, cls: Option<&str>, title: &str, mode: &str| {
        serde_json::json!({ "h1": h1, "lang": lang, "cls": cls, "x": null, "kept": true, "session": true, "hidden": false, "gate_first": true, "s": null, "mode": mode, "title": title })
    };
    let mut str_page = shown("Str ana", Some("es"), None, "Str", "CSS1Compat");
    str_page["s"] = serde_json::json!("<html data-x=1><head>");
    assert_eq!(
        r,
        serde_json::json!({
            "controlled": true, "down": true,
            "/cond": shown("Cond ana", Some("fr"), Some("modern"), "Cond", "CSS1Compat"),
            "/frag": shown("Frag ana", None, None, "", r["/frag"]["mode"].as_str().unwrap_or("")),
            "/nohead": shown("Nohead ana", Some("de"), None, "", "CSS1Compat"),
            "/str": str_page,
            "/note": shown("Note ana", Some("it"), None, "", "CSS1Compat"),
            // Third review, 4: `<!-->`, `<!--->` and `--!>` end comments, as the browser reads
            // them; a comment never closed is all the page, the gate before it.
            "/c1": shown("C1 ana", Some("pt"), None, "", "BackCompat"),
            "/c2": shown("C2 ana", Some("nl"), None, "", "CSS1Compat"),
            "/c3": shown("C3 ana", Some("sv"), None, "", "CSS1Compat"),
            "/c4": { "h1": null, "lang": null, "cls": null, "x": null, "kept": true, "session": true, "hidden": false, "gate_first": true, "s": null, "mode": "BackCompat", "title": "" },
            "/": { "h1": "Home", "lang": null, "cls": null, "x": null, "kept": true, "session": false, "hidden": false, "gate_first": false, "s": null, "mode": "CSS1Compat", "title": "" },
        })
    );
}

/// A session's page and a page of no one's, neither with a component (no runtime on them), and a
/// sign-out that leaves the site.
fn told(stages: u16) -> String {
    format!("route GET /in/:who {{\n    set_session({{ user: who }})\n    redirect(\"/\")\n}}\nroute GET /out {{\n    end_session()\n    redirect(\"http://127.0.0.1:{}/stage\")\n}}\nroute offline GET / = <html><body><h1>Home</h1></body></html>\nroute offline GET /mine = <html><body><h1>Mine {{session().user ?? \"nobody\"}}</h1></body></html>\nroute GET /ping = \"pong\"\n", stages)
}

const TOLD_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {};
  out.controlled = await controlled();
  await look("/in/ana");
  out.mine = (await look("/mine")).h1;
  out.signed = await kept();
  // Signed out by a redirect off the site, which the worker does not see.
  await frame("/out");
  out.cookie = tagNow();
  await stage(1);
  out.down = await down();
  out.still = await kept();
  // Offline, a page of no one's, which has no runtime: its register script tells the worker.
  out.home = (await look("/")).h1;
  for (let i = 0; i < 100 && Object.keys(await kept()).length > 1; i++) await tick(25);
  out.after = await kept();
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// Second review, 4: an offline route's page tells Quillo's worker which session the browser holds
// (its register script, as the runtime does on pages with components): a sign-out the worker did
// not see (a redirect off the site), the server then stopped, drops that session's kept pages at
// the next page opened, one with no component, kept, from the cache.
#[test]
fn b2_review2_a_page_without_components_tells_the_worker_the_session() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let stages = support::serve(&dir("r2-stages2", STAGES).join("main.qwe"), &[]);
    let d = dir("r2-told", &told(stages.port));
    let (s, port) = logged(&d, &[]);
    let (c, url, sp) = (chrome.clone(), format!("http://127.0.0.1:{}/", port), stages.port);
    let page = std::thread::spawn(move || in_chrome_staged(&c, &url, &TOLD_SCRIPT.replace("@KIT@", SW_KIT), sp));
    stage_reached(stages.port, 1, &page);
    drop(s);
    let public = serde_json::json!({ "quillo::V": ["/"] });
    // The sign-out off the site no longer goes unseen: the worker reads `quillo_tag` after the
    // redirect, and hears of its change, and drops the session's pages at once, before the server
    // stops (`still`) (changed with FEEDBACK75.md, Q-1082; branch feedback75).
    assert_eq!(
        page.join().unwrap(),
        serde_json::json!({ "controlled": true, "mine": "Mine ana", "signed": { "quillo::V": ["/"], "quillo::V:S": ["/mine"] }, "cookie": "-", "down": true, "still": public, "home": "Home", "after": public })
    );
}

/// A program of no offline route, whose own service worker writes down the messages it gets; a page
/// with a component (the runtime on it).
const OWN_WORKER: &str = r#"let OWN = "self.addEventListener('install', () => self.skipWaiting()); self.addEventListener('activate', (e) => e.waitUntil(clients.claim())); self.addEventListener('message', (e) => e.waitUntil(caches.open('got').then((c) => c.put('/m/' + Date.now() + Math.random(), new Response(JSON.stringify(e.data))))));"
route GET /own.js = respond(200, OWN, { "content-type": "text/javascript" })
component C() {
    state n = 0
    <button id="b" onclick={() => { n += 1 }}>{n}</button>
}
route GET / = <html><body><h1>Home</h1><script>navigator.serviceWorker.register("/own.js")</script></body></html>
route GET /c = <html><body><C/></body></html>
"#;

const OWN_SCRIPT: &str = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms)), sw = navigator.serviceWorker;
  await sw.ready;
  for (let i = 0; i < 300 && !sw.controller; i++) await tick(20);
  const got = async () => { const c = await caches.open("got"); return Promise.all((await c.keys()).map(async (k) => (await c.match(k)).json())); };
  // A page with the runtime, controlled by the program's own worker.
  const f = document.createElement("iframe");
  f.src = "/c";
  document.body.appendChild(f);
  await new Promise((r) => (f.onload = r));
  for (let i = 0; i < 200 && !f.contentWindow[Symbol.for("quillo")]; i++) await tick(25);
  await tick(500);
  const before = await got();
  sw.controller.postMessage({ probe: 1 });
  for (let i = 0; i < 100 && (await got()).length === before.length; i++) await tick(25);
  document.body.dataset.result = JSON.stringify({ controlled: !!f.contentWindow.navigator.serviceWorker.controller, before, after: await got() });
})();"#;

// Second review, from the suspicions: the runtime tells the session to Quillo's worker alone; a
// worker of the program's own controlling the page gets nothing from it (a message the page sends
// it does arrive).
#[test]
fn b2_review2_the_runtime_tells_quillos_worker_alone() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let d = dir("r2-own", OWN_WORKER);
    let (_s, port) = logged(&d, &[]);
    let r = in_chrome_staged(&chrome, &format!("http://127.0.0.1:{}/", port), OWN_SCRIPT, port);
    assert_eq!(r, serde_json::json!({ "controlled": true, "before": [], "after": [{ "probe": 1 }] }));
}

const LATE_SCRIPT: &str = r#"
const b = await makeBrowser(O);
await b.install();
await b.go("/in/ana");
await b.go("/mine");
const mine = async () => Object.keys(await b.dump()).filter((k) => /:V:/.test(k) && !k.endsWith(":-")).length;
const before = await mine();
// Signed out elsewhere, as a page tells it: ana's pages go.
await b.message({ quillo: "session", who: "-" });
const after = await mine();
// An answer of her session that comes later (the server still takes her cookie): not kept again.
const late = (await b.go("/mine")).h1;
console.log(JSON.stringify({ before, after, late, not_again: await mine() }));
"#;

// Second review (the race of the first review's finding 4): an answer that carries the name of a
// session the worker dropped is not kept again, nor taken for the session the browser holds.
#[test]
fn b2_review2_a_late_answer_of_a_dropped_session_is_not_kept() {
    let d = dir("r2-late", SECOND);
    let (_s, port) = logged(&d, &["--no-check"]);
    assert_eq!(harness(port, LATE_SCRIPT), serde_json::json!({ "before": 1, "after": 0, "late": "Mine ana", "not_again": 0 }));
}

const REFUSED_SCRIPT: &str = r#"
const b = await makeBrowser(O);
await b.install();
await b.go("/menu");
const kept = () => b.dump().then((d) => d["quillo::V"].includes("/menu"));
const before = await kept();
// A prefetch of it (the browser's speculation rules), which Quillo refuses: it sets a cookie.
const pre = await b.go("/menu", "GET", undefined, { "sec-purpose": "prefetch" });
console.log(JSON.stringify({ before, prefetch: pre.status, after: await kept() }));
"#;

// Second review, from the answer to "an unmarked answer drops the copy": a prefetch through the
// worker that Quillo refuses (its 503: the page would set a cookie) says nothing of the page, and
// leaves its kept copy.
#[test]
fn b2_review2_a_refused_prefetch_leaves_the_kept_copy() {
    let d = dir("r2-refused", "let n = 0\nroute offline GET / = <html><body><h1>Home</h1></body></html>\nroute offline GET /menu {\n    set_cookie(\"seen\", \"1\")\n    <html><body><h1>Menu</h1></body></html>\n}\n");
    let (_s, port) = logged(&d, &[]);
    assert_eq!(harness(port, REFUSED_SCRIPT), serde_json::json!({ "before": true, "prefetch": 503, "after": true }));
}

// ---- B.2, third review -------------------------------------------------------------------------

const THIRD: &str = r#"fn mcp ping() = "pong"
route GET /in/:who {
    set_session({ user: who })
    redirect("/")
}
route GET /short/:who {
    set_session({ user: who }, { max_age: 1 })
    redirect("/")
}
route offline GET / = <html><body><h1>Home</h1></body></html>
route offline GET /mine = <html><body><h1>{session().user ?? "nobody"}</h1></body></html>
route offline GET /bye {
    let u = session().user ?? "nobody"
    end_session()
    <html><body><h1>Bye {u}</h1></body></html>
}
route offline GET /bye2 {
    let u = session().user ?? "nobody"
    set_session({})
    <html><body><h1>Bye {u}</h1></body></html>
}
route offline GET /hello {
    set_session({ visits: 1 })
    <html><body><h1>Hello</h1></body></html>
}
route offline GET /dunder = <html><body><h1>{request.path} {__request.cookies.secret ?? "none"}</h1></body></html>
route offline GET /upper = respond(200, "<!doctype html><html><head><style>html{display:block}</style></head><body><h1>" + (session().user ?? "nobody") + "</h1></body></html>", { "content-type": "Text/HTML; charset=utf-8", "content-security-policy": "script-src 'self'; style-src 'unsafe-inline'" })
route offline GET /sandboxed = respond(200, <html><body><h1>Sandboxed {session().user ?? "nobody"}</h1></body></html>, { "content-security-policy": "sandbox allow-same-origin" })
route offline GET /token = <html><body><h1>Token {mcp_token()}</h1></body></html>
mount /blog = "blog.qwe"
route GET /ping = "pong"
"#;

/// A mounted program that writes the worker's marks itself.
const MOUNTED: &str = "route GET /forged = respond(200, <html><body><h1>Blog</h1></body></html>, { \"x-quillo-offline\": \"1\", \"x-quillo-session\": \"abc\" })\n";

fn third_dir(name: &str) -> PathBuf {
    let d = dir(name, THIRD);
    std::fs::write(d.join("blog.qwe"), MOUNTED).unwrap();
    d
}

// Third review, 1: an answer that set or ended the session is never kept, private or public: one
// that read Ana and then ended or emptied the session would have been kept under the new
// session's name with Ana's name on it; one that only set it, too.
#[test]
fn b3_an_answer_that_changed_the_session_is_not_kept() {
    let d = third_dir("r3-changed");
    let (_s, port) = logged(&d, &["--no-check"]);
    let (ana, tag) = session_step(port, "/in/ana", "");
    let mine = format!("{}; quillo_tag={}", ana, tag);
    for p in ["/bye", "/bye2", "/hello"] {
        let (offline, said, _) = second_marks(port, p, &mine, &[]);
        assert_eq!(offline, None, "{}", p);
        assert!(said.is_some(), "{}: it still says the session it leaves", p);
    }
    let gone = harness(port, "const b = await makeBrowser(O); await b.install(); await b.go(\"/in/ana\"); const on = (await b.go(\"/bye\")).h1; b.online = false; console.log(JSON.stringify({ on, off: (await b.go(\"/bye\")).offlinePage }));");
    assert_eq!(gone, serde_json::json!({ "on": "Bye ana", "off": true }));
}

// Third review, 2: `__request`, the request as sent, is read as `request` is: a page showing its
// cookies is the visitor's, `private` with `Vary: Cookie` for caches in a program with no offline
// route (Q-886's marking, which it got past), and not kept for offline in one with them.
#[test]
fn b3_dunder_request_is_the_visitors() {
    let d = third_dir("r3-dunder");
    let (_s, port) = logged(&d, &["--no-check"]);
    assert_eq!(second_marks(port, "/dunder", "secret=s1", &[]).0, None);
    let d = dir("r3-dunder-plain", "route GET /d = <html><body><h1>{__request.cookies.secret ?? \"none\"}</h1></body></html>\nroute GET /n = <html><body><h1>{request.path}</h1></body></html>\n");
    let (_s2, port) = logged(&d, &[]);
    let (_, h, body) = fetch(port, "GET", "/d", &[("cookie", "secret=s1")]);
    assert!(String::from_utf8_lossy(&body).contains("s1"));
    assert_eq!((header(&h, "cache-control"), header(&h, "vary")), (Some("private"), Some("Cookie")), "{:?}", h);
    let (_, h, _) = fetch(port, "GET", "/n", &[("cookie", "secret=s1")]);
    assert_eq!(header(&h, "cache-control"), None, "a page of nobody's stays so: {:?}", h);
}

// Third review, 3: a page whose content type is written `Text/HTML` is a page: it gets the
// register script, and its policy names the gate, which its kept copy needs.
#[test]
fn b3_a_mixed_case_content_type_is_a_page() {
    let d = third_dir("r3-upper");
    let (_s, port) = logged(&d, &["--no-check"]);
    let (ana, tag) = session_step(port, "/in/ana", "");
    let (_, h, body) = fetch(port, "GET", "/upper", &[("cookie", &format!("{}; quillo_tag={}", ana, tag))]);
    assert_eq!(header(&h, "x-quillo-offline"), Some("private"));
    assert!(header(&h, "content-security-policy").unwrap_or("").contains(&policy_hash(quillo::pwa::gate())), "{:?}", h);
    assert!(String::from_utf8_lossy(&body).contains(&register_script("")));
}

// Third review, 5 and the rest: a session cookie the page did not read, which no longer opens
// (forged, or past its time): no one, as no cookie is (`-`), and `quillo_tag` goes; a valid one
// says nothing. `-` is said on a GET's answer only (and a server fn's). A mounted program's own
// marks are taken off. A page that made an MCP token (shown once) is not kept.
#[test]
fn b3_a_session_the_server_no_longer_takes_is_no_one() {
    let d = third_dir("r3-ended");
    let (_s, port) = logged(&d, &["--no-check"]);
    let (ana, tag) = session_step(port, "/in/ana", "");
    let mine = format!("{}; quillo_tag={}", ana, tag);
    assert_eq!(second_marks(port, "/", &mine, &[]), (Some("1".into()), None, None));
    let forged = format!("{}x; quillo_tag={}", ana, tag);
    assert_eq!(second_marks(port, "/", &forged, &[]), (Some("1".into()), Some("-".into()), Some("cleared".into())));
    let (short, short_tag) = session_step(port, "/short/bea", "");
    std::thread::sleep(Duration::from_millis(2100));
    assert_eq!(second_marks(port, "/", &format!("{}; quillo_tag={}", short, short_tag), &[]), (Some("1".into()), Some("-".into()), Some("cleared".into())));
    // Not on a POST's answer.
    let (_, h, _) = fetch(port, "POST", "/", &[("cookie", &forged)]);
    assert_eq!(header(&h, "x-quillo-session"), None);
    // A mounted program's own marks.
    let (_, h, _) = fetch(port, "GET", "/blog/forged", &[("cookie", &mine)]);
    assert_eq!((header(&h, "x-quillo-offline"), header(&h, "x-quillo-session")), (None, None), "{:?}", h);
    // An MCP token on the page.
    let (status, h, body) = fetch(port, "GET", "/token", &[("cookie", &mine)]);
    assert!(status == 200 && String::from_utf8_lossy(&body).contains("Token "), "{}", String::from_utf8_lossy(&body));
    assert_eq!(header(&h, "x-quillo-offline"), None);
    // As the worker sees it: Ana's copies go at the first page after her cookie stopped opening.
    let r = harness(port, "const b = await makeBrowser(O); await b.install(); await b.go(\"/in/ana\"); await b.go(\"/mine\"); const k = b.jar.keys().next().value; b.jar.set(k, b.jar.get(k) + \"x\"); await b.go(\"/\"); const left = Object.keys(await b.dump()).filter((n) => /:V:/.test(n) && !n.endsWith(\":-\")).length; b.online = false; console.log(JSON.stringify({ left, tag: b.jar.has(\"quillo_tag\"), offline: (await b.go(\"/mine\")).offlinePage }));");
    assert_eq!(r, serde_json::json!({ "left": 0, "tag": false, "offline": true }));
}

// Third review, 3: a session's kept page whose policy would stop the gate (a `sandbox` without
// `allow-scripts`) is not given back: the offline page instead. Without the gate its own sheet
// could have shown it.
#[test]
fn b3_a_kept_page_whose_policy_stops_the_gate_is_not_given_back() {
    let d = third_dir("r3-sandbox");
    let (_s, port) = logged(&d, &["--no-check"]);
    let r = harness(port, "const b = await makeBrowser(O); await b.install(); await b.go(\"/in/ana\"); await b.go(\"/sandboxed\"); await b.go(\"/mine\"); const kept = (await b.dump()); b.online = false; console.log(JSON.stringify({ kept: Object.entries(kept).filter(([n]) => /:V:/.test(n)).map(([, v]) => v.sort()), sandboxed: (await b.go(\"/sandboxed\")).offlinePage, mine: (await b.go(\"/mine\")).h1 }));");
    assert_eq!(r, serde_json::json!({ "kept": [["/mine", "/sandboxed"]], "sandboxed": true, "mine": "ana" }));
}

/// The worker's own `start` and `gated`, taken from the served text, run in Node on pages and
/// policies.
const WORKER_PARTS: &str = r#"const fs = require("fs");
const src = fs.readFileSync(process.argv[2], "utf8");
const GATE_HASH = /GATE_HASH = ("[^"]*")/.exec(src)[1];
const body = src.slice(src.indexOf("function start(html)"), src.indexOf("async function state()"));
const [start, gated] = new Function("const GATE_HASH = " + GATE_HASH + ";\n" + body + "; return [start, gated];")();
const long = "<!--" + "x".repeat(5000) + "-->";
const pages = {
  empty_comment: "<!--><html><body><h1>a</h1></body></html>",
  dash_comment: "<!---><html><body><h1>a</h1></body></html>",
  bang_end: "<!-- a --!><html><body><h1>a</h1></body></html>",
  unclosed: "<!-- never closed <html><body><h1>a</h1>",
  four_dashes: "<!----><html>",
  bom_doctype: "\uFEFF<!doctype html><!-- x --> <html>",
  pi: "<?xml version=\"1.0\"?><html>",
  nbsp: "\u00a0<html>",
  past_4kb: long + "<html>",
  doctype_unclosed: "<!doctype html",
};
const at = {};
for (const [k, p] of Object.entries(pages)) at[k] = start(p);
const h = (p) => new Headers(p === null ? {} : { "content-security-policy": p });
const policies = {
  none: null, self: "script-src 'self'", hashed: "script-src 'self' " + JSON.parse(GATE_HASH), inline: "script-src 'unsafe-inline'",
  inline_nonce: "script-src 'unsafe-inline' 'nonce-x'", default_only: "default-src 'self'", style_only: "style-src 'self'",
  sandbox: "sandbox", sandbox_same_origin: "sandbox allow-same-origin", sandbox_scripts: "sandbox allow-scripts",
  two: "script-src 'unsafe-inline', script-src 'self'",
};
const runs = {};
for (const [k, p] of Object.entries(policies)) runs[k] = gated(h(p));
console.log(JSON.stringify({ at, runs }));
"#;

// Third review, 4 and 3: where the worker puts its marks, by the HTML standard's comment states:
// `<!-->` and `<!--->` end at once, `--!>` ends a comment, `<?...>` and an unclosed doctype as
// bogus comments; a comment never closed, or no content in the first 4 KB, puts them at the very
// start after a byte-order mark; a no-break space is content. And whether a kept policy lets
// the gate run: no policy, inline scripts let in, its hash named, a sandbox with scripts; not
// `'self'` alone, a nonce beside `'unsafe-inline'`, `default-src` without inline, a sandbox
// without scripts, nor one policy of two that refuses.
#[test]
fn b3_where_the_marks_go_and_whether_the_gate_runs() {
    let d = third_dir("r3-parts");
    let (_s, port) = logged(&d, &["--no-check"]);
    let (status, _, worker) = fetch_body(port, "/_quillo/sw.js");
    assert!(status == 200 && worker.contains("GATE_HASH = "), "{} {}", status, worker.len());
    std::fs::write(d.join("sw.js"), worker).unwrap();
    std::fs::write(d.join("parts.js"), WORKER_PARTS).unwrap();
    let mut c = std::process::Command::new("node");
    c.arg(d.join("parts.js")).arg(d.join("sw.js"));
    let o = within(c, 30);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let r: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        r,
        serde_json::json!({
            "at": { "empty_comment": 5, "dash_comment": 6, "bang_end": 11, "unclosed": 0, "four_dashes": 7, "bom_doctype": 27, "pi": 21, "nbsp": 0, "past_4kb": 0, "doctype_unclosed": 0 },
            // A sandbox with scripts but not `allow-same-origin` stops the gate too: its page's origin
            // is opaque, where the gate's `document.cookie` throws (changed with FEEDBACK75.md,
            // Q-1078; branch feedback75).
            "runs": { "none": true, "self": false, "hashed": true, "inline": true, "inline_nonce": false, "default_only": false, "style_only": true, "sandbox": false, "sandbox_same_origin": false, "sandbox_scripts": false, "two": false },
        })
    );
}

// FEEDBACK68.md, Q-985 (found in FEEDBACK67's third review): a page state's initializer that reads
// the session ran before the request was there: it found none, and so did every `session()` of
// that request after it, the session kept as none. Now it reads the request's (its path and
// query too, through `url()`), and the page is the visitor's: private, and for offline kept for
// that session.
#[test]
fn q985_a_page_states_initializer_reads_the_session() {
    let src = "state who = session().user ?? \"nobody\"\nstate at = url().path\nroute GET /in/:who {\n    set_session({ user: who })\n    redirect(\"/\")\n}\nroute GET /page = <html><body><h1>{who} {session().user ?? \"nobody\"} {at}</h1></body></html>\nroute offline GET /kept = <html><body><h1>{who}</h1></body></html>\n";
    let d = dir("q985", src);
    assert!(check(&d).0, "{:?}", check(&d));
    let (_s, port) = logged(&d, &[]);
    let (ana, tag) = session_step(port, "/in/ana", "");
    let (_, h, body) = fetch(port, "GET", "/page", &[("cookie", &ana)]);
    assert!(String::from_utf8_lossy(&body).contains("<h1>ana ana /page</h1>"), "{}", String::from_utf8_lossy(&body));
    assert_eq!(header(&h, "cache-control"), Some("private"));
    let (_, _, body) = fetch(port, "GET", "/page", &[]);
    assert!(String::from_utf8_lossy(&body).contains("<h1>nobody nobody /page</h1>"));
    assert_eq!(second_marks(port, "/kept", &format!("{}; quillo_tag={}", ana, tag), &[]), (Some("private".into()), Some(tag), None));
}


// ---- B.3: notifications (slice 4) ---------------------------------------------------------------

/// The push service and the browser's keys, played by Node: a sink on 127.0.0.1 that takes what
/// Quillo posts, decrypts it by RFC 8291 with Node's own crypto (an implementation apart from
/// Quillo's: the probe's, .git/wip/feedback67/probe-push-decrypt.mjs) and checks its VAPID header by
/// RFC 8292. `/_made?n=K&kind=ok|404|410|500|slow|redir|noisy`: K subscriptions, their keys made as
/// `pushManager.subscribe` makes them, whose push service answers 201 or as `kind` says; `/_got`:
/// what came, in order, with the connection it came on; `/_reset`.
const SINK: &str = r#"const crypto = require("crypto"), http = require("http");
let PORT = +process.argv[2];
const subs = new Map();
let got = [], n = 0;
const b64u = (b) => Buffer.from(b).toString("base64url");
function open(path, headers, body) {
  const s = subs.get(path);
  const out = { path, ttl: headers.ttl, urgency: headers.urgency, content_encoding: headers["content-encoding"], content_type: headers["content-type"], length: body.length };
  try {
    const salt = body.subarray(0, 16), rs = body.readUInt32BE(16), idlen = body[20], asPub = body.subarray(21, 21 + idlen), ct = body.subarray(21 + idlen);
    const shared = s.ecdh.computeSecret(asPub);
    const keyInfo = Buffer.concat([Buffer.from("WebPush: info\0"), s.ecdh.getPublicKey(), asPub]);
    const ikm = Buffer.from(crypto.hkdfSync("sha256", shared, s.auth, keyInfo, 32));
    const cek = Buffer.from(crypto.hkdfSync("sha256", ikm, salt, Buffer.from("Content-Encoding: aes128gcm\0"), 16));
    const nonce = Buffer.from(crypto.hkdfSync("sha256", ikm, salt, Buffer.from("Content-Encoding: nonce\0"), 12));
    const d = crypto.createDecipheriv("aes-128-gcm", cek, nonce);
    d.setAuthTag(ct.subarray(ct.length - 16));
    const plain = Buffer.concat([d.update(ct.subarray(0, ct.length - 16)), d.final()]);
    let end = plain.length - 1;
    while (end > 0 && plain[end] === 0) end--;
    Object.assign(out, { record_size: rs, key_length: idlen, delimiter: plain[end], text: plain.subarray(0, end).toString("utf8") });
    out.message = JSON.parse(out.text);
  } catch (e) { out.error = String(e); }
  try {
    const a = headers.authorization || "";
    const t = /t=([^,\s]+)/.exec(a)[1], k = /k=([^,\s]+)/.exec(a)[1];
    const [h, c, sig] = t.split(".");
    const kb = Buffer.from(k, "base64url");
    const pub = crypto.createPublicKey({ key: { kty: "EC", crv: "P-256", x: b64u(kb.subarray(1, 33)), y: b64u(kb.subarray(33, 65)) }, format: "jwk" });
    const claims = JSON.parse(Buffer.from(c, "base64url"));
    out.vapid = { scheme: a.split(" ")[0], header: JSON.parse(Buffer.from(h, "base64url")), aud: claims.aud, sub: claims.sub, exp_in_h: (claims.exp - Date.now() / 1000) / 3600, k,
      signature_ok: crypto.verify("sha256", Buffer.from(`${h}.${c}`), { key: pub, dsaEncoding: "ieee-p1363" }, Buffer.from(sig, "base64url")) };
  } catch (e) { out.vapid_error = String(e); }
  return out;
}
http.createServer((req, res) => {
  const parts = [];
  req.on("data", (d) => parts.push(d));
  req.on("end", () => {
    const u = new URL(req.url, "http://x");
    const reply = (status, j) => {
      const text = JSON.stringify(j);
      res.writeHead(status, { "content-type": "application/json", "content-length": Buffer.byteLength(text), "access-control-allow-origin": "*" });
      res.end(text);
    };
    if (u.pathname === "/_made") {
      const k = +(u.searchParams.get("n") || 1), kind = u.searchParams.get("kind") || "ok", out = [];
      for (let i = 0; i < k; i++) {
        const ecdh = crypto.createECDH("prime256v1");
        ecdh.generateKeys();
        const auth = crypto.randomBytes(16), path = `/push/${kind}-${++n}`;
        subs.set(path, { ecdh, auth });
        out.push({ endpoint: `http://127.0.0.1:${PORT}${path}`, expirationTime: null, keys: { p256dh: b64u(ecdh.getPublicKey()), auth: b64u(auth) } });
      }
      return reply(200, out);
    }
    if (u.pathname === "/_got") return reply(200, got);
    if (u.pathname === "/_reset") { got = []; return reply(200, {}); }
    const kind = u.pathname.split("/")[2] || "";
    // A push service that never answers, one that redirects, and one whose answer would write
    // lines of its own in a log (the review's).
    if (kind.startsWith("slow")) { got.push({ path: u.pathname, slow: true, sock: req.socket.remotePort }); return; }
    if (kind.startsWith("redir")) { got.push({ path: u.pathname, redir: true }); res.writeHead(307, { location: "http://127.0.0.1:" + PORT + "/push/ok-redirected" }); return res.end(); }
    if (kind.startsWith("noisy")) { got.push({ path: u.pathname }); res.writeHead(500); return res.end("line1\n\u001b[31mFAKE quillo: injected log line\u001b[0m"); }
    const status = kind.startsWith("404") ? 404 : kind.startsWith("410") ? 410 : kind.startsWith("500") ? 500 : 201;
    got.push({ ...open(u.pathname, req.headers, Buffer.concat(parts)), status, method: req.method, sock: req.socket.remotePort });
    res.writeHead(status);
    res.end();
  });
}).listen(PORT, "127.0.0.1", function () { PORT = this.address().port; console.log(PORT); });
"#;

/// The sink, running: killed with the test.
struct Sink {
    _node: support::Guard,
    port: u16,
}

fn sink() -> Sink {
    // A folder of its own: tests run at once, and one folder for every sink was removed by one while
    // another wrote into it (EINVAL, found in slice 7's runs; a shown bug in the test, branch
    // security).
    static SINKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = support::temp_dir(&format!("f67-sink-{}", SINKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::write(d.join("sink.cjs"), SINK).unwrap();
    // It listens on port 0 and prints the port it was given: one picked here was taken before it
    // bound it, on CI (run 44152090200; a shown bug in the test, branch security).
    let child = std::process::Command::new("node")
        .arg(d.join("sink.cjs"))
        .arg("0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("node");
    let mut node = support::Guard::new(child);
    let port = support::port_on_stdout(&mut node.0, "the sink", Duration::from_secs(20));
    Sink { _node: node, port }
}

impl Sink {
    fn get(&self, path: &str) -> serde_json::Value {
        serde_json::from_str(&support::send(self.port, "GET", path, &[], "").body).unwrap()
    }

    /// `n` subscriptions whose push service answers as `kind` says.
    fn made(&self, n: usize, kind: &str) -> Vec<serde_json::Value> {
        self.get(&format!("/_made?n={}&kind={}", n, kind)).as_array().unwrap().clone()
    }

    fn got(&self) -> Vec<serde_json::Value> {
        self.get("/_got").as_array().unwrap().clone()
    }

    fn reset(&self) {
        self.get("/_reset");
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

/// `quillo run` of `d` with `env` after QUILLO_SECRET and QUILLO_PUSH_LOCAL=1 (the sink is on this
/// machine): the server, its log in server.log, and its port.
fn push_server(d: &Path, opts: &[&str], env: &[(&str, &str)]) -> (Logged, u16) {
    let log = d.join("server.log");
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", "0"])
        .args(opts)
        .env("QUILLO_SECRET", "test secret for the tests")
        .env("QUILLO_PUSH_LOCAL", "1")
        .envs(env.iter().copied())
        .current_dir(d)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    // The system picks the port, as `logged` has it (branch security).
    let mut s = Logged(child, log);
    let port = support::port_in_log(&mut s.0, &s.1, Duration::from_secs(20));
    (s, port)
}

/// What a page posts to /_quillo/push, as the runtime makes it: the subscription, `user` (a token
/// of `notification.user`, or null) and the page's origin, from the page's own site.
fn bind(port: u16, sub: &serde_json::Value, user: serde_json::Value, cookie: &str) -> support::Answer {
    let mut body = sub.clone();
    body["user"] = user;
    let (host, origin) = (format!("127.0.0.1:{}", port), format!("http://127.0.0.1:{}", port));
    let mut hs = vec![("host", host.as_str()), ("origin", origin.as_str())];
    if !cookie.is_empty() {
        hs.push(("cookie", cookie));
    }
    support::send(port, "POST", "/_quillo/push", &hs, &body.to_string())
}

/// `POST /send/<who>` with the message: what `notification.send` gave, as JSON.
fn send_to(port: u16, who: &str, msg: serde_json::Value) -> serde_json::Value {
    let a = support::send(port, "POST", &format!("/send/{}", who), &[], &msg.to_string());
    assert_eq!(a.status, 200, "{}", a.body);
    serde_json::from_str(&a.body).unwrap_or_else(|_| panic!("{}", a.body))
}

/// The server's public key, as a page subscribes with it.
fn push_key(port: u16) -> String {
    let a = support::send(port, "GET", "/_quillo/push", &[], "");
    assert_eq!((a.status, a.header("cache-control")), (200, Some("no-store")), "{}", a.body);
    serde_json::from_str::<serde_json::Value>(&a.body).unwrap()["key"].as_str().unwrap().to_string()
}

/// A program that signs people in and sends to them.
const PUSHER: &str = r#"route GET /in/:who {
    set_session({ user: who })
    redirect("/")
}
route GET /in-int/:n(int) {
    set_session({ user: n })
    redirect("/")
}
route GET /cart {
    set_session({ cart: 3 })
    redirect("/")
}
route GET /out {
    end_session()
    redirect("/")
}
route GET /ticket/:who {
    let t = notification.user(who)
    return is_err(t) ? { error: t.error } : t
}
route GET /signin-ticket/:who {
    set_session({ user: who })
    return notification.user(who)
}
route POST /send/:who {
    let r = notification.send(who, request.json)
    is_err(r) ? { error: r.error } : r
}
route POST /send-int/:n(int) = notification.send(n, request.json)
route POST /many/:who/:n(int) {
    let sent = 0
    for i in 0..n {
        sent += notification.send(who, { title: `n${i}` }).sent
    }
    { sent: sent }
}
route GET / = "home"
"#;

// B.3: what `notification.send` posts decrypts exactly, by Node's own RFC 8291 (unicode and emoji
// included): one aes128gcm record of 4096, the key's 65 bytes, the delimiter 2, Declarative Web
// Push's JSON with `navigate` from the subscription's origin; TTL and Urgency as given, else a day
// and normal. Its VAPID header verifies by RFC 8292: ES256 by `k`, which is the key a page gets,
// `aud` the push service's origin, `sub` the site's, 12 hours ahead. Under --base, `navigate`
// carries the prefix.
#[test]
fn notification_a_message_decrypts_exactly_and_its_vapid_verifies() {
    let sink = sink();
    let d = dir("n-exact", PUSHER);
    assert!(check(&d).0, "{:?}", check(&d));
    let (_s, port) = push_server(&d, &[], &[]);
    let (ana, _) = session_step(port, "/in/ana", "");
    let subs = sink.made(1, "ok");
    assert_eq!(bind(port, &subs[0], serde_json::Value::Null, &ana).status, 200);
    let msg = serde_json::json!({ "title": "Grüße ✓", "body": "Order 1042 has shipped: 日本 🎉", "url": "/orders/1042", "tag": "o1042", "badge": 3, "silent": true, "ttl": "1h", "urgency": "high" });
    assert_eq!(send_to(port, "ana", msg), serde_json::json!({ "sent": 1, "gone": 0 }));
    let g = &sink.got()[0];
    let want = format!(r#"{{"web_push":8030,"notification":{{"title":"Grüße ✓","body":"Order 1042 has shipped: 日本 🎉","navigate":"http://127.0.0.1:{}/orders/1042","tag":"o1042","silent":true,"app_badge":3}}}}"#, port);
    assert_eq!(g["text"], want.as_str(), "{}", g);
    assert_eq!((g["delimiter"].as_u64(), g["record_size"].as_u64(), g["key_length"].as_u64()), (Some(2), Some(4096), Some(65)));
    assert_eq!((g["content_encoding"].as_str(), g["content_type"].as_str(), g["ttl"].as_str(), g["urgency"].as_str()), (Some("aes128gcm"), Some("application/octet-stream"), Some("3600"), Some("high")));
    assert_eq!(g["length"].as_u64(), Some(86 + want.len() as u64 + 1 + 16));
    let v = &g["vapid"];
    assert_eq!((v["scheme"].as_str(), &v["header"], v["signature_ok"].as_bool()), (Some("vapid"), &serde_json::json!({ "typ": "JWT", "alg": "ES256" }), Some(true)), "{}", g);
    assert_eq!((v["aud"].as_str().unwrap(), v["sub"].as_str().unwrap()), (sink.origin().as_str(), format!("http://127.0.0.1:{}", port).as_str()));
    let h = v["exp_in_h"].as_f64().unwrap();
    assert!(h > 11.9 && h <= 12.0, "{}", h);
    assert_eq!(v["k"].as_str().unwrap(), push_key(port));
    // The least: a title alone.
    sink.reset();
    assert_eq!(send_to(port, "ana", serde_json::json!({ "title": "t" })), serde_json::json!({ "sent": 1, "gone": 0 }));
    let g = &sink.got()[0];
    assert_eq!(g["message"], serde_json::json!({ "web_push": 8030, "notification": { "title": "t", "navigate": format!("http://127.0.0.1:{}/", port) } }));
    assert_eq!((g["ttl"].as_str(), g["urgency"].as_str()), (Some("86400"), Some("normal")));
    // Under --base, as a proxy hands requests on with the prefix taken off.
    let d = dir("n-exact-base", PUSHER);
    let (_s, port) = push_server(&d, &["--base", "/shop"], &[]);
    let (ana, _) = session_step(port, "/in/ana", "");
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::Value::Null, &ana).status, 200);
    sink.reset();
    send_to(port, "ana", serde_json::json!({ "title": "t", "url": "/orders/7" }));
    send_to(port, "ana", serde_json::json!({ "title": "t" }));
    let navs: Vec<String> = sink.got().iter().map(|g| g["message"]["notification"]["navigate"].as_str().unwrap().to_string()).collect();
    assert_eq!(navs, [format!("http://127.0.0.1:{}/shop/orders/7", port), format!("http://127.0.0.1:{}/shop/", port)]);
}

// Q-956: every instance signs with one key (`--instances 2`, 40 asks), the same after a restart
// with the same QUILLO_SECRET, another with another: the key is derived from the session key.
// It is a P-256 public key as a page subscribes with it: 65 bytes, uncompressed.
#[test]
fn notification_instances_and_restarts_sign_with_one_key() {
    let d = dir("n-key", PUSHER);
    let (s, port) = push_server(&d, &["--instances", "2"], &[]);
    let keys: std::collections::HashSet<String> = (0..40).map(|_| push_key(port)).collect();
    assert_eq!(keys.len(), 1, "{:?}", keys);
    let key = keys.into_iter().next().unwrap();
    assert_eq!(key.len(), 87);
    assert!(key.starts_with('B'), "{}", key);
    drop(s);
    let (s, port) = push_server(&d, &[], &[]);
    assert_eq!(push_key(port), key);
    drop(s);
    let (_s, port) = push_server(&d, &[], &[("QUILLO_SECRET", "another secret, another key, as sessions")]);
    assert_ne!(push_key(port), key);
}

// B.3: what a push service says has ended (404, 410) is dropped and counted as `gone`; one that
// failed otherwise (500) is kept, not counted, and said in the log; the next send reaches the rest.
// Who is the user as JSON: the int 7 is not the text "7". No subscription: nothing sent, none gone.
#[test]
fn notification_ended_subscriptions_are_dropped_and_counted() {
    let sink = sink();
    let d = dir("n-gone", PUSHER);
    let (s, port) = push_server(&d, &[], &[]);
    let (ana, _) = session_step(port, "/in/ana", "");
    for kind in ["ok", "ok", "404", "410", "500"] {
        assert_eq!(bind(port, &sink.made(1, kind)[0], serde_json::Value::Null, &ana).status, 200);
    }
    assert_eq!(send_to(port, "ana", serde_json::json!({ "title": "one" })), serde_json::json!({ "sent": 2, "gone": 2 }));
    assert_eq!(sink.got().len(), 5);
    sink.reset();
    assert_eq!(send_to(port, "ana", serde_json::json!({ "title": "two" })), serde_json::json!({ "sent": 2, "gone": 0 }));
    // Sent at once (review): in any order.
    let mut paths: Vec<String> = sink.got().iter().map(|g| g["path"].as_str().unwrap().split('-').next().unwrap().to_string()).collect();
    paths.sort();
    assert_eq!(paths, ["/push/500", "/push/ok", "/push/ok"]);
    assert!(log_with(&s, "the push service answered 500").contains("quillo: notification.send: 127.0.0.1: the push service answered 500"));
    assert_eq!(send_to(port, "bob", serde_json::json!({ "title": "none" })), serde_json::json!({ "sent": 0, "gone": 0 }));
    let (seven, _) = session_step(port, "/in-int/7", "");
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::Value::Null, &seven).status, 200);
    assert_eq!(send_to(port, "7", serde_json::json!({ "title": "text 7" })), serde_json::json!({ "sent": 0, "gone": 0 }));
    let a = support::send(port, "POST", "/send-int/7", &[], r#"{"title":"int 7"}"#);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&a.body).unwrap(), serde_json::json!({ "sent": 1, "gone": 0 }));
}

/// A message of exactly `size` bytes as sent from `origin` (a title, and a body of é's and x's).
fn sized(origin: &str, size: usize) -> serde_json::Value {
    let fixed = format!(r#"{{"web_push":8030,"notification":{{"title":"t","body":"","navigate":"{}/"}}}}"#, origin).len();
    let n = size - fixed;
    serde_json::json!({ "title": "t", "body": format!("{}{}", "é".repeat(n / 2), "x".repeat(n % 2)) })
}

// B.3: one push message holds 3,800 bytes as sent (one record of 4096): 3,799 and 3,800 go and
// decrypt whole, 3,801 is an error value and sends nothing, with a subscription or none. A message
// without a title, with an option that is none, a url that is not a path here, a bad badge, ttl
// or urgency, is a mistake that stops the route, as `http`'s unknown options do.
#[test]
fn notification_a_message_past_one_record_is_an_error_value() {
    let sink = sink();
    let d = dir("n-size", PUSHER);
    let (s, port) = push_server(&d, &[], &[]);
    let origin = format!("http://127.0.0.1:{}", port);
    let none = sized("", 3801);
    assert_eq!(send_to(port, "ana", none)["error"].as_str(), Some("notification.send: the message is 3801 bytes as sent, more than the 3800 one push message holds: shorten its body"));
    let (ana, _) = session_step(port, "/in/ana", "");
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::Value::Null, &ana).status, 200);
    for size in [3799, 3800] {
        assert_eq!(send_to(port, "ana", sized(&origin, size)), serde_json::json!({ "sent": 1, "gone": 0 }), "{}", size);
    }
    assert_eq!(send_to(port, "ana", sized(&origin, 3801))["error"].as_str(), Some("notification.send: the message is 3801 bytes as sent, more than the 3800 one push message holds: shorten its body"));
    let got: Vec<usize> = sink.got().iter().map(|g| g["text"].as_str().unwrap().len()).collect();
    assert_eq!(got, [3799, 3800]);
    assert!(sink.got().iter().all(|g| g["length"].as_u64().unwrap() <= 4096 && g["delimiter"] == 2));
    for (msg, says) in [
        (r#"{"body":"no title"}"#, "`notification.send` needs a `title`"),
        (r#"{"title":""}"#, "`notification.send` needs a `title`"),
        (r#"{"title":"t","icon":"x.png"}"#, "unknown option `icon` (the options are title, body, url, tag, badge, silent, ttl, urgency)"),
        (r#"{"title":"t","url":"https://elsewhere.example/x"}"#, "`url` is a path on this site"),
        (r#"{"title":"t","url":"//elsewhere.example/x"}"#, "`url` is a path on this site"),
        (r#"{"title":"t","badge":-1}"#, "`badge` is a whole number, 0 or more"),
        (r#"{"title":"t","ttl":2419201}"#, "at most 4 weeks"),
        (r#"{"title":"t","urgency":"now"}"#, "`urgency` is \"very-low\", \"low\", \"normal\" or \"high\""),
        (r#"{"title":5}"#, "`notification.send`'s `title` is text, got int"),
    ] {
        let a = support::send(port, "POST", "/send/ana", &[], msg);
        assert_eq!(a.status, 500, "{}", msg);
        assert!(log_with(&s, says).contains(says), "{}: {}", msg, says);
    }
    // The longest TTL is taken.
    sink.reset();
    assert_eq!(send_to(port, "ana", serde_json::json!({ "title": "t", "ttl": 2419200, "urgency": "very-low" })), serde_json::json!({ "sent": 1, "gone": 0 }));
    assert_eq!((sink.got()[0]["ttl"].as_str(), sink.got()[0]["urgency"].as_str()), (Some("2419200"), Some("very-low")));
}

// Q-960: sends to one push service go over kept connections: 50 in one request, one each, all on
// one; a send to 20 subscriptions at once over at most 8 (review: a person's devices are sent to
// at once, 8 at a time), and the next send over the same ones.
#[test]
fn notification_sends_reuse_one_connection() {
    let sink = sink();
    let d = dir("n-conn", PUSHER);
    let (_s, port) = push_server(&d, &[], &[]);
    let (ana, _) = session_step(port, "/in/ana", "");
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::Value::Null, &ana).status, 200);
    let a = support::send(port, "POST", "/many/ana/50", &[], "");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&a.body).unwrap(), serde_json::json!({ "sent": 50 }), "{}", a.body);
    let got = sink.got();
    assert_eq!(got.len(), 50);
    let socks: std::collections::HashSet<u64> = got.iter().map(|g| g["sock"].as_u64().unwrap()).collect();
    assert_eq!(socks.len(), 1, "{:?}", socks);
    let (bob, _) = session_step(port, "/in/bob", "");
    for sub in sink.made(20, "ok") {
        assert_eq!(bind(port, &sub, serde_json::Value::Null, &bob).status, 200);
    }
    sink.reset();
    assert_eq!(send_to(port, "bob", serde_json::json!({ "title": "all" })), serde_json::json!({ "sent": 20, "gone": 0 }));
    let socks: std::collections::HashSet<u64> = sink.got().iter().map(|g| g["sock"].as_u64().unwrap()).collect();
    assert!(!socks.is_empty() && socks.len() <= 8, "{:?}", socks);
    sink.reset();
    assert_eq!(send_to(port, "bob", serde_json::json!({ "title": "again" })), serde_json::json!({ "sent": 20, "gone": 0 }));
    let again: std::collections::HashSet<u64> = sink.got().iter().map(|g| g["sock"].as_u64().unwrap()).collect();
    assert!(again.is_subset(&socks), "{:?} then {:?}", socks, again);
}

// B.3 review, 3 and 5: a person has 20 devices here at most, a new one replacing the oldest; a
// session changes 30 subscriptions a minute at most (the 31st is a 429). A send reaches the
// devices at once: two push services that never answer take 10 s each, together, not one after
// the other; a redirect is no delivery, and is not followed. What a push service answered is one
// line in the log, its control characters escaped, and the log names its host, never the endpoint.
#[test]
fn notification_devices_are_capped_and_sent_to_at_once() {
    let sink = sink();
    let d = dir("n-cap", PUSHER);
    let (s, port) = push_server(&d, &[], &[]);
    let (hal, _) = session_step(port, "/in/hal", "");
    let subs = sink.made(25, "ok");
    for sub in &subs {
        assert_eq!(bind(port, sub, serde_json::Value::Null, &hal).status, 200);
    }
    assert_eq!(send_to(port, "hal", serde_json::json!({ "title": "t" })), serde_json::json!({ "sent": 20, "gone": 0 }));
    let mut reached: Vec<String> = sink.got().iter().map(|g| g["path"].as_str().unwrap().to_string()).collect();
    let mut newest: Vec<String> = subs[5..].iter().map(|s| url_path(s["endpoint"].as_str().unwrap())).collect();
    reached.sort();
    newest.sort();
    assert_eq!(reached, newest, "the 5 oldest replaced");
    // 25 so far, then 5 more: the 31st in the minute is refused.
    for sub in sink.made(5, "ok") {
        assert_eq!(bind(port, &sub, serde_json::Value::Null, &hal).status, 200);
    }
    let a = bind(port, &sink.made(1, "ok")[0], serde_json::Value::Null, &hal);
    assert_eq!(a.status, 429, "{}", a.body);
    assert!(a.body.contains("more than 30 subscriptions changed in a minute by this session"), "{}", a.body);
    // Another session is counted apart.
    let (ivy, _) = session_step(port, "/in/ivy", "");
    for kind in ["slow", "slow", "redir", "noisy"] {
        assert_eq!(bind(port, &sink.made(1, kind)[0], serde_json::Value::Null, &ivy).status, 200);
    }
    let t0 = Instant::now();
    assert_eq!(send_to(port, "ivy", serde_json::json!({ "title": "t" })), serde_json::json!({ "sent": 0, "gone": 0 }));
    let took = t0.elapsed();
    assert!(took >= Duration::from_secs(9) && took < Duration::from_secs(16), "{:?}", took);
    assert!(!sink.got().iter().any(|g| g["path"] == "/push/ok-redirected"), "the redirect was followed");
    let log = log_with(&s, "took more than 10 s");
    let lines: Vec<&str> = log.lines().filter(|l| l.starts_with("quillo: notification.send:")).collect();
    assert_eq!(lines.len(), 4, "{}", log);
    assert!(lines.iter().filter(|l| l.contains("took more than 10 s")).count() == 2, "{:?}", lines);
    assert!(lines.iter().any(|l| l.ends_with("127.0.0.1: the push service answered 307")), "{:?}", lines);
    assert!(lines.iter().any(|l| l.ends_with("127.0.0.1: the push service answered 500: line1\\n\\u{1b}[31mFAKE quillo: injected log line\\u{1b}[0m")), "{:?}", lines);
    assert!(!log.contains('\u{1b}') && !log.lines().any(|l| l.starts_with("FAKE")), "{}", log);
    assert!(!log.contains("/push/"), "an endpoint in the log: {}", log);
}

/// The path of a URL.
fn url_path(u: &str) -> String {
    let rest = u.split_once("://").map_or(u, |(_, r)| r);
    rest.find('/').map_or("/".to_string(), |i| rest[i..].to_string())
}

// B.3 review, 3: `notification.send` inside a transaction is an error value saying to send after
// it (a SQLite transaction keeps the interpreter's lock, and every request would wait on the push
// services), sending nothing; `quillo check` warns where it sees it, in `db.transaction` and
// `db.atomic`, not after one.
#[test]
fn notification_send_in_a_transaction_is_refused() {
    let src = "route GET /in/:who {\n    set_session({ user: who })\n    redirect(\"/\")\n}\nroute POST /send-tx/:who {\n    db.exec(\"create table if not exists t (x int)\")\n    let r = db.transaction(() => {\n        db.exec(\"insert into t values (1)\")\n        return notification.send(who, { title: \"in a transaction\" })\n    })\n    return is_err(r) ? { error: r.error } : r\n}\nroute POST /send-after/:who {\n    db.transaction(() => db.exec(\"insert into t values (2)\"))?\n    return notification.send(who, { title: \"after\" })\n}\nroute GET / = \"home\"\n";
    let d = dir("n-tx", src);
    std::fs::write(d.join(".env"), "QUILLO_DB=app.db\n").unwrap();
    let (ok, lines) = check_lines("n-tx-check", src);
    assert!(ok, "{:?}", lines);
    assert_eq!(lines, [format!("main.qwe:9: warning: {}", IN_A_TRANSACTION)]);
    let sink = sink();
    let (_s, port) = push_server(&d, &[], &[]);
    let (who, _) = session_step(port, "/in/tx", "");
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::Value::Null, &who).status, 200);
    let a = support::send(port, "POST", "/send-tx/tx", &[], "");
    assert!(a.body.contains("`notification.send` waits on push services, which inside a transaction"), "{} {}", a.status, a.body);
    assert!(sink.got().is_empty());
    let a = support::send(port, "POST", "/send-after/tx", &[], "");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&a.body).unwrap(), serde_json::json!({ "sent": 1, "gone": 0 }), "{}", a.body);
}

const IN_A_TRANSACTION: &str = "`notification.send` waits on push services, which inside a transaction holds up every other request (a SQLite transaction keeps the interpreter's lock), and sends even when the transaction is rolled back: call it after the transaction";

// B.3 review, 6 and the database's errors: the origin a send's address and VAPID's `sub` are made
// from is the request's (its Host, https where a proxy says the request came so), never the body's;
// a database that cannot be had at /_quillo/push is "internal error" to the visitor (its words in
// the log), in full under --watch.
#[test]
fn notification_the_origin_is_the_requests_and_errors_are_hidden() {
    let sink = sink();
    let d = dir("n-origin", PUSHER);
    // The proxy named: since Q-986 only a `--trust-proxy` proxy's X-Forwarded-Proto makes the origin
    // https (FEEDBACK70.md, Q-986; branch security: the decided change).
    let (_s, port) = push_server(&d, &["--trust-proxy", "127.0.0.1"], &[]);
    let (ivan, _) = session_step(port, "/in/ivan", "");
    let host = format!("127.0.0.1:{}", port);
    let mut body = sink.made(1, "ok").remove(0);
    body["user"] = serde_json::Value::Null;
    body["origin"] = serde_json::json!("https://evil.example");
    let a = support::send(port, "POST", "/_quillo/push", &[("host", &host), ("cookie", &ivan)], &body.to_string());
    assert_eq!(a.status, 200, "{}", a.body);
    let mut over = sink.made(1, "ok").remove(0);
    over["user"] = serde_json::Value::Null;
    let a = support::send(port, "POST", "/_quillo/push", &[("host", "shop.example:8443"), ("x-forwarded-proto", "https"), ("cookie", &ivan)], &over.to_string());
    assert_eq!(a.status, 200, "{}", a.body);
    send_to(port, "ivan", serde_json::json!({ "title": "t", "url": "/o/1" }));
    let mut got: Vec<(String, String)> = sink.got().iter().map(|g| (g["message"]["notification"]["navigate"].as_str().unwrap().to_string(), g["vapid"]["sub"].as_str().unwrap().to_string())).collect();
    got.sort();
    assert_eq!(got, [(format!("http://127.0.0.1:{}/o/1", port), format!("http://127.0.0.1:{}", port)), ("https://shop.example:8443/o/1".to_string(), "https://shop.example:8443".to_string())]);
    // The system database cannot be opened: a folder where its file would be.
    for (opts, says) in [(&[][..], "\"internal error\""), (&["--watch"][..], "the system database")] {
        let d = dir(&format!("n-dberror-{}", opts.len()), PUSHER);
        std::fs::create_dir(d.join("quillo-system.db")).unwrap();
        let (s, port) = push_server(&d, opts, &[]);
        let (who, _) = session_step(port, "/in/una", "");
        let a = bind(port, &sink.made(1, "ok")[0], serde_json::Value::Null, &who);
        assert_eq!(a.status, 500, "{}", a.body);
        assert!(a.body.contains(says), "{:?}: {}", opts, a.body);
        assert!(log_with(&s, "/_quillo/push:").contains("quillo: /_quillo/push: the system database"), "{:?}", opts);
    }
}

// B.3: `end_session()` unbinds the session's subscriptions, and so does a change of person in
// `set_session` (Bob signing in where Ana was): the next person on the device gets nothing meant
// for the one before. `user:` takes only what `notification.user(x)` made in the session the post
// comes from (review: one made at sign-in, in the request that set the session, is that session's;
// none is made where there is no session; another session's, or none, is refused), and it ends
// with that session. A plain value, a token changed, or neither a token nor a signed-in visitor is
// refused, saying why. DELETE unbinds the visitor's own alone.
#[test]
fn notification_sessions_end_and_the_user_option() {
    let sink = sink();
    let d = dir("n-sessions", PUSHER);
    let (_s, port) = push_server(&d, &[], &[]);
    let one = serde_json::json!({ "title": "t" });
    let (ana, _) = session_step(port, "/in/ana", "");
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::Value::Null, &ana).status, 200);
    assert_eq!(send_to(port, "ana", one.clone())["sent"], 1);
    session_step(port, "/out", &ana);
    assert_eq!(send_to(port, "ana", one.clone())["sent"], 0);
    // Ana again, then Bob on her browser without signing her out.
    let (ana, _) = session_step(port, "/in/ana", "");
    let (ana_phone, _) = session_step(port, "/in/ana", "");
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::Value::Null, &ana).status, 200);
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::Value::Null, &ana_phone).status, 200);
    assert_eq!(send_to(port, "ana", one.clone())["sent"], 2);
    let (bob, _) = session_step(port, "/in/bob", &ana);
    assert!(bob != ana);
    assert_eq!(send_to(port, "ana", one.clone())["sent"], 1, "her phone's stays");
    // No ticket where there is no session.
    let a = support::send(port, "GET", "/ticket/guest-1", &[], "");
    assert!(a.body.contains("notification.user: this request has no session to seal it for"), "{}", a.body);
    // Made at sign-in, in the request that set the session: that session's alone.
    let a = support::send(port, "GET", "/signin-ticket/erin", &[], "");
    let erin = a.headers.iter().find(|(k, v)| k == "set-cookie" && v.contains("quillo_session=")).map(|(_, v)| v.split(';').next().unwrap().to_string()).unwrap();
    let ticket = a.body.clone();
    let (mallory, _) = session_step(port, "/in/mallory", "");
    let sub = sink.made(1, "ok").remove(0);
    for cookie in [mallory.as_str(), ""] {
        let a = bind(port, &sub, serde_json::json!(ticket), cookie);
        assert_eq!(a.status, 403, "{}", a.body);
    }
    assert_eq!(bind(port, &sub, serde_json::json!(ticket), &erin).status, 200);
    assert_eq!(send_to(port, "erin", one.clone())["sent"], 1);
    // Made in a session (a cart, no user): only from it, and unbound when it ends.
    let (cart, _) = session_step(port, "/cart", "");
    let zoe = support::send(port, "GET", "/ticket/zoe", &[("cookie", &cart)], "").body;
    let sub = sink.made(1, "ok").remove(0);
    let a = bind(port, &sub, serde_json::json!(zoe), &mallory);
    assert_eq!(a.status, 403);
    assert!(a.body.contains("not one notification.user made here, for this session"), "{}", a.body);
    assert_eq!(bind(port, &sub, serde_json::json!(zoe), &bob).status, 403);
    assert_eq!(bind(port, &sub, serde_json::json!(zoe), &cart).status, 200);
    assert_eq!(send_to(port, "zoe", one.clone())["sent"], 1);
    session_step(port, "/out", &cart);
    assert_eq!(send_to(port, "zoe", one.clone())["sent"], 0);
    // What is refused, and why.
    let (cart2, _) = session_step(port, "/cart", "");
    let sub = sink.made(1, "ok").remove(0);
    for (user, cookie, says) in [
        (serde_json::json!("zoe"), cart2.as_str(), "not one notification.user made here"),
        (serde_json::json!(format!("{}x", zoe)), cart2.as_str(), "not one notification.user made here"),
        (serde_json::json!(7), cart2.as_str(), "made on the server with notification.user(x), so that a page cannot name someone else"),
        (serde_json::json!({ "user": "zoe" }), cart2.as_str(), "made on the server with notification.user(x)"),
        (serde_json::Value::Null, cart2.as_str(), "no one to send to: the visitor is not signed in (session().user)"),
        (serde_json::Value::Null, "", "no one to send to"),
        (serde_json::json!(zoe), "", "no one to send to"),
    ] {
        let a = bind(port, &sub, user.clone(), cookie);
        assert_eq!(a.status, 403, "{} {}", user, a.body);
        assert!(a.body.contains(says), "{}: {}", user, a.body);
    }
    // DELETE, as `stop` sends it: the visitor's own alone.
    let (cy, _) = session_step(port, "/in/cy", "");
    let sub = sink.made(1, "ok").remove(0);
    assert_eq!(bind(port, &sub, serde_json::Value::Null, &cy).status, 200);
    let host = format!("127.0.0.1:{}", port);
    let delete = |cookie: &str| {
        let mut hs = vec![("host", host.as_str())];
        if !cookie.is_empty() {
            hs.push(("cookie", cookie));
        }
        support::send(port, "DELETE", "/_quillo/push", &hs, &serde_json::json!({ "endpoint": sub["endpoint"] }).to_string())
    };
    assert_eq!(delete("").status, 403);
    assert_eq!(delete(&mallory).status, 200);
    assert_eq!(send_to(port, "cy", one.clone())["sent"], 1, "a stranger's delete took nothing");
    let a = delete(&cy);
    assert_eq!((a.status, a.body.as_str()), (200, r#"{"ok":true}"#));
    assert_eq!(send_to(port, "cy", one)["sent"], 0);
}

/// A page's look at its browser's subscription as it starts (`check`), with `cookie`: what the
/// server says it is.
fn check_sub(port: u16, sub: &serde_json::Value, cookie: &str) -> serde_json::Value {
    let host = format!("127.0.0.1:{}", port);
    let mut hs = vec![("host", host.as_str())];
    if !cookie.is_empty() {
        hs.push(("cookie", cookie));
    }
    let a = support::send(port, "POST", "/_quillo/push", &hs, &serde_json::json!({ "endpoint": sub["endpoint"], "check": true }).to_string());
    assert_eq!(a.status, 200, "{}", a.body);
    serde_json::from_str(&a.body).unwrap()
}

// B.3 review, 1: a device is one person's. (a) Any change of person in `set_session` unbinds the
// session's subscriptions, to no one and from no one too: Alice's, then a cart set on her session,
// then Bob signing in on that cookie, kept Alice's coming to Bob. The same person signing in again
// keeps them. (b) A page's look as it starts: a subscription bound to someone else than the
// session names now (Carol's, her cookie gone and Dave signed in fresh), or with no one signed in,
// is unbound, and the page is told "default"; one of the visitor's, or made in this very session,
// stays "granted"; one not bound at all is "default".
#[test]
fn notification_a_device_is_one_persons() {
    let sink = sink();
    let d = dir("n-one-person", PUSHER);
    let (_s, port) = push_server(&d, &[], &[]);
    let one = serde_json::json!({ "title": "t" });
    // (a) Alice, a cart, Bob.
    let (alice, _) = session_step(port, "/in/alice", "");
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::Value::Null, &alice).status, 200);
    let (carted, _) = session_step(port, "/cart", &alice);
    assert_eq!(send_to(port, "alice", one.clone())["sent"], 0, "a person to no one unbinds");
    session_step(port, "/in/bob", &carted);
    assert_eq!(send_to(port, "alice", one.clone())["sent"], 0);
    // From no one to someone: a guest's ticket-bound device is not the person's who signs in.
    let (guest, _) = session_step(port, "/cart", "");
    let t = support::send(port, "GET", "/ticket/guest-9", &[("cookie", &guest)], "").body;
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::json!(t), &guest).status, 200);
    assert_eq!(send_to(port, "guest-9", one.clone())["sent"], 1);
    session_step(port, "/in/gus", &guest);
    assert_eq!(send_to(port, "guest-9", one.clone())["sent"], 0, "no one to a person unbinds");
    // The same person again: kept.
    let (al, _) = session_step(port, "/in/al", "");
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::Value::Null, &al).status, 200);
    let (al2, _) = session_step(port, "/in/al", &al);
    assert_eq!(send_to(port, "al", one.clone())["sent"], 1);
    // (b) Carol's device, her cookie gone, Dave signed in fresh on it.
    let (carol, _) = session_step(port, "/in/carol", "");
    let sub = sink.made(1, "ok").remove(0);
    assert_eq!(bind(port, &sub, serde_json::Value::Null, &carol).status, 200);
    assert_eq!(check_sub(port, &sub, &carol), serde_json::json!({ "state": "granted" }));
    assert_eq!(send_to(port, "carol", one.clone())["sent"], 1, "her own look keeps it");
    let (dave, _) = session_step(port, "/in/dave", "");
    assert_eq!(check_sub(port, &sub, &dave), serde_json::json!({ "state": "default" }));
    assert_eq!(send_to(port, "carol", one.clone())["sent"], 0, "Dave's page took Carol's away");
    // No one signed in: unbound too; one not bound: "default".
    let sub = sink.made(1, "ok").remove(0);
    assert_eq!(bind(port, &sub, serde_json::Value::Null, &al2).status, 200);
    assert_eq!(check_sub(port, &sub, ""), serde_json::json!({ "state": "default" }));
    assert_eq!(send_to(port, "al", one.clone())["sent"], 1, "only the first of Al's");
    assert_eq!(check_sub(port, &sink.made(1, "ok")[0], &al2), serde_json::json!({ "state": "default" }));
    // Made in this very session with a ticket: the session's.
    let (cart, _) = session_step(port, "/cart", "");
    let t = support::send(port, "GET", "/ticket/kit", &[("cookie", &cart)], "").body;
    let sub = sink.made(1, "ok").remove(0);
    assert_eq!(bind(port, &sub, serde_json::json!(t), &cart).status, 200);
    assert_eq!(check_sub(port, &sub, &cart), serde_json::json!({ "state": "granted" }));
    assert_eq!(send_to(port, "kit", one)["sent"], 1);
}

// B.3: /_quillo/push takes only the program's own pages, as a server fn does: another site's post
// or delete is refused (Sec-Fetch-Site, or an Origin of another host), and so is anything but JSON
// of a few hundred bytes. It takes no endpoint a send should not reach: only https on port 443 to
// the public internet, or plain http to this machine with QUILLO_PUSH_LOCAL=1 alone (review: not
// --watch); no private address (local-use NAT64 too), no name of this machine with a final dot,
// no other scheme or port, no user in it, nothing over 1,000 characters; and only a P-256 key and
// a 16-byte secret. The page's origin is the request's, never what the body says (review).
#[test]
fn notification_what_the_push_endpoint_refuses() {
    let d = dir("n-refuse", PUSHER);
    let (_s, port) = push_server(&d, &["--watch"], &[("QUILLO_PUSH_LOCAL", "0")]);
    let (ana, _) = session_step(port, "/in/ana", "");
    let (host, origin) = (format!("127.0.0.1:{}", port), format!("http://127.0.0.1:{}", port));
    let p256dh = "BF5JjJmHYcp-lHVMIDyVSu7Q4fzLCDsfNqASRtVyiv5i8aoIztqkMiDtok0G5oXrmrnwzD1uoXEWEeXDg1_Yzpg";
    let sub = |endpoint: &str| serde_json::json!({ "endpoint": endpoint, "keys": { "p256dh": p256dh, "auth": "swC8VcTjKc8fIaxz_rdm_Q" } });
    let post = |hs: &[(&str, &str)], body: &str| {
        let mut all = vec![("host", host.as_str()), ("cookie", ana.as_str())];
        all.extend_from_slice(hs);
        support::send(port, "POST", "/_quillo/push", &all, body)
    };
    let good = sub("https://push.example.com/send/abc").to_string();
    for (hs, body, status, says) in [
        (vec![("origin", origin.as_str()), ("sec-fetch-site", "cross-site")], good.clone(), 403, "only the program's own pages"),
        (vec![("origin", "http://evil.example")], good.clone(), 403, "only the program's own pages"),
        (vec![("origin", origin.as_str()), ("content-type", "text/plain")], good.clone(), 415, "send JSON"),
        (vec![("origin", origin.as_str())], format!("{{\"endpoint\":\"https://push.example.com/{}\"}}", "x".repeat(9000)), 413, "a few hundred bytes"),
        (vec![("origin", origin.as_str())], "{".to_string(), 400, "not JSON"),
        (vec![("origin", origin.as_str())], "{}".to_string(), 400, "no endpoint"),
    ] {
        let a = post(&hs, &body);
        assert_eq!(a.status, status, "{:?} {}", hs, a.body);
        assert!(a.body.contains(says), "{:?}: {}", hs, a.body);
    }
    // An origin in the body is not read: the request's Host is the page's.
    let mut with = sub("https://push.example.com/send/abc");
    with["origin"] = serde_json::json!("https://evil.example");
    assert_eq!(post(&[], &with.to_string()).status, 200);
    let long = format!("https://push.example.com/{}", "a".repeat(1000));
    // Another session for the rest: a session changes 30 subscriptions a minute at most.
    let (ana, _) = session_step(port, "/in/ana", "");
    let post = |hs: &[(&str, &str)], body: &str| {
        let mut all = vec![("host", host.as_str()), ("cookie", ana.as_str())];
        all.extend_from_slice(hs);
        support::send(port, "POST", "/_quillo/push", &all, body)
    };
    for (endpoint, says) in [
        ("https://10.0.0.1/push", "not https to an address on the public internet"),
        ("https://169.254.169.254/latest", "not https to an address on the public internet"),
        ("https://[::1]/push", "not https to an address on the public internet"),
        ("https://localhost/push", "not https to an address on the public internet"),
        ("https://a.localhost/push", "not https to an address on the public internet"),
        ("http://push.example.com/push", "not https to an address on the public internet"),
        ("http://127.0.0.1:9/push", "taken only with QUILLO_PUSH_LOCAL=1 (a test's push service)"),
        ("http://localhost:9/push", "taken only with QUILLO_PUSH_LOCAL=1 (a test's push service)"),
        ("https://localhost./push", "not https to an address on the public internet"),
        ("https://[64:ff9b:1::a00:1]/push", "not https to an address on the public internet"),
        ("https://[fd00::1]/push", "not https to an address on the public internet"),
        ("https://push.example.com:22/push", "https on another port than 443"),
        ("https://push.example.com:8443/push", "https on another port than 443"),
        ("ftp://push.example.com/push", "the endpoint is ftp:, not https"),
        ("https://me:pw@push.example.com/push", "holds a user"),
        ("https://push.example.com/push#x", "holds a user or a fragment"),
        ("https://push.example.com/pü", "or not ASCII"),
        (long.as_str(), "longer than 1000 characters"),
        ("push.example.com", "not a URL"),
    ] {
        let a = post(&[("origin", origin.as_str())], &sub(endpoint).to_string());
        assert_eq!(a.status, 400, "{} {}", endpoint, a.body);
        assert!(a.body.contains(says), "{}: {}", endpoint, a.body);
    }
    let (ana, _) = session_step(port, "/in/ana", "");
    let post = |hs: &[(&str, &str)], body: &str| {
        let mut all = vec![("host", host.as_str()), ("cookie", ana.as_str())];
        all.extend_from_slice(hs);
        support::send(port, "POST", "/_quillo/push", &all, body)
    };
    // Port 443 written out is the same as none.
    assert_eq!(post(&[("origin", origin.as_str())], &sub("https://push.example.com:443/send/x").to_string()).status, 200);
    for (keys, says) in [
        (serde_json::json!({ "auth": "swC8VcTjKc8fIaxz_rdm_Q" }), "no keys.p256dh"),
        (serde_json::json!({ "p256dh": "BF5J", "auth": "swC8VcTjKc8fIaxz_rdm_Q" }), "keys.p256dh is not 65 bytes"),
        (serde_json::json!({ "p256dh": format!("B{}", "A".repeat(86)), "auth": "swC8VcTjKc8fIaxz_rdm_Q" }), "keys.p256dh is not a P-256 public key"),
        (serde_json::json!({ "p256dh": p256dh, "auth": "swC8VcTjKc8fIaxz_rdm" }), "keys.auth is not 16 bytes"),
        (serde_json::json!({ "p256dh": p256dh, "auth": "not base64url!" }), "keys.auth is not 16 bytes"),
    ] {
        let a = post(&[("origin", origin.as_str())], &serde_json::json!({ "endpoint": "https://push.example.com/x", "keys": keys }).to_string());
        assert_eq!(a.status, 400, "{}", a.body);
        assert!(a.body.contains(says), "{}: {}", says, a.body);
    }
    // Padded base64, as some libraries write it, is taken.
    let padded = serde_json::json!({ "endpoint": "https://push.example.com/padded", "keys": { "p256dh": format!("{}=", p256dh), "auth": "swC8VcTjKc8fIaxz_rdm_Q==" } });
    assert_eq!(post(&[("origin", origin.as_str())], &padded.to_string()).status, 200);
    let a = support::send(port, "DELETE", "/_quillo/push", &[("host", &host), ("sec-fetch-site", "cross-site")], r#"{"endpoint":"https://push.example.com/padded"}"#);
    assert_eq!(a.status, 403);
    assert_eq!(support::send(port, "PUT", "/_quillo/push", &[("host", &host)], "{}").status, 405);
}

// B.3: a program that never names `notification` pays nothing: no /_quillo/push, the worker slice
// 3 serves (the one that takes itself away), and no system database made by sign-ins and sign-outs.
// One that does gets Quillo's worker for it: with no offline route (and under --watch, with or
// without one) the one that shows notifications and keeps nothing (no `fetch` handler); with
// offline routes, the offline worker with the notifications part; an offline program without
// `notification` has no notifications part. Reading the key, a send with no subscription and a
// sign-out make no table either.
#[test]
fn notification_a_program_without_it_pays_nothing() {
    let plain = "route GET /in/:who {\n    set_session({ user: who })\n    redirect(\"/\")\n}\nroute GET /out {\n    end_session()\n    redirect(\"/\")\n}\nroute GET / = \"home\"\n";
    let d = dir("n-none", plain);
    let (_s, port) = push_server(&d, &[], &[]);
    assert_eq!(support::send(port, "GET", "/_quillo/push", &[], "").status, 404);
    assert_eq!(support::send(port, "POST", "/_quillo/push", &[], "{}").status, 404);
    let (status, _, worker) = fetch_body(port, "/_quillo/sw.js");
    assert!(status == 200 && worker.contains("registration.unregister()") && !worker.contains("\"push\""), "{}", worker);
    let (ana, _) = session_step(port, "/in/ana", "");
    session_step(port, "/out", &ana);
    assert!(!d.join("quillo-system.db").exists());
    // Uses it, no offline route: the worker that shows notifications.
    let d = dir("n-worker", PUSHER);
    let (_s, port) = push_server(&d, &[], &[]);
    let (status, h, worker) = fetch_body(port, "/_quillo/sw.js");
    assert_eq!((status, header(&h, "cache-control"), header(&h, "service-worker-allowed")), (200, Some("no-cache"), Some("/")));
    // It answers no request: its `fetch` listener only looks at the subscription on navigations
    // (second review).
    assert!(worker.contains("addEventListener(\"push\"") && worker.contains("addEventListener(\"notificationclick\"") && !worker.contains("respondWith") && !worker.contains("unregister"), "{}", worker);
    assert!(!worker.lines().any(|l| l.trim_start().starts_with("//")), "comments are not served: {}", worker);
    let etag = header(&h, "etag").unwrap().to_string();
    assert_eq!(fetch(port, "GET", "/_quillo/sw.js", &[("if-none-match", &etag)]).0, 304);
    push_key(port);
    assert_eq!(send_to(port, "ana", serde_json::json!({ "title": "t" })), serde_json::json!({ "sent": 0, "gone": 0 }));
    let (ana, _) = session_step(port, "/in/ana", "");
    session_step(port, "/out", &ana);
    assert!(!d.join("quillo-system.db").exists());
    // Under --watch, the same worker, and the dev key's: no worker that takes itself away.
    let d = dir("n-worker-watch", &format!("{}route offline GET /menu = <html><body>menu</body></html>\n", PUSHER));
    let (_s, port) = push_server(&d, &["--watch"], &[("QUILLO_SECRET", "")]);
    let watched = fetch_body(port, "/_quillo/sw.js").2;
    assert!(watched.contains("addEventListener(\"push\"") && !watched.contains("respondWith") && !watched.contains("unregister"), "{}", watched);
    // With offline routes: both parts; an offline program without `notification`: no push part.
    let d = dir("n-worker-offline", &format!("{}route offline GET /menu = <html><body>menu</body></html>\n", PUSHER));
    let (_s, port) = push_server(&d, &[], &[]);
    let both = fetch_body(port, "/_quillo/sw.js").2;
    assert!(both.contains("addEventListener(\"fetch\"") && both.contains("addEventListener(\"push\"") && both.contains("addEventListener(\"notificationclick\""), "{}", both);
    let d = dir("n-worker-offline-only", "route offline GET /menu = <html><body>menu</body></html>\n");
    let (_s, port) = push_server(&d, &[], &[]);
    let offline = fetch_body(port, "/_quillo/sw.js").2;
    assert!(offline.contains("addEventListener(\"fetch\"") && !offline.contains("\"push\"") && !offline.contains("notificationclick"), "{}", offline);
    assert_eq!(support::send(port, "GET", "/_quillo/push", &[], "").status, 404);
}

// B.3 review, 4: a program uses `notification` only where it calls the module: a variable of that
// name (a loop's, a parameter, a `let`, a route's path parameter, a match arm's, a top-level one)
// turns nothing on: no /_quillo/push, the worker that takes itself away, no warning at start, no
// table. A call of the module where the name is bound only elsewhere (another block) does.
#[test]
fn notification_a_variable_of_its_name_turns_nothing_on() {
    let hiding = [
        ("loop", "let notifications = [{ title: \"Welcome\" }, { title: \"Ship\" }]\nroute GET / {\n    let out = []\n    for notification in notifications {\n        out = out + [notification.title]\n    }\n    return out\n}\n"),
        ("param", "fn tell(notification) = notification.send(\"a\", { title: \"t\" })\nroute GET / = tell({ send: (a, b) => 1 })\n"),
        ("let", "route GET / {\n    let notification = { stop: () => 1 }\n    return notification.stop()\n}\n"),
        ("path", "route GET /n/:notification = notification.state()\n"),
        ("arm", "route GET / = match { state: () => 1 } {\n    notification => notification.state()\n}\n"),
        ("top", "let notification = { send: (a, b) => 1 }\nroute GET / = notification.send(\"a\", { title: \"t\" })\n"),
    ];
    const SAID: &str = "uses `notification` without QUILLO_SECRET";
    for (name, src) in hiding {
        let d = dir(&format!("n-hidden-{}", name), src);
        let (s, port) = push_server(&d, &["--no-check"], &[("QUILLO_SECRET", "")]);
        assert_eq!(support::send(port, "GET", "/_quillo/push", &[], "").status, 404, "{}", name);
        let worker = fetch_body(port, "/_quillo/sw.js").2;
        assert!(worker.contains("registration.unregister()") && !worker.contains("\"push\""), "{}: {}", name, worker);
        support::send(port, "GET", "/", &[], "");
        assert!(!log_with(&s, "listening").contains(SAID), "{}", name);
        assert!(!d.join("quillo-system.db").exists(), "{}", name);
    }
    let used = "fn f() {\n    for notification in [1] {\n        print(notification)\n    }\n    return notification.send(\"a\", { title: \"t\" })\n}\nroute POST /send = f()\n";
    let d = dir("n-hidden-elsewhere", used);
    let (s, port) = push_server(&d, &["--no-check"], &[("QUILLO_SECRET", "")]);
    assert_eq!(support::send(port, "GET", "/_quillo/push", &[], "").status, 200);
    assert!(log_with(&s, SAID).contains(SAID));
    // Any reference to the module turns it on (second review): a pipe's, an alias of a function or
    // of the module, an index, a function that gives it back.
    let forms = [
        ("pipe", "route GET /t = \"u\" | notification.user\n"),
        ("alias-fn", "let tell = notification.send\nroute POST /s = tell(\"a\", { title: \"t\" })\n"),
        ("alias", "let n = notification\nroute POST /s = n.send(\"a\", { title: \"t\" })\n"),
        ("index", "route POST /s = notification[\"send\"](\"a\", { title: \"t\" })\n"),
        ("helper", "fn module() {\n    return notification\n}\nroute POST /s = module().send(\"a\", { title: \"t\" })\n"),
    ];
    for (name, src) in forms {
        let d = dir(&format!("n-used-{}", name), src);
        let (_s, port) = push_server(&d, &["--no-check"], &[]);
        assert_eq!(support::send(port, "GET", "/_quillo/push", &[], "").status, 200, "{}", name);
    }
}

/// `quillo check` of `src`: its error and warning lines.
fn check_lines(name: &str, src: &str) -> (bool, Vec<String>) {
    let d = dir(name, src);
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(["check", "main.qwe"]).current_dir(&d);
    let o = within(c, 30);
    let text = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    (o.status.success(), text.lines().filter(|l| l.contains(": error: ") || l.contains(": warning: ")).map(String::from).collect())
}

const BROWSER_ONLY: &str = "`notification.ask`, `notification.state` and `notification.stop` ask and read the browser: call them from an event handler of a component (`@click`); the server sends with `notification.send(user, { title, body, url })`";

// B.3, `quillo check`: `send` and `user` are the server's, refused in a component (its body or a
// handler); `ask`, `state` and `stop` the browser's, refused where the code surely runs on the
// server (a route, a server fn, a tick, a job, the top level) and in a component's body (`state`
// may set a state); an `ask` after a wait in its handler (a server fn, `http`, `stop`) is warned
// of, not one before it or after `state`; a variable named `notification` hides the module and is
// said. The forms that work pass: ask, state and stop in handlers, state setting a state, send in
// a route, a server fn, a tick and a job, user in a route.
#[test]
fn notification_quillo_check_says_where_each_belongs() {
    let good = r#"server fn save() -> int = 1
server fn ship(who: str) -> map = notification.send(who, { title: "Shipped" })
route POST /send/:who = notification.send(who, { title: "t", body: "b", url: "/x", tag: "t", badge: 1, silent: true, ttl: "1h", urgency: "high" })
route GET /ticket/:who = notification.user(who)
every(3600000, () => notification.send("x", { title: "tick" }))
schedule("0 3 * * *", () => notification.send("x", { title: "job" }))
component Alerts() {
    state on = notification.state()
    <div>
        <button @click={() => { on = notification.ask() }}>ask</button>
        <button @click={() => { on = notification.ask({ user: "t" })
            on = str(save()) }}>ask, then save</button>
        <button @click={() => { if notification.state() != "granted" { on = notification.ask() } }}>state, then ask</button>
        <button @click={() => { on = notification.stop() }}>stop</button>
        <button @mount={() => { on = notification.state() }}>mount</button>
    </div>
}
route GET / = <html><body><Alerts/></body></html>
"#;
    let (ok, lines) = check_lines("n-check-good", good);
    assert!(ok && lines.is_empty(), "{:?}", lines);
    let bad = r#"server fn save() -> int = 1
server fn s() {
    notification.state()
    notification.stop()
}
route GET /a {
    notification.ask()
    "x"
}
every(3600000, () => notification.ask())
schedule("0 3 * * *", () => notification.stop())
component C() {
    state a = notification.stop()
    let b = notification.state()
    let c = notification.ask()
    let t = notification.user("x")
    <div>
        <button @click={() => notification.send("x", { title: "t" })}>{a}{b}{c}{t}</button>
        <button @click={() => { save()
            a = notification.ask() }}>save, then ask</button>
        <button @click={() => { let r = http.get("/x")
            a = notification.ask() }}>get, then ask</button>
        <button @click={() => { notification.stop()
            a = notification.ask() }}>stop, then ask</button>
    </div>
}
route GET / = <html><body><C/></body></html>
"#;
    let (ok, lines) = check_lines("n-check-bad", bad);
    assert!(!ok);
    let on = |line: u32, f: &str, place: &str| format!("main.qwe:{}: error: `notification.{}` {} runs on the server; {}", line, f, place, BROWSER_ONLY);
    let body = |line: u32, f: &str, or: &str| format!("main.qwe:{}: error: `notification.{}` in <C> runs in the browser, and the server renders <C> first: call it from an event handler (@click){}", line, f, or);
    let late = |line: u32, w: &str| format!("main.qwe:{}: warning: `notification.ask` comes after `{}` waits in this handler: browsers show the permission prompt only close to the click, so ask first, then wait", line, w);
    let want = vec![
        on(3, "state", "in a `server fn`"),
        on(4, "stop", "in a `server fn`"),
        on(7, "ask", "in a route"),
        on(10, "ask", "in a tick"),
        on(11, "stop", "in a job"),
        body(13, "stop", ""),
        body(14, "state", ", or set a state with it: state on = notification.state()"),
        body(15, "ask", ""),
        "main.qwe:16: error: `notification.user` is the server's, and <C> also runs in the browser: call it from a `server fn`".to_string(),
        "main.qwe:18: error: `notification.send` is the server's, and <C> also runs in the browser: call it from a `server fn`".to_string(),
        late(20, "save"),
        late(22, "http.get"),
        late(24, "notification.stop"),
    ];
    let mut got: Vec<String> = lines.into_iter().filter(|l| l.contains("notification")).collect();
    got.sort_by_key(|l| l.split(':').nth(1).and_then(|n| n.parse::<u32>().ok()).unwrap_or(0));
    assert_eq!(got, want);
    // A variable that hides the module.
    let (ok, lines) = check_lines("n-check-hidden", "let notification = { send: (a, b) => 1 }\nroute GET / = notification.send(\"a\", { title: \"t\" })\n");
    assert!(!ok && lines.iter().any(|l| l.contains("`notification.send`: `notification` is the program's variable from main.qwe:1, which hides the module")), "{:?}", lines);
    let (ok, lines) = check_lines("n-check-local", "fn f(who: str) {\n    let notification = { title: \"Shipped\" }\n    notification.send(who, notification)\n}\nroute GET / = f(\"a\")\n");
    assert!(!ok && lines.iter().any(|l| l.contains("main.qwe:3: error: `notification.send`: `notification` here is map, a variable, which hides the module `notification`: rename the variable")), "{:?}", lines);
    // A map of that name used as one is no mistake.
    let (ok, lines) = check_lines("n-check-data", "fn f() {\n    let notification = { title: \"Shipped\" }\n    notification.title\n}\nroute GET / = f()\n");
    assert!(ok, "{:?}", lines);
    let (ok, lines) = check_lines("n-check-hidden2", "let notification = 3\nroute GET / = notification.send(\"a\", { title: \"t\" })\n");
    assert!(!ok);
    assert!(lines.iter().any(|l| l.contains("`notification` is the program's variable from main.qwe:1, which hides the module `notification` in every file: rename the variable")), "{:?}", lines);
    // Options written in the call: those there are, and a message with its title.
    let (ok, lines) = check_lines("n-check-options", "server fn ship(who: str) -> map = notification.send(who, { body: \"b\", icon: \"x.png\" })\ncomponent C() {\n    state on = \"\"\n    <button @click={() => { on = notification.ask({ who: 1 }) }}>{on}</button>\n}\nroute GET / = <html><body><C/></body></html>\n");
    assert!(!ok);
    assert_eq!(
        lines,
        [
            "main.qwe:1: error: `notification.send`: unknown option `icon` (the options are title, body, url, tag, badge, silent, ttl, urgency)",
            "main.qwe:1: error: `notification.send` needs a `title`: the line the device shows first",
            "main.qwe:4: error: `notification.ask`: unknown option `who` (the option is user)",
        ]
    );
    // A wait in one branch is not before an ask in another (review); after an `if` that may have
    // waited, it is.
    let branches = "server fn save(n: int) -> int = n\ncomponent Alerts() {\n    state on = notification.state()\n    state ready = false\n    <div>\n        <button @click={() => {\n            if ready {\n                on = str(save(1))\n            } else {\n                on = notification.ask()\n            }\n        }}>one</button>\n        <button @click={() => {\n            if ready {\n                on = str(save(1))\n            }\n            on = notification.ask()\n        }}>two</button>\n        <button @click={() => {\n            let f = () => save(1)\n            on = notification.ask()\n            on = str(f())\n        }}>three</button>\n    </div>\n}\nroute GET / = <html><body><Alerts/></body></html>\n";
    let (ok, lines) = check_lines("n-check-branches", branches);
    assert!(ok, "{:?}", lines);
    assert_eq!(lines, ["main.qwe:17: warning: `notification.ask` comes after `save` waits in this handler: browsers show the permission prompt only close to the click, so ask first, then wait"]);
    // A send in what `db.atomic` runs is warned of too.
    let (_, lines) = check_lines("n-check-atomic", "route POST /a = db.atomic([\"x\"], () => notification.send(\"a\", { title: \"t\" }))\n");
    assert_eq!(lines, [format!("main.qwe:1: warning: {}", IN_A_TRANSACTION)]);
    // What a module does not have.
    let (ok, lines) = check_lines("n-check-none", "route GET / = notification.push(\"a\")\n");
    assert!(!ok && lines.iter().any(|l| l.contains("`notification` has no function `push`")), "{:?}", lines);
}

// B.3, at run time without the check: `ask` and `stop` on the server fail saying where they go, as
// does `state` outside a component's render; `state` in one gives null, which the browser reads
// again. A component that sends, or seals a user, is refused as it is compiled for the browser,
// and the program does not start.
#[test]
fn notification_on_the_wrong_side_it_fails_saying_why() {
    let src = "route GET /ask = notification.ask()\nroute GET /stop = notification.stop()\nroute GET /state = notification.state()\ncomponent S() {\n    state on = notification.state()\n    <p id=\"on\">[{on ?? \"null\"}]</p>\n}\nroute GET /s = <html><body><S/></body></html>\n";
    let d = dir("n-sides", src);
    let (s, port) = push_server(&d, &["--no-check"], &[]);
    for path in ["/ask", "/stop", "/state"] {
        assert_eq!(support::send(port, "GET", path, &[], "").status, 500, "{}", path);
    }
    assert_eq!(log_with(&s, BROWSER_ONLY).matches(BROWSER_ONLY).count(), 3);
    assert!(page(port, "/s").contains("<p id=\"on\">[null]</p>"));
    for (f, call) in [("send", "notification.send(\"x\", { title: \"t\" })"), ("user", "notification.user(\"x\")")] {
        let d = dir(&format!("n-sides-{}", f), &format!("component Bad() {{\n    let r = {}\n    <p>{{r}}</p>\n}}\nroute GET / = <html><body><Bad/></body></html>\n", call));
        let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"));
        c.args(["run", "main.qwe", "--no-check", "--port", "0"]).current_dir(&d);
        let o = within(c, 30);
        let log = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success() && log.contains(&format!("main.qwe:2: <Bad> (compiled for the browser): `notification.{}` is the server's: call it from a `server fn`", f)), "{}", log);
    }
}

// B.3, Q-956: a program that uses `notification` with no QUILLO_SECRET is told at start that a
// restart ends every subscription; not with one, not under --watch (the dev key in .quillo), not
// a program without it, and once a process (`--instances 2`).
#[test]
fn notification_warns_at_start_without_a_secret() {
    const SAID: &str = "quillo: this program uses `notification` without QUILLO_SECRET: its push key is made at start, so a restart ends every device's subscription; set QUILLO_SECRET, the same on every server";
    let d = dir("n-warn", PUSHER);
    let (s, _) = push_server(&d, &["--instances", "2"], &[("QUILLO_SECRET", "")]);
    assert_eq!(log_with(&s, "listening").matches(SAID).count(), 1);
    drop(s);
    let (s, _) = push_server(&d, &[], &[]);
    assert!(!log_with(&s, "listening").contains(SAID));
    drop(s);
    let (s, _) = push_server(&d, &["--watch"], &[("QUILLO_SECRET", "")]);
    assert!(!log_with(&s, "listening").contains(SAID));
    drop(s);
    let d = dir("n-warn-none", "route GET / = \"home\"\n");
    let (s, _) = push_server(&d, &[], &[("QUILLO_SECRET", "")]);
    let log = log_with(&s, "listening");
    assert!(log.contains("listening") && !log.contains("notification"), "{}", log);
}

/// Runs the worker a server serves (`@CODE@`) in Node's vm, with a browser's worker played: each
/// case delivers a push (`push`: its data as text, or null for none) or clicks a notification
/// (`click`: its `navigate`), with the windows open given (`windows`; `uncontrolled`: their
/// `navigate` fails, as for a window the worker does not control), or makes requests through it
/// (`fetches`: each a mode and the second it is made at, the browser holding the subscription
/// `sub`, or none), and gives what the worker did.
const WORKER_CASES: &str = r#"
const vm = require("vm");
const code = require("fs").readFileSync(process.argv[2], "utf8");
async function run(c) {
  const handlers = {}, did = [];
  const win = (url) => ({ url, focus: async () => { did.push("focus " + url); }, navigate: async (to) => { if (c.uncontrolled) throw new TypeError("not controlled"); did.push("navigate " + to); } });
  const self = {
    addEventListener: (n, f) => { handlers[n] = f; },
    skipWaiting() {},
    registration: { showNotification: async (t, o) => { did.push(["show", t, o]); } },
    clients: { matchAll: async (o) => { did.push("all " + JSON.stringify(o)); return (c.windows || []).map(win); }, openWindow: async (u) => { did.push("open " + u); }, claim: async () => {} },
  };
  const navigator = c.badges ? { setAppBadge: async (n) => { did.push("badge " + n); }, clearAppBadge: async () => { did.push("badge cleared"); } } : {};
  let clock = 0;
  self.registration.pushManager = { getSubscription: async () => (c.sub ? { endpoint: c.sub } : null) };
  const fetch = async (u, o) => { did.push(["fetch", u, o.method, o.credentials, o.headers["content-type"], JSON.parse(o.body)]); return { ok: true }; };
  const setTimeout = (f, ms) => { did.push("wait " + ms); f(); };
  const ctx = { self, caches: { keys: async () => [], delete: async () => true }, location: { origin: "http://app.test" }, navigator, URL, Promise, Number, JSON, TypeError, fetch, setTimeout, Date: { now: () => clock } };
  vm.createContext(ctx);
  vm.runInContext(code, ctx);
  const waits = [];
  for (const [mode, at] of c.fetches || []) {
    clock = at * 1000;
    handlers.fetch({ request: { mode, url: "http://app.test/page", method: "GET" }, waitUntil: (p) => waits.push(p), respondWith: () => did.push("answered") });
  }
  if ("push" in c) handlers.push({ data: c.push === null ? null : { json: () => JSON.parse(c.push) }, waitUntil: (p) => waits.push(p) });
  if ("click" in c) handlers.notificationclick({ notification: { data: c.click === undefined ? undefined : { navigate: c.click }, close: () => did.push("close") }, waitUntil: (p) => waits.push(p) });
  await Promise.all(waits);
  return did;
}
(async () => {
  const out = [];
  for (const c of JSON.parse(process.argv[3])) out.push(await run(c));
  console.log(JSON.stringify(out));
})();
"#;

fn worker_did(code: &str, cases: serde_json::Value) -> serde_json::Value {
    let d = support::temp_dir("f67-worker-did");
    std::fs::write(d.join("sw.js"), code).unwrap();
    std::fs::write(d.join("run.cjs"), WORKER_CASES).unwrap();
    let mut c = std::process::Command::new("node");
    c.arg(d.join("run.cjs")).arg(d.join("sw.js")).arg(cases.to_string());
    let o = within(c, 30);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout).unwrap()
}

// B.3: Quillo's worker shows a push as its declarative message says: title, body, tag, `silent`,
// and `app_badge` as the app's badge where the browser has one (0 takes it away); a push that is
// no JSON, or empty, still shows (Chrome would show its own line otherwise). A click closes the
// notification and focuses a window of the app at the message's address, moves one elsewhere
// there, or opens one; never another site's address (the app's home instead), and a window the
// worker cannot move is left for a new one. Under --base, a window outside it is not the app's.
#[test]
fn notification_the_worker_shows_a_push_and_opens_its_page() {
    let d = dir("n-worker-vm", PUSHER);
    let (_s, port) = push_server(&d, &[], &[]);
    let worker = fetch_body(port, "/_quillo/sw.js").2;
    let push = |n: serde_json::Value| serde_json::json!({ "web_push": 8030, "notification": n }).to_string();
    let all = r#"all {"type":"window","includeUncontrolled":true}"#;
    let r = worker_did(&worker, serde_json::json!([
        { "push": push(serde_json::json!({ "title": "Shipped ✓", "body": "Order 7", "navigate": "http://app.test/orders/7", "tag": "o7", "silent": true, "app_badge": 3 })), "badges": true },
        { "push": push(serde_json::json!({ "title": "t", "navigate": "http://app.test/", "app_badge": 0 })), "badges": true },
        { "push": push(serde_json::json!({ "title": "t", "navigate": "http://app.test/", "app_badge": 2 })) },
        { "push": "not json" },
        { "push": null },
        { "click": "http://app.test/orders/7", "windows": ["http://app.test/orders/7", "http://app.test/"] },
        { "click": "http://app.test/orders/7", "windows": ["http://app.test/cart"] },
        { "click": "http://app.test/orders/7", "windows": [] },
        { "click": "https://evil.test/x", "windows": [] },
        { "click": "http://app.test/orders/7", "windows": ["http://app.test/cart"], "uncontrolled": true },
        { "click": null },
        { "fetches": [["navigate", 100], ["no-cors", 101], ["navigate", 130], ["navigate", 161]], "sub": "https://push.example/s1" },
        { "fetches": [["navigate", 100]] },
        { "fetches": [["cors", 100], ["same-origin", 200]], "sub": "https://push.example/s1" },
    ]));
    assert_eq!(
        r,
        serde_json::json!([
            [["show", "Shipped ✓", { "body": "Order 7", "data": { "navigate": "http://app.test/orders/7" }, "tag": "o7", "silent": true }], "badge 3"],
            [["show", "t", { "body": "", "data": { "navigate": "http://app.test/" } }], "badge cleared"],
            [["show", "t", { "body": "", "data": { "navigate": "http://app.test/" } }]],
            [["show", "", { "body": "", "data": { "navigate": "" } }]],
            [["show", "", { "body": "", "data": { "navigate": "" } }]],
            ["close", all, "focus http://app.test/orders/7"],
            ["close", all, "focus http://app.test/cart", "navigate http://app.test/orders/7"],
            ["close", all, "open http://app.test/orders/7"],
            ["close", all, "open http://app.test/"],
            ["close", all, "focus http://app.test/cart", "open http://app.test/orders/7"],
            ["close", all, "open http://app.test/"],
            // A navigation's look, a little after it, at most once a minute; none for other
            // requests, none without a subscription, and the request itself is not answered.
            ["wait 2000", "wait 2000", ["fetch", "/_quillo/push", "POST", "same-origin", "application/json", { "endpoint": "https://push.example/s1", "check": true }], ["fetch", "/_quillo/push", "POST", "same-origin", "application/json", { "endpoint": "https://push.example/s1", "check": true }]],
            ["wait 2000"],
            [],
        ])
    );
    // Under --base: the app is under /shop.
    let d = dir("n-worker-vm-base", PUSHER);
    let (_s, port) = push_server(&d, &["--base", "/shop"], &[]);
    let worker = fetch_body(port, "/_quillo/sw.js").2;
    let r = worker_did(&worker, serde_json::json!([
        { "click": "http://app.test/shop/orders/7", "windows": ["http://app.test/other"] },
        { "click": "http://app.test/shop/orders/7", "windows": ["http://app.test/other", "http://app.test/shop"] },
        { "click": null },
    ]));
    assert_eq!(
        r,
        serde_json::json!([
            ["close", all, "open http://app.test/shop/orders/7"],
            ["close", all, "focus http://app.test/shop", "navigate http://app.test/shop/orders/7"],
            ["close", all, "open http://app.test/shop/"],
        ])
    );
}

/// The Chrome cases' program: sign-in, a send, a worker of the page's own, and buttons for each
/// of `ask` (its error value shown), `ask` given an option it does not have, `stop` and `state`.
const CHROME_PUSH: &str = r#"route POST /in/:who {
    set_session({ user: who })
    "ok"
}
route POST /send/:who = notification.send(who, request.json)
route GET /own-sw.js = file("own-sw.js")
component Alerts() {
    state on = notification.state()
    state n = 0
    <div>
        <p id="on" data-n={n}>{on}</p>
        <button id="ask" @click={() => {
            let r = notification.ask()
            on = is_err(r) ? `error: ${r.error}` : r
            n += 1
        }}>ask</button>
        <button id="bad" @click={() => {
            let o = { who: 1 }
            on = notification.ask(o)
        }}>bad</button>
        <button id="stop" @click={() => {
            on = notification.stop()
            n += 1
        }}>stop</button>
        <button id="state" @click={() => {
            on = notification.state()
            n += 1
        }}>state</button>
    </div>
}
route GET / = <html><body><Alerts/></body></html>
"#;

/// What the Chrome cases share: DevTools commands asked of run.js (QUILLO_PAGE_CDP), the button
/// clicked and `#on` waited on, and `PushManager` played, headless Chrome having no push service of
/// its own here: a subscription whose keys Node made (`give`), or one that never answers (`hang`).
const PUSH_KIT: &str = r#"
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const cdp = async (cmds) => {
    document.body.setAttribute("data-cdp-out", "");
    document.body.setAttribute("data-cdp", JSON.stringify(cmds));
    for (let i = 0; i < 1000 && document.body.hasAttribute("data-cdp"); i++) await tick(20);
    return JSON.parse(document.body.getAttribute("data-cdp-out"));
  };
  const on = () => document.getElementById("on").textContent;
  const count = () => document.getElementById("on").getAttribute("data-n");
  const press = async (id, n = 1500) => {
    const was = count();
    document.getElementById(id).click();
    for (let i = 0; i < n && count() === was; i++) await tick(20);
    return count() === was ? "no answer (was " + on() + ")" : on();
  };
  const b64 = (b) => btoa(String.fromCharCode(...new Uint8Array(b))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  const sink = "http://127.0.0.1:@SINK@";
  const made = async (kind = "ok") => (await (await fetch(sink + "/_made?n=1&kind=" + kind)).json())[0];
  const played = { subscribed: [], unsubscribed: [] };
  let mine = null;
  const asSub = (s, key) => ({ endpoint: s.endpoint, options: { applicationServerKey: key }, toJSON: () => s, unsubscribe: async () => { played.unsubscribed.push(s.endpoint.split("/").pop()); mine = null; return true; } });
  const give = (s) => {
    PushManager.prototype.subscribe = async function (o) { played.subscribed.push({ userVisibleOnly: o.userVisibleOnly, key: b64(o.applicationServerKey) }); mine = asSub(s, o.applicationServerKey); return mine; };
    PushManager.prototype.getSubscription = async function () { return mine; };
  };
  const hang = () => {
    PushManager.prototype.subscribe = () => new Promise(() => {});
    PushManager.prototype.getSubscription = async function () { return null; };
  };
  const grant = () => cdp([["Browser.grantPermissions", { origin: location.origin, permissions: ["notifications"] }]]);
  const signIn = (who) => fetch("/in/" + who, { method: "POST" });
"#;

const PUSH_FLOW: &str = r#"(async () => {@KIT@
  const out = {};
  try {
    for (let i = 0; i < 500 && on() === ""; i++) await tick(20);
    out.first = on();
    await grant();
    out.permission = Notification.permission;
    // A subscription the browser holds from another key (a QUILLO_SECRET changed): made again.
    const old = await made(), fresh = await made();
    mine = asSub(old, new Uint8Array(65).fill(4).buffer);
    give(fresh);
    out.before = await press("state");
    await signIn("ana");
    out.asked = await press("ask");
    out.again = await press("ask");
    out.state = await press("state");
    out.played = played;
    out.key = (await (await fetch("/_quillo/push")).json()).key;
    out.sent = await (await fetch("/send/ana", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ title: "Shipped ✓", body: "Grüße", url: "/orders/7", tag: "o7" }) })).json();
    const got = await (await fetch(sink + "/_got")).json();
    out.got = got.map((g) => ({ path: g.path.split("-")[0], message: g.message, signed: g.vapid.signature_ok, k: g.vapid.k === out.key }));
    // The push delivered to Quillo's worker by DevTools, which shows it.
    await cdp([["ServiceWorker.enable", {}]]);
    let reg = null;
    for (let i = 0; i < 100 && !reg; i++) {
      const [evs] = await cdp([["@events", { prefix: "ServiceWorker.workerRegistrationUpdated" }]]);
      reg = evs.flatMap((e) => e.params.registrations).find((r) => r.scopeURL === location.origin + "/" && !r.isDeleted);
      if (!reg) await tick(50);
    }
    const r = await navigator.serviceWorker.getRegistration("/");
    out.script = new URL(r.active.scriptURL).pathname;
    // Delivered again where none shows in 3 s: run right after another Chrome went away, the
    // system took the first ones away again (seen on macOS, Chrome 157; never alone). The tag
    // makes a second one replace the first.
    let shown = [];
    for (let round = 0; round < 6 && !shown.length; round++) {
      out.delivered = await cdp([["ServiceWorker.deliverPushMessage", { origin: location.origin + "/", registrationId: reg.registrationId, data: got[0].text }]]);
      for (let i = 0; i < 120 && !shown.length; i++) { shown = await r.getNotifications(); if (!shown.length) await tick(25); }
    }
    out.shown = shown.map((n) => ({ title: n.title, body: n.body, tag: n.tag, data: n.data }));
    out.stopped = await press("stop");
    out.after = await (await fetch("/send/ana", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ title: "t" }) })).json();
    out.unsubscribed = played.unsubscribed;
    document.getElementById("bad").click();
    for (let i = 0; i < 200 && !window.__quilloErrors.length; i++) await tick(20);
    out.errors = window.__quilloErrors;
  } catch (e) { out.error = String(e && e.stack || e); }
  document.body.dataset.result = JSON.stringify(out);
})();"#;

/// `script` in Chrome at `url` with DevTools commands for the page, within `ms`.
fn in_chrome_cdp(chrome: &Path, url: &str, script: &str, ms: u64) -> serde_json::Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let t = support::temp_dir(&format!("f67-chrome-push-{}", script.len()));
    std::fs::write(t.join("t.js"), script).unwrap();
    let mut c = std::process::Command::new("node");
    // Chrome's own notification centre, not the system's: on macOS a Chrome started while the one
    // before was going away showed nothing through the system's, which a test cannot ask.
    c.arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(t.join("profile")).arg(ms.to_string()).arg(t.join("t.js")).env("QUILLO_PAGE_CDP", "1").env("QUILLO_CHROME_FLAGS", "--disable-features=NativeNotifications,SystemNotifications");
    let out = within(c, ms / 1000 + 30);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

// B.3 in Chrome: the permission granted (DevTools), `state` says "default" until `ask`, which
// registers Quillo's worker, makes again a subscription held from another key, subscribes with the
// server's key (`userVisibleOnly`), posts it bound to the signed-in visitor, and says "granted";
// asked again, it keeps the one it has. The server's send reaches Node's push service, decrypted
// and signed; delivered to Quillo's worker (DevTools' deliverPushMessage), the push is shown with
// its title, body, tag and address. `stop` unsubscribes and unbinds it; an option `ask` has not is
// a mistake.
#[test]
fn notification_in_chrome_ask_send_show_stop() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let sink = sink();
    let d = dir("n-chrome", CHROME_PUSH);
    std::fs::write(d.join("own-sw.js"), "self.addEventListener(\"install\", () => self.skipWaiting());\n").unwrap();
    assert!(check(&d).0, "{:?}", check(&d));
    let (_s, port) = push_server(&d, &[], &[]);
    let script = PUSH_FLOW.replace("@KIT@", PUSH_KIT).replace("@SINK@", &sink.port.to_string());
    let r = in_chrome_cdp(&chrome, &format!("http://127.0.0.1:{}/", port), &script, 120000);
    assert_eq!(r["error"], serde_json::Value::Null, "{}", r);
    let key = push_key(port);
    assert_eq!((r["first"].as_str(), r["permission"].as_str(), r["before"].as_str()), (Some("default"), Some("granted"), Some("default")), "{}", r);
    assert_eq!((r["asked"].as_str(), r["again"].as_str(), r["state"].as_str(), r["key"].as_str()), (Some("granted"), Some("granted"), Some("granted"), Some(key.as_str())), "{}", r);
    assert_eq!(r["played"]["subscribed"], serde_json::json!([{ "userVisibleOnly": true, "key": key }]), "{}", r);
    assert_eq!(r["sent"], serde_json::json!({ "sent": 1, "gone": 0 }));
    let nav = format!("http://127.0.0.1:{}/orders/7", port);
    assert_eq!(r["got"], serde_json::json!([{ "path": "/push/ok", "message": { "web_push": 8030, "notification": { "title": "Shipped ✓", "body": "Grüße", "navigate": nav, "tag": "o7" } }, "signed": true, "k": true }]), "{}", r);
    assert_eq!((r["delivered"].clone(), r["script"].as_str()), (serde_json::json!([{}]), Some("/_quillo/sw.js")));
    assert_eq!(r["shown"], serde_json::json!([{ "title": "Shipped ✓", "body": "Grüße", "tag": "o7", "data": { "navigate": nav } }]), "{}", r);
    assert_eq!((r["stopped"].as_str(), r["after"].clone()), (Some("default"), serde_json::json!({ "sent": 0, "gone": 0 })));
    // The one from the other key, then the one `stop` ended.
    assert_eq!(r["unsubscribed"].as_array().map(|a| a.len()), Some(2), "{}", r);
    assert!(r["errors"].to_string().contains("`notification.ask`: unknown option `who` (the option is user)"), "{}", r);
}

const PUSH_EDGES: &str = r#"(async () => {@KIT@
  const out = {};
  try {
    for (let i = 0; i < 500 && on() === ""; i++) await tick(20);
    await grant();
    // No one signed in: the server refuses, ask says why, and what it made is undone.
    give(await made());
    out.nobody = await press("ask");
    out.nobody_state = await press("state");
    out.unsubscribed = played.unsubscribed.length;
    // No push here (a Safari tab on an iPhone): each says so.
    const PM = window.PushManager;
    delete window.PushManager;
    out.unsupported = [await press("state"), await press("ask"), await press("stop")];
    window.PushManager = PM;
    // A worker of the page's own at Quillo's scope: not replaced.
    for (const r of await navigator.serviceWorker.getRegistrations()) await r.unregister();
    const own = await navigator.serviceWorker.register("/own-sw.js", { scope: "/" });
    for (let i = 0; i < 600 && !(own.active && own.active.scriptURL.endsWith("/own-sw.js")); i++) await tick(25);
    await signIn("ana");
    out.own = await press("ask");
    out.own_state = await press("state");
    out.own_stop = await press("stop");
    await own.unregister();
    // A subscribe that never answers: "unsupported" once ask's 20 s are up.
    hang();
    const t0 = Date.now();
    out.late = await press("ask", 2000);
    out.ms = Date.now() - t0;
  } catch (e) { out.error = String(e && e.stack || e); }
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// B.3 in Chrome, the edges: with no one signed in the server refuses the subscription, `ask` gives
// the server's reason as an error value and unsubscribes what it made, so `state` stays "default";
// where the page has no push, `state`, `ask` and `stop` say "unsupported"; a worker of the page's
// own at the scope is not replaced (an error value), `state` and `stop` then say "default"; and a
// subscribe that never answers (as headless Chrome's did in the report) gives "unsupported" after
// ask's 20 s, not a handler waiting for good.
#[test]
fn notification_in_chrome_refusals_and_the_time_limit() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let sink = sink();
    let d = dir("n-chrome-edges", CHROME_PUSH);
    std::fs::write(d.join("own-sw.js"), "self.addEventListener(\"install\", () => self.skipWaiting());\n").unwrap();
    let (_s, port) = push_server(&d, &[], &[]);
    let script = PUSH_EDGES.replace("@KIT@", PUSH_KIT).replace("@SINK@", &sink.port.to_string());
    let r = in_chrome_cdp(&chrome, &format!("http://127.0.0.1:{}/", port), &script, 120000);
    assert_eq!(r["error"], serde_json::Value::Null, "{}", r);
    assert_eq!(r["nobody"].as_str(), Some("error: notification.ask: no one to send to: the visitor is not signed in (session().user), and ask was given no user: notification.user(x), made on the server, says who"), "{}", r);
    assert_eq!((r["nobody_state"].as_str(), r["unsubscribed"].as_u64()), (Some("default"), Some(1)));
    assert_eq!(r["unsupported"], serde_json::json!(["unsupported", "unsupported", "unsupported"]));
    assert_eq!(r["own"].as_str(), Some(format!("error: notification: this page has a service worker of its own (http://127.0.0.1:{}/own-sw.js), which Quillo's would replace", port).as_str()), "{}", r);
    assert_eq!((r["own_state"].as_str(), r["own_stop"].as_str()), (Some("default"), Some("default")));
    assert_eq!(r["late"].as_str(), Some("unsupported"), "{}", r);
    let ms = r["ms"].as_u64().unwrap();
    assert!((19_500..32_000).contains(&ms), "{}", ms);
}

/// `PushManager` played in every page and frame from its start (DevTools'
/// `Page.addScriptToEvaluateOnNewDocument`), the subscription kept in localStorage as the browser
/// keeps its own across page loads: `stub-next` holds the one Node made, which `subscribe` gives.
const STUB_ON_EACH_PAGE: &str = r#"(() => {
  const K = "stub-sub";
  const load = () => { try { return JSON.parse(localStorage.getItem(K)); } catch (_) { return null; } };
  const asSub = (s) => s && { endpoint: s.json.endpoint, options: { applicationServerKey: Uint8Array.from(s.key).buffer }, toJSON: () => s.json, unsubscribe: async () => { localStorage.removeItem(K); return true; } };
  PushManager.prototype.getSubscription = async function () { return asSub(load()); };
  PushManager.prototype.subscribe = async function (o) {
    const s = { json: JSON.parse(localStorage.getItem("stub-next")), key: Array.from(new Uint8Array(o.applicationServerKey)) };
    localStorage.setItem(K, JSON.stringify(s));
    return asSub(s);
  };
})();"#;

const DEVICE_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {};
  try {
    await grant();
    await cdp([["Page.addScriptToEvaluateOnNewDocument", { source: @STUB@ }]]);
    localStorage.setItem("stub-next", JSON.stringify(await made()));
    // A page of the app, started in a frame: its state once the page has looked.
    const open = async () => {
      const f = document.createElement("iframe");
      await new Promise((ok) => { f.onload = ok; f.src = "/"; document.body.appendChild(f); });
      const at = () => f.contentDocument.getElementById("on");
      for (let i = 0; i < 500 && !(at() && at().textContent); i++) await tick(20);
      return f;
    };
    const shown = (f) => f.contentDocument.getElementById("on").textContent;
    await signIn("carol");
    const a = await open();
    out.first = shown(a);
    const was = a.contentDocument.getElementById("on").getAttribute("data-n");
    a.contentDocument.getElementById("ask").click();
    for (let i = 0; i < 500 && a.contentDocument.getElementById("on").getAttribute("data-n") === was; i++) await tick(20);
    out.asked = shown(a);
    out.again = shown(await open());
    const sendCarol = async () => (await (await fetch("/send/carol", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ title: "for carol" }) })).json()).sent;
    out.sent = await sendCarol();
    // Her cookie gone (it ended, or was cleared), and Dave signed in on the device.
    await cdp([["Network.clearBrowserCookies", {}]]);
    await signIn("dave");
    out.dave = shown(await open());
    out.after = await sendCarol();
  } catch (e) { out.error = String(e && e.stack || e); }
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// B.3 review, 1 (b), in Chrome: a page that starts where the browser holds a subscription asks the
// server whether it is still the signed-in person's. Carol's own pages say "granted" and her
// notifications come; once her cookie is gone and Dave signs in on the device, his page's look
// unbinds hers, says "default" so that the app asks him, and Carol's stop coming.
#[test]
fn notification_in_chrome_a_device_is_one_persons() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let sink = sink();
    let d = dir("n-chrome-device", CHROME_PUSH);
    std::fs::write(d.join("own-sw.js"), "self.addEventListener(\"install\", () => self.skipWaiting());\n").unwrap();
    let (_s, port) = push_server(&d, &[], &[]);
    let script = DEVICE_SCRIPT.replace("@KIT@", PUSH_KIT).replace("@SINK@", &sink.port.to_string()).replace("@STUB@", &serde_json::Value::String(STUB_ON_EACH_PAGE.to_string()).to_string());
    let r = in_chrome_cdp(&chrome, &format!("http://127.0.0.1:{}/", port), &script, 120000);
    assert_eq!(r, serde_json::json!({ "first": "default", "asked": "granted", "again": "granted", "sent": 1, "dave": "default", "after": 0 }), "{}", r);
}

/// A server that answers and then resets each connection a second after it goes idle (the second
/// review's rst.cjs, with its port given): `push` paths get a 201, others a 200.
const RESETS: &str = r#"const http = require("http");
const srv = http.createServer((req, res) => {
  req.resume();
  req.on("end", () => {
    res.writeHead(req.url.startsWith("/push/") ? 201 : 200, { "content-type": "text/plain", "content-length": 2 });
    res.end("ok");
    const s = req.socket;
    clearTimeout(s._rt);
    s._rt = setTimeout(() => { if (!s.destroyed) s.resetAndDestroy(); }, 1000);
  });
});
srv.keepAliveTimeout = 60000;
srv.listen(+process.argv[2], "127.0.0.1", () => console.log(srv.address().port));
"#;

// B.3, second review, 1: `http.*`'s agents are as they were, keeping one connection a host: after
// eight calls at once and the server resetting the idle connections, six calls in a row (POSTs,
// then GETs) fail once at most, as before the slice. Push keeps its own connections, as many as it
// sends at once, and a message whose kept connection the server had reset goes again on a new one:
// no failure.
#[test]
fn notification_a_reset_kept_connection_fails_nothing() {
    let d = support::temp_dir("f67-resets");
    std::fs::write(d.join("rst.cjs"), RESETS).unwrap();
    // On port 0, the port it was given printed, as the sink (branch security).
    let child = std::process::Command::new("node").arg(d.join("rst.cjs")).arg("0").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().unwrap();
    let mut _rst = support::Guard::new(child);
    let rst = support::port_on_stdout(&mut _rst.0, "the resetting server", Duration::from_secs(20));
    let url = format!("http://127.0.0.1:{}/x", rst);
    let calls: Vec<String> = (0..8).map(|_| format!("() => http.post({:?}, \"a\")", url)).collect();
    let src = format!(
        "{}route GET /burst {{\n    let rs = parallel({})\n    return rs | map((r) => is_err(r) ? \"ERR\" : `${{r.status}}`)\n}}\nroute GET /one {{\n    let r = http.post({:?}, \"a\")\n    return is_err(r) ? \"ERR\" : `${{r.status}}`\n}}\nroute GET /oneget {{\n    let r = http.get({:?})\n    return is_err(r) ? \"ERR\" : `${{r.status}}`\n}}\n",
        PUSHER, calls.join(", "), url, url
    );
    let d = dir("n-resets", &src);
    let (_s, port) = push_server(&d, &[], &[]);
    for one in ["/one", "/oneget"] {
        let burst = support::send(port, "GET", "/burst", &[], "").body;
        assert!(!burst.contains("ERR"), "{}", burst);
        std::thread::sleep(Duration::from_millis(1600));
        let failed = (0..6).filter(|_| support::send(port, "GET", one, &[], "").body.contains("ERR")).count();
        assert!(failed <= 1, "{}: {} of 6 failed", one, failed);
    }
    // Push, to the same server: eight devices, sent to at once, their connections reset idle.
    let sink = sink();
    let (ana, _) = session_step(port, "/in/ana", "");
    for (i, mut sub) in sink.made(8, "ok").into_iter().enumerate() {
        sub["endpoint"] = serde_json::json!(format!("http://127.0.0.1:{}/push/{}", rst, i));
        assert_eq!(bind(port, &sub, serde_json::Value::Null, &ana).status, 200);
    }
    for _ in 0..3 {
        assert_eq!(send_to(port, "ana", serde_json::json!({ "title": "t" })), serde_json::json!({ "sent": 8, "gone": 0 }));
        std::thread::sleep(Duration::from_millis(1600));
    }
    assert_eq!(send_to(port, "ana", serde_json::json!({ "title": "t" })), serde_json::json!({ "sent": 8, "gone": 0 }));
}

// B.3, second review, 2 and 4: the person's own device found from a later session (her cookie
// lost, a sign-in again) is that session's now, by a page's look or by a bind, so that its end
// unbinds it. A ticket names the person its session had: after a step with no one and another
// person signing in on the same session (its hidden id kept, Q-887), it is refused.
#[test]
fn notification_a_device_moves_with_its_person() {
    let sink = sink();
    let d = dir("n-moves", PUSHER);
    let (_s, port) = push_server(&d, &[], &[]);
    let one = serde_json::json!({ "title": "t" });
    let (x, _) = session_step(port, "/in/alice", "");
    let sub = sink.made(1, "ok").remove(0);
    assert_eq!(bind(port, &sub, serde_json::Value::Null, &x).status, 200);
    let (z, _) = session_step(port, "/in/alice", "");
    assert_eq!(check_sub(port, &sub, &z), serde_json::json!({ "state": "granted" }));
    session_step(port, "/out", &z);
    assert_eq!(send_to(port, "alice", one.clone())["sent"], 0, "signed out where the look moved it");
    let (y1, _) = session_step(port, "/in/aly", "");
    let sub = sink.made(1, "ok").remove(0);
    assert_eq!(bind(port, &sub, serde_json::Value::Null, &y1).status, 200);
    let (y2, _) = session_step(port, "/in/aly", "");
    assert_eq!(bind(port, &sub, serde_json::Value::Null, &y2).status, 200);
    session_step(port, "/out", &y2);
    assert_eq!(send_to(port, "aly", one.clone())["sent"], 0, "signed out where the bind moved it");
    // Alice9's ticket, a session of no one, then Bob9 on it.
    let (a9, _) = session_step(port, "/in/alice9", "");
    let t = support::send(port, "GET", "/ticket/alice9", &[("cookie", &a9)], "").body;
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::json!(t), &a9).status, 200);
    let (nobody, _) = session_step(port, "/cart", &a9);
    let (b9, _) = session_step(port, "/in/bob9", &nobody);
    let a = bind(port, &sink.made(1, "ok")[0], serde_json::json!(t), &b9);
    assert_eq!(a.status, 403, "{}", a.body);
    assert!(a.body.contains("not one notification.user made here, for this session"), "{}", a.body);
    // A guest's ticket: taken from its session of no one, not once someone signs in on it.
    let (g, _) = session_step(port, "/cart", "");
    let t = support::send(port, "GET", "/ticket/guest-3", &[("cookie", &g)], "").body;
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::json!(t), &g).status, 200);
    let (kim, _) = session_step(port, "/in/kim", &g);
    assert_eq!(bind(port, &sink.made(1, "ok")[0], serde_json::json!(t), &kim).status, 403);
}

// B.3, second review, 5: a send in a service's server fn, called in the caller's SQLite
// transaction, is refused as one in the transaction itself is: the service's own interpreter has
// none open, and it held other requests for the push services.
#[test]
fn notification_send_in_a_service_in_a_transaction_is_refused() {
    let main = "db.exec(\"create table if not exists t (id integer primary key, v text)\")\nservice notifier = \"notifier.qwe\"\nroute POST /tx-service/:who {\n    return db.transaction(() => {\n        db.exec(\"insert into t (v) values ('A1')\")\n        return notifier.tell(who)\n    })\n}\nroute POST /no-tx/:who = notifier.tell(who)\nroute GET /t = notification.user(\"x\")\n";
    let svc = "server fn tell(who: str) -> map {\n    let r = notification.send(who, { title: \"from the service\" })\n    return is_err(r) ? { error: r.error } : r\n}\n";
    let d = dir("n-svc-tx", main);
    std::fs::write(d.join("notifier.qwe"), svc).unwrap();
    std::fs::write(d.join(".env"), "QUILLO_DB=app.db\n").unwrap();
    let (_s, port) = push_server(&d, &["--no-check"], &[]);
    let a = support::send(port, "POST", "/tx-service/ana", &[], "");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&a.body).unwrap_or_default()["error"], IN_A_TRANSACTION, "{} {}", a.status, a.body);
    let a = support::send(port, "POST", "/no-tx/ana", &[], "");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&a.body).unwrap_or_default(), serde_json::json!({ "sent": 0, "gone": 0 }), "{}", a.body);
}

// B.3, second review, 6: looks at a subscription, which unbind where the session has no one, are
// held to 60 a minute by the client's address; binds with no session to 30 (their 403s counted),
// each apart.
#[test]
fn notification_looks_and_binds_with_no_session_are_limited() {
    let sink = sink();
    let d = dir("n-limits", PUSHER);
    let (_s, port) = push_server(&d, &[], &[]);
    let sub = sink.made(1, "ok").remove(0);
    let host = format!("127.0.0.1:{}", port);
    let look = || support::send(port, "POST", "/_quillo/push", &[("host", host.as_str())], &serde_json::json!({ "endpoint": sub["endpoint"], "check": true }).to_string());
    for i in 0..60 {
        assert_eq!(look().status, 200, "look {}", i);
    }
    let a = look();
    assert_eq!(a.status, 429, "{}", a.body);
    assert!(a.body.contains("more than 60 looks at a subscription in a minute from this address"), "{}", a.body);
    for i in 0..30 {
        assert_eq!(bind(port, &sub, serde_json::Value::Null, "").status, 403, "bind {}", i);
    }
    let a = bind(port, &sub, serde_json::Value::Null, "");
    assert_eq!(a.status, 429, "{}", a.body);
    assert!(a.body.contains("more than 30 subscriptions changed in a minute from this address"), "{}", a.body);
    // A session's own binds are counted by the session.
    let (ana, _) = session_step(port, "/in/ana", "");
    assert_eq!(bind(port, &sub, serde_json::Value::Null, &ana).status, 200);
}

const WORKER_LOOK_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {};
  try {
    await grant();
    await cdp([["Page.addScriptToEvaluateOnNewDocument", { source: @STUB@ }]]);
    localStorage.setItem("stub-next", JSON.stringify(await made()));
    const open = async (path) => {
      const f = document.createElement("iframe");
      await new Promise((ok) => { f.onload = ok; f.src = path; document.body.appendChild(f); });
      return f;
    };
    const sendFrank = async () => (await (await fetch("/send/frank", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ title: "for frank" }) })).json()).sent;
    await signIn("frank");
    const a = await open("/");
    const on = () => a.contentDocument.getElementById("on");
    for (let i = 0; i < 500 && !(on() && on().textContent); i++) await tick(20);
    const was = on().getAttribute("data-n");
    a.contentDocument.getElementById("ask").click();
    for (let i = 0; i < 500 && on().getAttribute("data-n") === was; i++) await tick(20);
    out.asked = on().textContent;
    out.sent = await sendFrank();
    // The worker's own pushManager holds what the page's stub made (DevTools, in the worker).
    const endpoint = JSON.parse(localStorage.getItem("stub-sub")).json.endpoint;
    const [t] = await cdp([["Target.getTargets", {}]]);
    const sw = t.targetInfos.find((x) => x.type === "service_worker" && x.url.endsWith("/_quillo/sw.js"));
    const [att] = await cdp([["Target.attachToTarget", { targetId: sw.targetId, flatten: true }]]);
    const [ev] = await cdp([["Runtime.evaluate", { expression: `self.registration.pushManager.getSubscription = async () => ({ endpoint: ${JSON.stringify(endpoint)} }); "stubbed"`, returnByValue: true }, att.sessionId]]);
    out.stubbed = ev.result && ev.result.value;
    // Frank's cookie gone, George signed in, and a page with no component opened.
    await cdp([["Network.clearBrowserCookies", {}]]);
    await signIn("george");
    const plain = await open("/plain");
    out.plain = plain.contentDocument.body.textContent.trim();
    out.scripts = plain.contentDocument.scripts.length;
    let left = 1;
    for (let i = 0; i < 40 && left; i++) { await tick(250); left = await sendFrank(); }
    out.after = left;
  } catch (e) { out.error = String(e && e.stack || e); }
  document.body.dataset.result = JSON.stringify(out);
})();"#;

// B.3, second review, 3, in Chrome: a page with no component has no runtime to look at the
// subscription, so Quillo's worker does on navigations (a little after, at most once a minute):
// Frank's device, his cookie gone and George signed in, stops getting Frank's notifications once
// George opens a plain page.
#[test]
fn notification_in_chrome_the_worker_looks_on_a_plain_page() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let sink = sink();
    let d = dir("n-chrome-plain", &format!("{}route GET /plain = <html><body><p>plain</p></body></html>\n", CHROME_PUSH));
    std::fs::write(d.join("own-sw.js"), "self.addEventListener(\"install\", () => self.skipWaiting());\n").unwrap();
    let (_s, port) = push_server(&d, &[], &[]);
    let script = WORKER_LOOK_SCRIPT.replace("@KIT@", PUSH_KIT).replace("@SINK@", &sink.port.to_string()).replace("@STUB@", &serde_json::Value::String(STUB_ON_EACH_PAGE.to_string()).to_string());
    let r = in_chrome_cdp(&chrome, &format!("http://127.0.0.1:{}/", port), &script, 120000);
    assert_eq!(r, serde_json::json!({ "asked": "granted", "sent": 1, "stubbed": "stubbed", "plain": "plain", "scripts": 0, "after": 0 }), "{}", r);
}
