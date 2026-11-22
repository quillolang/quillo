//! FEEDBACK82-85, slice H (FEEDBACK84.md, single-page apps' routing and head; FEEDBACK81.md,
//! Q-1275): `page_head({ title, description, image, canonical })` writes the page's head on the
//! server, the innermost call winning, and follows state in the browser (Q-1320, Q-1321, Q-1275);
//! `page_url()` (Q-1328); `quillo check` on a component in `<head>` (Q-1322) and on
//! `json.stringify` in a JSON-LD script (Q-1329); `moves_in_place()`, every same-site link moved to
//! its route's page in place, components on both pages and the page's states kept (Q-1323, Q-1327),
//! Back giving the scroll back (Q-1324), the focus moved and the title said (Q-1325), `navigate`'s
//! `replace` and `scroll` (Q-1326), and a link clicked before its page came asking once (Q-1330).
//! Branch feedback82-85.

mod support;

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Dir {
    fn new(name: &str, src: &str) -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = support::temp_dir(&format!("f8285h-{}-{}", name, N.fetch_add(1, Ordering::SeqCst)));
        std::fs::write(d.join("main.qwe"), src).unwrap();
        Dir(d)
    }
}

fn quillo() -> PathBuf {
    std::env::var_os("QUILLO_TEST_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_quillo")))
}

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

/// `quillo <cmd> main.qwe` on `src` under a deadline: whether it succeeded, and what it printed.
fn quillo_on(cmd: &str, src: &str) -> (bool, String) {
    let d = Dir::new(cmd, src);
    let mut c = Command::new(quillo()).arg(cmd).arg(d.0.join("main.qwe")).current_dir(&d.0).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
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

/// What `quillo check` says of `src`, which passes.
fn checked(src: &str) -> String {
    let (ok, out) = quillo_on("check", src);
    assert!(ok, "quillo check:\n{}\n{}", out, src);
    out
}

struct Served {
    dir: Dir,
    log: PathBuf,
    port: u16,
    _guard: support::Guard,
}

fn served(name: &str, src: &str, opts: &[&str]) -> Served {
    served_files(name, src, opts, &[])
}

/// `served`, with other files beside main.qwe (a mounted program's).
fn served_files(name: &str, src: &str, opts: &[&str], files: &[(&str, &str)]) -> Served {
    let dir = Dir::new(name, src);
    for (f, text) in files {
        std::fs::write(dir.0.join(f), text).unwrap();
    }
    let out = dir.0.join("check.txt");
    let mut c = Command::new(quillo())
        .arg("check")
        .arg(dir.0.join("main.qwe"))
        .current_dir(&dir.0)
        .stdout(std::fs::File::create(&out).unwrap())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let ok = wait_for(&mut c, Duration::from_secs(120)).is_some_and(|s| s.success());
    assert!(ok, "quillo check:\n{}\n{}", std::fs::read_to_string(&out).unwrap_or_default(), src);
    let log = dir.0.join("log.txt");
    let mut child = Command::new(quillo())
        .arg("run")
        .arg(dir.0.join("main.qwe"))
        .args(["--port", "0"])
        .args(opts)
        .current_dir(&dir.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let port = support::port_in_log(&mut child, &log, Duration::from_secs(30));
    Served { dir, log, port, _guard: support::Guard::new(child) }
}

impl Served {
    fn get(&self, path: &str) -> support::Answer {
        support::send(self.port, "GET", path, &[], "")
    }
    fn get_with(&self, path: &str, headers: &[(&str, &str)]) -> support::Answer {
        support::send(self.port, "GET", path, headers, "")
    }
    fn page(&self, path: &str) -> String {
        let a = self.get(path);
        assert_eq!(a.status, 200, "{}: {}\n{}", path, a.body, self.log());
        a.body
    }
    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

/// The text between the page's `<head>` and `</head>`.
fn head_of(page: &str) -> &str {
    let from = page.find("<head>").map_or(0, |i| i + 6);
    let to = page.find("</head>").unwrap_or(page.len());
    &page[from..to]
}

// ---- Q-1320, Q-1321, Q-1275: page_head on the server --------------------------------------------

const LAYOUT: &str = r#"
view Page(title: str, children) {
    page_head({ title: `${title} · Blog`, description: "The blog" })
    return <html lang="en">
        <head><meta charset="utf-8"/><title>Layout's own</title><meta name="description" content="layout's"/></head>
        <body><nav><a href="/">Home</a> <a href="/posts/3">Post</a></nav>{children}</body>
    </html>
}
view Post(id: int) {
    page_head({ title: `Post ${id} · Blog`, image: "/img/card.png", canonical: `/posts/${id}` })
    return <article><h2>Post {id}</h2></article>
}
view Deep(name: str) = <section><Named name={name}/></section>
view Named(name: str) {
    page_head({ title: name })
    return <p>{name}</p>
}
view PostPage(id: int) {
    page_head({ title: `Page view ${id}` })
    return <Page title="Ignored"><Post id={id}/></Page>
}
view OwnPage() {
    page_head({ title: "Own" })
    return <Page title="Layout"><p>own</p></Page>
}
route GET / = <Page title="Home"><h2>Home</h2></Page>
route GET /posts/:id(int) = <Page title="Posts"><Post id={id}/></Page>
route GET /deep = <Page title="Deep"><Deep name="From deep inside"/></Page>
route GET /page/:id(int) = <PostPage id={id}/>
route GET /own = <OwnPage/>
route GET /null = <Page title="N">{page_head({ title: null, description: "said" })}<p>n</p></Page>
route GET /bare = <html><body>{page_head({ title: "Bare" })}<p>b</p></body></html>
route GET /part = <li>{page_head({ title: "Part" })}x</li>
"#;

/// The layout's title stands where the page says none; a view inside it, a view deep in the page,
/// a page view that wraps itself in the layout, each wins for what it says; the layout's own tags
/// of those keys go, and the others stay.
#[test]
fn the_innermost_page_head_call_writes_the_head() {
    let s = served("head", LAYOUT, &[]);
    let home = s.page("/");
    let h = head_of(&home);
    assert!(h.contains("<title>Home · Blog</title>") && h.contains("<meta property=\"og:title\" content=\"Home · Blog\">"), "{}", h);
    assert!(h.contains("<meta name=\"description\" content=\"The blog\">") && !h.contains("Layout's own") && !h.contains("layout's"), "{}", h);
    assert!(h.contains("<meta charset=\"utf-8\">"), "{}", h);
    assert_eq!(home.matches("<title>").count(), 1, "{}", home);
    let post = s.page("/posts/3");
    let h = head_of(&post);
    // The tests' requests say `Host: localhost`.
    let me = "http://localhost";
    assert!(h.contains("<title>Post 3 · Blog</title>"), "{}", h);
    assert!(h.contains(&format!("<meta property=\"og:image\" content=\"{}/img/card.png\">", me)), "{}", h);
    assert!(h.contains(&format!("<link rel=\"canonical\" href=\"{}/posts/3\"><meta property=\"og:url\" content=\"{}/posts/3\">", me, me)), "{}", h);
    // The layout's description, which the post says nothing of.
    assert!(h.contains("content=\"The blog\""), "{}", h);
    assert!(head_of(&s.page("/deep")).contains("<title>From deep inside</title>"));
    // A page view around the layout: its own title, and the post's inside it.
    assert!(head_of(&s.page("/page/7")).contains("<title>Post 7 · Blog</title>"));
    assert!(head_of(&s.page("/own")).contains("<title>Own</title>"));
    // Null says nothing: the layout's title stands.
    let n = s.page("/null");
    assert!(head_of(&n).contains("<title>N · Blog</title>") && head_of(&n).contains("content=\"said\""), "{}", n);
    // Asked again (a view kept by the view cache): the same head.
    assert_eq!(head_of(&s.page("/posts/3")), head_of(&post));
    // A page with no head gets one before its body; a part of a page, nothing.
    let bare = s.page("/bare");
    assert!(bare.contains("<html><title>Bare</title><meta property=\"og:title\" content=\"Bare\"><body>"), "{}", bare);
    let part = s.page("/part");
    assert!(!part.contains("<title>"), "{}", part);
}

/// What goes in the head is escaped as text and attributes are, unicode kept whole; a key that is
/// none of the four, or a value that is no text, is an error that names it.
#[test]
fn page_head_values_are_escaped_and_its_keys_checked() {
    let src = r#"
route GET /x = <html><head><title>t</title></head><body>{page_head({ title: "</title><script>alert(1)</script>", description: "He said \"hi\" & left <b>" })}x</body></html>
route GET /u = <html><head></head><body>{page_head({ title: "Café · 東京 😀", description: "" })}x</body></html>
route GET /n = <html><head></head><body>{page_head({ title: 3 })}x</body></html>
route GET /bad = <html><head></head><body>{page_head({ titel: "x" })}x</body></html>
route GET /list = <html><head></head><body>{page_head({ title: ["a"] })}x</body></html>
route GET /path = <html><head></head><body>{page_head({ image: "img.png" })}x</body></html>
route GET /js = <html><head></head><body>{page_head({ canonical: "javascript:alert(1)" })}x</body></html>
route GET /whole = <html><head></head><body>{page_head({ canonical: "https://other.example/a?b=1" })}x</body></html>
"#;
    let s = served("esc", src, &[]);
    let x = s.page("/x");
    assert!(head_of(&x).contains("<title>&lt;/title&gt;&lt;script&gt;alert(1)&lt;/script&gt;</title>"), "{}", x);
    assert!(head_of(&x).contains("content=\"He said &quot;hi&quot; &amp; left &lt;b&gt;\""), "{}", x);
    assert!(!x.contains("<script>alert"), "{}", x);
    let u = s.page("/u");
    assert!(head_of(&u).contains("<title>Café · 東京 😀</title>") && head_of(&u).contains("<meta name=\"description\" content=\"\">"), "{}", u);
    assert!(head_of(&s.page("/n")).contains("<title>3</title>"));
    for (path, said) in [("/bad", "has no key `titel`"), ("/list", "is text or null"), ("/path", "takes a path that starts with /"), ("/js", "takes a path that starts with /")] {
        let a = s.get(path);
        assert_eq!(a.status, 500, "{}: {}", path, a.body);
        assert!(s.log().contains(said), "{}: {}", path, s.log());
    }
    assert!(head_of(&s.page("/whole")).contains("<link rel=\"canonical\" href=\"https://other.example/a?b=1\">"));
}

/// A component's call writes the head on the server as a view's does.
#[test]
fn a_components_page_head_is_in_the_servers_head() {
    let src = r#"
component Title(name: str) {
    state n = 1
    page_head({ title: `${name} ${n}` })
    <button onclick={() => { n = n + 1 }}>{n}</button>
}
route GET / = <html><head><title>Old</title></head><body><Title name="Shop"/></body></html>
"#;
    let s = served("comp", src, &[]);
    let p = s.page("/");
    assert!(head_of(&p).contains("<title>Shop 1</title>") && !p.contains("Old"), "{}", p);
}

// ---- Q-1328: page_url ---------------------------------------------------------------------------

#[test]
fn page_url_is_the_pages_public_address() {
    let src = r#"
route GET /posts/:id(int) = <p id="u">{page_url()}|{page_url("/img/card.png")}|{page_url("https://cdn.example/x.png")}</p>
route GET /bad = <p>{page_url("img.png")}</p>
route GET /net = <p>{page_url("//evil.example/x")}</p>
"#;
    let s = served("url", src, &[]);
    // The tests' requests say `Host: localhost`.
    let me = "http://localhost";
    let p = s.page("/posts/3?x=1");
    assert!(p.contains(&format!("{me}/posts/3|{me}/img/card.png|https://cdn.example/x.png")), "{}", p);
    assert_eq!(s.get("/bad").status, 500);
    assert_eq!(s.get("/net").status, 500);
    // A proxy nobody trusts says nothing of the scheme.
    let p = s.get_with("/posts/3", &[("x-forwarded-proto", "https")]).body;
    assert!(p.contains(&format!("{me}/posts/3|")), "{}", p);
    // Under --base, behind a proxy --trust-proxy names: its scheme and the prefix.
    let b = served("base", src, &["--base", "/shop", "--trust-proxy", "127.0.0.1"]);
    let p = b.get_with("/posts/3", &[("x-forwarded-proto", "https"), ("host", "shop.example")]).body;
    assert!(p.contains("https://shop.example/shop/posts/3|https://shop.example/shop/img/card.png|"), "{}", p);
}

// ---- Q-1322, Q-1329: what quillo check says ---------------------------------------------------

#[test]
fn a_component_in_head_is_refused_and_a_view_there_is_not() {
    let (ok, out) = quillo_on(
        "check",
        r#"component Head() {
    state t = "x"
    <title>{t}</title>
}
route GET / = <html><head><Head/><meta charset="utf-8"/></head><body>x</body></html>
"#,
    );
    assert!(!ok && out.contains("a component cannot stand in <head>") && out.contains("page_head"), "{}", out);
    let out = checked("view Meta(t: str) = <title>{t}</title>\nroute GET / = <html><head><Meta t=\"x\"/></head><body>x</body></html>\n");
    assert!(!out.contains("cannot stand"), "{}", out);
}

#[test]
fn json_stringify_in_a_json_ld_script_is_warned_of_and_the_map_is_not() {
    let out = checked(
        r#"let item = { "@type": "Product", name: "Kettle" }
route GET / = <html><head><script type="application/ld+json">{json.stringify(item)}</script></head><body>x</body></html>
"#,
    );
    assert!(out.contains("`json.stringify(...)` in a JSON-LD script"), "{}", out);
    let out = checked(
        r#"let item = { "@type": "Product", name: "Kettle" }
route GET / = <html><head><script type="application/ld+json">{item}</script><script>{json.stringify(item)}</script></head><body>x</body></html>
"#,
    );
    assert!(!out.contains("JSON-LD"), "{}", out);
}

// ---- Q-1326: navigate's options on the server -----------------------------------------------------

#[test]
fn navigate_takes_replace_and_scroll() {
    let src = r#"
route GET /a = <p>{navigate("/x", { replace: true, scroll: false })}{navigate("/y", null)}{navigate("/z")}ok</p>
route GET /b = <p>{navigate("/x", { reload: true })}</p>
route GET /c = <p>{navigate("/x", { replace: "yes" })}</p>
"#;
    let s = served("nav", src, &[]);
    assert!(s.page("/a").contains("ok"));
    for (p, said) in [("/b", "has no option `reload`"), ("/c", "is true or false")] {
        assert_eq!(s.get(p).status, 500, "{}", p);
        assert!(s.log().contains(said), "{}: {}", p, s.log());
    }
    // Options that are no map are the checker's to refuse.
    let (ok, out) = quillo_on("check", "route GET /d = <p>{navigate(\"/d\", \"replace\")}</p>\n");
    assert!(!ok && out.contains("argument 2 of `navigate` must be map | null"), "{}", out);
}

// ---- Q-1323: moves_in_place, the server's side ---------------------------------------------------

#[test]
fn moves_in_place_puts_the_whole_bundle_on_every_page_and_nothing_without_it() {
    let pages = "route GET / = <html><head><title>A</title></head><body><a href=\"/b\">b</a></body></html>\nroute GET /b = <html><head><title>B</title></head><body>b</body></html>\n";
    let s = served("ip", &format!("moves_in_place()\n{}", pages), &[]);
    let p = s.page("/");
    assert!(p.contains("/_quillo/quillo.js") && p.contains("if(1||d.querySelector"), "{}", p);
    // The runtime asks for pages ahead itself: no prefetch script.
    assert!(p.contains("w.pf=1;") && !p.contains("speculationrules"), "{}", p);
    let js = s.page("/_quillo/quillo.js");
    assert!(js.contains("W.inPlace()"), "{}", &js[js.len().saturating_sub(400)..]);
    // Without it, a page with no component carries no bundle, and the runtime none of it.
    let plain = served("plain", "component C() {\n    state n = 0\n    <button onclick={() => { n = n + 1 }}>{n}</button>\n}\nroute GET /c = <html><body><C/></body></html>\n".to_string().as_str(), &[]);
    let plain_p = s.page("/b");
    assert!(plain_p.contains("quillo.js"), "{}", plain_p);
    let c = plain.page("/c");
    assert!(!c.contains("1||"), "{}", c);
    let js = plain.page("/_quillo/quillo.js");
    assert!(!js.contains("inPlace") && !js.contains("ipPlan"), "the runtime of a program without moves in place has none of their code");
    let (ok, out) = quillo_on("check", "moves_in_place(1)\n");
    assert!(!ok, "{}", out);
}

// ---- in Chrome ------------------------------------------------------------------------------------

fn chrome() -> Option<PathBuf> {
    let node = Command::new("node").args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"]).output();
    if !node.map(|o| o.status.success()).unwrap_or(false) {
        return None;
    }
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium", "/usr/bin/google-chrome", "/usr/bin/chromium"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// `body` run in a page at `/_t` beside `src`'s routes, with `open(path)`: the program's page at
/// `path` in an iframe, its window (`w`) and document (`d`) and `tick`; `wait(f)` until f() holds.
/// A full load in the iframe leaves the test's page as it is. What `body` returns is the result.
fn in_chrome(name: &str, src: &str, body: &str) -> Option<(serde_json::Value, String)> {
    in_chrome_files(name, src, &[], body)
}

fn in_chrome_files(name: &str, src: &str, files: &[(&str, &str)], body: &str) -> Option<(serde_json::Value, String)> {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return None;
    };
    let src = format!("{}\nroute GET /_t = <html><head><title>t</title></head><body><p>t</p></body></html>\n", src);
    let s = served_files(name, &src, &[], files);
    let script = format!(
        r#"(async () => {{
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const wait = async (f, ms = 5000) => {{ const end = Date.now() + ms; while (Date.now() < end) {{ try {{ if (f()) return true; }} catch (_) {{}} await tick(25); }} return false; }};
  let f = null;
  const open = async (path) => {{
    f = document.createElement("iframe");
    f.style.width = "800px"; f.style.height = "600px";
    document.body.appendChild(f);
    f.src = path;
    await new Promise((r) => f.addEventListener("load", r, {{ once: true }}));
    await wait(() => f.contentWindow[Symbol.for("quillo")] && f.contentWindow[Symbol.for("quillo")].booted);
    return [f.contentWindow, f.contentDocument];
  }};
  const doc = () => f.contentDocument;
  const QW = () => f.contentWindow[Symbol.for("quillo")] || {{}};
  let out;
  try {{ out = await (async () => {{
{body}
  }})(); }} catch (e) {{ out = {{ exception: String((e && e.stack) || e) }}; }}
  out.errors = f && f.contentWindow.__quilloErrors;
  document.body.dataset.result = JSON.stringify(out);
}})();"#
    );
    let t = s.dir.0.join("t.js");
    std::fs::write(&t, script).unwrap();
    let run = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/run.js");
    let out = Command::new("node")
        .env_remove("QUILLO_PAGE_FOCUS")
        .env_remove("QUILLO_PAGE_CDP")
        .arg(&run)
        .arg(&chrome)
        .arg(format!("http://127.0.0.1:{}/_t", s.port))
        .arg(s.dir.0.join("profile"))
        .arg("60000")
        .arg(&t)
        .output()
        .expect("node");
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stderr), s.log());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)));
    assert!(v.get("exception").is_none(), "{}\n{}", v, s.log());
    Some((v, s.log()))
}

const SITE: &str = r##"
moves_in_place()
state visits = 0
component Counter() {
    state n = 0
    <button id="count" onclick={() => { n = n + 1 }}>{n}</button>
}
component Visits() {
    <b id="visits" onclick={() => { visits = visits + 1 }}>{visits}</b>
}
component Fresh(tag: str) {
    state made = tag
    <i id="fresh">{made}</i>
}
view Layout(title: str, children) {
    page_head({ title: `${title} · Site`, description: `About ${title}` })
    return <html lang="en">
        <head><meta charset="utf-8"/><title>x</title></head>
        <body>
            <nav><a id="home" href="/">Home</a> <a id="long" href="/long">Long</a> <a id="about" href="/about">About</a> <a id="post" href="/posts/5">Post</a> <a id="reload" href="/about" data-reload>Reload</a> <a id="code" href="/code">Code</a> <a id="file" href="/report.pdf">File</a> <a id="away" href="/gone">Gone</a><Counter/><Visits/></nav>
            {children}
        </body>
    </html>
}
route GET / = <Layout title="Home"><main><h1>Home</h1><Fresh tag="home"/></main></Layout>
route GET /long = <Layout title="Long"><main><h1>Long</h1>{for i in range(0, 200) => <p>line {i}</p>}<a id="to-about" href="/about">About</a></main></Layout>
route GET /about = <Layout title="About"><main><h1>About</h1><Fresh tag="about"/></main></Layout>
route GET /posts/:id(int) = <Layout title={`Post ${id}`}><article><h2>Post {id}</h2><a id="spot" href="#end">end</a><p id="end">end</p></article></Layout>
route GET /code = <Layout title="Code"><main><h1>Code</h1></main><script>{raw("window.ran = 1")}</script></Layout>
route GET /gone = redirect("/about")
"##;

/// A link moves in place: no load (the window's mark stays), the head the new page's, the
/// counter on both pages kept at 2 and the page's state at 1, the component on the new page only
/// starting from the server's state; the focus at the new page's `<main>`, its title said.
#[test]
fn a_link_moves_in_place_keeping_what_is_on_both_pages() {
    let Some((v, log)) = in_chrome(
        "move",
        SITE,
        r#"
  const [w, d] = await open("/");
  w.mark = 1;
  d.getElementById("count").click(); await tick(30); d.getElementById("count").click();
  d.getElementById("visits").click();
  await wait(() => d.getElementById("visits").textContent === "1");
  const count = d.getElementById("count");
  d.getElementById("about").click();
  await wait(() => w.document.title === "About · Site");
  await wait(() => w.document.querySelector("[data-quillo-said]") && w.document.querySelector("[data-quillo-said]").textContent !== "");
  const r = {
    mark: w.mark, path: w.location.pathname, title: w.document.title,
    desc: w.document.querySelector('meta[name="description"]').content,
    og: w.document.querySelector('meta[property="og:title"]').content,
    count: w.document.getElementById("count").textContent, same: w.document.getElementById("count") === count,
    visits: w.document.getElementById("visits").textContent, fresh: w.document.getElementById("fresh").textContent,
    h1: w.document.querySelector("h1").textContent, focus: w.document.activeElement.localName,
    said: w.document.querySelector("[data-quillo-said]").textContent,
    titles: w.document.querySelectorAll("title").length, ran: w.ran,
  };
  // A redirect followed: the address is where it ended.
  w.document.getElementById("away").click();
  await wait(() => w.location.pathname === "/about" && w.history.length >= 3);
  // The new page's component takes clicks.
  w.document.getElementById("count").click();
  await wait(() => w.document.getElementById("count").textContent === "3");
  r.after = w.document.getElementById("count").textContent;
  r.mark2 = w.mark;
  return r;
"#,
    ) else {
        return;
    };
    assert_eq!(v["mark"], 1, "{}", v);
    assert_eq!(v["path"], "/about");
    assert_eq!(v["title"], "About · Site");
    assert_eq!(v["desc"], "About About");
    assert_eq!(v["og"], "About · Site");
    assert_eq!(v["count"], "2", "{}", v);
    assert_eq!(v["same"], true);
    assert_eq!(v["visits"], "1", "{}", v);
    assert_eq!(v["fresh"], "about");
    assert_eq!(v["h1"], "About");
    assert_eq!(v["focus"], "main");
    assert_eq!(v["said"], "About · Site");
    assert_eq!(v["titles"], 1);
    assert_eq!(v["after"], "3");
    assert_eq!(v["mark2"], 1);
    assert_eq!(v["errors"], serde_json::json!([]), "{}", v);
    assert!(log.contains("GET /gone"), "{}", log);
}

/// A full load where in place is not safe or not asked for: `data-reload`, a page with a script of
/// its own in its body, a file; Back after one still works.
#[test]
fn a_move_that_cannot_be_in_place_is_a_page_load() {
    let Some((v, _)) = in_chrome(
        "full",
        SITE,
        r#"
  let [w, d] = await open("/");
  const r = {};
  w.mark = 1;
  d.getElementById("reload").click();
  await wait(() => f.contentWindow.mark === undefined && f.contentDocument.title === "About · Site" && QW().booted);
  r.reload = f.contentWindow.mark === undefined;
  w = f.contentWindow; w.mark = 2;
  w.document.getElementById("code").click();
  await wait(() => f.contentWindow.mark === undefined && f.contentWindow.ran === 1);
  r.code = f.contentWindow.ran;
  w = f.contentWindow; w.mark = 3;
  // A click with a modifier key is the browser's own.
  const e = new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true });
  w.document.getElementById("home").dispatchEvent(e);
  r.modifier = !e.defaultPrevented || null;
  return r;
"#,
    ) else {
        return;
    };
    assert_eq!(v["reload"], true, "{}", v);
    assert_eq!(v["code"], 1, "{}", v);
    assert_eq!(v["modifier"], true, "{}", v);
}

/// Back after a move: the page before, in place, at the scroll it had; Forward again; a spot on
/// the page by a link is the browser's; `navigate` moves in place too, with `replace` adding no
/// entry and `scroll: false` leaving the scroll.
#[test]
fn back_gives_the_page_and_its_scroll_back_and_navigate_moves_in_place() {
    let src = format!(
        "{}\ncomponent Go() {{\n    <span><button id=\"go\" onclick={{() => navigate(\"/long\", {{ replace: true, scroll: false }})}}>go</button><button id=\"go2\" onclick={{() => navigate(\"/about\")}}>go2</button></span>\n}}\nroute GET /nav = <Layout title=\"Nav\"><main><h1>Nav</h1>{{for i in range(0, 200) => <p>line {{i}}</p>}}<Go/></main></Layout>\n",
        SITE
    );
    let Some((v, _)) = in_chrome(
        "back",
        &src,
        r#"
  const [w, d] = await open("/long");
  w.mark = 1;
  w.scrollTo(0, 1500);
  await tick(300);
  w.document.getElementById("to-about").click();
  await wait(() => w.document.title === "About · Site");
  const r = { top: w.scrollY };
  w.history.back();
  await wait(() => w.document.title === "Long · Site" && Math.abs(w.scrollY - 1500) < 2);
  r.back = w.scrollY; r.backPath = w.location.pathname; r.mark = w.mark;
  w.history.forward();
  await wait(() => w.document.title === "About · Site");
  r.forward = w.location.pathname;
  // A spot on the page.
  w.document.getElementById("post").click();
  await wait(() => w.document.title === "Post 5 · Site");
  const len = w.history.length;
  w.document.getElementById("spot").click();
  await tick(200);
  r.spot = w.location.hash; r.spotTitle = w.document.title;
  // navigate with replace, scroll false.
  w.document.getElementById("home").click();
  await wait(() => w.document.title === "Home · Site");
  w.location.href;
  return r;
"#,
    ) else {
        return;
    };
    assert_eq!(v["top"], 0, "{}", v);
    assert_eq!(v["back"], 1500, "{}", v);
    assert_eq!(v["backPath"], "/long");
    assert_eq!(v["mark"], 1);
    assert_eq!(v["forward"], "/about");
    assert_eq!(v["spot"], "#end");
    assert_eq!(v["spotTitle"], "Post 5 · Site");
}

#[test]
fn navigate_in_place_replaces_and_keeps_the_scroll_when_asked() {
    let src = format!(
        "{}\ncomponent Go() {{\n    <span><button id=\"go\" onclick={{() => navigate(\"/long\", {{ replace: true, scroll: false }})}}>go</button><button id=\"go2\" onclick={{() => navigate(\"/about\")}}>go2</button></span>\n}}\nroute GET /nav = <Layout title=\"Nav\"><main><h1>Nav</h1>{{for i in range(0, 200) => <p>line {{i}}</p>}}<Go/></main></Layout>\n",
        SITE
    );
    let Some((v, _)) = in_chrome(
        "navip",
        &src,
        r#"
  const [w, d] = await open("/nav");
  w.mark = 1;
  const len = w.history.length;
  w.scrollTo(0, 800);
  await tick(200);
  w.document.getElementById("go").click();
  await wait(() => w.document.title === "Long · Site");
  const r = { path: w.location.pathname, same: w.history.length === len, scroll: w.scrollY, mark: w.mark };
  // And without options, a new entry, at the top.
  w.document.getElementById("to-about").click();
  await wait(() => w.document.title === "About · Site");
  r.pushed = w.history.length === len + 1; r.top = w.scrollY;
  return r;
"#,
    ) else {
        return;
    };
    assert_eq!(v["path"], "/long", "{}", v);
    assert_eq!(v["same"], true, "{}", v);
    assert_eq!(v["mark"], 1, "{}", v);
    assert!(v["scroll"].as_f64().unwrap() > 0.0, "{}", v);
    assert_eq!(v["pushed"], true, "{}", v);
    assert_eq!(v["top"], 0, "{}", v);
}

/// A link pressed before its page came: the click waits for that request and asks no second time.
#[test]
fn a_link_pressed_before_its_page_comes_asks_once() {
    let src = format!("{}\nroute GET /slow {{\n    sleep(300)\n    return <Layout title=\"Slow\"><main><h1>Slow</h1><a id=\"s\" href=\"/slow2\">s</a></main></Layout>\n}}\nroute GET /slow2 {{\n    sleep(300)\n    return <Layout title=\"Slow2\"><main><h1>Slow2</h1></main></Layout>\n}}\n", SITE.replace("<a id=\"home\"", "<a id=\"slow\" href=\"/slow\">s</a> <a id=\"home\""));
    let Some((v, log)) = in_chrome(
        "once",
        &src,
        r#"
  const [w, d] = await open("/");
  const a = d.getElementById("slow");
  a.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  await tick(20);
  a.click();
  await wait(() => w.document.title === "Slow · Site");
  return { title: w.document.title };
"#,
    ) else {
        return;
    };
    assert_eq!(v["title"], "Slow · Site", "{}", v);
    assert_eq!(log.matches("GET /slow ").count() + log.matches("GET /slow\n").count(), 1, "{}", log);
}

// ---- a single-page app without moves in place: the head follows, Back's scroll ------------------

const SPA: &str = r#"
server fn list() -> list {
    sleep(150)
    return range(0, 120)
}
component App() {
    let path = url().path
    page_head({ title: path == "/" ? "Home · Shop" : `Item ${split(path, "/")[2]} · Shop`, description: `At ${path}` })
    <div>
        <nav><a id="home" href="/">Home</a> <a id="i1" href="/items/1">One</a> <input id="search" oninput={(e) => navigate(`/items/${e.target.value}`, { replace: true })}/></nav>
        {path == "/" ? <List/> : <main><h1 id="item">{path}</h1><div style="height: 3000px">tall</div><button id="q" onclick={() => navigate("?page=2", { replace: true, scroll: false })}>q</button></main>}
    </div>
}
component List() {
    state items = list()
    {items == null ? <p>Loading</p> : <ul>{for i in items => <li key={i}><a id={`l${i}`} href={`/items/${i}`}>Item {i}</a></li>}</ul>}
}
route GET /*rest = <html><head><title>Shop</title></head><body><App/></body></html>
"#;

#[test]
fn a_spas_title_follows_its_page_and_back_gives_the_scroll_back() {
    let Some((v, _)) = in_chrome(
        "spa",
        SPA,
        r#"
  const [w, d] = await open("/");
  const r = { first: w.document.title };
  w.scrollTo(0, 1200);
  await tick(300);
  // Pressed, then clicked while its page is on its way: the move waits for that page (Q-1330).
  w.document.getElementById("l60").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  w.document.getElementById("l60").click();
  await wait(() => w.document.title === "Item 60 · Shop");
  r.title = w.document.title;
  r.desc = w.document.querySelector('meta[name="description"]').content;
  await wait(() => w.document.activeElement && w.document.activeElement.localName === "main");
  r.focus = w.document.activeElement.localName;
  await wait(() => w.document.querySelector("[data-quillo-said]") && w.document.querySelector("[data-quillo-said]").textContent === "Item 60 · Shop");
  r.said = w.document.querySelector("[data-quillo-said]").textContent;
  const len = w.history.length;
  w.scrollTo(0, 50);
  w.document.getElementById("q").click();
  await tick(100);
  r.query = w.location.search; r.qsame = w.history.length === len; r.qscroll = w.scrollY;
  // Back and Forward leave no listeners behind once the scroll is back.
  let on = 0, off = 0;
  const add = w.addEventListener.bind(w), rm = w.removeEventListener.bind(w);
  w.addEventListener = (t, f, o) => { if (t === "wheel") on++; return add(t, f, o); };
  w.removeEventListener = (t, f, o) => { if (t === "wheel") off++; return rm(t, f, o); };
  w.history.back();
  await wait(() => w.document.title === "Home · Shop" && Math.abs(w.scrollY - 1200) < 2);
  r.back = w.scrollY;
  await tick(100);
  // A restore that another move takes the place of goes too.
  w.history.forward();
  await tick(10);
  w.history.back();
  await wait(() => w.document.title === "Home · Shop" && Math.abs(w.scrollY - 1200) < 2);
  await tick(200);
  r.listeners = on - off;
  // A search box that moves the page on each key keeps the focus.
  const box = w.document.getElementById("search");
  box.focus();
  for (const v of ["7", "77"]) { box.value = v; box.dispatchEvent(new Event("input", { bubbles: true })); await tick(150); }
  await wait(() => w.document.title === "Item 77 · Shop");
  await tick(150);
  r.typing = [w.document.activeElement.id, w.document.title];
  return r;
"#,
    ) else {
        return;
    };
    assert_eq!(v["first"], "Home · Shop", "{}", v);
    assert_eq!(v["title"], "Item 60 · Shop", "{}", v);
    assert_eq!(v["desc"], "At /items/60");
    assert_eq!(v["focus"], "main");
    assert_eq!(v["said"], "Item 60 · Shop");
    assert_eq!(v["query"], "?page=2");
    assert_eq!(v["qsame"], true);
    assert!(v["qscroll"].as_f64().unwrap() > 0.0, "{}", v);
    assert_eq!(v["back"], 1200, "{}", v);
    assert_eq!(v["listeners"], 0, "{}", v);
    assert_eq!(v["typing"], serde_json::json!(["search", "Item 77 · Shop"]), "{}", v);
}

const EDGES: &str = r##"
moves_in_place()
let hits = 0
component Titled(t: str) {
    state n = 0
    page_head({ title: n == 0 ? null : `${t} ${n}`, description: n == 0 ? null : "changed", image: n > 1 ? "/card.png" : null })
    <span><button id="t" onclick={() => { n = n + 1 }}>{n}</button><button id="t0" onclick={() => { n = 0 }}>0</button></span>
}
component Bad() {
    state which = ""
    <span>
        <button id="b1" onclick={() => { let r = page_head(json.parse("\"x\"")); which = which + (is_err(r) ? "e" : "o") }}>1</button>
        <button id="b2" onclick={() => { let r = page_head(json.parse("{\"nope\": 1}")); which = which + (is_err(r) ? "e" : "o") }}>2</button>
        <button id="b3" onclick={() => { let r = page_head(json.parse("{\"title\": [1]}")); which = which + (is_err(r) ? "e" : "o") }}>3</button>
        <button id="b4" onclick={() => { let r = page_url(json.parse("\"x\"")); which = which + (is_err(r) ? "e" : "o") }}>4</button>
        <button id="b5" onclick={() => { let r = navigate("/b", json.parse("{\"bad\": true}")); which = which + (is_err(r) ? "e" : "o") }}>5</button>
        <button id="b6" onclick={() => { let r = navigate("/b", json.parse("{\"replace\": 1}")); which = which + (is_err(r) ? "e" : "o") }}>6</button>
        <button id="b7" onclick={() => { let r = navigate("/b", json.parse("\"x\"")); which = which + (is_err(r) ? "e" : "o") }}>7</button>
        <button id="b8" onclick={() => { which = `${page_url()}|${page_url("/x")}|${page_url("https://a.example/")}` }}>8</button>
        <button id="hash" onclick={() => navigate("#spot")}>h</button>
        <button id="hash2" onclick={() => navigate("/a#spot")}>h2</button>
        <i id="which">{which}</i>
    </span>
}
route GET /a = <html><head><title>A</title><link rel="stylesheet" href="/s.css"/></head><body class="a" data-x="1"><main><h1>A</h1><Titled t="T"/><Bad/><a id="tob" href="/b">b</a><a id="tow" href="/w">w</a><a id="toj" href="/j">j</a><a id="tohs" href="/hs">hs</a><p id="spot">spot</p></main></body></html>
route GET /s.css = respond(200, "p { color: rgb(1, 2, 3) }", { "content-type": "text/css" })
route GET /b = <html><head><title>B</title></head><body><p id="spot">b</p><a id="toa" href="/a">a</a><a id="here" href="#spot">here</a><input id="in"/></body></html>
route GET /w {
    hits = hits + 1
    return <html><head><title>W {hits}</title></head><body><a id="toa2" href="/a">a</a></body></html>
}
route GET /j = { ok: true }
route GET /hs = <html><head><title>HS</title><script src="/own.js"></script></head><body>hs</body></html>
"##;

/// The edges of a move in place and of `page_head` in the browser: a head a component sets, takes
/// back (null) and that goes when the component leaves; errors as the server says them; a style
/// sheet only one page has, waited for and taken away; the body's own attributes; a page that
/// writes, which a prefetch refuses, asked for again as a move; an answer that is no page, a head
/// with a script of the program's own and a file, loaded; a spot on the page, by `navigate` and by
/// Back; the focus let go on a page with no `<main>` or heading.
#[test]
fn the_edges_of_moves_in_place_and_of_the_browsers_page_head() {
    let Some((v, log)) = in_chrome(
        "edges",
        EDGES,
        r#"
  let [w, d] = await open("/a");
  const r = {};
  const $ = (id) => f.contentDocument.getElementById(id);
  const click = async (id) => { $(id).click(); await tick(60); };
  await click("t");
  r.t1 = [w.document.title, w.document.querySelector('meta[name="description"]').content, w.document.querySelector('meta[property="og:image"]')];
  await click("t");
  r.img = w.document.querySelector('meta[property="og:image"]').content;
  await click("t0");
  r.back0 = [w.document.title, w.document.querySelector('meta[name="description"]'), w.document.querySelector('meta[property="og:image"]')];
  for (const b of ["b1", "b2", "b3", "b4", "b5", "b6", "b7"]) await click(b);
  r.errs = (w.__quilloErrors || []).map(String);
  await click("b8");
  r.which = $("which").textContent;
  r.sheet = getComputedStyle(w.document.querySelector("p")).color;
  await click("t");
  w.mark = 1;
  await click("hash");
  r.hash = [w.location.hash, w.document.title];
  await click("hash2");
  r.hash2 = w.location.hash;
  $("tob").click();
  await wait(() => w.document.title === "B");
  r.b = { mark: w.mark, cls: w.document.body.className, x: w.document.body.dataset.x || null, sheets: w.document.querySelectorAll('link[rel="stylesheet"][href="/s.css"]').length, title: w.document.title };
  await wait(() => w.document.activeElement === w.document.body);
  r.blur = w.document.activeElement.localName;
  await click("here");
  r.here = w.location.hash;
  w.history.back();
  await tick(200);
  r.hereBack = [w.location.pathname, w.location.hash, w.mark];
  $("toa").click();
  await wait(() => w.document.title === "A");
  r.sheetAgain = getComputedStyle(w.document.querySelector("p")).color;
  // A page that writes: its prefetch refused, then asked for as the move.
  $("tow").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  await tick(300);
  $("tow").click();
  await wait(() => /^W /.test(f.contentDocument.title) && QW().booted);
  r.w = [f.contentDocument.title, f.contentWindow.mark === undefined];
  f.contentWindow.mark = 7;
  $("toa2").click();
  await wait(() => w.document.title === "A");
  $("toj").click();
  await wait(() => f.contentWindow.mark === undefined);
  r.j = f.contentDocument.body.textContent;
  [w, d] = await open("/a");
  w.mark = 5;
  $("tohs").click();
  await wait(() => f.contentWindow.mark === undefined);
  await tick(200);
  r.hs = f.contentWindow.location.pathname;
  return r;
"#,
    ) else {
        return;
    };
    assert_eq!(v["t1"], serde_json::json!(["T 1", "changed", null]), "{}", v);
    assert_eq!(v["img"], format!("{}/card.png", v["which"].as_str().unwrap().split('|').next().unwrap().trim_end_matches("/a")), "{}", v);
    assert_eq!(v["back0"], serde_json::json!(["A", null, null]), "{}", v);
    // Each refused in the words the server has.
    let errors = v["errs"].to_string();
    for said in ["`page_head` takes a map", "has no key `nope`", "is text or null", "`page_url` takes a path that starts with /", "has no option `bad`", "is true or false", "options are a map"] {
        assert!(errors.contains(said), "{}: {}", said, errors);
    }
    let which = v["which"].as_str().unwrap();
    assert!(which.ends_with("/a|") == false && which.contains("/a|http://127.0.0.1:") && which.ends_with("/x|https://a.example/"), "{}", which);
    assert_eq!(v["sheet"], "rgb(1, 2, 3)");
    assert_eq!(v["hash"], serde_json::json!(["#spot", "T 1"]), "{}", v);
    assert_eq!(v["hash2"], "#spot");
    assert_eq!(v["b"], serde_json::json!({ "mark": 1, "cls": "", "x": null, "sheets": 0, "title": "B" }), "{}", v);
    assert_eq!(v["blur"], "body");
    assert_eq!(v["here"], "#spot");
    assert_eq!(v["hereBack"], serde_json::json!(["/b", "", 1]), "{}", v);
    assert_eq!(v["sheetAgain"], "rgb(1, 2, 3)");
    // Refused as a move (it writes), then loaded: its one run that counts.
    assert_eq!(v["w"], serde_json::json!(["W 1", true]), "{}", v);
    assert_eq!(v["j"], "{\"ok\":true}", "{}", v);
    assert_eq!(v["hs"], "/hs", "{}", v);
    // The page that writes: refused as the move asked for it, then made once, as it loaded.
    assert!(log.contains("GET /w -> 503"), "{}", log);
    assert_eq!(log.matches("GET /w -> 200").count(), 1, "{}", log);
}

// ---- the review of slice H ----------------------------------------------------------------------

/// The key a page that moves in place carries (`w.ik`), or none.
fn key_of(page: &str) -> Option<String> {
    let at = page.find("w.ik=\"")? + 6;
    Some(page[at..at + page[at..].find('"')?].to_string())
}

const KEYED: &str = r#"
moves_in_place()
site_headers("/admin", { "referrer-policy": "no-referrer" })
mount /blog = "blog.qwe"
route GET / = <html><head><title>A</title></head><body><a id="tob" href="/b">b</a><a id="admin" href="/admin">admin</a><a id="blog" href="/blog/x">blog</a><a id="in" href="/in">in</a></body></html>
route GET /b = <html><head><title>B</title></head><body><a id="toa" href="/">a</a></body></html>
route GET /admin = <html><head><title>Admin</title></head><body>admin</body></html>
route GET /in {
    set_session({ user: "ann" })
    return <html><head><title>In</title></head><body>in</body></html>
}
"#;

const BLOG: &str = r#"
moves_in_place()
component Post() {
    state n = 0
    <button id="post" onclick={() => { n = n + 1 }}>{n}</button>
}
route GET /x = <html><head><title>Blog</title></head><body><Post/></body></html>
"#;

/// Two pages of one program, for one visitor, with the same policies, share a key; another
/// policy, a session, a mounted program each make another (review of slice H, 1 to 3).
#[test]
fn a_pages_key_names_its_program_its_policies_and_its_visitor() {
    let s = served_files("keys", KEYED, &[], &[("blog.qwe", BLOG)]);
    let a = key_of(&s.page("/")).expect("a key");
    assert_eq!(key_of(&s.page("/b")).as_deref(), Some(a.as_str()));
    assert_ne!(key_of(&s.page("/admin")).unwrap(), a);
    assert_ne!(key_of(&s.page("/blog/x")).unwrap(), a);
    // Signed in: the page that sets the session, and one asked for with it, have another key.
    let signed = s.get("/in");
    let k_in = key_of(&signed.body).unwrap();
    assert_ne!(k_in, a);
    let cookie = signed.headers.iter().find(|(k, _)| k == "set-cookie").map(|(_, v)| v.split(';').next().unwrap().to_string()).unwrap();
    let with = s.get_with("/", &[("cookie", cookie.as_str())]).body;
    assert_eq!(key_of(&with).as_deref(), Some(k_in.as_str()), "{}", cookie);
    assert!(!with.contains(cookie.split('=').nth(1).unwrap()), "the session's id is in the page");
}

/// In Chrome: a mounted program's page, a page with another policy and one after a sign-in are
/// loaded, not swapped in; one of the same program, policy and visitor is.
#[test]
fn a_page_of_another_program_policy_or_visitor_is_loaded() {
    let Some((v, _)) = in_chrome_files(
        "keyed",
        KEYED,
        &[("blog.qwe", BLOG)],
        r#"
  const r = {};
  const go = async (id, title) => {
    f.contentWindow.mark = 1;
    f.contentDocument.getElementById(id).click();
    await wait(() => f.contentDocument.title === title && QW().booted);
    await tick(100);
    return f.contentWindow.mark === 1 ? "in place" : "loaded";
  };
  await open("/");
  r.b = await go("tob", "B");
  f.contentDocument.getElementById("toa").click();
  await wait(() => f.contentDocument.title === "A");
  r.admin = await go("admin", "Admin");
  await open("/");
  r.blog = await go("blog", "Blog");
  await open("/");
  r.in = await go("in", "In");
  return r;
"#,
    ) else {
        return;
    };
    assert_eq!(v["b"], "in place", "{}", v);
    assert_eq!(v["admin"], "loaded", "{}", v);
    assert_eq!(v["blog"], "loaded", "{}", v);
    assert_eq!(v["in"], "loaded", "{}", v);
}

/// An outer component's own title, said again as it runs, does not take the place of the inner
/// page's, which the server wrote (review of slice H, 4); a move another took the place of leaves
/// its style sheet out of the page and its page unshown (6).
#[test]
fn an_outer_title_said_again_gives_way_and_a_move_given_up_leaves_nothing() {
    let src = r#"
moves_in_place()
component Shell(children) {
    state n = 0
    page_head({ title: n == 0 ? "Site" : `Site ${n}` })
    <div><button id="sh" onclick={() => { n = n + 1 }}>{n}</button>{children}</div>
}
component Post() {
    page_head({ title: "Post" })
    <p>post</p>
}
route GET / = <html><head><title>t</title></head><body><Shell><Post/></Shell><a id="toc" href="/c">c</a></body></html>
route GET /slow.css {
    sleep(1500)
    return respond(200, "p { color: red }", { "content-type": "text/css" })
}
route GET /c = <html><head><title>C</title><link rel="stylesheet" href="/slow.css"/></head><body>c</body></html>
"#;
    let Some((v, _)) = in_chrome(
        "outer",
        src,
        r#"
  const [w, d] = await open("/");
  const r = { first: w.document.title };
  d.getElementById("sh").click(); await tick(100); d.getElementById("sh").click(); await tick(100);
  r.after = [w.document.title, d.getElementById("sh").textContent];
  d.getElementById("toc").click();
  await tick(300);
  w.history.back();
  await tick(2500);
  r.gaveUp = [w.location.pathname, w.document.title, w.document.querySelectorAll('link[href="/slow.css"]').length];
  return r;
"#,
    ) else {
        return;
    };
    assert_eq!(v["first"], "Post", "{}", v);
    assert_eq!(v["after"], serde_json::json!(["Post", "2"]), "{}", v);
    assert_eq!(v["gaveUp"], serde_json::json!(["/", "Post", 0]), "{}", v);
}

/// On the server, of two calls at one depth the one inside more components wins, as in the browser
/// (review of slice H, 9); under --watch, `page_url` takes --allowed-hosts' first name rather than
/// the visitor's Host (8); a waiting part's `page_head` that says otherwise than the head is said in
/// the log, and one that says the same is not (11).
#[test]
fn the_servers_tie_the_allowed_host_and_a_late_head() {
    let src = r#"
view V() {
    page_head({ title: "View" })
    return <p>v</p>
}
component C() {
    page_head({ title: "Comp" })
    <p>c</p>
}
server fn later() -> str {
    sleep(50)
    return "came"
}
component Late() {
    state got = defer(later())
    page_head({ title: got == null ? "Waiting" : "Came" })
    <p>{got ?? "…"}</p>
}
component Same() {
    state got = defer(later())
    page_head({ description: "same" })
    <p>{got ?? "…"}</p>
}
route GET /tie = <html><head><title>t</title></head><body><C/><V/></body></html>
route GET /u = <p>{page_url()}</p>
route GET /same = <html><head><title>t</title></head><body><Same/></body></html>
route GET /late = <html><head><title>t</title></head><body><Late/></body></html>
"#;
    let s = served("tie", src, &[]);
    assert!(head_of(&s.page("/tie")).contains("<title>Comp</title>"));
    s.page("/same");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!s.log().contains("waits (defer)"), "{}", s.log());
    let late = s.page("/late");
    assert!(head_of(&late).contains("<title>Waiting</title>"), "{}", late);
    std::thread::sleep(Duration::from_millis(300));
    assert!(s.log().contains("<Late> waits (defer), so its page_head reaches the page's head in the browser only"), "{}", s.log());
    let w = served("watch", src, &["--watch", "--allowed-hosts", "myapp.test,.myapp.test"]);
    let p = w.get_with("/u", &[("host", "shop.myapp.test")]).body;
    assert!(p.contains("<p>http://myapp.test/u</p>"), "{}", p);
}

// ---- the second review of slice H ----------------------------------------------------------------

const ONCE: &str = r##"
moves_in_place()
let n = 0
let e = 0
route GET / = <html><head><title>A</title><script src="/lib.js"></script></head><body><a id="bump" href="/bump">bump</a><a id="export" href="/export">export</a><a id="b" href="/b">b</a><a id="form" href="/form">form</a></body></html>
route GET /lib.js = respond(200, "window.libbed = (window.libbed || 0) + 1", { "content-type": "text/javascript" })
route GET /bump {
    n = n + 1
    set_cookie("seen", "1")
    return <html><head><title>Bump</title></head><body>bumped</body></html>
}
route GET /export {
    e = e + 1
    return respond(200, "data", { "content-type": "text/plain" })
}
route GET /count = { n: n, e: e }
route GET /b = <html lang="fr" data-theme="dark"><head><title>B</title><script src="/lib.js"></script></head><body><p>b</p><a id="fake" href="/fake">fake</a></body></html>
route GET /fake = <html><head><title>Fake</title><script src="/lib.js"></script></head><body><script type="text/plain">{raw(`w.ik="${url().query.k ?? ""}";`)}</script><p>fake</p></body></html>
site_headers("/fake", { "referrer-policy": "no-referrer" })
route GET /form = <html><head><title>Form</title><script src="/lib.js"></script></head><body><form id="f" method="post" action="/posted"><button>go</button></form></body></html>
route POST /posted = <html><head><title>Posted</title><script src="/lib.js"></script></head><body><a id="tob" href="/b">b</a></body></html>
"##;

/// A move never runs a GET's effects twice: one that writes (a counter and a cookie) is refused as
/// the move asks for it and runs once as it loads; an answer that is no page, the same (second
/// review of slice H, 1).
#[test]
fn a_move_that_cannot_be_in_place_runs_its_get_once() {
    let Some((v, log)) = in_chrome(
        "once2",
        ONCE,
        r#"
  await open("/");
  f.contentDocument.getElementById("bump").click();
  await wait(() => f.contentDocument.title === "Bump");
  await tick(300);
  await open("/");
  f.contentDocument.getElementById("export").click();
  await wait(() => f.contentDocument.body && f.contentDocument.body.textContent === "data");
  await tick(300);
  return await (await fetch("/count")).json();
"#,
    ) else {
        return;
    };
    assert_eq!(v["n"], 1, "{}\n{}", v, log);
    assert_eq!(v["e"], 1, "{}\n{}", v, log);
}

/// A library the head was written with and a script added to the head later (analytics) leave
/// moves in place as they are; the new page's own `<html>` attributes come with it; a key written
/// in a page's text is not its key; a page a form's POST made moves nothing (second review of slice
/// H, 2, 3, 7, 8).
#[test]
fn moves_in_place_read_the_servers_scripts_and_key_alone() {
    let Some((v, _)) = in_chrome(
        "scripts",
        ONCE,
        r#"
  let [w, d] = await open("/");
  const r = {};
  const s = d.createElement("script"); s.textContent = "window.analytics = 1"; d.head.appendChild(s);
  w.mark = 1;
  d.getElementById("b").click();
  await wait(() => w.document.title === "B");
  r.b = [w.mark, w.document.documentElement.lang, w.document.documentElement.dataset.theme || null, w.libbed];
  // A page whose text says this page's key, under another policy: loaded.
  w.document.getElementById("fake").href = "/fake?k=" + QW().ik;
  w.document.getElementById("fake").click();
  await wait(() => f.contentDocument.title === "Fake" && QW().booted);
  await tick(200);
  r.fake = f.contentWindow.mark === undefined;
  // A page a POST made: its link loads.
  [w, d] = await open("/form");
  d.getElementById("f").requestSubmit();
  await wait(() => f.contentDocument.title === "Posted" && QW().booted);
  r.postedKey = QW().ik || null;
  f.contentWindow.mark = 2;
  f.contentDocument.getElementById("tob").click();
  await wait(() => f.contentDocument.title === "B" && QW().booted);
  await tick(200);
  r.posted = f.contentWindow.mark === undefined;
  return r;
"#,
    ) else {
        return;
    };
    assert_eq!(v["b"], serde_json::json!([1, "fr", "dark", 1]), "{}", v);
    assert_eq!(v["fake"], true, "{}", v);
    assert_eq!(v["postedKey"], serde_json::Value::Null, "{}", v);
    assert_eq!(v["posted"], true, "{}", v);
}

/// The key holds the visitor's `Authorization` and cookies; a page whose key holds them is
/// `private`, or tells a cache the cookie matters where the program set its own `cache-control`
/// (second review of slice H, 4, 5).
#[test]
fn a_key_with_the_visitor_in_it_is_kept_by_their_browser_alone() {
    let src = r#"
moves_in_place()
route GET / = <html><head><title>A</title></head><body>a</body></html>
route GET /own = respond(200, <html><head><title>Own</title></head><body>own</body></html>, { "cache-control": "max-age=60", "content-type": "text/html; charset=utf-8" })
"#;
    let s = served("private", src, &[]);
    let plain = s.get("/");
    let k0 = key_of(&plain.body).unwrap();
    assert_eq!(plain.header("cache-control"), None, "{:?}", plain.headers);
    let auth = s.get_with("/", &[("authorization", "Bearer abc")]);
    assert_ne!(key_of(&auth.body).unwrap(), k0);
    assert_eq!(auth.header("cache-control"), Some("private"), "{:?}", auth.headers);
    let other = s.get_with("/", &[("authorization", "Bearer abd")]);
    assert_ne!(key_of(&other.body).unwrap(), key_of(&auth.body).unwrap());
    // A cookie no route reads leaves the key as it was (third review); Quillo's session does not.
    let cookie = s.get_with("/", &[("cookie", "theme=dark")]);
    assert_eq!(key_of(&cookie.body).unwrap(), k0);
    assert_eq!(cookie.header("cache-control"), None, "{:?}", cookie.headers);
    let session = s.get_with("/", &[("cookie", "quillo_session=abc")]);
    assert_ne!(key_of(&session.body).unwrap(), k0);
    let own = s.get_with("/own", &[("cookie", "quillo_session=abc")]);
    assert_eq!(own.header("cache-control"), Some("max-age=60"), "{:?}", own.headers);
    assert!(own.headers.iter().any(|(k, v)| k == "vary" && v.contains("Cookie")), "{:?}", own.headers);
}

// ---- the third review of slice H ------------------------------------------------------------------

const MOVES: &str = r#"
moves_in_place()
let glob = 0
let tries = 0
route GET /glob {
    glob = glob + 1
    return <html><head><title>G</title></head><body>{glob}</body></html>
}
route GET /count = { glob: glob, tries: tries }
route writes GET /guess/:c {
    if c != "ok" {
        tries = tries + 1
    }
    return <html><head><title>Guess</title></head><body>{c}</body></html>
}
route GET /a = <html><head><title>A</title><script src="/x.js"></script><script src="/y.js"></script></head><body>a</body></html>
route GET /b = <html><head><title>B</title><script src="/y.js"></script><script src="/x.js"></script></head><body>b</body></html>
route GET /c = <html><head><title>C</title><script src="/x.js"></script><script src="/y.js"></script></head><body>c</body></html>
route GET /theme = <html><head><title>T</title></head><body>{cookie("theme") ?? "none"}</body></html>
"#;

/// A move's own fetch changes nothing whatever --no-prefetch says, is never kept by the browser,
/// and a route that says it writes is refused before it runs, whichever way it would go; a
/// page's key keeps its head's scripts in their order, and holds only Quillo's session cookie
/// unless the route reads cookies itself (third review of slice H, 1, 2, 3, 5, 6).
#[test]
fn a_moves_fetch_is_read_only_and_its_key_holds_what_counts() {
    for opts in [&[][..], &["--no-prefetch"][..]] {
        let s = served("moves", MOVES, opts);
        let mv = [("x-quillo-move", "1")];
        let a = s.get_with("/glob", &mv);
        assert_eq!(a.status, 503, "{:?}: {}", opts, a.body);
        let ok = s.get_with("/guess/ok", &mv);
        assert_eq!(ok.status, 503, "{:?}: a route that writes is refused before it runs", opts);
        assert_eq!(s.get_with("/guess/no", &mv).status, 503);
        let c = s.get("/count").body;
        assert!(c.contains("\"glob\":0") && c.contains("\"tries\":0"), "{:?}: {}", opts, c);
        let page = s.get_with("/a", &mv);
        assert_eq!(page.status, 200, "{:?}", opts);
        assert!(!page.header("cache-control").is_some_and(|v| v.contains("max-age")), "{:?}: {:?}", opts, page.headers);
        // Its key: the head's scripts in order.
        let (ka, kb, kc) = (key_of(&s.page("/a")).unwrap(), key_of(&s.page("/b")).unwrap(), key_of(&s.page("/c")).unwrap());
        assert_eq!(ka, kc);
        assert_ne!(ka, kb);
        // An analytics cookie changes nothing a route reads no cookie of; Quillo's session does,
        // and a route that reads a cookie takes them all.
        let g1 = s.get_with("/a", &[("cookie", "_ga=1")]).body;
        let g2 = s.get_with("/a", &[("cookie", "_ga=2")]).body;
        assert_eq!(key_of(&g1), key_of(&g2));
        assert_eq!(key_of(&g1).unwrap(), ka);
        assert_ne!(key_of(&s.get_with("/a", &[("cookie", "_ga=1; quillo_session=x")]).body).unwrap(), ka);
        let t1 = s.get_with("/theme", &[("cookie", "_ga=1")]).body;
        let t2 = s.get_with("/theme", &[("cookie", "_ga=2")]).body;
        assert_ne!(key_of(&t1), key_of(&t2));
    }
}

const FRESH: &str = r##"
moves_in_place()
let saved = 0
server fn save() -> int {
    saved = saved + 1
    return saved
}
component Saver() {
    state n = 0
    <button id="save" onclick={() => { n = save() }}>{n}</button>
}
route GET / = <html><head><title>Home</title></head><body><Saver/><a id="list" href="/list">list</a><a id="ownq" href="/ownq">ownq</a></body></html>
route GET /list = <html><head><title>List</title></head><body><p id="saved">{saved}</p></body></html>
route GET /ownq = <html><head><title>OwnQ</title></head><body><script>{raw("/* Symbol.for(\"quillo\") */ window.ownq = 1")}</script></body></html>
"##;

/// A page asked for ahead before a server fn saved something is not the one moved to; a script of
/// the program's that names Quillo's key is the program's, and its page loads (third review of
/// slice H, 2, 7).
#[test]
fn a_guess_from_before_a_save_is_dropped_and_a_programs_script_is_its_own() {
    let Some((v, _)) = in_chrome(
        "fresh",
        FRESH,
        r#"
  const [w, d] = await open("/");
  const r = {};
  d.getElementById("list").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  await tick(300);
  d.getElementById("save").click();
  await wait(() => d.getElementById("save").textContent === "1");
  w.mark = 1;
  d.getElementById("list").click();
  await wait(() => w.document.title === "List");
  r.list = [w.document.getElementById("saved").textContent, w.mark];
  w.document.title = "x";
  await open("/");
  f.contentWindow.mark = 2;
  f.contentDocument.getElementById("ownq").click();
  await wait(() => f.contentDocument.title === "OwnQ" && f.contentWindow.ownq === 1);
  r.ownq = [f.contentWindow.mark === undefined, f.contentWindow.ownq];
  return r;
"#,
    ) else {
        return;
    };
    assert_eq!(v["list"], serde_json::json!(["1", 1]), "{}", v);
    assert_eq!(v["ownq"], serde_json::json!([true, 1]), "{}", v);
}

/// A program with offline pages moves in place too: the script that registers Quillo's worker is
/// Quillo's own, said so (fourth review of slice H).
#[test]
fn an_offline_programs_link_moves_in_place() {
    let src = r#"
moves_in_place()
route offline GET / = <html><head><title>Shop</title></head><body><h1>Today</h1><a id="o" href="/orders/1">order</a></body></html>
route offline GET /orders/:id(int) = <html><head><title>Order {id}</title></head><body><h1>Order {id}</h1></body></html>
"#;
    let s = served("offline-page", src, &[]);
    let p = s.page("/");
    assert!(p.contains("<script data-quillo>") && p.contains("serviceWorker"), "{}", p);
    drop(s);
    let Some((v, _)) = in_chrome(
        "offline",
        src,
        r#"
  const [w, d] = await open("/");
  w.mark = 1;
  d.getElementById("o").click();
  await wait(() => w.document.title === "Order 1");
  await tick(200);
  return { mark: w.mark, title: w.document.title };
"#,
    ) else {
        return;
    };
    assert_eq!(v["mark"], 1, "{}", v);
    assert_eq!(v["title"], "Order 1", "{}", v);
}
