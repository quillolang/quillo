//! Security headers, the Content-Security-Policy and its nonce, and rate limits.

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;

fn app(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    it.load_source(src, "app.qwe").unwrap_or_else(|e| panic!("{}", e));
    it
}

fn send(it: &mut Interp, url: &str, headers: &[(&str, &str)]) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "acme.example.com".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers: h, body: vec![], remote: "10.1.2.3".into() })
}

#[test]
fn answers_carry_security_headers_unless_told_otherwise() {
    let mut it = app(
        r#"
route GET / = "hi"
route GET /framed {
    security_headers({ frame: false })
    "embed me"
}
route GET /own = respond(200, "x", { "referrer-policy": "no-referrer", "x-frame-options": "DENY" })
route GET /none {
    security_headers(false)
    "bare"
}
"#,
    );
    let r = send(&mut it, "/", &[]);
    assert_eq!(r.header("referrer-policy"), Some("strict-origin-when-cross-origin"));
    assert_eq!(r.header("x-frame-options"), Some("SAMEORIGIN"));
    assert_eq!(r.header("content-security-policy"), Some("frame-ancestors 'self'"));
    assert_eq!(r.header("x-content-type-options"), None, "nosniff is added by the server, around the program");
    // HSTS only when the proxy in front says the request came over HTTPS.
    assert_eq!(r.header("strict-transport-security"), None);
    let r = send(&mut it, "/", &[("X-Forwarded-Proto", "https")]);
    assert_eq!(r.header("strict-transport-security"), Some("max-age=31536000"));
    let r = send(&mut it, "/", &[("Forwarded", "for=1.2.3.4; Proto=HTTPS")]);
    assert_eq!(r.header("strict-transport-security"), Some("max-age=31536000"));
    // A route that turns one off, or sets its own.
    let r = send(&mut it, "/framed", &[]);
    assert_eq!((r.header("x-frame-options"), r.header("content-security-policy")), (None, None));
    assert_eq!(r.header("referrer-policy"), Some("strict-origin-when-cross-origin"));
    let r = send(&mut it, "/own", &[]);
    assert_eq!((r.header("referrer-policy"), r.header("x-frame-options")), (Some("no-referrer"), Some("DENY")));
    let r = send(&mut it, "/none", &[("X-Forwarded-Proto", "https")]);
    assert!(["referrer-policy", "x-frame-options", "strict-transport-security", "content-security-policy"].iter().all(|h| r.header(h).is_none()));
    // The next request has the program's again.
    assert_eq!(send(&mut it, "/framed", &[]).header("referrer-policy"), Some("strict-origin-when-cross-origin"));

    // For the whole program.
    let mut it = app(r#"security_headers({ hsts: "max-age=63072000; includeSubDomains", frame: "DENY", referrer_policy: false })
route GET / = "hi""#);
    let r = send(&mut it, "/", &[("X-Forwarded-Proto", "https")]);
    assert_eq!(r.header("strict-transport-security"), Some("max-age=63072000; includeSubDomains"));
    assert_eq!((r.header("x-frame-options"), r.header("content-security-policy")), (Some("DENY"), Some("frame-ancestors 'none'")));
    assert_eq!(r.header("referrer-policy"), None);
    assert!(Interp::new(".").load_source(r#"security_headers({ frame: "ALLOW" })"#, "x.qwe").is_err());
    assert!(Interp::new(".").load_source(r#"security_headers({ hsts: "a\nb" })"#, "x.qwe").is_err());
}

/// The nonce in a policy.
fn nonce_of(r: &HttpResponse) -> String {
    let p = r.header("content-security-policy").unwrap();
    let at = p.find("'nonce-").unwrap() + 7;
    p[at..at + p[at..].find('\'').unwrap()].to_string()
}

#[test]
fn a_policy_with_a_nonce_for_every_inline_script() {
    let mut it = app(
        r#"
csp({ img_src: ["'self'", "data:"] })
component Counter(start: int) {
    state n = start
    <button @click={() => { n = n + 1 }}>{n}</button>
}
route GET / = <html><body>
    <script nonce={csp_nonce()}>window.ok = 1</script>
    <a href="/other">other</a>
    <Counter start={1}/>
</body></html>
route GET /off {
    csp(false)
    <html><body><Counter start={2}/></body></html>
}
route GET /api = { ok: true }
"#,
    );
    let r = send(&mut it, "/", &[]);
    let n = nonce_of(&r);
    assert_eq!(
        r.header("content-security-policy").unwrap(),
        format!("script-src 'self' 'nonce-{}'; object-src 'none'; base-uri 'self'; frame-ancestors 'self'; img-src 'self' data:", n)
    );
    assert!(n.len() >= 22, "{}", n);
    let page = r.text();
    // Every inline script and style carries it: the program's, the
    // component loader, prefetching; components are laid out by a style
    // element, with no style attributes.
    let inline: Vec<&str> = page.split("<script").skip(1).filter(|s| !s.trim_start().starts_with("src=")).collect();
    assert!(inline.len() >= 3, "{}", page);
    for s in inline.iter().chain(page.split("<style").skip(1).collect::<Vec<_>>().iter()) {
        assert!(s.starts_with(&format!(" nonce=\"{}\">", n)), "{}", page);
    }
    assert!(!page.contains("style=\""), "{}", page);
    assert!(page.contains("<style nonce=") && page.contains("quillo-island{display:contents}"), "{}", page);
    // A new one for every answer.
    assert_ne!(nonce_of(&send(&mut it, "/", &[])), n);
    assert!(send(&mut it, "/api", &[]).header("content-security-policy").unwrap().starts_with("script-src 'self' 'nonce-"));
    // Off for one route: as before.
    let r = send(&mut it, "/off", &[]);
    assert_eq!(r.header("content-security-policy"), Some("frame-ancestors 'self'"));
    assert!(r.text().contains("<quillo-island style=\"display:contents\""), "{}", r.text());
    assert!(!r.text().contains(" nonce=\""), "{}", r.text());

    // Report only: nothing is blocked, frames aside.
    let mut it = app(r#"csp({ report_only: true, report_uri: "/csp-reports" })
route GET / = "hi""#);
    let r = send(&mut it, "/", &[]);
    assert!(r.header("content-security-policy-report-only").unwrap().ends_with("; report-uri /csp-reports"));
    assert_eq!(r.header("content-security-policy"), Some("frame-ancestors 'self'"));
    assert!(Interp::new(".").load_source(r#"csp({ script_src: "x; y" })"#, "x.qwe").is_err());
}

#[test]
fn rate_limits_refuse_past_the_limit_and_say_when_to_come_back() {
    let mut it = app(
        r#"
tenant_from("example.com")
route GET /try = rate_limit(`login:${request.ip}`, 3, "1m")
route GET /api {
    rate_limit("api", 2, "1h", { reply: true })
    "ok"
}
route GET /peek = rate_limit("fails", 2, "1m", { cost: 0 })
route GET /fail = rate_limit("fails", 2, "1m")
route GET /reset = rate_limit_reset("fails")
"#,
    );
    let tries: Vec<String> = (0..5).map(|_| send(&mut it, "/try", &[]).text()).collect();
    assert_eq!(tries, ["true", "true", "true", "false", "false"]);
    // Another tenant counts apart.
    let h = vec![("Host".to_string(), "globex.example.com".to_string())];
    let r = it.handle(&HttpRequest { method: "GET".into(), url: "/try".into(), headers: h, body: vec![], remote: "10.1.2.3".into() });
    assert_eq!(r.text(), "true");

    assert_eq!(send(&mut it, "/api", &[]).text(), "ok");
    assert_eq!(send(&mut it, "/api", &[]).text(), "ok");
    let r = send(&mut it, "/api", &[]);
    assert_eq!(r.status, 429);
    let wait: i64 = r.header("retry-after").unwrap().parse().unwrap();
    // The rest of this hour's window, and a third of the next.
    assert!((1200..=4800).contains(&wait), "{}", wait);
    assert_eq!(r.header("x-frame-options"), Some("SAMEORIGIN"));

    // Peeking counts nothing; failures do; a reset forgets them.
    let seq: Vec<String> = ["/peek", "/peek", "/fail", "/peek", "/fail", "/peek", "/reset", "/peek"].iter().map(|u| send(&mut it, u, &[]).text()).collect();
    assert_eq!(seq, ["true", "true", "true", "true", "true", "false", "", "true"]);
}

#[test]
fn rate_limits_are_shared_by_servers_when_sessions_are_in_the_database() {
    let dir = std::env::temp_dir().join(format!("quillo-limits-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = r#"
db.open("app.db")
sessions({ store: "db" })
route GET /try = rate_limit("login:ada", 3, "1m")
route GET /mine = rate_limit("login:ada", 3, "1m", { store: "memory" })
"#;
    let mut servers: Vec<Interp> = (0..2)
        .map(|_| {
            let mut it = Interp::new(&dir);
            it.load_source(src, "app.qwe").unwrap_or_else(|e| panic!("{}", e));
            it
        })
        .collect();
    let seq: Vec<String> = (0..4).map(|i| send(&mut servers[i % 2], "/try", &[]).text()).collect();
    assert_eq!(seq, ["true", "true", "true", "false"]);
    assert_eq!(send(&mut servers[1], "/mine", &[]).text(), "true");
    let _ = std::fs::remove_dir_all(&dir);
}
