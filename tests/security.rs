//! Requests that try to make a program do what its author did not write.

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;

fn app(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "test.qwe") {
        panic!("{}", e);
    }
    it
}

fn send(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "localhost:8080".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: h, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
}

#[test]
fn spread_data_cannot_add_event_handlers_or_break_out_of_the_tag() {
    let mut it = app(r#"route POST /a = <a {...request.json}>"hi"</a>"#);
    let body = r#"{"href":"/ok","title":"t","onmouseover":"alert(1)","ONCLICK":"x","srcdoc":"<b>","x><script>alert(2)</script><b y":"1"}"#;
    let r = send(&mut it, "POST", "/a", &[("Content-Type", "application/json")], body);
    let html = r.text();
    assert!(html.starts_with(r#"<a href="/ok" title="t">"#), "{}", html);
    assert!(!html.contains("<script") && !html.to_lowercase().contains("onmouseover") && !html.to_lowercase().contains("onclick"), "{}", html);
    assert!(!html.contains("srcdoc"), "{}", html);
}

#[test]
fn script_urls_are_not_rendered() {
    let mut it = app("route GET /l ? { u: str } = <a href={u}>\"x\"</a>");
    for u in ["javascript:alert(1)", "%20JaVaScRiPt:alert(1)", "java%09script:alert(1)", "vbscript:x"] {
        let r = send(&mut it, "GET", &format!("/l?u={}", u), &[], "");
        assert!(r.text().contains(r#"href="about:blank#blocked""#), "{} -> {}", u, r.text());
    }
    let r = send(&mut it, "GET", "/l?u=/notes", &[], "");
    assert!(r.text().contains(r#"href="/notes""#), "{}", r.text());
}

#[test]
fn request_text_cannot_add_response_headers() {
    let mut it = app(
        r#"
route GET /h ? { v: str } = respond(200, "ok", { "x-echo": v })
route GET /go ? { to: str } = redirect(to)
route GET /status = redirect("/", 70000)
"#,
    );
    for url in ["/h?v=a%0d%0aset-cookie:%20x=1", "/go?to=/x%0d%0aset-cookie:%20x=1", "/go?to=/x%0aset-cookie:%20x=1"] {
        let r = send(&mut it, "GET", url, &[], "");
        assert_eq!(r.status, 500, "{}", url);
        assert!(r.header("set-cookie").is_none(), "{}", url);
    }
    assert_eq!(send(&mut it, "GET", "/h?v=fine", &[], "").header("x-echo"), Some("fine"));
    assert_eq!(send(&mut it, "GET", "/status", &[], "").status, 500);
    assert!(quillo::server::valid_header("x-a", "b\tc"));
    assert!(!quillo::server::valid_header("x a", "b"));
    assert!(!quillo::server::valid_header("x-a", "b\r\nc"));
}

#[test]
fn server_fns_answer_only_the_programs_own_pages() {
    let mut it = app("server fn add(n: int) -> int = n + 1");
    let call = |it: &mut Interp, headers: &[(&str, &str)]| send(it, "POST", "/_quillo/rpc/add", headers, "[41]").status;
    // Content types a page on another site can send without asking.
    assert_eq!(call(&mut it, &[("Content-Type", "text/plain; x=application/json")]), 415);
    assert_eq!(call(&mut it, &[("Content-Type", "text/plain")]), 415);
    let json = ("Content-Type", "application/json");
    assert_eq!(call(&mut it, &[json, ("Sec-Fetch-Site", "cross-site")]), 403);
    assert_eq!(call(&mut it, &[json, ("Sec-Fetch-Site", "same-site")]), 403);
    assert_eq!(call(&mut it, &[json, ("Origin", "https://evil.example")]), 403);
    assert_eq!(call(&mut it, &[json, ("Origin", "null")]), 403);
    assert_eq!(call(&mut it, &[json, ("Sec-Fetch-Site", "same-origin"), ("Origin", "http://localhost:8080")]), 200);
    assert_eq!(call(&mut it, &[("Content-Type", "application/json; charset=utf-8"), ("Origin", "http://localhost:8080")]), 200);
    // Scripts and tests send neither header.
    assert_eq!(call(&mut it, &[json]), 200);
}

#[test]
fn other_sites_cannot_post_forms_here() {
    let mut it = app(
        r#"
allow_cross_site("/sso")
route POST /notes = "saved"
route POST /sso/callback = "signed in"
route GET /notes = "list"
"#,
    );
    let form = ("Content-Type", "application/x-www-form-urlencoded");
    // Another site's page, by the browser's word or by Origin alone.
    for from in [[form, ("Sec-Fetch-Site", "cross-site")], [form, ("Origin", "https://evil.example")]] {
        let r = send(&mut it, "POST", "/notes", &from, "text=hi");
        assert_eq!(r.status, 403, "{:?}", from);
    }
    for ctype in ["multipart/form-data; boundary=x", "text/plain", ""] {
        let r = send(&mut it, "POST", "/notes", &[("Content-Type", ctype), ("Sec-Fetch-Site", "cross-site")], "x");
        assert_eq!(r.status, 403, "{}", ctype);
    }
    // The site's own pages, and programs that send no Origin.
    assert_eq!(send(&mut it, "POST", "/notes", &[form, ("Sec-Fetch-Site", "same-origin")], "text=hi").text(), "saved");
    assert_eq!(send(&mut it, "POST", "/notes", &[form, ("Origin", "http://localhost:8080")], "text=hi").text(), "saved");
    assert_eq!(send(&mut it, "POST", "/notes", &[form], "text=hi").text(), "saved");
    // JSON from another site needs the browser to ask first; reading is fine.
    assert_eq!(send(&mut it, "POST", "/notes", &[("Content-Type", "application/json"), ("Sec-Fetch-Site", "cross-site")], "{}").text(), "saved");
    assert_eq!(send(&mut it, "GET", "/notes", &[("Sec-Fetch-Site", "cross-site")], "").text(), "list");
    // A path the program allows.
    assert_eq!(send(&mut it, "POST", "/sso/callback", &[form, ("Sec-Fetch-Site", "cross-site")], "x=1").text(), "signed in");
    let mut bad = Interp::new(env!("CARGO_MANIFEST_DIR"));
    let e = bad.load_source("allow_cross_site(\"sso\")", "t.qwe").err().expect("a path without / is refused");
    assert!(e.to_string().contains("starts with /"), "{}", e);
}

#[test]
fn json_patterns_need_a_json_body() {
    let mut it = app(r#"route POST /order { json: { type: "order" } } = "accepted""#);
    let body = r#"{"type":"order"}"#;
    assert_eq!(send(&mut it, "POST", "/order", &[("Content-Type", "text/plain")], body).status, 400);
    assert_eq!(send(&mut it, "POST", "/order", &[("Content-Type", "application/x-www-form-urlencoded")], body).status, 400);
    assert_eq!(send(&mut it, "POST", "/order", &[("Content-Type", "application/json")], body).status, 200);
    assert_eq!(send(&mut it, "POST", "/order", &[], body).status, 200);
}

#[test]
fn errors_do_not_show_the_program_to_the_client() {
    let mut it = app("route GET /bad = 1 + \"x\" * 2");
    let r = send(&mut it, "GET", "/bad", &[], "");
    assert_eq!((r.status, r.text().as_str()), (500, "internal error\n"));
}

#[test]
fn uuids_are_version_4() {
    let v = quillo::eval_str("[uuid(), uuid()]").unwrap().repr();
    let ids: Vec<&str> = v.trim_matches(|c| c == '[' || c == ']').split(", ").map(|s| s.trim_matches('"')).collect();
    assert_ne!(ids[0], ids[1]);
    for id in ids {
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!("89ab".contains(&id[19..20]));
    }
}

#[test]
fn url_attributes_are_checked_however_they_are_written() {
    let mut it = app(r#"route GET /a = <a {...request.query} HREF={request.query.u}>x</a>"#);
    let r = send(&mut it, "GET", "/a?HREF=javascript:alert(1)&FormAction=JavaScript:x&u=javascript:y", &[], "");
    let body = r.text().to_lowercase();
    assert!(!body.contains("href=\"javascript:") && !body.contains("formaction=\"javascript:"), "{}", body);
    assert_eq!(body.matches("about:blank#blocked").count(), 2, "{}", body);
}

#[test]
fn server_fn_arguments_are_checked_all_the_way_in() {
    let mut it = app(
        r#"
type Role = "viewer" | "editor"
type Item = { name: str, qty: int }
server fn grant(roles: list<Role>) -> int = len(roles)
server fn total(ns: list<int>) -> int = sum(ns)
server fn order(items: list<Item>) -> int = len(items)
server fn tagged(m: { tags: list<str> }) -> int = len(m.tags)
"#,
    );
    let call = |it: &mut Interp, f: &str, body: &str| send(it, "POST", &format!("/_quillo/rpc/{}", f), &[("Content-Type", "application/json")], body);
    // What the review sent: a role the type does not have, text for a number,
    // an item missing a field, a list item of the wrong type in a map.
    for (f, body) in [("grant", r#"[["admin","owner"]]"#), ("total", r#"[["1) or 1=1 --", 2]]"#), ("order", r#"[[{"name":"x"}]]"#), ("tagged", r#"[{"tags":[1]}]"#)] {
        let r = call(&mut it, f, body);
        assert_eq!(r.status, 400, "{} {} -> {}", f, body, r.text());
    }
    assert!(call(&mut it, "grant", r#"[["admin"]]"#).text().contains("[0]"), "the error says where");
    for (f, body, want) in [("grant", r#"[["viewer","editor"]]"#, "2"), ("total", "[[1, 2]]", "3"), ("order", r#"[[{"name":"x","qty":1}]]"#, "1"), ("tagged", r#"[{"tags":["a"]}]"#, "1")] {
        let r = call(&mut it, f, body);
        assert_eq!((r.status, r.text()), (200, format!(r#"{{"ok":{}}}"#, want)), "{} {}", f, body);
    }
}

// ---- the server itself, over a socket ------------------------------------------------------

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Ports outside the range the system gives clients, so that a client's
/// connection cannot take a server's port before it listens.
fn port() -> u16 {
    static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
    40000 + (std::process::id() % 900) as u16 * 10 + NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

fn serve(name: &str, src: &str, extra: &[&str]) -> Server {
    let d = std::env::temp_dir().join(format!("quillo-security-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("app.qwe"), src).unwrap();
    let port = port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("app.qwe"))
        .args(["--port", &port.to_string()])
        .args(extra)
        .stderr(Stdio::null())
        .spawn()
        .expect("cannot start quillo");
    let deadline = Instant::now() + Duration::from_secs(20);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    Server { child, port }
}

/// Sends `raw` and reads the answer until the server closes (or 10 s pass).
fn exchange(port: u16, raw: &[u8]) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let _ = s.write_all(raw);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

fn get_ok(port: u16) -> bool {
    exchange(port, b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").starts_with("HTTP/1.1 200")
}

/// The server's resident memory, in KiB.
#[cfg(unix)]
fn rss_kib(srv: &Server) -> u64 {
    let out = Command::new("ps").args(["-o", "rss=", "-p", &srv.child.id().to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0)
}

const ECHO: &str = "route GET / = \"up\"\nroute POST /echo = len(request.body)\n";

#[test]
fn a_chunk_size_that_overflows_does_not_stop_the_server() {
    let srv = serve("overflow", ECHO, &[]);
    // The request from the review: a size that wrapped around the checks.
    let r = exchange(srv.port, b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nA\r\nffffffffffffffff\r\nBB");
    assert!(r.starts_with("HTTP/1.1 413"), "{}", r);
    // Longer than any size can be.
    let r = exchange(srv.port, b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n00000000000000000001\r\nA\r\n0\r\n\r\n");
    assert!(r.starts_with("HTTP/1.1 400"), "{}", r);
    assert!(get_ok(srv.port), "the server stopped");
    // A chunked body still works, and so does the connection after it.
    let r = exchange(
        srv.port,
        b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5;x=1\r\npedia\r\n0\r\nX-T: 1\r\n\r\nGET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert!(r.starts_with("HTTP/1.1 200") && r.contains("\r\n\r\n9") && r.ends_with("up"), "{}", r);
}

#[cfg(unix)]
#[test]
fn endless_chunk_trailers_and_extensions_are_refused() {
    let srv = serve("trailers", ECHO, &[]);
    let before = rss_kib(&srv);
    // Trailer lines without end: refused once there are too many, not kept.
    let mut s = TcpStream::connect(("127.0.0.1", srv.port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nA\r\n0\r\n").unwrap();
    let line = format!("X-T: {}\r\n", "y".repeat(1000));
    let mut sent = 0;
    for _ in 0..20_000 {
        if s.write_all(line.as_bytes()).is_err() {
            break;
        }
        sent += line.len();
    }
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out);
    assert!(out.starts_with("HTTP/1.1 400"), "after {} bytes: {}", sent, out);
    // 1-byte chunks with long extensions, the same.
    let mut body = b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    for _ in 0..1000 {
        body.extend_from_slice(format!("1;{}\r\nA\r\n", "e".repeat(500)).as_bytes());
    }
    assert!(exchange(srv.port, &body).starts_with("HTTP/1.1 400"));
    let grew = rss_kib(&srv).saturating_sub(before);
    assert!(grew < 32 * 1024, "memory grew by {} KiB", grew);
    assert!(get_ok(srv.port));
}

/// The CPU time the server has used, in seconds.
#[cfg(unix)]
fn cpu_secs(srv: &Server) -> f64 {
    let out = Command::new("ps").args(["-o", "time=", "-p", &srv.child.id().to_string()]).output().unwrap();
    // [[dd-]hh:]mm:ss[.cc]
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    t.rsplit(|c| c == ':' || c == '-').zip([1.0, 60.0, 3600.0, 86400.0]).map(|(p, k)| p.parse::<f64>().unwrap_or(0.0) * k).sum()
}

#[cfg(unix)]
#[test]
fn chunked_bodies_are_decoded_once_however_they_arrive() {
    let srv = serve("trickle", ECHO, &["--max-body", "20000000"]);
    let mut s = TcpStream::connect(("127.0.0.1", srv.port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    s.write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
    // 4 MB in small chunks, then a byte at a time: each read used to decode
    // (and copy) the whole body again.
    let chunk = format!("40\r\n{}\r\n", "z".repeat(64));
    s.write_all(chunk.repeat(65_536).as_bytes()).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let before = cpu_secs(&srv);
    for _ in 0..1500 {
        s.write_all(b"1\r\nz\r\n").unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    s.write_all(b"0\r\n\r\n").unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 200") && out.ends_with(&(65_536 * 64 + 1500).to_string()), "{}", &out[..out.len().min(300)]);
    let used = cpu_secs(&srv) - before;
    assert!(used < 1.0, "the server used {:.2}s of CPU for 1500 small reads", used);
}

/// The server's count of open connections (`--metrics`), not counting the
/// one asking.
fn connections(port: u16) -> i64 {
    let r = exchange(port, b"GET /_quillo/metrics HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    let n = r.lines().find_map(|l| l.strip_prefix("quillo_connections ")).and_then(|n| n.trim().parse::<i64>().ok());
    n.expect("no connection count") - 1
}

#[test]
fn a_client_that_never_reads_its_answer_is_let_go() {
    let srv = serve("never-reads", "route GET /big = repeat(\"x\", 64000000)\n", &["--metrics"]);
    // Asks for a large answer, and reads none of it.
    let mut s = TcpStream::connect(("127.0.0.1", srv.port)).unwrap();
    s.write_all(b"GET /big HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(connections(srv.port), 1);
    // After 30 seconds without taking any, the connection (and the answer
    // it holds) is closed.
    let deadline = Instant::now() + Duration::from_secs(45);
    while connections(srv.port) > 0 {
        assert!(Instant::now() < deadline, "the connection is still open");
        std::thread::sleep(Duration::from_secs(1));
    }
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut got = 0usize;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => got += n,
        }
    }
    assert!(got < 64_000_000, "read the whole answer ({} bytes)", got);
}
