//! FEEDBACK36.md: the HTTP Quillo speaks, and plain HTML and other text (Q-432 to Q-451; branch
//! feedback36-39).

mod support;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use support::send;

fn dir(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback36-{}", name));
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

fn quillo(d: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quillo")).args(args).current_dir(d).stdin(Stdio::null()).output().unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Raw bytes to the server, and all it sent back.
fn raw(port: u16, bytes: &[u8]) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(bytes).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

const SERVER: &str = r##"
route GET /hello = "hi"
route POST /hello = "posted"
route POST /echo = { len: len(request.body), json: request.json }
route GET /tagged = respond(200, "v1", { etag: "\"abc\"" })
route GET /s = stream((send) => {
    send("one")
    send("two")
})
route GET /ev = events((send) => {
    send({ n: 1 }, { event: "tick", id: "7", retry: 1500 })
    send(null, { id: "8" })
    send(2, "plain")
})
route GET /reset = respond(205, "dropped")
route GET /teapot = respond(418, "")
route GET /map = <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><url><loc>x</loc></url></urlset>
route GET /feed = <rss version="2.0"><channel></channel></rss>
route GET /svg = <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"></svg>
route GET /c = <div><!-- note --><p class='a b' id=top>a</p><i data-x=1/></div>
route GET /md = markdown_page("# Hello\n\ntext", { lang: "en" })
route GET /md2 = markdown_page("no heading", { title: "T", html: true })
route GET /page = <html><body><a href="/x">x</a></body></html>
route GET /*path = files("site", path)
"##;

fn site() -> Vec<(&'static str, Vec<u8>)> {
    let big = format!("<html><body>{}</body></html>", "x".repeat(3000));
    vec![
        ("main.qwe", SERVER.as_bytes().to_vec()),
        ("site/index.html", b"<html><body>home</body></html>".to_vec()),
        ("site/about.html", b"<p>about</p>".to_vec()),
        ("site/docs/index.html", b"docs".to_vec()),
        ("site/404.html", b"nope".to_vec()),
        ("site/a.vtt", b"WEBVTT".to_vec()),
        ("site/cal.ics", b"BEGIN:VCALENDAR".to_vec()),
        ("site/.env", b"secret".to_vec()),
        ("site/x.qwe", b"secret".to_vec()),
        ("site/.well-known/security.txt", b"Contact: x".to_vec()),
        ("site/big.html", big.into_bytes()),
    ]
}

fn served(name: &str) -> (support::Server, PathBuf) {
    let files = site();
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let d = dir(name, &refs);
    (support::serve(&d.join("main.qwe"), &[]), d)
}

#[test]
fn a_folder_of_files_is_served_as_a_static_host_serves_it() {
    let (s, _d) = served("a_folder_of_files_is_ser");
    let get = |p: &str| send(s.port, "GET", p, &[], "");
    assert_eq!(get("/").body, "<html><body>home</body></html>", "the index, as written: no script added (Q-447)");
    assert_eq!(get("/about").body, "<p>about</p>", "a clean URL");
    let docs = get("/docs");
    assert_eq!((docs.status, docs.header("location")), (301, Some("/docs/")), "a folder named without its /");
    assert_eq!(get("/docs/").body, "docs");
    let miss = get("/nothing");
    assert_eq!((miss.status, miss.body.as_str()), (404, "nope"), "the folder's 404 page");
    assert!(miss.header("etag").is_none(), "not the page's own validators");
    assert_eq!(get("/a.vtt").header("content-type"), Some("text/vtt; charset=utf-8"), "Q-451");
    assert_eq!(get("/cal.ics").header("content-type"), Some("text/calendar; charset=utf-8"));
    assert_eq!(get("/.env").status, 404, "a dotfile is not served");
    assert_eq!(get("/x.qwe").status, 404, "nor Quillo's own");
    assert_eq!(get("/.well-known/security.txt").body, "Contact: x", ".well-known is");
    assert_eq!(get("/../main.qwe").status, 404);
    // A range of an HTML file is the file's bytes (Q-447).
    let part = send(s.port, "GET", "/index.html", &[("Range", "bytes=0-5")], "");
    assert_eq!((part.status, part.body.as_str()), (206, "<html>"));
    // A page a route makes still prefetches.
    assert!(get("/page").body.contains("__quillo_pf"));
}

#[test]
fn files_takes_its_options_and_refuses_others() {
    let d = dir(
        "files-opts",
        &[
            ("main.qwe", b"route GET /a/*p = files(\"pub\", p, { index: \"home.htm\", extensions: [\"htm\"], not_found: null, cache: \"1h\" })\nroute GET /b/*p = files(\"pub\", p, { nope: 1 })\nroute GET /c/*p = files(\"pub\", p, { index: 1 })\nroute GET /d/*p = files(\"pub\", 3)\nroute GET /e/*p = files(\"pub\", p, 3)\n"),
            ("pub/home.htm", b"home"),
            ("pub/page.htm", b"page"),
            ("pub/404.html", b"not this"),
        ],
    );
    let s = support::serve_unchecked(&d.join("main.qwe"));
    let get = |p: &str| send(s.port, "GET", p, &[], "");
    assert_eq!(get("/a/").body, "home");
    assert_eq!(get("/a/page").body, "page");
    assert!(get("/a/page").header("cache-control").unwrap().contains("max-age=3600"));
    assert_eq!(get("/a/missing").body, "not found", "not_found: null");
    for p in ["/b/x", "/c/x", "/d/x", "/e/x"] {
        assert_eq!(get(p).status, 500, "{}", p);
    }
}

#[test]
fn methods_options_and_statuses() {
    let (s, _d) = served("methods_options_and_stat");
    let del = send(s.port, "DELETE", "/hello", &[], "");
    assert_eq!((del.status, del.header("allow")), (405, Some("GET, HEAD, OPTIONS, POST")), "Q-440");
    let opt = send(s.port, "OPTIONS", "/hello", &[], "");
    assert_eq!((opt.status, opt.header("allow")), (204, Some("GET, HEAD, OPTIONS, POST")));
    assert_eq!(send(s.port, "FROB", "/hello", &[], "").status, 501);
    assert!(raw(s.port, b"OPTIONS * HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").starts_with("HTTP/1.1 204"));
    let reset = raw(s.port, b"GET /reset HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    assert!(reset.starts_with("HTTP/1.1 205 Reset Content") && reset.contains("content-length: 0") && !reset.contains("dropped"), "{}", reset);
    assert!(raw(s.port, b"GET /teapot HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").starts_with("HTTP/1.1 418 I'm a teapot"));
    assert!(raw(s.port, b"GET /hello HTTP/1.1\r\nHost: x\r\nExpect: foo\r\n\r\n").starts_with("HTTP/1.1 417"));
    let long = format!("GET /{} HTTP/1.1\r\nHost: x\r\n\r\n", "a".repeat(70_000));
    assert!(raw(s.port, long.as_bytes()).starts_with("HTTP/1.1 414"), "Q-446");
    let heads = format!("GET / HTTP/1.1\r\nHost: x\r\nX-A: {}\r\n\r\n", "a".repeat(70_000));
    assert!(raw(s.port, heads.as_bytes()).starts_with("HTTP/1.1 431"));
}

#[test]
fn http_1_0_is_answered_as_1_0() {
    let (s, _d) = served("http_1_0_is_answered_as_");
    let streamed = raw(s.port, b"GET /s HTTP/1.0\r\n\r\n");
    assert!(!streamed.to_ascii_lowercase().contains("transfer-encoding"), "Q-434: {}", streamed);
    assert!(streamed.ends_with("onetwo"), "{}", streamed);
    let head = raw(s.port, b"HEAD /s HTTP/1.0\r\n\r\n");
    assert!(head.contains("connection: close") && !head.contains("onetwo"), "{}", head);
    let posted = raw(s.port, b"POST /hello HTTP/1.0\r\nExpect: 100-continue\r\nContent-Length: 5\r\n\r\nhello");
    assert!(posted.starts_with("HTTP/1.1 200") && !posted.contains("100 Continue"), "{}", posted);
    let upgrade = raw(s.port, b"GET /_quillo/live HTTP/1.0\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n");
    assert!(!upgrade.starts_with("HTTP/1.1 101"), "Q-445: a 1.0 request does not upgrade");
    // 1.1 still gets its 100.
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.write_all(b"POST /hello HTTP/1.1\r\nHost: x\r\nExpect: 100-continue\r\nContent-Length: 5\r\n\r\n").unwrap();
    let mut buf = [0u8; 64];
    let n = c.read(&mut buf).unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 100 Continue"));
}

#[test]
fn websocket_handshakes_are_checked() {
    let (s, _d) = served("websocket_handshakes_are");
    let hs = |method: &str, version: &str| {
        let r = format!("{} /_quillo/live HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: {}\r\nContent-Length: 0\r\n\r\n", method, version);
        let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(r.as_bytes()).unwrap();
        let mut buf = [0u8; 200];
        let n = c.read(&mut buf).unwrap_or(0);
        String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string()
    };
    assert_eq!(hs("GET", "8"), "HTTP/1.1 426 Upgrade Required");
    assert_eq!(hs("POST", "13"), "HTTP/1.1 400 Bad Request");
    assert_eq!(hs("GET", "13"), "HTTP/1.1 101 Switching Protocols");
}

#[test]
fn caches_revalidate() {
    let (s, _d) = served("caches_revalidate");
    let about = send(s.port, "GET", "/about.html", &[], "");
    let lm = about.header("last-modified").unwrap().to_string();
    let now = "Sun, 01 Jan 2090 00:00:00 GMT";
    assert_eq!(send(s.port, "GET", "/about.html", &[("If-Modified-Since", now)], "").status, 304, "a later date (Q-441)");
    assert_eq!(send(s.port, "GET", "/about.html", &[("If-Modified-Since", &lm)], "").status, 304);
    assert_eq!(send(s.port, "GET", "/about.html", &[("If-Modified-Since", "Monday, 01-Jan-89 00:00:00 GMT")], "").status, 200, "RFC 850, 2089 read as 1989");
    assert_eq!(send(s.port, "GET", "/about.html", &[("If-Modified-Since", "Sun Jan  1 00:00:00 2090")], "").status, 304, "asctime");
    assert_eq!(send(s.port, "GET", "/about.html", &[("If-Modified-Since", "Mon, 01 Jan 2001 00:00:00 GMT")], "").status, 200);
    assert_eq!(send(s.port, "GET", "/about.html", &[("If-Modified-Since", "nonsense")], "").status, 200);
    assert_eq!(send(s.port, "GET", "/about.html", &[("If-Match", "\"nope\"")], "").status, 412);
    assert_eq!(send(s.port, "GET", "/about.html", &[("If-Match", "*")], "").status, 200);
    assert_eq!(send(s.port, "GET", "/about.html", &[("If-Unmodified-Since", "Mon, 01 Jan 2001 00:00:00 GMT")], "").status, 412);
    assert_eq!(send(s.port, "GET", "/about.html", &[("If-Unmodified-Since", now)], "").status, 200);
    assert_eq!(send(s.port, "GET", "/tagged", &[("If-None-Match", "\"abc\"")], "").status, 304, "a route's own etag");
    assert_eq!(send(s.port, "POST", "/tagged", &[("If-None-Match", "\"abc\"")], "").status, 405);
    let media = send(s.port, "GET", "/_quillo/media.js", &[], "");
    let etag = media.header("etag").unwrap().to_string();
    assert_eq!(send(s.port, "GET", "/_quillo/media.js", &[("If-None-Match", &etag)], "").status, 304);
}

#[test]
fn gzip_is_read_as_accept_encoding_says() {
    let (s, _d) = served("gzip_is_read_as_accept_e");
    let size = |ae: &str| send(s.port, "GET", "/big.html", &[("Accept-Encoding", ae)], "").header("content-encoding").is_some();
    assert!(size("gzip") && size("GZIP") && size("x-gzip") && size("*") && size("br, gzip;q=0.5"), "Q-439");
    assert!(!size("gzip;q=0") && !size("deflate, gzip;q=0") && !size("identity") && !size("*;q=0") && !size("gzip;q=0, *"));
}

#[test]
fn a_compressed_body_is_decoded_or_refused() {
    use flate2::write::GzEncoder;
    let (s, _d) = served("a_compressed_body_is_dec");
    let mut gz = GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(br#"{"a":1}"#).unwrap();
    let body = gz.finish().unwrap();
    let mut head = format!("POST /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    head.extend_from_slice(&body);
    let r = raw(s.port, &head);
    assert!(r.ends_with(r#"{"len":7,"json":{"a":1}}"#), "Q-438: {}", r);
    assert!(raw(s.port, b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Encoding: br\r\nContent-Length: 2\r\n\r\nxx").starts_with("HTTP/1.1 415"));
    assert!(raw(s.port, b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Encoding: gzip\r\nContent-Length: 3\r\n\r\nabc").starts_with("HTTP/1.1 400"));
    assert!(raw(s.port, b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Encoding: identity\r\nContent-Length: 2\r\nConnection: close\r\n\r\nab").contains("\"len\":2"));
    // A small body that would grow past --max-body once decompressed.
    let mut bomb = GzEncoder::new(Vec::new(), flate2::Compression::best());
    bomb.write_all(&vec![0u8; 11 << 20]).unwrap();
    let bomb = bomb.finish().unwrap();
    let mut req = format!("POST /echo HTTP/1.1\r\nHost: x\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n", bomb.len()).into_bytes();
    req.extend_from_slice(&bomb);
    assert!(raw(s.port, &req).starts_with("HTTP/1.1 413"));
}

#[test]
fn markup_takes_html_as_written_and_says_what_it_does_not() {
    let (s, _d) = served("markup_takes_html_as_wri");
    let get = |p: &str| send(s.port, "GET", p, &[], "");
    assert_eq!(get("/c").body, "<div><p class=\"a b\" id=\"top\">a</p><i data-x=\"1\"></i></div>", "Q-448 and Q-449");
    assert_eq!(get("/map").header("content-type"), Some("application/xml; charset=utf-8"), "Q-451");
    assert_eq!(get("/feed").header("content-type"), Some("application/rss+xml; charset=utf-8"));
    assert_eq!(get("/svg").header("content-type"), Some("image/svg+xml"));
    let md = get("/md");
    assert!(md.body.starts_with("<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">") && md.body.contains("<title>Hello</title>") && md.body.contains("<h1>Hello</h1>"), "{}", md.body);
    assert!(get("/md2").body.contains("<title>T</title>"));
    let d = dir(
        "markup",
        &[
            ("doc.qwe", b"route GET / = <!DOCTYPE html>\n<html><body>hi</body></html>\n"),
            ("doc2.qwe", b"route GET / = <!doctype html><p>no</p>\n"),
            ("cm.qwe", b"route GET / = <div><!-- never closed </div>\n"),
            ("q.qwe", b"route GET / = <p class='never closed>a</p>\n"),
            ("ret.qwe", b"route GET / = <pre>x { return \"hidden\" } y</pre>\n"),
            ("caps.qwe", b"route GET / = <DIV>a</DIV>\n"),
            ("md.qwe", b"route GET / = markdown_page(\"x\", { nope: 1 })\n"),
        ],
    );
    let ok = quillo(&d, &["check", "doc.qwe"]);
    assert!(ok.status.success(), "{}", stderr(&ok));
    assert!(stderr(&quillo(&d, &["check", "doc2.qwe"])).contains("goes before an <html> element"));
    assert!(stderr(&quillo(&d, &["check", "cm.qwe"])).contains("`<!--` is never closed with `-->`"));
    assert!(stderr(&quillo(&d, &["check", "q.qwe"])).contains("never closed with `'`"));
    assert!(stderr(&quillo(&d, &["check", "ret.qwe"])).contains("`{ return ... }` in <pre> leaves the function"));
    assert!(stderr(&quillo(&d, &["check", "caps.qwe"])).contains("HTML tags are written in lowercase here, <div>"));
    let s2 = support::serve_unchecked(&d.join("md.qwe"));
    assert_eq!(send(s2.port, "GET", "/", &[], "").status, 500);
}

#[test]
fn one_xx_statuses_are_refused() {
    let d = dir("1xx", &[("a.qwe", b"route GET / = respond(103, \"\")\n"), ("b.qwe", b"route GET / = reply 101\n"), ("c.qwe", b"let n = 150\nroute GET / = respond(n, \"\")\n")]);
    assert!(stderr(&quillo(&d, &["check", "a.qwe"])).contains("`respond` status must be 200-599, got 103: a 1xx is an interim answer"), "Q-433");
    assert!(stderr(&quillo(&d, &["check", "b.qwe"])).contains("reply status must be an HTTP status code (200-599), got 101"));
    let s = support::serve(&d.join("c.qwe"), &[]);
    let r = raw(s.port, b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    assert!(r.starts_with("HTTP/1.1 500"), "a status made at run time: {}", r);
}

#[test]
fn events_carry_ids_and_retry() {
    let (s, _d) = served("events_carry_ids_and_ret");
    let r = send(s.port, "GET", "/ev", &[], "");
    assert!(r.body.contains("retry: 1500\nid: 7\nevent: tick\ndata: {\"n\":1}\n\n"), "Q-444: {:?}", r.body);
    assert!(r.body.contains("id: 8\n\n") && r.body.contains("event: plain\ndata: 2"), "{:?}", r.body);
    let d = dir("ev-bad", &[("main.qwe", b"route GET /a = events((send) => { send(1, { nope: 1 }) })\nroute GET /b = events((send) => { send(1, { retry: \"x\" }) })\n")]);
    let s = support::serve_unchecked(&d.join("main.qwe"));
    assert!(!send(s.port, "GET", "/a", &[], "").body.contains("data"));
    assert!(!send(s.port, "GET", "/b", &[], "").body.contains("data"));
}

/// A server standing in for other sites: repeated headers, a 103, redirects, codings, events.
fn peer() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            std::thread::spawn(move || peer_answer(c, port));
        }
    });
    port
}

fn peer_answer(mut c: TcpStream, port: u16) {
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        match c.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let len = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
    while buf.len() < head_end + len {
        match c.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let body = String::from_utf8_lossy(&buf[head_end..]).into_owned();
    let mut first = head.lines().next().unwrap_or("").split(' ');
    let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
    let auth = head.lines().find_map(|l| l.strip_prefix("authorization: ").or_else(|| l.strip_prefix("Authorization: "))).unwrap_or("none").to_string();
    let out = |status: &str, headers: &[&str], body: &[u8]| -> Vec<u8> {
        let mut o = format!("HTTP/1.1 {}\r\n", status).into_bytes();
        for h in headers {
            o.extend_from_slice(h.as_bytes());
            o.extend_from_slice(b"\r\n");
        }
        o.extend_from_slice(format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes());
        o.extend_from_slice(body);
        o
    };
    let gz = |b: &[u8]| {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(b).unwrap();
        e.finish().unwrap()
    };
    let reply = match path.as_str() {
        "/cookies" => out("200 OK", &["Set-Cookie: a=1; Expires=Wed, 21 Oct 2026 07:28:00 GMT", "Set-Cookie: b=2", "Link: </x>; rel=a", "Link: </y>; rel=b"], b"ok"),
        "/early" => {
            let mut o = b"HTTP/1.1 103 Early Hints\r\nLink: </a.css>; rel=preload\r\n\r\nHTTP/1.1 103 Early Hints\r\n\r\n".to_vec();
            o.extend(out("200 OK", &["X-Final: 1"], b"after hints"));
            o
        }
        "/early-gz" => {
            let mut o = b"HTTP/1.1 103 Early Hints\r\n\r\n".to_vec();
            o.extend(out("200 OK", &["Content-Encoding: gzip"], &gz(b"zipped after hints")));
            o
        }
        "/early-chunked" => b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n".to_vec(),
        "/early-closed" => b"HTTP/1.1 103 Early Hints\r\n\r\n".to_vec(),
        "/early-bad" => b"HTTP/1.1 103 Early Hints\r\n\r\nnot http at all\r\n\r\n".to_vec(),
        "/same-host" => out("301 Moved Permanently", &["Location: /needs-auth"], b""),
        "/other-host" => out("301 Moved Permanently", &[&format!("Location: http://localhost:{}/needs-auth", port)], b""),
        "/needs-auth" => out(if auth == "none" { "401 No" } else { "200 OK" }, &[], auth.as_bytes()),
        "/r307" | "/r308" => out(if path == "/r307" { "307 Temporary Redirect" } else { "308 Permanent Redirect" }, &["Location: /landed"], b""),
        "/r307-away" => out("307 Temporary Redirect", &[&format!("Location: http://localhost:{}/landed", port)], b""),
        "/loop" => out("307 Temporary Redirect", &["Location: /loop"], b""),
        "/landed" => out("200 OK", &["Content-Type: application/json"], format!("{{\"method\":\"{}\",\"body\":{:?},\"auth\":\"{}\"}}", method, body, auth).as_bytes()),
        "/deflate" => {
            let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(b"deflated").unwrap();
            out("200 OK", &["Content-Encoding: deflate"], &e.finish().unwrap())
        }
        "/rawdeflate" => {
            let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(b"raw deflated").unwrap();
            out("200 OK", &["Content-Encoding: deflate"], &e.finish().unwrap())
        }
        "/zstd" => out("200 OK", &["Content-Encoding: zstd"], b"\x28\xb5\x2f\xfd"),
        "/events" => out("200 OK", &["Content-Type: text/event-stream"], b"id: 7\nretry: 1000\ndata: one\n\ndata: two\n\nid: 9\ndata: three\n\n"),
        _ => out("404 Not Found", &[], b"no"),
    };
    let _ = c.write_all(&reply);
}

#[test]
fn the_client_reads_answers_as_http_says() {
    let p = peer();
    let src = format!(
        r#"let u = "http://127.0.0.1:{p}"
let r = http.get(`${{u}}/cookies`)
print(r.headers["set-cookie"])
print(r.cookies, r.headers.link)
let e = http.get(`${{u}}/early`, {{}}, {{ timeout: 5 }})
print(e.status, e.body, e.headers["x-final"])
print(http.get(`${{u}}/early-gz`, {{}}, {{ timeout: 5 }}).body)
print(http.get(`${{u}}/early-chunked`, {{}}, {{ timeout: 5 }}).body)
print(error(http.get(`${{u}}/early-closed`, {{}}, {{ timeout: 5 }})))
print(error(http.get(`${{u}}/early-bad`, {{}}, {{ timeout: 5 }})))
print(http.get(`${{u}}/same-host`, {{ authorization: "Bearer k" }}).body)
print(http.get(`${{u}}/other-host`, {{ authorization: "Bearer k" }}).body)
print(http.post(`${{u}}/r307`, {{ a: 1 }}, {{ authorization: "Bearer k" }}).json)
print(http.put(`${{u}}/r308`, "x").json)
print(http.post(`${{u}}/r307-away`, "y", {{ authorization: "Bearer k", cookie: "c=1" }}).json)
print(http.post(`${{u}}/r307`, {{ a: 1 }}, {{}}, {{ redirects: 0 }}).status)
print(error(http.post(`${{u}}/loop`, "z", {{}}, {{ redirects: 2 }})))
print(http.get(`${{u}}/deflate`).body, http.get(`${{u}}/rawdeflate`).body)
print(error(http.get(`${{u}}/zstd`)))
http.stream(`${{u}}/events`, (data, name, id, retry) => {{
    print(data, name, id, retry)
}})
let s = http.stream(`${{u}}/cookies`, (x) => x)
print(s.cookies)
"#
    );
    let d = dir("client", &[("main.qwe", src.as_bytes())]);
    let o = quillo(&d, &["run", "main.qwe"]);
    let out = stdout(&o);
    let want = [
        "a=1; Expires=Wed, 21 Oct 2026 07:28:00 GMT\nb=2",
        "[\"a=1; Expires=Wed, 21 Oct 2026 07:28:00 GMT\", \"b=2\"] </x>; rel=a, </y>; rel=b",
        "200 after hints 1",
        "zipped after hints",
        "hello world",
        "the connection closed after an interim (1xx) answer, with no final one",
        "a bad answer after an interim (1xx) one",
        "Bearer k",
        "none",
        "{ method: \"POST\", body: \"{\\\"a\\\":1}\", auth: \"Bearer k\" }",
        "{ method: \"PUT\", body: \"x\", auth: \"none\" }",
        "{ method: \"POST\", body: \"y\", auth: \"none\" }",
        "307",
        "more than 2 redirects",
        "deflated raw deflated",
        "the answer is in a coding Quillo cannot read: content-encoding zstd",
        "one null 7 1000",
        "two null 7 null",
        "three null 9 null",
        "[\"a=1; Expires=Wed, 21 Oct 2026 07:28:00 GMT\", \"b=2\"]",
    ];
    for w in want {
        assert!(out.contains(w), "missing {:?} in:\n{}\n{}", w, out, stderr(&o));
    }
}

#[test]
fn the_client_takes_certificates_and_proxies() {
    let d = dir(
        "tls",
        &[(
            "main.qwe",
            br#"print(error(http.get("https://127.0.0.1:1/", {}, { ca: "nope.pem" })))
print(error(http.get("https://127.0.0.1:1/", {}, { ca: "empty.pem" })))
print(error(http.get("https://127.0.0.1:1/", {}, { cert: "c.pem", key: "k.pem" })))
print(error(http.get("https://127.0.0.1:1/", {}, { ca: "system", timeout: 2 })))
"#,
        ), ("empty.pem", b"")],
    );
    let out = stdout(&quillo(&d, &["run", "main.qwe"]));
    assert!(out.contains("ca ./nope.pem: I/O error") || out.contains("ca ./nope.pem"), "{}", out);
    assert!(out.contains("no certificate in it"), "{}", out);
    assert!(out.contains("cert ./c.pem"), "{}", out);
    let bad = dir("tls-bad", &[("a.qwe", b"http.get(\"https://x/\", {}, { cert: \"c.pem\" })\n"), ("b.qwe", b"http.get(\"https://x/\", {}, { ca: 3 })\n")]);
    assert!(stderr(&quillo(&bad, &["run", "a.qwe"])).contains("`cert` and `key` go together"));
    assert!(stderr(&quillo(&bad, &["run", "b.qwe"])).contains("`ca` is the path of a PEM file or \"system\""));
    // A proxy from the environment, but not for the machine itself or NO_PROXY's hosts.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = l.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let mut b = [0u8; 2048];
            let n = c.read(&mut b).unwrap_or(0);
            s2.lock().unwrap().push(String::from_utf8_lossy(&b[..n]).lines().next().unwrap_or("").to_string());
            let _ = c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nproxied");
        }
    });
    let p = peer();
    let src = format!(
        "print(http.get(\"http://example.test/a\", {{}}, {{ timeout: 3 }}).body)\nprint(http.get(\"http://127.0.0.1:{}/cookies\").body)\nprint(error(http.get(\"http://skip.example.test/a\", {{}}, {{ timeout: 3 }})) != null)\n",
        p
    );
    let pd = dir("proxy", &[("main.qwe", src.as_bytes())]);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe"])
        .current_dir(&pd)
        .env("HTTP_PROXY", format!("http://127.0.0.1:{}", proxy))
        .env("NO_PROXY", "skip.example.test, .other.test:80")
        .output()
        .unwrap();
    let out = stdout(&o);
    assert!(out.starts_with("proxied\nok\ntrue"), "{}\n{}", out, stderr(&o));
    assert!(seen.lock().unwrap().iter().any(|l| l.contains("example.test")), "{:?}", seen.lock().unwrap());
}

#[test]
fn the_command_line_says_what_a_file_is_and_serves_a_folder() {
    let d = dir("cli", &[("page.html", b"<p>x</p>"), ("notes.md", b"# x"), ("x.css", b""), ("t.rs", b""), ("site/index.html", b"<p>home</p>"), ("site/about.html", b"about")]);
    assert!(stderr(&quillo(&d, &["run", "page.html"])).contains("page.html is HTML, not a Quillo program: serve its folder as it is with `quillo serve .`"), "Q-450");
    assert!(stderr(&quillo(&d, &["run", "notes.md"])).contains("notes.md is Markdown, not a Quillo program"));
    assert!(stderr(&quillo(&d, &["run", "x.css"])).contains("x.css is not a Quillo program"));
    assert!(stderr(&quillo(&d, &["run", "t.rs"])).contains("t.rs is Rust, not a Quillo program: a program uses it with `use \"t.rs\"`"));
    assert!(stderr(&quillo(&d, &["run", "site"])).contains("site has no main.qwe; name a .qwe file; to serve its files as they are, `quillo serve site`"));
    assert!(stderr(&quillo(&d, &["serve", "nowhere"])).contains("nowhere is not a folder"));
    assert!(stderr(&quillo(&d, &["serve", "a", "b"])).contains("`serve` takes one folder"));
    assert!(stdout(&quillo(&d, &["serve", "--help"])).contains("quillo serve <folder>"));
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["serve", "site", "--port", &port.to_string()]).current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let _g = support::Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(send(port, "GET", "/", &[], "").body, "<p>home</p>");
    assert_eq!(send(port, "GET", "/about", &[], "").body, "about");
}

#[test]
fn a_slow_body_is_taken_while_it_keeps_coming_and_a_stalled_one_is_told() {
    // 30 s each: they run side by side.
    let (s, d) = served("a_slow_body_is_taken_whi");
    let port = s.port;
    let steady = std::thread::spawn(move || {
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        c.write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: 330000\r\n\r\n").unwrap();
        for _ in 0..33 {
            c.write_all(&[b'a'; 10_000]).unwrap();
            std::thread::sleep(Duration::from_millis(1000));
        }
        let mut out = String::new();
        let _ = c.read_to_string(&mut out);
        out
    });
    let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    c.write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 100\r\n\r\nabc").unwrap();
    let t = Instant::now();
    let mut out = String::new();
    let _ = c.read_to_string(&mut out);
    assert!(out.starts_with("HTTP/1.1 408"), "Q-432: {}", out);
    assert!(t.elapsed() >= Duration::from_secs(29), "{:?}", t.elapsed());
    let steady = steady.join().unwrap();
    assert!(steady.contains("{\"len\":330000"), "a body that keeps coming is taken: {}", &steady[..steady.len().min(200)]);
    drop(d);
}
