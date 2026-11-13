//! FEEDBACK65.md: what qwertime.com found in production on 0.8.1.
#![cfg(unix)]

mod support;

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

// ---- Q-942: a connection is idle from the end of its last answer ----------------------------
// The server closes a kept-alive connection idle for `IDLE` (60 s). It counted from the start of
// the last request, so one whose answer took longer (a stream, a long wait) was closed within a
// second of its end, its answer kept alive, and the client's next request there was cut off.
// The tests here make `IDLE` 5 s with QUILLO_IDLE_MS_FOR_TESTS, which only ever shortens it; the
// first runs with the real 60 s, as the branch's parent build has no such setting.

/// The quillo that serves: this build's, or the one QUILLO_TEST_BIN names (the branch's parent,
/// to see a test fail there).
fn quillo() -> PathBuf {
    std::env::var_os("QUILLO_TEST_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_quillo")))
}

const PROGRAM: &str = r#"route GET / = "home"
route GET /after = "after"
route GET /slow/:ms(int) {
    sleep(ms)
    return "slow"
}
route GET /stream/:n(int)/:ms(int) = events((send) => {
    for i in range(n) {
        send({ i, s: "é😀" })
        sleep(ms)
    }
})
route GET /quiet/:ms(int) = events((send) => {
    sleep(ms)
})
route POST /posted = events((send) => {
    send({ got: request.body })
})
route POST /plain = request.body
route GET /closing = stream((send) => {
    send("a")
    send("b")
}, { headers: { connection: "Close" } })
route GET /closing-whole = respond(200, "ab", { connection: "close" })
route GET /big/:n(int) = repeat("x", n)
"#;

/// `IDLE` as these tests make it.
const IDLE_MS: u64 = 5000;
/// How long a test leaves a connection between answers: under `IDLE_MS`, and longer than the
/// server's sweeps are apart (one a second, two at most), so that one has looked at it.
const PAUSE: Duration = Duration::from_millis(2500);

/// How much longer a debug build of the server may take (CI's), for bounds on how long a thing
/// took; never for the times a test means (`IDLE_MS`, `PAUSE`, `SLOW`).
fn slower() -> u32 {
    if std::env::var_os("QUILLO_TEST_BIN").is_none() && cfg!(debug_assertions) {
        5
    } else {
        1
    }
}

/// A `quillo run` of `PROGRAM`, killed when dropped, its folder removed.
struct Served {
    child: Child,
    port: u16,
    dir: PathBuf,
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Served with `IDLE` made `idle_ms`, or the server's own.
fn served(name: &str, idle_ms: Option<u64>) -> Served {
    let dir = support::temp_dir(&format!("feedback65-{}", name));
    std::fs::write(dir.join("main.qwe"), PROGRAM).unwrap();
    let log = dir.join("server.log");
    let mut c = Command::new(quillo());
    c.arg("run").arg(dir.join("main.qwe")).args(["--port", "0", "--workers", "8"]);
    c.env("QUILLO_SECRET", "test secret for the tests").env_remove("QUILLO_IDLE_MS_FOR_TESTS");
    if let Some(ms) = idle_ms {
        c.env("QUILLO_IDLE_MS_FOR_TESTS", ms.to_string());
    }
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(std::fs::File::create(&log).unwrap());
    let child = c.spawn().unwrap();
    support::stop_with_tests(&child);
    let mut s = Served { child, port: 0, dir };
    let deadline = Instant::now() + Duration::from_secs(30) * slower();
    loop {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let port = text.split("listening on http://").nth(1).and_then(|r| r.split(['/', '\n']).next()).and_then(|h| h.rsplit(':').next()?.parse::<u16>().ok());
        if let Some(p) = port {
            s.port = p;
            return s;
        }
        if let Ok(Some(st)) = s.child.try_wait() {
            panic!("quillo run stopped: {}\n{}", st, text);
        }
        assert!(Instant::now() < deadline, "the server did not start:\n{}", text);
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// An answer: its status, its head in lower case, its body.
struct Got {
    status: u16,
    head: String,
    body: String,
}

impl Got {
    fn says_close(&self) -> bool {
        self.head.lines().any(|l| l.trim() == "connection: close")
    }
}

/// One connection, read answer by answer.
struct Conn {
    s: TcpStream,
    buf: Vec<u8>,
}

impl Conn {
    fn to(port: u16) -> Conn {
        let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(20) * slower())).unwrap();
        Conn { s, buf: Vec::new() }
    }

    fn send(&mut self, raw: &str) {
        self.s.write_all(raw.as_bytes()).unwrap();
    }

    fn get(&mut self, path: &str) {
        self.send(&format!("GET {} HTTP/1.1\r\nHost: x\r\n\r\n", path));
    }

    /// More bytes; false when the server closed the connection (or reset it).
    fn more(&mut self) -> bool {
        let mut chunk = [0u8; 16 << 10];
        match self.s.read(&mut chunk) {
            Ok(0) => false,
            Ok(n) => {
                self.buf.extend_from_slice(&chunk[..n]);
                true
            }
            Err(e) if matches!(e.kind(), ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::BrokenPipe) => false,
            Err(e) => panic!("no answer in time: {} (so far {:?})", e, String::from_utf8_lossy(&self.buf)),
        }
    }

    /// The next answer's status and head, its body left to read; None when the server closed the
    /// connection before any of it.
    fn head(&mut self) -> Option<(u16, String)> {
        let end = loop {
            if let Some(i) = self.buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
            if !self.more() {
                assert!(self.buf.is_empty(), "closed in the middle of a head: {:?}", String::from_utf8_lossy(&self.buf));
                return None;
            }
        };
        let head = String::from_utf8_lossy(&self.buf[..end]).to_ascii_lowercase();
        self.buf.drain(..end);
        Some((head.get(9..12).and_then(|s| s.parse().ok()).unwrap_or(0), head))
    }

    /// The next answer, a `100 Continue` too; None when the server closed the connection before
    /// any of it.
    fn answer(&mut self) -> Option<Got> {
        let (status, head) = self.head()?;
        if status == 100 {
            return Some(Got { status, head, body: String::new() });
        }
        let field = |name: &str| head.lines().find_map(|l| l.strip_prefix(name).map(|v| v.trim().to_string()));
        let mut body = Vec::new();
        if let Some(len) = field("content-length:").and_then(|v| v.parse::<usize>().ok()) {
            while self.buf.len() < len {
                assert!(self.more(), "closed in the middle of a body");
            }
            body = self.buf.drain(..len).collect();
        } else if field("transfer-encoding:").is_some_and(|v| v.contains("chunked")) {
            loop {
                let line = loop {
                    if let Some(i) = self.buf.windows(2).position(|w| w == b"\r\n") {
                        break i;
                    }
                    assert!(self.more(), "closed in the middle of a chunk");
                };
                let size = usize::from_str_radix(String::from_utf8_lossy(&self.buf[..line]).trim(), 16).unwrap();
                self.buf.drain(..line + 2);
                while self.buf.len() < size + 2 {
                    assert!(self.more(), "closed in the middle of a chunk");
                }
                body.extend(self.buf.drain(..size));
                self.buf.drain(..2);
                if size == 0 {
                    break;
                }
            }
        } else {
            // As long as the connection.
            while self.more() {}
            body = std::mem::take(&mut self.buf);
        }
        Some(Got { status, head, body: String::from_utf8(body).unwrap() })
    }

    /// Waits until the server closes the connection, sending nothing: how long that took.
    fn closed_after(&mut self) -> Duration {
        let t = Instant::now();
        let more = self.more();
        assert!(!more, "the server sent {:?}", String::from_utf8_lossy(&self.buf));
        t.elapsed()
    }
}

/// `path` answered on `c`, the connection then left for `PAUSE`, and another request answered
/// there.
fn kept_after(c: &mut Conn, path: &str, events: usize) {
    c.get(path);
    let a = c.answer().expect("an answer");
    assert_eq!(a.status, 200, "{}", a.head);
    assert!(!a.says_close(), "{}", a.head);
    assert_eq!(a.body.matches("data: ").count(), events, "{}", a.body);
    std::thread::sleep(PAUSE);
    c.get("/after");
    let b = c.answer().unwrap_or_else(|| panic!("Q-942: the connection was closed {:?} after an answer to {}", PAUSE, path));
    assert_eq!(b.body, "after");
}

// The report's case, with the real `IDLE`: a stream of 61 s, and a request on its connection
// 2.5 s after it ends. It waits a minute, as the branch's parent build cannot be given a shorter
// `IDLE`; the others make it 5 s.
#[test]
fn a_stream_longer_than_a_minute_leaves_its_connection_to_use_again() {
    let s = served("minute", None);
    let mut c = Conn::to(s.port);
    c.s.set_read_timeout(Some(Duration::from_secs(90) * slower())).unwrap();
    let t = Instant::now();
    kept_after(&mut c, "/stream/61/1000", 61);
    assert!(t.elapsed() < Duration::from_secs(64) + Duration::from_secs(16) * slower(), "{:?}", t.elapsed());
}

// The real `IDLE` still closes a connection with nothing to do, after a minute counted from its
// answer's end; and the tests' setting cannot make it longer.
#[test]
fn a_connection_idle_for_a_minute_is_closed_whatever_the_tests_setting_says() {
    let s = served("lengthen", Some(120_000));
    let mut c = Conn::to(s.port);
    c.s.set_read_timeout(Some(Duration::from_secs(90) * slower())).unwrap();
    c.get("/");
    assert_eq!(c.answer().unwrap().body, "home");
    let took = c.closed_after();
    assert!(took >= Duration::from_secs(59) && took < Duration::from_secs(60) + Duration::from_secs(5) * slower(), "closed {:?} after its answer", took);
}

// QUILLO_IDLE_MS_FOR_TESTS under a second is taken as not given: with 0, a new connection could be
// closed before its first request was read. A second is honoured.
#[test]
fn a_tests_setting_under_a_second_is_taken_as_none_and_one_second_is_honoured() {
    let check = |ms: u64| {
        let s = served(&format!("setting-{}", ms), Some(ms));
        let mut c = Conn::to(s.port);
        if ms < 1000 {
            // Quiet for longer than it says, new and after an answer: still there.
            std::thread::sleep(Duration::from_secs(4));
            c.get("/");
            assert_eq!(c.answer().expect("closed before its first request").body, "home");
            std::thread::sleep(Duration::from_secs(6));
            c.get("/after");
            assert_eq!(c.answer().expect("closed after its answer").body, "after");
        } else {
            c.get("/");
            assert_eq!(c.answer().unwrap().body, "home");
            let took = c.closed_after();
            assert!(took >= Duration::from_millis(900) && took < Duration::from_secs(1) + Duration::from_secs(5) * slower(), "{} ms: closed {:?} after its answer", ms, took);
        }
    };
    let runs: Vec<_> = [0, 999, 1000].into_iter().map(|ms| std::thread::spawn(move || check(ms))).collect();
    for r in runs {
        r.join().unwrap();
    }
}

#[test]
fn a_whole_answer_longer_than_idle_leaves_its_connection_to_use_again() {
    let s = served("whole", Some(IDLE_MS));
    let mut c = Conn::to(s.port);
    c.get("/slow/6000");
    assert_eq!(c.answer().unwrap().body, "slow");
    std::thread::sleep(PAUSE);
    c.get("/after");
    assert_eq!(c.answer().expect("Q-942: closed after a 6 s answer").body, "after");
}

// The limit and one either side: streams ending 0.5 s before and after `IDLE`, an empty stream,
// and one of one event, longer than `IDLE`. Side by side, on connections of their own.
#[test]
fn streams_either_side_of_idle_empty_or_of_one_event_leave_their_connections_to_use_again() {
    let s = served("either-side", Some(IDLE_MS));
    let port = s.port;
    let cases = [("/stream/9/500", 9), ("/stream/11/500", 11), ("/quiet/5500", 0), ("/stream/1/5500", 1)];
    let runs: Vec<_> = cases.iter().map(|&(path, n)| std::thread::spawn(move || kept_after(&mut Conn::to(port), path, n))).collect();
    for r in runs {
        r.join().unwrap();
    }
}

// What must keep working: a connection with nothing to do for `IDLE` is closed, after a short
// answer as after a long one, counted from the answer's end.
#[test]
fn a_connection_idle_for_idle_after_its_answer_is_still_closed() {
    let s = served("idle", Some(IDLE_MS));
    let port = s.port;
    let check = move |path: &'static str| {
        let mut c = Conn::to(port);
        c.get(path);
        assert_eq!(c.answer().unwrap().status, 200);
        let took = c.closed_after();
        let idle = Duration::from_millis(IDLE_MS);
        // Counted from the stream's start, it was closed within two seconds of its end.
        assert!(took >= idle - Duration::from_secs(1) && took < idle + Duration::from_secs(5) * slower(), "{}: closed {:?} after its answer", path, took);
    };
    let long = std::thread::spawn(move || check("/stream/6/1000"));
    check("/");
    long.join().unwrap();
}

#[test]
fn requests_pipelined_with_a_long_stream_or_during_it_are_answered_and_the_connection_kept() {
    let s = served("pipelined", Some(IDLE_MS));
    // Sent with it.
    let mut c = Conn::to(s.port);
    c.send("GET /stream/6/1000 HTTP/1.1\r\nHost: x\r\n\r\nGET /after HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(c.answer().unwrap().body.matches("data: ").count(), 6);
    assert_eq!(c.answer().unwrap().body, "after");
    std::thread::sleep(PAUSE);
    c.get("/");
    assert_eq!(c.answer().expect("Q-942: closed after pipelined answers").body, "home");
    // Sent while it streams: answered after it.
    let mut c = Conn::to(s.port);
    c.get("/stream/6/1000");
    while !String::from_utf8_lossy(&c.buf).contains("data: ") {
        assert!(c.more());
    }
    c.get("/after");
    assert_eq!(c.answer().unwrap().body.matches("data: ").count(), 6);
    assert_eq!(c.answer().unwrap().body, "after");
    std::thread::sleep(PAUSE);
    c.get("/");
    assert_eq!(c.answer().expect("Q-942: closed after a request sent during a stream").body, "home");
}

#[test]
fn a_client_closing_during_a_stream_leaves_the_server_answering() {
    let s = served("client-closes", Some(IDLE_MS));
    // Gone in the middle.
    let mut c = Conn::to(s.port);
    c.get("/stream/1000/50");
    while !String::from_utf8_lossy(&c.buf).contains("data: ") {
        assert!(c.more());
    }
    drop(c);
    // Done sending, still reading: the stream goes to its end, then the connection is closed.
    let mut c = Conn::to(s.port);
    c.get("/stream/3/300");
    c.s.shutdown(std::net::Shutdown::Write).unwrap();
    assert_eq!(c.answer().unwrap().body.matches("data: ").count(), 3);
    assert!(c.closed_after() < Duration::from_secs(3) * slower());
    let mut c = Conn::to(s.port);
    c.get("/");
    assert_eq!(c.answer().unwrap().body, "home");
}

#[test]
fn http10_answers_say_close_and_end_with_their_connection_unless_kept_alive() {
    let s = served("http10", Some(IDLE_MS));
    let mut c = Conn::to(s.port);
    c.send("GET / HTTP/1.0\r\n\r\n");
    let a = c.answer().unwrap();
    assert!(a.says_close() && a.body == "home", "{}{}", a.head, a.body);
    assert!(c.closed_after() < Duration::from_secs(3) * slower());
    // A stream, which HTTP/1.0 cannot have in chunks, ends with the connection.
    let mut c = Conn::to(s.port);
    c.send("GET /stream/2/100 HTTP/1.0\r\n\r\n");
    let a = c.answer().unwrap();
    assert!(a.says_close() && !a.head.contains("transfer-encoding"), "{}", a.head);
    assert_eq!(a.body.matches("data: ").count(), 2, "{}", a.body);
    // Kept alive when asked, after a long answer too.
    let mut c = Conn::to(s.port);
    c.send("GET /slow/5500 HTTP/1.0\r\nConnection: keep-alive\r\n\r\n");
    let a = c.answer().unwrap();
    assert!(!a.says_close() && a.body == "slow", "{}", a.head);
    std::thread::sleep(PAUSE);
    c.send("GET /after HTTP/1.0\r\nConnection: keep-alive\r\n\r\n");
    assert_eq!(c.answer().expect("Q-942: an HTTP/1.0 keep-alive connection closed").body, "after");
}

#[test]
fn a_client_asking_to_close_is_told_so_by_a_stream_too() {
    let s = served("asks-close", Some(IDLE_MS));
    let mut c = Conn::to(s.port);
    c.send("GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    let a = c.answer().unwrap();
    assert!(a.says_close() && a.body == "home", "{}", a.head);
    assert!(c.closed_after() < Duration::from_secs(3) * slower());
    // A stream's head went before its end, and said nothing of the close that followed it.
    let mut c = Conn::to(s.port);
    c.send("GET /stream/2/100 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    let a = c.answer().unwrap();
    assert!(a.says_close(), "Q-942: a stream to a client that asked to close: {}", a.head);
    assert!(a.head.contains("transfer-encoding: chunked"), "{}", a.head);
    assert_eq!(a.body.matches("data: ").count(), 2, "{}", a.body);
    assert!(c.closed_after() < Duration::from_secs(3) * slower());
}

// A stop: an answer given while the server stops ends its connection, and says so. A request
// whose body was arriving when the stop came is still answered, a stream too.
#[test]
fn a_stream_begun_while_the_server_stops_says_close() {
    let s = served("stopping", Some(IDLE_MS));
    let mut plain = Conn::to(s.port);
    let mut streamed = Conn::to(s.port);
    // Its `100 Continue` says the server has the head, and is waiting for the body.
    for (c, path) in [(&mut plain, "/plain"), (&mut streamed, "/posted")] {
        c.send(&format!("POST {} HTTP/1.1\r\nHost: x\r\nExpect: 100-continue\r\nContent-Length: 6\r\n\r\nabc", path));
        assert_eq!(c.head().unwrap().0, 100);
    }
    unsafe { libc::kill(s.child.id() as i32, libc::SIGTERM) };
    // The port refusing connections says the stop has begun.
    let deadline = Instant::now() + Duration::from_secs(10) * slower();
    while TcpStream::connect(("127.0.0.1", s.port)).is_ok() {
        assert!(Instant::now() < deadline, "the stop did not begin");
        std::thread::sleep(Duration::from_millis(20));
    }
    plain.send("def");
    streamed.send("dé");
    let a = plain.answer().unwrap();
    assert!(a.says_close() && a.body == "abcdef", "{}{}", a.head, a.body);
    // Its head only: a stream's `send` during a stop ends it, as a client gone does.
    let (status, head) = streamed.head().unwrap();
    assert_eq!(status, 200, "{}", head);
    assert!(head.lines().any(|l| l.trim() == "connection: close"), "Q-942: a stream begun during a stop: {}", head);
    let t = Instant::now();
    while streamed.more() {}
    assert!(t.elapsed() < Duration::from_secs(3) * slower());
}

// A program's own `connection: close`, kept to on a stream as on a whole answer: said once, and
// the request sent after it on the connection is not answered. On a stream it was dropped and
// the connection kept.
#[test]
fn a_programs_own_connection_close_is_kept_to_on_a_stream_as_on_a_whole_answer() {
    let s = served("program-close", Some(IDLE_MS));
    for path in ["/closing", "/closing-whole"] {
        let mut c = Conn::to(s.port);
        c.send(&format!("GET {} HTTP/1.1\r\nHost: x\r\n\r\nGET /after HTTP/1.1\r\nHost: x\r\n\r\n", path));
        let a = c.answer().unwrap();
        assert!(a.says_close() && a.head.matches("connection:").count() == 1, "Q-942: {}: {}", path, a.head);
        assert_eq!(a.body, "ab");
        assert!(c.answer().is_none(), "{}: the connection was kept", path);
    }
}

// ---- Q-942: a request held up by an answer is timed from that answer's end ------------------
// A request head must come whole in `SLOW` (30 s), and a body must not stop for that long. One
// that had partly arrived with the request before it waited unread while that was answered, and
// was timed from before: behind an answer of over 30 s it was answered 408 as that answer ended.
// These run with the real `SLOW`, side by side.

/// Sends `first` with `part` of the next request; once `first` is answered, waits `wait` and
/// sends `rest`: what answers the next request.
fn held_up(port: u16, first: &str, part: &str, wait: Duration, rest: &str) -> Got {
    let mut c = Conn::to(port);
    c.s.set_read_timeout(Some(Duration::from_secs(60) * slower())).unwrap();
    c.send(&format!("GET {} HTTP/1.1\r\nHost: x\r\n\r\n{}", first, part));
    assert_eq!(c.answer().unwrap().body, "slow");
    std::thread::sleep(wait);
    c.send(rest);
    c.answer().expect("an answer")
}

const HEAD_PART: &str = "GET /after HTTP/1.1\r\nHo";
const BODY_PART: &str = "POST /plain HTTP/1.1\r\nHost: x\r\nContent-Length: 6\r\n\r\nabc";

#[test]
fn a_request_partly_sent_behind_a_long_answer_is_answered_when_its_rest_comes() {
    let s = served("held-up", None);
    let port = s.port;
    // Behind 31 s, the rest at once: a head, a body (unicode); behind 10 s, the rest 26 s later.
    let cases = [
        ("/slow/31000", HEAD_PART, 0, "st: x\r\n\r\n", "after"),
        ("/slow/31000", BODY_PART, 0, "dé", "abcdé"),
        ("/slow/10000", HEAD_PART, 26, "st: x\r\n\r\n", "after"),
    ];
    let runs: Vec<_> = cases
        .into_iter()
        .map(|(first, part, wait, rest, want)| {
            std::thread::spawn(move || {
                let a = held_up(port, first, part, Duration::from_secs(wait), rest);
                assert_eq!((a.status, a.body.as_str()), (200, want), "Q-942: behind {}, then {} s: {}", first, wait, a.head);
            })
        })
        .collect();
    for r in runs {
        r.join().unwrap();
    }
}

// What must keep working: one held up whose rest never comes gets its 408, `SLOW` after the
// answer's end (not 20 s after, as when timed from before a 10 s answer); one trickling in, with
// no answer before it, `SLOW` after it began, its bytes now and then not making it later.
#[test]
fn a_request_that_does_not_come_whole_still_gets_its_408_after_slow() {
    let s = served("still-408", None);
    let port = s.port;
    let late = Duration::from_secs(30) + Duration::from_secs(4) * slower();
    let behind = move |part: &'static str| {
        let mut c = Conn::to(port);
        c.s.set_read_timeout(Some(Duration::from_secs(60) * slower())).unwrap();
        c.send(&format!("GET /slow/10000 HTTP/1.1\r\nHost: x\r\n\r\n{}", part));
        assert_eq!(c.answer().unwrap().body, "slow");
        let t = Instant::now();
        let (status, _) = c.head().expect("a 408");
        assert!(status == 408 && t.elapsed() >= Duration::from_millis(29_500) && t.elapsed() < late, "Q-942: {} after {:?}", status, t.elapsed());
    };
    let head = std::thread::spawn(move || behind(HEAD_PART));
    let body = std::thread::spawn(move || behind(BODY_PART));
    // A header line every 5 s, the head never ending.
    let mut c = Conn::to(port);
    c.s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let t = Instant::now();
    c.send("GET /after HTTP/1.1\r\n");
    let mut lines = 0;
    let mut got = [0u8; 512];
    let n = loop {
        match c.s.read(&mut got) {
            Ok(n) => break n,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                assert!(t.elapsed() < late, "never let go");
                lines += 1;
                let _ = c.s.write_all(format!("x-{}: 1\r\n", lines).as_bytes());
            }
            Err(e) => panic!("{}", e),
        }
    };
    let took = t.elapsed();
    assert!(String::from_utf8_lossy(&got[..n]).starts_with("HTTP/1.1 408") && took >= Duration::from_secs(29) && took < late, "trickling: {:?} after {:?}", String::from_utf8_lossy(&got[..n]), took);
    head.join().unwrap();
    body.join().unwrap();
}

// ---- Q-949: a request's last bytes, come while the answers ahead are made, are read ---------
// Requests ahead whose answers pass 256 KiB wait in `pending`. The event saying the last bytes of
// a request behind them had come arrived while they were answered, and was used up starting the
// next of them without a read: that request waited unread for its 408, 30 s later, and an
// Expect one for its `100 Continue`.

/// `ahead`, then `last` but for its final two bytes, those sent `late` later: what answers each,
/// a `100 Continue` among them for an Expect `last`, whose body then goes. A stream's body is
/// counted in events, a long one in bytes.
fn last_bytes_late(port: u16, ahead: &[&str], last: &str, late: u64) -> Vec<(u16, String)> {
    let mut c = Conn::to(port);
    let mut out: String = ahead.iter().map(|p| format!("GET {} HTTP/1.1\r\nHost: x\r\n\r\n", p)).collect();
    out.push_str(&last[..last.len() - 2]);
    c.send(&out);
    std::thread::sleep(Duration::from_millis(late));
    c.send("\r\n");
    let mut got = next(&mut c, ahead.len() + 1);
    if got.last().is_some_and(|a| a.0 == 100) {
        c.send("abc");
        got.extend(next(&mut c, 1));
    }
    got.into_iter()
        .map(|(status, body)| match body {
            b if b.starts_with("data: ") => (status, format!("{} events", b.matches("data: ").count())),
            b if b.len() > 1000 => (status, format!("{} bytes", b.len())),
            b => (status, b),
        })
        .collect()
}

/// Each of `cases`, `ahead` with its `last`, 1, 10 and 50 ms late, side by side: the answers
/// `want` gives.
fn read_behind(name: &str, cases: Vec<(Vec<&'static str>, &'static str, Vec<(u16, &'static str)>)>) {
    let s = served(name, Some(IDLE_MS));
    let port = s.port;
    let mut runs = Vec::new();
    for (ahead, last, want) in cases {
        for late in [1, 10, 50] {
            let (ahead, want) = (ahead.clone(), want.clone());
            runs.push(std::thread::spawn(move || {
                let got = last_bytes_late(port, &ahead, last, late);
                let want: Vec<(u16, String)> = want.iter().map(|(s, b)| pair(*s, b)).collect();
                assert_eq!(got, want, "Q-949: behind {:?}, the last bytes {} ms late", ahead, late);
            }));
        }
    }
    for r in runs {
        r.join().unwrap();
    }
}

#[test]
fn a_requests_last_bytes_come_while_answers_ahead_wait_in_pending_are_read() {
    read_behind(
        "pending-read",
        vec![
            (vec!["/stream/2/15", "/big/300000", "/stream/2/15"], AFTER, vec![(200, "2 events"), (200, "300000 bytes"), (200, "2 events"), (200, "after")]),
            (vec!["/slow/40", "/big/300000", "/"], EXPECTING, vec![(200, "slow"), (200, "300000 bytes"), (200, "home"), (100, ""), (200, "abc")]),
        ],
    );
}

// What must keep working: the same with nothing in `pending`.
#[test]
fn a_requests_last_bytes_come_while_answers_ahead_are_made_are_read() {
    read_behind(
        "no-pending-read",
        vec![
            (vec!["/stream/2/15", "/", "/stream/2/15"], AFTER, vec![(200, "2 events"), (200, "home"), (200, "2 events"), (200, "after")]),
            (vec!["/slow/40", "/", "/"], EXPECTING, vec![(200, "slow"), (200, "home"), (200, "home"), (100, ""), (200, "abc")]),
        ],
    );
}

const AFTER: &str = "GET /after HTTP/1.1\r\nHost: x\r\n\r\n";

// ---- Q-948: a pipelined request's `100 Continue` comes after the answers ahead of it ---------
// A request with `Expect: 100-continue`, its head read with a request before it, had its
// `100 Continue` written at once, ahead of that request's answer, which the client then took it
// for.

/// The statuses and bodies of the next `n` answers on `c`, a `100 Continue` among them.
fn next(c: &mut Conn, n: usize) -> Vec<(u16, String)> {
    (0..n).map(|_| c.answer().map(|a| (a.status, a.body)).expect("an answer")).collect()
}

fn pair(status: u16, body: &str) -> (u16, String) {
    (status, body.to_string())
}

const EXPECTING: &str = "POST /plain HTTP/1.1\r\nHost: x\r\nExpect: 100-continue\r\nContent-Length: 3\r\n\r\n";

#[test]
fn a_pipelined_requests_100_continue_comes_after_the_answers_ahead_of_it() {
    let s = served("continue", Some(IDLE_MS));
    // Behind a short answer, a stream, and a 2 s answer.
    for first in ["/", "/stream/3/500", "/slow/2000"] {
        let mut c = Conn::to(s.port);
        c.send(&format!("GET {} HTTP/1.1\r\nHost: x\r\n\r\n{}", first, EXPECTING));
        let got = next(&mut c, 2);
        assert!(got[0].0 == 200 && got[1] == pair(100, ""), "Q-948: behind {}: {:?}", first, got);
        c.send("abc");
        assert_eq!(next(&mut c, 1), [pair(200, "abc")]);
    }
    // Two in a row: the first's 100 at once, the second's after the first's answer.
    let mut c = Conn::to(s.port);
    c.send(EXPECTING);
    assert_eq!(next(&mut c, 1), [pair(100, "")]);
    c.send(&format!("abc{}", EXPECTING));
    assert_eq!(next(&mut c, 2), [pair(200, "abc"), pair(100, "")], "Q-948: the second's 100 before the first's answer");
    c.send("dé");
    assert_eq!(next(&mut c, 1), [pair(200, "dé")]);
    // Part of the body with it, the rest at once without waiting: in order, a 100 at most between,
    // after the first answer.
    let mut c = Conn::to(s.port);
    c.send(&format!("GET / HTTP/1.1\r\nHost: x\r\n\r\n{}a", EXPECTING));
    c.send("bc");
    let mut got = next(&mut c, 2);
    if got[1].0 == 100 {
        got.extend(next(&mut c, 1));
        got.remove(1);
    }
    assert_eq!(got, [pair(200, "home"), pair(200, "abc")], "Q-948");
    // Behind an answer that ends the connection: no 100, and closed.
    let mut c = Conn::to(s.port);
    c.send(&format!("GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n{}", EXPECTING));
    assert_eq!(next(&mut c, 1), [pair(200, "home")], "Q-948");
    assert!(c.answer().is_none(), "Q-948: a 100 after connection: close");
}

// What must keep working: not pipelined, the 100 at once; a body sent with its head, alone or
// pipelined, owes none.
#[test]
fn a_100_continue_not_pipelined_comes_at_once_and_none_is_owed_for_a_body_sent_with_its_head() {
    let s = served("continue-kept", Some(IDLE_MS));
    let mut c = Conn::to(s.port);
    let t = Instant::now();
    c.send(EXPECTING);
    assert_eq!(next(&mut c, 1), [pair(100, "")]);
    assert!(t.elapsed() < Duration::from_secs(2) * slower(), "{:?}", t.elapsed());
    c.send("abc");
    assert_eq!(next(&mut c, 1), [pair(200, "abc")]);
    // The body with the head, alone and pipelined: no 100 is owed.
    let mut c = Conn::to(s.port);
    c.send(&format!("{}abcGET / HTTP/1.1\r\nHost: x\r\n\r\n{}abc", EXPECTING, EXPECTING));
    assert_eq!(next(&mut c, 3), [pair(200, "abc"), pair(200, "home"), pair(200, "abc")]);
}

