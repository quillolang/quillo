//! Where data goes wrong at the edges: `idempotent`'s table in a state no
//! running server leaves it in, a call to a service seen as a request with
//! nothing but an id, a client or a server leaving part way through an answer,
//! a slow reader of a large file, and values PostgreSQL sends that no column
//! holds. Tests with databases are skipped when the server is not installed.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::value::Value;
use quillo::Interp;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Once;
use std::time::{Duration, Instant};
use support::{free_port, send, serve, temp_dir, Guard};

fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn eval(it: &mut Interp, e: &str) -> String {
    it.load_source(&format!("fn __t() {{\n{}\n}}\n", e), "case.qwe").unwrap_or_else(|x| panic!("{}\n{}", x, e));
    match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    }
}

fn get(it: &mut Interp, url: &str, headers: &[(&str, &str)]) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "x".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers: h, body: vec![], remote: "127.0.0.1".into() })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// A folder with `app.qwe` holding `src`, and its path.
fn app(name: &str, src: &str) -> (PathBuf, PathBuf) {
    let d = temp_dir(&format!("cover-data-{}", name));
    let f = d.join("app.qwe");
    std::fs::write(&f, src).unwrap();
    (d, f)
}

/// A `quillo run` server whose process the test can signal, its log in a file.
fn run_logged(file: &Path, extra: &[&str], env: &[(&str, &str)], log: &Path) -> (Guard, u16) {
    let port = free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(extra)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(log).unwrap())
        .spawn()
        .expect("cannot start quillo");
    let guard = Guard::new(child);
    eventually(20, "the server did not start", || support::listening(port).then_some(()));
    (guard, port)
}

fn interrupt(server: &Guard) {
    let _ = Command::new("kill").args(["-INT", &server.0.id().to_string()]).status();
}

fn wait_for_file(path: &Path) {
    eventually(15, "the handler did not get there", || path.exists().then_some(()));
}

/// Asks for `path` and gives the open connection.
fn ask(port: u16, path: &str) -> TcpStream {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    write!(s, "GET {} HTTP/1.1\r\nHost: localhost\r\n\r\n", path).unwrap();
    s
}

fn read_all(s: &mut TcpStream) -> String {
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

// ---- idempotent: its table changed under it ------------------------------------------------

/// A program whose `POST /pay` is idempotent, and whose `POST /spoil` runs `sql`.
fn idempotent_app(name: &str, sql: &str) -> (PathBuf, PathBuf) {
    app(
        name,
        &format!(
            r#"
db.open("app.db")
route POST /pay = idempotent(() => respond(201, "paid"))
route POST /spoil = db.exec("{}")
route GET /flag = str(db.one("select body_b64 as b from _quillo_idempotency").b)
"#,
            sql
        ),
    )
}

#[track_caller]
fn pay_then_spoil(port: u16) {
    let first = send(port, "POST", "/pay", &[("Idempotency-Key", "k1")], "");
    assert_eq!((first.status, first.body.as_str()), (201, "paid"));
    assert_eq!(first.header("idempotent-replayed"), None);
    assert_eq!(send(port, "POST", "/spoil", &[], "").status, 200);
}

#[test]
fn a_key_whose_table_lost_a_column_is_refused_as_unrecorded() {
    let (_d, f) = idempotent_app("idem-column", "alter table _quillo_idempotency drop column body_b64");
    let s = serve(&f, &[]);
    pay_then_spoil(s.port);
    let again = send(s.port, "POST", "/pay", &[("Idempotency-Key", "k1")], "");
    assert_eq!(again.status, 503, "{}", again.body);
    assert!(again.body.contains("could not record the Idempotency-Key") && again.body.contains("body_b64"), "{}", again.body);
}

#[test]
fn an_answer_with_a_null_where_its_encoding_was_is_given_as_text() {
    let (_d, f) = idempotent_app("idem-null", "update _quillo_idempotency set body_b64 = null");
    let s = serve(&f, &[]);
    pay_then_spoil(s.port);
    let again = send(s.port, "POST", "/pay", &[("Idempotency-Key", "k1")], "");
    assert_eq!((again.status, again.body.as_str()), (201, "paid"));
    assert_eq!(again.header("idempotent-replayed"), Some("true"));
}

#[test]
fn a_status_kept_as_text_is_read_as_a_number() {
    let sql = "alter table _quillo_idempotency drop column status; alter table _quillo_idempotency add column status text; update _quillo_idempotency set status = '207'";
    let (_d, f) = idempotent_app("idem-text", sql);
    let s = serve(&f, &[]);
    pay_then_spoil(s.port);
    let again = send(s.port, "POST", "/pay", &[("Idempotency-Key", "k1")], "");
    assert_eq!((again.status, again.body.as_str()), (207, "paid"));
    assert_eq!(again.header("idempotent-replayed"), Some("true"));
}

#[test]
fn a_flag_kept_as_a_boolean_is_read_as_one() {
    // `aGVsbG8` is "hello": the flag, read as 1, says the body is encoded.
    let sql = "alter table _quillo_idempotency drop column body_b64; alter table _quillo_idempotency add column body_b64 boolean; \
               update _quillo_idempotency set body_b64 = 1, body = 'aGVsbG8'";
    let (_d, f) = idempotent_app("idem-bool", sql);
    let s = serve(&f, &[]);
    pay_then_spoil(s.port);
    let again = send(s.port, "POST", "/pay", &[("Idempotency-Key", "k1")], "");
    assert_eq!(send(s.port, "GET", "/flag", &[], "").body, "true", "a boolean, as this program reads it");
    assert_eq!((again.status, again.body.as_str()), (201, "hello"));
}

#[test]
fn a_claim_taken_and_a_table_that_can_no_longer_be_read_is_a_503() {
    // Another connection takes the claim and changes the table while the
    // function runs: its answer is not kept, and the first request's cannot
    // be read to give it instead.
    let (_d, f) = app(
        "idem-lost",
        r#"
db.open("app.db")
let other = db.connect("app.db")
fn pay() {
    other.exec("update _quillo_idempotency set token = 'stolen'")
    other.exec("alter table _quillo_idempotency drop column path")
    "paid"
}
route POST /pay = idempotent(() => pay())
"#,
    );
    let s = serve(&f, &[]);
    let r = send(s.port, "POST", "/pay", &[("Idempotency-Key", "k1")], "");
    assert_eq!(r.status, 503, "{}", r.body);
    assert!(r.body.contains("could not read the Idempotency-Key") && r.body.contains("path"), "{}", r.body);
}

#[test]
fn a_session_that_is_no_map_has_no_user() {
    let (_d, f) = app(
        "idem-session",
        r#"
route POST /pay = idempotent(() => `session ${session()}`)
"#,
    );
    let s = serve(&f, &[]);
    // A cookie signed with the server's secret, around something no
    // `set_session` of this version writes.
    let key = quillo::crypto::hmac_sha256(b"test secret for the tests", b"quillo session");
    let payload = quillo::crypto::b64url(format!("{{\"e\":{},\"d\":7}}", now_secs() + 1000).as_bytes());
    let mut msg = b"\0\0".to_vec();
    msg.extend_from_slice(payload.as_bytes());
    let token = format!("{}.{}", payload, quillo::crypto::b64url(&quillo::crypto::hmac_sha256(&key, &msg)));
    let cookie = format!("quillo_session={}; __Host-quillo_session={}", token, token);
    let first = send(s.port, "POST", "/pay", &[("Idempotency-Key", "k1"), ("Cookie", &cookie)], "");
    assert_eq!((first.status, first.body.as_str()), (200, "session 7"));
    // As one without a session: the same key is the same request.
    let again = send(s.port, "POST", "/pay", &[("Idempotency-Key", "k1")], "");
    assert_eq!((again.status, again.body.as_str()), (200, "session 7"));
    assert_eq!(again.header("idempotent-replayed"), Some("true"));
}

// ---- a call to a service is a request with only an id --------------------------------------

#[test]
fn in_a_call_from_a_service_there_are_no_cookies_or_keys() {
    let secret = "a-test-secret-for-services-long-enough";
    let (_d, f) = app(
        "service-request",
        r#"
server fn visit() = { theme: cookie("theme"), ran: idempotent(() => "without a key"), id: request.id }
"#,
    );
    let s = serve(&f, &[("QUILLO_SERVICE_SECRET", secret)]);
    let (body, call) = ("[]", quillo::service::new_id(16));
    let t = now_secs();
    let sig = quillo::service::signature(secret, t, "app", "visit", "", &call, body.as_bytes());
    let sig = format!("t={},sig={}", t, sig);
    let r = send(s.port, "POST", "/_quillo/service/visit", &[("X-Quillo-Service", &sig), ("X-Quillo-To", "app"), ("X-Quillo-Call", &call), ("X-Request-Id", "abc123")], body);
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body, r#"{"ok":{"theme":null,"ran":"without a key","id":"abc123"}}"#);
}

// ---- cookies --------------------------------------------------------------------------------

#[test]
fn a_percent_sign_in_a_cookie_is_encoded_and_comes_back() {
    let d = temp_dir("cover-data-cookie");
    let mut it = Interp::new(&d);
    it.load_source("route GET /set {\n    set_cookie(\"tip\", \"50%\")\n    \"ok\"\n}\nroute GET /tip = cookie(\"tip\")\n", "app.qwe").unwrap();
    let r = get(&mut it, "/set", &[]);
    assert!(r.header("set-cookie").unwrap().starts_with("tip=50%25;"), "{:?}", r.header("set-cookie"));
    assert_eq!(get(&mut it, "/tip", &[("Cookie", "tip=50%25")]).text(), "50%");
}

// ---- services --------------------------------------------------------------------------------

const SECRET: &str = "a-test-secret-for-services-long-enough";

fn signing() {
    static SET: Once = Once::new();
    SET.call_once(|| std::env::set_var("QUILLO_SERVICE_SECRET", SECRET));
}

#[test]
fn a_services_options_are_a_map() {
    let d = temp_dir("cover-data-declare");
    std::fs::write(d.join("work.qwe"), "server fn add(a: int) -> int = a\n").unwrap();
    let mut it = Interp::new(&d);
    let e = match quillo::service::declare(&mut it, "billing", None, "work.qwe", Some(&Value::Int(5)), 3) {
        Err(quillo::interp::Ctrl::Error(e)) => e,
        _ => panic!("options that are no map were taken"),
    };
    assert_eq!(e.msg, "service billing: its options are a map, like { timeout: \"2s\", retries: 2 }");
}

#[test]
fn calls_at_once_when_an_address_gets_its_one_try_take_turns() {
    signing();
    let d = temp_dir("cover-data-flap");
    std::fs::write(d.join("work.qwe"), "server fn add(a: int) -> int = a\n").unwrap();
    let port = free_port();
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let d = d.clone();
            std::thread::spawn(move || {
                let mut it = Interp::new(&d);
                it.shared.service_urls.lock().unwrap().insert("flap".into(), format!("http://127.0.0.1:{}", port));
                it.load_source("service flap = \"work.qwe\" { retries: 0, failures: 1, cooldown: 2 }\n", "main.qwe").unwrap();
                let (mut tried, mut left_alone) = (0, 0);
                let until = Instant::now() + Duration::from_secs(3);
                while Instant::now() < until {
                    let out = it.load_source("flap.add(1)\n", "call.qwe").map(|v| v.repr()).unwrap();
                    if out.contains("unreachable at") {
                        tried += 1;
                    } else if out.contains("unavailable (every address failed lately") {
                        left_alone += 1;
                    } else {
                        panic!("{}", out);
                    }
                }
                (tried, left_alone)
            })
        })
        .collect();
    let (mut tried, mut left_alone) = (0, 0);
    for t in threads {
        let (a, b) = t.join().unwrap();
        tried += a;
        left_alone += b;
    }
    assert!(tried > 8 && left_alone > 8, "tried {}, left alone {}", tried, left_alone);
}

#[test]
fn a_call_made_later_whose_arguments_are_spoiled_is_made_without_them() {
    let d = temp_dir("cover-data-later");
    std::fs::write(d.join("mailer.qwe"), "let sent = []\nserver fn send(to: str = \"nobody\") -> int {\n    sent = sent + [to]\n    len(sent)\n}\nroute GET /sent = sent | join(\",\")\n").unwrap();
    std::fs::write(
        d.join("front.qwe"),
        "db.open(\"front.db\")\nservice mailer = \"mailer.qwe\"\nroute GET /send/:to = mailer.later.send(to)\nroute POST /spoil = db.exec(\"update _quillo_later set args = 'not json'\").changes\n",
    )
    .unwrap();
    let pm = free_port();
    let url = format!("http://127.0.0.1:{}", pm);
    let f = serve(&d.join("front.qwe"), &[("QUILLO_SERVICE_SECRET", SECRET), ("QUILLO_SERVICE_MAILER", &url)]);
    assert_eq!(send(f.port, "GET", "/send/ana", &[], "").status, 200);
    assert_eq!(send(f.port, "POST", "/spoil", &[], "").body, "1");
    let port = pm;
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("mailer.qwe"))
        .args(["--port", &port.to_string()])
        .env("QUILLO_SERVICE_SECRET", SECRET)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _mailer = Guard::new(child);
    eventually(20, "the mailer did not start", || support::listening(port).then_some(()));
    let sent = eventually(40, "the call made later was not made", || {
        let body = send(pm, "GET", "/sent", &[], "").body;
        (!body.is_empty()).then_some(body)
    });
    assert_eq!(sent, "nobody");
}

/// A service that had run for a minute is back after a second, whatever
/// waits came before. (This test takes a little over a minute.)
#[test]
fn a_service_that_ran_a_minute_is_started_again_at_once() {
    let d = temp_dir("cover-data-split");
    std::fs::write(d.join("steady.qwe"), "server fn add(a: int) -> int = a\nroute GET /up = \"up\"\n").unwrap();
    std::fs::write(d.join("app.qwe"), "service /steady = \"steady.qwe\"\nroute GET / = \"app\"\n").unwrap();
    let log = d.join("log");
    let (_app, port) = run_logged(&d.join("app.qwe"), &["--split"], &[], &log);
    eventually(20, "the service did not answer", || (send(port, "GET", "/steady/up", &[], "").status == 200).then_some(()));
    let service = d.join("steady.qwe").display().to_string();
    let kill = || {
        let pids = String::from_utf8(Command::new("pgrep").args(["-f", &service]).output().unwrap().stdout).unwrap();
        assert!(!pids.trim().is_empty(), "the service is not running");
        for pid in pids.split_whitespace() {
            let _ = Command::new("kill").args(["-9", pid]).status();
        }
    };
    let said = |what: &str| std::fs::read_to_string(&log).unwrap_or_default().matches(what).count();
    // Stopped soon after it started: the wait grows to two seconds.
    kill();
    eventually(10, "it was not started again", || (said("started again") == 1).then_some(()));
    assert_eq!(said("starting it again in 1 s"), 1);
    eventually(20, "the service did not come back", || (send(port, "GET", "/steady/up", &[], "").status == 200).then_some(()));
    // A minute later it has shown it runs: a second again.
    std::thread::sleep(Duration::from_secs(62));
    kill();
    eventually(10, "it was not noticed", || (said("starting it again in") == 2).then_some(()));
    assert_eq!(said("starting it again in 1 s"), 2, "{}", std::fs::read_to_string(&log).unwrap_or_default());
}

// ---- answers cut short ----------------------------------------------------------------------

const PAGE: &str = r#"
server fn load() {
    write_file("loading", "yes")
    sleep(700)
    1 / len(env("NOT_SET_ANYWHERE", ""))
}
component Late() {
    state s = defer(load())
    <p id="late">{s ?? "waiting"}</p>
}
fn slow_page() {
    write_file("making", "yes")
    sleep(700)
    <html><body><Late/><footer>end</footer></body></html>
}
route GET /ready = "ok"
route GET /page = <html><body><Late/><footer>end</footer></body></html>
route GET /slow = slow_page()
"#;

#[test]
fn a_waiting_part_that_fails_leaves_its_placeholder_and_the_page_ends() {
    let (_d, f) = app("defer-fails", PAGE);
    let s = serve(&f, &[]);
    let r = read_all(&mut ask(s.port, "/page"));
    assert!(r.starts_with("HTTP/1.1 200"), "{}", r);
    assert!(r.contains("data-qd=") && r.contains("waiting") && r.contains("</body>"), "{}", r);
    assert!(!r.contains("<template"), "{}", r);
}

#[test]
fn a_server_stopping_before_a_page_is_sent_sends_only_its_first_part() {
    let (d, f) = app("stop-first", PAGE);
    let log = d.join("log");
    let (server, port) = run_logged(&f, &[], &[("QUILLO_SECRET", "x")], &log);
    let mut c = ask(port, "/slow");
    wait_for_file(&d.join("making"));
    interrupt(&server);
    let r = read_all(&mut c);
    assert!(r.starts_with("HTTP/1.1 200") && r.contains("data-qd="), "{}", r);
    assert!(!r.contains("</body>") && !r.contains("0\r\n\r\n"), "the rest should not follow: {}", r);
    assert!(!d.join("loading").exists(), "the waiting part was not to be worked out");
}

#[test]
fn a_server_stopping_after_a_failed_part_does_not_end_the_answer() {
    let (d, f) = app("stop-tail", PAGE);
    let log = d.join("log");
    let (server, port) = run_logged(&f, &[], &[("QUILLO_SECRET", "x")], &log);
    let mut c = ask(port, "/page");
    wait_for_file(&d.join("loading"));
    interrupt(&server);
    let r = read_all(&mut c);
    assert!(r.contains("data-qd=") && r.contains("</body>"), "{}", r);
    assert!(!r.trim_end().ends_with("\r\n0"), "no last chunk while stopping: {}", r);
}

#[test]
fn send_after_the_client_left_fails_again() {
    // `db.connect` with a function: what the function fails with comes back
    // as an error value, so the stream's function goes on after `send` failed.
    let (d, f) = app(
        "stream-gone",
        r#"
fn pour(send) {
    for i in range(100000) {
        send("tick\n")
        sleep(5)
    }
    "never.db"
}
route GET /s = stream((send) => {
    send("hello\n")
    let each = db.connect((t) => pour(send))
    let r = with_tenant("a", () => each.query("select 1"))
    write_file("after.txt", `${r}`)
    send("again\n")
    write_file("never.txt", "sent")
})
"#,
    );
    let s = serve(&f, &[]);
    let mut c = ask(s.port, "/s");
    let mut buf = [0u8; 256];
    let n = c.read(&mut buf).unwrap();
    assert!(n > 0, "nothing came");
    drop(c);
    wait_for_file(&d.join("after.txt"));
    std::thread::sleep(Duration::from_millis(200));
    let after = std::fs::read_to_string(d.join("after.txt")).unwrap();
    assert!(after.contains("the client went away"), "{}", after);
    assert!(!d.join("never.txt").exists(), "`send` should have failed");
    assert_eq!(send(s.port, "GET", "/nothing", &[], "").status, 404, "the server goes on");
}

// ---- a large file and a slow reader ---------------------------------------------------------

#[cfg(target_os = "macos")]
fn connect_with_small_window(port: u16) -> TcpStream {
    use std::os::fd::FromRawFd;
    // SAFETY: plain socket calls, on a socket made here and owned by the stream.
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        assert!(fd >= 0);
        let size: libc::c_int = 2048;
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVBUF, &size as *const _ as *const libc::c_void, std::mem::size_of::<libc::c_int>() as libc::socklen_t);
        let mut a: libc::sockaddr_in = std::mem::zeroed();
        a.sin_family = libc::AF_INET as _;
        a.sin_port = port.to_be();
        a.sin_addr.s_addr = u32::from_ne_bytes([127, 0, 0, 1]);
        let r = libc::connect(fd, &a as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t);
        assert_eq!(r, 0, "cannot connect");
        TcpStream::from_raw_fd(fd)
    }
}

/// A reader slower than the slowest allowed is let go after the grace period.
/// (This test takes as long as that, and more: 45 seconds.) On macOS: Linux's larger send
/// buffers keep what was sent above that rate for far longer.
#[cfg(target_os = "macos")]
#[test]
fn a_reader_that_barely_reads_a_large_file_is_let_go() {
    let (d, f) = app("slow-reader", "route GET /big = file(\"big.bin\")\n");
    let size: u64 = 256 << 20;
    std::fs::File::create(d.join("big.bin")).unwrap().set_len(size).unwrap();
    let s = serve(&f, &[]);
    let mut c = connect_with_small_window(s.port);
    c.set_read_timeout(Some(Duration::from_secs(40))).unwrap();
    write!(c, "GET /big HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    // A few kilobytes a second, well under the slowest allowed, past the
    // grace period and until what was sent is less than that rate allows;
    // then all there is. What was sent before it was let go is still on its
    // way, but a server that had not let go would send the whole file.
    let began = Instant::now();
    let (mut got, mut buf) = (0u64, [0u8; 1024]);
    loop {
        match c.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => got += n as u64,
        }
        if began.elapsed() < Duration::from_secs(45) {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    assert!(got > 0 && got < size / 2, "it got {} bytes", got);
    // Its place is free again.
    assert_eq!(send(s.port, "HEAD", "/big", &[], "").status, 200);
    let _ = std::fs::remove_dir_all(&d);
}

// ---- http with public_only -------------------------------------------------------------------

#[test]
fn a_public_address_is_let_through_public_only() {
    let d = temp_dir("cover-data-public");
    let mut it = Interp::new(&d);
    let inside = eval(&mut it, r#"http.get("http://127.0.0.1:1/", {}, { public_only: true, timeout: 1 })"#);
    assert!(inside.contains("refused by public_only: 127.0.0.1 is not on the public internet"), "{}", inside);
    // Nothing answers on this port of this address, or there is no network
    // here: either way, the call was let go, as the one above was not.
    let outside = eval(&mut it, r#"http.get("http://1.1.1.1:1/", {}, { public_only: true, timeout: 1 })"#);
    assert!(outside.starts_with("err("), "{}", outside);
    assert!(!outside.contains("public internet"), "{}", outside);
}

// ---- images -----------------------------------------------------------------------------------

#[test]
fn an_image_wider_than_any_taken_is_refused_from_its_header() {
    let d = temp_dir("cover-data-wide");
    image::GrayImage::new(13_000, 1).save(d.join("wide.png")).unwrap();
    let mut it = Interp::new(&d);
    let out = eval(&mut it, r#"image.info("wide.png")"#);
    assert_eq!(out, "err(\"image: Image size exceeds limit\")");
}

#[test]
fn a_program_at_the_root_cannot_convert_onto_its_own_folder() {
    let d = temp_dir("cover-data-root");
    image::RgbImage::new(4, 4).save(d.join("a.png")).unwrap();
    let from_root = d.canonicalize().unwrap().join("a.png").display().to_string();
    let mut it = Interp::new(Path::new("/"));
    let out = eval(&mut it, &format!("image.convert(\"{}\", \".\")", from_root.trim_start_matches('/')));
    assert!(out.starts_with("err(\"image: ") && out.contains("directory"), "{}", out);
}

// ---- PostgreSQL --------------------------------------------------------------------------------

fn psql(pg: &support::Pg, url: &str, sql: &[&str]) {
    let mut c = Command::new(pg.bin.join("psql"));
    c.args([url, "-q", "-v", "ON_ERROR_STOP=1"]);
    for s in sql {
        c.args(["-c", s]);
    }
    let out = c.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

fn psql_session(pg: &support::Pg, url: &str, script: &str) {
    let mut c = Command::new(pg.bin.join("psql"));
    c.args([url, "-q", "-v", "ON_ERROR_STOP=1"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

fn psql_value(pg: &support::Pg, url: &str, sql: &str) -> String {
    let out = Command::new(pg.bin.join("psql")).args([url, "-qAt", "-c", sql]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn a_uuid_of_the_wrong_length_is_said_to_be_bad() {
    let Some(pg) = support::postgres("cover-uuid") else { return };
    // This server now sends a uuid as four bytes.
    psql(&pg, &pg.url, &["update pg_type set typsend = 'int4send'::regproc where oid = 2950"]);
    let d = temp_dir("cover-data-uuid");
    let mut it = Interp::new(&d);
    let db = eval(&mut it, &format!("db.open(\"{}\")\ndb.query(\"select '00000000-0000-0000-0000-000000000001'::uuid as u\")", pg.url));
    assert!(db.contains("bad uuid"), "{}", db);
}

#[test]
fn recovery_takes_the_decision_another_server_recorded_at_the_same_moment() {
    let Some(pg) = support::postgres("cover-race") else { return };
    psql(&pg, &pg.url, &["alter system set max_prepared_transactions = 10"]);
    let dir = std::env::temp_dir().join(format!("quillo-pg-cover-race-{}", std::process::id()));
    let ok = Command::new(pg.bin.join("pg_ctl")).arg("-D").arg(&dir).arg("-l").arg(dir.join("log")).args(["-w", "restart"]).output().unwrap();
    assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
    let url = pg.database("raced");
    // Recovery's first look finds nothing; its own record is then refused
    // because the row is there, as when another server is recovering too;
    // its second look finds it.
    psql(
        &pg,
        &url,
        &[
            "create table t (x text)",
            "create table decisions (gid text primary key, decision text not null, at bigint not null)",
            "create sequence looks",
            "create view _quillo_atomic as select * from decisions where nextval('looks') > 1",
            "insert into decisions values ('quillo_1000_raced', 'commit', 1)",
        ],
    );
    psql_session(&pg, &url, "begin;\ninsert into t values ('kept');\nprepare transaction 'quillo_1000_raced_0';\n");
    let d = temp_dir("cover-data-race");
    let mut it = Interp::new(&d);
    it.load_source(&format!("db.open(\"{}\")\n", url), "app.qwe").unwrap();
    quillo::db::recover(&mut it);
    assert_eq!(psql_value(&pg, &url, "select count(*) from pg_prepared_xacts"), "0");
    assert_eq!(psql_value(&pg, &url, "select x from t"), "kept");
}

/// Passes connections on to `to`, each after `delay`.
fn slow_proxy(to: u16, delay: Duration) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for client in listener.incoming().flatten() {
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                let Ok(server) = TcpStream::connect(("127.0.0.1", to)) else { return };
                let (mut a, mut b) = (client.try_clone().unwrap(), server.try_clone().unwrap());
                std::thread::spawn(move || {
                    let _ = std::io::copy(&mut a, &mut b);
                    let _ = b.shutdown(std::net::Shutdown::Both);
                });
                let (mut a, mut b) = (server, client);
                let _ = std::io::copy(&mut a, &mut b);
                let _ = b.shutdown(std::net::Shutdown::Both);
            });
        }
    });
    port
}

#[test]
fn tenants_opened_while_the_default_database_opens() {
    let Some(pg) = support::postgres("cover-open") else { return };
    let pg_port: u16 = pg.url.rsplit(':').next().unwrap().split('/').next().unwrap().parse().unwrap();
    let url = format!("postgres://quillo@127.0.0.1:{}/postgres", slow_proxy(pg_port, Duration::from_millis(800)));
    let (_d, f) = app(
        "open-race",
        r#"
route GET /first {
    let rows = db.query("select 1 as one")
    `${rows}`
}
route GET /tenants {
    db.open("t/{tenant}.db")
    "opened"
}
"#,
    );
    let s = serve(&f, &[("QUILLO_DB", &url)]);
    let port = s.port;
    let first = std::thread::spawn(move || send(port, "GET", "/first", &[], ""));
    std::thread::sleep(Duration::from_millis(250));
    let other = send(port, "GET", "/tenants", &[], "");
    assert_eq!((other.status, other.body.as_str()), (200, "opened"));
    let r = first.join().unwrap();
    // Its rows, or an error value saying no tenant is in force: not a panic.
    assert_eq!(r.status, 200, "{}", r.body);
}

/// The smallest interval there is: `abs` of its microseconds does not fit.
const LEAST_INTERVAL: &str = "-2562047788:00:54.775808";
const LEAST_INTERVAL_SQL: &str = "interval '-2562047788:00:00' - interval '54.775808 seconds'";

fn interval_app(name: &str, pg: &support::Pg) -> PathBuf {
    let (_, f) = app(
        name,
        &format!(
            r#"
db.open("{}")
route GET /i {{
    let rows = db.query("select {} as i")
    `${{rows}}`
}}
route GET /one = str(db.one("select 1 as n").n)
"#,
            pg.url, LEAST_INTERVAL_SQL
        ),
    );
    f
}

#[test]
fn a_query_that_fails_unexpectedly_is_an_error_value_and_the_server_goes_on() {
    let Some(pg) = support::postgres("cover-interval") else { return };
    let f = interval_app("interval", &pg);
    let s = serve(&f, &[]);
    let r = send(s.port, "GET", "/i", &[], "");
    // Reading this value panics in a debug build, where overflow is checked (see the
    // finding below), and the request hears of it; once it does not, the request gets
    // the value. A release build gives wrong text today, and goes on all the same.
    assert_eq!(r.status, 200, "{}", r.body);
    if cfg!(debug_assertions) {
        assert!(r.body.contains("the database call failed unexpectedly") || r.body.contains(LEAST_INTERVAL), "{}", r.body);
    }
    assert_eq!(send(s.port, "GET", "/one", &[], "").body, "1");
}

#[test]
fn the_smallest_interval_is_read_as_it_is() {
    let Some(pg) = support::postgres("cover-interval2") else { return };
    let f = interval_app("interval2", &pg);
    let s = serve(&f, &[]);
    let r = send(s.port, "GET", "/i", &[], "");
    assert!(r.body.contains(LEAST_INTERVAL), "{}", r.body);
}
