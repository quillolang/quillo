//! A front end built elsewhere (React, Vue or Svelte with Vite): its `dist/` folder served beside
//! the program's routes, as USERGUIDE.md's "A front end built elsewhere (React, Vite)" has it.
//! Each claim of that section is checked here, so a change to `file()`, `files()`, route order or
//! the server fn door that breaks the recipe fails a test, not someone's deploy (branch spa-docs).

mod support;

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::send;

const INDEX: &str = r#"<!doctype html><html><head><script type="module" src="/assets/index-3f9a1c.js"></script></head><body><div id="root"></div></body></html>"#;

/// The guide's routes, word for word, and a server fn for the app to call.
const MAIN: &str = r#"
server fn add_note(text: str) = `saved ${text}`

route GET /api/hello = { ok: true }                                    // the program's routes first
route GET /assets/*path = files("dist/assets", path, { cache: "1y" })   // Vite's hashed files
route GET /favicon.ico = file("dist/favicon.ico")                       // each file of public/
route GET /robots.txt = file("dist/robots.txt")
route GET /*path = file("dist/index.html")                              // the app's own paths
route GET /never = "shadowed by the catch-all"
"#;

/// A Vite build as it lands: the page, hashed files under assets/, and public/'s files at the top.
fn build(name: &str, main: &str) -> PathBuf {
    let d = support::temp_dir(&format!("front-end-build-{}", name));
    let files: &[(&str, &[u8])] = &[
        ("main.qwe", main.as_bytes()),
        ("dist/index.html", INDEX.as_bytes()),
        ("dist/assets/index-3f9a1c.js", b"console.log('app')"),
        ("dist/assets/index-77b0e2.css", b"#root{margin:0}"),
        ("dist/favicon.ico", b"\x00\x00\x01\x00"),
        ("dist/robots.txt", b"User-agent: *"),
        ("dist/manifest.webmanifest", b"{}"),
        ("dist/assets/.env", b"SECRET=1"),
    ];
    for (f, bytes) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    d
}

#[test]
fn a_built_app_answers_its_deep_links_with_its_page() {
    let d = build("deep", MAIN);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| send(s.port, "GET", p, &[], "");

    for path in ["/", "/products/42", "/products/42/reviews", "/a.b/c", "/settings/"] {
        let a = get(path);
        assert_eq!((a.status, a.body.as_str()), (200, INDEX), "{} is the app's page", path);
        assert_eq!(a.header("content-type"), Some("text/html; charset=utf-8"), "{}", path);
        assert_eq!(a.header("cache-control"), Some("no-cache"), "{}: a deploy is seen at the next load", path);
        assert!(!a.body.contains("w.pf=1"), "{}: sent as built, nothing added", path);
    }

    // The program's routes come first and keep their answers; one after the catch-all is never reached.
    let api = get("/api/hello");
    assert_eq!((api.status, api.body.as_str(), api.header("content-type")), (200, r#"{"ok":true}"#, Some("application/json")));
    assert_eq!(get("/never").body, INDEX, "routes are tried in the order they are written");
    let check = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["check", "main.qwe"]).current_dir(&d).stdin(Stdio::null()).output().unwrap();
    let said = String::from_utf8_lossy(&check.stderr).into_owned() + &String::from_utf8_lossy(&check.stdout);
    assert!(check.status.success(), "a warning, not an error: {}", said);
    assert!(said.contains("main.qwe:9: warning: this route is never reached: `GET /*path` (line 8) takes every request it would"), "{}", said);

    // The hashed files are kept for a year; the ones at the top are asked for each time.
    let js = get("/assets/index-3f9a1c.js");
    assert_eq!((js.status, js.body.as_str()), (200, "console.log('app')"));
    assert_eq!(js.header("content-type"), Some("text/javascript; charset=utf-8"));
    assert_eq!(js.header("cache-control"), Some("public, max-age=31536000, immutable"));
    assert_eq!(get("/assets/index-77b0e2.css").header("content-type"), Some("text/css; charset=utf-8"));
    let ico = get("/favicon.ico");
    assert_eq!((ico.status, ico.header("content-type"), ico.header("cache-control")), (200, Some("image/x-icon"), Some("no-cache")));
    assert_eq!(get("/robots.txt").body, "User-agent: *");
    // A file at the top with no route of its own is the app's page, as the guide warns.
    assert_eq!(get("/manifest.webmanifest").body, INDEX);
}

#[test]
fn a_built_app_misses_stay_misses_and_its_page_answers_as_a_file() {
    let d = build("edges", MAIN);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| send(s.port, "GET", p, &[], "");

    // An old chunk asked for after a deploy is a 404, never the page in place of JavaScript.
    let old = get("/assets/index-0000aa.js");
    assert_eq!(old.status, 404);
    assert_ne!(old.body, INDEX);
    assert_eq!(get("/assets/").status, 404, "no index in assets/");
    assert_eq!(get("/assets/.env").status, 404, "a dot file in the build is not served");
    assert_eq!(get("/assets/../main.qwe").status, 404);
    assert_eq!(get("/assets/%2e%2e/main.qwe").status, 404);

    // HEAD, a browser's current copy, another method.
    let head = send(s.port, "HEAD", "/products/42", &[], "");
    assert_eq!((head.status, head.body.as_str(), head.header("content-type")), (200, "", Some("text/html; charset=utf-8")));
    let etag = get("/products/42").header("etag").expect("the page has an ETag").to_string();
    let again = send(s.port, "GET", "/somewhere/else", &[("If-None-Match", &etag)], "");
    assert_eq!((again.status, again.body.as_str()), (304, ""), "one page for every path, one ETag");
    assert_eq!(send(s.port, "POST", "/products/42", &[], "").status, 405, "only GET and HEAD reach the catch-all");
    let range = send(s.port, "GET", "/products/42", &[("Range", "bytes=0-14")], "");
    assert_eq!((range.status, range.body.as_str()), (206, "<!doctype html>"));

    // A page replaced by the next build is served at once: the files are not kept past a change.
    std::fs::write(d.join("dist/index.html"), "<!doctype html><p>v2</p>").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let a = get("/products/42");
        if a.body == "<!doctype html><p>v2</p>" {
            assert_eq!(a.status, 200);
            break;
        }
        assert!(Instant::now() < deadline, "the new build's page is served: {:?}", a.body);
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn not_found_and_quillo_serve_answer_a_deep_link_with_a_404() {
    // Why the guide's catch-all is `file()`: the folder's page for a miss goes out as a 404.
    let d = build("not-found", "route GET /*path = files(\"dist\", path, { not_found: \"index.html\" })\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let deep = send(s.port, "GET", "/products/42", &[], "");
    assert_eq!(deep.status, 404, "the page, but as a miss");
    // The page, but not as built: a miss's page loses its ETag, so the prefetching script is added
    // (against the guide's "Pages sent as written", told to the user 2026-11-16; branch spa-docs).
    assert!(deep.body.starts_with(&INDEX[..INDEX.len() - "</body></html>".len()]), "{}", deep.body);
    assert_eq!(send(s.port, "GET", "/", &[], "").status, 200, "the index itself is found");
    drop(s);

    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["serve", "dist", "--port", &port.to_string()])
        .current_dir(&d)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    wait_for(port);
    assert_eq!(send(port, "GET", "/", &[], "").body, INDEX);
    assert_eq!(send(port, "GET", "/products/42", &[], "").status, 404, "quillo serve has no catch-all");
}

#[test]
fn the_app_calls_a_server_fn_from_the_programs_own_origin_only() {
    let d = build("rpc", MAIN);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let call = |headers: &[(&str, &str)], body: &str| send(s.port, "POST", "/_quillo/rpc/add_note", headers, body);
    let json = ("Content-Type", "application/json");

    // As the guide's fetch() sends it, from the app's page on the program's own origin.
    let ok = call(&[json, ("Sec-Fetch-Site", "same-origin"), ("Origin", "http://localhost")], r#"["buy milk"]"#);
    assert_eq!((ok.status, ok.body.as_str()), (200, r#"{"ok":"saved buy milk"}"#));
    // The catch-all does not take the server fn door.
    assert_ne!(ok.body, INDEX);

    // Through Vite's proxy: a browser says same-origin (it asked Vite), whatever Host the proxy sends.
    let short_form = call(&[json, ("Host", "localhost:8080"), ("Origin", "http://localhost:5173"), ("Sec-Fetch-Site", "same-origin")], r#"["a"]"#);
    assert_eq!(short_form.status, 200, "a browser through the short form");
    // A client sending only Origin passes with changeOrigin: false (the browser's host kept) ...
    let kept = call(&[json, ("Host", "localhost:5173"), ("Origin", "http://localhost:5173")], r#"["a"]"#);
    assert_eq!(kept.status, 200, "changeOrigin: false");
    // ... and is refused when the proxy put the program's host in its place.
    let changed = call(&[json, ("Host", "localhost:8080"), ("Origin", "http://localhost:5173")], r#"["a"]"#);
    assert_eq!(changed.status, 403, "the short form, without Sec-Fetch-Site");
    assert!(changed.body.contains(r#""error":"#), "a refusal says why: {}", changed.body);

    // Another port, another host, another subdomain of the same site: refused.
    for site in ["cross-site", "same-site"] {
        assert_eq!(call(&[json, ("Sec-Fetch-Site", site)], r#"["a"]"#).status, 403, "{}", site);
    }
    assert_eq!(call(&[json, ("Origin", "http://localhost:3000")], r#"["a"]"#).status, 403, "another port");
    assert_eq!(call(&[json, ("Origin", "https://app.example.com"), ("Host", "api.example.com")], r#"["a"]"#).status, 403, "another subdomain");

    // A wrong type is a 400 with `error`; a form body is not a call.
    let wrong = call(&[json], "[1]");
    assert_eq!(wrong.status, 400);
    assert!(wrong.body.contains(r#""error":"#), "{}", wrong.body);
    assert_eq!(call(&[("Content-Type", "application/x-www-form-urlencoded")], "text=a").status, 415);
}

fn wait_for(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "quillo serve did not listen on {}", port);
        std::thread::sleep(Duration::from_millis(50));
    }
}
