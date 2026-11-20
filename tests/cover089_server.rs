//! Coverage of 0.8.9 (branch feedback75): the lines of Quillo's HTTP server (src/httpd.rs), the
//! request map's identity and `site_headers` rules (src/server.rs), split services' words and
//! answers (src/service.rs, src/stream.rs), the prefetch guard (src/interp.rs) and fetch
//! (src/fetch.rs) that FEEDBACK75.md and FEEDBACK78.md added and no test ran. Servers are this
//! test's own, on the port the system gives, stopped by the test; folders are removed.

mod support;

use quillo::server::HttpRequest;
use quillo::Interp;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// CI runs an unoptimized build, many times slower: time bounds get this margin there.
const SLOWER: u64 = if cfg!(debug_assertions) { 10 } else { 1 };

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("cover089-server-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

/// Removes a test's folder when the test ends, passed or not.
struct Gone(PathBuf);

impl Drop for Gone {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `quillo check` of a program this test runs: every snippet passes it (warnings allowed).
fn checked(d: &Path, file: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["check", file]).current_dir(d).output().unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{}", said);
    said
}

/// `quillo run` of `file` as a server with `args` and `env`, its log in `log`, on the port the
/// system picks, once it says where it listens.
fn serve(d: &Path, file: &str, args: &[&str], env: &[(&str, &str)], log: &str) -> (support::Guard, u16) {
    let out = std::fs::File::create(d.join(log)).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", file, "--port", "0"])
        .args(args)
        .current_dir(d)
        .env("QUILLO_SECRET", "a secret for the cover089 server tests, long enough")
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(out.try_clone().unwrap())
        .stderr(out)
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let port = support::port_in_log(&mut g.0, &d.join(log), Duration::from_secs(20 * SLOWER));
    (g, port)
}

/// `serve` of an app with `--split` whose service under `/shop` answers `/` with "shop", once it
/// does.
fn split(d: &Path, args: &[&str], log: &str) -> (support::Guard, u16) {
    let (g, port) = serve(d, "app.qwe", &[&["--split"], args].concat(), &[], log);
    let end = Instant::now() + Duration::from_secs(30 * SLOWER);
    while support::send(port, "GET", "/shop/", &[], "").body != "shop" {
        assert!(Instant::now() < end, "the service did not start: {}", std::fs::read_to_string(d.join(log)).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(100));
    }
    (g, port)
}

/// The log once it holds `needle`, or as it is after `secs`.
fn log_holding(log: &Path, needle: &str, secs: u64) -> String {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if text.contains(needle) || Instant::now() > end {
            return text;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// In-process tests one at a time: some set `--max-memory`, which is the process's.
fn serial() -> MutexGuard<'static, ()> {
    static ONE: Mutex<()> = Mutex::new(());
    ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `--max-memory` for this process while the value lives; none again after.
struct Limit;

impl Limit {
    fn set(bytes: u64) -> Limit {
        quillo::httpd::limit_memory(bytes);
        Limit
    }
}

impl Drop for Limit {
    fn drop(&mut self) {
        quillo::httpd::limit_memory(0);
    }
}

fn app(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "test.qwe") {
        panic!("{}", e);
    }
    it
}

fn request(method: &str, url: &str, headers: &[(&str, &str)], body: &[u8]) -> HttpRequest {
    let mut h = vec![("Host".to_string(), "localhost:8080".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    HttpRequest { method: method.into(), url: url.into(), headers: h, body: body.to_vec(), remote: "127.0.0.1".into() }
}

fn header<'a>(r: &'a quillo::server::HttpResponse, name: &str) -> Option<&'a str> {
    r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// Resets the connection when it is closed (`SO_LINGER` 0), as a client that goes at once does.
fn resets_when_closed(s: &TcpStream) {
    use std::os::fd::AsRawFd;
    let l = libc::linger { l_onoff: 1, l_linger: 0 };
    // SAFETY: a socket this test holds open, and a `linger` that lives through the call.
    let ok = unsafe { libc::setsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_LINGER, &l as *const libc::linger as *const libc::c_void, std::mem::size_of::<libc::linger>() as libc::socklen_t) };
    assert_eq!(ok, 0);
}

/// A connection with `GET path` sent on it, kept open.
fn asking(port: u16, path: &str) -> TcpStream {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60 * SLOWER))).unwrap();
    s.write_all(format!("GET {} HTTP/1.1\r\nHost: localhost\r\n\r\n", path).as_bytes()).unwrap();
    s
}

/// What `s` sends until `needle` has come (or the connection ends).
fn read_until(s: &mut TcpStream, needle: &[u8]) -> Vec<u8> {
    let mut got = Vec::new();
    let mut buf = [0u8; 4096];
    while !got.windows(needle.len()).any(|w| w == needle) {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    got
}

// ---- httpd.rs: a thread the system refuses -----------------------------------------------------

/// A thread for waiting work that the system refuses to start (here every thread without a size
/// of its own asks for a 128 TiB stack, by `RUST_MIN_STACK`, which no address space holds) is said
/// once, with the threads there are, and tried again a little later, at most once a second, while
/// the loop goes on: a request that waits for such a thread waits, and the others are answered
/// (src/httpd.rs `Pool::start`, FEEDBACK41.md Q-480 as FEEDBACK78.md Q-1168 left it; coverage of
/// 0.8.9; branch feedback75)
#[test]
fn a_thread_the_system_refuses_is_said_once_and_the_loop_goes_on() {
    let src = "route GET / = \"up\"\nroute GET /wait = str(http.get(\"http://127.0.0.1:9/\"))\n";
    let d = dir("no-thread", &[("main.qwe", src)]);
    let _gone = Gone(d.clone());
    checked(&d, "main.qwe");
    let huge = (1u64 << 47).to_string();
    let (_g, port) = serve(&d, "main.qwe", &[], &[("RUST_MIN_STACK", huge.as_str())], "no-thread.log");
    // The loop's clock starts with its first request; a failed start is tried again once it has
    // run a second.
    assert_eq!(support::send(port, "GET", "/", &[], "").body, "up");
    std::thread::sleep(Duration::from_millis(1200));
    let mut first = asking(port, "/wait");
    let log = log_holding(&d.join("no-thread.log"), "cannot start a thread for waiting work", 20 * SLOWER);
    assert!(
        log.contains("quillo: cannot start a thread for waiting work (") && log.contains("; the account's process limit?); requests wait for the 0 threads there are"),
        "{}",
        log
    );
    // The loop answers meanwhile; a second request that waits is not said again.
    assert_eq!(support::send(port, "GET", "/", &[], "").body, "up");
    let mut second = asking(port, "/wait");
    assert_eq!(support::send(port, "GET", "/", &[], "").body, "up");
    std::thread::sleep(Duration::from_millis(200));
    let log = std::fs::read_to_string(d.join("no-thread.log")).unwrap();
    assert_eq!(log.matches("cannot start a thread for waiting work").count(), 1, "{}", log);
    // Neither has been answered: no thread is there to do their waiting.
    for s in [&mut first, &mut second] {
        s.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        let mut b = [0u8; 64];
        let got = s.read(&mut b).map_err(|e| e.kind());
        assert!(matches!(got, Err(std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)), "{:?}", got);
    }
}

// ---- httpd.rs: a stream inside a SQLite transaction ---------------------------------------------

const TX_EXPORT: &str = "db.exec(\"create table if not exists items (id integer primary key, body text)\")\ndb.exec(\"create table if not exists seen (n int)\")\nif db.one(\"select count(*) as n from items\").n == 0 {\n    db.transaction(() => {\n        for i in range(2000) {\n            db.exec(\"insert into items (body) values (?)\", \"x\".repeat(10000))\n        }\n    })\n}\nroute GET /export.txt = stream(send => db.transaction(() => {\n    db.exec(\"insert into seen (n) values (1)\")\n    for row in db.query(\"select body from items\") {\n        send(row.body)\n    }\n}))\nroute GET /seen = db.one(\"select count(*) as n from seen\").n\nroute GET /ping = \"pong\"\n";

/// A client that resets its connection while a stream inside a SQLite transaction sends to it: the
/// write fails, the send fails as a client gone's and the transaction rolls back, and the instance
/// answers at once; a client that reads all of it afterwards has its transaction committed
/// (src/httpd.rs `held_write`, FEEDBACK75.md Q-1067; coverage of 0.8.9; branch feedback75)
#[test]
fn a_reset_inside_a_sqlite_transaction_rolls_it_back() {
    let d = dir("tx-reset", &[("main.qwe", TX_EXPORT)]);
    let _gone = Gone(d.clone());
    assert!(checked(&d, "main.qwe").contains("main.qwe:13: warning: `send` inside `db.transaction` waits for the client to read"));
    let (_g, port) = serve(&d, "main.qwe", &["--no-check"], &[], "tx-reset.log");
    let mut c = asking(port, "/export.txt");
    let got = read_until(&mut c, b"xxxx");
    assert!(got.windows(4).any(|w| w == b"xxxx"), "{:?}", String::from_utf8_lossy(&got));
    resets_when_closed(&c);
    drop(c);
    let t = Instant::now();
    assert_eq!(support::send(port, "GET", "/ping", &[], "").body, "pong");
    assert!(t.elapsed() < Duration::from_secs(3 * SLOWER), "{:?}", t.elapsed());
    assert_eq!(support::send(port, "GET", "/seen", &[], "").body, "0");
    let all = support::send(port, "GET", "/export.txt", &[], "").body;
    assert!(all.len() >= 20_000_000, "{}", all.len());
    assert_eq!(support::send(port, "GET", "/seen", &[], "").body, "1");
}

/// Readers that keep reading, a KiB at a time, at about 4 and 2 MB a second, slower than a stream
/// inside a SQLite transaction sends where the server outpaces them (20 MB in 10 ms to a reader that
/// keeps up, in an optimized build): each send waits a moment for its reader, and past the answer's
/// second in all the rest is dropped and the connection ends short. What holds in any build: an
/// answer cut short never commits its transaction, a whole one commits, and the instance answers
/// meanwhile (src/httpd.rs `held_write`, FEEDBACK75.md Q-1067; coverage of 0.8.9; branch feedback75)
#[test]
fn a_steady_slow_reader_inside_a_sqlite_transaction_has_a_second_in_all() {
    let d = dir("tx-steady", &[("main.qwe", TX_EXPORT)]);
    let _gone = Gone(d.clone());
    checked(&d, "main.qwe");
    let (_g, port) = serve(&d, "main.qwe", &["--no-check"], &[], "tx-steady.log");
    let seen = || support::send(port, "GET", "/seen", &[], "").body.parse::<u32>().unwrap();
    let mut committed = 0;
    for (round, every) in [250u64, 500].into_iter().enumerate() {
        let mut c = asking(port, "/export.txt");
        let reading = std::thread::spawn(move || {
            // Waited for here: a sleep that short takes longer.
            let (mut got, mut buf) = (Vec::new(), [0u8; 1024]);
            loop {
                match c.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got.extend_from_slice(&buf[..n]),
                }
                // A whole answer ends with its last chunk; the connection is kept.
                if got.ends_with(b"\r\n0\r\n\r\n") {
                    break;
                }
                let at = Instant::now();
                while at.elapsed() < Duration::from_micros(every) {
                    std::hint::spin_loop();
                }
            }
            got
        });
        if round == 0 {
            std::thread::sleep(Duration::from_millis(300));
            let t = Instant::now();
            assert_eq!(support::send(port, "GET", "/ping", &[], "").body, "pong");
            assert!(t.elapsed() < Duration::from_secs(3 * SLOWER), "{:?}", t.elapsed());
        }
        let got = reading.join().unwrap();
        // Whole: chunked to its last chunk.
        let whole = got.ends_with(b"\r\n0\r\n\r\n") && got.len() >= 20_000_000;
        committed += u32::from(whole);
        assert_eq!(seen(), committed, "a KiB every {} µs: {} bytes, whole: {}", every, got.len(), whole);
    }
}

// ---- server.rs: who the visitor is, under --split -------------------------------------------------

const WHO: &str = "route GET /who = { cert: request.client_cert, user: request.remote_user }\n";

/// The service's routes the split tests ask for: who the visitor is, its cookies, and streams that
/// fail, pause or end once their client has gone.
const SHOP: &str = "route GET / = \"shop\"\nroute GET /who = { cert: request.client_cert, user: request.remote_user }\nroute GET /cookies = keys(request.cookies)\nroute GET /st-fail = stream((send) => {\n    send(\"first\\n\")\n    sleep(300)\n    1 / (len(\"a\") - 1)\n})\nroute GET /st-gap = stream((send) => {\n    send(\"a\\n\")\n    sleep(1500)\n    send(\"b\\n\")\n    sleep(600000)\n    send(\"c\\n\")\n})\nroute GET /st-end = stream((send) => {\n    send(\"a\\n\")\n    sleep(1500)\n})\n";

/// Under `--split` with `--user-header` alone, the app and its service read the user from the
/// trusted proxy's header and no certificate: the app's word has a null certificate, which the
/// service reads as null; with no header, nothing; with `--client-cert-header`
/// alone, the certificate and a null user (src/server.rs `proxy_identity` and
/// `proxy_identity_json`, FEEDBACK78.md A.13 and A.14; coverage of 0.8.9; branch feedback75)
#[test]
fn a_split_service_knows_the_visitor_by_one_header() {
    let app = format!("service /shop = \"shop.qwe\"\n{}", WHO);
    let d = dir("who-one", &[("app.qwe", &app), ("shop.qwe", SHOP)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (_g, port) = split(&d, &["--trust-proxy", "127.0.0.1", "--user-header", "x-remote-user"], "who-user.log");
    // In unicode too, which a service's page answered with a 502 before Q-1174.
    for p in ["/who", "/shop/who"] {
        assert_eq!(support::send(port, "GET", p, &[("x-remote-user", "ünï@EXAMPLE.ORG")], "").body, r#"{"cert":null,"user":"ünï@EXAMPLE.ORG"}"#, "{}", p);
    }
    let user = [("x-remote-user", "ann@EXAMPLE.ORG")];
    for p in ["/who", "/shop/who"] {
        assert_eq!(support::send(port, "GET", p, &user, "").body, r#"{"cert":null,"user":"ann@EXAMPLE.ORG"}"#, "{}", p);
        assert_eq!(support::send(port, "GET", p, &[], "").body, r#"{"cert":null,"user":null}"#, "{}", p);
    }
    // A certificate header no option names is no one's.
    let cert = [("x-forwarded-client-cert", "Hash=abc;Subject=\"CN=ann,O=Example\"")];
    assert_eq!(support::send(port, "GET", "/shop/who", &cert, "").body, r#"{"cert":null,"user":null}"#);
    drop(_g);
    let (_h, port) = split(&d, &["--trust-proxy", "127.0.0.1", "--client-cert-header", "x-forwarded-client-cert"], "who-cert.log");
    let want = r#"{"cert":{"name":"ann","subject":"CN=ann,O=Example"},"user":null}"#;
    for p in ["/who", "/shop/who"] {
        assert_eq!(support::send(port, "GET", p, &[cert[0], user[0]], "").body, want, "{}", p);
    }
}

// ---- stream.rs and service.rs: a split service's answers passed on ---------------------------------

/// Under `--split`, a service's stream passed on as it comes: one that fails part way ends the
/// visitor's answer there, unfinished, and the app says the service's answer stopped coming; once
/// the visitor has gone, the app stops reading the service's answer at its next piece (where the
/// service would go on for ten minutes) and at its end; a `Cookie` line with none of the app's
/// session cookies is passed on as it is (src/stream.rs `run_reader`, src/service.rs
/// `without_apps_sessions`, FEEDBACK75.md Q-1071 and Q-1081; coverage of 0.8.9; branch feedback75)
#[test]
fn a_split_services_stream_ends_with_it_or_with_its_visitor() {
    let d = dir("split-streams", &[("app.qwe", "service /shop = \"shop.qwe\"\nroute GET / = \"app\"\n"), ("shop.qwe", SHOP)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (_g, port) = split(&d, &[], "streams.log");
    let log = d.join("streams.log");
    // Cookies with none of the app's: as they came, in their order.
    assert_eq!(support::send(port, "GET", "/shop/cookies", &[("Cookie", "theme=dark; lang=fr")], "").body, r#"["theme","lang"]"#);
    assert_eq!(support::send(port, "GET", "/shop/cookies", &[("Cookie", "ünï=1")], "").body, r#"["ünï"]"#);
    // A stream that fails: what came, then the end of the connection, unfinished.
    let mut c = asking(port, "/shop/st-fail");
    let mut got = Vec::new();
    let _ = c.read_to_end(&mut got);
    let text = String::from_utf8_lossy(&got);
    assert!(text.starts_with("HTTP/1.1 200") && text.contains("first\n") && !text.ends_with("0\r\n\r\n"), "{:?}", text);
    let said = log_holding(&log, "a service's answer stopped coming", 10 * SLOWER);
    assert!(said.contains("quillo: a service's answer stopped coming: "), "{}", said);
    // The visitor goes after the first piece: the app's request ends at the second, not after the
    // service's ten minutes.
    for (path, line) in [("/shop/st-gap", "GET /shop/st-gap -> 200"), ("/shop/st-end", "GET /shop/st-end -> 200")] {
        let mut c = asking(port, path);
        let got = read_until(&mut c, b"a\n");
        assert!(String::from_utf8_lossy(&got).contains("a\n"), "{:?}", String::from_utf8_lossy(&got));
        resets_when_closed(&c);
        drop(c);
        let said = log_holding(&log, line, 20 * SLOWER);
        assert!(said.lines().any(|l| l.starts_with(line)), "{}: {}", path, said);
    }
    assert_eq!(support::send(port, "GET", "/shop/", &[], "").body, "shop");
    let said = std::fs::read_to_string(&log).unwrap();
    assert_eq!(said.matches("a service's answer stopped coming").count(), 1, "{}", said);
}

/// A server on a port of its own answering each request, read whole, with `answer`, then hanging
/// up: its URL.
fn answering(answer: Vec<u8>) -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let answer = Arc::new(answer);
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let answer = answer.clone();
            std::thread::spawn(move || {
                let mut got = Vec::new();
                let mut buf = [0u8; 4096];
                while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut buf) {
                        Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]),
                        _ => return,
                    }
                }
                let _ = s.write_all(&answer);
            });
        }
    });
    url
}

fn sized(n: usize) -> Vec<u8> {
    let mut a = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", n).into_bytes();
    a.extend(std::iter::repeat(b'q').take(n));
    a
}

/// A service's answer of a known length read whole where nothing streams it (here, the program
/// in-process): one exactly at `--max-memory` is passed on whole, one byte more is "not
/// answering", and an empty one is passed on (src/service.rs `pass_on`, FEEDBACK75.md Q-1071;
/// coverage of 0.8.9; branch feedback75)
#[test]
fn a_service_answer_read_whole_is_held_to_the_limit() {
    let _one = serial();
    let d = support::temp_dir("cover089-server-pass-on");
    let _gone = Gone(d.clone());
    let mut it = Interp::new(&d);
    let req = request("GET", "/shop/big", &[], b"");
    let _limit = Limit::set(1 << 20);
    let r = quillo::service::pass_on(&mut it, &answering(sized(1 << 20)), "/shop", &req, None);
    assert_eq!((r.status, r.body.len()), (200, 1 << 20));
    let r = quillo::service::pass_on(&mut it, &answering(sized((1 << 20) + 1)), "/shop", &req, None);
    assert_eq!((r.status, r.text()), (502, "this part of the site is not answering (its answer is larger than 1048576 bytes)\n".to_string()));
    let r = quillo::service::pass_on(&mut it, &answering(sized(0)), "/shop", &req, None);
    assert_eq!((r.status, r.body.len()), (200, 0));
}

// ---- Q-1174 and Q-1175: a split service's headers and answers ------------------------------------

/// A request read whole off the wire: its status, its headers (names in lower case) and its body,
/// unchunked, as bytes. `head` is the request's head as raw bytes, `Host` and `Connection: close`
/// added.
fn wire(port: u16, head: &[u8]) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60 * SLOWER))).unwrap();
    s.write_all(head).unwrap();
    s.write_all(b"Host: localhost\r\nConnection: close\r\n\r\n").unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let Some(end) = out.windows(4).position(|w| w == b"\r\n\r\n") else { return (0, Vec::new(), out) };
    let text = String::from_utf8_lossy(&out[..end]).to_string();
    let mut lines = text.split("\r\n");
    let status = lines.next().and_then(|l| l.split(' ').nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
    let hs: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let mut body = out[end + 4..].to_vec();
    if hs.iter().any(|(k, v)| k == "transfer-encoding" && v.contains("chunked")) {
        let (mut raw, mut plain) = (&body[..], Vec::new());
        while let Some(at) = raw.windows(2).position(|w| w == b"\r\n") {
            let n = usize::from_str_radix(String::from_utf8_lossy(&raw[..at]).trim(), 16).unwrap_or(0);
            if n == 0 || raw.len() < at + 2 + n {
                break;
            }
            plain.extend_from_slice(&raw[at + 2..at + 2 + n]);
            raw = &raw[(at + 4 + n).min(raw.len())..];
        }
        body = plain;
    }
    (status, hs, body)
}

const HEADERS_APP: &str = "service /shop = \"shop.qwe\"\nroute GET / = \"app\"\nroute GET /note = { note: request.headers[\"x-note\"] }\n";
const HEADERS_SHOP: &str = "route GET / = \"shop\"\nroute GET /in {\n    set_session({ user: \"shopper\" })\n    return \"in\"\n}\nroute GET /me = { user: session().user, note: request.headers[\"x-note\"], theme: request.cookies.theme }\nroute GET /note = { note: request.headers[\"x-note\"] }\nroute GET /fwd = { for: request.headers[\"x-forwarded-for\"], host: request.headers[\"x-forwarded-host\"], prefix: request.headers[\"x-forwarded-prefix\"], lang: request.cookies.lang }\n";

/// The repro: under `--split`, a visitor's header with a byte past ASCII (a UTF-8 cookie a page's
/// script set, `X-Note: café`) made every page of the service a 502 that wrote the header back, the
/// service's HttpOnly session with it; now the service reads them as the visitor sent them, its
/// session too, emoji and two lines of a name in their order (as the app reads them); a byte that
/// is not UTF-8 reaches it as the app reads it; a visitor's own word of headers is not taken, nor
/// its session; a control character is refused, and never written back (src/service.rs `pass_on`,
/// `sendable`, `headers_of_word`, src/server.rs `apps_headers`, FEEDBACK75.md follow-up found by
/// 0.8.9's coverage, Q-1174; coverage of 0.8.9; branch feedback75)
#[test]
fn a_split_service_gets_a_visitors_headers_past_ascii() {
    let d = dir("split-utf8", &[("app.qwe", HEADERS_APP), ("shop.qwe", HEADERS_SHOP)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (_g, port) = split(&d, &[], "utf8.log");
    let a = support::send(port, "GET", "/shop/in", &[], "");
    let session = a.headers.iter().find(|(k, v)| k == "set-cookie" && v.starts_with("quillo_session=")).map(|(_, v)| v.split(';').next().unwrap().to_string()).unwrap_or_else(|| panic!("{:?}", a.headers));
    let line = format!("{}; theme=ünï", session);
    let a = support::send(port, "GET", "/shop/me", &[("Cookie", &line), ("X-Note", "café")], "");
    assert_eq!((a.status, a.body.as_str()), (200, r#"{"user":"shopper","note":"café","theme":"ünï"}"#));
    assert_eq!(support::send(port, "GET", "/shop/me", &[("Cookie", &session), ("X-Note", "😀 ok")], "").body, r#"{"user":"shopper","note":"😀 ok","theme":null}"#);
    // Two lines of one name, one of them past ASCII: as the app's own route reads them.
    let (status, _, own) = wire(port, "GET /note HTTP/1.1\r\nX-Note: plain\r\nX-Note: café\r\n".as_bytes());
    let (status2, _, theirs) = wire(port, "GET /shop/note HTTP/1.1\r\nX-Note: plain\r\nX-Note: café\r\n".as_bytes());
    assert_eq!((status, status2), (200, 200));
    assert_eq!(String::from_utf8_lossy(&theirs), String::from_utf8_lossy(&own));
    // Latin-1, not UTF-8: as the app reads it.
    let (_, _, own) = wire(port, b"GET /note HTTP/1.1\r\nX-Note: caf\xe9\r\n");
    let (status, _, theirs) = wire(port, b"GET /shop/note HTTP/1.1\r\nX-Note: caf\xe9\r\n");
    assert!(status == 200 && theirs == own, "{} {:?} {:?}", status, String::from_utf8_lossy(&theirs), String::from_utf8_lossy(&own));
    // A visitor's own word of headers, carrying the session: dropped by the app.
    let forged = quillo::crypto::b64url(serde_json::json!([["cookie", session], ["x-note", "forged"]]).to_string().as_bytes());
    assert_eq!(support::send(port, "GET", "/shop/me", &[("X-Quillo-Headers", &forged)], "").body, r#"{"user":null,"note":null,"theme":null}"#);
    // A control character: refused, never written back, and not a 502.
    let (status, _, body) = wire(port, format!("GET /shop/me HTTP/1.1\r\nCookie: {}\r\nX-Ctl: a\x01SECRET-CTL\r\n", session).as_bytes());
    let text = String::from_utf8_lossy(&body);
    assert!(status != 502 && status != 200 && !text.contains("SECRET-CTL") && !text.contains(&session), "{} {}", status, text);
    let log = std::fs::read_to_string(d.join("utf8.log")).unwrap();
    assert!(!log.contains("Bad Header") && !log.contains(&session), "{}", log);
}

/// The names the app sets on a request it passes on are its own: a visitor's `X-Forwarded-For`,
/// `-Host` and `-Prefix` past ASCII, carried and put back after the app's, were the ones the service
/// read; now the service has the app's (the visitor's address, its host, the prefix), whatever the
/// visitor sent of each, in ASCII or not; and a `Cookie` line with one cookie past ASCII keeps its
/// others in the line (src/service.rs `pass_on`, `sendable`, `headers_of_word`, src/server.rs
/// `apps_headers`, FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1174's review; coverage of
/// 0.8.9; branch feedback75)
#[test]
fn a_visitor_cannot_set_the_headers_the_app_sets() {
    let d = dir("split-forwarded", &[("app.qwe", HEADERS_APP), ("shop.qwe", HEADERS_SHOP)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (_g, port) = split(&d, &[], "forwarded.log");
    let want = r#"{"for":"127.0.0.1","host":"localhost","prefix":"/shop","lang":"fr"}"#;
    for head in [
        &b"GET /shop/fwd HTTP/1.1\r\nCookie: lang=fr\r\n"[..],
        b"GET /shop/fwd HTTP/1.1\r\nCookie: theme=\xc3\xa9; lang=fr\r\nX-Forwarded-For: 6.6.6.6, \xc3\xa9\r\n",
        b"GET /shop/fwd HTTP/1.1\r\nCookie: lang=fr; theme=\xc3\xa9\r\nX-Forwarded-Host: evil.example/\xc3\xa9\r\n",
        b"GET /shop/fwd HTTP/1.1\r\nCookie: lang=fr\r\nX-Forwarded-Prefix: /evil\xc3\xa9\r\n",
        b"GET /shop/fwd HTTP/1.1\r\nCookie: lang=fr\r\nX-Forwarded-For: 6.6.6.6\r\nX-Forwarded-Host: evil.example\r\nX-Forwarded-Prefix: /evil\r\nAccept-Encoding: \xc3\xa9\r\n",
    ] {
        let (status, _, body) = wire(port, head);
        assert_eq!((status, String::from_utf8_lossy(&body).to_string()), (200, want.to_string()), "{}", String::from_utf8_lossy(head));
    }
}

/// A service run apart (`--as-service`, which the app reaches by `--service-url`) that shares the
/// app's secret takes the app's signed word of headers past ASCII: a UTF-8 cookie beside its
/// session, and `X-Note: café`, reach it as sent, the session too; a word a client sends it
/// straight, unsigned or signed six minutes ago, is not taken; nor is one signed for another path,
/// another query, another method or another service (its prefix), where a word signed with the
/// secret was taken on any request; one signed for the request is (src/service.rs `headers_word`,
/// `headers_bound` and `headers_of_word`, src/server.rs `apps_headers`, FEEDBACK75.md follow-up found
/// by 0.8.9's coverage, Q-1174's review and recheck; coverage of 0.8.9; branch feedback75)
#[test]
fn a_service_run_apart_takes_the_apps_signed_word_of_headers() {
    signing();
    let d = dir("apart-utf8", &[("app.qwe", HEADERS_APP), ("shop.qwe", HEADERS_SHOP)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (_s, shop) = serve(&d, "shop.qwe", &["--as-service", "--base", "/shop"], &[("QUILLO_SERVICE_SECRET", SERVICE_SECRET)], "apart-shop.log");
    let url = format!("shop=http://127.0.0.1:{}", shop);
    let (_g, port) = serve(&d, "app.qwe", &["--service-url", &url], &[("QUILLO_SERVICE_SECRET", SERVICE_SECRET)], "apart-app.log");
    let a = support::send(port, "GET", "/shop/in", &[], "");
    let session = a.headers.iter().find(|(k, v)| k == "set-cookie" && v.starts_with("quillo_session=")).map(|(_, v)| v.split(';').next().unwrap().to_string()).unwrap_or_else(|| panic!("{} {:?}", a.status, a.headers));
    let line = format!("{}; theme=ünï", session);
    assert_eq!(support::send(port, "GET", "/shop/me", &[("Cookie", &line), ("X-Note", "café")], "").body, r#"{"user":"shopper","note":"café","theme":"ünï"}"#);
    // Straight to the service: a word of its own, an old one, or one for another request is nothing.
    let text = quillo::crypto::b64url(serde_json::json!([["Cookie", session], ["X-Note", "forged"]]).to_string().as_bytes());
    let signed = |t: u64, prefix: &str, method: &str, path: &str| format!("t={};sig={};{}", t, hmac_hex(&format!("quillo-headers-2\n{}\n{}\n{}\n{}\n{}", prefix, method, path, text, t)), text);
    let now = now_secs();
    let none = r#"{"user":null,"note":null,"theme":null}"#;
    for (word, path) in [
        (text.clone(), "/me"),
        (format!("t={};sig={};{}", now, "0".repeat(64), text), "/me"),
        (signed(now - 400, "/shop", "GET", "/me"), "/me"),
        (signed(now, "/shop", "GET", "/note"), "/me"),
        (signed(now, "/shop", "GET", "/me"), "/me?x=1"),
        (signed(now, "/shop", "POST", "/me"), "/me"),
        (signed(now, "/other", "GET", "/me"), "/me"),
    ] {
        assert_eq!(support::send(shop, "GET", path, &[("x-quillo-headers", &word)], "").body, none, "{} {}", path, word);
    }
    let shopper = r#"{"user":"shopper","note":"forged","theme":null}"#;
    assert_eq!(support::send(shop, "GET", "/me", &[("x-quillo-headers", &signed(now, "/shop", "GET", "/me"))], "").body, shopper);
    assert_eq!(support::send(shop, "GET", "/me?x=1", &[("x-quillo-headers", &signed(now, "/shop", "GET", "/me?x=1"))], "").body, shopper);
}

/// A minimal WebSocket client's frame: masked, text unless `op` says.
fn ws_frame(data: &[u8], op: u8) -> Vec<u8> {
    let mut f = vec![0x80 | op];
    match data.len() {
        n if n < 126 => f.push(0x80 | n as u8),
        n if n < 65536 => {
            f.push(0x80 | 126);
            f.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            f.push(0x80 | 127);
            f.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    let mask = [7u8, 21, 42, 99];
    f.extend_from_slice(&mask);
    f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    f
}

/// Under `--split --max-memory 64KB`, a service's channel: the app hands the service its batch as
/// bytes, which it reads as the app reads its own; messages of 1,000 to 65,000 letters and one after
/// them each reach the service's handler or are counted as dropped, where the three under 64 KB came
/// and the rest, the one after them too, were lost uncounted (src/wt.rs `forward`, src/service.rs
/// `signed_post_as`, FEEDBACK75.md follow-up found by 0.8.9's review of Q-1172; coverage of 0.8.9;
/// branch feedback75)
#[test]
fn a_split_services_channel_batch_is_taken_or_counted() {
    let chat = "let heard = []\nchannel.on(\"chat\", (m, p) => {\n    heard = heard + [len(str(m))]\n}, { join: (peer) => true })\nroute GET /heard = heard\n";
    let d = dir("split-channel", &[("app.qwe", "service chat = \"chat.qwe\"\nroute GET / = \"app\"\n"), ("chat.qwe", chat)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (_g, port) = serve(&d, "app.qwe", &["--split", "--no-wt", "--metrics", "--max-memory", "64KB"], &[], "channel.log");
    let log = log_holding(&d.join("channel.log"), "service chat (chat.qwe) on http://127.0.0.1:", 20 * SLOWER);
    let shop: u16 = log.split("service chat (chat.qwe) on http://127.0.0.1:").nth(1).and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next()).and_then(|p| p.parse().ok()).unwrap_or_else(|| panic!("{}", log));
    let end = Instant::now() + Duration::from_secs(30 * SLOWER);
    while !support::listening(shop) {
        assert!(Instant::now() < end, "{}", log);
        std::thread::sleep(Duration::from_millis(50));
    }
    let a = support::send(port, "POST", "/_quillo/wt-ticket", &[("Sec-Fetch-Site", "same-origin")], "");
    let ticket = serde_json::from_str::<serde_json::Value>(&a.body).ok().and_then(|v| v["ticket"].as_str().map(str::to_string)).unwrap_or_else(|| panic!("{} {}", a.status, a.body));
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30 * SLOWER))).unwrap();
    write!(s, "GET /_quillo/wt?t={} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", ticket).unwrap();
    let head = read_until(&mut s, b"\r\n\r\n");
    assert!(head.starts_with(b"HTTP/1.1 101"), "{}", String::from_utf8_lossy(&head));
    s.write_all(&ws_frame(br#"["j","chat"]"#, 1)).unwrap();
    let joined = read_until(&mut s, b"true]");
    assert!(String::from_utf8_lossy(&joined).contains(r#"["j","chat",true]"#), "{:?}", String::from_utf8_lossy(&joined));
    let sizes = [1000usize, 30_000, 50_000, 65_000];
    for n in sizes {
        s.write_all(&ws_frame(serde_json::json!(["m", "chat", "a".repeat(n)]).to_string().as_bytes(), 1)).unwrap();
    }
    s.write_all(&ws_frame(br#"["m","chat","after"]"#, 1)).unwrap();
    // Each heard or counted, "after" heard.
    let dropped = || {
        let m = support::send(port, "GET", "/_quillo/metrics", &[], "").body;
        m.lines().find_map(|l| l.strip_prefix("quillo_channel_dropped_total ")).and_then(|n| n.trim().parse::<usize>().ok()).unwrap_or(0)
    };
    let end = Instant::now() + Duration::from_secs(30 * SLOWER);
    let heard = loop {
        let heard: Vec<usize> = serde_json::from_str(&support::send(shop, "GET", "/heard", &[], "").body).unwrap_or_default();
        if heard.last() == Some(&5) || Instant::now() > end {
            break heard;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(heard.last(), Some(&5), "{:?}", heard);
    let big: Vec<usize> = heard[..heard.len() - 1].to_vec();
    assert!(big.iter().all(|n| sizes.contains(n)) && big.windows(2).all(|w| w[0] < w[1]), "{:?}", heard);
    assert_eq!(big.len() + dropped(), sizes.len(), "{:?}", heard);
    assert!(big.starts_with(&[1000, 30_000, 50_000]), "{:?}", heard);
}

/// A service that resets a connection kept idle (as a Quillo server's kernel does, about a minute
/// after it closed one idle for 60 s): the app's next request after an idle spell goes on a new
/// connection, a 200, where it took the kept one and answered 502 "Connection reset by peer"; one
/// soon after the last goes on the kept connection, as before. The times scale with the build: the
/// idle time the app is given, the service's reset and the spell, and the kept connection is
/// asserted only where the two requests came well within the app's half of the idle time
/// (src/service.rs `pass_agent`, FEEDBACK75.md follow-up found by 0.8.9's review, Q-1097; coverage of
/// 0.8.9; branch feedback75)
#[test]
fn a_service_connection_kept_idle_is_not_used_again() {
    // A Quillo server's idle time: 1.5 s, 15 s in an unoptimized build; the app keeps a service's
    // connection for half of it, and the test's service resets one idle for two thirds of it.
    let idle = Duration::from_millis(1500 * SLOWER);
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fake = l.local_addr().unwrap().port();
    let conns = Arc::new(AtomicUsize::new(0));
    let counted = conns.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            counted.fetch_add(1, SeqCst);
            std::thread::spawn(move || {
                s.set_read_timeout(Some(idle * 2 / 3)).unwrap();
                loop {
                    let head = read_until(&mut s, b"\r\n\r\n");
                    if !head.ends_with(b"\r\n\r\n") {
                        resets_when_closed(&s);
                        return;
                    }
                    if s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok").is_err() {
                        return;
                    }
                }
            });
        }
    });
    let d = dir("idle-service", &[("app.qwe", "service /shop = \"shop.qwe\"\nroute GET / = \"app\"\n"), ("shop.qwe", "route GET / = \"shop\"\n")]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let url = format!("shop=http://127.0.0.1:{}", fake);
    let ms = idle.as_millis().to_string();
    let (_g, port) = serve(&d, "app.qwe", &["--service-url", &url], &[("QUILLO_IDLE_MS_FOR_TESTS", ms.as_str())], "idle.log");
    let get = || {
        let a = support::send(port, "GET", "/shop/x", &[], "");
        (a.status, a.body)
    };
    let t = Instant::now();
    assert_eq!(get(), (200, "ok".to_string()));
    let gap = t.elapsed();
    assert_eq!(get(), (200, "ok".to_string()));
    if gap < idle / 4 {
        assert_eq!(conns.load(SeqCst), 1, "two requests {:?} apart: one kept connection", gap);
    }
    // Past the service's reset and the app's half: a new connection, never the reset one.
    let before = conns.load(SeqCst);
    std::thread::sleep(idle * 4 / 3);
    assert_eq!(get(), (200, "ok".to_string()));
    assert_eq!(conns.load(SeqCst), before + 1);
}

/// A recording server: each request's head as it came, answered with `answer`.
fn recording(answer: &'static [u8]) -> (String, std::sync::mpsc::Receiver<String>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let got = read_until(&mut s, b"\r\n\r\n");
            let _ = tx.send(String::from_utf8_lossy(&got).into_owned());
            let _ = s.write_all(answer);
        }
    });
    (url, rx)
}

/// The services' shared secret, for the tests that sign or check an app's word in this process;
/// set once (the servers started after it inherit it, as an app's services do).
fn signing() {
    static SET: std::sync::Once = std::sync::Once::new();
    SET.call_once(|| std::env::set_var("QUILLO_SERVICE_SECRET", SERVICE_SECRET));
}

/// What an app passes on for a visitor's headers past ASCII: the ASCII ones as they are, a `Cookie`
/// line's ASCII cookies too, the service's session among them, those past ASCII in the word
/// `x-quillo-headers` (signed, the base64url of their names and values, in their order), a control
/// character in none; and no 502 it gives writes back a header (the service's session, a UTF-8
/// cookie, `café`) or the status line a service sent: a service that is not there, one whose answer
/// is not HTTP (src/service.rs `pass_on`, `sendable`, `headers_word` and `unanswered`, FEEDBACK75.md
/// follow-up found by 0.8.9's coverage, Q-1174 and its review; coverage of 0.8.9; branch feedback75)
#[test]
fn a_passed_on_request_carries_its_headers_and_its_502_writes_none_back() {
    let _one = serial();
    signing();
    let d = support::temp_dir("cover089-server-carried");
    let _gone = Gone(d.clone());
    let mut it = Interp::new(&d);
    let hs = [("Cookie", "quillo_session=SECRET-SESSION-1; theme=ünï"), ("X-Note", "café"), ("X-Ctl", "a\u{1}SECRET-CTL"), ("Accept", "text/plain"), ("X-Note", "two")];
    let req = request("GET", "/shop/page?x=1", &hs, b"");
    let (url, heads) = recording(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    let r = quillo::service::pass_on(&mut it, &url, "/shop", &req, None);
    assert_eq!((r.status, r.text()), (200, "ok".to_string()));
    let head = heads.recv_timeout(Duration::from_secs(10 * SLOWER)).unwrap();
    assert!(head.starts_with("GET /page?x=1 HTTP/1.1\r\n") && head.contains("\r\nAccept: text/plain\r\n"), "{}", head);
    assert!(head.contains("\r\nCookie: quillo_session=SECRET-SESSION-1\r\n"), "{}", head);
    assert!(!head.contains("café") && !head.contains("ünï") && !head.contains("SECRET-CTL") && !head.to_ascii_lowercase().contains("x-ctl"), "{}", head);
    let word = head.lines().find_map(|l| l.strip_prefix("x-quillo-headers: ")).unwrap_or_else(|| panic!("{}", head)).trim();
    let mut parts = word.splitn(3, ';');
    let (t, sig, text) = (parts.next().unwrap().strip_prefix("t=").unwrap(), parts.next().unwrap().strip_prefix("sig=").unwrap(), parts.next().unwrap());
    assert_eq!(sig, hmac_hex(&format!("quillo-headers-2\n/shop\nGET\n/page?x=1\n{}\n{}", text, t)));
    let carried: Vec<(String, String)> = serde_json::from_slice(&quillo::crypto::unb64url(text).unwrap()).unwrap();
    let want: Vec<(String, String)> = [("Cookie", "theme=ünï"), ("X-Note", "café"), ("X-Note", "two")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    assert_eq!(carried, want);
    // All in ASCII: no word.
    let (url, heads) = recording(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
    let plain = request("GET", "/shop/", &[("Cookie", "quillo_session=s1"), ("X-Note", "plain")], b"");
    assert_eq!(quillo::service::pass_on(&mut it, &url, "/shop", &plain, None).status, 204);
    let head = heads.recv_timeout(Duration::from_secs(10 * SLOWER)).unwrap();
    assert!(head.contains("\r\nCookie: quillo_session=s1\r\n") && head.contains("\r\nX-Note: plain\r\n") && !head.contains("x-quillo-headers"), "{}", head);
    // Not there, and not HTTP: the reason, never a header or what the service said.
    let gone = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", l.local_addr().unwrap())
    };
    let (bad, _) = recording(b"HTTP/1.1 2x0 SECRET-STATUS\r\nSet-Cookie: quillo_session=SECRET-SESSION-2\r\n\r\n");
    for url in [gone, bad] {
        let r = quillo::service::pass_on(&mut it, &url, "/shop", &req, None);
        let text = r.text();
        assert!(r.status == 502 && text.starts_with("this part of the site is not answering ("), "{} {}", r.status, text);
        for secret in ["SECRET", "ünï", "café", "quillo_session", "2x0"] {
            assert!(!text.contains(secret), "{}: {}", secret, text);
        }
    }
}

/// The repro: under `--split --max-memory 512KB`, a service's answer of 700 KB was a 502, where one
/// of 2 MB streamed through; now each answer over the limit, or over 1 MiB, streams through whole:
/// one byte under the limit, at it and one byte over it, 700 KB and 2 MB, each its bytes
/// (src/service.rs `pass_on`, FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1175; coverage of
/// 0.8.9; branch feedback75)
#[test]
fn a_service_answer_past_a_small_memory_limit_streams_whole() {
    let shop = "route GET / = \"shop\"\nroute GET /a = file(\"a.bin\")\nroute GET /b = file(\"b.bin\")\nroute GET /c = file(\"c.bin\")\nroute GET /d = file(\"d.bin\")\nroute GET /e = file(\"e.bin\")\n";
    let d = dir("split-small-memory", &[("app.qwe", "service /shop = \"shop.qwe\"\nroute GET / = \"app\"\n"), ("shop.qwe", shop)]);
    let _gone = Gone(d.clone());
    let sizes = [("a", (512 << 10) - 1, b'a'), ("b", 512 << 10, b'b'), ("c", (512 << 10) + 1, b'c'), ("d", 700 << 10, b'd'), ("e", 2 << 20, b'e')];
    for (name, n, byte) in sizes {
        std::fs::write(d.join(format!("{}.bin", name)), vec![byte; n]).unwrap();
    }
    checked(&d, "app.qwe");
    let (_g, port) = split(&d, &["--max-memory", "512KB"], "small-memory.log");
    for (name, n, byte) in sizes {
        let (status, _, body) = wire(port, format!("GET /shop/{} HTTP/1.1\r\n", name).as_bytes());
        assert_eq!((status, body.len()), (200, n), "{}", name);
        assert!(body.iter().all(|b| *b == byte), "{}", name);
    }
}

// ---- httpd.rs and service.rs: a service's words from its app, over IPv6 -----------------------------

/// No comma: a comma parts secrets being rotated.
const SERVICE_SECRET: &str = "a service secret for the cover089 tests and 32 bytes more";

/// A service as its app starts one (`--as-service`, told to stop with its app), listening on
/// `[::1]`, killed when dropped: its stdin is the app's line to it.
struct Service {
    child: Child,
    _stdin: ChildStdin,
    port: u16,
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn service6(d: &Path, file: &str, log: &str) -> Service {
    let out = std::fs::File::create(d.join(log)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", file, "--port", "0", "--host", "::1", "--as-service", "--service-name", "shop", "--base", "/shop"])
        .current_dir(d)
        .env("QUILLO_STOP_WITH_APP", "1")
        .env("QUILLO_SERVICE_SECRET", SERVICE_SECRET)
        .stdin(Stdio::piped())
        .stdout(out.try_clone().unwrap())
        .stderr(out)
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let stdin = child.stdin.take().unwrap();
    let mut s = Service { child, _stdin: stdin, port: 0 };
    let end = Instant::now() + Duration::from_secs(20 * SLOWER);
    loop {
        if let Some(p) = support::port_said_in(&d.join(log)) {
            if TcpStream::connect(("::1", p)).is_ok() {
                s.port = p;
                return s;
            }
        }
        if let Ok(Some(status)) = s.child.try_wait() {
            panic!("the service stopped: {}\n{}", status, std::fs::read_to_string(d.join(log)).unwrap_or_default());
        }
        assert!(Instant::now() < end, "the service did not start: {}", std::fs::read_to_string(d.join(log)).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A request to `[::1]:port`: its status, headers (lower case) and body.
fn send6(port: u16, path: &str, headers: &[(&str, String)]) -> (u16, Vec<(String, String)>, String) {
    let mut s = TcpStream::connect(("::1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30 * SLOWER))).unwrap();
    let mut head = format!("GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n", path);
    for (k, v) in headers {
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let mut lines = head.lines();
    let status = lines.next().and_then(|l| l.split(' ').nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
    let hs = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    (status, hs, body.to_string())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

fn hmac_hex(text: &str) -> String {
    quillo::crypto::hex(&quillo::crypto::hmac_sha256(SERVICE_SECRET.as_bytes(), text.as_bytes()))
}

/// The app's word on the visitor's address, as `service::client_word` signs it, made at `t`.
fn client_word(ip: &str, t: u64) -> String {
    format!("{};t={};sig={}", ip, t, hmac_hex(&format!("quillo-client-1\n{}\n{}", ip, t)))
}

/// The app's word on the visitor's identity (`service::identity_word`), made at `t`.
fn identity_word(json: &str, t: u64) -> String {
    let text = quillo::crypto::b64url(json.as_bytes());
    format!("t={};sig={};{}", t, hmac_hex(&format!("quillo-identity-1\n{}\n{}", text, t)), text)
}

/// The app's word on its policies (`service::policy_word`), made at `t`.
fn policy_word(policies: &str, t: u64) -> String {
    let text = quillo::crypto::b64url(policies.as_bytes());
    format!("t={};sig={};{}", t, hmac_hex(&format!("quillo-policy-1\n{}\n{}", text, t)), text)
}

/// A service its app started takes the app's signed words from a peer on this machine over IPv6
/// too (`::1`): the visitor's address, who the visitor is (a null certificate read as null) and
/// the app's policies, each within the five minutes a word is good for; a word made six minutes
/// ago, or with a part more, says nothing, and a policy or identity word comes to nothing without
/// a good address word (src/httpd.rs `from_app`, src/service.rs `client_of_word`,
/// `identity_of_word` and `policies_of_word`, src/server.rs `proxy_identity`, FEEDBACK75.md Q-1064
/// and Q-1087, FEEDBACK78.md A.13 and A.14; coverage of 0.8.9; branch feedback75)
#[test]
fn a_service_takes_its_apps_words_over_ipv6_while_they_are_fresh() {
    if std::net::TcpListener::bind("[::1]:0").is_err() {
        eprintln!("no IPv6 loopback here: skipped");
        return;
    }
    let shop = "route GET / = \"shop\"\nroute GET /who = { ip: request.ip, cert: request.client_cert, user: request.remote_user }\nroute GET /page = <html><body><p>page</p></body></html>\n";
    let d = dir("words6", &[("shop.qwe", shop)]);
    let _gone = Gone(d.clone());
    checked(&d, "shop.qwe");
    let s = service6(&d, "shop.qwe", "words6.log");
    let now = now_secs();
    let who = |hs: &[(&str, String)]| send6(s.port, "/who", hs).2;
    let fresh = client_word("203.0.113.9", now);
    assert_eq!(who(&[]), r#"{"ip":"::1","cert":null,"user":null}"#);
    assert_eq!(who(&[("x-quillo-client", fresh.clone())]), r#"{"ip":"203.0.113.9","cert":null,"user":null}"#);
    assert_eq!(who(&[("x-quillo-client", client_word("2001:db8::7", now - 290))]), r#"{"ip":"2001:db8::7","cert":null,"user":null}"#);
    assert_eq!(who(&[("x-quillo-client", client_word("203.0.113.9", now - 400))]), r#"{"ip":"::1","cert":null,"user":null}"#);
    assert_eq!(who(&[("x-quillo-client", format!("{};x", fresh))]), r#"{"ip":"::1","cert":null,"user":null}"#);
    // Who the visitor is: a null certificate and a user; stale, nothing.
    let ann = r#"{"cert":null,"user":"ünï ann"}"#;
    assert_eq!(who(&[("x-quillo-client", fresh.clone()), ("x-quillo-identity", identity_word(ann, now))]), r#"{"ip":"203.0.113.9","cert":null,"user":"ünï ann"}"#);
    assert_eq!(who(&[("x-quillo-client", fresh.clone()), ("x-quillo-identity", identity_word(ann, now - 400))]), r#"{"ip":"203.0.113.9","cert":null,"user":null}"#);
    assert_eq!(who(&[("x-quillo-client", client_word("203.0.113.9", now - 400)), ("x-quillo-identity", identity_word(ann, now))]), r#"{"ip":"::1","cert":null,"user":null}"#);
    // The app's policy: taken below the page's own, its scripts let through; stale, not taken.
    let policy = r#"[["content-security-policy","default-src 'self'"]]"#;
    let (status, hs, body) = send6(s.port, "/page", &[("x-quillo-client", fresh.clone()), ("x-quillo-policy", policy_word(policy, now))]);
    let csp = hs.iter().find(|(k, _)| k == "content-security-policy").map(|(_, v)| v.clone());
    assert!(status == 200 && body.contains("<p>page</p>") && csp.as_deref().is_some_and(|c| c.starts_with("default-src 'self'")), "{} {:?} {}", status, hs, body);
    let (status, hs, _) = send6(s.port, "/page", &[("x-quillo-client", fresh), ("x-quillo-policy", policy_word(policy, now - 400))]);
    assert!(status == 200 && !hs.iter().any(|(k, _)| k == "content-security-policy"), "{:?}", hs);
}

// ---- server.rs: `site_headers` rules, mounted programs, the markdown door, server fns ---------------

/// A `site_headers` rule written with `%` escapes or a doubled `/` covers the path routes read it
/// as (`/caf%C3%A9` is `/café`, `/a//b` is `/a/b`), and only that path and those below it; a `%2F`
/// stays inside its piece (src/server.rs `site_rules`, FEEDBACK75.md Q-1087; coverage of 0.8.9;
/// branch feedback75)
#[test]
fn a_rule_written_another_way_covers_the_path_routes_read() {
    let _one = serial();
    let src = "site_headers(\"/caf%C3%A9\", { \"x-menu\": \"café\" })\nsite_headers(\"/a//b\", { \"x-ab\": \"1\" })\nsite_headers(\"/pay%2Freturn\", { \"x-pay\": \"1\" })\nroute GET /caf%C3%A9 = \"menu\"\nroute GET /caf%C3%A9/today = \"today\"\nroute GET /cafe = \"plain\"\nroute GET /a/b = \"ab\"\nroute GET /a/bc = \"abc\"\nroute GET /pay/return = \"split\"\n";
    let d = dir("rules-escaped", &[("main.qwe", src)]);
    let _gone = Gone(d.clone());
    checked(&d, "main.qwe");
    let mut it = app(src);
    let mut get = |url: &str, name: &str| {
        let r = it.handle(&request("GET", url, &[], b""));
        (r.status, header(&r, name).map(str::to_string))
    };
    assert_eq!(get("/caf%C3%A9", "x-menu"), (200, Some("café".into())));
    assert_eq!(get("/caf%C3%A9/today", "x-menu"), (200, Some("café".into())));
    assert_eq!(get("/cafe", "x-menu"), (200, None));
    assert_eq!(get("/a/b", "x-ab"), (200, Some("1".into())));
    assert_eq!(get("/a/bc", "x-ab"), (200, None));
    assert_eq!(get("/pay/return", "x-pay"), (200, None));
}

/// A program mounted inside a mounted program gets both programs' `site_headers`, the nearer one's
/// winning where both name a header; a rule of the outer program that takes a header away (null)
/// adds nothing to the inner pages and keeps Quillo's `X-Frame-Options` off them (src/server.rs
/// `rules_for_mount` and `parent_headers_into`, FEEDBACK75.md Q-1087; coverage of 0.8.9; branch
/// feedback75)
#[test]
fn a_mount_inside_a_mount_gets_both_programs_headers() {
    let top = "site_headers({ \"x-top\": \"top\", \"referrer-policy\": \"same-origin\", \"x-frame-options\": null })\nmount /mid = \"mid.qwe\"\nroute GET / = \"app\"\n";
    let mid = "site_headers({ \"x-mid\": \"mid ünï\", \"referrer-policy\": \"no-referrer\" })\nmount /leaf = \"leaf.qwe\"\nroute GET / = \"mid\"\n";
    let leaf = "route GET / = \"leaf\"\nroute GET /page = <html><body><p>leaf page</p></body></html>\n";
    let d = dir("mounts", &[("app.qwe", top), ("mid.qwe", mid), ("leaf.qwe", leaf)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (_g, port) = serve(&d, "app.qwe", &[], &[], "mounts.log");
    let get = |p: &str| support::send(port, "GET", p, &[], "");
    let a = get("/mid/leaf/");
    assert_eq!(a.body, "leaf");
    assert_eq!((a.header("x-top"), a.header("x-mid"), a.header("referrer-policy")), (Some("top"), Some("mid ünï"), Some("no-referrer")), "{:?}", a.headers);
    let a = get("/mid/leaf/page");
    assert!(a.body.contains("leaf page"), "{}", a.body);
    assert_eq!((a.header("x-top"), a.header("x-mid"), a.header("x-frame-options")), (Some("top"), Some("mid ünï"), None), "{:?}", a.headers);
    let a = get("/mid/");
    assert_eq!((a.body.as_str(), a.header("x-top"), a.header("referrer-policy")), ("mid", Some("top"), Some("no-referrer")));
    let a = get("/");
    assert_eq!((a.body.as_str(), a.header("x-mid"), a.header("referrer-policy")), ("app", None, Some("same-origin")));
}

/// The markdown door: a text whose JSON alone is larger than `--max-memory` is the limit's stop
/// before anything is made; a first argument that is not a text is refused as `markdown` refuses
/// it, a 400, as are options that are not a map; ordinary text as before (src/server.rs
/// `handle_internal`, FEEDBACK75.md Q-1069; coverage of 0.8.9; branch feedback75)
#[test]
fn the_markdown_door_refuses_what_it_cannot_take() {
    let _one = serial();
    let mut it = app("route GET / = \"hi\"\n");
    let json = [("Content-Type", "application/json")];
    let mut md = |body: &[u8]| {
        let r = it.handle(&request("POST", "/_quillo/markdown", &json, body));
        (r.status, r.text())
    };
    for bad in [&b"[5]"[..], b"[null]", b"[[\"a\"]]"] {
        let (status, text) = md(bad);
        assert!(status == 400 && text.contains("`markdown`"), "{:?}: {} {}", String::from_utf8_lossy(bad), status, text);
    }
    let (status, text) = md(br#"["*a*", 3]"#);
    assert!(status == 400 && text.contains("`markdown` expects options as a map, got int"), "{} {}", status, text);
    {
        let _limit = Limit::set(64 << 10);
        let big = serde_json::json!(["ü".repeat(60_000)]).to_string();
        assert_eq!(md(big.as_bytes()), (500, r#"{"error":"this request's value would be bigger than --max-memory allows (65536 bytes)"}"#.to_string()));
        assert_eq!(md(r#"["ünï *a*"]"#.as_bytes()), (200, r#"{"ok":"<p>ünï <em>a</em></p>\n"}"#.to_string()));
    }
    assert_eq!(md(r#"["ünï *a*"]"#.as_bytes()), (200, r#"{"ok":"<p>ünï <em>a</em></p>\n"}"#.to_string()));
}

/// A server fn's call with a file, under `--max-memory`, whose body the request's start read (it
/// is 1,260 bytes or more): its arguments and its file come from that reading, the file's name in
/// unicode, the other arguments as sent; a small call, which the start does not read, answers the
/// same way (src/server.rs `calls`, FEEDBACK75.md Q-1052; coverage of 0.8.9; branch feedback75)
#[test]
fn a_call_with_a_file_under_a_memory_limit_uses_the_body_read_at_its_start() {
    let src = "server fn named(f: map, n: int) -> str {\n    return `${f.filename} ${f.size} ${n}`\n}\nroute GET / = \"x\"\n";
    let d = dir("rpc-file", &[("main.qwe", src)]);
    let _gone = Gone(d.clone());
    checked(&d, "main.qwe");
    let (_g, port) = serve(&d, "main.qwe", &["--max-memory", "64MB"], &[], "rpc-file.log");
    let call = |content: &str| {
        let body = format!(
            "--BB\r\nContent-Disposition: form-data; name=\"args\"\r\n\r\n[{{\"$quillo_file\": 0}}, 7]\r\n--BB\r\nContent-Disposition: form-data; name=\"file0\"; filename=\"ünï.txt\"\r\nContent-Type: text/plain\r\n\r\n{}\r\n--BB--\r\n",
            content
        );
        let a = support::send(port, "POST", "/_quillo/rpc/named", &[("Content-Type", "multipart/form-data; boundary=BB"), ("x-quillo-rpc", "1"), ("Sec-Fetch-Site", "same-origin")], &body);
        (a.status, a.body)
    };
    assert_eq!(call(&"z".repeat(2000)), (200, r#"{"ok":"ünï.txt 2000 7"}"#.to_string()));
    assert_eq!(call("hello"), (200, r#"{"ok":"ünï.txt 5 7"}"#.to_string()));
}

/// The request log hides a query value whose name has a word that marks a secret before its last
/// one, the words told by their capitals (`sessionExpiry`, `tokenType`, `passwordHint`, `jwtClaims`);
/// a name whose first word only looks like one (`sessionsCount`, `tokenizeMode`) is written out
/// (src/server.rs `hidden_in_log`, FEEDBACK78.md Q-1158; coverage of 0.8.9; branch feedback75)
#[test]
fn the_log_hides_a_secret_word_that_starts_a_name() {
    use quillo::server::log_url;
    for name in ["sessionExpiry", "tokenType", "passwordHint", "jwtClaims", "SecretValue", "credentialId", "verifierCode"] {
        assert_eq!(log_url(&format!("/a?page=2&{}=s3cr3t&q=ünï", name)), format!("/a?page=2&{}=[hidden]&q=ünï", name), "{}", name);
    }
    for name in ["sessionsCount", "tokenizeMode", "signedUp", "pageSize"] {
        let url = format!("/a?{}=v", name);
        assert_eq!(log_url(&url), url, "{}", name);
    }
}

// ---- interp.rs: the prefetch guard --------------------------------------------------------------

/// A test listener: each HTTP request line it got, answered "ok".
fn sink() -> (u16, std::sync::mpsc::Receiver<String>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { break };
            let tx = tx.clone();
            std::thread::spawn(move || {
                s.set_read_timeout(Some(Duration::from_secs(5))).ok();
                let got = read_until(&mut s, b"\r\n\r\n");
                let _ = tx.send(String::from_utf8_lossy(&got).lines().next().unwrap_or("").to_string());
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok");
            });
        }
    });
    (port, rx)
}

/// `http.stream(url, f)` with no options reads (a GET), so a prefetch runs it as before, as it
/// runs `http.stream` with `{ method: "GET" }`; a POST route runs it too (src/interp.rs
/// `changes_as_called`, FEEDBACK75.md Q-1062; coverage of 0.8.9; branch feedback75)
#[test]
fn a_stream_with_no_options_is_prefetched() {
    let _one = serial();
    let (web, got) = sink();
    let src = format!(
        "route GET /two {{\n    let r = http.stream(\"http://127.0.0.1:{web}/two\", (d) => d)\n    return <p>{{r.status}} ünï</p>\n}}\nroute POST /posted {{\n    http.stream(\"http://127.0.0.1:{web}/posted\", (d) => d)\n    return \"posted\"\n}}\n"
    );
    let d = dir("prefetch-two", &[("main.qwe", &src)]);
    let _gone = Gone(d.clone());
    checked(&d, "main.qwe");
    let mut it = app(&src);
    let prefetch = [("x-quillo-prefetch", "1"), ("sec-purpose", "prefetch")];
    let r = it.handle(&request("GET", "/two", &prefetch, b""));
    assert_eq!((r.status, r.text()), (200, "<p>200 ünï</p>".to_string()));
    assert_eq!(got.recv_timeout(Duration::from_secs(5 * SLOWER)).unwrap(), "GET /two HTTP/1.1");
    let r = it.handle(&request("POST", "/posted", &[("Content-Type", "application/json")], b"{}"));
    assert_eq!((r.status, r.text()), (200, "posted".to_string()));
    assert_eq!(got.recv_timeout(Duration::from_secs(5 * SLOWER)).unwrap(), "GET /posted HTTP/1.1");
}

// ---- fetch.rs: what all calls read at once ---------------------------------------------------------

/// What a process takes now, in KB: Linux's VmRSS, macOS's physical footprint.
fn now_kb(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let s = std::fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
        s.lines().find_map(|l| l.strip_prefix("VmRSS:")).and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: `ri` lives through the call, which only writes it, as large as the flavour says.
        let mut ri: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
        let r = unsafe { libc::proc_pid_rusage(pid as i32, libc::RUSAGE_INFO_V4, &mut ri as *mut _ as *mut libc::rusage_info_t) };
        (r == 0).then_some(ri.ri_phys_footprint / 1024)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

/// A server on a port of its own that answers each of `n` connections with `head` and `a`, then
/// waits until `go` says and `apart` more for each connection before it, then sends `b` and
/// closes: its port, and how many have had `a`.
fn held_upstream(n: usize, head: &'static str, a: Vec<u8>, b: Vec<u8>, go: Arc<AtomicBool>, apart: Duration) -> (u16, Arc<AtomicUsize>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let had = Arc::new(AtomicUsize::new(0));
    let had2 = had.clone();
    let (a, b) = (Arc::new(a), Arc::new(b));
    std::thread::spawn(move || {
        for i in 0..n {
            let Ok((mut s, _)) = l.accept() else { return };
            let (a, b, go, had) = (a.clone(), b.clone(), go.clone(), had2.clone());
            std::thread::spawn(move || {
                let _ = read_until(&mut s, b"\r\n\r\n");
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&a);
                had.fetch_add(1, SeqCst);
                let until = Instant::now() + Duration::from_secs(120 * SLOWER);
                while !go.load(SeqCst) && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(10));
                }
                std::thread::sleep(apart * i as u32);
                let _ = s.write_all(&b);
            });
        }
    });
    (port, had)
}

/// A server on a port of its own that answers one connection, once `ready` says, with a 404 of
/// 48 MiB: its port, and whether it has written it (or its client has gone).
fn missing_upstream(ready: Arc<AtomicBool>) -> (u16, Arc<AtomicBool>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let done = Arc::new(AtomicBool::new(false));
    let done2 = done.clone();
    std::thread::spawn(move || {
        let Ok((mut s, _)) = l.accept() else { return };
        let _ = read_until(&mut s, b"\r\n\r\n");
        let until = Instant::now() + Duration::from_secs(120 * SLOWER);
        while !ready.load(SeqCst) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = s.write_all(format!("HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", 48 << 20).as_bytes());
        let _ = s.write_all(&vec![b'm'; 48 << 20]);
        drop(s);
        done2.store(true, SeqCst);
    });
    (port, done)
}

/// `http.stream` of an answer that is not a success reads its body whole, held with every other
/// call's to the 256 MiB read at once: with seven answers of 32 MiB held (217 MiB counted), its
/// 404 of 48 MiB is the error value that says so, and the seven, let go one after another, end
/// whole. The 404 is sent once the seven have stopped reading, by the process's memory over its
/// own size, not by an absolute size (src/fetch.rs `stream`, FEEDBACK75.md Q-1072; coverage of
/// 0.8.9; branch feedback75)
#[test]
fn a_streams_failed_answer_is_held_to_what_is_read_at_once() {
    let d = dir("stream-busy", &[]);
    let _gone = Gone(d.clone());
    let go = Arc::new(AtomicBool::new(false));
    let ready = Arc::new(AtomicBool::new(false));
    // Let go 300 ms apart: each joins its pieces holding half as much again for a moment, which
    // for all seven at once would pass the budget.
    let apart = Duration::from_millis(300 * SLOWER);
    let (big, had) = held_upstream(7, "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 33554432\r\nConnection: close\r\n\r\n", vec![b'z'; (32 << 20) - 1], vec![b'z'; 1], go.clone(), apart);
    let (missing, sent) = missing_upstream(ready.clone());
    let src = format!(
        "print(\"ready\")\nwhile is_err(read_file(\"go\")) {{\n    sleep(10)\n}}\nlet held = () => {{\n    let r = http.get(\"http://127.0.0.1:{big}/big\")\n    return is_err(r) ? str(r) : `${{r.status}} ${{len(r.body)}}`\n}}\nlet missing = () => {{\n    let r = http.stream(\"http://127.0.0.1:{missing}/missing\", (chunk) => chunk)\n    return is_err(r) ? str(r) : `${{r.status}} ${{len(r.body)}}`\n}}\nfor r in parallel(held, held, held, held, held, held, held, missing) {{\n    print(r)\n}}\n"
    );
    std::fs::write(d.join("s.qwe"), &src).unwrap();
    checked(&d, "s.qwe");
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["run", "s.qwe"]).current_dir(&d).env("MIMALLOC_PURGE_DELAY", "0").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    support::stop_with_tests(&child);
    let pid = child.id();
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let guard = support::Guard(child);
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = out.read(&mut buf) {
            if n == 0 || tx.send(String::from_utf8_lossy(&buf[..n]).into_owned()).is_err() {
                break;
            }
        }
    });
    let mut said = String::new();
    let until = Instant::now() + Duration::from_secs(60 * SLOWER);
    while !said.contains("ready\n") && Instant::now() < until {
        if let Ok(more) = rx.recv_timeout(Duration::from_millis(50)) {
            said.push_str(&more);
        }
    }
    assert!(said.contains("ready\n"), "{}", said);
    let base = now_kb(pid).unwrap_or(0);
    std::fs::write(d.join("go"), "").unwrap();
    // Once the seven hold what they were sent, the 404 is sent: all seven have had it, and the
    // process's memory, over its own size before, has stopped growing (a second without 1 MiB
    // more, ten in an unoptimized build) past a floor well under what they hold (7 × 32 MiB, an
    // instrumented or other build counting it otherwise); before the client's own 60 s at the latest.
    let until = Instant::now() + Duration::from_secs(40);
    let mut last = (0u64, Instant::now());
    while Instant::now() < until {
        let grown = now_kb(pid).unwrap_or(0).saturating_sub(base);
        if grown > last.0 + 1024 {
            last = (grown, Instant::now());
        } else if had.load(SeqCst) == 7 && grown >= 7 * 16 * 1024 && last.1.elapsed() > Duration::from_secs(SLOWER) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    ready.store(true, SeqCst);
    // The seven are let go once the 404 has been read, or refused.
    let until = Instant::now() + Duration::from_secs(20 * SLOWER);
    while !sent.load(SeqCst) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    go.store(true, SeqCst);
    let until = Instant::now() + Duration::from_secs(120 * SLOWER);
    while said.lines().filter(|l| *l != "ready").count() < 8 && Instant::now() < until {
        if let Ok(more) = rx.recv_timeout(Duration::from_millis(50)) {
            said.push_str(&more);
        }
    }
    drop(guard);
    let mut stderr = String::new();
    let _ = err.read_to_string(&mut stderr);
    let lines: Vec<&str> = said.lines().filter(|l| *l != "ready").collect();
    let whole = format!("200 {}", 32 << 20);
    assert_eq!(lines.len(), 8, "{}\n{}", said, stderr);
    assert!(lines[..7].iter().all(|l| *l == whole), "{}", said);
    assert!(
        lines[7].contains(&format!("http: http://127.0.0.1:{}/missing: the answers being read at once would take more than the 256 MiB read at once; try again shortly, or read it with http.stream", missing)),
        "{}",
        said
    );
}
