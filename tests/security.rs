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
