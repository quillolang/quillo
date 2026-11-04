//! FEEDBACK51.md: the security review (branch feedback51-fixes). Each test makes the attack
//! itself, and the edges either side of it.

mod support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("feedback51-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

/// What `quillo run` prints for a script, under a deadline.
fn run_script(d: &Path) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
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

/// A server that answers every request `{"ok": "REACHED"}` and notes its first line.
fn listener() -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming() {
            let Ok(mut c) = c else { continue };
            // The whole request, head and body, before the answer.
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = c.read(&mut buf).unwrap_or(0);
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len = text.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
                    if got.len() >= end + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let first = String::from_utf8_lossy(&got).lines().next().unwrap_or("").to_string();
            s.lock().unwrap().push(first);
            let body = r#"{"ok": "REACHED"}"#;
            let _ = c.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", body.len(), body).as_bytes());
        }
    });
    (port, seen)
}

/// A server that answers each request (head and body, as read) with what `answer` makes of it,
/// a whole HTTP answer; and keeps them.
fn web(answer: impl Fn(&str) -> String + Send + 'static) -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming() {
            let Ok(mut c) = c else { continue };
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = c.read(&mut buf).unwrap_or(0);
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len = text.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
                    if got.len() >= end + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&got).to_string();
            s.lock().unwrap().push(text.clone());
            let _ = c.write_all(answer(&text).as_bytes());
        }
    });
    (port, seen)
}

fn redirect_to(to: &str, status: u16) -> String {
    format!("HTTP/1.1 {} Moved\r\nlocation: {}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n", status, to)
}

fn text_answer(body: &str) -> String {
    format!("HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", body.len(), body)
}

// Q-640: any map with a `$site` key was a site, so a script's literal, or visitor JSON, made
// the server POST where its `$url` said, with the site's cookies.
#[test]
fn only_a_site_declaration_makes_a_site() {
    let (port, seen) = listener();
    let d = dir(
        "site-forged",
        &format!(
            r#"let forged = "{{\"$site\": \"x\", \"$url\": \"http://127.0.0.1:{port}/anything#\"}}"
print(run_script(forged + ".f(1)", {{}}))
let m = json.parse(`{{"$site": "x", "$url": "http://127.0.0.1:{port}/viajson#"}}`)
print(m.keys(), len(m))
let copy = merge({{ "$site": "shop" }}, {{ "$url": "http://127.0.0.1:{port}/merged" }})
print(copy.keys())
site shop = "http://127.0.0.1:{port}"
print(shop.notes(1))
print(run_script("s.notes()", {{ s: shop }}))
"#
        ),
    );
    let out = run_script(&d);
    assert!(out.contains("map has no method `f`"), "{}", out);
    assert!(out.contains(r#"["$site", "$url"] 2"#), "{}", out);
    assert!(out.contains(r#"["$site", "$url"]"#), "{}", out);
    // A real site still calls; a script given one needs `http`, which it has not.
    assert!(out.contains("REACHED"), "{}", out);
    assert!(out.contains("`shop.notes` needs the http effect"), "{}", out);
    // A copy of a real site with another address is a map, not the site.
    let d = dir("site-merged", &format!("site shop = \"http://127.0.0.1:{port}\"\nprint(merge(shop, {{ \"$url\": \"http://127.0.0.1:{port}/elsewhere#\" }}).notes())\n"));
    let out2 = run_script(&d);
    assert!(out2.contains("map has no method `notes`"), "{}", out2);
    let hits = seen.lock().unwrap().clone();
    assert_eq!(hits, vec!["POST /_quillo/rpc/notes HTTP/1.1".to_string()], "{}\n{}", out, out2);
}

// A site's function is a request: `quillo check` says so of a function `with` no `http`, by
// the name it is written with (a parameter of that name is not the site).
#[test]
fn a_sites_function_needs_http() {
    let (port, seen) = listener();
    let src = format!("site shop = \"http://127.0.0.1:{port}\"\nfn stamp() with time = shop.notes(now())\nfn fetch() with http = shop.notes(1)\nfn other(shop) with time = shop.notes(1)\nfn helper() = shop.notes(1)\nfn outer() with time = helper()\nprint(fetch())\nprint(stamp())\n");
    let diags: Vec<String> = quillo::check::check_source(&src, "t.qwe", Path::new(".")).diagnostics.iter().map(|d| format!("{} {}", d.line, d.message)).collect();
    assert_eq!(diags.iter().filter(|d| d.contains("http")).count(), 2, "{:?}", diags);
    assert!(diags.iter().any(|d| d.starts_with("2 ") && d.contains("http")), "{:?}", diags);
    assert!(diags.iter().any(|d| d.starts_with("6 ") && d.contains("http")), "{:?}", diags);
    // `quillo run` checks first: nothing is sent.
    let out = run_script(&dir("site-effect", &src));
    assert!(out.contains(":2: error: `shop.notes` needs the http effect, but `stamp` is declared `with time`"), "{}", out);
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn a_site_name_is_a_name() {
    let d = dir("site-name", "print(__site(\"../../escaped\", \"https://shop.example.com\"))\n");
    let out = run_script(&d);
    assert!(out.contains("`site` takes a name of ASCII letters, digits and `_`, got \"../../escaped\""), "{}", out);
    let d = dir("site-name", "print(__site(\"café\", \"https://shop.example.com\"))\n");
    assert!(run_script(&d).contains("`site` takes a name of ASCII letters"));
    let d = dir("site-name-ok", "print(keys(__site(\"shop_2\", \"https://shop.example.com\")))\n");
    assert!(run_script(&d).contains(r#"["$site", "$url"]"#));
}

// Declared again, a site is the one declared before; and the call itself takes only a site.
#[test]
fn a_site_declared_again_is_the_same_and_the_call_takes_only_sites() {
    use quillo::value::Value;
    let mut it = quillo::Interp::new(&support::temp_dir("feedback51-site-again"));
    let site = |it: &mut quillo::Interp, url: &str| quillo::sites::declare(it, vec![Value::str("again"), Value::str(url)], 1).unwrap_or_else(|_| panic!("not declared"));
    let (a, b, c) = (site(&mut it, "https://a.example.com"), site(&mut it, "https://a.example.com/"), site(&mut it, "https://b.example.com"));
    let ptr = |v: &Value| match v {
        Value::Map(m) => std::sync::Arc::as_ptr(m),
        _ => panic!("not a map"),
    };
    assert_eq!(ptr(&a), ptr(&b));
    assert_ne!(ptr(&a), ptr(&c));
    assert!(quillo::sites::is_site(&a) && quillo::sites::is_site(&c));
    // A map like one, a site with no function's name, or nothing: refused.
    let forged = quillo::eval_str(r#"{ "$site": "again", "$url": "https://a.example.com" }"#).unwrap();
    assert!(!quillo::sites::is_site(&forged));
    for args in [vec![forged, Value::str("f")], vec![a.clone(), Value::Int(1)], vec![]] {
        let Err(quillo::interp::Ctrl::Error(e)) = quillo::sites::call(&mut it, args, 3) else { panic!("called") };
        assert_eq!(e.msg, "a site's function is called on a value a `site` declaration made");
    }
}

// Q-641: `for ["billing"]` was checked on the services' path only: anyone with curl called it
// at /_quillo/rpc.
#[test]
fn a_for_function_takes_no_calls_from_pages() {
    let d = dir(
        "for-rpc",
        "accept_calls { keys: { billing: \"0123456789abcdef0123456789abcdef\" }, shared: false }\nserver fn add_credit(user: int, cents: int) for [\"billing\"] = { user: user, credited: cents }\nserver fn open_fn(n: int) = n + 1\nroute GET / = \"hi\"\n",
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let call = |f: &str, body: &str| support::send(s.port, "POST", &format!("/_quillo/rpc/{}", f), &[("content-type", "application/json")], body);
    let r = call("add_credit", "[1, 1000000]");
    assert_eq!(r.status, 404, "{}", r.body);
    assert!(r.body.contains("it takes calls from billing only"), "{}", r.body);
    assert!(!r.body.contains("credited"), "{}", r.body);
    // A function with no `for` is the pages' as before; one that does not exist is still 404.
    let r = call("open_fn", "[1]");
    assert_eq!((r.status, r.body.as_str()), (200, r#"{"ok":2}"#));
    assert_eq!(call("nothing", "[]").status, 404);
    // The page's script has no `server.add_credit` to call, or to name it.
    let bundle = support::send(s.port, "GET", "/_quillo/quillo.js", &[], "").body;
    assert!(bundle.contains(r#"W.server("open_fn")"#), "{}", bundle.len());
    assert!(!bundle.contains("add_credit"));
}

// Q-646: inside <svg> or <math>, <style> and <script> are markup to the browser, which decodes
// entities there: their raw text let a CSS colour add an <img onerror>. Each `<` in a sheet is
// now CSS's `\3c ` and each `&` in a script's JSON `\u0026`, wherever the element was made.
#[test]
fn style_and_script_in_svg_and_math_are_escaped() {
    let d = dir(
        "foreign",
        r##"view Sheet(c: str) = <style>{css`circle { fill: ${c} }`}</style>
view Icon(children) = <svg>{children}</svg>
route GET /svgstyle = <html><body><svg><style>{css`circle { fill: ${request.query.q} }`}</style><circle r="5"/></svg></body></html>
route GET /svgscript = <html><body><svg><script>{request.query.q}</script></svg></body></html>
route GET /mathstyle = <html><body><math><style>{request.query.q}</style></math></body></html>
route GET /fo = <html><body><svg><g><foreignObject><style>{request.query.q}</style></foreignObject></g></svg><style>{request.query.q}</style></body></html>
route GET /literal = <html><body><svg><style>{"svg > circle { fill: red }"}</style></svg><style>{"a > b { color: red }"}</style></body></html>
route GET /view = <html><body><svg><Sheet c={request.query.q}/></svg><Sheet c={request.query.q}/></body></html>
route GET /deep = <html><body><svg><g><g><style>{request.query.q}</style></g></g><desc><style>{request.query.q}</style></desc></svg></body></html>
route GET /all = {
    let q = request.query.q
    let s = <style>{q}</style>
    <html><head><title>t</title><style>{"#box > i { color: rgb(1, 2, 3) } #box::after { content: \"<&>\" }"}</style></head><body>
        <Icon><style>{q}</style><script>{q}</script></Icon>
        <svg>{s}</svg>
        <math><annotation-xml encoding="text/html"><style>{q}</style></annotation-xml><mi><style>{q}</style></mi><mglyph><style>{q}</style></mglyph></math>
        <math><mtext><style>{q}</style></mtext></math>
        <svg><foreignObject><math><style>{q}</style></math></foreignObject><title><style>{q}</style></title></svg>
        <div id="box"><i>i</i></div>
    </body></html>
}
"##,
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| support::send(s.port, "GET", p, &[], "").body;
    let attack = "red%7D%3Cimg%20src%3Dx%20onerror%3Dalert(1)%3E";
    let b = get(&format!("/svgstyle?q={attack}"));
    // `}` too, as a css`...` value (Q-668).
    assert!(b.contains(r"<svg><style>circle { fill: red\7d \3c img src=x onerror=alert(1)> }</style>"), "{}", b);
    let b = get("/svgscript?q=%26quot%3B%3Balert(2)%3B%2F%2F");
    assert!(b.contains(r#"<svg><script>"\u0026quot;;alert(2);//"</script></svg>"#), "{}", b);
    let b = get("/mathstyle?q=%3Cimg%20src%3Dx%20onerror%3Dalert(3)%3E");
    assert!(b.contains(r"<math><style>\3c img src=x onerror=alert(3)></style></math>"), "{}", b);
    // In HTML too: `</style>` cannot end the element early.
    let b = get("/fo?q=a%3C%2Fstyle%3Eb%20%3E%20c");
    assert!(b.contains(r"<foreignObject><style>a\3c /style>b > c</style></foreignObject></g></svg><style>a\3c /style>b > c</style>"), "{}", b);
    // `>`, CSS's child selector, stays as written, in and out of the svg.
    let b = get("/literal");
    assert!(b.contains("<svg><style>svg > circle { fill: red }</style></svg><style>a > b { color: red }</style>"), "{}", b);
    for _ in 0..2 {
        let b = get("/view?q=blue%3C");
        assert!(b.contains(r"<svg><style>circle { fill: blue\3c  }</style></svg><style>circle { fill: blue\3c  }</style>"), "{}", b);
    }
    let b = get("/deep?q=%3Cb%3E");
    assert!(b.contains(r"<g><g><style>\3c b></style></g></g><desc><style>\3c b></style></desc>"), "{}", b);
    let b = get("/all?q=%3C%2Fstyle%3E%3Cimg%20src%3Dx%20onerror%3D__pwn%3D1%3E");
    assert!(!b.contains("<img"), "{}", b);
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    // Every place a <style> or <script> can stand, in Chrome: nothing runs, no element is made
    // from the text, and the page's own sheet, `>` and `<&>` in it, still applies.
    let script = r#"(async () => {
  await new Promise((r) => setTimeout(r, 300));
  const box = document.getElementById("box");
  document.body.dataset.result = JSON.stringify({
    pwn: window.__pwn ?? null,
    imgs: document.querySelectorAll("img").length,
    color: getComputedStyle(box.querySelector("i")).color,
    after: getComputedStyle(box, "::after").content,
  });
})();"#;
    for q in ["%3C%2Fstyle%3E%3Cimg%20src%3Dx%20onerror%3D__pwn%3D1%3E", "%26lt%3Bimg%20src%3Dx%20onerror%3D__pwn%3D2%26gt%3B", "%26quot%3B%3B__pwn%3D3%3B%2F%2F", "%3Cimg%20src%3Dx%20onerror%3D__pwn%3D4%3E"] {
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/all?q={}", s.port, q), script);
        assert_eq!((r["pwn"].clone(), r["imgs"].as_u64()), (serde_json::Value::Null, Some(0)), "{}: {}", q, r);
        assert_eq!((r["color"].as_str(), r["after"].as_str()), (Some("rgb(1, 2, 3)"), Some("\"<&>\"")), "{}: {}", q, r);
    }
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
    let d = support::temp_dir("feedback51-chrome");
    std::fs::write(d.join("t.js"), script).unwrap();
    let out = Command::new("node").arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(d.join("profile")).arg("30000").arg(d.join("t.js")).output().expect("node");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

// Q-645: with nothing on the page following url(), navigate(next) did location.assign(next):
// a `?next=javascript:...` after a sign-in ran as the page.
#[test]
fn navigate_takes_no_javascript_url() {
    let d = dir(
        "navigate",
        "component Login(next: str) {\n    state said = \"\"\n    <div><button id=\"b\" @click={() => { said = \"clicked\"; navigate(next) }}>Sign in</button><p id=\"s\">{said}</p></div>\n}\nroute GET / = <html><head><title>t</title></head><body><Login next={request.query.next ?? \"/\"}/></body></html>\nroute GET /there = <html><body><p id=\"s\">there</p></body></html>\nroute GET /server = { navigate(request.query.to ?? \"/\"); \"went\" }\nroute GET /away = redirect(request.query.to ?? \"/\")\n",
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    // On the server, navigate and redirect refuse it too.
    assert_eq!(support::send(s.port, "GET", "/server?to=%2Fthere", &[], "").body, "went");
    assert_eq!(support::send(s.port, "GET", "/server?to=JaVaScRiPt%3Aalert(1)", &[], "").status, 500);
    assert_eq!(support::send(s.port, "GET", "/away?to=%2Fthere", &[], "").status, 303);
    assert_eq!(support::send(s.port, "GET", "/away?to=%20javascript%3Aalert(1)", &[], "").status, 500);
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  document.getElementById("b").click();
  await tick(300);
  document.body.dataset.result = JSON.stringify({ pwn: window.__pwn ?? null, at: location.pathname, said: document.getElementById("s").textContent });
})();"#;
    for next in ["javascript%3Awindow.__pwn%3D1%3Bvoid%200", "%20%09JaVaScRiPt%3Awindow.__pwn%3D2", "vbscript%3Amsgbox(1)"] {
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/?next={}", s.port, next), script);
        assert_eq!(r["pwn"], serde_json::Value::Null, "{}: {}", next, r);
        assert_eq!((r["at"].as_str(), r["said"].as_str()), (Some("/"), Some("clicked")), "{}: {}", next, r);
    }
}

// Q-666: a page named the topics it followed, and `acme@db:orders` from tenant evil's host was
// followed: evil learned when acme's orders changed. A tenant follows its own and the shared.
#[test]
fn a_page_follows_no_other_tenants_topics() {
    let d = dir("live-tenants", "tenant_from(\"example.com\")\nroute POST /change = { publish(\"orders\"); \"ok\" }\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let host = |t: &str| format!("{}.example.com", t);
    let change = |t: &str| assert_eq!(support::send(s.port, "POST", "/change", &[("Host", &host(t))], "").body, "ok");
    let seq = |t: &str| {
        let b = support::send(s.port, "GET", "/_quillo/live?since=0&t=x", &[("Host", &host(t))], "").body;
        serde_json::from_str::<serde_json::Value>(&b).unwrap()["seq"].as_u64().unwrap()
    };
    change("acme");
    // Each waits for a change of what it follows.
    let follow = |h: &str, since: u64| {
        let mut c = std::net::TcpStream::connect(("127.0.0.1", s.port)).unwrap();
        write!(c, "GET /_quillo/live?since={}&t=acme@orders,evil@orders,orders HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", since, h).unwrap();
        c
    };
    let since = seq("acme");
    let mut acme = follow(&host("acme"), since);
    let mut evil = follow(&host("evil"), since);
    let mut bare = follow("example.com", since);
    std::thread::sleep(std::time::Duration::from_millis(200));
    change("acme");
    let read = |c: &mut std::net::TcpStream, ms: u64| {
        c.set_read_timeout(Some(std::time::Duration::from_millis(ms))).unwrap();
        let mut out = String::new();
        let _ = c.read_to_string(&mut out);
        out
    };
    assert!(read(&mut acme, 10_000).contains("\"changed\":[\"acme@orders\"]"), "acme was not told");
    assert_eq!(read(&mut evil, 1_500), "", "evil was told of acme's change");
    assert_eq!(read(&mut bare, 500), "", "a page with no tenant was told of acme's change");
    // Its own, it is told of.
    change("evil");
    let b = read(&mut evil, 10_000);
    assert!(b.contains("\"changed\":[\"evil@orders\"]"), "{}", b);
    // Without waiting, on the program's own path (a mounted program's), the same.
    let mut it = quillo::Interp::new(&d);
    it.load_source("tenant_from(\"example.com\")\nroute POST /change = { publish(\"orders\"); \"ok\" }\nroute POST /pipe = { publish(\"inbox:x|evil@orders\"); \"ok\" }\n", "main.qwe").unwrap();
    let mut ask = |method: &str, url: &str, t: &str| {
        let r = it.handle(&quillo::server::HttpRequest { method: method.into(), url: url.into(), headers: vec![("host".into(), host(t))], body: Vec::new(), remote: "127.0.0.1".into() });
        r.text()
    };
    let since: serde_json::Value = serde_json::from_str(&ask("GET", "/_quillo/live?since=0&t=x", "acme")).unwrap();
    ask("POST", "/change", "acme");
    ask("POST", "/change", "evil");
    let url = format!("/_quillo/live?since={}&t=acme@orders,evil@orders", since["seq"]);
    let b = ask("GET", &url, "evil");
    assert!(b.contains("\"changed\":[\"evil@orders\"]"), "{}", b);
    assert!(ask("GET", &url, "acme").contains("\"changed\":[\"acme@orders\"]"));
    // A topic of acme's with a `|` in it is still acme's.
    ask("POST", "/pipe", "acme");
    let url = format!("/_quillo/live?since={}&t=acme@inbox:x|evil@orders", since["seq"]);
    assert!(ask("GET", &url, "acme").contains("\"changed\":[\"acme@inbox:x|evil@orders\"]"));
    let b = ask("GET", &url, "evil");
    assert!(b.contains("\"changed\":[]"), "{}", b);
}

// Q-651: middleware let the route go on for any map: `{ error: "bad api key" }`, the way a
// refusal is written as JSON, gave the secret; and for null, so a guard that ends with no
// value (`if root { return r }`) let in everyone it was written to keep out. The request, or a
// copy of it, goes on; null says the group's routes do not match, and the next route is tried
// (decided 2026-11-04).
#[test]
fn middleware_lets_the_route_go_on_only_with_the_request() {
    let src = r#"fn api_key(r) = r.headers["x-api-key"] == "k3y" ? r : { error: "bad api key" }
fn admins(r) {
    if r.headers["x-user"] == "root" {
        return r
    }
}
fn logs(r) {
    print(r.path)
}
fn copied(r) = { ...r, seen: true }
fn fine(r) = r.headers["x"] == "1" ? r : false
routes /api | api_key { route GET /secret = { data: "SECRET-API" } }
routes /admin | admins {
    route GET /x = "admin"
    route GET /only = "admin only"
    routes /deep | fine { route GET /y = "deep" }
}
route GET /admin/x = "the public page"
routes /log | logs { route GET /x = "log" }
routes /copy | copied { route GET /x = "copy" }
routes /ok | fine { route GET /x = "ok" }
"#;
    let diags: Vec<String> = quillo::check::check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().map(|d| format!("{} {}", d.line, d.message)).collect();
    // A middleware that can only give null leaves its routes never matching: refused.
    assert!(diags.iter().any(|d| d.starts_with("19 ") && d.contains("`logs` gives null")), "{:?}", diags);
    assert_eq!(diags.len(), 1, "{:?}", diags);
    let d = dir("middleware", &src.replace("routes /log | logs { route GET /x = \"log\" }\n", ""));
    let s = support::serve(&d.join("main.qwe"), &[]);
    let send = |m: &str, p: &str, h: &[(&str, &str)]| support::send(s.port, m, p, h, "");
    let get = |p: &str, h: &[(&str, &str)]| send("GET", p, h);
    let r = get("/api/secret", &[]);
    assert_eq!(r.status, 500, "{}", r.body);
    assert!(!r.body.contains("SECRET"), "{}", r.body);
    assert_eq!(get("/api/secret", &[("x-api-key", "k3y")]).body, r#"{"data":"SECRET-API"}"#);
    // Root: the group's routes. Anyone else: the next route of the path, or none.
    let root = [("x-user", "root")];
    assert_eq!(get("/admin/x", &root).body, "admin");
    assert_eq!(get("/admin/x", &[]).body, "the public page");
    assert_eq!(get("/admin/only", &root).body, "admin only");
    let r = get("/admin/only", &[]);
    assert_eq!((r.status, r.body.as_str()), (404, "not found\n"));
    assert_eq!(send("HEAD", "/admin/only", &[]).status, 404);
    // Nothing is said of the group: no 405 and its `Allow` for another method, nor what is
    // inside it, an inner group's routes included.
    let r = send("POST", "/admin/only", &[]);
    assert_eq!(r.status, 404, "{:?}", r.headers);
    assert_eq!(send("POST", "/admin/only", &root).status, 405);
    assert_eq!(get("/admin/deep/y", &[("x", "1")]).status, 404);
    assert_eq!(get("/admin/deep/y", &[("x", "1"), ("x-user", "root")]).body, "deep");
    assert_eq!(get("/admin/deep/y", &root).status, 403);
    assert_eq!(get("/admin/nothing", &[]).status, 404);
    assert_eq!(get("/admin/nothing", &root).status, 404);
    // A copy of the request goes on; `false` is a 403.
    assert_eq!(get("/copy/x", &[]).body, "copy");
    assert_eq!(get("/ok/x", &[("x", "1")]).body, "ok");
    assert_eq!(get("/ok/x", &[]).status, 403);
}

/// `quillo run` with these arguments and this environment, once it listens.
fn run_with(d: &Path, file: &str, args: &[&str], env: &[(&str, &str)]) -> (support::Guard, u16) {
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(args)
        .current_dir(d)
        .env_remove("QUILLO_SERVICE_SECRET")
        .envs(env.iter().copied())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let g = support::Guard::new(child);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !support::listening(port) {
        assert!(std::time::Instant::now() < deadline, "{} did not start", file);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    (g, port)
}

const JWT: &str = "jwt-secret-0123456789abcdef0123456";

/// An HS256 token for `claims`, for audience billing from issuer idp.
fn token(claims: serde_json::Value) -> String {
    let mut c = claims;
    c["iss"] = "idp".into();
    c["aud"] = "billing".into();
    c["exp"] = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 300).into();
    let m = format!("{}.{}", quillo::crypto::b64url(br#"{"alg":"HS256","typ":"JWT"}"#), quillo::crypto::b64url(c.to_string().as_bytes()));
    format!("{}.{}", m, quillo::crypto::b64url(&quillo::crypto::hmac_sha256(JWT.as_bytes(), m.as_bytes())))
}

const BILLING: &str = r#"accept_calls {
    jwt: { secret: env("JWT_KEY"), audience: "billing", issuer: "idp" },
    shared: false,
    check: (caller) => {
        if caller.name == "mallory" { return false }
        if caller.name == "trusty" { return true }
        if caller.name == "texty" { return "yes" }
        if caller.name == "shop" || caller.name == "notify" { caller }
    },
}
let runs = 0
server fn secret_report() for ["shop"] = "TOPSECRET-for-shop-only"
server fn count(n: int) {
    runs += 1
    { runs: runs, n: n }
}
server fn who() = request.caller.name
"#;

// Q-652, Q-664, Q-665: a service's guards. A `check` that gives null accepted the caller; a call
// id another caller had used got that caller's answer; and a visitor's token was a caller's.
#[test]
fn a_services_guards_fail_closed() {
    let d = support::temp_dir("feedback51-billing");
    std::fs::write(d.join("billing.qwe"), BILLING).unwrap();
    let (_b, port) = run_with(&d, "billing.qwe", &["--as-service"], &[("JWT_KEY", JWT)]);
    let call = |f: &str, id: &str, claims: serde_json::Value, body: &str| {
        let auth = format!("Bearer {}", token(claims));
        support::send(port, "POST", &format!("/_quillo/service/{}", f), &[("x-quillo-to", "billing"), ("x-quillo-call", id), ("authorization", &auth)], body)
    };
    let shop = || serde_json::json!({ "client_id": "shop", "sub": "shop" });
    let notify = || serde_json::json!({ "sub": "notify" });
    // Q-652: the check gives null for carol, text for texty: refused; true or a caller, taken.
    let r = call("who", "q6521", serde_json::json!({ "sub": "carol" }), "[]");
    assert_eq!((r.status, r.body.as_str()), (403, r#"{"error":"refused by accept_calls's check"}"#));
    assert_eq!(call("who", "q6522", serde_json::json!({ "sub": "texty" }), "[]").status, 403);
    assert_eq!(call("who", "q6523", serde_json::json!({ "sub": "mallory" }), "[]").status, 403);
    assert_eq!(call("who", "q6524", serde_json::json!({ "sub": "trusty" }), "[]").body, r#"{"ok":"trusty"}"#);
    assert_eq!(call("who", "q6525", shop(), "[]").body, r#"{"ok":"shop"}"#);

    // Q-664: shop's answer, remembered, is shop's alone.
    let id = "aaaa1111";
    assert_eq!(call("secret_report", id, shop(), "[]").body, r#"{"ok":"TOPSECRET-for-shop-only"}"#);
    assert_eq!(call("secret_report", "q664x", notify(), "[]").status, 403);
    let r = call("count", id, notify(), "[1]");
    assert_eq!((r.status, r.body.as_str()), (200, r#"{"ok":{"runs":1,"n":1}}"#));
    // The same call sent again is answered as it was, not run again.
    assert_eq!(call("count", id, notify(), "[1]").body, r#"{"ok":{"runs":1,"n":1}}"#);
    assert_eq!(call("secret_report", id, shop(), "[]").body, r#"{"ok":"TOPSECRET-for-shop-only"}"#);
    // With that id: another caller, another function, other arguments, each a call of its own.
    assert_eq!(call("count", id, shop(), "[1]").body, r#"{"ok":{"runs":2,"n":1}}"#);
    assert_eq!(call("count", id, notify(), "[2]").body, r#"{"ok":{"runs":3,"n":2}}"#);
    let r = call("who", id, notify(), "[]");
    assert_eq!(r.body, r#"{"ok":"notify"}"#);

    // Q-665: a token a user signed in for through the shop is not the shop's.
    let r = call("secret_report", "q6651", serde_json::json!({ "sub": "user-42", "azp": "shop" }), "[]");
    assert_eq!(r.status, 401, "{}", r.body);
    assert!(r.body.contains("the token is one a user signed in for (sub \\\"user-42\\\"), not client \\\"shop\\\"'s own"), "{}", r.body);
    assert_eq!(call("who", "q6652", serde_json::json!({ "sub": "user-42", "client_id": "shop" }), "[]").status, 401);
    // The client's own, as each identity provider writes one.
    for (n, claims) in [
        serde_json::json!({ "sub": "shop@clients", "azp": "shop" }),
        serde_json::json!({ "sub": "abc", "azp": "shop", "gty": "client-credentials" }),
        serde_json::json!({ "sub": "4b7c-uuid", "azp": "shop", "client_id": "shop", "preferred_username": "service-account-shop" }),
        serde_json::json!({ "sub": "oid-1", "azp": "shop", "idtyp": "app" }),
        serde_json::json!({ "sub": "oid-2", "oid": "oid-2", "azp": "shop", "roles": ["Reports.Read"] }),
        serde_json::json!({ "azp": "shop" }),
        serde_json::json!({ "sub": "shop" }),
    ]
    .into_iter()
    .enumerate()
    {
        let r = call("secret_report", &format!("q665own{}", n), claims.clone(), "[]");
        assert_eq!(r.body, r#"{"ok":"TOPSECRET-for-shop-only"}"#, "{}", claims);
    }
    // Not a service account of another name, nor a user who calls themself one, nor an Entra
    // user's token (its `sub` is not its `oid`, and it has scopes).
    for (n, claims) in [
        serde_json::json!({ "sub": "u", "azp": "shop", "preferred_username": "service-account-other" }),
        serde_json::json!({ "sub": "u", "azp": "shop", "preferred_username": "service-account-shop" }),
        serde_json::json!({ "sub": "pairwise", "oid": "oid-3", "azp": "shop", "scp": "Reports.Read" }),
        serde_json::json!({ "sub": "oid-4", "oid": "oid-4", "azp": "shop", "scp": "Reports.Read" }),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(call("who", &format!("q665no{}", n), claims.clone(), "[]").status, 401, "{}", claims);
    }
}

// Q-648: a script's `upper` was the program's `fn upper`, which ran with the program's effects.
// Q-670: a function a script made ran after it with no budget of steps.
#[test]
fn a_script_has_quillos_builtins_and_gives_back_no_function() {
    let d = dir(
        "script-names",
        r#"fn upper(s) {
    write_file("pwned.txt", `reached with ${s}`)
    "done"
}
let len = (x) => "the program's len"
print(run_script(`upper("from-script")`, {}))
print(run_script(`"x".upper()`, {}))
print(run_script(`len("abc")`, {}))
print(upper("mine"))
let f = run_script("(n) => n + 1", {})
print(is_err(f), f)
print(run_script("{ a: [1, (n) => n] }", {}))
print(run_script("f", { f: (x) => x * 2 }))
print(run_script("[1, { a: err(2) }, upper]", {}))
"#,
    );
    let out = run_script(&d);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(&lines[..4], ["FROM-SCRIPT", "X", "3", "done"], "{}", out);
    // Only the program's own call wrote the file.
    assert_eq!(std::fs::read_to_string(d.join("pwned.txt")).unwrap(), "reached with mine");
    let refused = "a script gives back no function (it would run without the script's limits): give back what it computes";
    assert_eq!(lines[4], format!("true err({:?})", refused), "{}", out);
    assert!(lines[5].contains(refused) && lines[6].contains(refused), "{}", out);
    // Values, error values and builtins in them are still given back.
    assert!(lines[7].starts_with("[1, { a: ") && !lines[7].contains(refused), "{}", out);
}

// Q-649 and Q-650: ureq followed redirects itself, past Quillo's checks: secret headers went to
// another port, a `file:` Location panicked, `redirects: N` followed N - 1, and through a proxy
// `public_only` was checked on the first address only.
#[test]
fn quillo_follows_each_redirect_and_checks_it() {
    let (other, other_seen) = web(|req| {
        let get = |h: &str| req.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix(&format!("{}: ", h)).map(str::to_string)).unwrap_or_default();
        text_answer(&format!("other: {} auth={} key={} accept={}", req.lines().next().unwrap_or(""), get("authorization"), get("x-api-key"), get("accept")))
    });
    let (port, _) = web(move |req| {
        let path = req.split(' ').nth(1).unwrap_or("").to_string();
        let get = |h: &str| req.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix(&format!("{}: ", h)).map(str::to_string)).unwrap_or_default();
        match path.as_str() {
            "/to-other-port" => redirect_to(&format!("http://127.0.0.1:{}/echo", other), 302),
            "/post-307" => redirect_to(&format!("http://127.0.0.1:{}/echo", other), 307),
            "/same" | "/hop1" => redirect_to("/echo", 302),
            "/hop2" => redirect_to("/hop1", 301),
            "/file" => redirect_to("file:///etc/passwd", 302),
            _ => text_answer(&format!("same: {} auth={} key={}", req.lines().next().unwrap_or(""), get("authorization"), get("x-api-key"))),
        }
    });
    let d = dir(
        "redirects",
        &format!(
            r#"let at = "http://127.0.0.1:{port}"
let hs = {{ authorization: "Bearer SECRET", "x-api-key": "KEY123", accept: "text/plain" }}
print(http.get(at + "/to-other-port", hs).body)
print(http.post(at + "/to-other-port", "x", hs).body)
print(http.post(at + "/post-307", "the body", hs).body)
print(http.get(at + "/same", hs).body)
let f = http.get(at + "/file")
print(f.status, f.headers["location"])
print(http.get(at + "/hop1", {{}}, {{ redirects: 1 }}).body)
print(http.get(at + "/hop1", {{}}, {{ redirects: 0 }}).status)
let two = http.get(at + "/hop2", {{}}, {{ redirects: 1 }})
print(is_err(two), two)
print(http.get(at + "/hop2", {{}}, {{ redirects: 2 }}).body)
"#
        ),
    );
    let out = run_script(&d);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 9, "{}", out);
    // To another origin: what says what it takes goes on, the rest stays behind.
    assert_eq!(lines[0], "other: GET /echo HTTP/1.1 auth= key= accept=text/plain");
    assert_eq!(lines[1], "other: GET /echo HTTP/1.1 auth= key= accept=text/plain");
    assert_eq!(lines[2], "other: POST /echo HTTP/1.1 auth= key= accept=text/plain");
    assert!(other_seen.lock().unwrap()[2].ends_with("the body"), "a 307 goes on with its body");
    // The same origin keeps them.
    assert_eq!(lines[3], "same: GET /echo HTTP/1.1 auth=bearer secret key=key123");
    // Not http(s): the redirect is the answer.
    assert_eq!(lines[4], "302 file:///etc/passwd");
    assert_eq!(lines[5], "same: GET /echo HTTP/1.1 auth= key=");
    assert_eq!(lines[6], "302");
    assert!(lines[7].starts_with("true ") && lines[7].contains("more than 1 redirects"), "{}", out);
    assert_eq!(lines[8], "same: GET /echo HTTP/1.1 auth= key=");

    // Through a proxy, each hop's address is checked too.
    let (proxy, proxy_seen) = web(|req| {
        if req.starts_with("GET http://93.184.215.14/start ") {
            redirect_to("http://169.254.169.254/latest/meta-data/", 302)
        } else {
            text_answer(&format!("PROXY GOT {}", req.lines().next().unwrap_or("")))
        }
    });
    let d = dir("proxy", "let r = http.get(\"http://93.184.215.14/start\", {}, { public_only: true })\nprint(is_err(r), r)\n");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .env("HTTP_PROXY", format!("http://127.0.0.1:{}", proxy))
        .env("http_proxy", format!("http://127.0.0.1:{}", proxy))
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&child.stdout).into_owned() + &String::from_utf8_lossy(&child.stderr);
    assert!(out.starts_with("true ") && out.contains("169.254.169.254"), "{}", out);
    assert!(!out.contains("PROXY GOT"), "{}", out);
    assert_eq!(proxy_seen.lock().unwrap().len(), 1, "the second address was not asked for");
}

// Q-668: css`...` put `${v}` in as it was, and a style map its keys and values: `red} input
// [value^=a]{...}` added a rule. `{`, `}`, `;` and `\` are CSS's escapes now, and keys names.
#[test]
fn css_values_are_values() {
    let d = dir(
        "css-values",
        r#"route GET /sheet = {
    let st = set({ color: request.query.c, fontSize: 12, "--brand-2": "rgb(1, 2, 3)" }, request.query.k ?? "x", "1")
    <html><body><style>{css`p { color: ${request.query.c} }`}</style><p style={st}>x</p></body></html>
}
"#,
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let b = support::send(s.port, "GET", "/sheet?c=red%7D%20input%5Bvalue%5E%3Da%5D%7Bbackground%3Aurl(%2F%2Fevil%2Fa)%7D&k=color%3Ared%3Bbackground", &[], "").body;
    assert!(b.contains(r"<style>p { color: red\7d  input[value^=a]\7b background:url(//evil/a)\7d  }</style>"), "{}", b);
    // The key that is no name is left out; numbers, parentheses and custom properties stay.
    assert!(b.contains(r#"style="color: red\7d  input[value^=a]\7b background:url(//evil/a)\7d ; font-size: 12px; --brand-2: rgb(1, 2, 3)""#), "{}", b);
    let b = support::send(s.port, "GET", "/sheet?c=rgb(10%2C%2020%2C%2030)", &[], "").body;
    assert!(b.contains("<style>p { color: rgb(10, 20, 30) }</style>"), "{}", b);
    let b = support::send(s.port, "GET", "/sheet?c=a%3Bb%5Cc", &[], "").body;
    assert!(b.contains(r"color: a\3b b\5c c }"), "{}", b);
}

// Q-669: `request.cookies` made a page with no `Cache-Control: private`, as `session()` did not.
#[test]
fn a_page_made_from_the_visitors_cookies_is_private() {
    let d = dir(
        "private",
        "route GET /cookies = <p>{request.cookies.sid ?? \"\"}</p>\nroute GET /header = <p>{request.headers.cookie ?? \"\"}</p>\nroute GET /auth = <p>{request.headers[\"authorization\"] ?? \"\"}</p>\nroute GET /path = <p>{request.path}</p>\nroute GET /mine = {\n    let m = { cookies: 1 }\n    <p>{m.cookies}</p>\n}\n",
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let private = |p: &str| {
        let r = support::send(s.port, "GET", p, &[("cookie", "sid=abc"), ("authorization", "Bearer t")], "");
        r.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("cache-control") && v.contains("private"))
    };
    for p in ["/cookies", "/header", "/auth"] {
        assert!(private(p), "{}", p);
    }
    // What is not the visitor's own: as before.
    assert!(!private("/path"));
    assert!(!private("/mine"));
}

// Q-660: JSON was read into serde's values and then Quillo's, with no limit on how many: a body
// of tiny values took 100 times its size.
#[test]
fn json_has_so_many_values_at_most() {
    let many = format!("[{}0]", "0,".repeat(1_100_000));
    let v = quillo::Value::parse_json(&many);
    assert!(v.is_err_and(|e| e.to_string().contains("more values than JSON of its length may have")));
    // Long values are as many as there may be: one per 8 bytes, or a million.
    let wide = format!("[{}\"x\"]", "\"0123456789abcdef\",".repeat(1_100_000));
    assert!(quillo::Value::parse_json(&wide).is_ok());
    assert!(quillo::Value::parse_json(&format!("[{}0]", "0,".repeat(999_000))).is_ok());
    // Nested ones count too, and the budget is each text's own.
    let nested = format!("[{}0]", "{\"a\":0},".repeat(600_000));
    assert!(quillo::Value::parse_json(&nested).is_err());
    assert!(quillo::Value::parse_json("[1, 2]").is_ok());
}

// Q-661: `==`, `str` and JSON of a value nested from data went down its levels on the stack, and
// a list 300,000 deep stopped the server.
#[test]
fn deep_values_compare_and_show_without_stopping_the_server() {
    let d = dir(
        "deep",
        r#"fn deep(n) {
    let x = []
    for i in range(0, n) {
        x = [x]
    }
    x
}
route GET /eq/:n = {
    let a = deep(int(n))
    let b = deep(int(n))
    `${a == b} ${a == [b]}`
}
route GET /str/:n = `${len(str(deep(int(n))))}`
route GET /json/:n = `${is_err(json.stringify(deep(int(n))))}`
route GET /sort/:n = {
    let a = deep(int(n))
    `${len(sort([a, a]))}`
}
"#,
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| support::send(s.port, "GET", p, &[], "");
    assert_eq!(get("/eq/300000").body, "true false");
    let r = get("/str/50000");
    assert_eq!(r.status, 200);
    // Shown 10,000 deep, then `…`.
    assert_eq!(r.body.parse::<usize>().unwrap(), 10_001 * 2 + 1, "{}", r.body);
    assert_eq!(get("/json/50000").body, "true");
    assert_eq!(get("/str/3").body, "8");
    // Past 10,000 levels two lists are not ordered: an error, not a crash.
    assert_eq!(get("/sort/100").body, "2");
    assert_eq!(get("/sort/20000").status, 500);
    // And the server still answers.
    assert_eq!(get("/eq/2").body, "true false");
}

// Q-675: a new SQLite database was readable by every user of the machine.
#[cfg(unix)]
#[test]
fn a_new_database_is_its_owners_alone() {
    use std::os::unix::fs::PermissionsExt;
    let d = dir("perms", "db.open(\"new.db\")\ndb.exec(\"create table t (x int)\")\nprint(\"made\")\n");
    std::fs::write(d.join("old.db"), "").unwrap();
    std::fs::set_permissions(d.join("old.db"), std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::write(d.join("old.qwe"), "db.open(\"old.db\")\ndb.exec(\"create table t (x int)\")\nprint(\"made\")\n").unwrap();
    assert!(run_script(&d).contains("made"));
    assert_eq!(std::fs::metadata(d.join("new.db")).unwrap().permissions().mode() & 0o777, 0o600);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("old.qwe")).output().unwrap();
    assert!(String::from_utf8_lossy(&o.stdout).contains("made"));
    // One there already keeps its own.
    assert_eq!(std::fs::metadata(d.join("old.db")).unwrap().permissions().mode() & 0o777, 0o644);
}

// Q-654: a chunk's size line ended at CRLF only, whatever came before it: `2;\n` was read as an
// extension, where a proxy that ends the line at the LF reads another request. And an empty
// `Transfer-Encoding` with a length was read by the length.
#[test]
fn chunked_framing_takes_no_control_characters() {
    let d = dir("chunked", "route POST /body = request.body\nroute GET /ip = request.headers.host\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let raw = |bytes: &[u8]| {
        let mut c = std::net::TcpStream::connect(("127.0.0.1", s.port)).unwrap();
        c.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        c.write_all(bytes).unwrap();
        let mut out = String::new();
        let _ = c.read_to_string(&mut out);
        out
    };
    for line in ["2;\n", "2;\r", "2;\0", "2;a\x01b"] {
        let req = format!("POST /body HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked\r\n\r\n{}xx\r\n42\r\n0\r\n\r\nGET /ip HTTP/1.1\r\nHost: smuggled.example\r\n\r\n\r\n0\r\n\r\n", line);
        let out = raw(req.as_bytes());
        assert!(out.starts_with("HTTP/1.1 400"), "{:?}: {}", line, out);
        assert!(!out.contains("smuggled"), "{:?}: {}", line, out);
    }
    // A trailer with a bare LF in it.
    let out = raw(b"POST /body HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\nxx\r\n0\r\nX-T: a\nb\r\n\r\n");
    assert!(out.starts_with("HTTP/1.1 400"), "{}", out);
    for te in [",", "", " , "] {
        let out = raw(format!("POST /body HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: {}\r\nContent-Length: 2\r\nConnection: close\r\n\r\nxx", te).as_bytes());
        assert!(out.starts_with("HTTP/1.1 400"), "{:?}: {}", te, out);
    }
    // What is well made still is: an extension, a tab, a trailer.
    let out = raw(b"POST /body HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2;name=\"v\"\t\r\nxx\r\n0\r\nX-T: ok\r\n\r\n");
    assert!(out.starts_with("HTTP/1.1 200") && out.ends_with("xx"), "{}", out);
}

// Q-656: `files()` hid `lib.qwe` and served `lib.QWE`, the same file on macOS and Windows; and
// `plug.rſ`, folded to `plug.rs`. Q-671: its folder redirect for `//docs` went to host `docs`.
#[test]
fn files_hides_the_programs_files_however_named() {
    let d = dir("files", "route GET /*path = files(\"site\", path)\n");
    std::fs::create_dir_all(d.join("site/docs")).unwrap();
    std::fs::write(d.join("site/lib.qwe"), "let SECRET = \"s3cr3t\"\n").unwrap();
    std::fs::write(d.join("site/plug.rs"), "// secret\n").unwrap();
    std::fs::write(d.join("site/docs/index.html"), "docs").unwrap();
    std::fs::write(d.join("site/Report.QWE.html"), "fine").unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| support::send(s.port, "GET", p, &[], "");
    let case_blind = std::fs::metadata(d.join("site/LIB.QWE")).is_ok();
    for p in ["/lib.qwe", "/lib.QWE", "/lib.Qwe", "/lib.qwe.", "/lib.qwe%20", "/plug.RS", "/plug.r%C5%BF", "/.GIT/config"] {
        let r = get(p);
        assert_eq!(r.status, 404, "{} (a file system blind to case: {})", p, case_blind);
        assert!(!r.body.contains("s3cr3t") && !r.body.contains("secret"), "{}", p);
    }
    // A name that only holds `.qwe` is a file like another.
    assert_eq!(get("/Report.QWE.html").body, "fine");
    // One `/` first, whatever was asked.
    for (asked, to) in [("/docs", "/docs/"), ("//docs", "/docs/"), ("/docs?x=1", "/docs/?x=1")] {
        let r = get(asked);
        assert_eq!((r.status, r.headers.iter().find(|(k, _)| k == "location").map(|(_, v)| v.as_str())), (301, Some(to)), "{}", asked);
    }
}

// Q-657: `file("public/" + path)`, the guide's example and the template's, served `.env` and
// `main.qwe`; `quillo check` now says so, and the template uses `files`.
#[test]
fn check_warns_about_a_file_named_by_the_visitor() {
    let diags = |src: &str| -> Vec<String> { quillo::check::check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().map(|d| d.message.clone()).collect() };
    let warned = |src: &str| diags(src).iter().any(|d| d.contains("`file` serves the one file it is given, whatever its name"));
    assert!(warned("route GET /static/*path = file(\"public/\" + path)\n"));
    assert!(warned("route GET /f/:name = file(`public/${name}`)\n"));
    assert!(warned("route GET /f/:name(str) = file(\"up/\" + name)\n"));
    // A number, a name the program wrote, or `files`: nothing said.
    assert!(!warned("route GET /f/:id(int) = file(`exports/${id}.pdf`)\n"));
    assert!(!warned("let name = \"a.pdf\"\nroute GET /f = file(\"exports/\" + name)\n"));
    assert!(!warned("route GET /static/*path = files(\"public\", path)\n"));
    // The template is clean.
    let t = quillo::check::check_file(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/app/main.qwe")));
    assert!(t.diagnostics.is_empty(), "{:#?}", t.diagnostics);
}

// Q-658: `image.convert` wrote over `.env` and `main.qwe`. Q-674: a conversion's memory is
// judged before it starts.
#[test]
fn image_convert_keeps_off_the_programs_files_and_its_memory_in_bounds() {
    let d = dir(
        "convert",
        r#"print(image.convert("tiny.png", ".env", { format: "png" }))
print(image.convert("tiny.png", "main.qwe", { format: "png" }))
print(image.convert("tiny.png", "out/small.png").width)
print(image.convert("big.png", "out/big.png", { width: 6000 }))
print(image.convert("big.png", "out/thumb.png", { width: 400 }).width)
"#,
    );
    image::GrayImage::new(4, 4).save(d.join("tiny.png")).unwrap();
    image::GrayImage::new(7000, 5600).save(d.join("big.png")).unwrap();
    std::fs::write(d.join(".env"), "SECRET=x\n").unwrap();
    let out = run_script(&d);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].contains("will not write .env: it is the program's settings (.env)"), "{}", out);
    assert!(lines[1].contains("will not write main.qwe: it is one of the program's own files"), "{}", out);
    assert_eq!(std::fs::read_to_string(d.join(".env")).unwrap(), "SECRET=x\n");
    assert!(std::fs::read_to_string(d.join("main.qwe")).unwrap().starts_with("print("));
    assert_eq!(lines[2], "4");
    assert!(lines[3].contains("converting the 7000×5600 image so would take about") && lines[3].contains("more than the 640 converted at once"), "{}", out);
    assert_eq!(lines[4], "400");
}

// Q-663: under --watch, a page elsewhere whose name was made to point here reached the server
// fns and `/_quillo/dev`. Only localhost, addresses and --allowed-hosts are answered.
#[test]
fn a_server_under_watch_answers_only_its_own_names() {
    let d = dir("hosts", "server fn secret() = \"S\"\nroute GET / = \"home\"\n");
    let (_g, port) = run_with(&d, "main.qwe", &["--watch", "--allowed-hosts", "myapp.test,.dev.test"], &[]);
    let get = |host: &str| support::send(port, "GET", "/", &[("Host", host)], "");
    for host in ["localhost", "localhost:8080", "app.localhost", "127.0.0.1", "[::1]:3000", "192.168.1.20:8080", "myapp.test", "a.dev.test", "dev.test"] {
        assert_eq!(get(host).body, "home", "{}", host);
    }
    for host in ["rebind.attacker.example:47602", "myapp.test.evil", "localhost.evil.example", "evil-dev.test"] {
        let r = get(host);
        assert_eq!(r.status, 421, "{}", host);
    }
    let r = support::send(port, "POST", "/_quillo/rpc/secret", &[("Host", "rebind.attacker.example"), ("Origin", "http://rebind.attacker.example"), ("content-type", "application/json")], "[]");
    assert_eq!(r.status, 421);
    // Without --watch, a name is the proxy's business, as before.
    let (_h, plain) = run_with(&d, "main.qwe", &[], &[]);
    assert_eq!(support::send(plain, "GET", "/", &[("Host", "quillolang.example")], "").body, "home");
}

// Q-672: a body that sent a byte every 25 s held its connection for good.
#[test]
fn a_body_that_comes_too_slowly_is_let_go() {
    let d = dir("slow", "route POST /b = request.body\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let mut c = std::net::TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.write_all(b"POST /b HTTP/1.1\r\nHost: a\r\nContent-Length: 100\r\n\r\nx").unwrap();
    let start = std::time::Instant::now();
    c.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let mut out = Vec::new();
    let mut buf = [0u8; 512];
    // A byte now and then, never enough for 1 KiB/s.
    loop {
        match c.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                out.extend_from_slice(&buf[..n]);
                break;
            }
            Err(_) => {
                assert!(start.elapsed() < std::time::Duration::from_secs(70), "never let go");
                let _ = c.write_all(b"y");
            }
        }
    }
    let text = String::from_utf8_lossy(&out);
    assert!(text.starts_with("HTTP/1.1 408"), "{}", text);
    assert!(start.elapsed() > std::time::Duration::from_secs(30) && start.elapsed() < std::time::Duration::from_secs(45), "{:?}", start.elapsed());
}

// Q-668 in the browser: what a component renders there escapes as the server does. And `{...m}`
// of JSON with a `__proto__` key kept it as a key, where `Object.assign` set the prototype.
#[test]
fn the_browser_writes_css_values_as_the_server_does() {
    let d = dir(
        "css-browser",
        r#"component Css() {
    state c = "red"
    let m = json.parse("{\"__proto__\": {\"polluted\": 1}, \"a\": 2}")
    let spread = { ...m }
    <div>
        <style>{css`#cv { color: ${c} }`}</style>
        <p id="cv" style={set({ color: c, "--ok": "1", fontSize: 12 }, "x;y", "2")}>cv</p>
        <p id="sp">{join(keys(spread), ",")}</p>
        <button id="b" @click={() => { c = "blue} p{color:red;background:url(//evil)" }}>b</button>
    </div>
}
route GET / = <html><head><title>t</title></head><body><Css/></body></html>
"#,
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let b = support::send(s.port, "GET", "/", &[], "").body;
    assert!(b.contains(r#"style="color: red; --ok: 1; font-size: 12px""#), "{}", b);
    let Some(chrome) = chrome() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  document.getElementById("b").click();
  await tick(300);
  document.body.dataset.result = JSON.stringify({
    style: document.getElementById("cv").getAttribute("style"),
    sheet: document.querySelector("style").textContent,
    color: getComputedStyle(document.getElementById("cv")).color,
    keys: document.getElementById("sp").textContent,
    polluted: ({}).polluted ?? null,
  });
})();"#;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), script);
    // The escaped value is one declaration, and not a colour: the browser keeps the one before,
    // and no `background` is added; the rest of the map is as it was.
    assert_eq!(r["style"], "color: red; --ok: 1; font-size: 12px", "{}", r);
    assert_eq!(r["sheet"], r"#cv { color: blue\7d  p\7b color:red\3b background:url(//evil) }", "{}", r);
    assert_eq!(r["color"], "rgb(255, 0, 0)", "{}", r);
    assert_eq!((r["keys"].as_str(), &r["polluted"]), (Some("__proto__,a"), &serde_json::Value::Null), "{}", r);
}
