//! The HTTP server's corners no other test reaches (src/httpd.rs): statuses seldom sent,
//! connections that time out or linger, a gzipped body that comes chunked, proxies named by
//! IPv6 addresses, a server out of descriptors with several instances or on a Unix socket,
//! and requests whose coroutines wait on no children or on a child that panics.

mod support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn until_for(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn until(f: impl FnMut() -> bool) -> bool {
    until_for(20, f)
}

struct Srv {
    child: Child,
    port: u16,
    dir: PathBuf,
}

impl Drop for Srv {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Srv {
    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("stderr.log")).unwrap_or_default()
    }

    fn get(&self, path: &str) -> support::Answer {
        support::send(self.port, "GET", path, &[], "")
    }

    fn connections(&self) -> i64 {
        self.get("/_quillo/metrics").body.lines().find_map(|l| l.strip_prefix("quillo_connections ")).and_then(|n| n.parse().ok()).unwrap_or(-1)
    }
}

fn folder(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("cover-httpd-{}", name)).canonicalize().unwrap();
    std::fs::write(d.join("app.qwe"), src).unwrap();
    d
}

/// `quillo run app.qwe` in `d` on a free port, with at most `descriptors` file descriptors when
/// given; `wait` false for a server that listens elsewhere than its port.
fn run(d: &Path, args: &[&str], descriptors: Option<u32>, wait: bool) -> Srv {
    let port = support::free_port();
    let bin = env!("CARGO_BIN_EXE_quillo");
    let mut c = match descriptors {
        Some(n) => {
            let mut c = Command::new("sh");
            c.args(["-c", &format!("ulimit -n {}; exec \"$0\" \"$@\"", n), bin]);
            c
        }
        None => Command::new(bin),
    };
    let child = c
        .args(["run", "app.qwe", "--port", &port.to_string(), "--metrics"])
        .args(args)
        .current_dir(d)
        .env("QUILLO_SECRET", "a test secret of more than thirty-two bytes")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(d.join("stderr.log")).unwrap())
        .spawn()
        .expect("cannot start quillo");
    support::stop_with_tests(&child);
    let s = Srv { child, port, dir: d.to_path_buf() };
    if wait {
        assert!(until(|| support::listening(port)), "the server did not start: {}", s.log());
    }
    s
}

fn connect(port: u16) -> TcpStream {
    let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    s
}

/// What arrives until the server closes, or the read times out.
fn all(s: &mut TcpStream) -> String {
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

const APP: &str = r#"
route GET / = "hi"
route GET /s/:n = respond(int(n), "x")
route POST /echo = `${request.headers["content-length"]}:${request.body}`
route GET /ip = request.ip
route GET /big = repeat("b", 40000000)
"#;

// ---- what is written ------------------------------------------------------------------

/// Each status a program may answer with goes with its reason phrase from RFC 9110 (and the
/// WebDAV and other RFCs that named them), the rarer ones too.
#[test]
fn a_rarely_used_status_is_sent_with_its_reason_phrase() {
    let srv = run(&folder("reasons", APP), &[], None, true);
    let rare = [
        (203, "Non-Authoritative Information"),
        (300, "Multiple Choices"),
        (402, "Payment Required"),
        (406, "Not Acceptable"),
        (407, "Proxy Authentication Required"),
        (421, "Misdirected Request"),
        (423, "Locked"),
        (424, "Failed Dependency"),
        (425, "Too Early"),
        (426, "Upgrade Required"),
        (428, "Precondition Required"),
        (451, "Unavailable For Legal Reasons"),
        (505, "HTTP Version Not Supported"),
        (507, "Insufficient Storage"),
        (508, "Loop Detected"),
        (511, "Network Authentication Required"),
    ];
    for (status, reason) in rare {
        let mut s = connect(srv.port);
        write!(s, "GET /s/{} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n", status).unwrap();
        let got = all(&mut s);
        assert!(got.starts_with(&format!("HTTP/1.1 {} {}\r\n", status, reason)), "{}: {}", status, got);
        assert!(got.ends_with("\r\n\r\nx"), "{}: {}", status, got);
    }
}

// ---- what is read ---------------------------------------------------------------------

/// A body both chunked and gzipped is decoded twice, and the program is told the length it
/// ends up with, there being none in the request to correct.
#[test]
fn a_gzipped_body_sent_in_chunks_is_given_its_decoded_length() {
    use flate2::write::GzEncoder;
    let srv = run(&folder("gzip-chunked", APP), &[], None, true);
    let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(b"hello, compressed and chunked").unwrap();
    let gz = enc.finish().unwrap();
    let (a, b) = gz.split_at(gz.len() / 2);
    let mut s = connect(srv.port);
    write!(s, "POST /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: text/plain\r\nContent-Encoding: gzip\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap();
    for part in [a, b] {
        write!(s, "{:x}\r\n", part.len()).unwrap();
        s.write_all(part).unwrap();
        s.write_all(b"\r\n").unwrap();
    }
    s.write_all(b"0\r\n\r\n").unwrap();
    let got = all(&mut s);
    assert!(got.starts_with("HTTP/1.1 200"), "{}", got);
    assert!(got.ends_with("\r\n\r\n29:hello, compressed and chunked"), "{}", got);
}

/// Hops in `X-Forwarded-For` written in IPv6, or as IPv4 mapped into IPv6, are compared with
/// the proxies named in the same family: an IPv4-mapped hop is its IPv4 address, and an IPv6 one
/// is never an IPv4 proxy.
#[test]
fn forwarded_hops_in_ipv6_are_matched_against_proxies_of_their_own_family() {
    let srv = run(&folder("proxy-v6", APP), &["--trust-proxy", "127.0.0.1,::1,fd00::/8"], None, true);
    let seen = |xff: &str| support::send(srv.port, "GET", "/ip", &[("x-forwarded-for", xff)], "").body;
    // ::1 is a proxy (IPv6 to IPv6); the hop before it is no proxy of either family.
    assert_eq!(seen("2001:db8::7, ::1"), "2001:db8::7");
    // Within fd00::/8, a proxy too.
    assert_eq!(seen("2001:db8::8, fd12::1"), "2001:db8::8");
    // A proxy written as IPv4 mapped into IPv6 is the IPv4 one named.
    assert_eq!(seen("203.0.113.5, ::ffff:127.0.0.1"), "203.0.113.5");
    // A mapped address no proxy has is the client, as written.
    assert_eq!(seen("::ffff:10.1.2.3"), "::ffff:10.1.2.3");
    // An IPv6 hop is not an IPv4 proxy: the client.
    assert_eq!(seen("2001:db8::9"), "2001:db8::9");
}

// ---- connections that wait ------------------------------------------------------------

/// After an answer that ends the connection, the server stops writing and waits for the client
/// to close too; a client that never does is let go after two seconds.
#[test]
fn a_client_that_never_closes_after_a_refusal_is_let_go() {
    let srv = run(&folder("linger", APP), &[], None, true);
    let mut s = connect(srv.port);
    s.write_all(b"NOT HTTP AT ALL\r\n\r\n").unwrap();
    let got = all(&mut s);
    assert!(got.starts_with("HTTP/1.1 400"), "{}", got);
    // The server has shut its side; ours is still open, and it is counted until let go.
    assert!(srv.connections() >= 2, "{}", srv.log());
    assert!(until_for(10, || srv.connections() == 1), "the lingering connection stayed: {}", srv.log());
    drop(s);
}

/// A request's head that never ends is answered with a 408 after thirty seconds.
#[test]
fn a_head_that_never_ends_is_answered_408_after_thirty_seconds() {
    let srv = run(&folder("slow-head", APP), &[], None, true);
    let mut s = connect(srv.port);
    let began = Instant::now();
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n").unwrap();
    let got = all(&mut s);
    assert!(got.starts_with("HTTP/1.1 408"), "{}", got);
    assert!(got.ends_with("request timeout\n"), "{}", got);
    assert!(began.elapsed() >= Duration::from_secs(29), "{:?}", began.elapsed());
    assert!(until(|| srv.connections() == 1), "{}", srv.log());
}

/// A client that asked for a big answer, reads none of it, and leaves its next request
/// unfinished is timed out too; its 408 cannot be written behind the answer, so the connection
/// is just closed, and what was left unsent is dropped.
#[test]
fn a_client_reading_nothing_with_a_head_unfinished_is_closed_without_its_408() {
    let srv = run(&folder("slow-head-full", APP), &[], None, true);
    let mut s = connect(srv.port);
    s.write_all(b"GET /big HTTP/1.1\r\nHost: x\r\n\r\nGET / HTTP/1.1\r\nHo").unwrap();
    // Open, with the answer stuck in it, past the thirty seconds.
    assert!(until_for(15, || srv.connections() == 2), "{}", srv.log());
    std::thread::sleep(Duration::from_secs(33));
    assert!(until_for(10, || srv.connections() == 1), "the connection was not closed: {}", srv.log());
    let got = all(&mut s);
    assert!(got.starts_with("HTTP/1.1 200"), "{}", &got[..got.len().min(200)]);
    assert!(got.len() < 40_000_000, "the whole answer came: {}", got.len());
    assert!(!got.contains("408"), "the 408 came after all");
}

// ---- out of descriptors ---------------------------------------------------------------

/// Connections, each asking for something, until the server's log says it could not take
/// another. Given back when dropped.
fn held_until_refused<T>(srv: &Srv, mut connect: impl FnMut() -> T) -> Vec<T> {
    let mut held = Vec::new();
    let out = until(|| {
        held.push(connect());
        srv.log().contains("cannot accept a connection") || held.len() > 300
    });
    assert!(out && srv.log().contains("cannot accept a connection"), "{}", srv.log());
    held
}

/// With several instances, one thread takes the connections and hands them on: out of
/// descriptors, it says so, waits a little, and goes on once some are given back.
#[test]
fn with_two_instances_out_of_descriptors_connections_wait_until_some_are_free() {
    let srv = run(&folder("instances-fds", APP), &["--instances", "2"], Some(64), true);
    assert!(until(|| srv.get("/").body == "hi"), "{}", srv.log());
    let held = held_until_refused(&srv, || {
        let mut c = TcpStream::connect(("127.0.0.1", srv.port)).unwrap();
        let _ = c.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n");
        c
    });
    drop(held);
    assert!(until(|| srv.get("/").body == "hi"), "{}", srv.log());
}

/// The same on a Unix socket (`--listen unix:PATH`).
#[test]
fn on_a_unix_socket_out_of_descriptors_connections_wait_until_some_are_free() {
    use std::os::unix::net::UnixStream;
    let d = folder("unix-fds", APP);
    let sock = d.join("s.sock");
    let srv = run(&d, &["--listen", "unix:s.sock"], Some(64), false);
    let ask = || -> String {
        let Ok(mut c) = UnixStream::connect(&sock) else { return String::new() };
        c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let _ = c.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
        let mut out = String::new();
        let _ = c.read_to_string(&mut out);
        out
    };
    assert!(until(|| ask().ends_with("\r\n\r\nhi")), "{}", srv.log());
    let held = held_until_refused(&srv, || {
        let c = UnixStream::connect(&sock);
        if let Ok(mut c) = c.as_ref() {
            let _ = c.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n");
        }
        c
    });
    drop(held);
    assert!(until(|| ask().ends_with("\r\n\r\nhi")), "{}", srv.log());
}

// ---- in this process --------------------------------------------------------------------

/// `--max-compute` as set and as read back (a policy's limit is compared with it).
#[test]
fn the_compute_limit_set_is_the_one_read_back() {
    let before = quillo::httpd::compute_limit_ms();
    quillo::httpd::limit_compute(60_000);
    assert_eq!(quillo::httpd::compute_limit_ms(), 60_000);
    assert!(quillo::httpd::compute_limited());
    quillo::httpd::limit_compute(before);
    assert_eq!(quillo::httpd::compute_limit_ms(), before);
}

/// Requests whose coroutine waits on children made outside `parallel`: none at all (it goes on
/// at once), and one that panics (the others and the request go on, and so does the server).
mod children {
    use quillo::interp::Ctrl;
    use quillo::server::{serve_with, App, Loaded};
    use quillo::value::define;
    use quillo::{Interp, Value};
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use std::sync::mpsc::channel;
    use std::sync::Once;
    use std::time::Duration;

    static RAN: AtomicUsize = AtomicUsize::new(0);

    fn no_children(_: &mut Interp, _: Vec<Value>, _: u32) -> Result<Value, Ctrl> {
        quillo::httpd::run_children(Vec::new());
        Ok(Value::str("went on"))
    }

    fn a_child_panics(_: &mut Interp, _: Vec<Value>, _: u32) -> Result<Value, Ctrl> {
        RAN.store(0, SeqCst);
        quillo::httpd::run_children(vec![
            Box::new(|| panic!("boom in a child")),
            Box::new(|| {
                RAN.fetch_add(1, SeqCst);
            }),
        ]);
        Ok(Value::Int(RAN.load(SeqCst) as i64))
    }

    /// This panic is expected; any other is shown as usual.
    fn quietly() {
        static SET: Once = Once::new();
        SET.call_once(|| {
            let shown = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |p| {
                if p.payload().downcast_ref::<&str>() != Some(&"boom in a child") {
                    shown(p);
                }
            }));
        });
    }

    #[test]
    fn a_request_waiting_on_no_children_or_on_one_that_panics_goes_on() {
        quietly();
        let d = super::support::temp_dir("cover-httpd-children");
        let (tx, rx) = channel();
        let (dir, main) = (d.clone(), d.join("main.qwe"));
        std::thread::spawn(move || {
            let mut it = Interp::new(&dir);
            define(&it.globals, "no_children", Value::Builtin("no_children", no_children));
            define(&it.globals, "a_child_panics", Value::Builtin("a_child_panics", a_child_panics));
            let src = "route GET /none = no_children()\nroute GET /panics = str(a_child_panics())\nroute GET /fine = \"fine\"\n";
            it.load_source(src, main.to_str().unwrap()).unwrap();
            let shared = it.shared.clone();
            let app = App::new(Loaded::Ready(it), shared, &main, &dir, false);
            serve_with(app, "127.0.0.1:0", 1, move |a| tx.send(a.port()).unwrap()).unwrap();
        });
        let port = rx.recv_timeout(Duration::from_secs(20)).expect("the server did not start");
        let a = super::support::send(port, "GET", "/none", &[], "");
        assert_eq!((a.status, a.body.as_str()), (200, "went on"));
        let a = super::support::send(port, "GET", "/panics", &[], "");
        assert_eq!((a.status, a.body.as_str()), (200, "1"), "the other child ran");
        assert_eq!(super::support::send(port, "GET", "/fine", &[], "").body, "fine");
        assert_eq!(super::support::send(port, "GET", "/none", &[], "").body, "went on");
    }

    /// The HTTP server alone, given a program and a listener, as one embedding it would.
    #[test]
    fn the_http_server_given_a_program_and_a_listener_answers_it() {
        let d = super::support::temp_dir("cover-httpd-plain");
        let main = d.join("main.qwe");
        let mut it = Interp::new(&d);
        it.load_source("route GET / = \"served plainly\"\n", main.to_str().unwrap()).unwrap();
        let shared = it.shared.clone();
        let app = std::sync::Arc::new(App::new(Loaded::Ready(it), shared.clone(), &main, &d, false));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // The event loop takes the program's lock when it starts.
        shared.gil.release();
        std::thread::spawn(move || quillo::httpd::serve(app, listener, 2));
        let a = super::support::send(port, "GET", "/", &[], "");
        assert_eq!((a.status, a.body.as_str()), (200, "served plainly"));
        assert_eq!(super::support::send(port, "GET", "/nothing", &[], "").status, 404);
    }
}
