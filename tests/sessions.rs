//! Cookies and signed sessions.

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;

fn app(src: &str) -> Interp {
    let mut it = Interp::new(&std::env::temp_dir());
    it.load_source(src, "app.qwe").unwrap_or_else(|e| panic!("{}", e));
    it
}

fn send(it: &mut Interp, method: &str, host: &str, url: &str, cookie: Option<&str>) -> HttpResponse {
    let mut headers = vec![("Host".to_string(), host.to_string())];
    if let Some(c) = cookie {
        headers.push(("Cookie".to_string(), c.to_string()));
    }
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers, body: vec![], remote: "127.0.0.1".into() })
}

fn set_cookies(r: &HttpResponse) -> Vec<&str> {
    r.headers.iter().filter(|(k, _)| k == "set-cookie").map(|(_, v)| v.as_str()).collect()
}

/// The `name=value` part of a `Set-Cookie` line, to send back.
fn pair(line: &str) -> &str {
    line.split(';').next().unwrap()
}

#[test]
fn cookies_have_safe_defaults() {
    let mut it = app(
        r#"
route GET /a {
    set_cookie("theme", "dark mode; yes", { max_age: "1y" })
    set_cookie("lang", "en")
    set_cookie("lang", "fr")
    redirect("/")
}
route GET /b {
    set_cookie("open", "1", { http_only: false, same_site: "Strict", path: "/b", secure: false })
    delete_cookie("theme")
    "ok"
}
route GET /c = set_cookie("x", "1", { same_site: "None" }) ?? "ok"
route GET /bad = set_cookie("a b", "1")
route GET /crlf = set_cookie("a", "1", { path: "/x\r\nSet-Cookie: evil=1" })
"#,
    );
    let r = send(&mut it, "GET", "example.com", "/a", None);
    assert_eq!(r.status, 303);
    assert_eq!(
        set_cookies(&r),
        [
            "theme=dark%20mode%3B%20yes; Path=/; Max-Age=31536000; HttpOnly; Secure; SameSite=Lax",
            "lang=fr; Path=/; HttpOnly; Secure; SameSite=Lax"
        ]
    );
    // Secure whatever the host (behind a proxy, every request is to
    // 127.0.0.1), unless asked otherwise.
    let r = send(&mut it, "GET", "localhost:9000", "/b", None);
    assert_eq!(
        set_cookies(&r),
        ["open=1; Path=/b; SameSite=Strict", "theme=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax"]
    );
    // SameSite=None needs Secure, always.
    let r = send(&mut it, "GET", "127.0.0.1:9000", "/c", None);
    assert_eq!(set_cookies(&r), ["x=1; Path=/; HttpOnly; Secure; SameSite=None"]);
    assert_eq!(send(&mut it, "GET", "example.com", "/bad", None).status, 500);
    assert_eq!(send(&mut it, "GET", "example.com", "/crlf", None).status, 500);
    // What the browser sends back is decoded.
    let mut it = app(r#"route GET / = request.cookies.theme"#);
    assert_eq!(send(&mut it, "GET", "example.com", "/", Some("theme=dark%20mode%3B%20yes")).text(), "dark mode; yes");
}

#[test]
fn sessions_are_signed_and_bound_to_their_tenant() {
    let src = r#"
tenant_from("example.com")
route POST /login {
    set_session({ user: 7, name: "Ada" })
    { now: session().user }
}
route GET /me = session()
route POST /logout {
    end_session()
    session()
}
route POST /big = set_session({ blob: repeat("x", 5000) })
"#;
    let mut it = app(src);
    let r = send(&mut it, "POST", "acme.example.com", "/login", None);
    assert_eq!(r.text(), r#"{"now":7}"#);
    let line = set_cookies(&r)[0].to_string();
    assert!(line.starts_with("quillo_session=") && line.ends_with("; Path=/; Max-Age=2592000; HttpOnly; Secure; SameSite=Lax"), "{}", line);
    let cookie = pair(&line).to_string();

    assert_eq!(send(&mut it, "GET", "acme.example.com", "/me", Some(&cookie)).text(), r#"{"user":7,"name":"Ada"}"#);
    // Another tenant, a changed cookie, or none: no session.
    assert_eq!(send(&mut it, "GET", "globex.example.com", "/me", Some(&cookie)).text(), "{}");
    let (payload, sig) = cookie.trim_start_matches("quillo_session=").split_once('.').unwrap();
    let forged = format!("quillo_session={}.{}", quillo::crypto::b64url(br#"{"e":9999999999,"d":{"user":1}}"#), sig);
    assert_eq!(send(&mut it, "GET", "acme.example.com", "/me", Some(&forged)).text(), "{}");
    let truncated = format!("quillo_session={}.{}", payload, &sig[..10]);
    assert_eq!(send(&mut it, "GET", "acme.example.com", "/me", Some(&truncated)).text(), "{}");
    assert_eq!(send(&mut it, "GET", "acme.example.com", "/me", None).text(), "{}");
    // One that is valid among others of the same name (a mounted program's).
    let both = format!("quillo_session=junk; {}", cookie);
    assert_eq!(send(&mut it, "GET", "acme.example.com", "/me", Some(&both)).text(), r#"{"user":7,"name":"Ada"}"#);

    let r = send(&mut it, "POST", "acme.example.com", "/logout", Some(&cookie));
    assert_eq!(r.text(), "{}");
    assert!(set_cookies(&r)[0].starts_with("quillo_session=; Path=/; Max-Age=0;"), "{:?}", set_cookies(&r));

    // Too large for a cookie: an error (logged), not a cookie the browser drops.
    let r = send(&mut it, "POST", "acme.example.com", "/big", None);
    assert_eq!((r.status, set_cookies(&r).len()), (500, 0));
}

#[test]
fn sessions_expire() {
    let mut it = app(
        r#"
route POST /in = set_session({ user: 1 }, { max_age: 1 })
route GET /me = session()
"#,
    );
    let r = send(&mut it, "POST", "example.com", "/in", None);
    let cookie = pair(set_cookies(&r)[0]).to_string();
    assert!(set_cookies(&r)[0].contains("Max-Age=1;"));
    assert_eq!(send(&mut it, "GET", "example.com", "/me", Some(&cookie)).text(), r#"{"user":1}"#);
    std::thread::sleep(std::time::Duration::from_millis(2100));
    assert_eq!(send(&mut it, "GET", "example.com", "/me", Some(&cookie)).text(), "{}");
}
