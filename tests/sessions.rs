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
    // Secure and for the whole site: `__Host-`, which no other host can set.
    assert!(line.starts_with("__Host-quillo_session=") && line.ends_with("; Path=/; Max-Age=2592000; HttpOnly; Secure; SameSite=Lax"), "{}", line);
    let cookie = pair(&line).to_string();

    assert_eq!(send(&mut it, "GET", "acme.example.com", "/me", Some(&cookie)).text(), r#"{"user":7,"name":"Ada"}"#);
    // Another tenant, a changed cookie, or none: no session.
    assert_eq!(send(&mut it, "GET", "globex.example.com", "/me", Some(&cookie)).text(), "{}");
    let (payload, sig) = cookie.trim_start_matches("__Host-quillo_session=").split_once('.').unwrap();
    let forged = format!("__Host-quillo_session={}.{}", quillo::crypto::b64url(br#"{"e":9999999999,"d":{"user":1}}"#), sig);
    assert_eq!(send(&mut it, "GET", "acme.example.com", "/me", Some(&forged)).text(), "{}");
    let truncated = format!("__Host-quillo_session={}.{}", payload, &sig[..10]);
    assert_eq!(send(&mut it, "GET", "acme.example.com", "/me", Some(&truncated)).text(), "{}");
    assert_eq!(send(&mut it, "GET", "acme.example.com", "/me", None).text(), "{}");
    // One that is valid among others of the same name (a mounted program's).
    let both = format!("__Host-quillo_session=junk; {}", cookie);
    assert_eq!(send(&mut it, "GET", "acme.example.com", "/me", Some(&both)).text(), r#"{"user":7,"name":"Ada"}"#);
    // Two valid ones are none: one was planted (another account's, by a
    // sibling subdomain). Nor is one under the name without the prefix.
    let other = pair(set_cookies(&send(&mut it, "POST", "acme.example.com", "/login", None))[0]).to_string();
    let two = format!("{}; {}", other, cookie);
    assert_eq!(send(&mut it, "GET", "acme.example.com", "/me", Some(&two)).text(), "{}");
    let unprefixed = cookie.trim_start_matches("__Host-").to_string();
    assert_eq!(send(&mut it, "GET", "acme.example.com", "/me", Some(&unprefixed)).text(), "{}");

    let r = send(&mut it, "POST", "acme.example.com", "/logout", Some(&cookie));
    assert_eq!(r.text(), "{}");
    assert!(set_cookies(&r)[0].starts_with("__Host-quillo_session=; Path=/; Max-Age=0;"), "{:?}", set_cookies(&r));

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

fn send_from(it: &mut Interp, method: &str, host: &str, url: &str, cookie: Option<&str>, agent: &str) -> HttpResponse {
    let mut headers = vec![("Host".to_string(), host.to_string()), ("User-Agent".to_string(), agent.to_string())];
    if let Some(c) = cookie {
        headers.push(("Cookie".to_string(), c.to_string()));
    }
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers, body: vec![], remote: "10.0.0.9".into() })
}

const DB_SESSIONS: &str = r#"
tenant_from("example.com")
sessions({ store: "db", idle: "3s", max_age: "1h" })
route POST /login/:id(int) {
    set_session({ user: id })
    session()
}
route POST /cart = set_session(merge(session(), { cart: ["tea"] }))
route POST /renew = renew_session()
route GET /me = session()
route POST /logout = end_session()
route POST /everywhere = end_sessions(session().user)
route POST /others = end_sessions(session().user, { keep_current: true })
route POST /one/:id = end_sessions(session().user, { id })
route GET /devices = user_sessions(session().user) | map((s) => { current: s.current, ip: s.ip, agent: s.agent, id: s.id })
route GET /rows = db.one("select count(*) as n from _quillo_sessions").n
"#;

fn db_app(dir: &std::path::Path) -> Interp {
    let mut it = Interp::new(dir);
    it.load_source(&format!("db.open(\"app.db\")\n{}", DB_SESSIONS), "app.qwe").unwrap_or_else(|e| panic!("{}", e));
    it
}

fn login(it: &mut Interp, host: &str, user: i64, agent: &str) -> String {
    let r = send_from(it, "POST", host, &format!("/login/{}", user), None, agent);
    assert_eq!(r.text(), format!(r#"{{"user":{}}}"#, user));
    pair(set_cookies(&r)[0]).to_string()
}

#[test]
fn sessions_in_the_database_end_when_asked_on_every_server() {
    let dir = std::env::temp_dir().join(format!("quillo-db-sessions-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Two servers of one program, sharing its database.
    let (mut a, mut b) = (db_app(&dir), db_app(&dir));
    let host = "acme.example.com";
    let cookie = login(&mut a, host, 7, "Laptop");
    // The cookie holds a random id, nothing more.
    let token = cookie.trim_start_matches("__Host-quillo_session=");
    assert_eq!(token.len(), 43, "{}", cookie);
    assert_eq!(send(&mut b, "GET", host, "/me", Some(&cookie)).text(), r#"{"user":7}"#);
    assert_eq!(send(&mut b, "GET", "globex.example.com", "/me", Some(&cookie)).text(), "{}");
    // The same user: the same id, and no new cookie.
    let r = send(&mut b, "POST", host, "/cart", Some(&cookie));
    assert!(set_cookies(&r).is_empty(), "{:?}", set_cookies(&r));
    assert_eq!(send(&mut a, "GET", host, "/me", Some(&cookie)).text(), r#"{"user":7,"cart":["tea"]}"#);
    // Another user: a new id; the old one is no session.
    let r = send(&mut a, "POST", host, "/login/8", Some(&cookie));
    let eight = pair(set_cookies(&r)[0]).to_string();
    assert_ne!(eight, cookie);
    assert_eq!(send(&mut b, "GET", host, "/me", Some(&cookie)).text(), "{}");
    assert_eq!(send(&mut b, "GET", host, "/me", Some(&eight)).text(), r#"{"user":8}"#);
    // Renewed: a new id with the same data.
    let r = send(&mut b, "POST", host, "/renew", Some(&eight));
    let renewed = pair(set_cookies(&r)[0]).to_string();
    assert_ne!(renewed, eight);
    assert_eq!(send(&mut a, "GET", host, "/me", Some(&eight)).text(), "{}");
    assert_eq!(send(&mut a, "GET", host, "/me", Some(&renewed)).text(), r#"{"user":8}"#);
    // Logged out: a copy of the cookie someone kept is no session either.
    let r = send(&mut a, "POST", host, "/logout", Some(&renewed));
    assert!(set_cookies(&r)[0].contains("Max-Age=0"));
    assert_eq!(send(&mut b, "GET", host, "/me", Some(&renewed)).text(), "{}");

    // A user's devices, and ending them.
    let laptop = login(&mut a, host, 7, "Laptop");
    let phone = login(&mut b, host, 7, "Phone");
    let tablet = login(&mut a, host, 7, "Tablet");
    let other = login(&mut a, host, 9, "Laptop");
    let devices: serde_json::Value = serde_json::from_str(&send(&mut b, "GET", host, "/devices", Some(&phone)).text()).unwrap();
    let list = devices.as_array().unwrap();
    assert_eq!(list.len(), 3, "{}", devices);
    assert_eq!(list.iter().filter(|d| d["current"] == true).map(|d| d["agent"].as_str().unwrap()).collect::<Vec<_>>(), ["Phone"]);
    assert!(list.iter().all(|d| d["ip"] == "10.0.0.9"));
    // One of them, by its id from the list.
    let tablet_id = list.iter().find(|d| d["agent"] == "Tablet").unwrap()["id"].as_str().unwrap().to_string();
    assert_eq!(send(&mut a, "POST", host, &format!("/one/{}", tablet_id), Some(&phone)).text(), "1");
    assert_eq!(send(&mut a, "GET", host, "/me", Some(&tablet)).text(), "{}");
    // The others, keeping this one; then all, this one too.
    assert_eq!(send(&mut a, "POST", host, "/others", Some(&phone)).text(), "1");
    assert_eq!(send(&mut b, "GET", host, "/me", Some(&laptop)).text(), "{}");
    assert_eq!(send(&mut b, "GET", host, "/me", Some(&phone)).text(), r#"{"user":7}"#);
    let r = send(&mut b, "POST", host, "/everywhere", Some(&phone));
    assert_eq!(r.text(), "1");
    assert!(set_cookies(&r)[0].contains("Max-Age=0"));
    assert_eq!(send(&mut a, "GET", host, "/me", Some(&phone)).text(), "{}");
    // Another user's are left alone.
    assert_eq!(send(&mut a, "GET", host, "/me", Some(&other)).text(), r#"{"user":9}"#);

    // Unused for longer than `idle`: ended. Used meanwhile: kept.
    let s = login(&mut a, host, 7, "Laptop");
    std::thread::sleep(std::time::Duration::from_millis(1700));
    assert_eq!(send(&mut a, "GET", host, "/me", Some(&s)).text(), r#"{"user":7}"#);
    std::thread::sleep(std::time::Duration::from_millis(1700));
    assert_eq!(send(&mut b, "GET", host, "/me", Some(&s)).text(), r#"{"user":7}"#);
    std::thread::sleep(std::time::Duration::from_millis(4200));
    assert_eq!(send(&mut a, "GET", host, "/me", Some(&s)).text(), "{}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ending_sessions_needs_them_in_the_database() {
    let mut it = app(r#"route POST /x = end_sessions(1)"#);
    let r = send(&mut it, "POST", "example.com", "/x", None);
    assert_eq!(r.status, 500);
    // Signed sessions are signed again by renew_session (nothing to end on the server).
    let mut it = app("route POST /in = set_session({ user: 1 })\nroute POST /renew = renew_session()\nroute GET /me = session()");
    let c = pair(set_cookies(&send(&mut it, "POST", "example.com", "/in", None))[0]).to_string();
    let r = send(&mut it, "POST", "example.com", "/renew", Some(&c));
    let renewed = pair(set_cookies(&r)[0]).to_string();
    assert_eq!(send(&mut it, "GET", "example.com", "/me", Some(&renewed)).text(), r#"{"user":1}"#);
}

#[test]
fn sessions_and_rate_limits_are_kept_apart_from_the_programs_transactions() {
    // On SQLite, with Quillo's tables in the program's database: what a
    // transaction's function does to sessions and rate limits is not undone
    // with a try that failed, nor done again by the next.
    let dir = std::env::temp_dir().join(format!("quillo-db-sessions-tx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut it = Interp::new(&dir);
    let src = r#"db.open("app.db")
db.exec("create table if not exists notes (n int)")
sessions({ store: "db" })
route POST /in/:id(int) = db.transaction(() => {
    set_session({ user: id })
    db.exec("insert into notes values (?)", id)
    err("refused")
}).error
route POST /failed = db.transaction(() => {
    let ok = rate_limit("k", 3, "1m")
    db.exec("insert into notes values (0)")
    err(`refused (${ok})`)
}).error
route POST /limited = db.transaction(() => [rate_limit("k", 3, "1m"), rate_limit("k", 3, "1m")])
route GET /me = session()
route GET /notes = db.query("select n from notes") | map((r) => r.n)
"#;
    it.load_source(src, "app.qwe").unwrap_or_else(|e| panic!("{}", e));
    let host = "acme.example.com";
    let r = send(&mut it, "POST", host, "/in/7", None);
    assert_eq!(r.text(), "refused");
    // The note is rolled back; the session stays, as the cookie says.
    let cookie = pair(set_cookies(&r)[0]).to_string();
    assert_eq!(send(&mut it, "GET", host, "/me", Some(&cookie)).text(), r#"{"user":7}"#);
    assert_eq!(send(&mut it, "GET", host, "/notes", None).text(), "[]");
    // A call in a transaction that failed counts, once; two in one, two.
    assert_eq!(send(&mut it, "POST", host, "/failed", None).text(), "refused (true)");
    assert_eq!(send(&mut it, "POST", host, "/limited", None).text(), "[true,true]");
    assert_eq!(send(&mut it, "POST", host, "/limited", None).text(), "[false,false]");
    let _ = std::fs::remove_dir_all(&dir);
}
