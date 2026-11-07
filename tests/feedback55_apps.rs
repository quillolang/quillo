//! What aligning the sample apps with 0.8.0 found (FEEDBACK55.md, Q-876 and Q-877; branch
//! feedback55): one viewer's `live` answer reaching another, and a `redirect` nothing takes.

mod support;

use quillo::check::{check_source, Severity};
use std::path::{Path, PathBuf};
use support::{send, serve_with, temp_dir, Server};

/// A folder holding `files`, for one test.
fn app(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = temp_dir(&format!("f55-apps-{}", name));
    for (path, text) in files {
        let p = d.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

fn run(d: &Path) -> Server {
    serve_with(&d.join("main.qwe"), &["--metrics"])
}

/// The session cookie of visitor `who`, from the app's `/as/:who`.
fn sign_in(s: &Server, who: u32) -> String {
    let a = send(s.port, "GET", &format!("/as/{}", who), &[], "");
    a.header("set-cookie").unwrap().split(';').next().unwrap().to_string()
}

/// What each `<p>` of a page says.
fn shown(page: &str) -> Vec<String> {
    page.split("<p").skip(1).map(|p| p.split_once('>').unwrap().1.split("</p>").next().unwrap().to_string()).collect()
}

fn page(s: &Server, path: &str, headers: &[(&str, &str)]) -> Vec<String> {
    shown(&send(s.port, "GET", path, headers, "").body)
}

fn metric(s: &Server, outcome: &str) -> i64 {
    let name = format!("quillo_live_runs_total{{outcome=\"{}\"}}", outcome);
    let text = send(s.port, "GET", "/_quillo/metrics", &[], "").body;
    text.lines().find_map(|l| l.strip_prefix(name.as_str()).and_then(|r| r.trim().parse().ok())).unwrap_or(0)
}

const TWO_LOADERS: &str = r#"server fn load_a() = { who: session().who }
server fn load_b() = { who: session().who }
component A() {
    state a = live(load_a)
    <p>{a.who}</p>
}
component B() {
    state b = live(load_b)
    <p>{b.who}</p>
}
route GET /as/:who(int) { set_session({ who }); "ok" }
route GET / = <html><body><A/><B/></body></html>
"#;

const AFTER_THE_ROUTE: &str = r#"server fn load_me() = { who: session().who }
component Me() {
    state me = live(load_me)
    <p>{me.who}</p>
}
route GET /as/:who(int) { set_session({ who }); "ok" }
route GET / {
    let s = session()
    <html><body><p>{s.who}</p><Me/></body></html>
}
"#;

// Q-876: `session()` gave its kept copy without saying the request was read, so the second
// loader of a page, or one after a route that read the session, was a run of nobody's, and the
// next viewer within two seconds was given it.
#[test]
fn the_second_loader_of_a_page_is_the_viewers() {
    let d = app("two", &[("main.qwe", TWO_LOADERS)]);
    let s = run(&d);
    let (one, two, three) = (sign_in(&s, 1), sign_in(&s, 2), sign_in(&s, 3));
    assert_eq!(page(&s, "/", &[("cookie", &one)]), ["1", "1"]);
    assert_eq!(page(&s, "/", &[("cookie", &two)]), ["2", "2"]);
    assert_eq!(page(&s, "/", &[("cookie", &three)]), ["3", "3"]);
    assert_eq!(page(&s, "/", &[("cookie", &one)]), ["1", "1"]);
}

#[test]
fn a_loader_after_a_route_that_read_the_session_is_the_viewers() {
    let d = app("route", &[("main.qwe", AFTER_THE_ROUTE)]);
    let s = run(&d);
    let (one, two) = (sign_in(&s, 1), sign_in(&s, 2));
    assert_eq!(page(&s, "/", &[("cookie", &one)]), ["1", "1"]);
    assert_eq!(page(&s, "/", &[("cookie", &two)]), ["2", "2"]);
}

// Nobody signed in after someone was, and the other way: neither is given the other's.
#[test]
fn a_visitor_with_no_session_is_given_nobodys() {
    let d = app("nobody", &[("main.qwe", TWO_LOADERS)]);
    let s = run(&d);
    let one = sign_in(&s, 1);
    assert_eq!(page(&s, "/", &[("cookie", &one)]), ["1", "1"]);
    assert_eq!(page(&s, "/", &[]), ["", ""]);
    assert_eq!(page(&s, "/", &[("cookie", &one)]), ["1", "1"]);
}

// The refresh a page makes, one after the other, for each.
#[test]
fn a_refresh_is_the_viewers() {
    let d = app("refresh", &[("main.qwe", TWO_LOADERS)]);
    let s = run(&d);
    let (one, two) = (sign_in(&s, 1), sign_in(&s, 2));
    for (cookie, who) in [(&one, "1"), (&two, "2"), (&one, "1")] {
        let a = send(s.port, "POST", "/_quillo/rpc/load_b", &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin"), ("cookie", cookie)], "[]");
        let v: serde_json::Value = serde_json::from_str(&a.body).unwrap();
        assert_eq!(v["ok"]["who"].to_string(), who, "{}", a.body);
    }
}

// The locale is kept the same way, and `t` in a loader was the first viewer's language.
#[test]
fn a_loaders_language_is_the_viewers() {
    let src = r#"translations("locales")
server fn load_word() = { word: t("hello"), locale: locale() }
component Word() {
    state v = live(load_word)
    <div><p>{v.word}</p><p>{v.locale}</p></div>
}
route GET / {
    let w = t("hello")
    <html><body><p>{w}</p><Word/></body></html>
}
"#;
    let d = app("locale", &[("main.qwe", src), ("locales/en.json", r#"{ "hello": "hello" }"#), ("locales/es.json", r#"{ "hello": "hola" }"#)]);
    let s = run(&d);
    assert_eq!(page(&s, "/", &[("accept-language", "es")]), ["hola", "hola", "es"]);
    assert_eq!(page(&s, "/", &[("accept-language", "en")]), ["hello", "hello", "en"]);
    assert_eq!(page(&s, "/", &[("accept-language", "es")]), ["hola", "hola", "es"]);
}

// What must stay: a loader reading nothing of the viewer's is one run for everyone, even on a
// page whose route read the session.
#[test]
fn a_loader_that_reads_nobody_stays_shared() {
    let src = r#"let clock = now()
server fn load_all() = { n: 12 }
component All() {
    state v = live(load_all)
    <p>{v.n}</p>
}
route GET /as/:who(int) { set_session({ who }); "ok" }
route GET / {
    let s = session()
    <html><body><p>{s.who}</p><All/></body></html>
}
"#;
    let d = app("shared", &[("main.qwe", src)]);
    let s = run(&d);
    let (one, two, three) = (sign_in(&s, 1), sign_in(&s, 2), sign_in(&s, 3));
    assert_eq!(page(&s, "/", &[("cookie", &one)]), ["1", "12"]);
    assert_eq!(page(&s, "/", &[("cookie", &two)]), ["2", "12"]);
    assert_eq!(page(&s, "/", &[("cookie", &three)]), ["3", "12"]);
    assert_eq!((metric(&s, "ran"), metric(&s, "shared")), (1, 2));
}

// And a run that read the visitor is no one else's, not even their other tab's: telling viewers
// apart took a list of what identifies nobody, and that let one see another's (FEEDBACK55.md,
// Q-881; branch feedback55).
#[test]
fn a_viewers_tabs_run_for_themselves() {
    let d = app("tabs", &[("main.qwe", AFTER_THE_ROUTE)]);
    let s = run(&d);
    let one = sign_in(&s, 1);
    assert_eq!(page(&s, "/", &[("cookie", &one)]), ["1", "1"]);
    assert_eq!(page(&s, "/", &[("cookie", &one)]), ["1", "1"]);
    assert_eq!((metric(&s, "ran"), metric(&s, "shared")), (2, 0));
}

/// What `quillo check` warns of `src`, "line: message".
fn warned(src: &str) -> Vec<String> {
    let c = check_source(src, "t.qwe", Path::new("."));
    assert!(!c.diagnostics.iter().any(|d| d.severity == Severity::Error), "{:?}", c.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
    c.diagnostics.iter().map(|d| format!("{}: {}", d.line, d.message)).collect()
}

// Q-877: a `redirect` under an `if`, with more after it, was made and left, and the page was
// shown past the check meant to stop it.
#[test]
fn a_redirect_nothing_takes_is_warned_of() {
    let says = |line: u32, name: &str, leave: &str| format!("{line}: this `{name}(...)` is an answer nothing takes, and the code goes on past it: to leave here, write `{leave}`");
    let redirect = "reply 303 redirect(...)";
    for (src, line) in [
        ("route GET / {\n    if request.query.x == null { redirect(\"/login\") }\n    \"page\"\n}\n", 2),
        ("route GET / {\n    redirect(\"/login\")\n    \"page\"\n}\n", 2),
        ("route GET / {\n    if request.query.x == null {\n        \"a\"\n    } else {\n        redirect(\"/login\")\n    }\n    \"page\"\n}\n", 5),
        ("route GET / {\n    match request.query.x {\n        \"a\" => redirect(\"/login\"),\n        _ => null,\n    }\n    \"page\"\n}\n", 3),
        ("fn guard(r) {\n    if r.query.x == null { redirect(\"/login\") }\n    1\n}\nroute GET / = guard(request)\n", 2),
    ] {
        assert_eq!(warned(src), [says(line, "redirect", redirect)], "{}", src);
    }
    assert_eq!(warned("route GET / {\n    if request.query.x == null { respond(404, \"no\") }\n    \"page\"\n}\n"), [says(2, "respond", "return respond(...)")]);
}

#[test]
fn a_redirect_that_is_the_answer_is_not_warned_of() {
    for src in [
        "route GET / {\n    redirect(\"/login\")\n}\n",
        "route GET / {\n    if request.query.x == null { redirect(\"/login\") } else { \"page\" }\n}\n",
        "route GET / {\n    if request.query.x == null { reply 303 redirect(\"/login\") }\n    \"page\"\n}\n",
        "route GET / {\n    if request.query.x == null { return redirect(\"/login\") }\n    \"page\"\n}\n",
        "route GET / {\n    let r = redirect(\"/login\")\n    r\n}\n",
        "fn redirect(to) = to\nroute GET / {\n    redirect(\"/login\")\n    \"page\"\n}\n",
        "route GET / {\n    let redirect = (to) => to\n    redirect(\"/login\")\n    \"page\"\n}\n",
    ] {
        let w = warned(src);
        assert!(!w.iter().any(|w| w.contains("nothing takes")), "{}: {:?}", src, w);
    }
}

// What the warning says to write does leave.
#[test]
fn what_the_warning_gives_leaves_the_route() {
    let src = r#"route GET /reply {
    if request.query.x == null { reply 303 redirect("/login") }
    "went on"
}
route GET /return {
    if request.query.x == null { return redirect("/login") }
    "went on"
}
route GET /respond {
    if request.query.x == null { return respond(404, "no") }
    "went on"
}
"#;
    let d = app("leaves", &[("main.qwe", src)]);
    let s = run(&d);
    for path in ["/reply", "/return"] {
        let a = send(s.port, "GET", path, &[], "");
        assert_eq!((a.status, a.header("location")), (303, Some("/login")), "{} {}", path, a.body);
        assert_eq!(send(s.port, "GET", &format!("{}?x=1", path), &[], "").body, "went on");
    }
    let a = send(s.port, "GET", "/respond", &[], "");
    assert_eq!((a.status, a.body.as_str()), (404, "no"));
}
