//! FEEDBACK47.md: `route` and `routes` groups (branch feedback40-43). Middleware that fails
//! closed and runs before the server's own answers, the request a pattern cannot replace, `| g`
//! after a pattern, and paths and hosts read as routes read them.

use quillo::check::{check_source, Severity};
use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::Path;

fn app(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "test.qwe") {
        panic!("{}", e);
    }
    it
}

fn send(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    let mut hs = vec![("Host".to_string(), "localhost".to_string())];
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("host") {
            hs.retain(|(n, _)| n != "Host");
        }
        hs.push((k.to_string(), v.to_string()));
    }
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: hs, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
}

fn get(it: &mut Interp, url: &str, headers: &[(&str, &str)]) -> HttpResponse {
    send(it, "GET", url, headers, "")
}

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new("."))
        .diagnostics
        .iter()
        .map(|d| format!("{}{} {}", d.line, if d.severity == Severity::Warning { " warning:" } else { "" }, d.message))
        .collect()
}

fn parse_error(src: &str) -> String {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    match it.load_source(src, "test.qwe") {
        Ok(_) => panic!("loaded: {}", src),
        Err(e) => e.to_string(),
    }
}

// Q-534: only an answer stopped the request; `false`, an error, text and anything else let the
// route run. Now only the request given back, or null, lets it go on.
#[test]
fn middleware_fails_closed() {
    let mut it = app(
        r#"
fn admin(r) = r.headers["x-role"] == "admin"
fn deny_text(r) = admin(r) ? r : "admins only"
fn deny_err(r) = admin(r) ? r : err("admins only")
fn deny_false(r) = admin(r) ? r : false
fn deny_null(r) = admin(r) ? r : null
fn deny_reply(r) = admin(r) ? r : reply 401 "sign in"
fn deny_respond(r) = admin(r) ? r : respond(418, "no tea")
fn yes(r) = admin(r) ? r : true
fn changed(r) = merge(r, { seen: true })
routes /text | deny_text { route GET /x = "secret" }
routes /err | deny_err { route GET /x = "secret" }
routes /false | deny_false { route GET /x = "secret" }
routes /null | deny_null { route GET /x = "secret" }
routes /reply | deny_reply { route GET /x = "secret" }
routes /respond | deny_respond { route GET /x = "secret" }
routes /true | yes { route GET /x = "secret" }
routes /changed | changed { route GET /x = "secret" }
fn always(r) = true
routes /always | always { route GET /x = "secret" }
"#,
    );
    let admin = [("x-role", "admin")];
    // Null: the group's routes do not match, and no other route has the path (FEEDBACK51.md,
    // Q-651, which changed it from going on).
    for (group, status) in [("text", 500), ("err", 500), ("false", 403), ("null", 404), ("reply", 401), ("respond", 418), ("true", 500), ("changed", 200)] {
        let r = get(&mut it, &format!("/{}/x", group), &[]);
        assert_eq!(r.status, status, "{}: {}", group, r.text());
        // The request given back, or a copy of it, lets the route go on; nothing else does.
        if group != "changed" {
            assert_ne!(r.text(), "secret", "{} let a stranger in", group);
        }
        // An admin goes on, whatever the group.
        if group != "true" {
            assert_eq!(get(&mut it, &format!("/{}/x", group), &admin).text(), "secret", "{}", group);
        }
    }
    assert_eq!(get(&mut it, "/false/x", &[]).text(), "forbidden\n");
    assert!(get(&mut it, "/err/x", &[]).text().contains("admins only"));
    // `true` is a mistake for anyone: a predicate is no middleware.
    assert_eq!(get(&mut it, "/always/x", &admin).status, 500);
}

#[test]
fn check_refuses_middleware_that_can_only_be_a_predicate_or_text() {
    assert_eq!(
        diags("fn admin(r) = r.headers[\"x-role\"] == \"admin\"\nroutes /a | admin { route GET /x = 1 }\n"),
        ["2 `admin` gives bool: middleware gives back the request to let the route go on (null: the route does not match), or an answer to stop it; a check is written `(r) => ok(r) ? r : reply 403 \"...\"`"]
    );
    assert_eq!(diags("fn note(r) = \"seen\"\nroutes /a | note { route GET /x = 1 }\n").len(), 1);
    // A factory's middleware, by its name and its call.
    let factory = diags("fn role(name) = (r) => r.headers[\"x-role\"] == name\nroutes /a | role(\"admin\") { route GET /x = 1 }\n");
    assert!(factory.iter().all(|d| !d.contains("gives bool") || d.contains("role(...)")), "{:?}", factory);
    // What may go on or answer passes: the request, null, a response, an error, or what is not
    // known.
    for ok in [
        "fn g(r) = r.headers.a == \"1\" ? r : reply 401 \"no\"\nroutes /a | g { route GET /x = 1 }\n",
        "fn g(r) = r.headers.a == \"1\" ? null : respond(403, \"no\")\nroutes /a | g { route GET /x = 1 }\n",
        "fn g(r) = r.headers.a == \"1\" ? r : false\nroutes /a | g { route GET /x = 1 }\n",
        "fn g(r) = r\nroutes /a | g { route GET /x = 1 }\n",
        "fn member_of(team) = (r) => team == 7 ? r : reply 403 \"no\"\nroutes /t/:team(int) | member_of(team) { route GET /x = team }\n",
    ] {
        assert!(diags(ok).is_empty(), "{} {:?}", ok, diags(ok));
    }
}

// Q-535: a pattern binding `request` replaced what the middleware was given: a JSON body could
// carry the headers it checked.
#[test]
fn a_field_named_request_is_not_the_request_middleware_checks() {
    let mut it = app(
        r#"
fn signed_in(r) = r.headers.authorization == "Bearer s3cret" ? r : reply 401 "sign in"
routes /api | signed_in {
    route POST /approve { json: { request: map } } = `approved ${request.id}`
    route POST /whole { json: request } = "whole secret"
    route GET /p/:request = `param ${request}`
    route GET /q ? { request: str } = `query ${request}`
}
"#,
    );
    let json = [("content-type", "application/json")];
    let forged = r#"{"request":{"id":7,"headers":{"authorization":"Bearer s3cret"}}}"#;
    assert_eq!(send(&mut it, "POST", "/api/approve", &json, forged).status, 401);
    assert_eq!(send(&mut it, "POST", "/api/whole", &json, r#"{"headers":{"authorization":"Bearer s3cret"}}"#).status, 401);
    assert_eq!(get(&mut it, "/api/p/Bearer%20s3cret", &[]).status, 401);
    assert_eq!(get(&mut it, "/api/q?request=x", &[]).status, 401);
    // With the token, the route's own `request` is still its pattern's.
    let signed = [("authorization", "Bearer s3cret"), ("content-type", "application/json")];
    assert_eq!(send(&mut it, "POST", "/api/approve", &signed, r#"{"request":{"id":7}}"#).text(), "approved 7");
    assert_eq!(get(&mut it, "/api/p/x", &[("authorization", "Bearer s3cret")]).text(), "param x");
    assert_eq!(get(&mut it, "/api/q?request=y", &[("authorization", "Bearer s3cret")]).text(), "query y");
}

// Q-536: `{ headers: ... } | g` read as a pattern with two alternatives, the second taking
// any request.
#[test]
fn a_bare_name_after_a_request_pattern_is_refused() {
    let group = parse_error("fn g(r) = reply 401 \"no\"\nroutes /api { headers: { \"x-key\": \"k1\" } } | g {\n    route GET /x = \"x\"\n}\n");
    assert!(group.contains("`| g` after a request pattern is another pattern, one that takes any request: a group's middleware goes before the pattern, `routes /api | g { headers: ... } { ... }`"), "{}", group);
    let route = parse_error("fn g(r) = reply 401 \"no\"\nroute GET /r { headers: { \"x-key\": \"k1\" } } | g = \"secret\"\n");
    assert!(route.contains("a route has no middleware of its own"), "{}", route);
    // Its edges: alternatives of patterns, and middleware before the pattern, stay.
    let mut it = app(
        r#"
fn g(r) = r.headers["x-ok"] == "1" ? r : reply 401 "no"
route POST /either { json: { a: x } } | { form: { a: x } } = `a ${x}`
routes /api | g { headers: { "x-key": "k1" } } { route GET /x = "x" }
"#,
    );
    assert_eq!(send(&mut it, "POST", "/either", &[("content-type", "application/json")], r#"{"a":"j"}"#).text(), "a j");
    assert_eq!(get(&mut it, "/api/x", &[("x-key", "k1")]).status, 401);
    assert_eq!(get(&mut it, "/api/x", &[("x-key", "k1"), ("x-ok", "1")]).text(), "x");
}

// Q-537: `site_headers` compared the raw path, routes its decoded pieces: `//admin/users` and
// `/%61dmin/users` got the admin page without its headers.
#[test]
fn site_headers_hold_for_every_spelling_routes_answer() {
    let mut it = app(
        r#"
site_headers("/admin", { "content-security-policy": "frame-ancestors 'none'" })
routes /admin {
    route GET /users = <html><body>users</body></html>
}
route GET /administrator = <html><body>other</body></html>
route GET /raw/*rest = `${request.path} ${request.route_path}`
"#,
    );
    for path in ["/admin/users", "//admin/users", "/%61dmin/users", "/admin//users", "/admin/users/", "/admin%2Fusers"] {
        let r = get(&mut it, path, &[]);
        assert_eq!(r.header("content-security-policy"), if r.status == 200 { Some("frame-ancestors 'none'") } else { r.header("content-security-policy") }, "{}", path);
        if path != "/admin%2Fusers" {
            assert_eq!(r.status, 200, "{}", path);
        }
    }
    // Its edge: a path that only starts with the same letters is not under it.
    assert_eq!(get(&mut it, "/administrator", &[]).header("content-security-policy"), None);
    assert_eq!(get(&mut it, "//raw/%61b//c", &[]).text(), "//raw/%61b//c /raw/ab/c");
    assert_eq!(get(&mut it, "/raw", &[]).text(), "/raw /raw");
}

// Q-545: a host pattern compared the header as sent.
#[test]
fn a_host_is_read_as_it_names_a_host() {
    let mut it = app(
        r#"
routes /h { headers: { host: "api.example.com" } } {
    route GET /me = `me ${request.host}`
}
route GET /host = `${request.host} ${request.headers.host}`
"#,
    );
    for host in ["api.example.com", "API.example.com", "api.example.com:443", "api.example.com:80", "api.example.com.", "API.Example.COM.:443"] {
        assert_eq!(get(&mut it, "/h/me", &[("host", host)]).text(), "me api.example.com", "{}", host);
    }
    // Its edges: another port is another place, and another host another host.
    assert_eq!(get(&mut it, "/h/me", &[("host", "api.example.com:8080")]).status, 400);
    assert_eq!(get(&mut it, "/h/me", &[("host", "api.example.org")]).status, 400);
    assert_eq!(get(&mut it, "/host", &[("host", "Example.com.:8080")]).text(), "example.com:8080 example.com:8080");
    assert_eq!(get(&mut it, "/host", &[("host", "[::1]:443")]).text(), "[::1] [::1]");
    assert_eq!(get(&mut it, "/host", &[("host", "[::1]:8443")]).text(), "[::1]:8443 [::1]:8443");
}

// Q-539: a guarded group's 400, 404, 405 and `OPTIONS` came before its middleware, so anyone
// could map its routes, methods and parameter types.
#[test]
fn a_guarded_group_says_nothing_before_its_middleware() {
    let mut it = app(
        r#"
fn g(r) = r.headers["x-ok"] == "1" ? r : reply 401 "no"
routes /admin | g {
    route GET /users ? { page: int } = `users page ${page}`
    route POST /users { json: { name: str } } = `made ${name}`
    route DELETE /users/:id(int) = `deleted ${id}`
    routes /deep | inner { route GET /x = "deep" }
}
fn inner(r) = r.headers["x-deep"] == "1" ? r : reply 403 "not deep"
fn member_of(team) = (r) => team == 7 ? r : reply 403 "not your team"
routes /t/:team(int) | member_of(team) { route GET /x = `team ${team}` }
routes /open { route GET /x = "open" }
route GET /admin2 = "outside"
"#,
    );
    let none: [(&str, &str); 0] = [];
    let ok = [("x-ok", "1")];
    for (method, url) in [("GET", "/admin/nothing"), ("GET", "/admin/users"), ("PUT", "/admin/users"), ("OPTIONS", "/admin/users/7"), ("DELETE", "/admin/users/abc"), ("BREW", "/admin/users"), ("GET", "/admin"), ("POST", "/admin/users")] {
        let r = send(&mut it, method, url, &none, "{}");
        assert_eq!((r.status, r.header("allow")), (401, None), "{} {}", method, url);
    }
    // Let in, the server's own answers, as before.
    assert_eq!(send(&mut it, "GET", "/admin/nothing", &ok, "").status, 404);
    assert_eq!(send(&mut it, "GET", "/admin/users", &ok, "").status, 400);
    let put = send(&mut it, "PUT", "/admin/users", &ok, "");
    assert_eq!((put.status, put.header("allow")), (405, Some("GET, HEAD, OPTIONS, POST")));
    assert_eq!(send(&mut it, "OPTIONS", "/admin/users/7", &ok, "").status, 204);
    assert_eq!(send(&mut it, "BREW", "/admin/users", &ok, "").status, 501);
    assert_eq!(send(&mut it, "GET", "/admin/users?page=2", &ok, "").text(), "users page 2");
    // Its edges: the inner group's own middleware after the outer's; a factory with the group's
    // parameter; a group with no middleware, and a path that only starts the same, unchanged.
    assert_eq!(get(&mut it, "/admin/deep/nothing", &ok).status, 403);
    assert_eq!(get(&mut it, "/admin/deep/nothing", &[("x-ok", "1"), ("x-deep", "1")]).status, 404);
    assert_eq!(get(&mut it, "/admin/deep/nothing", &[("x-deep", "1")]).status, 401);
    assert_eq!(get(&mut it, "/t/8/nothing", &none).status, 403);
    assert_eq!(get(&mut it, "/t/7/nothing", &none).status, 404);
    // A parameter of another type goes through the middleware too, or the 404 would say its type.
    assert_eq!(get(&mut it, "/t/x/nothing", &none).status, 403);
    assert_eq!(get(&mut it, "/open/nothing", &none).status, 404);
    assert_eq!(get(&mut it, "/admin2", &none).text(), "outside");
    // The hidden route is nowhere to be seen: `OPTIONS *`, `quillo check`'s count.
    let star = send(&mut it, "OPTIONS", "*", &none, "");
    assert!(!star.header("allow").unwrap_or("").contains("GUARD"), "{:?}", star.header("allow"));
    let r = check_source("fn g(r) = r\nroutes /a | g { route GET /x = 1 }\n", "t.qwe", Path::new("."));
    assert_eq!(r.symbols.iter().filter(|s| s.kind == quillo::check::SymbolKind::Route).count(), 1);
}

// Q-540: what followed `*rest` was never compared.
#[test]
fn nothing_follows_a_rest() {
    assert!(parse_error("route GET /f/*rest/more = rest\n").contains("`*rest` takes the rest of the path, so it comes last: nothing can follow it"));
    assert!(parse_error("route GET /two/*a/*b = a\n").contains("`*a` takes the rest of the path"));
    let group = parse_error("routes /docs/*path {\n    route GET /edit = path\n}\n");
    assert!(group.contains("a route in this group can add nothing after it; give the group no rest, and each route its own (`route GET /edit/*path`)"), "{}", group);
    // Its edges: a rest at the end, alone, in a group's route, and a group's rest with `/`.
    let mut it = app("route GET /files/*rest = rest\nroutes /docs { route GET /*p = p }\nroutes /all/*p { route GET / = p }\nroute GET /*any = `any ${any}`\n");
    assert_eq!(get(&mut it, "/files/a/b", &[]).text(), "a/b");
    assert_eq!(get(&mut it, "/files", &[]).text(), "");
    assert_eq!(get(&mut it, "/docs/x/y", &[]).text(), "x/y");
    assert_eq!(get(&mut it, "/all/m/n", &[]).text(), "m/n");
    assert_eq!(get(&mut it, "/zzz", &[]).text(), "any zzz");
}

// Q-542: typed values took `NaN`, `inf`, `+5`, `05` and spaces.
#[test]
fn typed_values_are_written_one_way() {
    let mut it = app(
        r#"
route GET /n/:id(int) = `int ${id}`
route GET /f/:x(float) = json.stringify({ x })
route GET /d/:x(dec) = `dec ${x}`
route GET /m/:x(num) = `num ${x}`
route GET /s/:x(str) = json.stringify({ x })
route GET /u/:x = json.stringify({ x })
route GET /q ? { x: float } = json.stringify({ x })
route GET /i ? { n: int } = `n ${n}`
"#,
    );
    for bad in ["/n/05", "/n/+5", "/n/%205", "/n/5%20", "/n/-0", "/n/1.0", "/n/1e3", "/n/99999999999999999999", "/f/NaN", "/f/inf", "/f/-inf", "/f/1e999", "/f/.5", "/f/5.", "/f/1e", "/f/0x10", "/d/NaN", "/d/1e999", "/m/+1", "/m/inf"] {
        assert_eq!(get(&mut it, bad, &[]).status, 404, "{}", bad);
    }
    for (url, want) in [
        ("/n/0", "int 0"), ("/n/-12", "int -12"), ("/n/9223372036854775807", "int 9223372036854775807"),
        ("/f/1.5", r#"{"x":1.5}"#), ("/f/-2e3", r#"{"x":-2000.0}"#), ("/f/0.25", r#"{"x":0.25}"#), ("/f/3", r#"{"x":3.0}"#), ("/f/1E-2", r#"{"x":0.01}"#),
        ("/d/12.50", "dec 12.50"), ("/m/7", "num 7"), ("/m/7.5", "num 7.5"),
        ("/s/%20a%20", r#"{"x":" a "}"#), ("/u/%20a%20", r#"{"x":" a "}"#), ("/q?x=2.5", r#"{"x":2.5}"#), ("/i?n=-3", "n -3"),
    ] {
        assert_eq!(get(&mut it, url, &[]).text(), want, "{}", url);
    }
    assert_eq!(get(&mut it, "/q?x=NaN", &[]).status, 400);
    assert_eq!(get(&mut it, "/i?n=%205", &[]).status, 400);
    // `int()` and `float()` keep reading what Python's do.
    let mut it = app("route GET / = json.stringify([int(\" 5 \"), int(\"+5\"), float(\"inf\") > 1.0, float(\" 2.5\")])\n");
    assert_eq!(get(&mut it, "/", &[]).text(), "[5,5,true,2.5]");
}

// Q-543: a blank field with a default was a 400, `tag[]` gave an empty list, `bool` took
// `yes` but not `TRUE`.
#[test]
fn form_and_query_values_meet_their_patterns() {
    let mut it = app(
        r#"
route GET /p ? { page: int = 1 } = `page=${page}`
route GET /o ? { q: str = null, size: int = null } = json.stringify({ q, size })
route GET /b ? { on: bool } = `on=${on}`
route GET /l ? { id: [int] } = json.stringify(id)
route POST /tags { form: { tag: [str] } } = json.stringify(tag)
route POST /agree { form: { agree: bool = false } } = `agree=${agree}`
"#,
    );
    let form = [("content-type", "application/x-www-form-urlencoded")];
    for (url, want) in [
        ("/p?page=", "page=1"), ("/p?page=%20", "page=1"), ("/p", "page=1"), ("/p?page=3", "page=3"),
        // A blank text is text; a blank number is one not sent (found in review).
        ("/o?q=&size=", r#"{"q":"","size":null}"#), ("/o?q=x&size=2", r#"{"q":"x","size":2}"#),
        ("/b?on=TRUE", "on=true"), ("/b?on=True", "on=true"), ("/b?on=YES", "on=true"), ("/b?on=Off", "on=false"), ("/b?on=0", "on=false"),
        ("/l?id[]=1&id[]=2", "[1,2]"), ("/l?id=1&id=2", "[1,2]"), ("/l", "[]"),
    ] {
        assert_eq!(get(&mut it, url, &[]).text(), want, "{}", url);
    }
    // Its edges: a field without a default stays required when blank; `name` wins over
    // `name[]` when both come; an unchecked box with a default is false.
    assert_eq!(get(&mut it, "/b?on=", &[]).status, 400);
    assert_eq!(get(&mut it, "/b?on=maybe", &[]).status, 400);
    assert_eq!(get(&mut it, "/l?id=1&id[]=9", &[]).text(), "[1]");
    assert_eq!(send(&mut it, "POST", "/tags", &form, "tag[]=a&tag[]=b").text(), r#"["a","b"]"#);
    assert_eq!(send(&mut it, "POST", "/agree", &form, "").text(), "agree=false");
    assert_eq!(send(&mut it, "POST", "/agree", &form, "agree=on").text(), "agree=true");
}

/// Held by each test that sets `SHOW_ERRORS`, which is the process's: one set it back while another
/// read it (found in review of FEEDBACK55's slice 9).
static SHOWN: std::sync::Mutex<()> = std::sync::Mutex::new(());

// Q-544: every miss was the same line, with nothing in the log.
#[test]
fn a_pattern_miss_says_which_field_and_what_it_wanted() {
    let _one = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    quillo::server::SHOW_ERRORS.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut it = app(
        r#"
route GET /p ? { page: int = 1 } = `page ${page}`
route POST /form { form: { agree: bool, name: str } } = "ok"
route POST /j { json: { n: int } } = "ok"
"#,
    );
    let form = [("content-type", "application/x-www-form-urlencoded")];
    let text = get(&mut it, "/p?page=x", &[]).text();
    assert!(text.ends_with("GET /p (line 2): `query.page` is text, not an int\n"), "{}", text);
    let text = send(&mut it, "POST", "/form", &form, "name=x").text();
    assert!(text.contains("POST /form (line 3): `form.agree` is missing, and must be a bool (an unchecked checkbox sends nothing: give it a default, `agree: bool = false`)"), "{}", text);
    let text = send(&mut it, "POST", "/form", &form, "agree=on").text();
    assert!(text.contains("`form.name` is missing, and must be a str"), "{}", text);
    let text = send(&mut it, "POST", "/j", &[("content-type", "application/json")], r#"{"n":"7"}"#).text();
    assert!(text.contains("`json.n` is text, not an int"), "{}", text);
    // Without `--watch`, the line only.
    quillo::server::SHOW_ERRORS.store(false, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(get(&mut it, "/p?page=x", &[]).text(), "bad request: no route pattern matches this request\n");
}

// Q-541: a route that can never be reached passed `check`.
#[test]
fn routes_no_request_reaches_are_said() {
    assert_eq!(
        diags("route GET /admin/x = \"public\"\nfn g(r) = reply 401 \"no\"\nroutes /admin | g { route GET /x = \"secret\" }\nroute GET /y = 1\nroute GET /y = 2\nroute HEAD /z = 1\nroutes /_quillo { route GET /stats = 1 }\n"),
        [
            "3 warning: this route is never reached: `GET /admin/x` (line 1) takes every request it would, without `g`, this route's middleware",
            "5 warning: this route is never reached: `GET /y` (line 4) takes every request it would",
            "6 `route HEAD` is never matched: a HEAD request is answered by the GET route, with its headers and no body",
            "7 a route under `/_quillo/` is never called: Quillo answers those paths itself, so pick another",
        ]
    );
    assert_eq!(diags("route GET /u/:id = 1\nroute GET /u/:id(int) = 2\n").len(), 1);
    assert_eq!(diags("route GET /v/:id(int) = 1\nroute GET /v/7 = 2\n").len(), 1);
    assert_eq!(diags("route ANY /a/*rest = 1\nroute POST /a/b = 2\n").len(), 1);
    // Its edges: what a pattern or a type lets through, another method, a later general route.
    for ok in [
        "route GET /w ? { q: str } = 1\nroute GET /w = 2\n",
        "route POST /j { json: { a: int } } = a\nroute POST /j = 2\n",
        "route GET /v/:id(int) = 1\nroute GET /v/new = 2\n",
        "route GET /x = 1\nroute POST /x = 2\n",
        "route GET /p/7 = 1\nroute GET /p/:id(int) = 2\n",
        "route GET /s/:a = 1\nroute GET /s/:a/:b = 2\n",
        "routes /h { headers: { host: \"a.example\" } } { route GET /me = 1 }\nroute GET /h/me = 2\n",
    ] {
        assert!(diags(ok).is_empty(), "{} {:?}", ok, diags(ok));
    }
}

// Q-538: a group guards its routes, not the server fns their pages call.
#[test]
fn a_server_fn_a_guarded_page_calls_is_said_when_it_checks_nothing() {
    let src = r#"fn admins(r) = r.headers["x-role"] == "admin" ? r : reply 403 "admins only"
fn me() = session().user
server fn all_users() = [{ name: "ana" }]
server fn ban(name: str) = `banned ${name}`
server fn careful(name: str) {
    admins(request)
    `ok ${name}`
}
server fn mine() = me() ?? []
server fn counted() = 3
server fn public_one() = 1
component Users() {
    state users = all_users()
    state n = live(counted)
    state m = mine()
    <ul>{for u in users => <li>{u.name} <button @click={() => ban(u.name)}>ban</button><button @click={() => careful(u.name)}>c</button></li>}</ul>
}
component Open() {
    state x = public_one()
    <p>{x}</p>
}
view Shell(children) = <html><body>{children}</body></html>
routes /admin | admins {
    route GET /users = <Shell><Users/></Shell>
}
route GET /public = <Shell><Open/></Shell>
"#;
    let d = diags(src);
    let said: Vec<&str> = d.iter().filter(|w| w.contains("guards its routes")).map(|w| w.split('`').nth(1).unwrap_or("")).collect();
    assert_eq!(said, ["all_users", "ban", "counted"], "{:?}", d);
    assert!(d[0].starts_with("3 warning: `all_users` is called by `GET /admin/users`'s page, which `admins` guards, but a group guards its routes, not the server fns their pages call: anyone can call it at POST /_quillo/rpc/all_users; if it is not for everyone, check the request in it too, as `admins(request)`"), "{:?}", d);
}

// Q-546: mistakes on the route line got other constructs' messages, or none.
#[test]
fn mistakes_on_the_route_line_say_what_they_are() {
    for (src, want) in [
        ("route GET /health { ok: true }\n", "a route's `{` starts its body, or its request pattern: to answer this map, write `= { ... }`"),
        ("route POST /x { json: j }\n", "this request pattern has no body after it: `= ...` or `{ ... }`"),
        ("route POST /x with db { json: j } = j\n", "the request pattern goes before `with`"),
        ("route POST /x { json: j } ? { q: str } = j\n", "the query pattern goes before the request pattern"),
        ("routes GET /a { route GET /x = 1 }\n", "a group takes no method, each of its routes has one"),
        ("routes admin { route GET /x = 1 }\n", "a group's path starts with `/`: `routes /admin { ... }`"),
        ("fn g(r) = r\nroutes /admin with db | g { route GET /x = 1 }\n", "a group's middleware goes before `with`"),
        ("routes /x ? { q: str } { route GET /y = q }\n", "a group takes no query pattern"),
        ("fn g(r) = r\nroute GET /x | g = 1\n", "a route has no middleware of its own"),
        ("route GET /a/:id(int)x = 1\n", "a path cannot hold `x` here"),
        ("route GET /a:b = 1\n", "a path cannot hold `:` here: a parameter is a piece of its own, `/a/:b`"),
        // `+`, `@` and the others a URL's path takes are taken since FEEDBACK53.md, Q-683.
        ("route GET /a'b = 1\n", "a path cannot hold `'` here"),
        ("route GET /a /b = 1\n", "a path has no spaces"),
        ("route GET /:if = 1\n", "`:if` is no name the route can read"),
        ("route GET /:1 = 1\n", "`:1` is no name the route can read"),
        ("route GET /f/*for = 1\n", "`*for` is no name the route can read"),
        ("routes /a { route GET /x = 1\n routes /b { } }\n", "this `routes` group has no routes in it"),
        ("fn g(r) = r\nroutes /a | g { }\n", "this `routes` group has no routes in it"),
    ] {
        let e = parse_error(src);
        assert!(e.contains(want), "{}: {}", src, e);
    }
    // What is right stays right.
    for ok in [
        "let routes = 10\nprint(routes / 2)\n",
        "route GET /file.txt = \"t\"\nroute GET /a-b_c.d~e = 1\n",
        "route GET /ok { json: { a: int } } = a\n",
        "route POST /w { json: { a: int } } with db = a\n",
        "route GET /m ? { q: str } { headers: { x: \"1\" } } = q\n",
        "route GET /%7Euser = 1\n",
        "route GET /health = { ok: true }\n",
        "routes /a { route GET /x = 1 }\n",
    ] {
        let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
        assert!(it.load_source(ok, "test.qwe").is_ok(), "{}: {:?}", ok, it.load_source(ok, "test.qwe").err().map(|e| e.to_string()));
    }
}

// The review of FEEDBACK47's first version: what it said or let through wrongly.
#[test]
fn the_reviews_cases_for_middleware_and_patterns() {
    let _one = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    quillo::server::SHOW_ERRORS.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut it = app(
        r#"
route POST /login { form: { password: str = "x", n: int } } = `[${password}]`
fn tenant_ok(r) = r.headers["x-tenant"] == "1" ? r : reply 401 "tenant"
fn admins(r) = r.headers["x-admin"] == "1" ? r : reply 403 "admins"
routes /:tenant | tenant_ok { route GET /home = "home" }
routes /admin | admins { route GET /users ? { page: int } = `users ${page}` }
fn member_of(team) = (r) => team == 7 ? r : reply 403 "not your team"
routes /t/:team(int) | member_of(team) { route GET /x = `team ${team}` }
allow_cross_site("/payments/return")
route POST /payments/:page = `page ${page}`
"#,
    );
    // No secret and no value sent in a 400, under `--watch` either.
    let mut hook = app("route POST /hook { headers: { \"x-hub-token\": \"whsec_live_9f8e7d\" } } = \"hooked\"\n");
    let r = send(&mut hook, "POST", "/hook", &[("x-hub-token", "whsec_guess")], "");
    assert_eq!(r.status, 400);
    assert!(!r.text().contains("whsec"), "{}", r.text());
    assert!(r.text().contains("`headers.x-hub-token` is text, not the value the route asks for"), "{}", r.text());
    let form = [("content-type", "application/x-www-form-urlencoded")];
    let r = send(&mut it, "POST", "/login", &form, "password=hunter2&n=abc");
    assert!(!r.text().contains("hunter2") && !r.text().contains("abc"), "{}", r.text());
    // A blank `str` is text, kept; the default is for a field not sent.
    assert_eq!(send(&mut it, "POST", "/login", &form, "password=%20%20&n=1").text(), "[  ]");
    assert_eq!(send(&mut it, "POST", "/login", &form, "n=1").text(), "[x]");
    // Overlapping guarded groups each ask: `admins` runs though `/:tenant` came first.
    let tenant = [("x-tenant", "1")];
    assert_eq!(get(&mut it, "/admin/users", &tenant).status, 403);
    assert_eq!(send(&mut it, "PUT", "/admin/users", &tenant, "").header("allow"), None);
    assert_eq!(get(&mut it, "/admin/users", &[("x-tenant", "1"), ("x-admin", "1")]).status, 400);
    // A guard holds only for the requests its group's pattern takes: another host is not under it.
    let mut site = app("fn app_login(r) = reply 401 \"sign in\"\nroutes / | app_login { headers: { host: \"app.example.com\" } } { route GET /dash = \"dash\" }\nroute GET /about = \"about\"\n");
    assert_eq!(get(&mut site, "/nothing", &[("host", "app.example.com")]).status, 401);
    assert_eq!(get(&mut site, "/nothing", &[("host", "www.example.com")]).status, 404);
    assert_eq!(get(&mut site, "/about", &[("host", "www.example.com")]).text(), "about");
    assert_eq!(send(&mut site, "POST", "/about", &[("host", "www.example.com")], "").status, 405);
    // A parameter of another type goes through the middleware too (each guarded group the path is
    // under asks, `/:tenant`'s first).
    assert_eq!(get(&mut it, "/t/abc/x", &[]).status, 401);
    assert_eq!(get(&mut it, "/t/abc/x", &tenant).status, 403);
    // `%2F` stays inside its piece: `/payments%2Freturn` is not under `/payments/return`.
    let cross = [("sec-fetch-site", "cross-site"), ("content-type", "application/x-www-form-urlencoded")];
    assert_eq!(send(&mut it, "POST", "/payments%2Freturn", &cross, "a=1").status, 403);
    assert_eq!(send(&mut it, "POST", "/payments/return", &cross, "a=1").text(), "page return");
    // `num` takes no `-0`; a method named as the hidden route's is unknown.
    let mut plain = app("route GET /n/:x(num) = `num ${x}`\nfn g(r) = r\nroutes /admin | g { route GET /users = \"u\" }\n");
    assert_eq!(get(&mut plain, "/n/-0", &[]).status, 404);
    assert_eq!(get(&mut plain, "/n/-0.5", &[]).text(), "num -0.5");
    assert_eq!(send(&mut plain, "__GUARD", "/admin/users", &[], "").status, 501);
    quillo::server::SHOW_ERRORS.store(false, std::sync::atomic::Ordering::Relaxed);
}

#[test]
fn a_pattern_cannot_rebind_what_the_middleware_reads() {
    let e = parse_error("fn member_of(team) = (r) => team == 7 ? r : reply 403 \"no\"\nroutes /t/:team(int) | member_of(team) {\n    route POST /move { json: { team: int } } = team\n}\n");
    assert!(e.contains("this route's pattern binds `team`, which the group's middleware reads"), "{}", e);
    // Its edges: another name is fine, and `__request` is never a pattern's.
    let mut it = app("fn g(r) = r.headers.ok == \"1\" ? r : reply 401 \"no\"\nroutes /a | g { route GET /q ? { __request: str } = \"q\" }\nfn member_of(team) = (r) => team == 7 ? r : reply 403 \"no\"\nroutes /t/:team(int) | member_of(team) { route POST /move { json: { to: int } } = `${team} ${to}` }\n");
    assert_eq!(get(&mut it, "/a/q?__request=x", &[("ok", "1")]).text(), "q");
    assert_eq!(get(&mut it, "/a/q?__request=x", &[]).status, 401);
    assert_eq!(send(&mut it, "POST", "/t/7/move", &[("content-type", "application/json")], r#"{"to":3}"#).text(), "7 3");
    assert_eq!(send(&mut it, "POST", "/t/8/move", &[("content-type", "application/json")], r#"{"to":3}"#).status, 403);
}

#[test]
fn a_block_body_in_braces_is_still_a_body() {
    let mut it = app("let n = 5\nroute GET /a { \"hi\" }\nroute GET /b { n }\nroute GET /c {\n    request.method\n}\n");
    assert_eq!(get(&mut it, "/a", &[]).text(), "hi");
    assert_eq!(get(&mut it, "/b", &[]).text(), "5");
    assert_eq!(get(&mut it, "/c", &[]).text(), "GET");
}

// The review of the guard dedupe: a group that only overlaps another still asks.
#[test]
fn an_overlapping_group_asks_as_a_nested_one_does_not_twice() {
    let mut it = app(
        r#"
let seen = []
fn note(r) {
    seen = seen | push(r.path)
    r
}
fn auth(r) = r.headers.ok == "1" ? r : reply 403 "no"
routes /api/v2 | note { route GET /x = "v2" }
routes /api | auth { route GET /y = "y" }
route GET /seen = json.stringify(seen)
"#,
    );
    // `/api/v2/zzz` is under both: `note` lets it on, `auth` refuses.
    assert_eq!(get(&mut it, "/api/v2/zzz", &[]).status, 403);
    assert_eq!(send(&mut it, "POST", "/api/v2/x", &[], "").status, 403);
    assert_eq!(get(&mut it, "/api/v2/zzz", &[("ok", "1")]).status, 404);
    assert_eq!(get(&mut it, "/seen", &[]).text(), r#"["/api/v2/zzz","/api/v2/x","/api/v2/zzz"]"#);
}
