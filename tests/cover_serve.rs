//! Corners of src/server.rs no other test reaches: markup that says it is XML, a route that
//! looks like a `build` declaration's and is not one, and a live loader that gives too much.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn folder(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("cover-serve-{}", name)).canonicalize().unwrap();
    std::fs::write(d.join("app.qwe"), src).unwrap();
    d
}

fn app(name: &str, src: &str) -> Interp {
    let d = folder(name, src);
    let mut it = Interp::new(d.to_str().unwrap());
    it.load_file(&d.join("app.qwe")).unwrap_or_else(|e| panic!("{}", e));
    it
}

fn get(it: &mut Interp, url: &str) -> HttpResponse {
    let headers = vec![("Host".to_string(), "localhost".to_string())];
    it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers, body: Vec::new(), remote: "127.0.0.1".into() })
}

fn header<'a>(r: &'a HttpResponse, name: &str) -> Option<&'a str> {
    r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// Markup that begins with an XML declaration is XML, whatever its root: a feed or a sitemap
/// made by hand.
#[test]
fn markup_that_begins_with_an_xml_declaration_is_sent_as_xml() {
    let mut it = app("xml", "route GET /feed = raw(\"<?xml version=\\\"1.0\\\"?><rss><channel/></rss>\")\nroute GET /page = raw(\"<p>hi</p>\")\n");
    let r = get(&mut it, "/feed");
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(header(&r, "content-type"), Some("application/xml; charset=utf-8"));
    assert!(r.text().starts_with("<?xml version=\"1.0\"?><rss>"), "{}", r.text());
    let r = get(&mut it, "/page");
    assert_eq!(header(&r, "content-type"), Some("text/html; charset=utf-8"));
}

/// Only a `build` declaration's route names a path to build: one calling `__built` with
/// anything but a string is no such declaration.
#[test]
fn a_route_calling_built_with_no_path_is_no_build_declaration() {
    let d = folder("built", "");
    let mut it = Interp::new(d.to_str().unwrap());
    let src = "build \"/made.txt\" = \"made\"\nroute GET /odd = __built(1, () => \"x\")\nroute GET /plain = \"plain\"\n";
    it.load_source(src, d.join("app.qwe").to_str().unwrap()).unwrap_or_else(|e| panic!("{}", e));
    assert_eq!(quillo::server::build_paths(&it), vec!["/made.txt".to_string()]);
}

/// Outside the server, a page asking for a live loader's run that another is making does not
/// wait for it (there is no coroutine to wait in): it makes its own, and both are answered.
#[test]
fn outside_the_server_a_live_run_in_progress_is_not_waited_for() {
    let mut it = app("live-turn", "let runs = 0\nserver fn slow() {\n    runs = runs + 1\n    sleep(600)\n    \"slow\"\n}\nroute GET /runs = str(runs)\n");
    let mut other = it.fork();
    let other_for_runs = it.fork();
    // Each takes the program's lock, as the server's threads do; `sleep` lets it go.
    it.shared.gil.release();
    let live = |it: &mut Interp| {
        let headers = [("Host", "localhost"), ("x-quillo-live", "1"), ("sec-fetch-site", "same-origin"), ("content-type", "application/json")]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        it.shared.gil.acquire();
        let r = it.handle(&HttpRequest { method: "POST".into(), url: "/_quillo/rpc/slow".into(), headers, body: b"[]".to_vec(), remote: "127.0.0.1".into() });
        it.shared.gil.release();
        r
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let tx2 = tx.clone();
    std::thread::spawn(move || tx.send(live(&mut it)).unwrap());
    std::thread::sleep(Duration::from_millis(150));
    std::thread::spawn(move || tx2.send(live(&mut other)).unwrap());
    for _ in 0..2 {
        let r = rx.recv_timeout(Duration::from_secs(20)).expect("a page was not answered");
        assert_eq!(r.status, 200, "{}", r.text());
        assert!(r.text().contains("\"slow\""), "{}", r.text());
    }
    let mut it = other_for_runs;
    it.shared.gil.acquire();
    assert_eq!(get(&mut it, "/runs").text(), "2", "the second waited for the first's run");
}

// ---- a running server -------------------------------------------------------------------

struct Srv {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Srv {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Srv {
    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

fn run(d: &Path) -> Srv {
    let port = support::free_port();
    let log = d.join("stderr.log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "app.qwe", "--port", &port.to_string()])
        .current_dir(d)
        .env("QUILLO_SECRET", "a test secret of more than thirty-two bytes")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .expect("cannot start quillo");
    support::stop_with_tests(&child);
    let s = Srv { child, port, log };
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "the server did not start: {}", s.log());
        std::thread::sleep(Duration::from_millis(20));
    }
    s
}

/// A live loader that gives every page megabytes is told of, once, in the log: it should give
/// what the page shows, not the table. Its answer is given all the same.
#[test]
fn a_live_loader_giving_megabytes_is_told_of_once() {
    let d = folder("live-large", "server fn huge() = repeat(\"x\", 5000000)\nserver fn small() = \"small\"\n");
    let srv = run(&d);
    let refresh = |f: &str| support::send(srv.port, "POST", &format!("/_quillo/rpc/{}", f), &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin")], "[]");
    let a = refresh("huge");
    assert_eq!(a.status, 200, "{}", &a.body[..a.body.len().min(300)]);
    assert!(a.body.len() > 5_000_000 && a.body.contains("xxxxxxxx"), "{}", a.body.len());
    let said = "`huge` gives 4 MB to each live page: a loader should give what the page shows, not the table";
    let deadline = Instant::now() + Duration::from_secs(10);
    while !srv.log().contains(said) {
        assert!(Instant::now() < deadline, "{}", srv.log());
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(refresh("huge").status, 200);
    let small = refresh("small");
    assert!(small.body.contains("small"), "{}", small.body);
    assert_eq!(srv.log().matches(said).count(), 1, "{}", srv.log());
}

/// Under `--watch`, the files a program served are watched too; one that can no longer be
/// opened when the watch is renewed (a new file served) leaves the server looking for changes
/// every 50 ms instead, as it says, and changes are still found.
#[test]
fn a_served_file_that_cannot_be_opened_any_more_is_watched_by_looking() {
    use std::os::unix::fs::PermissionsExt;
    let d = folder("watch-unreadable", "route GET /a.txt = file(\"a.txt\")\nroute GET /b.txt = file(\"b.txt\")\nroute GET / = \"one\"\n");
    std::fs::write(d.join("a.txt"), "a").unwrap();
    std::fs::write(d.join("b.txt"), "b").unwrap();
    let port = support::free_port();
    let log = d.join("stderr.log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "app.qwe", "--port", &port.to_string(), "--watch"])
        .current_dir(&d)
        .env("QUILLO_SECRET", "a test secret of more than thirty-two bytes")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .expect("cannot start quillo");
    support::stop_with_tests(&child);
    let srv = Srv { child, port, log };
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "the server did not start: {}", srv.log());
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(support::send(port, "GET", "/a.txt", &[], "").body, "a");
    std::fs::set_permissions(d.join("a.txt"), std::fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(support::send(port, "GET", "/b.txt", &[], "").body, "b");
    // macOS watches each file, and one it cannot open turns it to looking; Linux watches their
    // folders, which a file's mode does not stop, so there it goes on being told (found by CI).
    if cfg!(target_os = "macos") {
        let said = "looking for changed files every 50 ms (the system cannot tell of them:";
        let deadline = Instant::now() + Duration::from_secs(10);
        while !srv.log().contains(said) {
            assert!(Instant::now() < deadline, "{}", srv.log());
            std::thread::sleep(Duration::from_millis(20));
        }
        let log = srv.log();
        let line = log.lines().find(|l| l.contains(said)).unwrap();
        assert!(line.contains("a.txt: Permission denied"), "{}", line);
    }
    std::fs::set_permissions(d.join("a.txt"), std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::write(d.join("app.qwe"), "route GET / = \"two\"\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while support::send(port, "GET", "/", &[], "").body != "two" {
        assert!(Instant::now() < deadline, "the change was not found: {}", srv.log());
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// In the server, a page asking for a live loader's run that another is making waits for it;
/// a run that failed is not shared, so the waiting page then makes its own.
#[test]
fn a_page_waiting_for_a_live_run_that_failed_makes_its_own() {
    let d = folder("live-wait-failed", "let runs = 0\nserver fn slow() {\n    runs = runs + 1\n    sleep(500)\n    err(\"no rows today\")\n}\nroute GET /runs = str(runs)\n");
    let srv = run(&d);
    let port = srv.port;
    let refresh = move || support::send(port, "POST", "/_quillo/rpc/slow", &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin")], "[]");
    let first = std::thread::spawn(refresh);
    std::thread::sleep(Duration::from_millis(150));
    let second = std::thread::spawn(refresh);
    for a in [first.join().unwrap(), second.join().unwrap()] {
        assert_eq!(a.status, 200, "{}", a.body);
        assert!(a.body.contains("no rows today"), "{}", a.body);
    }
    assert_eq!(support::send(port, "GET", "/runs", &[], "").body, "2", "{}", srv.log());
}
