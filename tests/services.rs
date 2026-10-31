//! Services: a part of an app that runs in this process or as a server of
//! its own, called the same way either way (src/service.rs).

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BILLING: &str = r#"
let invoices = []

server fn invoice(customer: str, cents: int, note: str = "none") -> map ! str {
    if cents <= 0 { return err("An invoice is for something") }
    let inv = { id: len(invoices) + 1, customer, cents, note }
    invoices = invoices + [inv]
    inv
}

server fn count() -> int = len(invoices)

server fn whose() = tenant()

fn not_offered() = "only for billing itself"

route GET / = <p>{len(invoices)} invoices</p>
"#;

const SHOP: &str = r#"
service billing = "billing.qwe"

route GET /buy/:name {
    let inv = billing.invoice(name, 1200)
    is_err(inv) ? `refused: ${inv.error}` : `#${inv.id} ${inv.customer} ${inv.note}`
}
route GET /free = str(billing.invoice("x", 0).error)
route GET /count = str(billing.count())
route GET /as/:t = with_tenant(t, () => str(billing.whose()))
"#;

const APP: &str = r#"
service /shop = "shop.qwe"
service /billing = "billing.qwe"

route GET / = `billed ${billing.count()}`
"#;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-services-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for (f, src) in [("billing.qwe", BILLING), ("shop.qwe", SHOP), ("app.qwe", APP)] {
        std::fs::write(d.join(f), src).unwrap();
    }
    d
}

fn get(it: &mut Interp, url: &str) -> HttpResponse {
    it.handle(&HttpRequest {
        method: "GET".into(),
        url: url.into(),
        headers: vec![("Host".into(), "localhost".into())],
        body: Vec::new(),
        remote: "127.0.0.1".into(),
    })
}

fn post_json(it: &mut Interp, url: &str, body: &str) -> HttpResponse {
    it.handle(&HttpRequest {
        method: "POST".into(),
        url: url.into(),
        headers: vec![("Host".into(), "localhost".into()), ("Content-Type".into(), "application/json".into())],
        body: body.as_bytes().to_vec(),
        remote: "127.0.0.1".into(),
    })
}

fn text(r: &HttpResponse) -> String {
    String::from_utf8_lossy(&r.body).into_owned()
}

#[test]
fn in_one_process_a_service_is_one_and_called_directly() {
    let d = dir("one");
    let mut it = Interp::new(&d);
    it.load_file(&d.join("app.qwe")).unwrap();
    // The shop calls billing; what it leaves out, billing's own default fills.
    assert_eq!(text(&get(&mut it, "/shop/buy/ana")), "#1 ana none");
    assert_eq!(text(&get(&mut it, "/shop/buy/bea")), "#2 bea none");
    // An error from the service stays an error.
    assert_eq!(text(&get(&mut it, "/shop/free")), "An invoice is for something");
    // One billing for the whole process: the app, the shop and billing's own page agree.
    assert_eq!(text(&get(&mut it, "/")), "billed 2");
    assert_eq!(text(&get(&mut it, "/billing")), "<p>2 invoices</p>");
    assert_eq!(text(&get(&mut it, "/shop/count")), "2");
    // The tenant goes along.
    assert_eq!(text(&get(&mut it, "/shop/as/acme")), "acme");
}

#[test]
fn pages_cannot_call_a_services_server_fns() {
    let d = dir("no-rpc");
    let mut it = Interp::new(&d);
    it.load_file(&d.join("app.qwe")).unwrap();
    // The review's call: an invoice for 1 cent, past the shop's price.
    let r = post_json(&mut it, "/billing/_quillo/rpc/invoice", r#"["mallory",1,"free"]"#);
    assert_eq!(r.status, 404, "{}", text(&r));
    assert_eq!(text(&get(&mut it, "/billing")), "<p>0 invoices</p>");
    // Unsigned at its own address too.
    assert_eq!(post_json(&mut it, "/billing/_quillo/service/invoice", r#"["mallory",1]"#).status, 401);
    // The shop still calls it.
    assert_eq!(text(&get(&mut it, "/shop/buy/ana")), "#1 ana none");
    // A program's own server fns are still the pages' to call.
    std::fs::write(d.join("main.qwe"), "service /billing = \"billing.qwe\"\nserver fn hello() = \"hi\"\n").unwrap();
    let mut main = Interp::new(&d);
    main.load_file(&d.join("main.qwe")).unwrap();
    assert_eq!(text(&post_json(&mut main, "/_quillo/rpc/hello", "[]")), r#"{"ok":"hi"}"#);
}

#[test]
fn what_crosses_is_data() {
    let d = dir("data");
    std::fs::write(
        d.join("main.qwe"),
        r#"
service billing = "billing.qwe"
let f = (x) => x
route GET / = str(is_err(billing.invoice("ana", f)))
"#,
    )
    .unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    // A function cannot go to another service: it arrives as nothing, which `cents: int` refuses.
    assert_eq!(text(&get(&mut it, "/")), "true");
}

// ---- over the network ------------------------------------------------------------------------

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        // As Ctrl-C does: a server started with --split stops its services too.
        let _ = Command::new("kill").args(["-INT", &self.0.id().to_string()]).status();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.0.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn start(file: &Path, port: u16, extra: &[&str], env: &[(&str, &str)]) -> Server {
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(extra)
        .envs(env.iter().copied())
        .stderr(Stdio::null())
        .spawn()
        .expect("cannot start quillo");
    support::stop_with_tests(&child);
    let server = Server(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    server
}

fn http(port: u16, method: &str, path: &str, headers: &[(&str, String)], body: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut req = format!("{} {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n", method, path, body.len());
    for (k, v) in headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let body = out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
    (status, body)
}

/// The headers of a call to service `to`'s `f`, signed with `secret` at time `t`.
fn signed(secret: &str, t: u64, to: &str, f: &str, call: &str, body: &str) -> Vec<(&'static str, String)> {
    let sig = quillo::service::signature(secret, t, to, f, "", call, body.as_bytes());
    vec![
        ("Content-Type", "application/json".into()),
        ("X-Quillo-Service", format!("t={},sig={}", t, sig)),
        ("X-Quillo-To", to.into()),
        ("X-Quillo-Call", call.into()),
    ]
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

#[test]
fn split_a_service_is_called_over_http_signed() {
    let d = dir("http");
    let secret = "a-test-secret-for-services";
    std::env::set_var("QUILLO_SERVICE_SECRET", secret);
    let port = free_port();
    let _billing = start(&d.join("billing.qwe"), port, &["--base", "/billing"], &[("QUILLO_SERVICE_SECRET", secret)]);
    let url = format!("http://127.0.0.1:{}", port);
    // The shop, told where billing runs: the same code, its calls over HTTP.
    let mut it = Interp::new(&d);
    it.shared.service_urls.lock().unwrap().insert("billing".into(), url.clone());
    it.load_file(&d.join("shop.qwe")).unwrap();
    assert_eq!(text(&get(&mut it, "/buy/ana")), "#1 ana none");
    assert_eq!(text(&get(&mut it, "/free")), "An invoice is for something");
    assert_eq!(text(&get(&mut it, "/count")), "1");
    assert_eq!(text(&get(&mut it, "/as/acme")), "acme");
    // Unsigned, wrongly signed, or too old: refused. Only server fns are offered.
    let now = now_secs();
    let call = |f: &str, t: u64, key: &str| {
        let body = r#"["eve", 1]"#;
        http(port, "POST", &format!("/_quillo/service/{}", f), &signed(key, t, "billing", f, &quillo::service::new_id(16), body), body)
    };
    assert_eq!(http(port, "POST", "/_quillo/service/invoice", &[("Content-Type", "application/json".into())], r#"["eve", 1]"#).0, 401);
    assert_eq!(call("invoice", now, "another-secret").0, 401);
    assert_eq!(call("invoice", now - 3600, secret).0, 401);
    assert_eq!(call("not_offered", now, secret).0, 404);
    let (status, body) = call("invoice", now, secret);
    assert_eq!(status, 200);
    assert!(body.contains(r#""customer":"eve""#), "{}", body);
    // Not running: an error value, which a caller can handle.
    let mut gone = Interp::new(&d);
    gone.shared.service_urls.lock().unwrap().insert("billing".into(), format!("http://127.0.0.1:{}", free_port()));
    gone.load_file(&d.join("shop.qwe")).unwrap();
    assert!(text(&get(&mut gone, "/buy/ana")).starts_with("refused: service billing: unreachable"), "{}", text(&get(&mut gone, "/buy/ana")));
    // Its routes, under its prefix, are passed on to it (its links carry the prefix).
    let mut app = Interp::new(&d);
    app.shared.service_urls.lock().unwrap().insert("billing".into(), url);
    app.load_file(&d.join("app.qwe")).unwrap();
    assert_eq!(text(&get(&mut app, "/billing")), "<p>2 invoices</p>");
    assert_eq!(text(&get(&mut app, "/")), "billed 2");
    // But not calls of its functions, nor Quillo's other addresses.
    for path in ["/billing/_quillo/rpc/invoice", "/billing//_quillo/rpc/invoice", "/billing/_quillo/service/invoice", "/billing/_quillo/job/x", "/billing/_quillo/later"] {
        let r = post_json(&mut app, path, r#"["mallory",1,"free"]"#);
        assert_eq!(r.status, 404, "{}: {}", path, text(&r));
    }
    assert_eq!(text(&get(&mut app, "/billing")), "<p>2 invoices</p>");
}

#[test]
fn a_service_run_alone_answers_only_signed_calls() {
    let d = dir("alone");
    std::fs::write(d.join("ledger.qwe"), "server fn add(amounts: list<int>) -> int = sum(amounts)\n").unwrap();
    let secret = "a-test-secret-for-services";
    let port = free_port();
    let _ledger = start(&d.join("ledger.qwe"), port, &["--as-service"], &[("QUILLO_SERVICE_SECRET", secret)]);
    let json = ("Content-Type", "application/json".to_string());
    assert_eq!(http(port, "POST", "/_quillo/rpc/add", &[json.clone()], "[[1, 2]]").0, 404);
    // Signed calls are checked all the way in, as pages' calls are.
    let now = now_secs();
    let call = |body: &str| http(port, "POST", "/_quillo/service/add", &signed(secret, now, "ledger", "add", &quillo::service::new_id(16), body), body);
    assert_eq!(call(r#"[["1) or 1=1 --", 2]]"#).0, 400);
    assert_eq!(call("[[1, 2]]"), (200, r#"{"ok":3}"#.to_string()));
}

#[test]
fn a_signed_call_runs_once_and_only_at_the_service_it_names() {
    let d = dir("replay");
    std::fs::write(d.join("ledger.qwe"), "let runs = 0\nserver fn add(x: int) -> int {\n    runs += 1\n    runs\n}\nroute GET /runs = str(runs)\n").unwrap();
    std::fs::write(d.join("other.qwe"), "server fn add(x: int) -> int = 0\n").unwrap();
    let secret = "a-test-secret-for-services";
    let (port, named) = (free_port(), free_port());
    let _ledger = start(&d.join("ledger.qwe"), port, &[], &[("QUILLO_SERVICE_SECRET", secret)]);
    let _named = start(&d.join("other.qwe"), named, &["--service-name", "ledger"], &[("QUILLO_SERVICE_SECRET", secret)]);
    let now = now_secs();
    // What the review did: the same signed call sent again. It is answered
    // as it was the first time; the function does not run again.
    let headers = signed(secret, now, "ledger", "add", "abc123", "[1]");
    assert_eq!(http(port, "POST", "/_quillo/service/add", &headers, "[1]"), (200, r#"{"ok":1}"#.to_string()));
    assert_eq!(http(port, "POST", "/_quillo/service/add", &headers, "[1]"), (200, r#"{"ok":1}"#.to_string()));
    assert_eq!(http(port, "GET", "/runs", &[], "").1, "1");
    // The call id is signed: another id under the same signature is refused,
    // and so is a call with none.
    let mut other_id = headers.clone();
    other_id[3].1 = "abc124".into();
    assert_eq!(http(port, "POST", "/_quillo/service/add", &other_id, "[1]").0, 401);
    assert_eq!(http(port, "POST", "/_quillo/service/add", &headers[..3], "[1]").0, 401);
    // So is the service it is for: a call signed for another service is
    // refused, here by its file's name and there by --service-name.
    let for_billing = signed(secret, now, "billing", "add", "abc125", "[1]");
    assert_eq!(http(port, "POST", "/_quillo/service/add", &for_billing, "[1]").0, 401);
    let mut retargeted = headers.clone();
    retargeted[2].1 = "billing".into();
    assert_eq!(http(port, "POST", "/_quillo/service/add", &retargeted, "[1]").0, 401);
    assert_eq!(http(named, "POST", "/_quillo/service/add", &signed(secret, now, "ledger", "add", "abc126", "[1]"), "[1]").0, 200);
    assert_eq!(http(named, "POST", "/_quillo/service/add", &signed(secret, now, "other", "add", "abc127", "[1]"), "[1]").0, 401);
    assert_eq!(http(port, "GET", "/runs", &[], "").1, "1");
}

#[test]
fn split_runs_each_service_as_a_process_of_its_own() {
    let d = dir("split");
    let port = free_port();
    let app = start(&d.join("app.qwe"), port, &["--split"], &[]);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let (status, body) = http(port, "GET", "/shop/buy/ana", &[], "");
        if status == 200 {
            assert_eq!(body, "#1 ana none");
            break;
        }
        assert!(Instant::now() < deadline, "the shop did not answer: {} {}", status, body);
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(http(port, "GET", "/billing", &[], "").1, "<p>1 invoices</p>");
    assert_eq!(http(port, "GET", "/", &[], "").1, "billed 1");
    // Pages cannot call billing's functions, through the app or otherwise.
    let json = ("Content-Type", "application/json".to_string());
    assert_eq!(http(port, "POST", "/billing/_quillo/rpc/invoice", &[json], r#"["mallory",1,"free"]"#).0, 404);
    assert_eq!(http(port, "GET", "/billing", &[], "").1, "<p>1 invoices</p>");
    // Stopped with the app: no service left behind.
    drop(app);
    std::thread::sleep(Duration::from_millis(500));
    let left = Command::new("pgrep").args(["-f", &d.join("billing.qwe").display().to_string()]).output().unwrap();
    assert!(left.stdout.is_empty(), "a service was left running");
}

#[test]
fn services_may_use_one_another() {
    let d = dir("mutual");
    std::fs::write(d.join("a.qwe"), "service b = \"b.qwe\"\nserver fn ping() = `ping ${b.pong()}`\nserver fn name() = \"a\"\n").unwrap();
    std::fs::write(d.join("b.qwe"), "service a = \"a.qwe\"\nserver fn pong() = `pong from b, which knows ${a.name()}`\n").unwrap();
    std::fs::write(d.join("main.qwe"), "service a = \"a.qwe\"\nroute GET / = a.ping()\n").unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    assert_eq!(text(&get(&mut it, "/")), "ping pong from b, which knows a");
}

const PRICES: &str = r#"
server fn total(price: dec, qty: int) -> dec = price * qty
"#;

const TILL: &str = r#"
service prices = "prices.qwe"

route GET /total {
    let t = prices.total(12.50m, 3)
    `${type_of(t)} ${t}`
}
route GET /sum {
    let t = prices.total(0.10m, 3) + prices.total(0.20m, 1)
    `${type_of(t)} ${t}`
}
"#;

#[test]
fn a_decimal_crosses_to_a_service_and_back_exactly() {
    let d = dir("dec");
    std::fs::write(d.join("prices.qwe"), PRICES).unwrap();
    std::fs::write(d.join("till.qwe"), TILL).unwrap();
    // In one process: a decimal, scale and all, both ways (not a float, not text).
    let mut it = Interp::new(&d);
    it.load_file(&d.join("till.qwe")).unwrap();
    assert_eq!(text(&get(&mut it, "/total")), "dec 37.50");
    assert_eq!(text(&get(&mut it, "/sum")), "dec 0.50");
    // Over HTTP: tagged as for a page's server fns; text and ints for a `dec` parameter too.
    // The same secret as the other tests: the variable is the whole process's.
    let secret = "a-test-secret-for-services";
    std::env::set_var("QUILLO_SERVICE_SECRET", secret);
    let port = free_port();
    let _prices = start(&d.join("prices.qwe"), port, &[], &[("QUILLO_SERVICE_SECRET", secret)]);
    let mut it = Interp::new(&d);
    it.shared.service_urls.lock().unwrap().insert("prices".into(), format!("http://127.0.0.1:{}", port));
    it.load_file(&d.join("till.qwe")).unwrap();
    assert_eq!(text(&get(&mut it, "/total")), "dec 37.50");
    assert_eq!(text(&get(&mut it, "/sum")), "dec 0.50");
    let now = now_secs();
    for (i, (body, want)) in [(r#"[{"$dec": "0.10"}, 3]"#, r#"{"ok":{"$dec":"0.30"}}"#), (r#"["0.10", 3]"#, r#"{"ok":{"$dec":"0.30"}}"#), (r#"[2, 3]"#, r#"{"ok":{"$dec":"6"}}"#)].into_iter().enumerate() {
        let (status, got) = http(port, "POST", "/_quillo/service/total", &signed(secret, now, "prices", "total", &format!("dec{}", i), body), body);
        assert_eq!((status, got.as_str()), (200, want), "{}", body);
    }
    let body = r#"["ten", 3]"#;
    assert_eq!(http(port, "POST", "/_quillo/service/total", &signed(secret, now, "prices", "total", "dec9", body), body).0, 400);
}

// ---- holding up: replicas, retries, request ids, calls made later ----------------------------

/// A raw GET, with its headers: (status, headers in lower case, body).
fn get_raw(port: u16, path: &str, headers: &[(&str, &str)]) -> (u16, String, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut req = format!("GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n", path);
    for (k, v) in headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let (head, body) = out.split_once("\r\n\r\n").map(|(h, b)| (h.to_ascii_lowercase(), b.to_string())).unwrap_or_default();
    (status, head, body)
}

/// Waits until `f` gives Some, or fails after `secs`.
fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(100));
    }
}

const LEDGER: &str = r#"
let entries = []
server fn add(x: int) -> int {
    entries = entries + [x]
    len(entries)
}
route GET /count = str(len(entries))
"#;

#[test]
fn calls_go_to_each_address_in_turn_and_past_one_that_is_down() {
    let d = dir("replicas");
    std::fs::write(d.join("ledger.qwe"), LEDGER).unwrap();
    std::fs::write(d.join("front.qwe"), "service ledger = \"ledger.qwe\" { failures: 1, cooldown: \"60s\" }\nroute GET /add/:x = str(ledger.add(int(x)))\n").unwrap();
    let secret = "a-test-secret-for-services";
    std::env::set_var("QUILLO_SERVICE_SECRET", secret);
    let (a, b) = (free_port(), free_port());
    let _a = start(&d.join("ledger.qwe"), a, &[], &[("QUILLO_SERVICE_SECRET", secret)]);
    let _b = start(&d.join("ledger.qwe"), b, &[], &[("QUILLO_SERVICE_SECRET", secret)]);
    let dead = free_port();
    let mut it = Interp::new(&d);
    it.shared.service_urls.lock().unwrap().insert("ledger".into(), format!("http://127.0.0.1:{},http://127.0.0.1:{},http://127.0.0.1:{}", dead, a, b));
    it.load_file(&d.join("front.qwe")).unwrap();
    let start_at = Instant::now();
    for i in 0..9 {
        let got = text(&get(&mut it, &format!("/add/{}", i)));
        assert!(got.parse::<i64>().is_ok(), "call {}: {}", i, got);
    }
    // The address that is down was tried once, then left alone: no waiting on it.
    assert!(start_at.elapsed() < Duration::from_secs(3), "{:?}", start_at.elapsed());
    let (na, nb): (i64, i64) = (http(a, "GET", "/count", &[], "").1.parse().unwrap(), http(b, "GET", "/count", &[], "").1.parse().unwrap());
    assert_eq!(na + nb, 9);
    assert!(na > 0 && nb > 0, "both addresses take calls: {} and {}", na, nb);
}

#[test]
fn a_call_with_no_answer_in_time_is_sent_again_and_runs_once() {
    let d = dir("retry");
    std::fs::write(
        d.join("slowpoke.qwe"),
        "let runs = 0\nserver fn slow(x: int) -> int {\n    runs += 1\n    sleep(600)\n    x * 2\n}\nroute GET /runs = str(runs)\n",
    )
    .unwrap();
    std::fs::write(d.join("front.qwe"), "service slowpoke = \"slowpoke.qwe\" { timeout: \"250ms\", retries: 5 }\nroute GET /slow = str(slowpoke.slow(21))\n").unwrap();
    let secret = "a-test-secret-for-services";
    std::env::set_var("QUILLO_SERVICE_SECRET", secret);
    let port = free_port();
    let _s = start(&d.join("slowpoke.qwe"), port, &[], &[("QUILLO_SERVICE_SECRET", secret)]);
    let mut it = Interp::new(&d);
    it.shared.service_urls.lock().unwrap().insert("slowpoke".into(), format!("http://127.0.0.1:{}", port));
    it.load_file(&d.join("front.qwe")).unwrap();
    // The first try gives up at 250ms; the next ones find the call running,
    // then its answer, kept by the service: the function ran once.
    assert_eq!(text(&get(&mut it, "/slow")), "42");
    assert_eq!(http(port, "GET", "/runs", &[], "").1, "1");
}

#[test]
fn a_requests_id_goes_with_the_calls_it_makes() {
    let d = dir("rid");
    std::fs::write(d.join("echo.qwe"), "server fn id() = request.id\n").unwrap();
    std::fs::write(d.join("front.qwe"), "service echo = \"echo.qwe\"\nroute GET /rid = `${request.id} ${echo.id()}`\n").unwrap();
    let secret = "a-test-secret-for-services";
    let (pe, pf) = (free_port(), free_port());
    let _e = start(&d.join("echo.qwe"), pe, &[], &[("QUILLO_SERVICE_SECRET", secret)]);
    let url = format!("http://127.0.0.1:{}", pe);
    let _f = start(&d.join("front.qwe"), pf, &[], &[("QUILLO_SERVICE_SECRET", secret), ("QUILLO_SERVICE_ECHO", &url)]);
    // Given: the same id here, at the service, and in the answer.
    let (status, head, body) = get_raw(pf, "/rid", &[("X-Request-Id", "trace-42")]);
    assert_eq!((status, body.as_str()), (200, "trace-42 trace-42"));
    assert!(head.contains("x-request-id: trace-42"), "{}", head);
    // Not given (or not a plain one): one is made, and passed on all the same.
    let (_, head, body) = get_raw(pf, "/rid", &[("X-Request-Id", "no spaces <here>")]);
    let (here, there) = body.split_once(' ').unwrap();
    assert!(here.len() == 16 && here == there, "{}", body);
    assert!(head.contains(&format!("x-request-id: {}", here)), "{}", head);
}

#[test]
fn calls_made_later_are_made_once_the_service_answers() {
    for with_db in [false, true] {
        let d = dir(if with_db { "later-db" } else { "later" });
        std::fs::write(d.join("mailer.qwe"), "let sent = []\nserver fn send(to: str) -> int {\n    sent = sent + [to]\n    len(sent)\n}\nroute GET /sent = sent | join(\",\")\n").unwrap();
        let db = if with_db { "db.open(\"front.db\")\n" } else { "" };
        std::fs::write(d.join("front.qwe"), format!("{}service mailer = \"mailer.qwe\"\nroute GET /send/:to = mailer.later.send(to)\n", db)).unwrap();
        let secret = "a-test-secret-for-services";
        let (pm, pf) = (free_port(), free_port());
        let url = format!("http://127.0.0.1:{}", pm);
        let _f = start(&d.join("front.qwe"), pf, &[], &[("QUILLO_SERVICE_SECRET", secret), ("QUILLO_SERVICE_MAILER", &url)]);
        // The mailer is not running yet: the calls wait, and their ids come back now.
        let (status, id) = http(pf, "GET", "/send/ana", &[], "");
        assert_eq!(status, 200);
        assert!(id.len() == 24 && id.bytes().all(|b| b.is_ascii_hexdigit()), "{}", id);
        http(pf, "GET", "/send/bo", &[], "");
        std::thread::sleep(Duration::from_millis(1500));
        let _m = start(&d.join("mailer.qwe"), pm, &[], &[("QUILLO_SERVICE_SECRET", secret)]);
        let sent = eventually(15, "the calls made later were not made", || {
            let (_, body) = http(pm, "GET", "/sent", &[], "");
            (body.split(',').count() == 2).then_some(body)
        });
        let mut names: Vec<&str> = sent.split(',').collect();
        names.sort();
        assert_eq!(names, ["ana", "bo"]);
        // Made once each, though tried until they got through.
        std::thread::sleep(Duration::from_millis(2500));
        assert_eq!(http(pm, "GET", "/sent", &[], "").1.split(',').count(), 2);
    }
}

#[test]
fn how_a_service_is_called_is_set_where_it_is_declared() {
    let d = dir("options");
    for (opts, want) in [
        ("{ retries: 99 }", "`retries` is a number of tries after the first, 0 to 10"),
        ("{ timeout: \"soon\" }", "`timeout` is a duration"),
        ("{ speed: 2 }", "no option `speed` (timeout, retries, failures, cooldown)"),
        ("[1]", "expected end of statement"),
    ] {
        std::fs::write(d.join("main.qwe"), format!("service billing = \"billing.qwe\" {}\n", opts)).unwrap();
        let mut it = Interp::new(&d);
        let e = it.load_file(&d.join("main.qwe")).err().unwrap_or_else(|| panic!("{} loaded", opts));
        assert!(e.msg.contains(want), "{}: {}", opts, e.msg);
    }
    std::fs::write(d.join("main.qwe"), "service billing = \"billing.qwe\" { timeout: \"1.5s\", retries: 0, failures: 3, cooldown: 2000 }\n").unwrap();
    Interp::new(&d).load_file(&d.join("main.qwe")).unwrap();
    let o = quillo::service::opts("billing");
    assert_eq!((o.timeout, o.retries, o.failures, o.cooldown), (Duration::from_millis(1500), 0, 3, Duration::from_secs(2)));
}

// ---- reloading (--reload) ------------------------------------------------------------------

#[test]
fn a_request_running_when_the_program_reloads_finishes_on_its_version() {
    let d = dir("reload");
    let program = |v: &str| format!("service billing = \"version.qwe\"\nlet V = \"{}\"\nroute GET /slow {{\n    sleep(1500)\n    `${{V}}/${{billing.version()}}`\n}}\nroute GET /now = `${{V}}/${{billing.version()}}`\n", v);
    std::fs::write(d.join("version.qwe"), "server fn version() = \"one\"\n").unwrap();
    std::fs::write(d.join("main.qwe"), program("one")).unwrap();
    let port = free_port();
    let _s = start(&d.join("main.qwe"), port, &["--reload"], &[]);
    assert_eq!(http(port, "GET", "/now", &[], "").1, "one/one");
    let slow = std::thread::spawn(move || http(port, "GET", "/slow", &[], "").1);
    std::thread::sleep(Duration::from_millis(400));
    std::fs::write(d.join("version.qwe"), "server fn version() = \"two\"\n").unwrap();
    std::fs::write(d.join("main.qwe"), program("two")).unwrap();
    eventually(10, "the program was not loaded again", || (http(port, "GET", "/now", &[], "").1 == "two/two").then_some(()));
    // Its code, its variables and its services were the old version's to the end.
    assert_eq!(slow.join().unwrap(), "one/one");
}

const MAILER: &str = "let sent = []\nserver fn send(to: str) -> int {\n    sent = sent + [to]\n    len(sent)\n}\nroute GET /sent = sent | join(\",\")\n";

#[test]
fn calls_made_later_outlive_a_restart_without_a_database() {
    let d = dir("later-restart");
    std::fs::write(d.join("mailer.qwe"), MAILER).unwrap();
    std::fs::write(d.join("front.qwe"), "service mailer = \"mailer.qwe\"\nroute GET /send/:to = mailer.later.send(to)\n").unwrap();
    let secret = "a-test-secret-for-services";
    let (pm, pf) = (free_port(), free_port());
    let url = format!("http://127.0.0.1:{}", pm);
    let env = [("QUILLO_SERVICE_SECRET", secret), ("QUILLO_SERVICE_MAILER", url.as_str())];
    let front = start(&d.join("front.qwe"), pf, &[], &env);
    assert_eq!(http(pf, "GET", "/send/ana", &[], "").1.len(), 24);
    drop(front);
    // Kept in a file next to the program, made again when it starts again.
    assert!(d.join("quillo-system.db").exists());
    let _front = start(&d.join("front.qwe"), pf, &[], &env);
    let _m = start(&d.join("mailer.qwe"), pm, &[], &[("QUILLO_SERVICE_SECRET", secret)]);
    eventually(15, "the call made later was lost", || (http(pm, "GET", "/sent", &[], "").1 == "ana").then_some(()));
}

#[test]
fn a_version_that_first_calls_services_has_its_calls_made_later_made() {
    let d = dir("reload-later");
    std::fs::write(d.join("mailer.qwe"), MAILER).unwrap();
    std::fs::write(d.join("front.qwe"), "route GET / = \"no services yet\"\n").unwrap();
    let secret = "a-test-secret-for-services";
    let (pm, pf) = (free_port(), free_port());
    let url = format!("http://127.0.0.1:{}", pm);
    let _front = start(&d.join("front.qwe"), pf, &["--reload"], &[("QUILLO_SERVICE_SECRET", secret), ("QUILLO_SERVICE_MAILER", url.as_str())]);
    std::thread::sleep(Duration::from_millis(300));
    std::fs::write(d.join("front.qwe"), "service mailer = \"mailer.qwe\"\nroute GET /send/:to = mailer.later.send(to)\n").unwrap();
    eventually(10, "the program was not loaded again", || (http(pf, "GET", "/send/bo", &[], "").1.len() == 24).then_some(()));
    let _m = start(&d.join("mailer.qwe"), pm, &[], &[("QUILLO_SERVICE_SECRET", secret)]);
    eventually(15, "the call made later was not made", || (http(pm, "GET", "/sent", &[], "").1 == "bo").then_some(()));
}

/// A topic a service publishes while answering goes back with the answer,
/// for the caller to announce to its own pages: split, a service's
/// `publish` reached no page before (FEEDBACK4.md, Q-038; branch bank-fixes).
/// Its own tables and variables stay its own.
#[test]
fn what_a_service_publishes_goes_back_with_its_answer() {
    let d = dir("published");
    std::fs::write(
        d.join("accounts.qwe"),
        "let n = 0\nserver fn spend(id: int) -> int {\n    n = n + 1\n    db.exec(\"create table if not exists t (x)\")\n    db.exec(\"insert into t (x) values (?)\", id)\n    db.transaction(() => publish(`account:${id}`))\n    publish(\"accounts\")\n    n\n}\nserver fn quiet() -> int = 1\nserver fn undone(id: int) {\n    db.transaction(() => {\n        publish(`account:${id}`)\n        err(\"rolled back\")\n    })\n}\n",
    )
    .unwrap();
    let secret = "a-test-secret-for-services";
    let port = free_port();
    let _accounts = start(&d.join("accounts.qwe"), port, &["--as-service"], &[("QUILLO_SERVICE_SECRET", secret)]);
    let call = |f: &str, body: &str| http(port, "POST", &format!("/_quillo/service/{}", f), &signed(secret, now_secs(), "accounts", f, &quillo::service::new_id(16), body), body);
    assert_eq!(call("spend", "[7]"), (200, r#"{"ok":1,"published":["account:7","accounts"]}"#.to_string()));
    // Nothing published: the answer is as it always was.
    assert_eq!(call("quiet", "[]"), (200, r#"{"ok":1}"#.to_string()));
    // Published in a transaction that rolled back: not sent.
    assert_eq!(call("undone", "[7]"), (200, r#"{"err":"rolled back","own":true}"#.to_string()));
}

/// A file the app and one of its services both `use` runs in each, whichever
/// line comes first: with the service first, the app lacked the file's
/// functions (FEEDBACK4.md, Q-040; branch bank-fixes).
#[test]
fn a_file_the_app_and_a_service_both_use_runs_in_each() {
    let d = dir("shared-use");
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::create_dir_all(d.join("svc")).unwrap();
    std::fs::write(d.join("lib/text.qwe"), "fn shout(s: str) -> str = upper(s) + \"!\"\n").unwrap();
    std::fs::write(d.join("svc/greeter.qwe"), "use \"../lib/text.qwe\"\nserver fn hello() -> str = shout(\"from the service\")\n").unwrap();
    std::fs::write(
        d.join("app.qwe"),
        "service greeter = \"svc/greeter.qwe\"\nuse \"lib/text.qwe\"\ncomponent Hi() {\n    state n = 0\n    <button @click={() => { n += 1 }}>{shout(\"page\")}</button>\n}\nroute GET / = [shout(\"app\"), greeter.hello() ?? \"no answer\"]\nroute GET /page = <html><body><Hi/></body></html>\n",
    )
    .unwrap();
    let port = free_port();
    let _app = start(&d.join("app.qwe"), port, &[], &[]);
    assert_eq!(http(port, "GET", "/", &[], ""), (200, r#"["APP!","FROM THE SERVICE!"]"#.to_string()));
    assert!(http(port, "GET", "/page", &[], "").1.contains("PAGE!"));
}

/// `--split` starts a service again when its process ends while the app
/// runs (FEEDBACK4.md, Q-042; branch bank-fixes).
#[test]
fn split_starts_a_service_again_when_it_ends() {
    let d = dir("split-again");
    let port = free_port();
    let _app = start(&d.join("app.qwe"), port, &["--split"], &[]);
    eventually(20, "the shop did not answer", || (http(port, "GET", "/shop/buy/ana", &[], "").0 == 200).then_some(()));
    let billing = d.join("billing.qwe").display().to_string();
    let pids = String::from_utf8(Command::new("pgrep").args(["-f", &billing]).output().unwrap().stdout).unwrap();
    assert!(!pids.trim().is_empty(), "billing is not running");
    for pid in pids.split_whitespace() {
        let _ = Command::new("kill").args(["-9", pid]).status();
    }
    // Back, and holding what a new process holds: no invoices yet.
    let body = eventually(30, "billing was not started again", || {
        let (status, body) = http(port, "GET", "/shop/buy/bo", &[], "");
        (status == 200 && body.starts_with('#')).then_some(body)
    });
    assert_eq!(body, "#1 bo none");
}

/// The scheduler's own call each second (asking that calls made `later` be
/// sent) is not in the request log (FEEDBACK4.md, Q-043; branch bank-fixes).
#[test]
fn the_schedulers_own_calls_are_not_logged() {
    let d = dir("quiet-log");
    let port = free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", &d.join("shop.qwe").display().to_string(), "--port", &port.to_string()])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    eventually(20, "the shop did not start", || support::listening(port).then_some(()));
    assert_eq!(http(port, "GET", "/free", &[], "").0, 200);
    std::thread::sleep(Duration::from_millis(2500));
    let _ = child.kill();
    let mut log = String::new();
    child.stderr.take().unwrap().read_to_string(&mut log).unwrap();
    assert!(log.contains("GET /free -> 200"), "{}", log);
    assert!(!log.contains("/_quillo/later"), "{}", log);
}
