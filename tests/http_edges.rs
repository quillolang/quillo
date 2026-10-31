//! The HTTP server at the edges of the protocol: raw bytes over a socket,
//! clients that stop reading or vanish, and a server that is stopping.

mod support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const APP: &str = r#"
route GET / = "hi"
route GET /s/:n = respond(int(n), "x")
route POST /echo = request.body
route GET /big = repeat("b", 8000000)
route GET /big-later { sleep(250); repeat("b", 8000000) }
route GET /nap { sleep(120); "ok" }
route GET /p = str(parallel([]))
route GET /each/:n = str(parallel(map(range(int(n)), (i) => () => { sleep(30); i })))
route GET /bad-header = stream((send) => send("x"), { headers: { "x-bad": "a\nb", "x-fine": "yes" } })
route GET /forever = stream((send) => { while true { send(repeat("y", 65536)) } })
route GET /short = stream((send) => { send("one"); send({ two: 2 }); send(null) })
route GET /forged = respond(200, "x", { "x-quillo-stream": "1" })
route GET /forged-file = respond(200, "x", { "x-quillo-send": "forged" })
"#;

struct Srv {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Srv {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Srv {
    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// Waits until the log has `text`.
    fn logged(&self, text: &str) -> bool {
        until(|| self.log().contains(text))
    }

    fn connections(&self) -> i64 {
        let a = support::send(self.port, "GET", "/_quillo/metrics", &[], "");
        a.body.lines().find_map(|l| l.strip_prefix("quillo_connections ")).and_then(|n| n.parse().ok()).unwrap_or(-1)
    }

    /// Waits until only the connection asking is open.
    fn connections_end(&self) -> bool {
        until(|| self.connections() == 1)
    }

    fn signal(&self, name: &str) {
        assert!(Command::new("kill").args([name, &self.child.id().to_string()]).status().unwrap().success());
    }
}

fn until(mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn start(name: &str, src: &str, args: &[&str]) -> Srv {
    start_from(name, src, |c| {
        c.args(args);
    })
}

fn start_from(name: &str, src: &str, set: impl Fn(&mut Command)) -> Srv {
    let d = support::temp_dir(&format!("edges-{}", name));
    let f = d.join("app.qwe");
    std::fs::write(&f, src).unwrap();
    let log = d.join("stderr.log");
    // A port found free can be taken before the server listens on it: ask for another.
    for _ in 0..5 {
        let port = support::free_port();
        let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
        c.arg("run").arg(&f).args(["--port", &port.to_string()]);
        set(&mut c);
        let child = c
            .env("QUILLO_SECRET", "a test secret of more than thirty-two bytes")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log).unwrap())
            .spawn()
            .expect("cannot start quillo");
        support::stop_with_tests(&child);
        let s = Srv { child, port, log: log.clone() };
        let taken = || s.log().contains("Address already in use");
        assert!(until(|| support::listening(port) || taken()), "the server did not start: {}", s.log());
        if !taken() {
            return s;
        }
    }
    panic!("no free port in 5 tries");
}

fn connect(port: u16) -> TcpStream {
    let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    s.set_nodelay(true).unwrap();
    s
}

/// Sends `bytes` and reads until the server closes.
fn raw(port: u16, bytes: &[u8]) -> String {
    let mut s = connect(port);
    s.write_all(bytes).unwrap();
    all(&mut s)
}

fn all(s: &mut TcpStream) -> String {
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

/// Reads until the head of an answer has come, and its `content-length`
/// bytes after it.
fn one_answer(s: &mut TcpStream) -> String {
    let mut out = Vec::new();
    let mut b = [0u8; 4096];
    loop {
        let text = String::from_utf8_lossy(&out).into_owned();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let len = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length: ").map(str::to_string)).and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
            if body.len() >= len {
                return text;
            }
        }
        match s.read(&mut b) {
            Ok(0) | Err(_) => return text,
            Ok(n) => out.extend_from_slice(&b[..n]),
        }
    }
}

/// Closes with a reset instead of a goodbye: what a crashed client leaves.
#[cfg(unix)]
fn reset(s: TcpStream) {
    use std::os::fd::AsRawFd;
    let l = libc::linger { l_onoff: 1, l_linger: 0 };
    // SAFETY: an open socket, and a value of the size given.
    let r = unsafe { libc::setsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_LINGER, &l as *const _ as *const libc::c_void, std::mem::size_of::<libc::linger>() as u32) };
    assert_eq!(r, 0);
}

#[test]
fn every_status_goes_out_with_its_reason() {
    let srv = start("reasons", APP, &[]);
    let known = [
        (100, "Continue"),
        (101, "Switching Protocols"),
        (200, "OK"),
        (201, "Created"),
        (202, "Accepted"),
        (204, "No Content"),
        (206, "Partial Content"),
        (301, "Moved Permanently"),
        (302, "Found"),
        (303, "See Other"),
        (304, "Not Modified"),
        (307, "Temporary Redirect"),
        (308, "Permanent Redirect"),
        (400, "Bad Request"),
        (401, "Unauthorized"),
        (403, "Forbidden"),
        (404, "Not Found"),
        (405, "Method Not Allowed"),
        (409, "Conflict"),
        (410, "Gone"),
        (411, "Length Required"),
        (413, "Content Too Large"),
        (415, "Unsupported Media Type"),
        (416, "Range Not Satisfiable"),
        (422, "Unprocessable Content"),
        (429, "Too Many Requests"),
        (431, "Request Header Fields Too Large"),
        (500, "Internal Server Error"),
        (501, "Not Implemented"),
        (502, "Bad Gateway"),
        (503, "Service Unavailable"),
        (504, "Gateway Timeout"),
        (299, ""),
    ];
    for (code, reason) in known {
        let a = raw(srv.port, format!("GET /s/{} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n", code).as_bytes());
        assert!(a.starts_with(&format!("HTTP/1.1 {} {}\r\n", code, reason)), "{}", a);
    }
}

#[test]
fn a_request_that_expects_100_continue_is_told_to_go_on() {
    let srv = start("continue", APP, &[]);
    let mut s = connect(srv.port);
    s.write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nExpect: 100-continue\r\nContent-Length: 5\r\n\r\n").unwrap();
    let mut b = [0u8; 25];
    s.read_exact(&mut b).unwrap();
    assert_eq!(&b, b"HTTP/1.1 100 Continue\r\n\r\n");
    s.write_all(b"hello").unwrap();
    let a = one_answer(&mut s);
    assert!(a.starts_with("HTTP/1.1 200 OK") && a.ends_with("\r\n\r\nhello"), "{}", a);
}

#[test]
fn http_1_0_closes_unless_it_asks_to_keep_the_connection() {
    let srv = start("http10", APP, &[]);
    let a = raw(srv.port, b"GET / HTTP/1.0\r\nHost: x\r\n\r\n");
    assert!(a.contains("connection: close") && a.ends_with("hi"), "{}", a);
    let mut s = connect(srv.port);
    s.write_all(b"GET / HTTP/1.0\r\nConnection: keep-alive\r\n\r\n").unwrap();
    let a = one_answer(&mut s);
    assert!(!a.contains("connection: close") && a.ends_with("hi"), "{}", a);
    s.write_all(b"GET / HTTP/1.0\r\nConnection: keep-alive\r\n\r\n").unwrap();
    assert!(one_answer(&mut s).ends_with("hi"));
}

#[test]
fn a_line_that_is_no_request_is_refused_and_logged_as_it_came() {
    let srv = start("garbage", APP, &[]);
    let a = raw(srv.port, b"garbage\r\n\r\n");
    assert!(a.starts_with("HTTP/1.1 400 Bad Request") && a.contains("bad request") && a.contains("connection: close"), "{}", a);
    assert!(srv.logged("garbage -> 400 (bad request)"), "{}", srv.log());
}

#[test]
fn too_many_or_too_large_headers_are_refused() {
    let srv = start("headers", APP, &[]);
    let mut many = String::from("GET / HTTP/1.1\r\n");
    for i in 0..101 {
        many.push_str(&format!("x-{}: 1\r\n", i));
    }
    many.push_str("\r\n");
    let a = raw(srv.port, many.as_bytes());
    assert!(a.starts_with("HTTP/1.1 431") && a.contains("too many request headers"), "{}", a);
    // Complete, and larger than a head may be.
    let long = format!("GET / HTTP/1.1\r\nx-long: {}\r\n\r\n", "a".repeat(70_000));
    let a = raw(srv.port, long.as_bytes());
    assert!(a.starts_with("HTTP/1.1 431") && a.contains("request headers too large"), "{}", a);
}

#[test]
fn a_chunked_body_can_be_cut_anywhere() {
    let srv = start("chunks", APP, &[]);
    let head = b"POST /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n";
    let pieces: [&[u8]; 6] = [b"4\r\nWi", b"ki", b"\r", b"\n5;x=1\r\npedia\r\n0\r\nX-T", b": 1\r", b"\n\r\n"];
    let mut s = connect(srv.port);
    s.write_all(head).unwrap();
    for p in pieces {
        std::thread::sleep(Duration::from_millis(15));
        s.write_all(p).unwrap();
    }
    let a = all(&mut s);
    assert!(a.starts_with("HTTP/1.1 200") && a.ends_with("\r\n\r\nWikipedia"), "{}", a);
}

#[test]
fn a_chunk_not_ended_by_a_line_break_is_refused() {
    let srv = start("chunk-end", APP, &[]);
    let a = raw(srv.port, b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWikiXX");
    assert!(a.starts_with("HTTP/1.1 400") && a.contains("bad chunked body"), "{}", a);
}

#[test]
fn a_header_a_response_cannot_carry_is_left_out_and_logged() {
    let srv = start("bad-header", APP, &[]);
    let a = raw(srv.port, b"GET /bad-header HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    assert!(a.starts_with("HTTP/1.1 200"), "{}", a);
    assert!(a.contains("x-fine: yes") && !a.contains("x-bad"), "{}", a);
    assert!(srv.logged("header `x-bad` not sent"), "{}", srv.log());
}

#[test]
fn a_program_cannot_mark_an_answer_as_a_stream_or_a_file_itself() {
    let srv = start("forged", APP, &[]);
    assert_eq!(support::send(srv.port, "GET", "/forged", &[], "").status, 500);
    assert_eq!(support::send(srv.port, "GET", "/forged-file", &[], "").status, 500);
}

#[test]
fn a_head_of_a_stream_has_no_body_and_keeps_the_connection() {
    let srv = start("head-stream", APP, &[]);
    let mut s = connect(srv.port);
    s.write_all(b"HEAD /short HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let a = one_answer(&mut s);
    assert!(a.starts_with("HTTP/1.1 200") && a.contains("transfer-encoding: chunked") && a.ends_with("\r\n\r\n"), "{}", a);
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    let a = all(&mut s);
    assert!(a.starts_with("HTTP/1.1 200") && a.ends_with("hi"), "{}", a);
}

#[test]
fn what_a_stream_sends_goes_out_in_chunks() {
    let srv = start("stream-chunks", APP, &[]);
    let a = raw(srv.port, b"GET /short HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    // `send(null)` sends nothing.
    assert!(a.ends_with("\r\n\r\n3\r\none\r\na\r\n{\"two\":2}\n\r\n0\r\n\r\n"), "{}", a);
}

#[test]
fn a_full_log_is_written_out_without_waiting() {
    let srv = start("log", APP, &[]);
    let path = format!("/{}", "l".repeat(300));
    let mut batch = String::new();
    for _ in 0..200 {
        batch.push_str(&format!("GET {} HTTP/1.1\r\nHost: x\r\n\r\n", path));
    }
    batch.push_str("GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    let a = raw(srv.port, batch.as_bytes());
    assert_eq!(a.matches("HTTP/1.1 404").count(), 200);
    assert!(until(|| srv.log().matches("-> 404").count() == 200), "{}", srv.log().len());
}

#[test]
fn a_flood_of_pipelined_requests_is_answered_in_order() {
    // The most a connection holds is about 128 KiB with this limit.
    let srv = start("flood", APP, &["--max-body", "10"]);
    let mut batch = String::new();
    for _ in 0..20_000 {
        batch.push_str("GET / HTTP/1.1\r\nHost: x\r\n\r\n");
    }
    batch.push_str("GET /s/201 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    let port = srv.port;
    let mut s = connect(port);
    let mut w = s.try_clone().unwrap();
    let t = std::thread::spawn(move || w.write_all(batch.as_bytes()));
    let a = all(&mut s);
    t.join().unwrap().unwrap();
    assert_eq!(a.matches("HTTP/1.1 200").count(), 20_000);
    assert!(a.contains("HTTP/1.1 201 Created"));
}

#[test]
fn a_request_ending_exactly_where_a_read_does_is_answered_though_the_client_stopped_sending() {
    let srv = start("exact", APP, &[]);
    // A read takes 16 KiB: the end of the input comes with the next.
    let pad = 16384 - "POST /echo HTTP/1.1\r\nContent-Length: 10\r\nx-pad: \r\n\r\n".len() - 10;
    let req = format!("POST /echo HTTP/1.1\r\nContent-Length: 10\r\nx-pad: {}\r\n\r\n0123456789", "p".repeat(pad));
    assert_eq!(req.len(), 16384);
    let mut s = connect(srv.port);
    s.write_all(req.as_bytes()).unwrap();
    s.shutdown(std::net::Shutdown::Write).unwrap();
    let a = all(&mut s);
    assert!(a.starts_with("HTTP/1.1 200") && a.ends_with("0123456789"), "{}", a);
}

#[test]
fn a_large_answer_reaches_a_slow_reader_before_the_connection_closes() {
    let srv = start("slow-close", APP, &[]);
    let mut s = connect(srv.port);
    s.write_all(b"GET /big HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let a = all(&mut s);
    assert!(a.starts_with("HTTP/1.1 200"), "{}", &a[..40]);
    assert_eq!(a.split_once("\r\n\r\n").unwrap().1.len(), 8_000_000);
}

#[test]
fn bytes_that_arrive_while_a_large_answer_is_made_are_read_after_it() {
    let srv = start("again", APP, &[]);
    let mut s = connect(srv.port);
    s.write_all(b"GET /big-later HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(60));
    // The start of the next request, while the first sleeps.
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConn").unwrap();
    std::thread::sleep(Duration::from_millis(350));
    s.write_all(b"ection: close\r\n\r\n").unwrap();
    let a = all(&mut s);
    assert_eq!(a.matches("HTTP/1.1 200").count(), 2, "{}", a.len());
    assert!(a.ends_with("hi"));
}

#[cfg(unix)]
#[test]
fn a_client_that_resets_a_large_answer_is_forgotten() {
    let srv = start("reset", APP, &["--metrics"]);
    let mut s = connect(srv.port);
    s.write_all(b"GET /big HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut b = [0u8; 1000];
    s.read_exact(&mut b).unwrap();
    reset(s);
    assert!(srv.connections_end(), "{}", srv.connections());
    assert_eq!(support::send(srv.port, "GET", "/", &[], "").body, "hi");
}

#[cfg(unix)]
#[test]
fn a_client_that_resets_after_its_answer_is_forgotten() {
    let srv = start("reset-after", APP, &["--metrics"]);
    let mut s = connect(srv.port);
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    let a = all(&mut s);
    assert!(a.ends_with("hi"));
    reset(s);
    assert!(srv.connections_end(), "{}", srv.connections());
}

#[cfg(unix)]
#[test]
fn a_stream_ends_when_its_client_resets() {
    let srv = start("reset-stream", APP, &["--metrics"]);
    let mut s = connect(srv.port);
    s.write_all(b"GET /forever HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut b = [0u8; 70_000];
    s.read_exact(&mut b).unwrap();
    reset(s);
    assert!(srv.connections_end(), "{}", srv.connections());
    assert_eq!(support::send(srv.port, "GET", "/", &[], "").body, "hi");
}

#[cfg(unix)]
#[test]
fn a_client_that_resets_while_its_answer_is_made_is_forgotten() {
    let srv = start("reset-early", APP, &["--metrics"]);
    let mut s = connect(srv.port);
    s.write_all(b"GET /nap HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(40));
    reset(s);
    assert!(srv.connections_end(), "{}", srv.connections());
}

#[test]
fn waits_beyond_the_pools_threads_take_their_turn() {
    let srv = start("pool", APP, &["--workers", "1"]);
    let port = srv.port;
    let all: Vec<_> = (0..6).map(|_| std::thread::spawn(move || support::send(port, "GET", "/nap", &[], ""))).collect();
    for t in all {
        assert_eq!(t.join().unwrap().body, "ok");
    }
}

#[test]
fn parallel_with_nothing_to_run_gives_nothing() {
    let srv = start("parallel", APP, &[]);
    assert_eq!(support::send(srv.port, "GET", "/p", &[], "").body, "[]");
    assert_eq!(support::send(srv.port, "GET", "/each/3", &[], "").body, "[0, 1, 2]");
}

#[cfg(unix)]
#[test]
fn a_stopping_server_waits_for_a_request_still_arriving() {
    let srv = start("stop-partial", APP, &[]);
    let mut s = connect(srv.port);
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConn").unwrap();
    // It has been read: the server counts it as arriving.
    std::thread::sleep(Duration::from_millis(80));
    let mut idle = connect(srv.port);
    assert!(support::send(srv.port, "GET", "/", &[], "").body == "hi");
    srv.signal("-TERM");
    // The idle connection is closed at once; the other is waited for.
    assert_eq!(all(&mut idle), "");
    s.write_all(b"ection: close\r\n\r\n").unwrap();
    let a = all(&mut s);
    assert!(a.starts_with("HTTP/1.1 200") && a.ends_with("hi"), "{}", a);
    let mut srv = srv;
    assert!(until(|| srv.child.try_wait().unwrap().is_some()), "the server did not stop");
    assert!(srv.child.wait().unwrap().success());
}

/// With few file descriptors, what cannot be accepted waits in the queue,
/// and the server goes on with those it has.
#[cfg(unix)]
#[test]
fn a_server_out_of_descriptors_says_so_and_carries_on() {
    let bin = env!("CARGO_BIN_EXE_quillo");
    let srv = start_from("fds", APP, |c| {
        let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        *c = Command::new("sh");
        c.args(["-c", "ulimit -n 64; exec \"$0\" \"$@\"", bin]).args(args);
    });
    let mut held = Vec::new();
    assert!(
        until(|| {
            held.push(connect(srv.port));
            held.last_mut().unwrap().write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            srv.log().contains("cannot accept a connection") || held.len() > 200
        }),
        "{}",
        srv.log()
    );
    assert!(srv.log().contains("cannot accept a connection"), "{}", srv.log());
    drop(held);
    assert!(until(|| {
        let mut s = connect(srv.port);
        s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
        s.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
        all(&mut s).ends_with("hi")
    }));
}

/// Clients that keep the event loop computing, for a test about a reload.
struct Busy {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    clients: Vec<std::thread::JoinHandle<()>>,
}

impl Busy {
    fn on(port: u16) -> Busy {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let clients = (0..3)
            .map(|_| {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        support::send(port, "GET", "/spin", &[], "");
                    }
                })
            })
            .collect();
        Busy { stop, clients }
    }

    fn off(self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for t in self.clients {
            t.join().unwrap();
        }
    }
}

fn reloading(name: &str) -> (Srv, PathBuf) {
    let srv = start(name, RELOADS, &["--reload"]);
    let f = srv.log.with_file_name("app.qwe");
    (srv, f)
}

const RELOADS: &str = "route GET /v = \"one\"\nroute GET /spin { let n = 0; for i in range(200000) { n += 1 }; str(n) }\n";

#[test]
fn a_reload_waiting_behind_a_busy_server_comes_when_it_quietens() {
    let (srv, f) = reloading("busy-then-quiet");
    let busy = Busy::on(srv.port);
    std::thread::sleep(Duration::from_millis(100));
    std::fs::write(&f, RELOADS.replace("one", "two")).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    busy.off();
    assert!(until(|| support::send(srv.port, "GET", "/v", &[], "").body == "two"), "{}", srv.log());
}

#[test]
fn a_reload_gets_its_turn_on_a_busy_server() {
    let (srv, f) = reloading("busy");
    let busy = Busy::on(srv.port);
    std::thread::sleep(Duration::from_millis(100));
    std::fs::write(&f, RELOADS.replace("one", "two")).unwrap();
    let reloaded = until(|| support::send(srv.port, "GET", "/v", &[], "").body == "two");
    busy.off();
    assert!(reloaded, "{}", srv.log().lines().filter(|l| !l.starts_with("GET")).collect::<Vec<_>>().join("\n"));
}
