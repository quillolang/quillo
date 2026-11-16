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
    (logged_at(d, port, opts), port)
}

/// `logged`, on `port`: a server started again where the one before was.
fn logged_at(d: &Path, port: u16, opts: &[&str]) -> Logged {
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
    s
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
        let port = support::free_port();
        let mut first = Some(logged_at(&d, port, opts));
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
  const kept = async () => {
    const out = {};
    for (const n of await caches.keys()) {
      if (n.endsWith(":gone")) continue;
      const k = (await (await caches.open(n)).keys()).map((r) => decodeURI(new URL(r.url).pathname).replace(/\/[0-9a-f]{16}\./, "/H.")).filter((p) => !p.endsWith("/_quillo/sw-ready")).sort();
      out[n.replace(/:[0-9a-f]{16}/, ":V").replace(/:V:.+$/, ":V:S")] = k;
    }
    return out;
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
// it. Online, a page comes from the network each time and a server fn is answered by the server;
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
            "menu": ["Menu 2", "Menu 3"], "call": "add 2", "ready": 200,
            "kept": { "quillo::V": files },
            "down": true,
            "offline": {
                "/menu": { "h1": "Menu 3", "kept": "iso", "offline": false, "title": "" },
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
  out.privates = await privates();
  // A server fn: the runtime tells the worker.
  document.getElementById("out").click();
  out.after_fn = await until(async () => (await privates()) === 0 && (await kept()));
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

/// What registers the worker, at the base given.
fn register_script(base: &str) -> String {
    format!("<script>(n=>n&&n.register(\"{0}/_quillo/sw.js\",{{scope:\"{0}/\"}}))(navigator.serviceWorker)</script>", base)
}

/// Under the guide's content policy, with nothing of its own on the page but the register script;
/// its stages its own, as the policy lets the page reach no other server.
const GUARDED: &str = "let stage = 0\nroute GET /stage/:n(int) {\n    stage = n\n    \"ok\"\n}\nroute GET /stage = stage\nsite_headers(\"/\", { \"content-security-policy\": \"default-src 'self'\" })\nroute offline GET / = <html><head><title>p</title></head><body><h1>Home</h1></body></html>\nroute offline GET /menu = <html><body><h1>Menu</h1></body></html>\nroute GET /ping = \"pong\"\n";

const WATCHED_SCRIPT: &str = r#"(async () => {@KIT@
  const out = {}, refused = [];
  document.addEventListener("securitypolicyviolation", (e) => refused.push(e.violatedDirective));
  out.controlled = await controlled();
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
}).listen(listen, "127.0.0.1", () => console.log("ready"));
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
    let listen = support::free_port();
    let proxy = std::process::Command::new("node").arg("-e").arg(PROXY).arg(listen.to_string()).arg(port.to_string()).arg("/shop").stdout(std::process::Stdio::null()).spawn().unwrap();
    let _proxy = support::Guard::new(proxy);
    let end = Instant::now() + Duration::from_secs(20);
    while !support::listening(listen) {
        assert!(Instant::now() < end, "the proxy did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
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
  const W = WebSocket;
  let made = 0, closed = 0;
  window.WebSocket = function (u, p) { const s = p === undefined ? new W(u) : new W(u, p); made++; s.addEventListener("close", () => closed++); return s; };
  window.WebSocket.prototype = W.prototype;
  for (let i = 0; i < 200 && document.getElementById("v").textContent !== "count 0"; i++) await tick(25);
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
    assert_eq!(register_script("").len(), 90);
    let m = |method: &str, path: &str| marks_of(port, method, path, None);
    let one = || Some("1".to_string());
    assert_eq!(m("GET", "/"), (200, one(), None, None, 1));
    assert_eq!(m("HEAD", "/"), (200, one(), None, None, 0));
    assert_eq!(m("GET", "/item/5"), (200, one(), None, None, 1));
    assert_eq!(m("GET", "/caf%C3%A9"), (200, one(), None, None, 1));
    assert_eq!(m("GET", "/docs/a"), (200, one(), None, None, 1));
    // No session: the visitor's page is the one of no session's, `-`.
    assert_eq!(m("GET", "/mine"), (200, Some("private".into()), Some("-".into()), None, 1));
    for path in ["/nulled", "/theme", "/agent", "/order"] {
        assert_eq!(m("GET", path), (200, None, None, None, 1), "{}", path);
    }
    assert_eq!(m("POST", "/docs/b"), (200, None, None, None, 0));
    assert_eq!(m("GET", "/plain"), (200, None, None, None, 0));
    assert_eq!(m("GET", "/data"), (200, None, None, None, 0));
    assert_eq!(m("GET", "/missing"), (404, None, None, None, 0));
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

// B.2 review: the session's name is a digest of its hidden id, so a cart or a message written into
// the session keeps it, and two sign-ins of one browser as one person too; another person is
// another name. `quillo_tag`, the cookie that names it, comes with the session cookie: readable
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
        // A cart in the session: the same name.
        let (_, h, _) = fetch(port, "GET", "/cart", &[("cookie", &ana)]);
        assert_eq!(header(&h, "x-quillo-session"), Some(tag.as_str()));
        let cart = h.iter().find(|(k, v)| k == "set-cookie" && v.contains("quillo_session=")).map(|(_, v)| v.split(';').next().unwrap().to_string()).unwrap();
        assert_ne!(cart, ana, "the session cookie changed");
        assert_eq!(marks_of(port, "GET", "/mine", Some(&format!("{}; quillo_tag={}", cart, tag))).2, Some(tag.clone()));
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
  // A late answer of a session dropped is not kept again (the server still takes ana's cookie).
  out.late = (await look("/mine")).h1;
  out.not_again = await kept();
  // Another person's tag: the offline page.
  await look("/in/bob");
  const bob = tagNow();
  out.bob = (await look("/mine")).h1;
  await stage(3);
  out.down2 = await down();
  document.cookie = "quillo_tag=someone" + secure;
  out.other = await lookUntil("/mine", (d) => d.documentElement.hasAttribute("data-quillo-offline"));
  out.after_other = await kept();
  out.dropped2 = (await dropped()).map((t) => t === ana ? "ana" : t === bob ? "bob" : t);
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
            "up": true, "late": "Mine ana", "not_again": public,
            "bob": "Mine bob", "down2": true,
            "other": offline, "after_other": public, "dropped2": ["ana", "bob"],
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
  const versions = async () => { const out = []; for (const n of await caches.keys()) if (!n.endsWith(":gone")) out.push((await (await caches.open(n)).keys()).map((r) => new URL(r.url).pathname).filter((p) => !p.endsWith("sw-ready")).sort().join(" ")); return out.sort(); };
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
fn proxy(port: u16, base: &str, mode: &str) -> (support::Guard, u16) {
    let listen = support::free_port();
    let child = std::process::Command::new("node").arg("-e").arg(PROXY).arg(listen.to_string()).arg(port.to_string()).arg(base).arg(mode).stdout(std::process::Stdio::null()).spawn().unwrap();
    let g = support::Guard::new(child);
    let end = Instant::now() + Duration::from_secs(20);
    while !support::listening(listen) {
        assert!(Instant::now() < end, "the proxy did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
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
    // With the tag, `/menu` is also made once as the worker installs.
    for (tag, controlled, menu) in [("offline ", true, "Menu 3"), ("", false, "Menu 2")] {
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
const caches = { async open(n) { if (!store.has(n)) store.set(n, new Map()); return cache(n); }, async keys() { return [...store.keys()]; }, async delete(n) { return store.delete(n); } };
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
