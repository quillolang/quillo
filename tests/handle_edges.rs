//! `Interp::handle` at its edges: Quillo's own addresses, cookies and their
//! options, server fns and live calls, and pages put together for the browser.

use quillo::server::{launch_gate, launch_token, HttpRequest, HttpResponse};
use quillo::{jobs, session, Interp};
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;

fn app(src: &str) -> Interp {
    // Errors say why, as under `--watch`.
    quillo::server::SHOW_ERRORS.store(true, Relaxed);
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "edges.qwe") {
        panic!("{}", e);
    }
    it
}

/// A program in files of its own.
fn files(name: &str, all: &[(&str, &str)]) -> (Interp, PathBuf) {
    quillo::server::SHOW_ERRORS.store(true, Relaxed);
    let d = std::env::temp_dir().join(format!("quillo-handle-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for (f, src) in all {
        std::fs::write(d.join(f), src).unwrap();
    }
    let mut it = Interp::new(d.to_str().unwrap());
    it.load_file(&d.join("app.qwe")).unwrap_or_else(|e| panic!("{}", e));
    (it, d)
}

fn request(method: &str, url: &str, headers: &[(&str, &str)], body: &[u8]) -> HttpRequest {
    let mut h = vec![("Host".to_string(), "localhost:8080".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    HttpRequest { method: method.into(), url: url.into(), headers: h, body: body.to_vec(), remote: "127.0.0.1".into() }
}

fn send(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    it.handle(&request(method, url, headers, body.as_bytes()))
}

fn get(it: &mut Interp, url: &str) -> HttpResponse {
    send(it, "GET", url, &[], "")
}

const JSON: (&str, &str) = ("Content-Type", "application/json");

fn call(it: &mut Interp, name: &str, args: &str) -> HttpResponse {
    send(it, "POST", &format!("/_quillo/rpc/{}", name), &[JSON], args)
}

fn live(it: &mut Interp, name: &str, args: &str, more: &[(&str, &str)]) -> HttpResponse {
    let mut h = vec![JSON, ("x-quillo-live", "1")];
    h.extend_from_slice(more);
    send(it, "POST", &format!("/_quillo/rpc/{}", name), &h, args)
}

fn says(r: HttpResponse, status: u16, text: &str) {
    assert!(r.status == status && r.text().contains(text), "{}: {}", r.status, r.text());
}

#[test]
fn a_percent_that_starts_no_escape_stays() {
    let mut it = app("route GET /q ? { a: str } = a\nroute GET /p/:x = x\n");
    assert_eq!(get(&mut it, "/q?a=100%25").text(), "100%");
    assert_eq!(get(&mut it, "/q?a=%zz").text(), "%zz");
    assert_eq!(get(&mut it, "/q?a=%a").text(), "%a");
    assert_eq!(get(&mut it, "/p/%4").text(), "%4");
}

#[test]
fn a_pattern_that_fails_answers_500() {
    let mut it = app("fn boom() = 1 / 0\nroute GET /x ? { a: int = boom() } = a\n");
    says(get(&mut it, "/x"), 500, "internal error");
    assert_eq!(get(&mut it, "/x?a=2").text(), "2");
}

#[test]
fn a_cookies_options_are_checked() {
    let mut it = app(
        r#"
fn with(o) { set_cookie("a", "b", o); "set" }
route GET /domain = with({ domain: "example.com" })
route GET /bad-domain = with({ domain: "a b" })
route GET /empty-domain = with({ domain: "" })
route GET /number-domain = with({ domain: 3 })
route GET /path = with({ path: "/x" })
route GET /bad-path = with({ path: "x" })
route GET /path-break = with({ path: "/x\nset-cookie: y" })
route GET /age = with({ max_age: "1h" })
route GET /bad-age = with({ max_age: "soon" })
route GET /script = with({ http_only: false })
route GET /strict = with({ same_site: "strict" })
route GET /none = with({ same_site: "None", secure: false })
route GET /lax = with({ same_site: "LAX" })
route GET /bad-same-site = with({ same_site: "sometimes" })
route GET /number-same-site = with({ same_site: 1 })
route GET /plain = with({ secure: false })
route GET /not-a-map { set_cookie("a", "b", 3); "set" }
route GET /no-options { set_cookie("a", "b", null); "set" }
"#,
    );
    let cookie = |it: &mut Interp, url: &str| get(it, url).header("set-cookie").map(str::to_string).unwrap_or_default();
    assert_eq!(cookie(&mut it, "/domain"), "a=b; Path=/; Domain=example.com; HttpOnly; Secure; SameSite=Lax");
    assert_eq!(cookie(&mut it, "/path"), "a=b; Path=/x; HttpOnly; Secure; SameSite=Lax");
    assert_eq!(cookie(&mut it, "/age"), "a=b; Path=/; Max-Age=3600; HttpOnly; Secure; SameSite=Lax");
    assert_eq!(cookie(&mut it, "/script"), "a=b; Path=/; Secure; SameSite=Lax");
    assert_eq!(cookie(&mut it, "/strict"), "a=b; Path=/; HttpOnly; Secure; SameSite=Strict");
    assert_eq!(cookie(&mut it, "/lax"), "a=b; Path=/; HttpOnly; Secure; SameSite=Lax");
    // `None` is only kept by browsers on a Secure cookie.
    assert_eq!(cookie(&mut it, "/none"), "a=b; Path=/; HttpOnly; Secure; SameSite=None");
    assert_eq!(cookie(&mut it, "/plain"), "a=b; Path=/; HttpOnly; SameSite=Lax");
    assert_eq!(cookie(&mut it, "/no-options"), "a=b; Path=/; HttpOnly; Secure; SameSite=Lax");
    for (url, why) in [
        ("/bad-domain", "domain is a host name"),
        ("/empty-domain", "domain is a host name"),
        ("/number-domain", "domain is a host name"),
        ("/bad-path", "path starts with /"),
        ("/path-break", "path starts with /"),
        ("/bad-age", "max_age is seconds or a duration"),
        ("/bad-same-site", "same_site is \"Lax\""),
        ("/number-same-site", "same_site is a str, got int"),
        ("/not-a-map", "`set_cookie` expects options as a map, got int"),
    ] {
        says(get(&mut it, url), 500, why);
    }
}

#[test]
fn a_cookies_name_and_value_are_made_safe() {
    let mut it = app(
        r#"
route GET /text { set_cookie("a", "x y;é%"); "set" }
route GET /number { set_cookie("a", 12); "set" }
route GET /none { set_cookie("a"); "set" }
route GET /null { set_cookie("a", null); "set" }
route GET /twice { set_cookie("a", "1"); set_cookie("a", "2"); set_cookie("b", "3"); "set" }
route GET /bad-name { set_cookie("a b", "x"); "set" }
route GET /no-name { set_cookie(); "set" }
route GET /delete { delete_cookie("a", { path: "/x" }); "set" }
route GET /delete-bad { delete_cookie("a=", {}); "set" }
route GET /delete-options { delete_cookie("a", 1); "set" }
route GET /read = str(cookie("a"))
route GET /read-nothing = cookie()
route GET /read-number = cookie(1)
"#,
    );
    assert!(get(&mut it, "/text").header("set-cookie").unwrap().starts_with("a=x%20y%3B%C3%A9%25;"));
    assert!(get(&mut it, "/number").header("set-cookie").unwrap().starts_with("a=12;"));
    assert!(get(&mut it, "/none").header("set-cookie").unwrap().starts_with("a=;"));
    assert!(get(&mut it, "/null").header("set-cookie").unwrap().starts_with("a=;"));
    let twice = get(&mut it, "/twice");
    let set: Vec<&str> = twice.headers.iter().filter(|(k, _)| k == "set-cookie").map(|(_, v)| v.as_str()).collect();
    assert_eq!(set.len(), 2);
    assert!(set[0].starts_with("a=2;") && set[1].starts_with("b=3;"), "{:?}", set);
    says(get(&mut it, "/bad-name"), 500, "`set_cookie` needs a cookie name of letters");
    says(get(&mut it, "/no-name"), 500, "`set_cookie` needs a cookie name");
    assert_eq!(get(&mut it, "/delete").header("set-cookie").unwrap(), "a=; Path=/x; Max-Age=0; HttpOnly; Secure; SameSite=Lax");
    says(get(&mut it, "/delete-bad"), 500, "`delete_cookie` needs a cookie name");
    says(get(&mut it, "/delete-options"), 500, "`delete_cookie` expects options as a map");
    assert_eq!(send(&mut it, "GET", "/read", &[("Cookie", "a=caf%C3%A9; b=2")], "").text(), "café");
    assert_eq!(get(&mut it, "/read").text(), "null");
    says(get(&mut it, "/read-nothing"), 500, "`cookie` expects a cookie's name, got nothing");
    says(get(&mut it, "/read-number"), 500, "`cookie` expects a cookie's name, got int");
}

#[test]
fn outside_a_request_there_are_no_cookies_and_setting_one_does_no_harm() {
    let mut it = app("let seen = cookie(\"a\")\nset_cookie(\"early\", \"1\")\nroute GET / = str(seen)\n");
    assert_eq!(get(&mut it, "/").text(), "null");
}

#[test]
fn a_session_needs_a_map_and_an_age() {
    let mut it = app(
        r#"
route GET /end { set_session(null); "ended" }
route GET /number { set_session(3); "no" }
route GET /nothing { set_session(); "no" }
route GET /zero { set_session({ a: 1 }, { max_age: 0 }); "no" }
route GET /soon { set_session({ a: 1 }, { max_age: "soon" }); "no" }
route GET /options { set_session({ a: 1 }, 3); "no" }
route GET /hour { set_session({ a: 1 }, { max_age: "1h" }); "ok" }
route GET /no-age { set_session({ a: 1 }, { max_age: null }); "ok" }
route GET /a = str(session().a)
"#,
    );
    let ended = get(&mut it, "/end");
    assert!(ended.header("set-cookie").unwrap().starts_with("__Host-quillo_session=; Path=/; Max-Age=0"), "{:?}", ended.header("set-cookie"));
    says(get(&mut it, "/number"), 500, "`set_session` expects a map, got int");
    says(get(&mut it, "/nothing"), 500, "`set_session` expects a map, got nothing");
    says(get(&mut it, "/zero"), 500, "max_age is seconds or a duration");
    says(get(&mut it, "/soon"), 500, "max_age is seconds or a duration");
    says(get(&mut it, "/options"), 500, "`set_session` expects options as a map");
    assert!(get(&mut it, "/hour").header("set-cookie").unwrap().contains("Max-Age=3600"));
    assert!(get(&mut it, "/no-age").header("set-cookie").unwrap().contains(&format!("Max-Age={}", 30 * 86400)));
    // Two cookies that are both good are no session: one was planted.
    let good = get(&mut it, "/hour").header("set-cookie").unwrap().split(';').next().unwrap().to_string();
    assert_eq!(send(&mut it, "GET", "/a", &[("Cookie", &good)], "").text(), "1");
    let two = format!("{}; {}", good, good);
    assert_eq!(send(&mut it, "GET", "/a", &[("Cookie", &two)], "").text(), "null");
}

#[test]
fn a_mounted_programs_session_cookie_is_its_own() {
    let mut it = app("route GET /in { set_session({ a: 1 }); set_cookie(\"x\", \"1\"); \"ok\" }\n");
    it.base = "/app".into();
    let r = get(&mut it, "/in");
    let set: Vec<&str> = r.headers.iter().filter(|(k, _)| k == "set-cookie").map(|(_, v)| v.as_str()).collect();
    assert!(set.iter().any(|c| c.starts_with("quillo_session=") && c.contains("Path=/app;")), "{:?}", set);
    assert!(set.iter().any(|c| c.starts_with("x=1; Path=/app;")), "{:?}", set);
}

#[test]
fn a_secure_cookie_sent_over_plain_http_still_goes_out() {
    let mut it = app("route GET / { set_cookie(\"a\", \"1\"); \"ok\" }\n");
    let from_afar = |host: &str, more: &[(&str, &str)]| {
        let mut r = request("GET", "/", more, b"");
        r.headers[0].1 = host.to_string();
        r.remote = "10.1.2.3".into();
        r
    };
    for req in [from_afar("[::1]:8080", &[]), from_afar("app.localhost", &[]), from_afar("site.example", &[("x-forwarded-proto", "https")]), from_afar("site.example:8080", &[]), from_afar("site.example", &[])] {
        assert!(it.handle(&req).header("set-cookie").unwrap().contains("; Secure"));
    }
}

#[test]
fn a_seal_is_good_only_for_its_purpose_and_unchanged() {
    let s = session::seal("a purpose", "payload");
    assert_eq!(session::unseal("a purpose", &s).as_deref(), Some("payload"));
    assert_eq!(session::unseal("another purpose", &s), None);
    assert_eq!(session::unseal("a purpose", &format!("{}x", s)), None);
    assert_eq!(session::unseal("a purpose", "no dot"), None);
}

#[test]
fn streams_are_checked_and_made_whole_outside_the_server() {
    let mut it = app(
        r##"
let saved = null
route GET /text = stream((send) => { send("a"); send(<b>c</b>); send(null); send([1]) })
route GET /events = events((send) => { send(null); send({ a: 1 }, "na\nme"); send(2, null) })
route GET /keep = stream((send) => { saved = send })
route GET /late = str(saved("x"))
route GET /not-a-function = stream(1)
route GET /nothing = stream()
route GET /options = stream((send) => send("x"), 3)
route GET /status = stream((send) => send("x"), { status: 99 })
route GET /headers = stream((send) => send("x"), { type: "text/csv", status: 201, headers: { "Content-Type": "text/tab", "x-a": 1 } })
route GET /no-options = events((send) => send(1), null)
route GET /listen = str(listen("/p", (x) => x))
route GET /listen-bad = listen(1)
route GET /receive = str(receive("/p", print))
route GET /receive-bad = receive("/p")
"##,
    );
    assert_eq!(get(&mut it, "/text").text(), "a<b>c</b>[1]\n");
    assert_eq!(get(&mut it, "/events").text(), ":\n\nevent: name\ndata: {\"a\":1}\n\ndata: 2\n\n");
    assert_eq!(get(&mut it, "/keep").status, 200);
    says(get(&mut it, "/late"), 500, "`send` works while a `stream` or `events` answer is being sent");
    says(get(&mut it, "/not-a-function"), 500, "`stream` takes a function that sends, as in stream((send) => send(\"...\")), got int");
    says(get(&mut it, "/nothing"), 500, "got nothing");
    says(get(&mut it, "/options"), 500, "`stream` takes options as a map, got int");
    says(get(&mut it, "/status"), 500, "`stream`: status must be an int from 200 to 599, got 99");
    let r = get(&mut it, "/headers");
    assert_eq!((r.status, r.header("content-type"), r.header("x-a")), (201, Some("text/tab"), Some("1")));
    assert_eq!(get(&mut it, "/no-options").header("content-type"), Some("text/event-stream"));
    assert_eq!(get(&mut it, "/listen").text(), "null");
    assert_eq!(get(&mut it, "/receive").text(), "null");
    says(get(&mut it, "/listen-bad"), 500, "`listen` takes a URL and a function");
    says(get(&mut it, "/receive-bad"), 500, "`receive` takes a URL and a function");
}

#[test]
fn quillos_own_addresses_are_for_those_they_are_for() {
    let mut it = app("route GET / = \"hi\"\n");
    let token = ("x-quillo-job", jobs::token());
    for path in ["job/x", "tick/x", "udp/1", "later", "channel/x"] {
        let url = format!("/_quillo/{}", path);
        // Only the scheduler's POST, with this process's secret.
        assert_eq!(send(&mut it, "POST", &url, &[], "").status, 404, "{}", url);
        assert_eq!(send(&mut it, "GET", &url, &[token], "").status, 404, "{}", url);
    }
    says(send(&mut it, "POST", "/_quillo/job/x", &[token], ""), 404, "no such job");
    says(send(&mut it, "POST", "/_quillo/tick/x", &[token], ""), 404, "no such tick");
    says(send(&mut it, "POST", "/_quillo/job/billing~x", &[token], ""), 404, "no such service");
    says(send(&mut it, "POST", "/_quillo/tick/billing~x", &[token], ""), 404, "no such service");
    assert_eq!(send(&mut it, "GET", "/_quillo/nothing-here", &[], "").status, 404);
    assert_eq!(get(&mut it, "/_quillo/src/nohash/none.js").status, 404);
    assert_eq!(send(&mut it, "GET", "/_quillo/wt-ticket", &[], "").status, 405);
    assert_eq!(send(&mut it, "POST", "/_quillo/wt-ticket", &[("Sec-Fetch-Site", "cross-site")], "").status, 403);
    let ticket = send(&mut it, "POST", "/_quillo/wt-ticket", &[], "");
    assert!(ticket.status == 200 && ticket.text().contains("ticket"), "{}", ticket.text());
    let text = ticket.text();
    let j: serde_json::Value = serde_json::from_str(&text).unwrap();
    let query = format!("t={}", j["ticket"].as_str().unwrap());
    assert!(quillo::wt::ticket_of(&query).is_some());
    assert!(quillo::wt::ticket_of("t=forged.ticket").is_none());
    assert!(quillo::wt::ticket_of("").is_none());
}

#[test]
fn what_a_process_gets_from_another_must_be_signed() {
    let mut it = app("route GET / = \"hi\"\n");
    std::env::remove_var("QUILLO_SERVICE_SECRET");
    says(send(&mut it, "POST", "/_quillo/service-channel/room", &[], ""), 403, "QUILLO_SERVICE_SECRET");
    says(send(&mut it, "POST", "/_quillo/service-channels", &[], ""), 403, "QUILLO_SERVICE_SECRET");
    says(send(&mut it, "POST", "/_quillo/channel-relay", &[], ""), 403, "QUILLO_SERVICE_SECRET");
}

#[test]
fn a_job_or_a_tick_of_a_service_runs_in_the_service() {
    let (mut it, d) = files(
        "service-job",
        &[
            ("app.qwe", "service billing = \"billing.qwe\"\nroute GET / = str(billing.runs())\n"),
            (
                "billing.qwe",
                "let n = 0\nfn work() { n += 1 }\nfn bad() { n = 1 / 0 }\nschedule(\"1h\", work, { name: \"sweep\" })\nschedule(\"1h\", bad, { name: \"fails\" })\nevery(3600000, work)\nserver fn runs() = n\n",
            ),
        ],
    );
    let token = ("x-quillo-job", jobs::token());
    says(send(&mut it, "POST", "/_quillo/job/billing~sweep?at=1", &[token], ""), 200, "ok");
    says(send(&mut it, "POST", "/_quillo/job/billing~none", &[token], ""), 404, "no such job");
    says(send(&mut it, "POST", "/_quillo/job/billing~fails", &[token], ""), 500, "internal error");
    assert_eq!(get(&mut it, "/").text(), "1");
    says(send(&mut it, "POST", "/_quillo/tick/billing~work", &[token], ""), 200, "ok");
    says(send(&mut it, "POST", "/_quillo/tick/billing~none", &[token], ""), 404, "no such tick");
    assert_eq!(get(&mut it, "/").text(), "2");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_failing_tick_says_so() {
    let mut it = app("let n = 0\nfn bad() { n = 1 / 0 }\nevery(3600000, bad)\nroute GET / = \"hi\"\n");
    let name = it.site.ticks[0].name.clone();
    assert_eq!(send(&mut it, "POST", &format!("/_quillo/tick/{}", name), &[("x-quillo-job", jobs::token())], "").status, 500);
}

#[test]
fn a_server_fn_can_answer_with_a_response_of_its_own() {
    let mut it = app(
        r#"
server fn made() = respond(201, "made")
server fn early() { reply 202 "early"; 1 }
server fn fails() = 1 / 0
server fn gives_err() -> int ! str = err("no")
"#,
    );
    says(call(&mut it, "made", "[]"), 201, "made");
    says(call(&mut it, "early", "[]"), 202, "early");
    says(call(&mut it, "fails", "[]"), 500, "internal error in `fails`");
    says(call(&mut it, "gives_err", "[]"), 200, r#"{"err":"#);
    says(live(&mut it, "made", "[]", &[]), 201, "made");
    says(live(&mut it, "early", "[]", &[]), 202, "early");
    says(live(&mut it, "fails", "[]", &[]), 500, "internal error in `fails`");
    says(live(&mut it, "gives_err", "[]", &[]), 200, r#""err":"#);
}

#[test]
fn a_name_no_longer_a_function_is_no_server_fn() {
    let mut it = app("server fn f() = 1\nroute GET /x { f = 2; \"changed\" }\n");
    assert_eq!(call(&mut it, "f", "[]").status, 200);
    assert_eq!(get(&mut it, "/x").text(), "changed");
    says(call(&mut it, "f", "[]"), 404, "no server fn `f`");
}

#[test]
fn a_record_argument_whose_default_fails_is_refused() {
    let mut it = app("fn boom() = 1 / 0\ntype Thing = { a: int = boom() }\nserver fn f(t: Thing) = 1\n");
    says(call(&mut it, "f", "[{}]"), 400, "argument `t` of `f`");
}

#[test]
fn a_default_that_replies_cannot_make_an_argument() {
    let mut it = app("fn stop() { reply 418 \"no\" }\ntype Thing = { a: int = stop() }\nserver fn f(t: Thing) = 1\n");
    says(call(&mut it, "f", "[{}]"), 400, "argument `t` of `f` could not be made");
}

#[test]
fn a_live_answer_is_shared_unless_the_run_set_a_cookie_or_changed_something() {
    let mut it = app(
        r#"
let runs = 0
server fn plain() { runs += 1; { runs } }
server fn cookie_setter() { set_cookie("a", "b"); len(str(random())) }
"#,
    );
    let text = |r: HttpResponse| r.text();
    let first = text(live(&mut it, "plain", "[]", &[]));
    assert!(first.contains("\"runs\":1"), "{}", first);
    // It changed `runs`, which the run read: not shared.
    let second = text(live(&mut it, "plain", "[]", &[]));
    assert!(second.contains("\"runs\":2"), "{}", second);
    // A cookie is for the one asking.
    let r = live(&mut it, "cookie_setter", "[]", &[]);
    assert!(r.header("set-cookie").is_some());
    let r = live(&mut it, "cookie_setter", "[]", &[]);
    assert!(r.header("set-cookie").is_some());
}

#[test]
fn a_live_call_with_files_is_not_shared() {
    let mut it = app("let runs = 0\nserver fn count(n: int) { runs += n; 1 }\nserver fn read(n: int) = n\n");
    let body = "--b\r\nContent-Disposition: form-data; name=\"args\"\r\n\r\n[4]\r\n--b--\r\n";
    let h = [("Content-Type", "multipart/form-data; boundary=b"), ("x-quillo-rpc", "1"), ("x-quillo-live", "1")];
    let r = send(&mut it, "POST", "/_quillo/rpc/read", &h, body);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&r.text()).unwrap()["ok"], 4);
    let r = send(&mut it, "POST", "/_quillo/rpc/read", &h, body);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&r.text()).unwrap()["ok"], 4);
}

#[test]
fn a_live_answer_says_what_changed_since_what_the_page_has() {
    let mut it = app(
        r#"
let rows = [{ id: 1, t: "a" }, { id: 2, t: "b" }]
route GET /touch { rows = rows + [{ id: 3, t: "c" }]; "ok" }
server fn all() = rows
"#,
    );
    let first: serde_json::Value = serde_json::from_str(&live(&mut it, "all", "[]", &[]).text()).unwrap();
    let h = first["h"].as_str().unwrap().to_string();
    assert!(first["ok"].is_array());
    let same: serde_json::Value = serde_json::from_str(&live(&mut it, "all", "[]", &[("x-quillo-had", &h)]).text()).unwrap();
    assert_eq!(same["patch"], serde_json::json!({ "$=": 1 }));
    assert_eq!(get(&mut it, "/touch").text(), "ok");
    let next: serde_json::Value = serde_json::from_str(&live(&mut it, "all", "[]", &[("x-quillo-had", &h)]).text()).unwrap();
    assert_eq!(next["patch"], serde_json::json!({ "$l": [[0, 2], { "$v": { "id": 3, "t": "c" } }] }));
}

#[test]
fn another_pages_runtime_is_answered_with_this_programs() {
    let mut it = app("component C() {\n    ~n = 0\n    <button @click={() => { n = n + 1 }}>{n}</button>\n}\nroute GET / = <html><body><C/></body></html>\n");
    let page = get(&mut it, "/").text();
    let at = page.find("/_quillo/r/").expect(&page);
    let own = page[at..at + page[at..].find('"').unwrap()].to_string();
    let r = get(&mut it, &own);
    assert!(r.header("cache-control").unwrap().contains("immutable"));
    let other = get(&mut it, "/_quillo/r/0000000000.js");
    assert_eq!((other.header("cache-control"), other.text()), (Some("no-cache"), r.text()));
}

#[test]
fn a_pages_sheet_goes_in_its_head_or_as_near_as_it_can() {
    let mut it = app(
        r#"
view Tag() {
    style css`.tag { color: green; }`
    <i.tag>"x"</i>
}
route GET /head = <html><head><title>"t"</title></head><body><Tag/></body></html>
route GET /body = raw("<body class=\"a\"><Tag/></body>") + <Tag/>
route GET /bare = <Tag/>
route GET /no-end = <Tag/> + raw("<body")
"#,
    );
    let sheet = "<link rel=\"stylesheet\" href=\"/_quillo/s/";
    let head = get(&mut it, "/head").text();
    assert!(head.find(sheet).unwrap() < head.find("</head>").unwrap(), "{}", head);
    let body = get(&mut it, "/body").text();
    assert!(body.starts_with("<body class=\"a\"><link"), "{}", body);
    let bare = get(&mut it, "/bare").text();
    assert!(bare.starts_with(sheet), "{}", bare);
    let open = get(&mut it, "/no-end").text();
    assert!(open.starts_with(sheet), "{}", open);
}

#[test]
fn a_page_that_has_the_prefetch_script_does_not_get_it_twice() {
    let code = quillo::server::PREFETCH_JS;
    let src = format!("route GET /own = raw(\"<body><script>\" + read_file(\"prefetch.js\") + \"</script></body>\")\nroute GET /quoted = raw(\"<body><code>\" + read_file(\"prefetch.js\") + \"</code></body>\")\n");
    let (mut it, d) = files("prefetch", &[("app.qwe", &src), ("prefetch.js", code)]);
    assert_eq!(get(&mut it, "/own").text().matches("__quillo_pf").count(), 2);
    // Text that quotes it is no script: the page still gets its own.
    assert_eq!(get(&mut it, "/quoted").text().matches("<script>").count(), 1);
    assert_eq!(get(&mut it, "/quoted").text().matches("__quillo_pf").count(), 4);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_prefetch_of_a_page_with_waiting_parts_makes_it_whole() {
    let mut it = app(
        r#"
fn slow() = [1, 2]
component Page() {
    state s = defer(slow())
    <div>{s == null ? <p>"waiting"</p> : <ul>{for v in s => <li>{v}</li>}</ul>}</div>
}
route GET / = <html><body><Page/></body></html>
route GET /fragment = respond(404, <Page/>)
"#,
    );
    let r = send(&mut it, "GET", "/", &[("x-quillo-prefetch", "1")], "");
    assert_eq!(r.status, 200);
    assert_eq!(r.header("cache-control"), Some("private, max-age=30"));
    let text = r.text();
    assert!(text.contains("<li>1</li>") && text.contains("__quillo_swap"), "{}", text);
    // Not a page that is sent: nothing follows it.
    let r = send(&mut it, "GET", "/fragment", &[("sec-purpose", "prefetch")], "");
    assert_eq!(r.status, 404);
    assert!(!r.text().contains("<li>"), "{}", r.text());
}

#[test]
fn a_desktop_window_is_admitted_by_its_token_or_cookie() {
    let token = launch_token().unwrap();
    assert_eq!(token.len(), 64);
    assert_ne!(token, launch_token().unwrap());
    let gate = |url: &str, cookie: Option<&str>| {
        let mut r = request("GET", url, &[], b"");
        if let Some(c) = cookie {
            r.headers.push(("Cookie".into(), c.into()));
        }
        launch_gate(&r, &token)
    };
    assert_eq!(gate("/", None).unwrap().status, 403);
    let ok = format!("quillo_launch={}", token);
    assert!(gate("/", Some(&ok)).is_none());
    // Other cookies, or another token, admit no one.
    assert_eq!(gate("/", Some("other=1; plain; quillo_launch=wrong")).unwrap().status, 403);
    assert!(gate("/", Some(&format!("other=1; plain; {}", ok))).is_none());
    assert_eq!(gate("/?_quillo_launch=wrong", None).unwrap().status, 403);
    let r = gate(&format!("/page?a=1&_quillo_launch={}&&b=2", token), None).unwrap();
    assert_eq!(r.status, 303);
    assert_eq!(r.header("location"), Some("/page?a=1&b=2"));
    assert!(r.header("set-cookie").unwrap().contains("HttpOnly; SameSite=Strict"));
    let r = gate(&format!("/page?_quillo_launch={}", token), None).unwrap();
    assert_eq!(r.header("location"), Some("/page"));
}

#[test]
fn serving_no_instances_is_an_error() {
    let e = quillo::server::serve_many(Vec::new(), "127.0.0.1:0", 1).unwrap_err();
    assert_eq!(e, "no instances");
    let e = quillo::server::serve_many(Vec::new(), "256.0.0.1:1", 1).unwrap_err();
    assert!(e.starts_with("cannot listen on 256.0.0.1:1"), "{}", e);
}

#[test]
fn outside_a_request_there_is_nothing_to_wait_elsewhere_for() {
    assert!(quillo::httpd::await_elsewhere(Box::pin(async { 1 })).is_none());
    assert!(!quillo::httpd::in_task());
    assert_eq!(quillo::httpd::wait_elsewhere(|| 2).unwrap_or_else(|f| f()), 2);
}
