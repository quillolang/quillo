//! What one viewer may and may not get of another's, through anything kept between requests
//! (FEEDBACK55.md, Q-878 to Q-888; branch feedback55): the view cache, `live` runs, the values
//! kept for patches, what a worker keeps, and what a cache in front is told.

mod support;

use quillo::check::{check_source, Severity};
use quillo::server::HttpRequest;
use quillo::Interp;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::{send, serve, serve_with, temp_dir, Answer, Server};

/// A folder holding `files`, its `main.qwe` checked first.
fn app(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = temp_dir(&format!("viewers-{}", name));
    for (path, text) in files {
        let p = d.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
    }
    let main = std::fs::read_to_string(d.join("main.qwe")).unwrap();
    let c = check_source(&main, &d.join("main.qwe").display().to_string(), &d);
    let bad: Vec<String> = c.diagnostics.iter().filter(|x| x.severity == Severity::Error).map(|x| format!("{}: {}", x.line, x.message)).collect();
    assert!(bad.is_empty(), "{:?}", bad);
    d
}

fn run(d: &Path) -> Server {
    serve_with(&d.join("main.qwe"), &["--metrics"])
}

fn one(name: &str, src: &str) -> (PathBuf, Server) {
    let d = app(name, &[("main.qwe", src)]);
    let s = run(&d);
    (d, s)
}

/// What each `<p>` of a page says.
fn shown(page: &str) -> Vec<String> {
    page.split("<p").skip(1).map(|p| p.split_once('>').unwrap().1.split("</p>").next().unwrap().to_string()).collect()
}

fn page(s: &Server, path: &str, headers: &[(&str, &str)]) -> Vec<String> {
    shown(&send(s.port, "GET", path, headers, "").body)
}

/// The session cookie `/as/:who` gives.
fn sign_in(s: &Server, who: &str) -> String {
    let a = send(s.port, "GET", &format!("/as/{}", who), &[], "");
    a.header("set-cookie").unwrap().split(';').next().unwrap().to_string()
}

const SIGNS_IN: &str = "route GET /as/:who { set_session({ who }); \"ok\" }\n";

/// A refresh of a loader, as a page's script sends it: JSON and the page's headers, and those
/// `headers` say.
fn refresh(s: &Server, f: &str, headers: &[(&str, &str)]) -> Answer {
    let mut all = vec![("x-quillo-live", "1"), ("sec-fetch-site", "same-origin")];
    all.extend_from_slice(headers);
    send(s.port, "POST", &format!("/_quillo/rpc/{}", f), &all, "[]")
}

fn value(a: &Answer) -> serde_json::Value {
    let j: serde_json::Value = serde_json::from_str(&a.body).unwrap_or_else(|_| panic!("{}", a.body));
    j["ok"].clone()
}

fn metric(s: &Server, outcome: &str) -> i64 {
    let name = format!("quillo_live_runs_total{{outcome=\"{}\"}}", outcome);
    let text = send(s.port, "GET", "/_quillo/metrics", &[], "").body;
    text.lines().find_map(|l| l.strip_prefix(name.as_str()).and_then(|r| r.trim().parse().ok())).unwrap_or(0)
}

/// A component `C<name>` showing the loader `name`, and a page of it at `/<name>`, for each.
fn pages_of(names: &[&str]) -> String {
    names
        .iter()
        .map(|n| format!("component C{n}() {{\n    state v = live({n})\n    <p>{{v}}</p>\n}}\nroute GET /{n} = <html><body><C{n}/></body></html>\n"))
        .collect()
}

/// The program in this process, for what is told by the job token.
fn direct(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "test.qwe") {
        panic!("{}", e);
    }
    it
}

fn ask(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut h: Vec<(String, String)> = vec![("Host".into(), "localhost".into())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: h, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() }).text()
}

// ---- Q-878: the view cache ------------------------------------------------------------------

const USERS: &str = "server fn me() = { who: request.forwarded.authorization }\nserver fn plan() = { tenant: tenant() }\n";

// `__service` was not in the list of what cannot be kept, and a list of those is what lets the
// next builtin in too: a visitor's answer in a view was everyone's until the server stopped.
#[test]
fn a_view_that_calls_a_service_is_no_ones_to_keep() {
    let d = app("service-view", &[
        ("users.qwe", USERS),
        (
            "main.qwe",
            "service users = \"users.qwe\" { forward: [\"authorization\"] }\nview Me() {\n    let m = users.me()\n    <p>{m.who}</p>\n}\nroute GET /me = <html><body><Me/></body></html>\n",
        ),
    ]);
    let env = [("QUILLO_SERVICE_SECRET", "abcdefghijklmnopqrstuvwxyz012345abcdefgh")];
    let users = serve(&d.join("users.qwe"), &env);
    let url = format!("http://127.0.0.1:{}", users.port);
    let env = [env[0], ("QUILLO_SERVICE_USERS", url.as_str())];
    let s = serve(&d.join("main.qwe"), &env);
    assert_eq!(page(&s, "/me", &[("authorization", "Bearer alice")]), ["Bearer alice"]);
    assert_eq!(page(&s, "/me", &[("authorization", "Bearer bob")]), ["Bearer bob"]);
    // What came back may be theirs: a cache in front is told.
    let a = send(s.port, "GET", "/me", &[("authorization", "Bearer carol")], "");
    assert_eq!(a.header("cache-control"), Some("private"));
}

#[test]
fn a_tenants_answer_from_a_service_is_not_the_next_tenants() {
    let d = app("service-tenant", &[
        ("users.qwe", USERS),
        ("main.qwe", "service users = \"users.qwe\"\ntenant_from((r) => r.cookies.org)\nview Plan() {\n    let p = users.plan()\n    <p>{p.tenant}</p>\n}\nroute GET / = <html><body><Plan/></body></html>\n"),
    ]);
    let s = run(&d);
    assert_eq!(page(&s, "/", &[("cookie", "org=acme")]), ["acme"]);
    assert_eq!(page(&s, "/", &[("cookie", "org=globex")]), ["globex"]);
}

// Time too: a date read in a view stayed the date it was first read on.
#[test]
fn a_view_that_reads_the_clock_is_not_kept() {
    let mut it = direct("view Now() = <p>{date.now()}</p>\nview Sum(a: int) = <p>{format_number(a + 1)}{join([\"x\"], \",\")}</p>\nroute GET /n = <Now/>\nroute GET /s = <Sum a={1}/>\n");
    ask(&mut it, "GET", "/n", &[], "");
    ask(&mut it, "GET", "/n", &[], "");
    assert_eq!(it.view_cache_stats().0, 0);
    // What depends on its arguments alone still is.
    ask(&mut it, "GET", "/s", &[], "");
    ask(&mut it, "GET", "/s", &[], "");
    assert_eq!(it.view_cache_stats().0, 1);
}

// ---- Q-879: the kept tenant -----------------------------------------------------------------

const TENANTS: &str = "tenant_from((r) => r.cookies.org)
server fn load_a() = tenant()
server fn load_b() = tenant()
server fn fixed() = with_tenant(\"acme\", () => tenant())
component A() {
    state a = live(load_a)
    <p>{a}</p>
}
component B() {
    state b = live(load_b)
    <p>{b}</p>
}
component F() {
    state f = live(fixed)
    <p>{f}</p>
}
route GET / = <html><body><A/><B/></body></html>
route GET /after {
    let t = tenant()
    <html><body><p>{t}</p><B/></body></html>
}
route GET /fixed = <html><body><F/></body></html>
";

// The second loader of a page took the kept name, and was a run of nobody's.
#[test]
fn a_second_loader_is_its_tenants() {
    let (_d, s) = one("tenant-two", TENANTS);
    assert_eq!(page(&s, "/", &[("cookie", "org=acme")]), ["acme", "acme"]);
    assert_eq!(page(&s, "/", &[("cookie", "org=globex")]), ["globex", "globex"]);
}

#[test]
fn a_loader_after_a_route_that_read_the_tenant_is_its_tenants() {
    let (_d, s) = one("tenant-after", TENANTS);
    assert_eq!(page(&s, "/after", &[("cookie", "org=acme")]), ["acme", "acme"]);
    assert_eq!(page(&s, "/after", &[("cookie", "org=globex")]), ["globex", "globex"]);
}

#[test]
fn a_tenants_rows_stay_its_own() {
    let d = app("tenant-rows", &[(
        "main.qwe",
        "tenant_from((r) => r.cookies.org)\ndb.open(\"data/{tenant}.db\")\nserver fn load_a() = db.one(\"select v from t\").v\nserver fn load_b() = db.one(\"select v from t\").v\ncomponent A() {\n    state a = live(load_a)\n    <p>{a}</p>\n}\ncomponent B() {\n    state b = live(load_b)\n    <p>{b}</p>\n}\nroute GET / = <html><body><A/><B/></body></html>\nroute GET /make/:v {\n    let _m = new_tenant(v, () => db.exec(\"create table t (v text)\"))\n    with_tenant(v, () => db.exec(\"insert into t values (?)\", v))\n    \"ok\"\n}\n",
    )]);
    std::fs::create_dir_all(d.join("data")).unwrap();
    let s = run(&d);
    send(s.port, "GET", "/make/acme", &[], "");
    send(s.port, "GET", "/make/globex", &[], "");
    assert_eq!(page(&s, "/", &[("cookie", "org=acme")]), ["acme", "acme"]);
    assert_eq!(page(&s, "/", &[("cookie", "org=globex")]), ["globex", "globex"]);
}

// The tenant in force is in the run's key: a tenant's visitors share what reads no one of them.
#[test]
fn a_tenants_visitors_share_its_run() {
    let (_d, s) = one("tenant-named", TENANTS);
    assert_eq!(page(&s, "/fixed", &[("cookie", "org=one; me=1")]), ["acme"]);
    assert_eq!(page(&s, "/fixed", &[("cookie", "org=one; me=2")]), ["acme"]);
    assert_eq!((metric(&s, "ran"), metric(&s, "shared")), (1, 1));
    assert_eq!(page(&s, "/", &[("cookie", "org=two; me=1")]), ["two", "two"]);
    assert_eq!(page(&s, "/", &[("cookie", "org=two; me=2")]), ["two", "two"]);
    assert_eq!((metric(&s, "ran"), metric(&s, "shared")), (3, 3));
}

// However it was named: `with_tenant` around a page gave one tenant's rows to the next.
#[test]
fn a_tenant_named_around_a_page_is_the_runs() {
    let d = app("tenant-around", &[(
        "main.qwe",
        "tenant_from((r) => r.cookies.org)\ndb.open(\"data/{tenant}.db\")\nserver fn rows() = db.one(\"select v from t\").v\nserver fn name() = tenant()\ncomponent R() {\n    state v = live(rows)\n    <p>{v}</p>\n}\ncomponent N() {\n    state v = live(name)\n    <p>{v}</p>\n}\nroute GET /make/:t = new_tenant(t, () => {\n    db.exec(\"create table t(v text)\")\n    db.exec(\"insert into t values (?)\", `secret of ${t}`)\n    \"made\"\n})\nroute GET /as/:org { set_session({ org }); \"ok\" }\nroute GET /mine = with_tenant(session().org, () => <html><body><R/></body></html>)\nroute GET /rows/:org = with_tenant(org, () => <html><body><R/></body></html>)\nroute GET /name/:org = with_tenant(org, () => <html><body><N/></body></html>)\n",
    )]);
    std::fs::create_dir_all(d.join("data")).unwrap();
    let s = run(&d);
    send(s.port, "GET", "/make/acme", &[], "");
    send(s.port, "GET", "/make/globex", &[], "");
    let (acme, globex) = (sign_in(&s, "acme"), sign_in(&s, "globex"));
    assert_eq!(page(&s, "/mine", &[("cookie", &acme)]), ["secret of acme"]);
    assert_eq!(page(&s, "/mine", &[("cookie", &globex)]), ["secret of globex"]);
    assert_eq!(page(&s, "/rows/acme", &[]), ["secret of acme"]);
    assert_eq!(page(&s, "/rows/globex", &[]), ["secret of globex"]);
    assert_eq!(page(&s, "/name/acme", &[]), ["acme"]);
    assert_eq!(page(&s, "/name/globex", &[]), ["globex"]);
}

// The view cache's key holds no tenant, as what it keeps depends on none: a view that reads one
// is not kept, named by `with_tenant` too.
#[test]
fn a_view_under_a_named_tenant_is_not_kept() {
    let (_d, s) = one(
        "tenant-view",
        "view Who() = <p>{tenant()}</p>\nview Fixed() = <p>same</p>\nroute GET /who/:org = with_tenant(org, () => <html><body><Who/><Fixed/></body></html>)\n",
    );
    assert_eq!(page(&s, "/who/acme", &[]), ["acme", "same"]);
    assert_eq!(page(&s, "/who/globex", &[]), ["globex", "same"]);
}

// With no rule, or with the host's, nothing changed: the host is in the run's key.
#[test]
fn a_run_with_no_tenant_is_shared() {
    let (_d, s) = one("tenant-none", &format!("server fn load() = tenant()\n{}", pages_of(&["load"])));
    assert_eq!(page(&s, "/load", &[("cookie", "a=1")]), [""]);
    assert_eq!(page(&s, "/load", &[("cookie", "a=2")]), [""]);
    assert_eq!((metric(&s, "ran"), metric(&s, "shared")), (1, 1));
}

#[test]
fn a_tenant_told_by_the_host_is_kept_apart_by_it() {
    let (_d, s) = one("tenant-host", &format!("tenant_from(\"example.com\")\nserver fn load() = tenant()\n{}", pages_of(&["load"])));
    assert_eq!(page(&s, "/load", &[("host", "acme.example.com")]), ["acme"]);
    assert_eq!(page(&s, "/load", &[("host", "globex.example.com")]), ["globex"]);
}

// ---- Q-880: code that names `request` where the walker did not look ---------------------------

const DEFAULTS: &str = "server fn param(who = request.cookies.uid) = who
server fn pattern() {
    let { who = request.cookies.uid } = {}
    return who
}
server fn lambda() {
    let f = (x = request.cookies.uid) => x
    return f()
}
server fn inner() {
    fn g(x = request.cookies.uid) = x
    return g()
}
server fn arm() = match {} {
    { who = request.cookies.uid } => who,
    _ => \"no\",
}
server fn each() {
    let seen = []
    for { who = request.cookies.uid } in [{}] {
        seen = push(seen, who)
    }
    return seen[0]
}
server fn plain() = \"everyone's\"
";

#[test]
fn a_default_that_reads_the_request_is_the_viewers() {
    let names = ["param", "pattern", "lambda", "inner", "arm", "each"];
    let (_d, s) = one("defaults", &format!("{}{}", DEFAULTS, pages_of(&names)));
    for n in names {
        assert_eq!(page(&s, &format!("/{}", n), &[("cookie", "uid=alice")]), ["alice"], "{}", n);
        assert_eq!(page(&s, &format!("/{}", n), &[("cookie", "uid=bob")]), ["bob"], "{}", n);
    }
    std::thread::sleep(Duration::from_millis(2100));
    for n in names {
        assert_eq!(value(&refresh(&s, n, &[("cookie", "uid=alice")])), "alice", "{}", n);
        assert_eq!(value(&refresh(&s, n, &[("cookie", "uid=bob")])), "bob", "{}", n);
    }
}

// What must stay: code that names no `request` is one run for everyone.
#[test]
fn a_loader_that_names_no_request_stays_shared() {
    let (_d, s) = one("no-default", &format!("{}{}", DEFAULTS, pages_of(&["plain"])));
    assert_eq!(page(&s, "/plain", &[("cookie", "uid=alice")]), ["everyone&#39;s"]);
    assert_eq!(page(&s, "/plain", &[("cookie", "uid=bob")]), ["everyone&#39;s"]);
    assert_eq!((metric(&s, "ran"), metric(&s, "shared")), (1, 1));
}

// The walker is `check`'s too: a name read only in a default was unseen by every check using it.
#[test]
fn the_walker_goes_into_what_holds_code() {
    for (src, why) in [
        ("fn f(x = 1 + seen) = x\n", "a parameter's default"),
        ("fn f() {\n    let { a = seen } = {}\n    a\n}\n", "a pattern's default"),
        ("fn f() = match {} {\n    { a = seen } => a,\n    _ => 1,\n}\n", "an arm's pattern"),
        ("fn f() {\n    for { a = seen } in [{}] {\n        print(a)\n    }\n}\n", "a loop's pattern"),
        ("fn f() {\n    let g = (x = seen) => x\n    g()\n}\n", "a lambda's default"),
        ("fn f() {\n    fn g(x = seen) = x\n    g()\n}\n", "an inner function's default"),
    ] {
        let stmts = quillo::parser::Parser::new(src).parse_program().unwrap();
        let quillo::ast::Stmt::Fn(d) = &stmts[0] else { panic!("{}", src) };
        let mut found = false;
        quillo::check::walk_fn(d, &mut |e| found |= matches!(e, quillo::ast::Expr::Ident(n, _) if n == "seen"));
        assert!(found, "{}", why);
    }
}

// ---- Q-881: who is a viewer ------------------------------------------------------------------

const READERS: &str = "server fn by_key() = request.headers[\"x-api-key\"]
server fn by_query() = request.query.token
server fn by_path() = url().path
server fn by_form() = request.form.who
server fn by_ip() = request.headers[\"x-forwarded-for\"]
server fn nobody() = \"same\"
server fn mine() = session().who
";

fn readers(name: &str) -> (PathBuf, Server) {
    let routes = "component F() {\n    state v = live(by_form)\n    <p>{v}</p>\n}\nroute POST /form = <html><body><F/></body></html>\ncomponent P() {\n    state v = live(by_path)\n    <p>{v}</p>\n}\nroute GET /p/:x = <html><body><P/></body></html>\n";
    one(name, &format!("{}{}{}{}", READERS, pages_of(&["by_key", "by_query", "by_ip", "nobody", "mine"]), routes, SIGNS_IN))
}

// Two visitors at one address, one browser: all that tells them apart is what the program reads.
#[test]
fn a_header_of_the_programs_own_tells_viewers_apart() {
    let (_d, s) = readers("own-header");
    assert_eq!(page(&s, "/by_key", &[("x-api-key", "alice")]), ["alice"]);
    assert_eq!(page(&s, "/by_key", &[("x-api-key", "bob")]), ["bob"]);
    assert_eq!(value(&refresh(&s, "by_key", &[("x-api-key", "carol")])), "carol");
    assert_eq!(value(&refresh(&s, "by_key", &[("x-api-key", "dave")])), "dave");
}

#[test]
fn the_query_and_the_path_tell_viewers_apart() {
    let (_d, s) = readers("query");
    assert_eq!(page(&s, "/by_query?token=alice", &[]), ["alice"]);
    assert_eq!(page(&s, "/by_query?token=bob", &[]), ["bob"]);
    assert_eq!(page(&s, "/p/a", &[]), ["/p/a"]);
    assert_eq!(page(&s, "/p/b", &[]), ["/p/b"]);
    let call = |t: &str| value(&send(s.port, "POST", &format!("/_quillo/rpc/by_query?token={}", t), &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin")], "[]"));
    assert_eq!(call("carol"), "carol");
    assert_eq!(call("dave"), "dave");
}

// Behind a proxy not named with `--trust-proxy`: every visitor is its address.
#[test]
fn a_forwarded_address_tells_viewers_apart() {
    let (_d, s) = readers("forwarded");
    assert_eq!(page(&s, "/by_ip", &[("x-forwarded-for", "1.1.1.1")]), ["1.1.1.1"]);
    assert_eq!(page(&s, "/by_ip", &[("x-forwarded-for", "2.2.2.2")]), ["2.2.2.2"]);
}

#[test]
fn what_a_form_sent_tells_viewers_apart() {
    let (_d, s) = readers("form");
    let post = |who: &str| shown(&send(s.port, "POST", "/form", &[("content-type", "application/x-www-form-urlencoded")], &format!("who={}", who)).body);
    assert_eq!(post("alice"), ["alice"]);
    assert_eq!(post("bob"), ["bob"]);
}

/// What a raw request's answer says, for what `send` cannot write: two lines of one header.
fn raw(port: u16, lines: &[String]) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "{}\r\n\r\n[]", lines.join("\r\n")).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.split_once("\r\n\r\n").map_or(out.clone(), |(_, b)| b.to_string())
}

// `request` reads the last line, and the run's viewer was told by the first.
#[test]
fn every_cookie_line_tells_viewers_apart() {
    let (_d, s) = readers("two-lines");
    let (alice, bob) = (sign_in(&s, "alice"), sign_in(&s, "bob"));
    let call = |cookie: &str| {
        let b = raw(s.port, &[
            "POST /_quillo/rpc/mine HTTP/1.1".into(),
            "Host: localhost".into(),
            "Connection: close".into(),
            "Content-Type: application/json".into(),
            "Content-Length: 2".into(),
            "X-Quillo-Live: 1".into(),
            "Sec-Fetch-Site: same-origin".into(),
            "Cookie: theme=dark".into(),
            format!("Cookie: {}", cookie),
        ]);
        serde_json::from_str::<serde_json::Value>(&b).unwrap()["ok"].clone()
    };
    assert_eq!(call(&alice), "alice");
    assert_eq!(call(&bob), "bob");
}

// What told viewers apart was a list of what identifies nobody, and each of these was on it: Bob
// was given `token=ALICE`. A run that read the request is now no one else's at all.
#[test]
fn nothing_that_read_the_request_goes_to_anyone_else() {
    let (_d, s) = one(
        "headers-read",
        "server fn by(name) = request.headers[name]\nserver fn by_id() = request.id\n",
    );
    for (name, alice, bob) in [
        ("referer", "https://x.test/invite?token=ALICE", "https://x.test/invite?token=BOB"),
        ("if-none-match", "\"alice\"", "\"bob\""),
        ("x-quillo-user", "alice", "bob"),
        ("tracestate", "user=alice", "user=bob"),
        ("accept-encoding", "gzip", "br"),
        ("cache-control", "alice", "bob"),
    ] {
        let call = |v: &str| {
            let body = format!("[\"{}\"]", name);
            let a = send(s.port, "POST", "/_quillo/rpc/by", &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin"), (name, v)], &body);
            value(&a)
        };
        assert_eq!(call(alice), alice, "{}", name);
        assert_eq!(call(bob), bob, "{}", name);
    }
    assert_eq!(value(&refresh(&s, "by_id", &[("x-request-id", "rid-alice")])), "rid-alice");
    assert_eq!(value(&refresh(&s, "by_id", &[("x-request-id", "rid-bob")])), "rid-bob");
}

// Not even the same person's: a tab, a reload and a refresh each run it, and each is theirs.
#[test]
fn a_run_that_read_the_visitor_is_not_kept_for_them_either() {
    let (_d, s) = readers("own-tabs");
    let me = sign_in(&s, "alice");
    for _ in 0..3 {
        assert_eq!(page(&s, "/mine", &[("cookie", &me)]), ["alice"]);
    }
    for _ in 0..2 {
        assert_eq!(value(&refresh(&s, "mine", &[("cookie", &me)])), "alice");
    }
    assert_eq!((metric(&s, "ran"), metric(&s, "shared")), (5, 0));
}

// And a run that read nothing is one run, for anyone, on any page.
#[test]
fn a_run_that_read_nothing_is_everyones() {
    let (_d, s) = readers("nothing");
    for (path, h) in [("/nobody", ("cookie", "a=1")), ("/nobody?x=2", ("x-api-key", "b")), ("/nobody", ("user-agent", "c")), ("/nobody", ("x-forwarded-for", "4.4.4.4"))] {
        assert_eq!(page(&s, path, &[h]), ["same"]);
    }
    assert_eq!(value(&refresh(&s, "nobody", &[("cookie", "z=1")])), "same");
    assert_eq!((metric(&s, "ran"), metric(&s, "shared")), (1, 4));
}

// A header that identifies somebody counts even with a name nobody listed, in any case.
#[test]
fn a_header_nobody_thought_of_counts() {
    let (_d, s) = one("unlisted", &format!("server fn all() = join(values(request.headers), \",\")\n{}", pages_of(&["all"])));
    let a = page(&s, "/all", &[("X-New-Identity", "one")]);
    let b = page(&s, "/all", &[("x-new-identity", "two")]);
    assert!(a[0].contains("one") && b[0].contains("two"), "{:?} {:?}", a, b);
}

// Two lines of one header, in either order: `request` has the last, and the run was everyone's.
#[test]
fn either_line_of_a_header_is_the_senders() {
    let (_d, s) = one("two-lines-any", "server fn who() = request.headers[\"x-who\"]\n");
    let call = |one: &str, two: &str| {
        let b = raw(s.port, &[
            "POST /_quillo/rpc/who HTTP/1.1".into(),
            "Host: localhost".into(),
            "Connection: close".into(),
            "Content-Type: application/json".into(),
            "Content-Length: 2".into(),
            "X-Quillo-Live: 1".into(),
            "Sec-Fetch-Site: same-origin".into(),
            format!("X-Who: {}", one),
            format!("X-Who: {}", two),
        ]);
        serde_json::from_str::<serde_json::Value>(&b).unwrap()["ok"].clone()
    };
    assert_eq!(call("alice", "bob"), "bob");
    assert_eq!(call("bob", "alice"), "alice");
}

// An argument a key cannot hold: a run that took it is not shared, as null it was any other's.
#[test]
fn an_argument_a_key_cannot_hold_is_no_ones_key() {
    let (_d, s) = one(
        "unkeyed",
        "route GET /as/:who { set_session({ who }); \"ok\" }\nserver fn by_fn(f) = str(f())\nserver fn by_num(x) = str(x)\ncomponent F(me) {\n    state v = live(by_fn, () => me)\n    <p>{v}</p>\n}\ncomponent N() {\n    state a = live(by_num, math.nan)\n    state b = live(by_num, math.inf)\n    state c = live(by_num, -math.inf)\n    state d = live(by_num, [math.nan])\n    <p>{a} {b} {c} {d}</p>\n}\nroute GET /f {\n    let me = session().who\n    <html><body><F me={me}/></body></html>\n}\nroute GET /n = <html><body><N/></body></html>\n",
    );
    let (alice, bob) = (sign_in(&s, "alice"), sign_in(&s, "bob"));
    assert_eq!(page(&s, "/f", &[("cookie", &alice)]), ["alice"]);
    assert_eq!(page(&s, "/f", &[("cookie", &bob)]), ["bob"]);
    let shown = page(&s, "/n", &[]);
    assert!(shown[0].contains("inf") && shown[0].contains("-inf"), "{:?}", shown);
    assert_eq!(metric(&s, "shared"), 0);
}

// A loader reading only what the URL says: the same answer each time, as public as the page.
#[test]
fn a_loader_of_what_the_url_says_keeps_the_page_public() {
    let (_d, s) = one("path-loader", "server fn where() = request.path\ncomponent W() {\n    state v = live(where)\n    <p>{v}</p>\n}\nroute GET /w = <html><body><W/></body></html>\n");
    for _ in 0..3 {
        let a = send(s.port, "GET", "/w", &[], "");
        assert_eq!((shown(&a.body), a.header("cache-control")), (vec!["/w".to_string()], None));
    }
}

// ---- Q-882: mounted programs ----------------------------------------------------------------

const MOUNTED: &str = "server fn load() = { v: \"NAME\" }\ncomponent A() {\n    state a = live(load)\n    <p>{a.v}</p>\n}\nroute GET / = <html><body><A/></body></html>\n";

#[test]
fn a_mounted_programs_run_is_its_own() {
    let (one_src, two_src) = (MOUNTED.replace("NAME", "one"), MOUNTED.replace("NAME", "two"));
    let root = MOUNTED.replace("NAME", "root");
    let main = format!("mount /one = \"one.qwe\"\nmount /two = \"two.qwe\"\n{}", root);
    let d = app("mounts", &[("one.qwe", &one_src), ("two.qwe", &two_src), ("main.qwe", &main)]);
    let s = run(&d);
    assert_eq!(page(&s, "/one/", &[]), ["one"]);
    assert_eq!(page(&s, "/two/", &[]), ["two"]);
    assert_eq!(page(&s, "/", &[]), ["root"]);
    let call = |at: &str| value(&send(s.port, "POST", &format!("{}/_quillo/rpc/load", at), &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin")], "[]"))["v"].clone();
    assert_eq!((call("/one"), call("/two"), call("")), ("one".into(), "two".into(), "root".into()));
    // And each still shares its own, a page with its refreshes.
    assert_eq!(page(&s, "/one/", &[]), ["one"]);
    assert_eq!((metric(&s, "ran"), metric(&s, "shared")), (3, 4));
}

// ---- Q-883: what a request leaves on its worker ---------------------------------------------

const AFTER_A_FAILURE: &str = "fn boom() {
    let e = {}
    return e.a.b.c()
}
let seen = \"nothing yet\"
server fn probe() = `path=${request.path ?? \"none\"} query=${request.query.secret ?? \"none\"}`
fn job() {
    seen = probe()
}
schedule(\"1h\", job)
channel.on(\"c\", (msg, peer) => {
    seen = probe()
})
route GET /fail ? { page: int = boom() } = \"never\"
route GET /seen = seen
";

fn job_header() -> (&'static str, &'static str) {
    ("x-quillo-job", quillo::jobs::token())
}

// The route's pattern failed with the request's map still to make: `pending_request` kept pointing
// at the request, and the next job on this worker read through it.
#[test]
fn a_failed_pattern_leaves_nothing_for_a_job() {
    let mut it = direct(AFTER_A_FAILURE);
    ask(&mut it, "GET", "/fail?secret=reset-token-of-alice", &[], "");
    ask(&mut it, "POST", "/_quillo/job/job", &[job_header()], "");
    assert_eq!(ask(&mut it, "GET", "/seen", &[], ""), "path=none query=none");
}

#[test]
fn a_failed_pattern_leaves_nothing_for_a_channel() {
    let mut it = direct(AFTER_A_FAILURE);
    ask(&mut it, "GET", "/fail?secret=reset-token-of-alice", &[], "");
    ask(&mut it, "POST", "/_quillo/channel/c", &[job_header()], "{\"k\":\"m\",\"p\":\"1\",\"s\":\"{}\",\"v\":\"1\"}\n");
    assert_eq!(ask(&mut it, "GET", "/seen", &[], ""), "path=none query=none");
}

// What a service's call left, with the visitor's forwarded headers in it.
#[test]
fn a_services_call_leaves_nothing_for_a_job() {
    let d = app("service-job", &[
        ("users.qwe", "server fn me() = { who: request.forwarded.authorization }\nserver fn audit() = request.forwarded?.authorization ?? \"nobody\"\nfn job() {\n    write_file(\"job.log\", audit())\n}\nschedule(\"1s\", job)\n"),
        ("main.qwe", "service users = \"users.qwe\" { forward: [\"authorization\"] }\nroute GET /me {\n    let m = users.me()\n    <p>{m.who}</p>\n}\n"),
    ]);
    let secret = ("QUILLO_SERVICE_SECRET", "abcdefghijklmnopqrstuvwxyz012345abcdefgh");
    let users = serve(&d.join("users.qwe"), &[secret]);
    let url = format!("http://127.0.0.1:{}", users.port);
    let s = serve(&d.join("main.qwe"), &[secret, ("QUILLO_SERVICE_USERS", url.as_str())]);
    // The job that follows a call, several times over: which comes after which is the clock's.
    for _ in 0..3 {
        assert_eq!(page(&s, "/me", &[("authorization", "Bearer alice-token")]), ["Bearer alice-token"]);
        std::thread::sleep(Duration::from_millis(1100));
        assert_eq!(std::fs::read_to_string(d.join("job.log")).unwrap_or_default().trim(), "nobody");
    }
}

// What must stay: the request is there for all of the answer, in `parallel` too.
#[test]
fn the_request_is_there_while_it_is_answered() {
    let (_d, s) = one(
        "still-there",
        "server fn path() = request.path\nserver fn q() = request.query.a\nroute GET /x {\n    let [a, b] = parallel(path, q)\n    `${a} ${b} ${url().path}`\n}\nroute GET /y = path()\n",
    );
    assert_eq!(send(s.port, "GET", "/x?a=1", &[], "").body, "/x 1 /x");
    assert_eq!(send(s.port, "GET", "/y", &[], "").body, "/y");
}

// A page with components works its locale out after the route, and a rule that waits lets the
// next request run: the stream the first left for `handle` went to the second's connection.
#[test]
fn a_stream_goes_to_who_it_was_made_for() {
    let (_d, s) = one(
        "stream-yield",
        "translations({ en: { hi: \"Hello\" } })\nlocale_from((r) => {\n    sleep(80)\n    return \"en\"\n})\ncomponent C() {\n    <i>x</i>\n}\nroute GET /s/:who {\n    let _p = <C/>\n    return stream((send) => send(`stream of ${who}`))\n}\n",
    );
    let port = s.port;
    let alice = std::thread::spawn(move || send(port, "GET", "/s/alice", &[], ""));
    std::thread::sleep(Duration::from_millis(30));
    let bob = send(port, "GET", "/s/bob", &[], "");
    let alice = alice.join().unwrap();
    assert!(alice.body.contains("stream of alice") && !alice.body.contains("bob"), "{} {}", alice.status, alice.body);
    assert!(bob.body.contains("stream of bob") && !bob.body.contains("alice"), "{} {}", bob.status, bob.body);
}

// ---- Q-884: the values kept for patches ------------------------------------------------------

const BOARD: &str = "fn rows() = range(0, 40).map((i) => ({ id: i, title: `Public row ${i}` }))
server fn board() = { rows: rows(), pin: session().who }
server fn open_board() = { rows: rows(), stamp: 1 }
";

fn board(name: &str) -> (PathBuf, Server) {
    one(name, &format!("{}{}{}", BOARD, pages_of(&["board", "open_board"]), SIGNS_IN))
}

fn hash_of(a: &Answer) -> String {
    let j: serde_json::Value = serde_json::from_str(&a.body).unwrap();
    j["h"].as_str().unwrap().to_string()
}

// Alice's value, and its hash as the server wrote it, however Bob came by it: he is answered as if
// nobody had it, not with what differs from it.
#[test]
fn another_viewers_hash_gets_no_patch() {
    let (_d, s) = board("oracle");
    let (alice, bob) = (sign_in(&s, "alice"), sign_in(&s, "bob"));
    let hers = hash_of(&refresh(&s, "board", &[("cookie", &alice)]));
    let a = refresh(&s, "board", &[("cookie", &bob), ("x-quillo-had", &hers)]);
    assert!(a.body.starts_with("{\"ok\":"), "{}", a.body);
    let unknown = refresh(&s, "board", &[("cookie", &bob), ("x-quillo-had", "0123456789abcdef")]);
    assert_eq!(a.body, unknown.body);
}

/// The hash a page was given for its state's value.
fn hash_in_page(page: &str) -> String {
    page.split("\"h\":\"").nth(1).or_else(|| page.split("&quot;h&quot;:&quot;").nth(1)).unwrap()[..16].to_string()
}

// The same from what her page was made with: a page's values are kept as a refresh's are.
#[test]
fn another_viewers_page_gets_no_patch() {
    let (_d, s) = board("oracle-page");
    let (alice, bob) = (sign_in(&s, "alice"), sign_in(&s, "bob"));
    let hers = hash_in_page(&send(s.port, "GET", "/board", &[("cookie", &alice)], "").body);
    let a = refresh(&s, "board", &[("cookie", &bob), ("x-quillo-had", &hers)]);
    assert!(a.body.starts_with("{\"ok\":"), "{}", &a.body[..80.min(a.body.len())]);
}

// A value for everyone at one tenant is everyone's there, not at the next.
#[test]
fn another_tenants_value_gets_no_patch() {
    let (_d, s) = one(
        "oracle-tenant",
        &format!("tenant_from((r) => r.cookies.org)\n{}\nserver fn t_board() = {{ rows: rows(), t: tenant() }}\n{}", BOARD, pages_of(&["t_board"])),
    );
    let hers = hash_of(&refresh(&s, "t_board", &[("cookie", "org=acme")]));
    let a = refresh(&s, "t_board", &[("cookie", "org=globex"), ("x-quillo-had", &hers)]);
    assert!(a.body.starts_with("{\"ok\":"), "{}", &a.body[..80.min(a.body.len())]);
    assert_eq!(value(&a)["t"], "globex");
}

/// The hash `open` has on a server started with this secret, or none.
fn hash_with(name: &str, secret: Option<&str>) -> String {
    let d = app(name, &[("main.qwe", "server fn open() = { n: 1 }\nroute GET / = \"ok\"\n")]);
    let port = support::free_port();
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(["run", "main.qwe", "--port", &port.to_string()]).current_dir(&d).env_remove("QUILLO_SECRET").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if let Some(s) = secret {
        c.env("QUILLO_SECRET", s);
    }
    let _server = support::Guard::new(c.spawn().unwrap());
    let until = std::time::Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(std::time::Instant::now() < until, "server did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    let a = send(port, "POST", "/_quillo/rpc/open", &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin")], "[]");
    hash_of(&a)
}

// A key comes from the secret, the site's and no one else's, and with none each server has its own.
#[test]
fn a_hash_is_the_sites_and_no_one_elses() {
    assert_eq!(hash_with("secret-a1", Some("one secret of the site's")), hash_with("secret-a2", Some("one secret of the site's")));
    assert_ne!(hash_with("secret-b1", Some("one secret of the site's")), hash_with("secret-b2", Some("another")));
    assert_ne!(hash_with("secret-c1", None), hash_with("secret-c2", None));
}

// Two servers behind a balancer, one secret: a value's hash is the same on both, and a page that
// moves still gets a patch. A hash of no key was that too, and anyone's to work out.
#[test]
fn a_page_that_moves_between_servers_gets_a_patch() {
    let src = format!("{}\nlet n = 0\nroute GET /bump {{\n    n = n + 1\n    \"ok\"\n}}\nserver fn open() = {{ rows: rows(), n }}\n{}", BOARD, pages_of(&["open"]));
    let (_d, a) = one("moved-a", &src);
    let (_e, b) = one("moved-b", &src);
    let (on_a, on_b) = (refresh(&a, "open", &[]), refresh(&b, "open", &[]));
    assert_eq!(hash_of(&on_a), hash_of(&on_b));
    send(b.port, "GET", "/bump", &[], "");
    let moved = refresh(&b, "open", &[("x-quillo-had", &hash_of(&on_a))]);
    assert!(moved.body.starts_with("{\"patch\":"), "{}", &moved.body[..80.min(moved.body.len())]);
}

// What a page says it has is known by a key of this process's: nobody can make the hash of a guess.
#[test]
fn a_hash_is_not_one_anyone_can_work_out() {
    use std::hash::{Hash, Hasher};
    let (_d, s) = board("keyed");
    let bob = sign_in(&s, "bob");
    let a = refresh(&s, "open_board", &[("cookie", &bob)]);
    let j: serde_json::Value = serde_json::from_str(&a.body).unwrap();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    j["ok"].to_string().hash(&mut h);
    assert_ne!(format!("{:016x}", h.finish()), hash_of(&a));
}

// What must stay: a viewer's own value, from their page or their last refresh, patches.
#[test]
fn a_viewers_own_value_patches() {
    let (_d, s) = one("own-patch", &format!("{}\nlet n = 0\nroute GET /bump {{\n    n = n + 1\n    \"ok\"\n}}\nserver fn own() = {{ rows: rows(), who: session().who, n }}\n{}{}", BOARD, pages_of(&["own"]), SIGNS_IN));
    let alice = sign_in(&s, "alice");
    let h = hash_in_page(&send(s.port, "GET", "/own", &[("cookie", &alice)], "").body);
    send(s.port, "GET", "/bump", &[], "");
    let a = refresh(&s, "own", &[("cookie", &alice), ("x-quillo-had", &h)]);
    assert!(a.body.starts_with("{\"patch\":"), "{}", &a.body[..80.min(a.body.len())]);
    // And one more, from that refresh.
    let h = hash_of(&a);
    send(s.port, "GET", "/bump", &[], "");
    let a = refresh(&s, "own", &[("cookie", &alice), ("x-quillo-had", &h)]);
    assert!(a.body.starts_with("{\"patch\":"), "{}", &a.body[..80.min(a.body.len())]);
}

// And a value nobody owns patches for anyone, who could be sent it whole.
#[test]
fn an_everyone_value_patches_for_anyone() {
    let (_d, s) = one("open-patch", &format!("{}\nlet n = 0\nroute GET /bump {{\n    n = n + 1\n    \"ok\"\n}}\nserver fn open() = {{ rows: rows(), n }}\n{}", BOARD, pages_of(&["open"])));
    let h = hash_of(&refresh(&s, "open", &[("cookie", "a=1")]));
    send(s.port, "GET", "/bump", &[], "");
    let a = refresh(&s, "open", &[("cookie", "a=2"), ("x-quillo-had", &h)]);
    assert!(a.body.starts_with("{\"patch\":"), "{}", &a.body[..80.min(a.body.len())]);
}

// ---- Q-885: what a page given a run is told ---------------------------------------------------

const KEPT: &str = "server fn inner() = { who: session().who }
server fn outer() = { got: live(inner).who }
server fn everyone() = { n: 1 }
component I() {
    state a = live(inner)
    <p>{a.who}</p>
}
component O() {
    state b = live(outer)
    <p>{b.got}</p>
}
component E() {
    state e = live(everyone)
    <p>{e.n}</p>
}
route GET /inner = <html><body><I/></body></html>
route GET /outer = <html><body><O/></body></html>
route GET /everyone = <html><body><E/></body></html>
";

// The second load went out as nobody's page, given the first's run. Each now runs, and says so.
#[test]
fn every_load_of_a_viewers_page_is_theirs() {
    let (_d, s) = one("kept-page", &format!("{}{}", KEPT, SIGNS_IN));
    let me = sign_in(&s, "alice");
    for _ in 0..3 {
        let a = send(s.port, "GET", "/inner", &[("cookie", &me)], "");
        assert_eq!((shown(&a.body), a.header("cache-control")), (vec!["alice".to_string()], Some("private")));
        assert!(a.headers.iter().any(|(k, v)| k == "vary" && v == "Cookie"), "{:?}", a.headers);
    }
}

// A loader around another: what the inner run read, the outer did.
#[test]
fn a_loader_around_a_viewers_run_is_theirs() {
    let (_d, s) = one("kept-nested", &format!("{}{}", KEPT, SIGNS_IN));
    let (alice, bob) = (sign_in(&s, "alice"), sign_in(&s, "bob"));
    assert_eq!(page(&s, "/inner", &[("cookie", &alice)]), ["alice"]);
    assert_eq!(page(&s, "/outer", &[("cookie", &alice)]), ["alice"]);
    assert_eq!(page(&s, "/outer", &[("cookie", &bob)]), ["bob"]);
}

// What must stay: a run for everyone makes nobody's page private.
#[test]
fn a_page_given_a_run_for_everyone_is_no_ones() {
    let (_d, s) = one("kept-open", &format!("{}{}", KEPT, SIGNS_IN));
    for _ in 0..2 {
        let a = send(s.port, "GET", "/everyone", &[], "");
        assert_eq!((shown(&a.body), a.header("cache-control")), (vec!["1".to_string()], None));
    }
    assert_eq!((metric(&s, "ran"), metric(&s, "shared")), (1, 1));
}

// ---- Q-886: what a cache in front is told ----------------------------------------------------

const HEADERS: &str = "fn helper(r) = r.cookies.uid
fn keeps(r) = r.path == \"/\" ? reply 403 \"no\" : r
server fn hdr() = request.headers[\"x-api-key\"]
server fn pathy() = request.path
component H() {
    state v = hdr()
    <p>{v}</p>
}
route GET /d403 { reply 403 <html><body><p>{session().who}</p></body></html> }
route GET /d404 = respond(404, <html><body><p>{session().who}</p></body></html>)
route GET /d500 = respond(500, <html><body><p>{session().who}</p></body></html>)
route GET /header = <p>{request.headers[\"x-api-key\"]}</p>
route GET /ip = <p>{request.ip}</p>
route GET /get1 = <p>{get(request.headers, \"authorization\")}</p>
route GET /get2 = <p>{get(request, \"cookies\").uid}</p>
route GET /values = <p>{join(values(request.headers), \",\")}</p>
route GET /keys = <p>{len(keys(request.cookies))}</p>
route GET /pattern {
    let { cookies } = request
    <p>{cookies.uid}</p>
}
route GET /match = match request {
    { cookies: { uid } } => <p>{uid}</p>,
    _ => <p>no</p>,
}
route GET /index = <p>{request[\"cookies\"].uid}</p>
route GET /helper = <p>{helper(request)}</p>
route GET /inline { cookies: { uid: str } } = <p>{uid}</p>
route GET /server = <html><body><H/></body></html>
fn gate(r) = r.cookies.uid != null ? r : respond(403, \"no\")
fn by_header(r) = r.headers[\"x-ok\"] == \"1\" ? r : reply 403 \"no\"
fn passes_on(r) = helper(r) == \"x\" ? r : r
fn role(name: str) = (r) => r
fn cookie_role(name: str) = (r) => r.cookies.uid == name ? r : reply 403 \"no\"
fn gives_back(r) {
    if r.path == \"/\" {
        return respond(403, \"no\")
    }
    return r
}
routes /g | keeps {
    route GET /grouped = <p>x</p>
}
routes /back | gives_back {
    route GET /grouped = <p>x</p>
}
routes /inline | (r) => r {
    route GET /grouped = <p>x</p>
}
routes /private | gate {
    route GET /grouped = <p>x</p>
}
routes /header | by_header {
    route GET /grouped = <p>x</p>
}
routes /passes | passes_on {
    route GET /grouped = <p>x</p>
}
routes /factory | role(\"admin\") {
    route GET /grouped = <p>x</p>
}
routes /reads | cookie_role(\"alice\") {
    route GET /grouped = <p>x</p>
}
route GET /path = <p>{request.path}</p>
route GET /query/:id = <p>{request.query.a}{request.params.id}{request.method}{request.host}</p>
route GET /sp = <p>{pathy()}</p>
route GET /none = <p>x</p>
route GET /as/:who { set_session({ who }); \"ok\" }
";

// Whatever the status, and however the visitor was read.
#[test]
fn every_page_made_from_the_visitor_is_private() {
    let (_d, s) = one("private", HEADERS);
    let me = sign_in(&s, "alice");
    let cookie = format!("{}; uid=alice", me);
    let h = [("cookie", cookie.as_str()), ("authorization", "Bearer x"), ("x-api-key", "k")];
    for p in ["/d403", "/d404", "/d500", "/header", "/ip", "/get1", "/get2", "/values", "/keys", "/pattern", "/match", "/index", "/helper", "/inline", "/server", "/private/grouped", "/passes/grouped", "/factory/grouped", "/reads/grouped"] {
        let a = send(s.port, "GET", p, &h, "");
        assert_eq!(a.header("cache-control"), Some("private"), "{} {} {}", p, a.status, a.body);
        assert!(a.headers.iter().any(|(k, v)| k == "vary" && v == "Cookie"), "{} {:?}", p, a.headers);
    }
}

// What must stay: what the URL says is nobody's, and a page of it can be kept.
#[test]
fn a_page_of_what_the_url_says_is_no_ones() {
    let (_d, s) = one("public", HEADERS);
    for p in ["/path", "/query/1?a=2", "/sp", "/none", "/g/grouped", "/back/grouped", "/inline/grouped"] {
        let a = send(s.port, "GET", p, &[("cookie", "uid=alice")], "");
        assert_eq!((a.status, a.header("cache-control"), a.header("vary")), (200, None, None), "{}", p);
    }
}

// The middleware that runs, not the one a name finds: a file's `private fn gate` lost to the
// program's `gate`, and the page lost `private`.
#[test]
fn the_middleware_that_runs_is_the_one_read() {
    let d = app("private-gate", &[
        ("a.qwe", "private fn gate(r) = r.cookies.uid != null ? r : reply 403 \"no\"\nroutes /a | gate {\n    route GET /page = <p>a</p>\n}\nfn a_marker() = 1\n"),
        ("main.qwe", "import \"./a.qwe\" use a_marker\nfn gate(r) = r\nroutes /m | gate {\n    route GET /page = <p>m</p>\n}\n"),
    ]);
    let s = run(&d);
    let a = send(s.port, "GET", "/a/page", &[("cookie", "uid=1")], "");
    assert_eq!((a.status, a.header("cache-control")), (200, Some("private")));
    let m = send(s.port, "GET", "/m/page", &[], "");
    assert_eq!((m.status, m.header("cache-control")), (200, None));
}

// A middleware that reads the visitor refuses what no route under it answers, as itself.
#[test]
fn a_refusal_for_what_no_route_answers_is_the_visitors() {
    let (_d, s) = one("private-guard", HEADERS);
    let a = send(s.port, "GET", "/header/nowhere", &[], "");
    assert_eq!((a.status, a.header("cache-control")), (403, Some("private")), "{:?}", a.headers);
    assert!(a.headers.iter().any(|(k, v)| k == "vary" && v == "Cookie"), "{:?}", a.headers);
}

const LOCALES: &str = "translations({ en: { hi: \"Hello\" }, fr: { hi: \"Bonjour\" } })
view V() = <p>{t(\"hi\")}</p>
view Plain() = <p>x</p>
component C() {
    <p>x</p>
}
route GET /t = <p>{t(\"hi\")}</p>
route GET /locale = <p>{locale()}</p>
route GET /money = <p>{money.format(money.of(\"12.50\", \"EUR\"))}</p>
route GET /number = <p>{format_number(1234.5)}</p>
route GET /view = <V/>
route GET /island = <html><body><C/></body></html>
route GET /plain = <Plain/>
route GET /own = respond(200, t(\"hi\"), { vary: null })
route GET /parallel {
    let [a, _b] = parallel(() => t(\"hi\"), () => 1)
    <p>{a}</p>
}
";

fn varies(a: &Answer, name: &str) -> bool {
    a.headers.iter().any(|(k, v)| k == "vary" && v.split(',').any(|x| x.trim().eq_ignore_ascii_case(name)))
}

// By what chose the language: a page in French was kept for everyone.
#[test]
fn a_page_in_the_visitors_language_varies_by_it() {
    let (_d, s) = one("vary", LOCALES);
    for p in ["/t", "/locale", "/money", "/number", "/view", "/view", "/island", "/parallel"] {
        let a = send(s.port, "GET", p, &[("accept-language", "fr")], "");
        assert!(varies(&a, "accept-language") && varies(&a, "cookie"), "{} {:?}", p, a.headers);
        assert_eq!(a.header("cache-control"), None, "{}", p);
    }
}

// What must stay: a page that used no locale does not vary, nor one the program said not to.
#[test]
fn a_page_with_no_locale_does_not_vary() {
    let (_d, s) = one("no-vary", LOCALES);
    let a = send(s.port, "GET", "/plain", &[], "");
    assert!(!varies(&a, "accept-language"), "{:?}", a.headers);
    let a = send(s.port, "GET", "/own", &[], "");
    assert!(!varies(&a, "accept-language") && !varies(&a, "cookie"), "{:?}", a.headers);
    // Money with no `translations` has one language.
    let (_d, s) = one("money", "route GET / = <p>{money.format(money.of(\"12.50\", \"EUR\"))}</p>\n");
    assert!(!varies(&send(s.port, "GET", "/", &[], ""), "accept-language"));
}

// A rule of the program's can read anything.
#[test]
fn a_locale_from_a_rule_is_private() {
    let (_d, s) = one("rule", "translations({ en: { hi: \"Hello\" }, fr: { hi: \"Bonjour\" } })\nlocale_from((r) => r.headers[\"x-lang\"])\nroute GET / = <p>{t(\"hi\")}</p>\n");
    let a = send(s.port, "GET", "/", &[("x-lang", "fr")], "");
    assert_eq!((a.header("cache-control"), shown(&a.body)), (Some("private"), vec!["Bonjour".to_string()]));
}

#[test]
fn a_page_of_a_tenant_told_by_a_rule_is_private() {
    let (_d, s) = one("tenant-private", "tenant_from((r) => r.headers[\"x-tenant\"])\nroute GET / = <p>{tenant()}</p>\n");
    assert_eq!(send(s.port, "GET", "/", &[("x-tenant", "acme")], "").header("cache-control"), Some("private"));
}

// And the program's own `cache-control` is still the program's.
#[test]
fn the_programs_own_cache_control_stands() {
    let (_d, s) = one("own-cc", "route GET / = respond(200, session(), { \"cache-control\": \"no-store\" })\n");
    assert_eq!(send(s.port, "GET", "/", &[], "").header("cache-control"), Some("no-store"));
}

// ---- Q-887: at most once, whoever asks -------------------------------------------------------

/// A program that charges in every way a first try and its retry can differ, on `target`.
fn charges(target: &str) -> String {
    format!(
        r#"db.open("{}")
db.exec("create table if not exists charges (who varchar(64))")
fn charge(who) {{
    db.exec("insert into charges (who) values (?)", who)
    db.one("select count(*) as n from charges").n
}}
route GET /count = str(db.one("select count(*) as n from charges").n)
route GET /lease {{
    db.exec("update _quillo_idempotency set lease_until = 0 where done = 0")
    "ok"
}}
route GET /signin/:who {{ set_session({{ user: who }}); "ok" }}
route GET /guest {{ set_session({{ cart: 1 }}); "ok" }}
route GET /record/:who {{ set_session({{ user: {{ id: who, credits: 10 }} }}); "ok" }}
route GET /out {{ end_session(); "ok" }}
route GET /anew/:who {{
    end_session()
    set_session({{ uid: who }})
    "ok"
}}
route GET /sees = session()
// Nothing touches the session.
route POST /pay = idempotent(() => ({{ charged: charge("pay") }}))
// `.user` is a record the work changes.
route POST /spend = idempotent(() => {{
    let u = session().user
    set_session({{ user: merge(u, {{ credits: u.credits - 1 }}) }})
    {{ charged: charge("spend") }}
}})
// A counter, written before the work.
route POST /counted {{
    let s = session()
    set_session(merge(s, {{ seen: (s.seen ?? 0) + 1 }}))
    idempotent(() => ({{ charged: charge("counted") }}))
}}
// The work signs in, signs out, or drops `.user`.
route POST /signup = idempotent(() => {{
    set_session({{ user: "new-customer" }})
    {{ charged: charge("signup") }}
}})
route POST /signup-outside = idempotent({{ transaction: false }}, () => {{
    set_session({{ user: "new-customer" }})
    {{ charged: charge("signup-outside") }}
}})
route POST /leave = idempotent(() => {{
    end_session()
    {{ charged: charge("leave") }}
}})
route POST /clear = idempotent(() => {{
    set_session({{ done: true }})
    {{ charged: charge("clear") }}
}})
// `{{ user }}` from the session, the work signing in; and given by the program.
route POST /named = idempotent({{ user: session().user }}, () => {{
    set_session({{ user: "signed-in-now" }})
    {{ charged: charge("named") }}
}})
route POST /session-user = idempotent({{ user: session().user }}, () => ({{ charged: charge("su") }}))
route POST /account/:who = idempotent({{ user: who }}, () => ({{ charged: charge(who) }}))
route POST /slow-account/:who = idempotent({{ user: who, transaction: false }}, () => {{
    let n = charge(who)
    sleep(300)
    {{ charged: n }}
}})
// A first try that fails is no try.
let tries = 0
route POST /flaky = idempotent(() => {{
    tries += 1
    if tries == 1 {{
        return err("declined")
    }}
    {{ charged: charge("flaky") }}
}})
// Work that outlives its lease, or the server.
route POST /hang = idempotent({{ transaction: false }}, () => {{
    let n = charge("hang")
    sleep(int(request.headers["x-hold"] ?? "0"))
    {{ charged: n }}
}})
route POST /hang-tx = idempotent(() => {{
    let n = charge("hang-tx")
    sleep(int(request.headers["x-hold"] ?? "0"))
    {{ charged: n }}
}})
"#,
        target
    )
}

const JSON: (&str, &str) = ("sec-fetch-site", "same-origin");

fn try_it(s: &Server, path: &str, key: &str, headers: &[(&str, &str)]) -> Answer {
    let mut h = vec![("idempotency-key", key), JSON];
    h.extend_from_slice(headers);
    send(s.port, "POST", path, &h, "{}")
}

/// The cookie a browser holds after `a`, given `sent`.
fn jar(a: &Answer, sent: &str) -> String {
    let mut now = sent.to_string();
    for (k, v) in &a.headers {
        if k == "set-cookie" {
            let pair = v.split(';').next().unwrap();
            now = if pair.ends_with('=') || v.to_ascii_lowercase().contains("max-age=0") { String::new() } else { pair.to_string() };
        }
    }
    now
}

fn cookie(c: &str) -> Vec<(&'static str, &str)> {
    if c.is_empty() {
        Vec::new()
    } else {
        vec![("cookie", c)]
    }
}

fn got(s: &Server, path: &str) -> String {
    jar(&send(s.port, "GET", path, &[], ""), "")
}

fn count(s: &Server) -> i64 {
    send(s.port, "GET", "/count", &[], "").body.parse().unwrap()
}

const SOMEONE_ELSES: &str = "someone else's";

/// Once however the session went, on each database: `n` is the charges so far.
fn retries_hold(s: &Server) {
    let mut n = count(s);
    let mut once = |what: &str, s: &Server| {
        n += 1;
        assert_eq!(count(s), n, "{}", what);
    };
    let replays = |a: &Answer, first: &Answer, what: &str| assert_eq!((a.status, a.body.as_str(), a.header("idempotent-replayed")), (200, first.body.as_str(), Some("true")), "{}", what);
    let refused = |a: &Answer, what: &str| {
        assert_eq!(a.status, 409, "{}: {}", what, a.body);
        assert!(a.body.contains(SOMEONE_ELSES) && !a.body.contains("charged"), "{}: {}", what, a.body);
    };
    // The work changes `.user`: the first answer's cookie, and the one before it.
    let c = got(s, "/record/alice");
    let first = try_it(s, "/spend", "spend", &cookie(&c));
    assert_eq!(first.status, 200, "{}", first.body);
    replays(&try_it(s, "/spend", "spend", &cookie(&jar(&first, &c))), &first, "spend, new cookie");
    replays(&try_it(s, "/spend", "spend", &cookie(&c)), &first, "spend, old cookie");
    once("spend", s);
    // A counter before the work.
    let c = got(s, "/guest");
    let first = try_it(s, "/counted", "counted", &cookie(&c));
    replays(&try_it(s, "/counted", "counted", &cookie(&jar(&first, &c))), &first, "counted");
    once("counted", s);
    // The same on a first visit, with the cookie and with the answer lost.
    let first = try_it(s, "/counted", "counted-new", &[]);
    replays(&try_it(s, "/counted", "counted-new", &cookie(&jar(&first, ""))), &first, "counted, first visit");
    replays(&try_it(s, "/counted", "counted-new", &[]), &first, "counted, first visit, answer lost");
    once("counted, first visit", s);
    // The work drops `.user`.
    let c = got(s, "/signin/bob");
    let first = try_it(s, "/clear", "clear", &cookie(&c));
    replays(&try_it(s, "/clear", "clear", &cookie(&jar(&first, &c))), &first, "clear");
    once("clear", s);
    // The work signs out: the old cookie is the visitor, and no cookie is nobody who was there. No
    // stranger is told, and that is the refusal the same visitor gets without their cookie.
    let c = got(s, "/signin/carol");
    let first = try_it(s, "/leave", "leave", &cookie(&c));
    assert_eq!(jar(&first, &c), "");
    replays(&try_it(s, "/leave", "leave", &cookie(&c)), &first, "leave, old cookie");
    refused(&try_it(s, "/leave", "leave", &[]), "leave, no cookie");
    refused(&try_it(s, "/leave", "leave", &cookie(&got(s, "/signin/bob"))), "leave, someone else");
    once("leave", s);
    // Each with a session: what else they share says nothing.
    let (a, b) = (got(s, "/signin/ann"), got(s, "/signin/ben"));
    let first = try_it(s, "/pay", "public-key", &[("cookie", &a), ("x-api-key", "pk_public")]);
    once("public key", s);
    refused(&try_it(s, "/pay", "public-key", &[("cookie", &b), ("x-api-key", "pk_public")]), "one public key, two sessions");
    replays(&try_it(s, "/pay", "public-key", &[("cookie", &a), ("x-api-key", "pk_other")]), &first, "the session decides");
    // What says nothing is no credential, as the whole value or after the scheme.
    for (i, empty) in ["", " ", "undefined", "null", "NULL", "Bearer", "Bearer undefined", "Bearer null", "bearer  Undefined"].into_iter().enumerate() {
        let key = format!("empty-{}", i);
        let _ = try_it(s, "/pay", &key, &[("cookie", &a), ("authorization", empty)]);
        once("an empty credential", s);
        refused(&try_it(s, "/pay", &key, &[("authorization", empty)]), empty);
        refused(&try_it(s, "/pay", &key, &[("cookie", &b), ("authorization", empty)]), empty);
    }
    // Bob on Alice's browser is not Alice: one `user` to another is a new id.
    let c = got(s, "/signin/alice");
    let first = try_it(s, "/pay", "browser", &cookie(&c));
    once("alice's browser", s);
    let bob = jar(&send(s.port, "GET", "/signin/bob", &[("cookie", &c)], ""), &c);
    refused(&try_it(s, "/pay", "browser", &cookie(&bob)), "bob on alice's browser");
    replays(&try_it(s, "/pay", "browser", &cookie(&c)), &first, "alice's own");
    // The work signs in.
    let first = try_it(s, "/signup", "signup", &[]);
    replays(&try_it(s, "/signup", "signup", &cookie(&jar(&first, ""))), &first, "signup, cookie");
    replays(&try_it(s, "/signup", "signup", &[]), &first, "signup, answer lost");
    once("signup", s);
    // The same with the work outside a transaction.
    let first = try_it(s, "/signup-outside", "signup-outside", &[]);
    replays(&try_it(s, "/signup-outside", "signup-outside", &cookie(&jar(&first, ""))), &first, "signup outside a transaction");
    once("signup outside a transaction", s);
    // The session gains fields between the tries, and signs in: its id stays.
    let c = got(s, "/guest");
    let first = try_it(s, "/pay", "gained", &cookie(&c));
    let later = jar(&send(s.port, "GET", "/signin/gina", &[("cookie", &c)], ""), &c);
    assert_ne!(later, c);
    replays(&try_it(s, "/pay", "gained", &cookie(&later)), &first, "a session that gained fields");
    once("gained", s);
    // Signed out between the tries, or the cookie lost: refused, and not run.
    let c = got(s, "/signin/dave");
    let first = try_it(s, "/pay", "gone", &cookie(&c));
    once("first of gone", s);
    let out = jar(&send(s.port, "GET", "/out", &[("cookie", &c)], ""), &c);
    refused(&try_it(s, "/pay", "gone", &cookie(&out)), "signed out");
    refused(&try_it(s, "/pay", "gone", &[]), "cookie lost");
    replays(&try_it(s, "/pay", "gone", &cookie(&c)), &first, "still theirs");
    assert_eq!(count(s), first_count(&first), "nothing ran");
    // An anonymous visitor who got there first: no receipt for Alice, and no second charge.
    let _ = try_it(s, "/pay", "pre", &[]);
    once("anonymous first", s);
    let c = got(s, "/signin/alice");
    refused(&try_it(s, "/pay", "pre", &cookie(&c)), "pre-answered");
    // Credentials, each by itself.
    let first = try_it(s, "/pay", "extra", &[("x-api-key", "a")]);
    replays(&try_it(s, "/pay", "extra", &[("x-api-key", "a"), ("authorization", "Bearer t")]), &first, "one more header");
    once("extra", s);
    let first = try_it(s, "/pay", "both", &[("x-api-key", "a"), ("authorization", "Bearer t")]);
    replays(&try_it(s, "/pay", "both", &[("authorization", "Bearer t")]), &first, "one header fewer");
    once("both", s);
    // Two clients with one key and no `{ user }`.
    let _ = try_it(s, "/pay", "clients", &[("x-api-key", "one")]);
    once("first client", s);
    refused(&try_it(s, "/pay", "clients", &[("x-api-key", "two")]), "second client");
    // `{ user }` from null to signed in, by the work.
    let first = try_it(s, "/named", "named", &[]);
    replays(&try_it(s, "/named", "named", &cookie(&jar(&first, ""))), &first, "null to signed in");
    once("named", s);
    // And signed in to null: the first try is found, and it is nobody's who asks now.
    let c = got(s, "/signin/erin");
    let first = try_it(s, "/session-user", "back", &cookie(&c));
    assert_eq!(first.status, 200);
    once("back", s);
    refused(&try_it(s, "/session-user", "back", &[]), "signed in to null");
    replays(&try_it(s, "/session-user", "back", &cookie(&c)), &first, "still signed in");
    // Two accounts, one key: each runs, as each is told of its own.
    let a = try_it(s, "/account/one", "shared", &[]);
    once("account one", s);
    let b = try_it(s, "/account/two", "shared", &[]);
    once("account two", s);
    assert_ne!(a.body, b.body);
    replays(&try_it(s, "/account/one", "shared", &[]), &a, "account one again");
    // A failed try runs again.
    assert_eq!(try_it(s, "/flaky", "flaky", &[]).status, 500);
    assert_eq!(try_it(s, "/flaky", "flaky", &[]).status, 200);
    once("flaky", s);
    // Another body is another request.
    let _ = try_it(s, "/pay", "body", &[("authorization", "Bearer a")]);
    once("body", s);
    let mut h = vec![("idempotency-key", "body"), JSON, ("authorization", "Bearer a")];
    let other = send(s.port, "POST", "/pay", &h, "{\"x\":1}");
    assert_eq!(other.status, 422, "{}", other.body);
    h[2] = ("authorization", "Bearer b");
    assert_eq!(send(s.port, "POST", "/pay", &h, "{\"x\":1}").status, 409, "and says nothing to a stranger");
}

fn first_count(a: &Answer) -> i64 {
    serde_json::from_str::<serde_json::Value>(&a.body).unwrap()["charged"].as_i64().unwrap()
}

/// Two accounts at once; and tries with and without the cookie at once, a hundred keys.
fn at_once_holds(s: &Server) {
    let before = count(s);
    let port = s.port;
    let at = |path: &'static str| std::thread::spawn(move || send(port, "POST", path, &[("idempotency-key", "both-running"), JSON], "{}"));
    let (a, b) = (at("/slow-account/ann"), at("/slow-account/ben"));
    assert_eq!((a.join().unwrap().status, b.join().unwrap().status), (200, 200));
    assert_eq!(count(s), before + 2);
    let c = got(s, "/guest");
    for i in 0..100 {
        let key = format!("race-{}", i);
        let (k, cc) = (key.clone(), c.clone());
        let with = std::thread::spawn(move || send(port, "POST", "/pay", &[("idempotency-key", &k), JSON, ("cookie", &cc)], "{}"));
        let k = key.clone();
        let without = std::thread::spawn(move || send(port, "POST", "/pay", &[("idempotency-key", &k), JSON], "{}"));
        let (w, o) = (with.join().unwrap(), without.join().unwrap());
        assert_eq!(count(s), before + 3 + i, "{}: {} {}, {} {}", key, w.status, w.body, o.status, o.body);
        assert!([200, 409].contains(&w.status) && [200, 409].contains(&o.status), "{} {}", w.status, o.status);
        assert!(w.status == 200 || o.status == 200, "{}: {} {}", key, w.body, o.body);
        assert!(w.status != 200 || o.status != 200, "one of them is not the other: {}: {} {}", key, w.body, o.body);
    }
    // With `{ user }` from the session, a row each: with the cookie, without, and the retry.
    let start = count(s);
    let c = got(s, "/signin/race");
    for i in 0..100 {
        let key = format!("named-race-{}", i);
        let (k, cc) = (key.clone(), c.clone());
        let with = std::thread::spawn(move || send(port, "POST", "/session-user", &[("idempotency-key", &k), JSON, ("cookie", &cc)], "{}"));
        let k = key.clone();
        let without = std::thread::spawn(move || send(port, "POST", "/session-user", &[("idempotency-key", &k), JSON], "{}"));
        let _ = (with.join().unwrap(), without.join().unwrap());
        let _ = try_it(s, "/session-user", &key, &cookie(&c));
        assert_eq!(count(s), start + 1 + i, "{}", key);
    }
}

/// Running, and past it: nobody is told it is gone. `kill` stops the server mid-work and `start`
/// brings it back on the same database.
fn past_the_lease_holds(d: &Path, target: &str, alive: bool) {
    let start = || serve_with(&d.join("main.qwe"), &["--metrics"]);
    let mut s = start();
    let c = got(&s, "/guest");
    let before = count(&s);
    let port = s.port;
    // With no transaction: it is not known whether the work took effect, for anyone, ever.
    let (k, cc) = ("no-tx", c.clone());
    let hung = std::thread::spawn(move || send(port, "POST", "/hang", &[("idempotency-key", k), JSON, ("cookie", &cc), ("x-hold", "20000")], "{}"));
    let (k2, cc2) = ("tx", c.clone());
    let hung_tx = (!target.ends_with(".db")).then(|| std::thread::spawn(move || send(port, "POST", "/hang-tx", &[("idempotency-key", k2), JSON, ("cookie", &cc2), ("x-hold", "20000")], "{}")));
    std::thread::sleep(Duration::from_millis(500));
    s.kill();
    let _ = hung.join();
    if let Some(h) = hung_tx {
        let _ = h.join();
    }
    let s = start();
    let other = got(&s, "/guest");
    assert_eq!(count(&s), before + 1, "the charge outside a transaction stays, the one in it went");
    for who in [&c, &other] {
        assert_eq!(try_it(&s, "/hang", "no-tx", &cookie(who)).status, 409);
    }
    send(s.port, "GET", "/lease", &[], "");
    let a = try_it(&s, "/hang", "no-tx", &cookie(&c));
    assert_eq!(a.status, 409, "{}", a.body);
    assert!(a.body.contains("not known"), "{}", a.body);
    let a = try_it(&s, "/hang", "no-tx", &cookie(&other));
    assert_eq!(a.status, 409, "{}", a.body);
    assert!(a.body.contains(SOMEONE_ELSES), "{}", a.body);
    assert_eq!(count(&s), before + 1);
    if target.ends_with(".db") {
        return;
    }
    // In a transaction the work rolled back: past the lease a stranger takes nothing over, and the
    // same visitor's next try runs, once.
    let stranger = try_it(&s, "/hang-tx", "tx", &cookie(&other));
    assert_eq!(stranger.status, 409, "{}", stranger.body);
    assert_eq!(count(&s), before + 1);
    let a = try_it(&s, "/hang-tx", "tx", &cookie(&c));
    assert_eq!(a.status, 200, "past the lease of `/lease` above: {}", a.body);
    assert_eq!(count(&s), before + 2);
    assert_eq!(try_it(&s, "/hang-tx", "tx", &cookie(&c)).body, a.body);
    if !alive {
        return;
    }
    // A first try that is in fact alive: taken over, it cannot commit, and one does.
    let port = s.port;
    let cc = c.clone();
    let slow = std::thread::spawn(move || send(port, "POST", "/hang-tx", &[("idempotency-key", "alive"), JSON, ("cookie", &cc), ("x-hold", "1200")], "{}"));
    std::thread::sleep(Duration::from_millis(300));
    send(s.port, "GET", "/lease", &[], "");
    let second = try_it(&s, "/hang-tx", "alive", &cookie(&c));
    let first = slow.join().unwrap();
    assert_eq!(second.status, 200, "{}", second.body);
    // The first replays, is refused, or the database rolled it back as the two met in a table.
    assert!(first.status == 200 && first.body == second.body || [409, 500].contains(&first.status), "{} {}", first.status, first.body);
    assert_eq!(count(&s), before + 3);
}

// The same on SQLite, where a transaction asleep holds up every other request: one at a time.
#[test]
fn a_stranger_takes_nothing_over_on_sqlite() {
    let d = app("idem-stale", &[("main.qwe", &charges("data.db"))]);
    let start = || serve_with(&d.join("main.qwe"), &["--metrics"]);
    let mut s = start();
    let c = got(&s, "/guest");
    let (port, cc) = (s.port, c.clone());
    let hung = std::thread::spawn(move || send(port, "POST", "/hang-tx", &[("idempotency-key", "stale"), JSON, ("cookie", &cc), ("x-hold", "20000")], "{}"));
    std::thread::sleep(Duration::from_millis(500));
    s.kill();
    let _ = hung.join();
    let s = start();
    assert_eq!(count(&s), 0, "it rolled back");
    let other = got(&s, "/guest");
    send(s.port, "GET", "/lease", &[], "");
    let a = try_it(&s, "/hang-tx", "stale", &cookie(&other));
    assert_eq!(a.status, 409, "{}", a.body);
    assert!(a.body.contains(SOMEONE_ELSES), "{}", a.body);
    assert_eq!(count(&s), 0);
    assert_eq!(try_it(&s, "/hang-tx", "stale", &cookie(&c)).status, 200);
    assert_eq!(count(&s), 1);
}

// `{ user }` is a name: a record the work changes would be a new namespace, and a second run.
#[test]
fn a_user_that_is_no_name_is_refused() {
    let src = format!(
        "{}route POST /map = idempotent({{ user: session().user }}, () => ({{ charged: charge(\"map\") }}))\nroute POST /list {{\n    let u = [request.query.a]\n    idempotent({{ user: u }}, () => ({{ charged: charge(\"list\") }}))\n}}\nroute POST /int = idempotent({{ user: 7 }}, () => ({{ charged: charge(\"int\") }}))\n",
        charges("data.db")
    );
    let (d, s) = one("idem-name", &src);
    let c = got(&s, "/record/alice");
    assert_eq!(try_it(&s, "/map", "k", &cookie(&c)).status, 500);
    assert_eq!(try_it(&s, "/list", "k", &[]).status, 500);
    assert_eq!(count(&s), 0);
    assert_eq!(try_it(&s, "/int", "k", &[]).status, 200);
    let _ = d;
    let says = |src: &str| {
        let c = check_source(src, "t.qwe", Path::new("."));
        c.diagnostics.iter().filter(|x| x.severity == Severity::Error).map(|x| x.message.clone()).collect::<Vec<_>>()
    };
    let named = |v: &str| format!("route POST / = idempotent({{ user: {} }}, () => 1)\n", v);
    assert!(says(&named("{ id: 1 }"))[0].contains("its `.id`"));
    assert!(says(&named("[1]"))[0].contains("its `.id`"));
    assert!(says(&named("\"shop\"")).is_empty() && says(&named("7")).is_empty() && says(&named("request.query.a")).is_empty());
    assert!(says("route POST / = idempotent({ isolation: \"serializable\" }, () => 1)\n").is_empty());
}

fn on_database(name: &str, target: &str, alive: bool) {
    let d = app(name, &[("main.qwe", &charges(target))]);
    {
        let s = run(&d);
        retries_hold(&s);
        at_once_holds(&s);
    }
    past_the_lease_holds(&d, target, alive);
}

#[test]
fn a_key_runs_once_on_sqlite() {
    on_database("idem-sqlite", "data.db", false);
}

#[test]
fn a_key_runs_once_on_postgres() {
    let Some(pg) = support::postgres("viewers-idem") else { return };
    on_database("idem-pg", &pg.url, true);
}

#[test]
fn a_key_runs_once_on_mariadb() {
    let Some(my) = support::mariadb("viewers-idem") else { return };
    on_database("idem-my", &my.url("viewers_idem"), true);
}

// The rows of a try go to the primary: on a replica the first try's claim is not there yet.
#[test]
fn a_key_is_read_on_the_primary() {
    let Some(pg) = support::postgres("viewers-replica") else { return };
    let (prim, rep) = (pg.database("prim"), pg.database("rep"));
    let d = app("idem-replica", &[("main.qwe", &format!(
        "db.open(\"{}\", {{ replicas: [\"{}\"] }})\ndb.exec(\"create table if not exists charges (who text)\")\nroute POST /buy = idempotent(() => {{\n    set_session({{ user: \"new-customer\" }})\n    db.exec(\"insert into charges (who) values ('x')\")\n    {{ ok: true }}\n}})\nroute GET /count {{\n    db.exec(\"create table if not exists z (a int)\")\n    str(db.one(\"select count(*) as n from charges\").n)\n}}\n",
        prim, rep
    ))]);
    let s = run(&d);
    let first = try_it(&s, "/buy", "r", &[]);
    let again = try_it(&s, "/buy", "r", &cookie(&jar(&first, "")));
    assert_eq!((again.status, again.header("idempotent-replayed")), (200, Some("true")), "{}", again.body);
    assert_eq!(send(s.port, "GET", "/count", &[], "").body, "1");
}

// A table of an older Quillo gains its columns, and a first try kept there is found.
#[test]
fn a_key_kept_by_an_older_quillo_is_found() {
    let hex = |o: &str, key: &str| quillo::idempotency::sha256_hex(&[b"", b"", o.as_bytes(), key.as_bytes(), b""]);
    let print = quillo::idempotency::sha256_hex(&[b"POST", b"/pay", b"", b"{}"]);
    let (anon, user) = (hex("null", "old-anon"), hex("\"alice\"", "old-user"));
    let src = format!(
        "{}route GET /old {{\n    db.exec(\"create table _quillo_idempotency (id varchar(64) primary key, path text not null, fingerprint varchar(64) not null, token varchar(32) not null, done int not null default 0, lease_until bigint not null, expires_at bigint not null, status int, headers text, body text, body_b64 int)\")\n    for id in [\"{}\", \"{}\"] {{\n        db.exec(\"insert into _quillo_idempotency (id, path, fingerprint, token, done, lease_until, expires_at, status, headers, body, body_b64) values (?, '/pay', '{}', 't', 1, 0, 99999999999999, 200, '[]', 'kept', 0)\", id)\n    }}\n    \"ok\"\n}}\n",
        charges("data.db"), anon, user, print
    );
    let (_d, s) = one("idem-older", &src);
    assert_eq!(send(s.port, "GET", "/old", &[], "").body, "ok");
    assert_eq!(try_it(&s, "/pay", "old-anon", &[]).body, "kept");
    let c = got(&s, "/signin/alice");
    assert_eq!(try_it(&s, "/pay", "old-user", &cookie(&c)).body, "kept");
    assert_eq!(count(&s), 0);
    // And a new key works on the table as it now is.
    assert_eq!(try_it(&s, "/pay", "new", &[]).status, 200);
}

// The hidden id: `session()` does not show it, a later `set_session` keeps it, `end_session` ends it.
#[test]
fn a_sessions_id_is_hidden_kept_and_ended() {
    let (_d, s) = one("idem-id", &charges("data.db"));
    let c = got(&s, "/guest");
    let shown = send(s.port, "GET", "/sees", &[("cookie", &c)], "").body;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&shown).unwrap(), serde_json::json!({ "cart": 1 }));
    let first = try_it(&s, "/pay", "k", &cookie(&c));
    assert_eq!(try_it(&s, "/pay", "k", &cookie(&jar(&send(s.port, "GET", "/sees", &[("cookie", &c)], ""), &c))).body, first.body);
    let anew = send(s.port, "GET", "/anew/alice", &[("cookie", &c)], "").headers.iter().filter(|(k, _)| k == "set-cookie").map(|(_, v)| v.split(';').next().unwrap().to_string()).find(|p| !p.ends_with('=')).unwrap();
    assert_eq!(try_it(&s, "/pay", "k", &cookie(&anew)).status, 409);
}

// What a program could keep in a session it still can: the id gives way, with no 500.
#[test]
fn the_hidden_id_takes_no_room_a_session_needs() {
    let src = format!("{}route GET /fill/:n {{\n    set_session({{ user: \"alice\", pad: repeat(\"x\", int(n)) }})\n    \"ok\"\n}}\nroute GET /bare/:n {{\n    set_session({{ pad: repeat(\"x\", int(n)) }})\n    \"ok\"\n}}\nroute POST /rewrite = idempotent(() => {{\n    set_session(session())\n    {{ charged: charge(\"rewrite\") }}\n}})\n", charges("data.db"));
    let (_d, s) = one("idem-room", &src);
    let fill = |n: usize| send(s.port, "GET", &format!("/fill/{}", n), &[], "");
    let room = (2000..3200).rev().find(|n| fill(*n).status == 200).unwrap();
    assert!(fill(room + 1).status == 500);
    let c = jar(&fill(room), "");
    // The most a cookie held before there were ids: its text, signed, up to the limit.
    assert!(c.len() > 3800, "{}", c.len());
    let payload = c.split_once('=').unwrap().1.split('.').next().unwrap();
    assert!(!String::from_utf8(quillo::crypto::unb64url(payload).unwrap()).unwrap().contains("\"i\":"));
    // Who it is then goes by what else says so.
    let first = try_it(&s, "/pay", "full", &cookie(&c));
    assert_eq!(try_it(&s, "/pay", "full", &cookie(&c)).body, first.body);
    assert_eq!(try_it(&s, "/pay", "full", &[]).status, 409);
    // With no `user` either, the session itself: a stranger is told nothing, as `nobody` would be.
    let bare = |n: usize| send(s.port, "GET", &format!("/bare/{}", n), &[], "");
    let room = (2000..3200).rev().find(|n| bare(*n).status == 200).unwrap();
    let c = jar(&bare(room), "");
    let first = try_it(&s, "/rewrite", "no-name", &cookie(&c));
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(try_it(&s, "/rewrite", "no-name", &cookie(&c)).body, first.body);
    let a = try_it(&s, "/rewrite", "no-name", &[]);
    assert_eq!(a.status, 409, "{}", a.body);
}

/// A session cookie of before there were ids, as `support::serve` signs one.
fn old_cookie(name: &str, data: &str) -> String {
    let key = quillo::crypto::hmac_sha256(b"test secret for the tests", b"quillo session");
    let payload = quillo::crypto::b64url(format!("{{\"e\":99999999999,\"d\":{}}}", data).as_bytes());
    let mut msg = b"\0\0".to_vec();
    msg.extend_from_slice(payload.as_bytes());
    format!("{}={}.{}", name, payload, quillo::crypto::b64url(&quillo::crypto::hmac_sha256(&key, &msg)))
}

// A cookie from before ids: `.user` when it is a name, and a record is no name.
#[test]
fn a_session_from_before_ids_goes_by_a_name_that_cannot_change() {
    let (_d, s) = one("idem-oldcookie", &charges("data.db"));
    let name = got(&s, "/guest").split('=').next().unwrap().to_string();
    let (alice, bob) = (old_cookie(&name, "{\"user\":\"alice\"}"), old_cookie(&name, "{\"user\":\"bob\"}"));
    assert_eq!(send(s.port, "GET", "/sees", &[("cookie", &alice)], "").body, "{\"user\":\"alice\"}");
    let first = try_it(&s, "/pay", "name", &cookie(&alice));
    assert_eq!(try_it(&s, "/pay", "name", &cookie(&alice)).body, first.body);
    assert_eq!(try_it(&s, "/pay", "name", &cookie(&bob)).status, 409);
    // The work moves it to a session with an id: both cookies are the visitor's.
    let first = try_it(&s, "/signup", "moved", &cookie(&alice));
    assert_eq!(try_it(&s, "/signup", "moved", &cookie(&jar(&first, &alice))).body, first.body);
    assert_eq!(try_it(&s, "/signup", "moved", &cookie(&alice)).body, first.body);
    // A record changes: it is not who anyone is.
    let record = old_cookie(&name, "{\"user\":{\"id\":\"alice\",\"credits\":10}}");
    let first = try_it(&s, "/spend", "record", &cookie(&record));
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(try_it(&s, "/spend", "record", &cookie(&jar(&first, &record))).body, first.body);
    assert_eq!(try_it(&s, "/spend", "record", &cookie(&record)).body, first.body);
    assert_eq!(count(&s), 3);
    // Told by the session itself: another value is a false refusal, and nobody else's receipt.
    let (ten, nine) = (old_cookie(&name, "{\"user\":{\"id\":\"a\",\"credits\":10}}"), old_cookie(&name, "{\"user\":{\"id\":\"a\",\"credits\":9}}"));
    let first = try_it(&s, "/pay", "credits", &cookie(&ten));
    assert_eq!(try_it(&s, "/pay", "credits", &cookie(&ten)).body, first.body);
    assert_eq!(try_it(&s, "/pay", "credits", &cookie(&nine)).status, 409);
    assert_eq!(try_it(&s, "/pay", "credits", &[]).status, 409);
    assert_eq!(count(&s), 4);
}

// ---- Q-888: a run that acted is not shared ---------------------------------------------------

const ACTS: &str = "db.open(\"data.db\")
server fn tok() = crypto.token(16)
server fn id() = uuid()
server fn dice() = random()
server fn dice2() = random_int(1, 1000000000)
server fn wrote() {
    write_file(\"out.txt\", \"x\")
    return \"wrote\"
}
server fn gone() {
    delete_file(\"out.txt\")
    return \"gone\"
}
server fn hidden() = unwatched(() => crypto.token(16))
server fn hidden_parallel() = unwatched(() => parallel(() => crypto.token(16), () => uuid()))
server fn by_helper() = helper_token()
fn helper_token() = crypto.token(16)
server fn around() = { inner: tok() }
server fn logged() {
    print(\"a line\")
    return \"logged\"
}
server fn clock() = now() > 0
server fn table() = len(db.query(\"select 1 as x\"))
";

fn acts(name: &str) -> (PathBuf, Server) {
    one(name, &format!("{}{}", ACTS, pages_of(&["tok", "id", "dice", "dice2", "hidden", "by_helper", "wrote", "gone", "logged", "clock", "table"])))
}

// Two viewers were given one token.
#[test]
fn a_run_that_made_something_is_no_one_elses() {
    let (_d, s) = acts("made");
    for f in ["tok", "id", "dice", "dice2", "hidden", "hidden_parallel", "by_helper", "around"] {
        let (a, b) = (refresh(&s, f, &[("cookie", "a=1")]), refresh(&s, f, &[("cookie", "a=2")]));
        assert_ne!(value(&a), value(&b), "{}", f);
    }
    assert_ne!(page(&s, "/tok", &[]), page(&s, "/tok", &[]));
}

// A write to a file was skipped: the second viewer's run was the first's.
#[test]
fn a_run_that_wrote_a_file_is_no_one_elses() {
    let (d, s) = acts("wrote");
    assert_eq!(value(&refresh(&s, "wrote", &[("cookie", "a=1")])), "wrote");
    std::fs::remove_file(d.join("out.txt")).unwrap();
    assert_eq!(value(&refresh(&s, "wrote", &[("cookie", "a=2")])), "wrote");
    assert!(d.join("out.txt").exists());
    assert_eq!(value(&refresh(&s, "gone", &[("cookie", "a=1")])), "gone");
    std::fs::write(d.join("out.txt"), "x").unwrap();
    assert_eq!(value(&refresh(&s, "gone", &[("cookie", "a=2")])), "gone");
    assert!(!d.join("out.txt").exists());
}

// Nothing waits for such a run, as nothing it made can be taken.
#[test]
fn nothing_waits_for_a_run_that_acted() {
    let (_d, s) = one("no-wait", &format!("server fn slow_token() {{\n    sleep(300)\n    return crypto.token(16)\n}}\n{}", pages_of(&["slow_token"])));
    refresh(&s, "slow_token", &[]);
    let port = s.port;
    let t = std::time::Instant::now();
    let calls: Vec<_> = (0..4).map(|_| std::thread::spawn(move || send(port, "POST", "/_quillo/rpc/slow_token", &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin")], "[]"))).collect();
    let mut seen: Vec<String> = calls.into_iter().map(|c| value(&c.join().unwrap()).to_string()).collect();
    assert!(t.elapsed() < Duration::from_millis(900), "{:?}", t.elapsed());
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 4);
}

// A page's run the same.
#[test]
fn no_page_waits_for_a_run_that_acted() {
    let (_d, s) = one("no-wait-page", &format!("server fn slow_token() {{\n    sleep(300)\n    return crypto.token(16)\n}}\n{}", pages_of(&["slow_token"])));
    send(s.port, "GET", "/slow_token", &[], "");
    let port = s.port;
    let t = std::time::Instant::now();
    let calls: Vec<_> = (0..4).map(|_| std::thread::spawn(move || send(port, "GET", "/slow_token", &[], ""))).collect();
    calls.into_iter().for_each(|c| assert_eq!(c.join().unwrap().status, 200));
    assert!(t.elapsed() < Duration::from_millis(900), "{:?}", t.elapsed());
}

/// A loader of `sql`, through `db.one`, on `target`: it is run for each, as it is no read.
fn not_a_read(name: &str, target: &str, setup: &[&str], sql: &str) {
    let setup: String = setup.iter().map(|q| format!("db.exec(\"{}\")\n", q)).collect();
    let src = format!("db.open(\"{}\")\n{}server fn ticket() = str(db.one(\"{}\").n)\n{}", target, setup, sql, pages_of(&["ticket"]));
    let (_d, s) = one(name, &src);
    assert_ne!(page(&s, "/ticket", &[]), page(&s, "/ticket", &[]));
    let (a, b) = (refresh(&s, "ticket", &[]), refresh(&s, "ticket", &[]));
    assert_ne!(value(&a), value(&b));
    assert_eq!(metric(&s, "shared"), 0);
}

// `select nextval(...)` looks like a read, and two pages were given one ticket.
#[test]
fn a_select_that_writes_is_not_a_read_on_postgres() {
    let Some(pg) = support::postgres("viewers-share") else { return };
    not_a_read("share-seq", &pg.url, &["create sequence if not exists tickets"], "select nextval('tickets') as n");
    let writes = [
        "create table if not exists ledger (id serial primary key)",
        "create or replace function charge_it() returns int language sql as 'insert into ledger default values returning id'",
    ];
    not_a_read("share-fn", &pg.url, &writes, "select charge_it() as n");
}

#[test]
fn a_select_that_writes_is_not_a_read_on_mariadb() {
    let Some(my) = support::mariadb("viewers-share") else { return };
    let url = my.url("viewers_share");
    let writes = [
        "create table if not exists ledger (id int auto_increment primary key)",
        "create or replace function charge_it() returns int begin insert into ledger () values (); return last_insert_id(); end",
    ];
    not_a_read("share-my", &url, &writes, "select charge_it() as n");
}

// What must stay: reading is not acting, and a line in the log changes nothing a page sees.
#[test]
fn a_run_that_only_read_is_shared() {
    let (_d, s) = acts("reads");
    for f in ["logged", "clock", "table"] {
        refresh(&s, f, &[("cookie", "a=1")]);
        refresh(&s, f, &[("cookie", "a=2")]);
    }
    assert_eq!((metric(&s, "ran"), metric(&s, "shared")), (3, 3));
}
