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

// ---- Q-941: a lambda kept in a `let` no longer keeps its scope for ever ---------------------
// `let f = (x) => ...` put a function holding its scope into that scope's own variables: neither
// was freed when the scope ended, and each request leaked all the scope held, 1.5 MB in the
// report. The memory tests serve the report's routes and the other ways a scope ends (`return`,
// a view, a loop, a failure, a match arm, a component, a script) and measure the server over 40
// requests: on the branch's parent (QUILLO_TEST_BIN) each grows by about 75 MB. The others run
// in this process and count the holds left on a list they pass in.
mod q941 {
    use super::support;
    use quillo::value::{lookup, Value};
    use quillo::Interp;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// The quillo that serves: this build's, or the one QUILLO_TEST_BIN names.
    fn bin() -> PathBuf {
        std::env::var_os("QUILLO_TEST_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_quillo")))
    }

    /// Each route makes 20,000 rows (about 1.5 MB) in a scope that keeps a lambda.
    const ROUTES: &str = r#"fn rows() = range(20000) | map((i) => `row number ${i} with some text é😀`)
route GET /plain = {
    let big = rows()
    `${len(big)}`
}
route GET /stored = {
    let big = rows()
    let keep = (r) => len(r) > 3
    `${len(big | filter(keep))}`
}
route GET /stored-unused = {
    let big = rows()
    let one = (x) => x + 1
    `${len(big)} ${one(1)}`
}
fn returned() {
    let big = rows()
    let one = (x) => x + 1
    return `${len(big)} ${one(1)}`
}
route GET /return = returned()
view Card(n) {
    let big = rows()
    let small = (x) => x + n
    <p>{len(big)} {small(1)}</p>
}
route GET /view = <main><Card n={1}/></main>
route GET /loop {
    let t = 0
    for i in range(2) {
        let big = rows()
        let f = (x) => x + i
        t += f(len(big))
    }
    return `${t}`
}
fn boom(n) {
    let big = rows()
    let f = (x) => x + n
    return f(len(big)) + unwrap(err("no"))
}
route GET /fails = boom(1)
route GET /match/:n(int) = match n {
    1 => {
        let big = rows()
        let f = (x) => x + 1
        `${f(len(big))}`
    }
    _ => "other"
}
component Inner(data) {
    let shown = (x) => `${x} rows`
    <span>{shown(len(data))}</span>
}
component Outer() {
    <div><Inner data={rows()}/></div>
}
route GET /component = <main><Outer/></main>
route GET /script = run_script("let f = (x) => x + 1\nf(len(rows))", { rows: rows() })
component Two() {
    state a = 1
    state b = 2
    let bump = () => { a += 1 }
    <button onclick={bump}>{a + b}</button>
}
route GET /two = <main><Two/></main>
route GET /record = {
    let big = rows()
    let acts = { count: (r) => len(r), first: (r) => r[0] }
    `${acts.count(big)}`
}
route GET /collected = {
    let big = rows()
    let fs = []
    for i in range(3) {
        fs = push(fs, () => len(big) + i)
    }
    `${fs[2]()}`
}
route GET /nested = {
    let big = rows()
    let menu = [{ name: "a", go: () => len(big) }, { name: "b", go: () => 1 }]
    `${menu[0].go()}`
}
fn mk_list(big) {
    let tags = [1, 2, 3]
    let f = (x) => x + len(big) + len(tags)
    return f
}
route GET /mk-list = `${mk_list(rows())(1)}`
fn mk_record(big) {
    let acts = { go: (x) => x + len(big), pad: range(120) }
    return acts.go
}
route GET /mk-record = `${mk_record(rows())(1)}`
route GET /for-list = {
    let big = rows()
    let fs = for i in range(3) => () => len(big) + i
    `${fs[1]()}`
}
fn pick(v) {
    return match v {
        { data } => {
            let n = len(data)
            data = () => n
            data()
        }
        _ => 0
    }
}
route GET /match-bind = `${pick({ data: rows() })}`
route GET /param/:n {
    let big = rows()
    n = () => len(big)
    return `${len(big)}`
}
fn mkf(i) {
    let f = (x) => x + i
    return f
}
route GET /many = `${len(range(300) | map((i) => mkf(i)))}`
route GET /stream = {
    let big = rows()
    let seen = (x) => x < len(big)
    events((send) => {
        send({ seen: seen(1) })
    })
}
route GET /slow-stream = {
    let big = rows()
    let seen = (x) => x < len(big)
    events((send) => {
        sleep(400)
        send({ seen: seen(1) })
    })
}
route GET /slow-plain = {
    let big = rows()
    sleep(400)
    `${len(big)}`
}
route GET /live = {
    let seen = (x) => x > 1
    events((send) => {
        for i in range(4) {
            if seen(i) {
                send({ i, s: "é😀" })
            }
        }
    })
}
"#;

    /// A `quillo run` of `ROUTES` (no view cache, so that the view renders each time), killed
    /// when dropped, its folder removed.
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

    fn served(name: &str) -> Served {
        let dir = support::temp_dir(&format!("feedback65-q941-{}", name));
        std::fs::write(dir.join("main.qwe"), ROUTES).unwrap();
        let log = dir.join("server.log");
        let child = Command::new(bin())
            .arg("run")
            .arg(dir.join("main.qwe"))
            .args(["--port", "0", "--workers", "4", "--no-view-cache"])
            .env("QUILLO_SECRET", "test secret for the tests")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log).unwrap())
            .spawn()
            .unwrap();
        support::stop_with_tests(&child);
        let mut s = Served { child, port: 0, dir };
        let deadline = Instant::now() + Duration::from_secs(30);
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

    /// The server's memory in KiB: its footprint on macOS, which counts what the memory
    /// compressor holds (its resident memory does not), and its resident memory elsewhere.
    fn memory_kib(pid: u32) -> u64 {
        if cfg!(target_os = "macos") {
            let out = Command::new("footprint").args(["-p", &pid.to_string()]).output().expect("footprint");
            let text = String::from_utf8_lossy(&out.stdout);
            let at = text.split("Footprint: ").nth(1).unwrap_or_else(|| panic!("no footprint in {}", text));
            let mut words = at.split_whitespace();
            let n: f64 = words.next().and_then(|w| w.parse().ok()).expect("a number");
            let unit = match words.next() {
                Some("B") => 1.0 / 1024.0,
                Some("KB") => 1.0,
                Some("MB") => 1024.0,
                Some("GB") => 1024.0 * 1024.0,
                other => panic!("footprint's unit {:?}", other),
            };
            (n * unit) as u64
        } else {
            let status = std::fs::read_to_string(format!("/proc/{}/status", pid)).unwrap();
            let line = status.lines().find(|l| l.starts_with("VmRSS:")).unwrap().to_string();
            line.split_whitespace().nth(1).unwrap().parse().unwrap()
        }
    }

    /// `n` requests for `path`, four at a time; their statuses.
    fn load(port: u16, path: &str, n: usize) -> Vec<u16> {
        let next = std::sync::atomic::AtomicUsize::new(0);
        let statuses = std::sync::Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    while next.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < n {
                        let a = support::send(port, "GET", path, &[], "");
                        statuses.lock().unwrap().push(a.status);
                    }
                });
            }
        });
        statuses.into_inner().unwrap()
    }

    /// How much the server grows over 40 requests for `path`, after 8 to warm it.
    fn grows_kib(s: &Served, path: &str, status: u16) -> u64 {
        assert!(load(s.port, path, 8).iter().all(|&c| c == status), "{}", path);
        let before = memory_kib(s.child.id());
        assert!(load(s.port, path, 40).iter().all(|&c| c == status), "{}", path);
        memory_kib(s.child.id()).saturating_sub(before)
    }

    /// Of 40 requests' 60 MB: what a server keeps for its next requests, not a leak.
    const GROWS_KIB: u64 = 30 << 10;

    #[test]
    fn the_reports_routes_keep_nothing_of_a_request() {
        let s = served("report");
        let grew: Vec<(&str, u64)> = ["/plain", "/stored", "/stored-unused"].into_iter().map(|p| (p, grows_kib(&s, p, 200))).collect();
        assert!(grew.iter().all(|&(_, kib)| kib < GROWS_KIB), "Q-941: KiB grown over 40 requests: {:?}", grew);
        assert_eq!(support::send(s.port, "GET", "/stored-unused", &[], "").body, "20000 2");
    }

    #[test]
    fn a_lambda_kept_in_a_scope_goes_with_it_however_the_scope_ends() {
        let s = served("ends");
        let routes = [("/return", 200), ("/view", 200), ("/loop", 200), ("/fails", 500), ("/match/1", 200), ("/component", 200), ("/script", 200), ("/match-bind", 200), ("/param/3", 200)];
        let grew: Vec<(&str, u64)> = routes.into_iter().map(|(p, status)| (p, grows_kib(&s, p, status))).collect();
        assert!(grew.iter().all(|&(_, kib)| kib < GROWS_KIB), "Q-941: KiB grown over 40 requests: {:?}", grew);
        let got = |p: &str| support::send(s.port, "GET", p, &[], "").body;
        assert_eq!(got("/return"), "20000 2");
        assert!(got("/view").contains("<p>20000 2</p>"));
        assert_eq!(got("/loop"), "40001");
        assert_eq!(got("/match/1"), "20001");
        assert_eq!(got("/match/2"), "other");
        assert!(got("/component").contains("<span>20000 rows</span>"));
        assert_eq!(got("/script"), "20001");
        // A lambda assigned to a `match` binding, and to a route's parameter (review 4).
        assert_eq!([got("/match-bind"), got("/param/3")], ["20000", "20000"]);
        // A component's states each hold its scope until its island is made, its handler too.
        assert!(got("/two").contains("<button>3</button>"), "{}", got("/two"));
        // A stream's function keeps the scope it was made in, and the lambda kept there, while it runs.
        let live = got("/live");
        assert!(live.contains(r#"{"i":2,"s":"é😀"}"#) && live.contains(r#"{"i":3,"s":"é😀"}"#) && !live.contains(r#""i":1"#), "{}", live);
    }

    #[test]
    fn a_lambda_in_a_list_or_record_of_a_scope_goes_with_it() {
        // A record of handlers, a list a loop fills, a list of records of lambdas, a list a
        // `for` makes (FEEDBACK65.md, Q-941, S2b; branch feedback65).
        let s = served("boxes");
        let routes = ["/record", "/collected", "/nested", "/for-list", "/mk-list", "/mk-record"];
        let grew: Vec<(&str, u64)> = routes.into_iter().map(|p| (p, grows_kib(&s, p, 200))).collect();
        assert!(grew.iter().all(|&(_, kib)| kib < GROWS_KIB), "Q-941: KiB grown over 40 requests: {:?}", grew);
        let got = |p: &str| support::send(s.port, "GET", p, &[], "").body;
        assert_eq!([got("/record"), got("/collected"), got("/nested"), got("/for-list")], ["20000", "20002", "20000", "20001"]);
        // A function giving back its kept lambda beside a list or a record it made (S2d).
        assert_eq!([got("/mk-list"), got("/mk-record")], ["20004", "20001"]);
    }

    /// `n` requests for `path` all at once; their statuses.
    fn at_once(port: u16, path: &str, n: usize) -> Vec<u16> {
        std::thread::scope(|s| {
            let all: Vec<_> = (0..n).map(|_| s.spawn(|| support::send(port, "GET", path, &[], "").status)).collect();
            all.into_iter().map(|h| h.join().unwrap()).collect()
        })
    }

    #[test]
    fn scopes_held_by_streams_are_freed_as_requests_come_and_once_a_server_goes_quiet() {
        // qwertime's live stream: a kept `seen` and 1.5 MB of rows, held by the `events` function
        // until the stream ends (FEEDBACK65.md, Q-941 review 1). After a burst of 300 held rings
        // (`/many`), 100 streams one after another: the fix 6 -> 11 MB, quillo-base 6 -> 191 MB.
        let s = served("streams");
        assert_eq!(support::send(s.port, "GET", "/many", &[], "").body, "300");
        let before = memory_kib(s.child.id());
        for _ in 0..100 {
            assert!(support::send(s.port, "GET", "/stream", &[], "").body.contains(r#"{"seen":true}"#));
        }
        let grew = memory_kib(s.child.id()).saturating_sub(before);
        assert!(grew < GROWS_KIB, "Q-941: 100 streams grew the server by {} KiB", grew);
        // 30 streams at once, a quiet second or more, then 30 plain requests at once, which ask for
        // no look: the streams' rows were freed meanwhile and are used again (the fix +19 MB,
        // quillo-base and the fix without the event loop's look +56 MB).
        assert!(at_once(s.port, "/slow-stream", 30).iter().all(|&c| c == 200));
        let streamed = memory_kib(s.child.id());
        std::thread::sleep(Duration::from_millis(3000));
        assert!(at_once(s.port, "/slow-plain", 30).iter().all(|&c| c == 200));
        let grew = memory_kib(s.child.id()).saturating_sub(streamed);
        assert!(grew < 38 << 10, "Q-941: after a quiet second the server grew by {} KiB", grew);
    }

    // ---- in this process: the holds left on a list the calls were given ----

    const FORMS: &str = r#"fn in_body(rows) {
    let f = (x) => len(rows) + x
    return f(1)
}
fn last_value(rows) {
    let f = (x) => len(rows) + x
    f(2)
}
fn in_block(rows) {
    let t = {
        let f = (x) => len(rows) + x
        f(3)
    }
    return t
}
fn in_for(rows) {
    let t = 0
    for i in range(3) {
        let f = (x) => len(rows) + x + i
        t += f(0)
    }
    return t
}
fn in_while(rows) {
    let i = 0
    let t = 0
    while i < 3 {
        let f = (x) => len(rows) + x
        t += f(i)
        i += 1
    }
    return t
}
fn in_arm(rows, n) {
    return match n {
        1 => {
            let f = (x) => len(rows) + x
            f(n)
        }
        _ => 0
    }
}
fn mutual(rows, k) {
    let even = (n) => n == 0 ? len(rows) : odd(n - 1)
    let odd = (n) => n == 0 ? -len(rows) : even(n - 1)
    return even(k)
}
fn captures(rows) {
    let a = (x) => x + len(rows)
    let b = (y) => a(y) * 2
    return b(1)
}
fn nested_fn(rows, k) {
    fn go(i) {
        return i == 0 ? len(rows) : 1 + go(i - 1)
    }
    return go(k)
}
fn factory(rows) {
    let mk = () => (x) => x + len(rows)
    let g = mk()
    return g(1)
}
fn outer_var(rows) {
    let pick = null
    if len(rows) > 0 {
        let lim = 0
        pick = (x) => x > lim
    }
    return len([1, 2] | filter(pick))
}
fn fails(rows) {
    let f = (x) => len(rows) + x
    return f(1) + unwrap(err("no"))
}
fn loop_return(rows) {
    for x in [1, 2, 3] {
        let big = (y) => y + len(rows) > 2
        if big(x) {
            return x
        }
    }
    return -1
}
fn default_lambda(rows, f = (x) => x + len(rows)) {
    return f(1)
}
fn ñandú(rows) {
    let ñ = (x) => `${x}é😀${len(rows)}`
    return ñ("ü")
}
fn empty(rows) {
    let n = len(rows)
    let t = {
        let f = () => null
        f()
    }
    return [n, t]
}
fn par(rows) {
    let a = () => len(rows)
    let b = () => 2
    return parallel(a, b)
}
fn shared(rows) {
    let n = 0
    let inc = () => { n = n + len(rows) }
    inc()
    inc()
    return n
}
fn script_kept(rows) = run_script("let f = (x) => x + 1\nf(len(rows))", { rows })
fn make(rows) {
    let f = (x) => x + len(rows)
    return f
}
fn use_once(rows) {
    let g = make(rows)
    return g(1)
}
let kept = {}
fn keep(rows) {
    let f = (x) => x + len(rows)
    kept = set(kept, "f", f)
    return kept.f(1)
}
fn forget() {
    kept = {}
    return 0
}
fn listed(rows) {
    let add = (x) => x + len(rows)
    return [add, { f: add }]
}
fn counter(rows) {
    let n = 0
    let inc = () => { n = n + 1 }
    let get = () => n + len(rows)
    return { inc, get }
}
fn same() {
    fn h() {
        return h
    }
    return h
}
fn same_kept() {
    let s = same()
    return s() == s
}
fn aliased(rows) {
    let f = (x) => len(rows) + x
    let g = f
    let top = in_body
    return g(1) + top([])
}
fn many_vars(rows) {
    let v1 = 1
    let v2 = 2
    let v3 = 3
    let v4 = 4
    let v5 = 5
    let v6 = 6
    let v7 = 7
    let v8 = 8
    let v9 = 9
    let v10 = 10
    let v11 = 11
    let v12 = 12
    let v13 = 13
    let v14 = 14
    let v15 = 15
    let v16 = 16
    let v17 = 17
    let f = (x) => x + len(rows) + v17
    return f(v1) + v2 + v3 + v4 + v5 + v6 + v7 + v8 + v9 + v10 + v11 + v12 + v13 + v14 + v15 + v16 - 135
}
fn while_return(rows) {
    let i = 0
    while i < 5 {
        let f = (x) => x + len(rows)
        if f(i) > 4 {
            return i
        }
        i += 1
    }
    return -1
}
fn range_return(rows) {
    for i in range(5) {
        let f = (x) => x + len(rows)
        if f(i) > 4 {
            return i
        }
    }
    return -1
}
fn cond_fails(rows) {
    let i = 0
    while i < len(rows) && unwrap(err("no")) {
        i += 1
    }
    return i
}
fn escapes_each_step(rows) {
    let fs = []
    for i in range(3) {
        let f = () => i + len(rows)
        fs = push(fs, f)
    }
    return fs | map((f) => f())
}
fn inline_returned(rows) {
    let f = (x) => x + len(rows)
    return () => f(1)
}
fn record(rows) {
    let acts = { count: (r) => len(r) + len(rows), first: (r) => r }
    return acts.count([1])
}
fn loop_list(rows) {
    let fs = []
    for i in range(3) {
        fs = push(fs, () => len(rows) + i)
    }
    return fs[2]()
}
fn for_list(rows) {
    let fs = for i in range(3) => () => len(rows) + i
    return fs[1]()
}
fn nested_boxes(rows) {
    let menu = [{ name: "a", go: () => len(rows) }, { name: "b", go: () => 1 }]
    return menu[0].go()
}
fn big_next_to_box(rows) {
    let big = range(20000) | map((i) => i)
    let acts = { go: () => len(rows) }
    return acts.go() + len(big) - 20000
}
fn box_and_direct(rows) {
    let f = (x) => x + len(rows)
    let acts = [f, { again: f }]
    return acts[0](1) + acts[1].again(0) - len(rows)
}
fn make_acts(rows) {
    let acts = { go: (x) => x + len(rows) }
    return acts
}
let cached = {}
fn cache_acts(rows) {
    let acts = { go: (x) => x + len(rows) }
    cached = set(cached, "acts", acts)
    return cached.acts.go(1)
}
fn match_bind(rows) {
    return match { data: rows } {
        { data } => {
            let n = len(data)
            data = () => n
            data()
        }
        _ => 0
    }
}
fn loop_push_kept(rows) {
    let fs = []
    for i in range(50) {
        let f = (x) => x + i + len(rows)
        fs = push(fs, f)
    }
    return fs[49](0)
}
fn loop_push_tags(rows) {
    let fs = []
    for i in range(50) {
        let tags = [i, i + 1]
        let f = (x) => x + i + len(tags) + len(rows)
        fs = push(fs, f)
    }
    return fs[49](0)
}
fn mk_list(rows) {
    let tags = range(100)
    let f = (x) => x + len(rows) + len(tags)
    return f
}
fn mk_record(rows) {
    let acts = { go: (x) => x + len(rows), pad: range(120) }
    return acts.go
}
let many = []
fn mkr(rows, i) {
    let f = () => i + len(rows)
    return f
}
fn hold_many(rows, n) {
    for i in range(n) {
        many = push(many, mkr(rows, i))
    }
    return len(many)
}
fn let_go_of_many() {
    many = []
    return 0
}
"#;

    fn app(src: &str) -> Interp {
        let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
        if let Err(e) = it.load_source(src, "test.qwe") {
            panic!("{}\n--- source ---\n{}", e, src);
        }
        it
    }

    /// `n` rows, unicode in them.
    fn rows(n: usize) -> Arc<Vec<Value>> {
        Arc::new((0..n).map(|i| Value::str(format!("row {} é😀", i))).collect())
    }

    fn call(it: &mut Interp, f: &Value, args: Vec<Value>) -> Result<Value, ()> {
        it.call(f, args, 0).map_err(|_| ())
    }

    fn call_named(it: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, ()> {
        let f = lookup(&it.globals, name).unwrap_or_else(|| panic!("no {}", name));
        call(it, &f, args)
    }

    /// `name(rows, extra...)` called `times` times: what the last call gave, shown, and the holds
    /// left on the rows besides this test's.
    fn left(it: &mut Interp, name: &str, rows: &Arc<Vec<Value>>, extra: &[Value], times: usize) -> (String, usize) {
        let f = lookup(&it.globals, name).unwrap_or_else(|| panic!("no {}", name));
        let mut last = String::new();
        for _ in 0..times {
            let mut args = vec![Value::List(rows.clone())];
            args.extend(extra.iter().cloned());
            last = call(it, &f, args).map_or_else(|_| "failed".to_string(), |v| v.repr());
        }
        (last, Arc::strong_count(rows) - 1)
    }

    #[test]
    fn a_lambda_kept_in_a_let_is_freed_with_its_scope_in_every_form() {
        let mut it = app(FORMS);
        let three = rows(3);
        let int = Value::Int;
        for (name, extra, want) in [
            ("in_body", vec![], "4"),
            ("last_value", vec![], "5"),
            ("in_block", vec![], "6"),
            ("in_for", vec![], "12"),
            ("in_while", vec![], "12"),
            ("in_arm", vec![int(1)], "4"),
            ("in_arm", vec![int(2)], "0"),
            ("mutual", vec![int(7)], "-3"),
            ("captures", vec![], "8"),
            ("nested_fn", vec![int(50)], "53"),
            ("factory", vec![], "4"),
            ("outer_var", vec![], "2"),
            ("fails", vec![], "failed"),
            ("loop_return", vec![], "1"),
            ("default_lambda", vec![], "4"),
            ("ñandú", vec![], "\"üé😀3\""),
            ("empty", vec![], "[3, null]"),
            ("par", vec![], "[3, 2]"),
            ("shared", vec![], "6"),
            ("script_kept", vec![], "4"),
            // The same function in two variables, a top-level one among them; a scope of more
            // than 16 variables; loops left by `return`; a loop whose condition fails.
            ("aliased", vec![], "5"),
            ("many_vars", vec![], "21"),
            ("while_return", vec![], "2"),
            ("range_return", vec![], "2"),
            ("cond_fails", vec![], "failed"),
            // In a list or map of the scope (S2b): a record, a list a loop fills, a `for`'s list,
            // records in a list, next to 20,000 plain values, one kept both ways.
            ("record", vec![], "4"),
            ("loop_list", vec![], "5"),
            ("for_list", vec![], "4"),
            ("nested_boxes", vec![], "3"),
            ("big_next_to_box", vec![], "3"),
            // Assigned to a `match` binding (review 4); a loop keeping each step's lambda in a
            // list, which keeps them all and works.
            ("match_bind", vec![], "3"),
        ] {
            let (got, held) = left(&mut it, name, &three, &extra, 3);
            assert_eq!(got, want, "{}", name);
            assert_eq!(held, 0, "Q-941: {} left {} holds on what it was given", name, held);
        }
        // Kept both in a variable and in a list of the same scope: listed as its scope ends, since
        // the list's hold and an outside one look alike there (S2d, review of S2c 1), and freed by
        // the next look, which a listing makes.
        let both = rows(3);
        let (got, held) = left(&mut it, "box_and_direct", &both, &[], 3);
        assert_eq!(got, "4");
        assert!(held <= 1, "Q-941: box_and_direct left {} holds", held);
        left(&mut it, "use_once", &rows(3), &[], 2);
        assert_eq!(Arc::strong_count(&both) - 1, 0, "Q-941: box_and_direct not freed by the next look");
        // One called 10,000 times; no rows, and one.
        assert_eq!(left(&mut it, "in_body", &three, &[], 10_000), ("4".to_string(), 0));
        assert_eq!(left(&mut it, "in_for", &rows(0), &[], 3), ("3".to_string(), 0));
        assert_eq!(left(&mut it, "mutual", &rows(1), &[int(0)], 3), ("1".to_string(), 0));
    }

    #[test]
    fn a_kept_lambda_that_escapes_keeps_its_scope_while_held_and_works() {
        let mut it = app(FORMS);
        let rows = rows(3);
        let made = lookup(&it.globals, "make").unwrap();
        // Returned: works, and holds what it was made with, as long as it is held.
        let f = call(&mut it, &made, vec![Value::List(rows.clone())]).unwrap();
        assert_eq!(call(&mut it, &f, vec![Value::Int(1)]).unwrap().repr(), "4");
        // Calls that end scopes in rings (and look at those still held) leave it working.
        assert_eq!(left(&mut it, "use_once", &rows, &[], 100).0, "4");
        assert!(Arc::strong_count(&rows) > 1);
        assert_eq!(call(&mut it, &f, vec![Value::Int(2)]).unwrap().repr(), "5");
        // In a list and a record; a global map's; sharing a variable, from outside too.
        let both = call_named(&mut it, "listed", vec![Value::List(rows.clone())]).unwrap();
        let Value::List(l) = &both else { panic!("{}", both.repr()) };
        assert_eq!(call(&mut it, &l[0], vec![Value::Int(1)]).unwrap().repr(), "4");
        let (got, _) = left(&mut it, "keep", &rows, &[], 1);
        assert_eq!(got, "4");
        let c = call_named(&mut it, "counter", vec![Value::List(rows.clone())]).unwrap();
        let Value::Map(m) = &c else { panic!("{}", c.repr()) };
        let (inc, get) = (m.get("inc").unwrap().clone(), m.get("get").unwrap().clone());
        call(&mut it, &inc, vec![]).unwrap();
        call(&mut it, &inc, vec![]).unwrap();
        assert_eq!(left(&mut it, "use_once", &rows, &[], 100).0, "4");
        assert_eq!(call(&mut it, &get, vec![]).unwrap().repr(), "5");
        assert_eq!(call_named(&mut it, "same_kept", vec![]).unwrap().repr(), "true");
        // Kept in its scope, called by a lambda given back; kept in a loop's scope and put in a list
        // each step.
        let back = call_named(&mut it, "inline_returned", vec![Value::List(rows.clone())]).unwrap();
        assert_eq!(call(&mut it, &back, vec![]).unwrap().repr(), "4");
        drop(back);
        assert_eq!(left(&mut it, "escapes_each_step", &rows, &[], 3).0, "[3, 4, 5]");
        assert_eq!(left(&mut it, "loop_push_kept", &rows, &[], 3).0, "52");
        assert_eq!(left(&mut it, "loop_push_tags", &rows, &[], 3).0, "54");
        // A record of lambdas given back, and one put in a global map: they work while held.
        let acts = call_named(&mut it, "make_acts", vec![Value::List(rows.clone())]).unwrap();
        let Value::Map(a) = &acts else { panic!("{}", acts.repr()) };
        assert_eq!(call(&mut it, a.get("go").unwrap(), vec![Value::Int(1)]).unwrap().repr(), "4");
        assert_eq!(left(&mut it, "cache_acts", &rows, &[], 1).0, "4");
        let Some(Value::Map(cached)) = lookup(&it.globals, "cached") else { panic!("no cached") };
        assert_eq!(call(&mut it, cached.get("acts").and_then(|m| if let Value::Map(m) = m { m.get("go").cloned() } else { None }).as_ref().unwrap(), vec![Value::Int(2)]).unwrap().repr(), "5");
        drop((a, cached));
        let Some(Value::Map(kept)) = lookup(&it.globals, "kept") else { panic!("no kept") };
        assert_eq!(call(&mut it, kept.get("f").unwrap(), vec![Value::Int(2)]).unwrap().repr(), "5");
        drop(kept);
        // Let go of, each is freed once the scopes in rings are looked at again (a list of them
        // looked at when it doubles): 100 more calls leave at most the last look's few.
        call_named(&mut it, "forget", vec![]).unwrap();
        drop((f, both, c, inc, get, acts));
        let (got, held) = left(&mut it, "use_once", &rows, &[], 100);
        assert_eq!(got, "4");
        assert!(held < 40, "Q-941: {} holds left after the lambdas were let go of", held);
    }

    #[test]
    fn thousands_of_lambdas_held_outside_at_once_are_freed_once_let_go() {
        let mut it = app(FORMS);
        let rows = rows(1);
        // Each call's scope keeps its lambda, which a global list holds: 3,000 rings held outside.
        assert_eq!(left(&mut it, "hold_many", &rows, &[Value::Int(3000)], 1).0, "3000");
        assert!(Arc::strong_count(&rows) > 1);
        // The list let go of, the next looks free them: at most a look's few are left.
        call_named(&mut it, "let_go_of_many", vec![]).unwrap();
        let (got, held) = left(&mut it, "use_once", &rows, &[], 5000);
        assert_eq!(got, "2");
        assert!(held < 100, "Q-941: {} holds left of 3,000 rings let go of", held);
    }

    #[test]
    fn no_call_waits_on_a_look_over_every_held_ring() {
        // 200,000 rings held at once, then let go: each call looks at two at most, where a look
        // over all of them had held one call up 38 ms with 300,000 (review 5). Measured twice, so
        // that a machine pausing the test does not count.
        let mut it = app(FORMS);
        let rows = rows(1);
        let made = lookup(&it.globals, "mkr").unwrap();
        let used = lookup(&it.globals, "use_once").unwrap();
        let mut worst = Vec::new();
        for round in 0..2 {
            let mut most = Duration::ZERO;
            let n = if round == 0 { 200_000 } else { 20_000 };
            let mut held = Vec::with_capacity(n as usize);
            for i in 0..n {
                let t = Instant::now();
                held.push(call(&mut it, &made, vec![Value::List(rows.clone()), Value::Int(i)]).unwrap());
                most = most.max(t.elapsed());
            }
            drop(held);
            for _ in 0..2_000 {
                let t = Instant::now();
                assert_eq!(call(&mut it, &used, vec![Value::List(rows.clone())]).unwrap().repr(), "2");
                most = most.max(t.elapsed());
            }
            worst.push(most);
        }
        let least = worst.iter().min().unwrap();
        assert!(*least < Duration::from_millis(10), "Q-941: a call took {:?} (each round's worst: {:?})", least, worst);
    }

    #[test]
    fn a_seconds_look_stays_short_and_frees_lambdas_beside_lists_over_the_next_ones() {
        // 20,000 lambdas each kept beside a list or a record of their scope's, held, then let go
        // (S2d, review of S2c 3): each `look_again` (an event loop's second) takes well under a
        // millisecond, the rest left for the next, and they free them all.
        let mut it = app(FORMS);
        let rows = rows(1);
        let mut held = Vec::new();
        for i in 0..20_000 {
            let f = if i % 2 == 0 { "mk_list" } else { "mk_record" };
            held.push(call_named(&mut it, f, vec![Value::List(rows.clone())]).unwrap());
        }
        assert_eq!(call(&mut it, &held[0], vec![Value::Int(1)]).unwrap().repr(), "102");
        assert_eq!(call(&mut it, &held[1], vec![Value::Int(1)]).unwrap().repr(), "2");
        drop(held);
        let mut worst = Duration::ZERO;
        let mut ticks = 0;
        while Arc::strong_count(&rows) > 1 && ticks < 2_000 {
            let t = Instant::now();
            quillo::value::look_again();
            worst = worst.max(t.elapsed());
            ticks += 1;
        }
        assert_eq!(Arc::strong_count(&rows) - 1, 0, "Q-941: {} ticks left holds", ticks);
        assert!(ticks > 1, "one look took them all");
        assert!(worst < Duration::from_millis(5), "Q-941: a second's look took {:?}", worst);
    }

    #[test]
    fn a_list_up_to_the_budget_of_items_is_looked_into() {
        // `RING_ITEMS` is 128: a list of 127 or 128 items, a lambda among them, is freed with its
        // scope; one of 129 is passed over, as it was, and works (FEEDBACK65.md, Q-941, S2b).
        for n in [127, 128, 129] {
            let src = format!("fn listed(rows) {{\n    let xs = [(x) => x + len(rows)] + range({})\n    return xs[0](1) + len(xs)\n}}\n", n - 1);
            let mut it = app(&src);
            let (got, held) = left(&mut it, "listed", &rows(3), &[], 3);
            assert_eq!(got, (4 + n).to_string(), "{} items", n);
            if n <= 128 {
                assert_eq!(held, 0, "Q-941: a list of {} items left {} holds", n, held);
            }
        }
    }

    /// A function whose body keeps `n` lambdas, each calling the one before.
    fn lambdas(n: usize) -> String {
        let mut s = "fn many(rows) {\n    let f1 = (x) => x + len(rows)\n".to_string();
        for i in 2..=n {
            s.push_str(&format!("    let f{} = (x) => f{}(x) + 1\n", i, i - 1));
        }
        s.push_str(&format!("    return f{}(0)\n}}\n", n));
        s
    }

    /// The same in a loop's body, two rounds.
    fn looped(n: usize) -> String {
        let mut s = "fn many(rows) {\n    let t = 0\n    for k in range(2) {\n        let f1 = (x) => x + len(rows) + k\n".to_string();
        for i in 2..=n {
            s.push_str(&format!("        let f{} = (x) => f{}(x) + 1\n", i, i - 1));
        }
        s.push_str(&format!("        t += f{}(0)\n    }}\n    return t\n}}\n", n));
        s
    }

    /// A function keeping a lambda made `depth` blocks inside the scope it is kept in.
    fn deep(depth: usize) -> String {
        let mut s = "fn deep(rows) {\n    let pick = null\n".to_string();
        for d in 1..=depth {
            let t = if d == 1 { "t" } else { "_t" };
            s.push_str(&format!("{}let {}{} = {{\n{}let a{} = {}\n", "    ".repeat(d), t, d, "    ".repeat(d + 1), d, d));
        }
        s.push_str(&format!("{}pick = (x) => x + len(rows)\n", "    ".repeat(depth + 1)));
        for d in (1..=depth).rev() {
            s.push_str(&format!("{}a{}\n{}}}\n", "    ".repeat(d + 1), d, "    ".repeat(d)));
        }
        s.push_str("    return pick(t1)\n}\n");
        s
    }

    #[test]
    fn rings_up_to_eight_lambdas_or_seven_blocks_deep_are_freed_and_larger_ones_still_work() {
        for n in [1, 7, 8, 9] {
            let mut it = app(&lambdas(n));
            let (got, held) = left(&mut it, "many", &rows(3), &[], 3);
            assert_eq!(got, (n + 2).to_string(), "{} lambdas", n);
            // Past eight it is left as it was: working, its scope kept.
            if n <= 8 {
                assert_eq!(held, 0, "Q-941: {} lambdas left {} holds", n, held);
            }
        }
        for n in [8, 9] {
            let mut it = app(&looped(n));
            let (got, held) = left(&mut it, "many", &rows(3), &[], 3);
            assert_eq!(got, (2 * (n + 2) + 1).to_string(), "{} lambdas in a loop", n);
            if n <= 8 {
                assert_eq!(held, 0, "Q-941: {} lambdas in a loop left {} holds", n, held);
            }
        }
        for depth in [1, 7, 8, 17] {
            let mut it = app(&deep(depth));
            let (got, held) = left(&mut it, "deep", &rows(3), &[], 3);
            assert_eq!(got, "4", "{} deep", depth);
            if depth <= 7 {
                assert_eq!(held, 0, "Q-941: a lambda {} blocks deep left {} holds", depth, held);
            }
        }
    }
}
