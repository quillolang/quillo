//! FEEDBACK69.md: what the test of "A front end built elsewhere" (tests/front_end_build.rs, branch
//! spa-docs) found. The page `files()` sends for a miss lost its `ETag` (Q-715), and so looked
//! like a page a route made: it was given the prefetching script, and `--watch`'s, against the
//! guide's "Pages sent as written" (Q-964; branch q964-miss-page).

mod support;

use std::path::PathBuf;
use support::send;

/// A miss's page with all a prefetch looks for: a link and a script.
const PAGE: &str = r#"<!doctype html><html><body><h1>Not here</h1><a href="/">Home</a><script src="/app.js"></script></body></html>"#;

const MAIN: &str = r#"
route GET /made = respond(404, <html><body><a href="/x">x</a></body></html>)
route GET /forged = respond(404, <html><body><a href="/x">x</a></body></html>, { "x-quillo-as-written": "1" })
route GET /forged-ok = respond(200, <html><body><a href="/x">x</a></body></html>, { "x-quillo-as-written": "1" })
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
        ("docs/site/404.html", PAGE.as_bytes()),
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
    assert_eq!(mounted.status, 404);
    assert!(mounted.body.contains("<h1>Not here</h1>") && !mounted.body.contains(PREFETCH), "{}", mounted.body);
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
fn the_mark_is_quillos_whoever_sets_it() {
    let d = site("forged");
    let s = support::serve(&d.join("main.qwe"), &[]);
    // A program that writes the mark on a 404 has it taken as the page's own word, as an `etag`
    // it wrote would be, and it is not sent.
    let forged = send(s.port, "GET", "/forged", &[], "");
    assert_eq!(forged.status, 404);
    assert!(!forged.body.contains(PREFETCH), "{}", forged.body);
    assert!(forged.header(MARK).is_none(), "taken off");
    // On any other status it means nothing to Quillo: the page prefetches, and the header is the
    // program's to send.
    let ok = send(s.port, "GET", "/forged-ok", &[], "");
    assert_eq!(ok.status, 200);
    assert!(ok.body.contains(PREFETCH), "{}", ok.body);
    assert_eq!(ok.header(MARK), Some("1"));
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
    assert!(!mounted.body.contains(DEV) && !mounted.body.contains(PREFETCH), "{}", mounted.body);
}
