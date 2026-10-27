//! Requests that try to make a program do what its author did not write.

mod support;

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
fn svg_animations_cannot_give_an_href_a_script_url() {
    // What the review sent: `<set attributeName="href" to={u}>`; `values`
    // is a list, any of which may be the URL.
    let mut it = app(
        r#"
route GET /s ? { u: str } = <svg><a>
    <set attributeName="href" to={u}/>
    <animate attributeName="xlink:href" values={`/a;${u}`} from={u}/>
    <animate attributeName="href" {...{ by: u }} begin="0s"/>
    <text>x</text>
</a></svg>
"#,
    );
    let body = send(&mut it, "GET", "/s?u=%20JaVaScRiPt:alert(1)", &[], "").text();
    assert!(!body.to_lowercase().contains("javascript:"), "{}", body);
    assert_eq!(body.matches("about:blank#blocked").count(), 4, "{}", body);
    let body = send(&mut it, "GET", "/s?u=/notes", &[], "").text();
    assert!(body.contains(r#"to="/notes""#) && body.contains(r#"values="/a;/notes""#) && !body.contains("blocked"), "{}", body);
}

#[test]
fn html_values_in_url_attributes_are_checked_too() {
    // What the review sent: an `html` value, which was not looked at.
    let mut it = app(
        r#"
route GET /m ? { u: str } = <a href={markdown(u, { inline: true })}>x</a>
route GET /r ? { u: str } = <a {...{ href: raw(u) }} src={raw(u)}>x</a>
"#,
    );
    for path in ["/m", "/r"] {
        let body = send(&mut it, "GET", &format!("{}?u=javascript:alert(1)", path), &[], "").text();
        assert!(!body.to_lowercase().contains("javascript:"), "{} -> {}", path, body);
        assert!(body.contains("about:blank#blocked"), "{} -> {}", path, body);
        let body = send(&mut it, "GET", &format!("{}?u=/notes", path), &[], "").text();
        assert!(body.contains("/notes") && !body.contains("blocked"), "{} -> {}", path, body);
    }
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

#[test]
fn server_fn_record_arguments_keep_only_their_own_fields() {
    let mut it = app(
        r#"
type Profile = { name: str, role: str = "member" }
server fn save(p: Profile) = json.stringify(p)
server fn many(ps: list<Profile>) = json.stringify(ps)
"#,
    );
    let call = |it: &mut Interp, f: &str, body: &str| send(it, "POST", &format!("/_quillo/rpc/{}", f), &[("Content-Type", "application/json")], body);
    // What the review sent: a field the record does not have, as a
    // `{ json: Profile }` route pattern would leave out.
    let r = call(&mut it, "save", r#"[{"name":"ada","is_admin":true}]"#);
    assert_eq!(r.status, 200, "{}", r.text());
    assert!(!r.text().contains("is_admin") && r.text().contains("member"), "{}", r.text());
    let r = call(&mut it, "many", r#"[[{"name":"ada","role":"x","is_admin":true}]]"#);
    assert_eq!(r.status, 200, "{}", r.text());
    assert!(!r.text().contains("is_admin") && r.text().contains("ada"), "{}", r.text());
}

#[test]
fn values_in_a_script_cannot_end_it() {
    let mut it = app(
        r#"
route GET /s ? { x: str } = <html><body><script>const d = {x}; const n = {[1, x]}; const r = {raw(json.stringify({ a: x }))};</script><p>after</p></body></html>
"#,
    );
    // What the review sent: text that ends the script and starts another.
    let x = "%3C/script%3E%3Cscript%3Ealert(1)%3C/script%3E%3C!--%E2%80%A8";
    let html = send(&mut it, "GET", &format!("/s?x={}", x), &[], "").text();
    let script = html.split_once("<script>").map(|(_, s)| s).unwrap_or("");
    let (code, rest) = script.split_once("</script>").expect("one script");
    assert!(rest.starts_with("<p>after</p>"), "{}", html);
    assert!(!code.contains("<!--"), "{}", code);
    // A value is JSON, so a JS string; raw text keeps what it says, with `</` as `<\/`.
    assert!(code.starts_with(r#"const d = "\u003c/script>\u003cscript>alert(1)\u003c/script>\u003c!--\u2028";"#), "{}", code);
    assert!(code.contains(r#"const n = [1,"\u003c/script>"#), "{}", code);
    assert!(code.contains(r#"const r = {"a":"<\/script><script>alert(1)<\/script><\!--"#), "{}", code);
    // A component's script, filled in the browser, would run what fills it.
    let e = Interp::new(env!("CARGO_MANIFEST_DIR")).load_source("component C(x: str) = <div><script>{x}</script></div>", "t.qwe").err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("<script> takes only text written in the program"), "{}", e);
}

#[test]
fn text_naming_the_bundle_does_not_stop_components_loading() {
    let mut it = app(
        r#"
component Counter() {
    state n = 0
    <button @click={() => { n += 1 }}>{n}</button>
}
route GET /c ? { note: str } = <html><body><p>{note}</p>{markdown(note)}<Counter/></body></html>
route GET /own = <html><head><script src="/_quillo/quillo.js" defer></script></head><body><Counter/></body></html>
"#,
    );
    // What the review sent: a comment saying where the bundle is, which
    // stopped the page loading it. Markdown cannot write the element either.
    for note in ["see%20/_quillo/quillo.js", "![x](/_quillo/quillo.js)", "%3Cscript%20src=%22/_quillo/quillo.js%22%3E"] {
        let page = send(&mut it, "GET", &format!("/c?note={}", note), &[], "").text();
        assert!(page.contains("t.src=\"/_quillo/b/"), "{} -> {}", note, page);
    }
    // A page that loads it itself is left as it is.
    let page = send(&mut it, "GET", "/own", &[], "").text();
    assert!(!page.contains("t.src="), "{}", page);
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
    support::free_port()
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
    support::stop_with_tests(&child);
    let server = Server { child, port };
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    server
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

#[test]
fn request_bodies_are_framed_one_way_only() {
    let srv = serve("framing", ECHO, &[]);
    // What the review sent, each accepted then: a signed length, sizes with
    // a sign or spaces, both a length and chunked (on a kept connection),
    // chunked not last, and chunked on HTTP/1.0. Each is refused, and the
    // request after it is not taken from the same connection.
    let next = "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n";
    for (head, body) in [
        ("POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: +5\r\n\r\n", "hello"),
        ("POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n", "+5\r\nhello\r\n0\r\n\r\n"),
        ("POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n", " 5 \r\nhello\r\n0\r\n\r\n"),
        ("POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n", "5\r\nhello\r\n0\r\n\r\n"),
        ("POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked, identity\r\n\r\n", "5\r\nhello\r\n0\r\n\r\n"),
        ("POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\nTransfer-Encoding: identity\r\n\r\n", "5\r\nhello\r\n0\r\n\r\n"),
        ("POST /echo HTTP/1.0\r\nHost: x\r\nConnection: keep-alive\r\nTransfer-Encoding: chunked\r\n\r\n", "5\r\nhello\r\n0\r\n\r\n"),
    ] {
        let r = exchange(srv.port, format!("{}{}{}", head, body, next).as_bytes());
        assert!(r.starts_with("HTTP/1.1 400"), "{:?} -> {}", head, r);
        assert_eq!(r.matches("HTTP/1.1 ").count(), 1, "{:?} -> {}", head, r);
    }
    // gzip, then chunked: framed right, but not decoded here.
    let r = exchange(srv.port, b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: gzip, chunked\r\n\r\n0\r\n\r\n");
    assert!(r.starts_with("HTTP/1.1 501"), "{}", r);
    // Written as allowed: still taken.
    let r = exchange(srv.port, format!("POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length:  5 \r\n\r\nhello{}", next).as_bytes());
    assert!(r.starts_with("HTTP/1.1 200") && r.ends_with("up"), "{}", r);
    let r = exchange(srv.port, format!("POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: Chunked\r\n\r\n5 ;a=b\r\nhello\r\n0\r\n\r\n{}", next).as_bytes());
    assert!(r.starts_with("HTTP/1.1 200") && r.contains("\r\n\r\n5") && r.ends_with("up"), "{}", r);
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

#[test]
fn scripts_cannot_reach_the_program_through_method_calls() {
    let dir = std::env::temp_dir().join(format!("quillo-script-escape-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("secret.txt"), "hunter2").unwrap();
    std::fs::write(dir.join("victim.txt"), "keep me").unwrap();
    let mut it = Interp::new(&dir);
    let src = r#"fn delete_everything(n) = "deleted"
route POST /rule { json: { code: str } } = json.stringify({ result: run_script(code, {}) })"#;
    if let Err(e) = it.load_source(src, "main.qwe") {
        panic!("{}", e);
    }
    let run = |it: &mut Interp, code: &str| {
        let body = format!("{{\"code\":{:?}}}", code);
        send(it, "POST", "/rule", &[("Content-Type", "application/json")], &body).text()
    };
    for code in [r#""HOME".env"#, r#""secret.txt".read_file"#, r#""victim.txt".delete_file"#, r#""main.qwe".write_file"#, "42.delete_everything"] {
        let out = run(&mut it, code);
        assert!(out.contains("has no field"), "{}: {}", code, out);
        assert!(!out.contains("hunter2") && !out.contains("deleted"), "{}: {}", code, out);
    }
    // The same with the program's own values handed to `map`.
    let out = run(&mut it, r#"map(["secret.txt"], (f) => f.read_file)"#);
    assert!(!out.contains("hunter2"), "{}", out);
    assert_eq!(std::fs::read_to_string(dir.join("victim.txt")).unwrap(), "keep me");
    assert!(!dir.join("main.qwe").exists());
    // A script's own values still have their methods.
    assert_eq!(run(&mut it, r#""abc".upper + [1, 2].len"#), r#"{"result":"ABC2"}"#);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn scripts_have_no_effects_of_their_own() {
    // A builtin handed over as a value does not run with effects.
    let e = quillo::eval_str(r#"run_script("e(\"HOME\")", { e: env })"#).unwrap().repr();
    assert!(e.starts_with("err(") && e.contains("env effect"), "{}", e);
    let e = quillo::eval_str(r#"run_script("map([\"Cargo.toml\"], f)", { f: read_file })"#).unwrap().repr();
    assert!(e.starts_with("err(") && e.contains("fs effect"), "{}", e);
    // A helper the program wrote keeps what the program may do, and what the
    // script hands it back runs as the script's.
    let v = quillo::eval_str(r#"fn has_home(f) = f(env("HOME")) != null
run_script("has_home((h) => h)", { has_home })"#).unwrap().repr();
    assert_eq!(v, "true");
    let e = quillo::eval_str(r#"fn call(f) = f()
run_script("call(() => e(\"HOME\"))", { call, e: env })"#).unwrap().repr();
    assert!(e.contains("env effect"), "{}", e);
}

/// Each shape of nesting, `n` deep.
fn nested_shapes(n: usize) -> Vec<(&'static str, String)> {
    vec![
        ("parens", format!("{}1{}", "(".repeat(n), ")".repeat(n))),
        ("negation", format!("{}1", "-".repeat(n))),
        ("not", format!("{}true", "!".repeat(n))),
        ("list", format!("{}1{}", "[".repeat(n), "]".repeat(n))),
        ("map", format!("{}1{}", "{ a: ".repeat(n), " }".repeat(n))),
        ("sum", vec!["1"; n].join(" + ")),
        ("fields", format!("\"x\"{}", ".upper".repeat(n))),
        ("calls", format!("\"x\"{}", ".upper()".repeat(n))),
        ("index", format!("{}1{}{}", "[".repeat(n), "]".repeat(n), "[0]".repeat(n))),
        ("blocks", format!("{}1{}", "{ let x = 1\n".repeat(n), " }".repeat(n))),
        ("ternary", format!("{}1{}", "true ? ".repeat(n), " : 2".repeat(n))),
        ("else if", format!("if false {{ 0 }}{} else {{ 1 }}", " else if false { 0 }".repeat(n))),
        ("lambda", format!("{}1{}", "(() => ".repeat(n), ")()".repeat(n))),
        ("template", format!("{}1{}", "`${".repeat(n), "}`".repeat(n))),
        ("html", format!("{}x{}", "<b>".repeat(n), "</b>".repeat(n))),
        ("pattern", format!("match [1] {{ {}_{} => 1, _ => 2 }}", "[".repeat(n), "]".repeat(n))),
        ("type", format!("((x: {}int{}) => 1)([])", "list<".repeat(n), ">".repeat(n))),
    ]
}

#[test]
fn deeply_nested_source_is_refused_not_a_crash() {
    // The review's case: 100,000 parentheses sent as a script.
    let parens = format!("{}1{}", "(".repeat(100_000), ")".repeat(100_000));
    let e = quillo::eval_str(&format!("run_script({:?}, {{}})", parens)).unwrap().repr();
    assert!(e.contains("nests too deeply"), "{:.200}", e);
    for (shape, src) in nested_shapes(10_000) {
        let e = quillo::eval_str(&format!("let x = {}", src)).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("nests too deeply"), "{}: {:.200}", shape, e);
        let e = quillo::eval_str(&format!("run_script({:?}, {{}})", src)).unwrap().repr();
        assert!(e.contains("nests too deeply"), "{}: {:.200}", shape, e);
        let a = quillo::check::check_source(&format!("let x = {}", src), "deep.qwe", std::path::Path::new("."));
        assert!(a.diagnostics.iter().any(|d| d.message.contains("nests too deeply")), "{}", shape);
    }
    // Patterns and types too.
    let n = 10_000;
    for src in [
        format!("let x = match 1 {{ {}_{} => 1 }}", "[".repeat(n), "]".repeat(n)),
        format!("fn f(x: {}int{}) = 1", "list<".repeat(n), ">".repeat(n)),
    ] {
        let e = quillo::eval_str(&src).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("nests too deeply"), "{:.200}", e);
    }
}

#[test]
fn source_nested_as_deep_as_allowed_runs_and_checks_on_a_small_stack() {
    // The deepest each shape may be: it parses, checks and runs, in a
    // function and in a script, on a thread with less stack than a request's.
    let t = std::thread::Builder::new().stack_size(512 << 10).spawn(|| {
        for (shape, _) in nested_shapes(1) {
            let mut accepted = 0;
            for n in (1..=256).rev() {
                let (_, src) = nested_shapes(n).into_iter().find(|(s, _)| *s == shape).unwrap();
                let src = format!("fn f() = {}\nlen(str(f()))", src);
                if let Ok(v) = quillo::eval_str(&src) {
                    let a = quillo::check::check_source(&src, "deep.qwe", std::path::Path::new("."));
                    assert!(!a.diagnostics.iter().any(|d| d.message.contains("too deeply")), "{}", shape);
                    assert!(v.repr().parse::<i64>().is_ok(), "{}: {}", shape, v.repr());
                    let script = format!("run_script({:?}, {{}})", format!("len(str({}))", nested_shapes(n).into_iter().find(|(s, _)| *s == shape).unwrap().1));
                    let v = quillo::eval_str(&script).unwrap().repr();
                    assert!(v.parse::<i64>().is_ok() || v.contains("nests too deeply"), "{}: {}", shape, v);
                    accepted = n;
                    break;
                }
            }
            assert!(accepted >= 60, "{} is refused from {} deep", shape, accepted + 1);
        }
    });
    t.unwrap().join().unwrap();
}

#[test]
fn check_program_reads_nothing_outside_the_program() {
    let dir = std::env::temp_dir().join(format!("quillo-check-outside-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("app")).unwrap();
    std::fs::create_dir_all(dir.join("outside")).unwrap();
    std::fs::write(dir.join("outside/.env"), "hunter2").unwrap();
    std::fs::write(dir.join("outside/m.qwe"), "let x = hunter3").unwrap();
    std::fs::write(dir.join("app/lib.qwe"), "fn helper() = 1").unwrap();
    let mut it = Interp::new(dir.join("app"));
    let src = r#"route POST /check { json: { code: str } } = json.stringify(check_program("p.qwe", { "p.qwe": code }))"#;
    if let Err(e) = it.load_source(src, "main.qwe") {
        panic!("{}", e);
    }
    let check = |it: &mut Interp, code: &str| {
        let body = format!("{{\"code\":{:?}}}", code);
        send(it, "POST", "/check", &[("Content-Type", "application/json")], &body).text()
    };
    for code in [
        r#"use "../outside/.env""#,
        r#"use "/etc/passwd""#,
        r#"mount /m = "../outside/m.qwe""#,
        r#"service billing = "../outside/m.qwe""#,
    ] {
        let out = check(&mut it, code);
        assert!(out.contains("must stay inside the program's folder"), "{}: {}", code, out);
        assert!(!out.contains("hunter") && !out.contains("root"), "{}: {}", code, out);
    }
    // Files inside it are still read.
    let out = check(&mut it, "use \"lib.qwe\"\nlet y = helper()\nprint(y)");
    assert_eq!(out, "[]");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn programs_that_use_or_mount_each_other_endlessly_are_an_error_not_a_crash() {
    let dir = std::env::temp_dir().join(format!("quillo-check-loop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.qwe"), "mount /a = \"a.qwe\"").unwrap();
    let a = quillo::check::check_file(&dir.join("a.qwe"));
    assert!(a.diagnostics.iter().any(|d| d.message.contains("more than 16 deep")), "{:?}", a.diagnostics);
    // A long chain of `use`, given as text to check.
    let files: std::collections::HashMap<_, _> =
        (0..1000).map(|i| (dir.join(format!("u{}.qwe", i)), format!("use \"u{}.qwe\"", i + 1))).collect();
    let a = quillo::check::check_file_with(&dir.join("u0.qwe"), files);
    assert!(a.diagnostics.iter().any(|d| d.message.contains("more than 64 deep")), "{:?}", a.diagnostics);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn spread_keys_cannot_become_handlers_by_bind_or_case() {
    let mut it = app(r#"route GET /q = <div {...request.query}>"hi"</div>"#);
    for q in [
        "bind:onfocus=alert(document.domain)&autofocus=1&tabindex=0",
        "BIND:onfocus=alert(1)",
        "bind:onclick=alert(1)",
        "Bind:OnClick=alert(1)",
        "bind:srcdoc=%3Cscript%3Ealert(1)%3C/script%3E",
        "bind:__x=1",
        "@click=alert(1)",
        "x:onclick=alert(1)",
    ] {
        let html = send(&mut it, "GET", &format!("/q?{}", q), &[], "").text();
        let lower = html.to_lowercase();
        assert!(!lower.contains("onfocus") && !lower.contains("onclick") && !lower.contains("srcdoc"), "{} -> {}", q, html);
        assert!(!html.contains("__x") && !html.contains('@') && !html.contains(':'), "{} -> {}", q, html);
    }
    let html = send(&mut it, "GET", "/q?title=t&xlink:href=/ok", &[], "").text();
    assert!(html.contains(r#"title="t""#) && html.contains(r#"xlink:href="/ok""#), "{}", html);
}

#[test]
fn spread_keys_in_another_case_do_not_replace_written_attributes() {
    let mut it = app(
        r#"route POST /f = <form {...request.json} action="/login" method="post">"x"</form>
route POST /a = <a {...request.json} href="/ok" class="c">"x"</a>"#,
    );
    let json = [("Content-Type", "application/json")];
    let html = send(&mut it, "POST", "/f", &json, r#"{"ACTION":"https://evil.example/steal","Method":"get","title":"t"}"#).text();
    assert!(!html.contains("evil") && !html.contains(r#""get""#), "{}", html);
    assert!(html.contains(r#"action="/login""#) && html.contains(r#"method="post""#) && html.contains(r#"title="t""#), "{}", html);
    for body in [r#"{"Href":"https://evil.example"}"#, r#"{"bind:href":"https://evil.example"}"#, r#"{"HREF":"https://evil.example"}"#] {
        let html = send(&mut it, "POST", "/a", &json, body).text();
        assert!(!html.contains("evil") && html.contains(r#"href="/ok""#), "{} -> {}", body, html);
    }
    // A spread's class is added to the written one, whatever its case.
    let html = send(&mut it, "POST", "/a", &json, r#"{"CLASS":"d"}"#).text();
    assert!(html.contains(r#"class="c d""#) && html.matches("class=").count() == 1, "{}", html);
}

#[test]
fn handlers_and_frame_pages_take_only_text_written_in_the_program() {
    let load = |src: &str| Interp::new(env!("CARGO_MANIFEST_DIR")).load_source(src, "t.qwe").err().map(|e| e.to_string()).unwrap_or_default();
    for (src, needle) in [
        ("route GET /a ? { u: str } = <div onmouseover={`go('${u}')`}>\"x\"</div>", "@mouseover"),
        ("route GET /a ? { u: str } = <div ONCLICK={u}>\"x\"</div>", "@click"),
        ("route GET /a ? { u: str } = <div bind:onclick={u}>\"x\"</div>", "@click"),
        ("route GET /a ? { onclick: str } = <div {onclick}>\"x\"</div>", "@click"),
        ("route GET /a ? { u: str } = <iframe srcdoc={u}></iframe>", "sandbox"),
        ("route GET /a ? { u: str } = <iframe sandbox=\"allow-scripts allow-same-origin\" srcdoc={u}></iframe>", "sandbox"),
    ] {
        let e = load(src);
        assert!(e.contains("takes only text written here") && e.contains(needle), "{}: {}", src, e);
        let a = quillo::check::check_source(src, "t.qwe", std::path::Path::new("."));
        assert!(a.diagnostics.iter().any(|d| d.message.contains("takes only text written here")), "{}", src);
    }
    // Text written in the program, a frame kept away from the site, and a
    // component's parameters are fine.
    let mut it = app(
        r#"view Picker(on_pick) = <p>"x"</p>
route GET /a ? { u: str } = <div>
    <button onclick="history.back()">"Back"</button>
    <iframe sandbox="" srcdoc={u}></iframe>
    <iframe sandbox="allow-scripts" srcdoc={u}></iframe>
    <Picker on_pick={(x) => x}/>
</div>"#,
    );
    let html = send(&mut it, "GET", "/a?u=%3Cb%3Ehi%3C/b%3E", &[], "").text();
    assert!(html.contains(r#"onclick="history.back()""#), "{}", html);
    assert_eq!(html.matches("srcdoc=\"&lt;b&gt;hi&lt;/b&gt;\"").count(), 2, "{}", html);
}

/// A server on this machine that answers `/go` with a redirect to `/secret`,
/// counting the connections it gets.
fn internal_server() -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = hits.clone();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut s = s;
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let answer = if req.starts_with("GET /go ") {
                format!("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{}/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", port)
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecret".to_string()
            };
            let _ = s.write_all(answer.as_bytes());
        }
    });
    (port, hits)
}

#[test]
fn http_calls_can_refuse_redirects_and_addresses_inside() {
    let (port, hits) = internal_server();
    let run = |src: String| quillo::eval_str(&src).unwrap().repr();
    // Redirects are followed, unless told not to.
    assert_eq!(run(format!("http.get(\"http://127.0.0.1:{}/go\").body", port)), "\"secret\"");
    assert_eq!(run(format!("http.get(\"http://127.0.0.1:{}/go\", {{}}, {{ redirects: 0 }}).status", port)), "302");
    let before = hits.load(std::sync::atomic::Ordering::SeqCst);
    // Only public addresses: this machine, private and link-local networks
    // (the metadata service) are refused before connecting, however written.
    for url in [
        format!("http://127.0.0.1:{}/secret", port),
        format!("http://localhost:{}/secret", port),
        format!("http://2130706433:{}/secret", port),
        format!("http://[::1]:{}/secret", port),
        format!("http://[::ffff:127.0.0.1]:{}/secret", port),
        "http://169.254.169.254/latest/meta-data/".to_string(),
        "http://10.0.0.1/".to_string(),
        "http://192.168.1.1/".to_string(),
        "http://[fd00::1]/".to_string(),
        "http://0.0.0.0/".to_string(),
    ] {
        let e = run(format!("http.post({:?}, {{ a: 1 }}, {{}}, {{ public_only: true }})", url));
        assert!(e.starts_with("err(") && e.contains("not an address on the public internet"), "{}: {}", url, e);
    }
    let e = run(format!("http.stream(\"http://127.0.0.1:{}/secret\", (x) => x, {{ public_only: true }})", port));
    assert!(e.contains("not an address on the public internet"), "{}", e);
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), before);
    for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
        assert!(quillo::fetch::public_ip(ip.parse().unwrap()), "{}", ip);
    }
    for ip in ["100.64.0.1", "172.16.0.1", "198.18.0.1", "255.255.255.255", "fe80::1", "64:ff9b::7f00:1", "2002:7f00:1::1", "::127.0.0.1"] {
        assert!(!quillo::fetch::public_ip(ip.parse().unwrap()), "{}", ip);
    }
}
