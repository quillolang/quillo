//! FEEDBACK69.md: what the test of "A front end built elsewhere" (tests/front_end_build.rs, branch
//! spa-docs) found. The page `files()` sends for a miss lost its `ETag` (Q-715), and so looked
//! like a page a route made: it was given the prefetching script, and `--watch`'s, against the
//! guide's "Pages sent as written" (Q-964; branch q964-miss-page).

mod support;

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::send;

/// A miss's page with all a prefetch looks for: a link and a script.
const PAGE: &str = r#"<!doctype html><html><body><h1>Not here</h1><a href="/">Home</a><script src="/app.js"></script></body></html>"#;

/// The mounted program's own page for a miss, told apart from the main one's.
const DOCS_PAGE: &str = r#"<!doctype html><html><body><h1>Docs: not here</h1><a href="/">Docs home</a><script src="/d.js"></script></body></html>"#;

const MAIN: &str = r#"
route GET /made = respond(404, <html><body><a href="/x">x</a></body></html>)
route GET /forged = respond(404, <html><body><a href="/x">x</a></body></html>, { "x-quillo-as-written": "1" })
route GET /forged-stream = stream((send) => send("x"), { headers: { "X-Quillo-As-Written": "1" } })
route GET /r200/*p { reply 200 files("site", p) }
route GET /r404/*p { reply 404 files("site", p) }
route GET /r410/*p { reply 410 files("site", p) }
route GET /seen/*p = files("site", p).headers
route GET /copied/*p = respond(404, <html><body><a href="/x">x</a></body></html>, files("site", p).headers)
route GET /txt/*p = files("site", p, { not_found: "missing.txt" })
route GET /named/*p = files("site", p, { not_found: "gone.html" })
mount /docs = "docs/app.qwe"
route GET /*p = files("site", p)
"#;

const PREFETCH: &str = "w.pf=1";
const DEV: &str = "/*quillo-dev*/";
const MARK: &str = "x-quillo-as-written";

fn site(name: &str) -> PathBuf {
    let d = support::temp_dir(&format!("feedback69-{}", name));
    let files: &[(&str, &[u8])] = &[
        ("main.qwe", MAIN.as_bytes()),
        ("site/index.html", br#"<html><body><a href="/a">a</a></body></html>"#),
        ("site/404.html", PAGE.as_bytes()),
        ("site/gone.html", PAGE.as_bytes()),
        ("site/missing.txt", b"gone"),
        ("docs/app.qwe", b"route GET /*p = files(\"site\", p)\n"),
        ("docs/site/index.html", b"<html><body>docs</body></html>"),
        ("docs/site/404.html", DOCS_PAGE.as_bytes()),
    ];
    for (f, bytes) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    d
}

#[test]
fn the_page_for_a_miss_is_sent_as_written() {
    let d = site("written");
    let s = support::serve(&d.join("main.qwe"), &[]);
    for path in ["/nothing", "/a/b/c", "/named/nothing"] {
        let a = send(s.port, "GET", path, &[], "");
        assert_eq!((a.status, a.body.as_str()), (404, PAGE), "{}: the file's bytes, nothing added", path);
        assert_eq!(a.header("content-type"), Some("text/html; charset=utf-8"), "{}", path);
        assert_eq!(a.header("cache-control"), Some("no-cache"), "{}: as before (Q-715)", path);
        assert!(a.header("etag").is_none() && a.header("last-modified").is_none(), "{}: still not the page's validators (Q-715)", path);
        assert!(a.header(MARK).is_none(), "{}: the mark is never sent", path);
    }
    // Whole, whatever the request's range and conditionals (Q-722), and as written.
    let ranged = send(s.port, "GET", "/nothing", &[("Range", "bytes=0-3"), ("If-None-Match", "*")], "");
    assert_eq!((ranged.status, ranged.body.as_str()), (404, PAGE));
    let head = send(s.port, "HEAD", "/nothing", &[], "");
    assert_eq!((head.status, head.body.as_str()), (404, ""));
    assert!(head.header(MARK).is_none());
    // A miss's page that is not HTML is as it was.
    let txt = send(s.port, "GET", "/txt/nothing", &[], "");
    assert_eq!((txt.status, txt.body.as_str(), txt.header(MARK)), (404, "gone", None));
    // A file found is as it was: no script either.
    assert_eq!(send(s.port, "GET", "/", &[], "").body, r#"<html><body><a href="/a">a</a></body></html>"#);
    // A mounted program's miss is its own files()'s, and goes out as written through the program
    // it is mounted in.
    let mounted = send(s.port, "GET", "/docs/nothing", &[], "");
    assert_eq!((mounted.status, mounted.body.as_str()), (404, DOCS_PAGE), "its own page, its links as written");
    assert!(mounted.header(MARK).is_none());
}

#[test]
fn a_404_a_route_makes_still_prefetches() {
    let d = site("made");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let made = send(s.port, "GET", "/made", &[], "");
    assert_eq!(made.status, 404);
    assert!(made.body.contains(PREFETCH), "a page a route made, as before: {}", made.body);
}

#[test]
fn the_mark_is_quillos_and_no_program_sets_it() {
    let d = site("forged");
    let s = support::serve(&d.join("main.qwe"), &[]);
    // respond's, stream's and events' headers refuse it, in any case of its letters (found in review).
    for path in ["/forged", "/forged-stream"] {
        let a = send(s.port, "GET", path, &[], "");
        assert_eq!(a.status, 500, "{}", path);
        assert!(a.header(MARK).is_none() && !a.body.contains("x-quillo"), "{}: {}", path, a.body);
    }
    // site_headers too: the program does not start.
    let d = support::temp_dir("feedback69-site-headers");
    std::fs::write(d.join("main.qwe"), "site_headers({ \"x-quillo-as-written\": \"1\" })\nroute GET / = \"x\"\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--port", "0"])
        .current_dir(&d)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break st;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("site_headers with the mark did not stop the program");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut err = String::new();
    std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut err).unwrap();
    assert!(!status.success());
    assert!(err.contains("`site_headers`: `x-quillo-as-written` is Quillo's own"), "{}", err);
}

#[test]
fn a_miss_given_another_status_is_the_programs_answer() {
    // `reply 200 files(...)` kept every header and changed the status, and the mark, which only a
    // 404 has taken off, went out (found in review).
    let d = site("reply");
    let s = support::serve(&d.join("main.qwe"), &[]);
    for (path, status) in [("/r200/nothing", 200), ("/r410/nothing", 410)] {
        let a = send(s.port, "GET", path, &[], "");
        assert_eq!(a.status, status, "{}", path);
        assert!(a.header(MARK).is_none(), "{}: the mark is not sent", path);
        assert!(a.body.starts_with(&PAGE[..PAGE.len() - "</body></html>".len()]) && a.body.contains(PREFETCH), "{}: the program's page, as in 0.8.5: {}", path, a.body);
    }
    // Still a 404: still the miss's page, as written.
    let kept = send(s.port, "GET", "/r404/nothing", &[], "");
    assert_eq!((kept.status, kept.body.as_str(), kept.header(MARK)), (404, PAGE, None));
    // A file found keeps its ETag whatever the status, as before.
    assert!(send(s.port, "GET", "/r200/index.html", &[], "").header("etag").is_some());
}

#[test]
fn a_program_reading_the_answer_does_not_see_the_mark() {
    let d = site("seen");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let seen = send(s.port, "GET", "/seen/nothing", &[], "");
    assert_eq!(seen.status, 200);
    assert!(seen.body.contains("cache-control") && !seen.body.contains("x-quillo"), "{}", seen.body);
    // Copied into a page the program makes, nothing of the miss's is carried: the page prefetches.
    let copied = send(s.port, "GET", "/copied/nothing", &[], "");
    assert_eq!(copied.status, 404);
    assert!(copied.body.contains(PREFETCH) && copied.header(MARK).is_none(), "{}", copied.body);
}

#[test]
fn under_watch_the_page_for_a_miss_has_no_dev_script() {
    let d = site("watch");
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let miss = send(s.port, "GET", "/nothing", &[], "");
    assert_eq!((miss.status, miss.body.as_str()), (404, PAGE), "neither --watch's script nor the prefetching one");
    assert!(miss.header(MARK).is_none());
    let made = send(s.port, "GET", "/made", &[], "");
    assert!(made.body.contains(DEV), "a page a route made still reloads at a save: {}", made.body);
    let mounted = send(s.port, "GET", "/docs/nothing", &[], "");
    assert_eq!(mounted.body, DOCS_PAGE, "no script of either kind");
}
