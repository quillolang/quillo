//! Services at their edges (src/service.rs): how calls are refused, retried and
//! given up on, copies that join, calls made later that wait, and `--split`
//! when something is wrong.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::service;
use quillo::Interp;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

const SECRET: &str = "a-test-secret-for-services-long-enough";

/// The tests in this process sign with one secret.
fn signing() {
    static SET: Once = Once::new();
    SET.call_once(|| std::env::set_var("QUILLO_SERVICE_SECRET", SECRET));
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-services-edge-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn text(r: &HttpResponse) -> String {
    String::from_utf8_lossy(&r.body).into_owned()
}

fn request(method: &str, url: &str, headers: Vec<(String, String)>, body: &str, remote: &str) -> HttpRequest {
    let mut h = vec![("Host".to_string(), "localhost".to_string()), ("Content-Type".to_string(), "application/json".to_string())];
    h.extend(headers);
    HttpRequest { method: method.into(), url: url.into(), headers: h, body: body.as_bytes().to_vec(), remote: remote.into() }
}

/// The headers of a signed call of `f` on service `to`.
fn signed(t: u64, to: &str, f: &str, call: &str, tenant: &str, body: &str) -> Vec<(String, String)> {
    let sig = service::signature(SECRET, t, to, f, tenant, call, body.as_bytes());
    let mut h = vec![("X-Quillo-Service".to_string(), format!("t={},sig={}", t, sig)), ("X-Quillo-To".to_string(), to.to_string()), ("X-Quillo-Call".to_string(), call.to_string())];
    if !tenant.is_empty() {
        h.push(("X-Quillo-Tenant".to_string(), tenant.to_string()));
    }
    h
}

const WORK: &str = r#"
server fn add(a: int, b: int = 1) -> int = a + b
server fn page() = redirect("/elsewhere")
server fn replies() {
    reply 200 "a page"
}
server fn broken() = 1 / len(env("NOT_SET_ANYWHERE", ""))
server fn fails() = err("it did not go well")
fn hidden() = 1
"#;

/// A program that declares service `name`, of a file of `WORK`, and has it
/// called at `url` when one is given.
fn program(name: &str, url: Option<&str>, options: &str) -> Interp {
    let d = dir(name);
    std::fs::write(d.join("work.qwe"), WORK).unwrap();
    let mut it = Interp::new(&d);
    if let Some(u) = url {
        it.shared.service_urls.lock().unwrap().insert(name.to_string(), u.to_string());
    }
    it.load_source(&format!("service {} = \"work.qwe\" {}\nroute GET / = \"up\"\n", name, options), "main.qwe").unwrap();
    it
}

fn ev(it: &mut Interp, src: &str) -> String {
    match it.load_source(src, "after.qwe") {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

// ---- declaring ---------------------------------------------------------------------

#[test]
fn how_a_service_is_called_is_checked_where_it_is_declared() {
    let d = dir("opts");
    std::fs::write(d.join("work.qwe"), WORK).unwrap();
    let bad = |o: &str| {
        let mut it = Interp::new(&d);
        match it.load_source(&format!("service opts_bad = \"work.qwe\" {}\n", o), "main.qwe") {
            Ok(_) => panic!("{} was taken", o),
            Err(e) => e.to_string(),
        }
    };
    assert!(bad("{ timeout: 0 }").contains("`timeout` is a duration (\"2s\", \"500ms\", or milliseconds), not 0"));
    assert!(bad("{ timeout: \"1h\" }").contains("`timeout` is a duration"));
    assert!(bad("{ timeout: \"x\" }").contains("`timeout` is a duration"));
    assert!(bad("{ timeout: true }").contains("`timeout` is a duration"));
    assert!(bad("{ timeout: -1 }").contains("`timeout` is a duration"));
    assert!(bad("{ timeout: -1.5 }").contains("`timeout` is a duration"));
    assert!(bad("{ cooldown: \"soon\" }").contains("`cooldown` is a duration (\"5s\", or milliseconds), not"));
    assert!(bad("{ retries: 11 }").contains("`retries` is a number of tries after the first, 0 to 10, not 11"));
    assert!(bad("{ retries: \"2\" }").contains("`retries` is a number"));
    assert!(bad("{ failures: 0 }").contains("`failures` is how many failures in a row leave an address alone, 1 or more, not 0"));
    assert!(bad("{ wait: 1 }").contains("no option `wait` (timeout, retries, failures, cooldown)"));

    let mut it = Interp::new(&d);
    it.load_source("service opts_good = \"work.qwe\" { timeout: 1500.5, cooldown: \"1m\", retries: 0, failures: 1 }\nservice opts_text = \"work.qwe\" { timeout: \"1.5\", cooldown: \"250ms\" }\n", "main.qwe").unwrap();
    assert_eq!(service::opts("opts_good"), service::Opts { timeout: Duration::from_secs_f64(1.5005), retries: 0, failures: 1, cooldown: Duration::from_secs(60) });
    let o = service::opts("opts_text");
    assert_eq!((o.timeout, o.cooldown, o.retries, o.failures), (Duration::from_millis(1500), Duration::from_millis(250), 2, 5));
    // Not declared: the defaults.
    assert_eq!(service::opts("opts_none"), service::Opts::default());
}

#[test]
fn a_service_is_one_file_however_often_it_is_declared() {
    let d = dir("twice");
    std::fs::write(d.join("work.qwe"), WORK).unwrap();
    std::fs::write(d.join("other.qwe"), WORK).unwrap();
    let mut it = Interp::new(&d);
    // The same file again is the same service.
    assert_eq!(it.load_source("service twice = \"work.qwe\"\nservice twice = \"work.qwe\"\ntwice.add(1, 2)", "main.qwe").unwrap().repr(), "3");
    let e = it.load_source("service twice = \"other.qwe\"\n", "main.qwe").err().unwrap().to_string();
    assert!(e.contains("service `twice` is") && e.contains("elsewhere in this app") && e.contains("other.qwe here"), "{}", e);
}

#[test]
fn what_a_service_offers_is_read_through_the_files_it_uses() {
    let d = dir("offered");
    std::fs::write(d.join("shared.qwe"), "server fn shared() = 1\n").unwrap();
    std::fs::write(d.join("one.qwe"), "use \"shared.qwe\"\nserver fn one() = 1\n").unwrap();
    std::fs::write(d.join("two.qwe"), "use \"shared.qwe\"\nserver fn two() = 2\n").unwrap();
    std::fs::write(d.join("chart.js"), "export const x = 1\n").unwrap();
    std::fs::write(d.join("main.qwe"), "use \"one.qwe\"\nuse \"two.qwe\"\nuse Chart from \"chart.js\"\nfn not_one() = 1\nserver fn mine() = 1\n").unwrap();
    let names: Vec<String> = service::offered(&d.join("main.qwe")).unwrap().iter().map(|f| f.name.clone()).collect();
    assert_eq!(names, ["shared", "one", "two", "mine"]);
    let e = service::offered(&d.join("missing.qwe")).err().unwrap();
    assert!(e.contains("cannot read") && e.contains("missing.qwe"), "{}", e);
    std::fs::write(d.join("broken.qwe"), "fn (\n").unwrap();
    assert!(service::offered(&d.join("broken.qwe")).err().unwrap().contains("broken.qwe"));
}

#[test]
fn a_prefix_given_by_a_later_declaration_counts() {
    let d = dir("declared");
    std::fs::write(d.join("risk.qwe"), "server fn score() = 1\n").unwrap();
    std::fs::write(d.join("shop.qwe"), "service /risk = \"risk.qwe\"\n").unwrap();
    std::fs::write(d.join("main.qwe"), "service risk = \"risk.qwe\"\nservice shop = \"shop.qwe\"\n").unwrap();
    let mut found = service::declared(&d.join("main.qwe")).unwrap();
    found.sort();
    let names: Vec<(String, Option<String>)> = found.iter().map(|(n, p, _)| (n.clone(), p.clone())).collect();
    assert_eq!(names, [("risk".to_string(), Some("/risk".to_string())), ("shop".to_string(), None)]);
}

// ---- calling, here ----------------------------------------------------------------

#[test]
fn a_call_here_gives_what_the_function_gives_or_says_why_not() {
    let mut it = program("here", None, "");
    assert_eq!(ev(&mut it, "here.add(2)"), "3");
    assert_eq!(ev(&mut it, "__service(\"here\", \"add\", [2, 5])"), "7");
    assert_eq!(ev(&mut it, "here.fails()"), "err(\"it did not go well\")");
    let e = |it: &mut Interp, src: &str| ev(it, src);
    assert!(e(&mut it, "__service(\"nothing\", \"f\", [], 0)").contains("service nothing: not running here, and no address for it (QUILLO_SERVICE_NOTHING or --service-url)"));
    assert!(e(&mut it, "__service(\"here\", \"nope\", [], 0)").contains("service here: no server fn `nope`"));
    assert!(e(&mut it, "__service(\"here\", \"hidden\", [], 0)").contains("no server fn `hidden`"));
    assert!(e(&mut it, "__service(\"here\", \"add\", [1, 2, 3], 3)").contains("service here: `add` takes 2 arguments, got 3"));
    assert!(e(&mut it, "here.page()").contains("service here: `page` answered with a page, not a value"));
    assert!(e(&mut it, "here.replies()").contains("service here: `replies` replied with a page, not a value"));
    assert!(e(&mut it, "here.broken()").contains("service here: internal error in `broken`"));
    let f = it.load_source("__service(1)", "x.qwe").err().unwrap().to_string();
    assert!(f.contains("`__service` is Quillo's own: call a service as `billing.invoice(...)`"), "{}", f);
    let f = it.load_source("__service_later(\"here\")", "x.qwe").err().unwrap().to_string();
    assert!(f.contains("`__service_later` is Quillo's own"), "{}", f);
}

// ---- calling, elsewhere ----------------------------------------------------------------

struct Req {
    head: String,
}

fn read_request(s: &mut TcpStream) -> Option<(Req, Vec<u8>)> {
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 4096]);
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
    while buf.len() < end + len {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    Some((Req { head }, buf[end..end + len].to_vec()))
}

/// A service played by the test: `f` is told how many calls came before each,
/// and answers it; its URL.
fn fake(f: impl Fn(usize, &Req, &mut TcpStream) + Send + Sync + 'static) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (f, n) = (Arc::new(f), Arc::new(AtomicUsize::new(0)));
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let (f, n) = (f.clone(), n.clone());
            std::thread::spawn(move || {
                if let Some((r, _)) = read_request(&mut s) {
                    f(n.fetch_add(1, Ordering::SeqCst), &r, &mut s);
                }
            });
        }
    });
    format!("http://127.0.0.1:{}", port)
}

fn answer(s: &mut TcpStream, status: u16, body: &str) {
    let _ = write!(s, "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", status, body.len(), body);
}

/// A URL no one answers at.
fn nobody() -> String {
    format!("http://127.0.0.1:{}", support::free_port())
}

#[test]
fn what_a_service_answers_is_taken_for_what_it_is() {
    signing();
    let url = fake(|n, _, s| match n {
        0 => answer(s, 200, "not json"),
        1 => answer(s, 200, "{\"neither\":1}"),
        2 => answer(s, 200, "[1]"),
        3 => answer(s, 500, "{\"error\":\"it broke\"}"),
        4 => answer(s, 404, "plain words"),
        5 => answer(s, 200, "{\"err\":\"no\"}"),
        6 => {
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\ncut short");
        }
        _ => answer(s, 200, "{\"ok\":5,\"published\":[\"orders\",7]}"),
    });
    let mut it = program("answers", Some(&url), "{ retries: 0 }");
    let call = |it: &mut Interp| ev(it, "answers.add(1)");
    assert_eq!(call(&mut it), "err(\"service answers: an answer that is not one\")");
    assert_eq!(call(&mut it), "err(\"service answers: an answer that is not one\")");
    assert_eq!(call(&mut it), "err(\"service answers: an answer that is not one\")");
    assert_eq!(call(&mut it), "err(\"service answers: 500 it broke\")");
    assert_eq!(call(&mut it), "err(\"service answers: 404 plain words\")");
    assert_eq!(call(&mut it), "err(\"no\")");
    let out = call(&mut it);
    assert!(out.contains("service answers: no answer from http://127.0.0.1:"), "{}", out);
    // What it published is announced here too.
    let before = it.shared.live_seq();
    assert_eq!(call(&mut it), "5");
    assert!(it.shared.live_seq() > before);
}

#[test]
fn a_call_goes_on_past_a_busy_server_and_asks_again_of_one_still_running_it() {
    signing();
    // No answer, in time: the same address again, which says it is still at it.
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let s2 = seen.clone();
    let url = fake(move |n, r, s| {
        s2.lock().unwrap().push(r.head.lines().find(|l| l.to_ascii_lowercase().starts_with("x-quillo-call")).unwrap().to_string());
        match n {
            0 => {}
            1 => answer(s, 503, "{\"error\":\"this call is still running\"}"),
            _ => answer(s, 200, "{\"ok\":\"done\"}"),
        }
    });
    let mut it = program("again", Some(&url), "{ retries: 3 }");
    assert_eq!(ev(&mut it, "again.add(1)"), "\"done\"");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(seen.iter().all(|c| *c == seen[0]), "the same call each time: {:?}", seen);
    drop(seen);

    // A 502 is not at all what was asked: the next address.
    let bad = fake(|_, _, s| answer(s, 502, "{\"error\":\"down\"}"));
    let good = fake(|_, _, s| answer(s, 200, "{\"ok\":\"there\"}"));
    let mut it = program("next", Some(&format!("{},{}", bad, good)), "{ retries: 2 }");
    for _ in 0..4 {
        assert_eq!(ev(&mut it, "next.add(1)"), "\"there\"");
    }
    // Every address busy, tried as often as it says.
    let mut it = program("busy", Some(&fake(|_, _, s| answer(s, 504, "gateway"))), "{ retries: 1 }");
    let out = ev(&mut it, "busy.add(1)");
    assert!(out.contains("service busy: 504 at http://127.0.0.1:") && out.contains("(gateway)"), "{}", out);
}

#[test]
fn a_connection_lost_or_a_timeout_says_so() {
    signing();
    let mut it = program("lost", Some(&fake(|_, _, _| {})), "{ retries: 0 }");
    let out = ev(&mut it, "lost.add(1)");
    assert!(out.contains("service lost: no answer from http://127.0.0.1:"), "{}", out);

    let quiet = fake(|_, _, _| std::thread::sleep(Duration::from_secs(2)));
    let mut it = program("slow", Some(&quiet), "{ timeout: 50, retries: 0 }");
    let out = ev(&mut it, "slow.add(1)");
    assert!(out.contains("service slow: took more than 0.05 s (its timeout), at"), "{}", out);

    let mut it = program("gone", Some(&nobody()), "{ retries: 1, failures: 10 }");
    let out = ev(&mut it, "gone.add(1)");
    assert!(out.contains("service gone: unreachable at http://127.0.0.1:"), "{}", out);
}

#[test]
fn an_address_that_keeps_failing_is_left_alone_for_a_while() {
    signing();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let down = fake(move |_, _, s| {
        h.fetch_add(1, Ordering::SeqCst);
        answer(s, 502, "{\"error\":\"down\"}");
    });
    let mut it = program("circuit", Some(&down), "{ retries: 3, failures: 1, cooldown: 1000 }");
    // Its circuit opens at the first failure: no second try.
    let first = ev(&mut it, "circuit.add(1)");
    assert!(first.contains("502 at"), "{}", first);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    // While it is left alone, no call is made.
    let second = ev(&mut it, "circuit.add(1)");
    assert_eq!(second, format!("err(\"service circuit: unavailable (every address failed lately: {})\")", down));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    // Its time comes: one call tries it.
    std::thread::sleep(Duration::from_millis(1100));
    assert!(ev(&mut it, "circuit.add(1)").contains("502 at"));
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[test]
fn a_call_in_a_call_waits_no_longer_than_its_caller() {
    signing();
    let quiet = fake(|_, _, _| std::thread::sleep(Duration::from_secs(5)));
    let mut it = program("chain", Some(&quiet), "{ timeout: \"10s\", retries: 3 }");
    it.deadline = Some(Instant::now() + Duration::from_millis(500));
    let out = ev(&mut it, "chain.add(1)");
    assert!(out.contains("service chain: took more than") && out.contains("(the time its caller had left), at"), "{}", out);
    // Nothing left: not called at all.
    it.deadline = Some(Instant::now());
    let out = ev(&mut it, "chain.add(1)");
    assert_eq!(out, "err(\"service chain: not called: the call this one is part of has run out of time\")");
    it.deadline = None;
}

// ---- answering ------------------------------------------------------------------------

#[test]
fn a_signed_call_is_refused_for_what_is_wrong_with_it() {
    signing();
    let call = |it: &mut Interp, f: &str, h: Vec<(String, String)>, body: &str| {
        let r = it.handle(&request("POST", &format!("/_quillo/service/{}", f), h, body, "127.0.0.1"));
        (r.status, text(&r))
    };
    let t = now_secs();
    let id = |n: u32| format!("edgecall{}x{}", n, std::process::id());
    let mut work = Interp::new(dir("called-work"));
    work.load_source(WORK, "work.qwe").unwrap();
    let w = &mut work;

    let r = w.handle(&request("GET", "/_quillo/service/add", Vec::new(), "", "127.0.0.1"));
    assert_eq!((r.status, text(&r).as_str()), (405, "{\"error\":\"a service call is a POST\"}"));
    assert_eq!(call(w, "add", Vec::new(), "[1]").0, 401);
    // A name that is no tenant's.
    assert_eq!(call(w, "add", signed(t, "work", "add", &id(1), "no spaces", "[1]"), "[1]"), (400, "{\"error\":\"not a tenant's name\"}".to_string()));
    // Not what the caller's copy says the function is.
    let mut h = signed(t, "work", "add", &id(2), "", "[1]");
    h.push(("X-Quillo-Contract".to_string(), "add(a: str) -> str".to_string()));
    let (status, body) = call(w, "add", h, "[1]");
    assert_eq!(status, 409);
    assert!(body.contains("the caller's copy of this service says `add(a: str) -> str`"), "{}", body);
    // Not arguments at all; the wrong ones.
    assert_eq!(call(w, "add", signed(t, "work", "add", &id(3), "", "{}"), "{}"), (400, "{\"error\":\"the body must be a JSON array of arguments\"}".to_string()));
    let (status, body) = call(w, "add", signed(t, "work", "add", &id(4), "", "[\"x\"]"), "[\"x\"]");
    assert_eq!(status, 400, "{}", body);
    // What a value is not.
    assert_eq!(call(w, "page", signed(t, "work", "page", &id(5), "", "[]"), "[]"), (500, "{\"error\":\"`page` answered with a page, not a value\"}".to_string()));
    assert_eq!(call(w, "replies", signed(t, "work", "replies", &id(6), "", "[]"), "[]"), (500, "{\"error\":\"`replies` replied with a page, not a value\"}".to_string()));
    assert_eq!(call(w, "broken", signed(t, "work", "broken", &id(7), "", "[]"), "[]"), (500, "{\"error\":\"internal error in `broken`\"}".to_string()));
    assert_eq!(call(w, "fails", signed(t, "work", "fails", &id(8), "", "[]"), "[]"), (200, "{\"err\":\"it did not go well\",\"own\":true}".to_string()));
    // In a tenant, and sent again: answered as it was.
    let h = signed(t, "work", "add", &id(9), "acme", "[1,2]");
    assert_eq!(call(w, "add", h.clone(), "[1,2]"), (200, "{\"ok\":3}".to_string()));
    assert_eq!(call(w, "add", h, "[1,2]"), (200, "{\"ok\":3}".to_string()));
}

#[test]
fn a_call_still_running_is_said_to_be_and_not_run_twice() {
    signing();
    let d = dir("running");
    std::fs::write(d.join("slow.qwe"), "let runs = 0\nserver fn slow() {\n runs = runs + 1\n sleep(600)\n runs\n}\n").unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("slow.qwe")).unwrap();
    let call = format!("stillrunning{}", std::process::id());
    let h = signed(now_secs(), "slow", "slow", &call, "", "[]");
    let both: Vec<_> = (0..2)
        .map(|_| {
            let (h, mut fork) = (h.clone(), it.fork());
            std::thread::spawn(move || {
                let r = fork.handle(&request("POST", "/_quillo/service/slow", h, "[]", "127.0.0.1"));
                (r.status, text(&r))
            })
        })
        .collect();
    let mut got: Vec<(u16, String)> = both.into_iter().map(|t| t.join().unwrap()).collect();
    got.sort();
    assert_eq!(got, [(200, "{\"ok\":1}".to_string()), (503, "{\"error\":\"this call is still running\"}".to_string())]);
}

#[test]
fn calls_remembered_past_the_limit_do_not_stop_new_ones() {
    signing();
    let d = dir("seen-full");
    std::fs::write(d.join("work.qwe"), WORK).unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("work.qwe")).unwrap();
    let t = now_secs();
    let body = "[1,2]";
    for n in 0..100_002 {
        let r = it.handle(&request("POST", "/_quillo/service/add", signed(t, "work", "add", &format!("full{}", n), "", body), body, "127.0.0.1"));
        assert_eq!(r.status, 200, "{}", text(&r));
    }
}

// ---- copies that join --------------------------------------------------------------------

fn join_headers(service_name: &str, nonce: &str, body: &str, t: u64) -> Vec<(String, String)> {
    let sig = service::signature(SECRET, t, "_join", service_name, "", nonce, body.as_bytes());
    vec![("X-Quillo-Service".to_string(), format!("t={},sig={}", t, sig)), ("X-Quillo-Call".to_string(), nonce.to_string())]
}

#[test]
fn joining_is_refused_for_what_is_wrong_with_it() {
    signing();
    let mut it = program("joined1", None, "");
    let mut join = |h: Vec<(String, String)>, body: &str, remote: &str| {
        let r = it.handle(&request("POST", "/_quillo/join", h, body, remote));
        (r.status, text(&r))
    };
    let t = now_secs();
    let body = |port: u32| format!("{{\"service\":\"joined1\",\"port\":{}}}", port);
    let nonce = |n: u32| format!("nonce{}x{}", n, std::process::id());
    assert_eq!(join(Vec::new(), &body(1), "127.0.0.1").0, 401);
    assert_eq!(join(vec![("X-Quillo-Call".to_string(), "not plain!".to_string())], &body(1), "127.0.0.1").0, 401);
    assert_eq!(join(join_headers("joined1", &nonce(1), "not json", t), "not json", "127.0.0.1").0, 400);
    // Signed for something else, or too long ago.
    assert_eq!(join(join_headers("joined1", &nonce(2), &body(2), t), &body(1), "127.0.0.1").0, 401);
    assert_eq!(join(join_headers("joined1", &nonce(3), &body(1), t - 1000), &body(1), "127.0.0.1").0, 401);
    // Sent twice.
    let b = "{\"service\":\"joined1\",\"port\":0}";
    assert_eq!(join(join_headers("joined1", &nonce(4), b, t), b, "127.0.0.1"), (400, "{\"error\":\"a join gives the copy's port\"}".to_string()));
    assert_eq!(join(join_headers("joined1", &nonce(4), b, t), b, "127.0.0.1"), (409, "{\"error\":\"this join was sent before\"}".to_string()));
    let b = "{\"service\":\"other\",\"port\":80}";
    let (status, out) = join(join_headers("other", &nonce(5), b, t), b, "127.0.0.1");
    assert_eq!((status, out.as_str()), (404, "{\"error\":\"this program has no service `other`\"}"));
    let b = body(80);
    assert_eq!(join(join_headers("joined1", &nonce(6), &b, t), &b, "somewhere"), (400, "{\"error\":\"the copy's address is not known\"}".to_string()));

    // From IPv6, then IPv4 with its port; each is a copy; leaving, it is not.
    let (status, out) = join(join_headers("joined1", &nonce(7), &b, t), &b, "[::1]:50000");
    assert_eq!((status, out.as_str()), (200, "{\"service\":\"joined1\",\"at\":\"http://[::1]:80\"}"));
    let (status, _) = join(join_headers("joined1", &nonce(8), &b, t), &b, "10.0.0.7");
    assert_eq!(status, 200);
    assert_eq!(service::joined_urls("joined1"), ["http://10.0.0.7:80", "http://[::1]:80"]);
    let again = join(join_headers("joined1", &nonce(9), &b, t), &b, "10.0.0.7");
    assert_eq!(again.0, 200);
    let leave = "{\"service\":\"joined1\",\"port\":80,\"leave\":true}";
    assert_eq!(join(join_headers("joined1", &nonce(10), leave, t), leave, "10.0.0.7").0, 200);
    assert_eq!(join(join_headers("joined1", &nonce(11), leave, t), leave, "10.0.0.7").0, 200);
    assert_eq!(service::joined_urls("joined1"), ["http://[::1]:80"]);
    assert_eq!(join(join_headers("joined1", &nonce(12), leave, t), leave, "::1").0, 200);
    assert!(service::joined_urls("joined1").is_empty());
    assert!(service::joined_urls("never-joined").is_empty());
    let r = it.handle(&request("GET", "/_quillo/join", Vec::new(), "", "127.0.0.1"));
    assert_eq!(r.status, 405);
}

/// A copy of `name` joined from this machine, at `port`.
fn join_copy(it: &mut Interp, name: &str, port: u16, nonce: &str) {
    let b = format!("{{\"service\":\"{}\",\"port\":{}}}", name, port);
    let r = it.handle(&request("POST", "/_quillo/join", join_headers(name, nonce, &b, now_secs()), &b, "127.0.0.1:40000"));
    assert_eq!(r.status, 200, "{}", text(&r));
}

#[test]
fn calls_go_in_turn_to_a_copy_that_joined_and_here_again_when_it_does_not_answer() {
    signing();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let url = fake(move |_, _, s| {
        h.fetch_add(1, Ordering::SeqCst);
        answer(s, 200, "{\"ok\":100}");
    });
    let port: u16 = url.rsplit(':').next().unwrap().parse().unwrap();
    let mut it = program("copies", None, "{ retries: 0 }");
    join_copy(&mut it, "copies", port, &format!("copies{}", std::process::id()));
    assert_eq!(service::urls_of(&it.shared, "copies"), [url.clone()]);
    assert_eq!(service::first_url_of(&it.shared, "copies"), Some(url.clone()));
    let got: Vec<String> = (0..4).map(|_| ev(&mut it, "copies.add(1)")).collect();
    assert_eq!(got.iter().filter(|g| *g == "100").count(), 2, "{:?}", got);
    assert_eq!(got.iter().filter(|g| *g == "2").count(), 2, "{:?}", got);
    assert_eq!(hits.load(Ordering::SeqCst), 2);

    // A copy that joined and is gone: here instead.
    let down: u16 = nobody().rsplit(':').next().unwrap().parse().unwrap();
    let mut it = program("gonecopy", None, "{ retries: 0 }");
    join_copy(&mut it, "gonecopy", down, &format!("gonecopy{}", std::process::id()));
    for _ in 0..3 {
        assert_eq!(ev(&mut it, "gonecopy.add(1)"), "2");
    }

    // A copy joins one that runs elsewhere: taken with the addresses it was given.
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let url = fake(move |_, _, s| {
        h.fetch_add(1, Ordering::SeqCst);
        answer(s, 200, "{\"ok\":1}");
    });
    let mut it = program("both", Some(&url), "{ retries: 0 }");
    let other = fake(|_, _, s| answer(s, 200, "{\"ok\":2}"));
    let port: u16 = other.rsplit(':').next().unwrap().parse().unwrap();
    join_copy(&mut it, "both", port, &format!("both{}", std::process::id()));
    let mut got: Vec<String> = (0..6).map(|_| ev(&mut it, "both.add(1)")).collect();
    got.sort();
    got.dedup();
    assert_eq!(got, ["1", "2"]);
}

// ---- channels between processes -----------------------------------------------------------

#[test]
fn a_signed_post_gives_the_answer_or_says_what_went_wrong() {
    signing();
    let seen: Arc<Mutex<Option<String>>> = Default::default();
    let s2 = seen.clone();
    let url = fake(move |n, r, s| match n {
        0 => {
            *s2.lock().unwrap() = Some(r.head.clone());
            answer(s, 200, "[1]");
        }
        _ => answer(s, 401, "{\"error\":\"no\"}"),
    });
    assert_eq!(service::signed_post(&format!("{}/", url), "/x", "svc", "_f", b"[]").unwrap(), b"[1]");
    let head = seen.lock().unwrap().clone().unwrap();
    assert!(head.starts_with("POST /x HTTP/1.1\r\n"), "{}", head);
    assert!(head.to_ascii_lowercase().contains("x-quillo-to: svc\r\n"), "{}", head);
    assert_eq!(service::signed_post(&url, "/x", "svc", "_f", b"[]").unwrap_err(), "401 {\"error\":\"no\"}");
    let e = service::signed_post(&nobody(), "/x", "svc", "_f", b"[]").unwrap_err();
    assert!(!e.is_empty() && !e.starts_with("401"), "{}", e);
}

#[test]
fn what_passes_between_processes_is_signed_for_the_process_it_reaches() {
    signing();
    let mut it = Interp::new(dir("signed-for"));
    it.load_source("channel.on(\"news\", (m) => null)\nroute GET / = \"x\"\n", "main.qwe").unwrap();
    let f = "_channels";
    let sign = |to: &str, f: &str, c: &str, body: &str, t: u64| {
        let sig = service::signature(SECRET, t, to, f, "", c, body.as_bytes());
        vec![("X-Quillo-Service".to_string(), format!("t={},sig={}", t, sig)), ("X-Quillo-To".to_string(), to.to_string()), ("X-Quillo-Call".to_string(), c.to_string())]
    };
    let t = now_secs();
    let post = |it: &mut Interp, path: &str, h: Vec<(String, String)>, body: &str| {
        let r = it.handle(&request("POST", path, h, body, "127.0.0.1"));
        (r.status, text(&r))
    };
    assert_eq!(post(&mut it, "/_quillo/service-channels", sign("svc", f, "c1", "[]", t), "[]"), (200, "[\"news\"]".to_string()));
    assert_eq!(post(&mut it, "/_quillo/service-channels", Vec::new(), "[]").0, 401);
    assert_eq!(post(&mut it, "/_quillo/service-channels", sign("svc", f, "c1", "[]", t - 1000), "[]").0, 401);
    let r = it.handle(&request("GET", "/_quillo/service-channels", sign("svc", f, "c1", "", t), "", "127.0.0.1"));
    assert_eq!(r.status, 401);
    // What a service sends is for the app, whoever it names.
    assert_eq!(post(&mut it, "/_quillo/channel-relay", sign("", "_channel-relay", "c2", "{\"op\":\"p\",\"ch\":\"news\"}", t), "{\"op\":\"p\",\"ch\":\"news\"}"), (200, "[]".to_string()));
    let (status, body) = post(&mut it, "/_quillo/channel-relay", sign("svc", "_channel-relay", "c3", "{}", t), "{}");
    assert_eq!((status, body.as_str()), (401, "{\"error\":\"this is for another process\"}"));
    // A batch for a channel, signed for that channel.
    let line = "{\"k\":\"j\",\"p\":\"1\",\"s\":\"{}\",\"t\":null,\"v\":\"\"}\n";
    assert_eq!(post(&mut it, "/_quillo/service-channel/news", sign("svc", "_channel:news", "c4", line, t), line), (200, "[true]".to_string()));
    assert_eq!(post(&mut it, "/_quillo/service-channel/news", sign("svc", "_channels", "c4", line, t), line).0, 401);
}

// ---- passing routes on ---------------------------------------------------------------------

#[test]
fn a_prefix_passes_requests_on_but_not_quillos_own() {
    signing();
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let s2 = seen.clone();
    let url = fake(move |_, r, s| {
        s2.lock().unwrap().push(r.head.clone());
        let _ = s.write_all(b"HTTP/1.1 201 Created\r\nX-Theirs: 1\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    });
    let d = dir("pass-on");
    std::fs::write(d.join("passed.qwe"), WORK).unwrap();
    std::fs::write(d.join("nobody.qwe"), WORK).unwrap();
    let mut it = Interp::new(&d);
    it.shared.service_urls.lock().unwrap().insert("passed".into(), url);
    it.load_source("service /passed = \"passed.qwe\"\nroute GET / = \"up\"\n", "main.qwe").unwrap();
    for own in ["rpc/add", "service/add", "job/x", "tick/x", "later", "channel/x", "udp/1", "wt-ticket", "announce", "metrics"] {
        let r = it.handle(&request("POST", &format!("/passed/_quillo/{}", own), Vec::new(), "", "127.0.0.1"));
        assert_eq!(r.status, 404, "{}", own);
    }
    assert!(seen.lock().unwrap().is_empty());
    // Those that are the service's own to answer are passed on.
    assert_eq!(it.handle(&request("GET", "/passed/_quillo/client.js", Vec::new(), "", "127.0.0.1")).status, 201);
    seen.lock().unwrap().clear();
    let r = it.handle(&request("GET", "/passed/page?q=1", vec![("X-Mine".to_string(), "yes".to_string())], "", "10.0.0.9"));
    assert_eq!((r.status, text(&r).as_str()), (201, "ok"));
    assert!(r.headers.iter().any(|(k, v)| k == "x-theirs" && v == "1"), "{:?}", r.headers);
    let head = seen.lock().unwrap()[0].clone().to_ascii_lowercase();
    assert!(head.starts_with("get /page?q=1 http/1.1"), "{}", head);
    for h in ["x-mine: yes", "x-forwarded-for: 10.0.0.9", "x-forwarded-prefix: /passed", "x-forwarded-host: localhost"] {
        assert!(head.contains(h), "{}: {}", h, head);
    }
    // Just the prefix, with a query; and a body.
    let r = it.handle(&request("POST", "/passed?x=1", Vec::new(), "{\"a\":1}", "127.0.0.1"));
    assert_eq!(r.status, 201);
    assert!(seen.lock().unwrap()[1].starts_with("POST /?x=1 HTTP/1.1"), "{:?}", seen.lock().unwrap()[1]);

    // No one there.
    let mut it = Interp::new(&d);
    it.shared.service_urls.lock().unwrap().insert("nobody".into(), nobody());
    it.load_source("service /nobody = \"nobody.qwe\"\nroute GET / = \"up\"\n", "main.qwe").unwrap();
    let r = it.handle(&request("GET", "/nobody/page", Vec::new(), "", "127.0.0.1"));
    assert_eq!(r.status, 502);
    assert!(text(&r).starts_with("this part of the site is not answering ("), "{}", text(&r));
}

// ---- running programs ------------------------------------------------------------------------

/// A running `quillo`, its log in a file; killed when dropped.
struct Run {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Run {
    fn drop(&mut self) {
        // As Ctrl-C does: a server started with --split stops its services too.
        let _ = Command::new("kill").args(["-INT", &self.child.id().to_string()]).status();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Run {
    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn logged(&self, what: &str, secs: u64) {
        eventually(secs, &format!("{:?} is not in the log", what), || self.log().contains(what).then_some(()));
    }

    fn get(&self, path: &str) -> (u16, String) {
        let a = support::send(self.port, "GET", path, &[], "");
        (a.status, a.body)
    }

    /// Stops it with a signal, as an operator would.
    fn interrupt(&mut self) {
        let _ = Command::new("kill").args(["-INT", &self.child.id().to_string()]).status();
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "it did not stop");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn spawn(exe: &Path, file: &Path, port: u16, extra: &[&str], env: &[(&str, &str)]) -> Run {
    static N: AtomicUsize = AtomicUsize::new(0);
    let log = file.with_extension(format!("log{}", N.fetch_add(1, Ordering::SeqCst)));
    let child = Command::new(exe)
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(extra)
        .env_remove("QUILLO_SERVICE_SECRET")
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .expect("cannot start quillo");
    support::stop_with_tests(&child);
    Run { child, port, log }
}

fn run(file: &Path, port: u16, extra: &[&str], env: &[(&str, &str)]) -> Run {
    let mut r = spawn(Path::new(env!("CARGO_BIN_EXE_quillo")), file, port, extra, env);
    let deadline = Instant::now() + Duration::from_secs(25);
    while !support::listening(port) {
        if let Ok(Some(status)) = r.child.try_wait() {
            panic!("quillo stopped ({}):\n{}", status, r.log());
        }
        assert!(Instant::now() < deadline, "server did not start:\n{}", r.log());
        std::thread::sleep(Duration::from_millis(20));
    }
    answers(port);
    r
}

/// Waits for a whole answer: a server may drop a first connection as it starts.
fn answers(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut out = Vec::new();
        let asked = TcpStream::connect(("127.0.0.1", port)).and_then(|mut s| {
            s.write_all(b"GET /_quillo/ready HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
            s.read_to_end(&mut out)
        });
        if asked.is_ok() && out.starts_with(b"HTTP/1.1") {
            return;
        }
        assert!(Instant::now() < deadline, "the server does not answer");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn without_the_secret_a_service_elsewhere_is_not_called_or_answered() {
    let d = dir("no-secret");
    std::fs::write(d.join("work.qwe"), WORK).unwrap();
    std::fs::write(d.join("main.qwe"), "service work = \"work.qwe\"\nroute GET / = str(work.add(1).error)\n").unwrap();
    let r = run(&d.join("main.qwe"), support::free_port(), &["--service-url", "work=http://127.0.0.1:1"], &[]);
    assert_eq!(r.get("/").1, "service work: runs at http://127.0.0.1:1, but QUILLO_SERVICE_SECRET is not set to sign the call");
    assert_eq!(support::send(r.port, "POST", "/_quillo/service/add", &[], "[1]").status, 403);
    assert_eq!(support::send(r.port, "POST", "/_quillo/join", &[], "{}").status, 403);
    assert_eq!(support::send(r.port, "POST", "/_quillo/service-channels", &[], "[]").status, 403);
}

#[test]
fn a_call_for_another_service_is_refused_by_name() {
    let d = dir("other-name");
    std::fs::write(d.join("work.qwe"), WORK).unwrap();
    let r = run(&d.join("work.qwe"), support::free_port(), &["--as-service"], &[("QUILLO_SERVICE_SECRET", SECRET)]);
    let h = signed(now_secs(), "billing", "add", "wrongname", "", "[1]");
    let headers: Vec<(&str, &str)> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let a = support::send(r.port, "POST", "/_quillo/service/add", &headers, "[1]");
    assert_eq!((a.status, a.body.as_str()), (401, "{\"error\":\"this call is for service `billing`; this is `work` (see --service-name)\"}"));
    // Channels of a process named otherwise.
    let t = now_secs();
    let sig = service::signature(SECRET, t, "billing", "_channels", "", "c1", b"[]");
    let a = support::send(r.port, "POST", "/_quillo/service-channels", &[("X-Quillo-Service", &format!("t={},sig={}", t, sig)), ("X-Quillo-To", "billing"), ("X-Quillo-Call", "c1")], "[]");
    assert_eq!((a.status, a.body.as_str()), (401, "{\"error\":\"this is for another process\"}"));
}

#[test]
fn a_copy_joins_says_it_is_there_and_leaves_when_it_stops() {
    let d = dir("copy");
    std::fs::write(d.join("risk.qwe"), "server fn who() = env(\"WHO\", \"here\")\nroute GET /ok = \"ok\"\n").unwrap();
    std::fs::write(d.join("app.qwe"), "service risk = \"risk.qwe\" { retries: 1 }\nroute GET / = risk.who()\n").unwrap();
    let env = [("QUILLO_SERVICE_SECRET", SECRET)];
    let app_port = support::free_port();
    let mut app = run(&d.join("app.qwe"), app_port, &["--log", "text"], &env);
    let url = format!("http://127.0.0.1:{}", app_port);
    let copy_port = support::free_port();
    let mut copy = run(&d.join("risk.qwe"), copy_port, &["--as-service", "--join", &url], &[("QUILLO_SERVICE_SECRET", SECRET), ("WHO", "the copy")]);
    copy.logged(&format!("joined {} as a copy of `risk`", url), 10);
    app.logged(&format!("service risk: http://127.0.0.1:{} joined", copy_port), 5);
    let mut seen: Vec<String> = (0..6).map(|_| app.get("/").1).collect();
    seen.sort();
    seen.dedup();
    assert_eq!(seen, ["here", "the copy"]);

    copy.interrupt();
    assert!(copy.log().contains(&format!("left {} (`risk`)", url)), "{}", copy.log());
    app.logged(&format!("service risk: http://127.0.0.1:{} left", copy_port), 5);
    let seen: Vec<String> = (0..4).map(|_| app.get("/").1).collect();
    assert!(seen.iter().all(|s| s == "here"), "{:?}", seen);

    // A copy whose app goes away says so, once a beat fails; and then at leaving.
    let copy_port = support::free_port();
    let mut copy = run(&d.join("risk.qwe"), copy_port, &["--as-service", "--join", &url], &env);
    copy.logged("joined", 10);
    app.interrupt();
    copy.logged(&format!("cannot reach {} to stay a copy of `risk`", url), 12);
    copy.interrupt();
    assert!(copy.log().contains(&format!("cannot leave {}:", url)), "{}", copy.log());
}

#[test]
fn a_copy_that_cannot_join_says_why() {
    let d = dir("cannot-join");
    std::fs::write(d.join("risk.qwe"), "server fn who() = 1\nroute GET /ok = \"ok\"\n").unwrap();
    std::fs::write(d.join("app.qwe"), "service risk = \"risk.qwe\"\nroute GET / = \"up\"\n").unwrap();
    let app_port = support::free_port();
    let _app = run(&d.join("app.qwe"), app_port, &[], &[("QUILLO_SERVICE_SECRET", SECRET)]);
    let url = format!("http://127.0.0.1:{}", app_port);
    // Not the app's secret: it says what the app said.
    let wrong = run(&d.join("risk.qwe"), support::free_port(), &["--as-service", "--join", &url], &[("QUILLO_SERVICE_SECRET", "another")]);
    wrong.logged(&format!("cannot join {} as a copy of `risk`: 401", url), 10);
    // No secret at all.
    let none = run(&d.join("risk.qwe"), support::free_port(), &["--as-service", "--join", &url], &[]);
    none.logged("QUILLO_SERVICE_SECRET is not set", 10);
    // Named as no service of the app's.
    let other = run(&d.join("risk.qwe"), support::free_port(), &["--as-service", "--service-name", "other", "--join", &url], &[("QUILLO_SERVICE_SECRET", SECRET)]);
    other.logged("404 {\"error\":\"this program has no service `other`\"}", 10);
}

#[test]
fn calls_in_a_log_of_json_can_be_followed() {
    let d = dir("json-log");
    std::fs::write(d.join("work.qwe"), WORK).unwrap();
    std::fs::write(d.join("main.qwe"), "service work = \"work.qwe\" { retries: 0 }\nroute GET / = str(work.add(1))\nroute GET /x = str(is_err(work.add(1)))\n").unwrap();
    let pw = support::free_port();
    let w = run(&d.join("work.qwe"), pw, &["--as-service"], &[("QUILLO_SERVICE_SECRET", SECRET)]);
    let r = run(&d.join("main.qwe"), support::free_port(), &["--log", "json", "--service-url", &format!("work=http://127.0.0.1:{}", pw)], &[("QUILLO_SERVICE_SECRET", SECRET)]);
    assert_eq!(r.get("/").1, "2");
    drop(w);
    assert_eq!(r.get("/x").1, "true");
    r.logged("\"service\":\"work\",\"fn\":\"add\",\"url\":\"http://127.0.0.1:", 10);
    r.logged("\"status\":200", 10);
    r.logged("\"status\":0", 10);
    let line = r.log().lines().find(|l| l.contains("\"service\":\"work\"")).unwrap().to_string();
    let j: serde_json::Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{}: {}", e, line));
    assert_eq!((j["fn"].as_str(), j["tries"].as_u64()), (Some("add"), Some(1)));
    assert!(j["rid"].as_str().is_some_and(|r| !r.is_empty()), "{}", line);
}

#[test]
fn a_call_made_later_waits_in_memory_when_no_database_can_keep_it() {
    let d = dir("later-memory");
    std::fs::write(d.join("mailer.qwe"), "let sent = []\nserver fn send(to: str) {\n sent = sent + [to]\n if to == \"bad\" { return err(\"no such address\") }\n to\n}\nroute GET /sent = sent | join(\",\")\n").unwrap();
    std::fs::write(d.join("front.qwe"), "service mailer = \"mailer.qwe\"\nroute GET /send/:to = mailer.later.send(to)\n").unwrap();
    let pm = support::free_port();
    let url = format!("http://127.0.0.1:{}", pm);
    let nowhere = d.join("no-such-folder").join("system.db").display().to_string();
    let env = [("QUILLO_SERVICE_SECRET", SECRET), ("QUILLO_SERVICE_MAILER", url.as_str()), ("QUILLO_SYSTEM_DB", nowhere.as_str())];
    let f = run(&d.join("front.qwe"), support::free_port(), &[], &env);
    let (status, id) = f.get("/send/ana");
    assert!(status == 200 && id.len() == 24, "{} {}", status, id);
    f.get("/send/bad");
    f.logged("calls made later are kept in memory", 5);
    f.logged(&format!("mailer.send (made later, {}) not made yet: ", id), 10);
    let m = run(&d.join("mailer.qwe"), pm, &[], &[("QUILLO_SERVICE_SECRET", SECRET)]);
    let sent = eventually(15, "the calls were not made", || {
        let body = m.get("/sent").1;
        (body.split(',').count() == 2).then_some(body)
    });
    assert!(sent.contains("ana") && sent.contains("bad"), "{}", sent);
    f.logged("gave an error: \"no such address\"", 5);
}

#[test]
fn a_call_made_later_kept_in_a_database_is_tried_until_it_gets_through() {
    let d = dir("later-db");
    std::fs::write(d.join("mailer.qwe"), "let sent = []\nserver fn send(to: str) {\n sent = sent + [to]\n err(\"refused\")\n}\nroute GET /sent = sent | join(\",\")\n").unwrap();
    std::fs::write(d.join("front.qwe"), "db.open(\"front.db\")\nservice mailer = \"mailer.qwe\"\nroute GET /send/:to = mailer.later.send(to)\n").unwrap();
    let pm = support::free_port();
    let url = format!("http://127.0.0.1:{}", pm);
    let f = run(&d.join("front.qwe"), support::free_port(), &[], &[("QUILLO_SERVICE_SECRET", SECRET), ("QUILLO_SERVICE_MAILER", url.as_str())]);
    let id = f.get("/send/ana").1;
    f.logged(&format!("(made later, {}) not made yet: ", id), 10);
    let m = run(&d.join("mailer.qwe"), pm, &[], &[("QUILLO_SERVICE_SECRET", SECRET)]);
    eventually(15, "the call was not made", || (m.get("/sent").1 == "ana").then_some(()));
    f.logged("gave an error: \"refused\"", 5);
}

#[test]
fn two_servers_on_one_database_make_each_call_made_later_once() {
    let d = dir("later-two");
    // Slow enough that the second server's turn comes while the first works
    // through what it saw due.
    std::fs::write(d.join("mailer.qwe"), "let sent = []\nserver fn send(to: str) {\n sent = sent + [to]\n sleep(700)\n}\nroute GET /sent = sent | join(\",\")\n").unwrap();
    std::fs::write(d.join("front.qwe"), "db.open(\"front.db\")\nservice mailer = \"mailer.qwe\" { timeout: \"20s\" }\nroute GET /send/:to = mailer.later.send(to)\n").unwrap();
    let pm = support::free_port();
    let url = format!("http://127.0.0.1:{}", pm);
    let m = run(&d.join("mailer.qwe"), pm, &["--workers", "8"], &[("QUILLO_SERVICE_SECRET", SECRET)]);
    let env = [("QUILLO_SERVICE_SECRET", SECRET), ("QUILLO_SERVICE_MAILER", url.as_str())];
    let a = run(&d.join("front.qwe"), support::free_port(), &[], &env);
    for n in 0..6 {
        a.get(&format!("/send/n{}", n));
    }
    let _b = run(&d.join("front.qwe"), support::free_port(), &[], &env);
    let sent = eventually(30, "the calls were not all made", || {
        let body = m.get("/sent").1;
        (body.split(',').count() >= 6).then_some(body)
    });
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(m.get("/sent").1, sent, "no call was made twice");
    let mut names: Vec<&str> = sent.split(',').collect();
    names.sort();
    assert_eq!(names, ["n0", "n1", "n2", "n3", "n4", "n5"]);
}

#[test]
fn copies_on_one_database_answer_a_call_as_the_first_did() {
    let d = dir("claims");
    std::fs::write(d.join("work.qwe"), "db.open(\"work.db\")\ndb.exec(\"create table if not exists runs (n integer)\")\nserver fn count() {\n db.exec(\"insert into runs values (1)\")\n db.query(\"select count(*) as n from runs\")[0].n\n}\nserver fn slow() {\n sleep(800)\n \"slept\"\n}\n").unwrap();
    let env = [("QUILLO_SERVICE_SECRET", SECRET)];
    let (a, b) = (run(&d.join("work.qwe"), support::free_port(), &["--as-service"], &env), run(&d.join("work.qwe"), support::free_port(), &["--as-service"], &env));
    let post = |port: u16, f: &str, h: &[(String, String)]| {
        let headers: Vec<(&str, &str)> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let r = support::send(port, "POST", &format!("/_quillo/service/{}", f), &headers, "[]");
        (r.status, r.body)
    };
    // Done at one copy: the other gives its answer without running it.
    let h = signed(now_secs(), "work", "count", "claimdone", "", "[]");
    assert_eq!(post(a.port, "count", &h), (200, "{\"ok\":1}".to_string()));
    assert_eq!(post(b.port, "count", &h), (200, "{\"ok\":1}".to_string()));
    assert_eq!(post(b.port, "count", &h), (200, "{\"ok\":1}".to_string()));
    // Running at one copy: the other says so, and can be asked again.
    let h = signed(now_secs(), "work", "slow", "claimrunning", "", "[]");
    let both: Vec<_> = [a.port, b.port]
        .into_iter()
        .map(|port| {
            let h = h.clone();
            std::thread::spawn(move || {
                let headers: Vec<(&str, &str)> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                let r = support::send(port, "POST", "/_quillo/service/slow", &headers, "[]");
                (r.status, r.body)
            })
        })
        .collect();
    let mut got: Vec<(u16, String)> = both.into_iter().map(|t| t.join().unwrap()).collect();
    got.sort();
    assert_eq!(got, [(200, "{\"ok\":\"slept\"}".to_string()), (503, "{\"error\":\"this call is still running\"}".to_string())]);
    eventually(5, "the copy that said so did not give the answer", || (post(a.port, "slow", &h).0 == 200 && post(b.port, "slow", &h) == (200, "{\"ok\":\"slept\"}".to_string())).then_some(()));
}

const SPLIT_APP: &str = "service /work = \"work.qwe\"\nroute GET / = str(work.add(1))\n";

fn split_dir(name: &str) -> PathBuf {
    let d = dir(name);
    std::fs::write(d.join("work.qwe"), format!("{}route GET /ok = \"ok\"\n", WORK)).unwrap();
    std::fs::write(d.join("app.qwe"), SPLIT_APP).unwrap();
    d
}

#[test]
fn split_says_what_is_wrong_with_scale() {
    let d = split_dir("scale");
    let out = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("app.qwe")).args(["--split", "--scale", "nope=2", "--port", &support::free_port().to_string()]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--scale nope: no service `nope` (the app declares work)"), "{}", String::from_utf8_lossy(&out.stderr));
    std::fs::write(d.join("alone.qwe"), "route GET / = \"x\"\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("alone.qwe")).args(["--split", "--scale", "nope=2", "--port", &support::free_port().to_string()]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("(the app declares none)"), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn split_makes_a_secret_when_none_is_given_and_passes_on_how_it_was_run() {
    let d = split_dir("split-flags");
    let port = support::free_port();
    let r = run(&d.join("app.qwe"), port, &["--split", "--watch", "--no-check"], &[]);
    let body = eventually(20, "the service did not answer", || {
        let (status, body) = r.get("/");
        (status == 200 && body == "2").then_some(body)
    });
    assert_eq!(body, "2");
    assert_eq!(r.get("/work/ok").1, "ok");
}

#[test]
fn a_service_that_will_not_start_stops_split() {
    let d = dir("split-stuck");
    std::fs::write(d.join("work.qwe"), "sleep(40000)\nroute GET /ok = \"ok\"\n").unwrap();
    std::fs::write(d.join("app.qwe"), SPLIT_APP).unwrap();
    let port = support::free_port();
    let out = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("app.qwe")).args(["--split", "--no-check", "--port", &port.to_string()]).stdin(Stdio::null()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("service work did not start (see its lines above)"), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn a_service_that_cannot_be_started_again_is_tried_later() {
    let d = split_dir("split-gone");
    // A copy of the program that can be taken away while it runs.
    let exe = d.join("quillo-copy");
    std::fs::copy(env!("CARGO_BIN_EXE_quillo"), &exe).unwrap();
    let port = support::free_port();
    let r = spawn(&exe, &d.join("app.qwe"), port, &["--split"], &[]);
    eventually(25, "the app did not answer", || (support::listening(port) && r.get("/").1 == "2").then_some(()));
    let work = d.join("work.qwe").display().to_string();
    let pids = String::from_utf8(Command::new("pgrep").args(["-f", &work]).output().unwrap().stdout).unwrap();
    assert!(!pids.trim().is_empty());
    std::fs::remove_file(&exe).unwrap();
    for pid in pids.split_whitespace() {
        let _ = Command::new("kill").args(["-9", pid]).status();
    }
    r.logged("service work stopped", 5);
    r.logged("cannot start service work:", 5);
    // It tries again, later.
    std::fs::copy(env!("CARGO_BIN_EXE_quillo"), &exe).unwrap();
    r.logged("service work started again", 10);
}
