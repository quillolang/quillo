//! Prefetching (FEEDBACK22.md; branch feedback22-fixes): what a page kept for a click may
//! outlive, who goes first, and what the log and the metrics say of it.

mod support;

use std::path::PathBuf;
use std::process::{Command, Stdio};
use support::{send, serve};

fn app(name: &str, src: &str) -> (PathBuf, PathBuf) {
    let d = support::temp_dir(&format!("prefetching-{}", name));
    let f = d.join("main.qwe");
    std::fs::write(&f, src).unwrap();
    (d, f)
}

const OWN: (&str, &str) = ("x-quillo-prefetch", "1");
const RULES: (&str, &str) = ("sec-purpose", "prefetch");

const PAGES: &str = r#"
db.open(":memory:")
db.exec("create table if not exists t (n int)")
let hits = 0
route GET / = "a page"
route GET /own {
    respond(200, "mine", { "cache-control": "no-store" })
}
route POST /write {
    db.exec("insert into t values (1)")
    "wrote"
}
route POST /read = len(db.query("select * from t"))
route POST /global {
    hits = hits + 1
    "counted"
}
route POST /select_writes = len(db.query("insert into t values (2) returning n"))
route POST /login {
    set_session({ user: "ada" })
    "in"
}
route POST /logout {
    end_session()
    "out"
}
route GET /sign_in {
    set_session({ user: "ada" })
    "in"
}
route GET /counted {
    db.exec("insert into t values (3)")
    "seen"
}
route POST /cookie {
    set_cookie("theme", "dark")
    "set"
}
route POST /mine {
    respond(200, "said", { "clear-site-data": "\"cache\"" })
}
"#;

#[test]
fn only_our_own_prefetch_is_kept_in_the_http_cache_and_for_one_session() {
    let (_d, f) = app("cache", PAGES);
    let s = serve(&f, &[]);
    let own = send(s.port, "GET", "/", &[OWN], "");
    assert_eq!(own.header("cache-control"), Some("private, max-age=30"));
    assert_eq!(own.header("vary"), Some("Cookie"));
    // Chrome keeps these itself, and an HTTP cache would outlive a logout.
    let rules = send(s.port, "GET", "/", &[RULES], "");
    assert_eq!((rules.status, rules.header("cache-control"), rules.header("vary")), (200, None, None));
    // A plain page, and a route's own word.
    let plain = send(s.port, "GET", "/", &[], "");
    assert_eq!((plain.header("cache-control"), plain.header("vary")), (None, None));
    assert_eq!(send(s.port, "GET", "/own", &[OWN], "").header("cache-control"), Some("no-store"));
}

#[test]
fn what_changes_something_lets_the_kept_pages_go() {
    let (_d, f) = app("clear", PAGES);
    let s = serve(&f, &[]);
    let said = "\"prefetchCache\", \"prerenderCache\"";
    for (method, path) in [("POST", "/write"), ("POST", "/global"), ("POST", "/select_writes"), ("POST", "/login"), ("POST", "/logout"), ("POST", "/cookie"), ("GET", "/sign_in")] {
        let r = send(s.port, method, path, &[], "");
        assert_eq!((r.status, r.header("clear-site-data")), (200, Some(said)), "{} {}", method, path);
    }
    // A read is a POST too, a `GET` may count a visit, and a program's own word stands.
    for (method, path) in [("POST", "/read"), ("GET", "/"), ("GET", "/counted")] {
        let r = send(s.port, method, path, &[], "");
        assert_eq!((r.status, r.header("clear-site-data")), (200, None), "{} {}", method, path);
    }
    assert_eq!(send(s.port, "POST", "/mine", &[], "").header("clear-site-data"), Some("\"cache\""));
}

#[test]
fn with_prefetching_off_nothing_is_kept_to_let_go() {
    let (_d, f) = app("off", PAGES);
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", f.to_str().unwrap(), "--port", &port.to_string(), "--no-prefetch"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    wait_for(port);
    assert_eq!(send(port, "POST", "/write", &[], "").header("clear-site-data"), None);
}

fn wait_for(port: u16) {
    for _ in 0..400 {
        if support::listening(port) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

// ---- Q-179: who goes first -------------------------------------------------------------------

const SLOW: &str = r#"
fn work() {
    let t = 0
    for i in range(0, 300000) {
        t = t + len(str(i))
    }
    t
}
route GET / {
    work()
    "made"
}
"#;

#[test]
fn prefetches_do_not_go_before_a_visitor() {
    let (_d, f) = app("line", SLOW);
    let s = serve(&f, &[]);
    let port = s.port;
    // One alone is made, as nobody is waiting.
    assert_eq!(send(port, "GET", "/", &[RULES], "").status, 200);
    let guesses: Vec<_> = (0..24).map(|_| std::thread::spawn(move || send(port, "GET", "/", &[RULES], "").status)).collect();
    let began = std::time::Instant::now();
    assert_eq!(send(port, "GET", "/", &[], "").status, 200);
    let visitor = began.elapsed();
    let statuses: Vec<u16> = guesses.into_iter().map(|g| g.join().unwrap()).collect();
    let refused = statuses.iter().filter(|s| **s == 503).count();
    assert!(refused > 0, "{:?}", statuses);
    assert!(statuses.iter().all(|s| *s == 200 || *s == 503), "{:?}", statuses);
    // Behind all of them it would have waited for 24 pages.
    let one = {
        let t = std::time::Instant::now();
        send(port, "GET", "/", &[], "");
        t.elapsed()
    };
    assert!(visitor < one * 12, "{:?} against {:?} for one page", visitor, one);
}

// ---- Q-181: telling them apart ---------------------------------------------------------------

fn logged(name: &str, args: &[&str], go: impl Fn(u16)) -> String {
    let (d, f) = app(name, "route GET / = <html><body><a href=\"/x\">x</a></body></html>\n");
    let port = support::free_port();
    let log = d.join("log.txt");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", f.to_str().unwrap(), "--port", &port.to_string()])
        .args(args)
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let g = support::Guard::new(child);
    wait_for(port);
    go(port);
    std::thread::sleep(std::time::Duration::from_millis(200));
    drop(g);
    std::fs::read_to_string(&log).unwrap()
}

#[test]
fn a_prefetch_is_marked_in_the_log() {
    let text = logged("log", &[], |port| {
        send(port, "GET", "/", &[RULES], "");
        send(port, "GET", "/", &[], "");
    });
    let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("GET /")).collect();
    assert_eq!(lines.len(), 2, "{}", text);
    assert!(lines[0].ends_with(" prefetch"), "{}", text);
    assert!(!lines[1].contains("prefetch"), "{}", text);
    let json = logged("log-json", &["--log", "json"], |port| {
        send(port, "GET", "/", &[OWN], "");
        send(port, "GET", "/", &[], "");
    });
    let lines: Vec<serde_json::Value> = json.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    assert_eq!(lines.len(), 2, "{}", json);
    assert_eq!(lines[0]["prefetch"], true);
    assert_eq!(lines[1].get("prefetch"), None);
}

#[test]
fn metrics_count_what_became_of_prefetches() {
    let (_d, f) = app("metrics", SLOW);
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", f.to_str().unwrap(), "--port", &port.to_string(), "--metrics"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    wait_for(port);
    assert_eq!(send(port, "GET", "/", &[RULES], "").status, 200);
    assert_eq!(send(port, "POST", "/_quillo/prefetch-used", &[], "").status, 204);
    let m = send(port, "GET", "/_quillo/metrics", &[], "").body;
    assert!(m.contains("quillo_prefetches_total{result=\"answered\"} 1"), "{}", m);
    assert!(m.contains("quillo_prefetches_total{result=\"refused\"} 0"), "{}", m);
    assert!(m.contains("quillo_prefetches_total{result=\"used\"} 1"), "{}", m);
}

#[test]
fn a_page_says_it_came_from_a_prefetch_only_where_that_is_counted() {
    let page = "route GET / = <html><body><a href=\"/x\">x</a></body></html>\n";
    let (_d, f) = app("beacon", page);
    let s = serve(&f, &[]);
    assert!(!send(s.port, "GET", "/", &[], "").body.contains("prefetch-used"));
    assert_ne!(send(s.port, "POST", "/_quillo/prefetch-used", &[], "").status, 204);
    let (_d, f) = app("beacon-on", page);
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", f.to_str().unwrap(), "--port", &port.to_string(), "--metrics"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    wait_for(port);
    assert!(send(port, "GET", "/", &[], "").body.contains("/_quillo/prefetch-used"));
}
