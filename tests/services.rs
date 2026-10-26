//! Services: a part of an app that runs in this process or as a server of
//! its own, called the same way either way (src/service.rs).

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
    let deadline = Instant::now() + Duration::from_secs(20);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    Server(child)
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

fn sign(secret: &str, t: u64, f: &str, tenant: &str, body: &str) -> String {
    let msg = format!("{}\n{}\n{}\n{}", t, f, tenant, body);
    quillo::crypto::hex(&quillo::crypto::hmac_sha256(secret.as_bytes(), msg.as_bytes()))
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
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let call = |f: &str, t: u64, key: &str| {
        let body = r#"["eve", 1]"#;
        http(port, "POST", &format!("/_quillo/service/{}", f), &[("Content-Type", "application/json".into()), ("X-Quillo-Service", format!("t={},sig={}", t, sign(key, t, f, "", body)))], body)
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
