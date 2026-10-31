//! The server's corners that take a particular client to reach: a page that goes away at
//! the wrong moment, a server out of file descriptors, a debugger's requests that cross
//! what the program is doing, and a program that fails after its answer has started.

// Descriptors running out, resets and what follows them are the system's own: these were
// worked out on macOS, where coverage is measured.
#![cfg(target_os = "macos")]

mod support;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn until(mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("cover-server-{}", name)).canonicalize().unwrap();
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

// ---- a running server ----------------------------------------------------------------

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

/// `quillo run app.qwe` in `d`, with at most `descriptors` file descriptors when given.
fn run(d: &Path, args: &[&str], descriptors: Option<u32>) -> Srv {
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
        .args(["run", "app.qwe", "--port", &port.to_string()])
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
    assert!(until(|| support::listening(port)), "the server did not start: {}", s.log());
    s
}

fn connect(port: u16) -> TcpStream {
    let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s
}

/// Connections, each asking for something, until the server has no descriptor left to
/// take another. They are given back when dropped.
fn take_every_descriptor(srv: &Srv) -> Vec<TcpStream> {
    let mut held = Vec::new();
    let out = until(|| {
        let mut c = connect(srv.port);
        c.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        held.push(c);
        srv.log().contains("cannot accept a connection") || held.len() > 300
    });
    assert!(out && srv.log().contains("cannot accept a connection"), "{}", srv.log());
    held
}

/// Reads an answer's head and body, as far as `want` says, from a connection kept open.
fn read_until(s: &mut TcpStream, want: &[u8]) -> Vec<u8> {
    let mut got = Vec::new();
    let mut b = [0u8; 4096];
    while !got.windows(want.len()).any(|w| w == want) {
        match s.read(&mut b) {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&b[..n]),
        }
    }
    got
}

/// Makes closing `s` reset the connection: the server is told at once, with an error.
fn resets_when_closed(s: &TcpStream) {
    use std::os::fd::AsRawFd;
    let l = libc::linger { l_onoff: 1, l_linger: 0 };
    let ok = unsafe {
        libc::setsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_LINGER, &l as *const libc::linger as *const libc::c_void, std::mem::size_of::<libc::linger>() as libc::socklen_t)
    };
    assert_eq!(ok, 0);
}

// ---- a page's connection by hand ---------------------------------------------------------

/// A WebSocket's upgrade, on a connection that has a ticket.
fn upgrade(s: &mut TcpStream, ticket: &str) -> Vec<u8> {
    write!(
        s,
        "GET /_quillo/wt?t={} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
        ticket
    )
    .unwrap();
    read_until(s, b"\r\n\r\n")
}

fn ticket(a: &str) -> String {
    let body = a.split("\r\n\r\n").nth(1).unwrap_or("");
    let v: Value = serde_json::from_str(body).unwrap_or_else(|e| panic!("{}: {:?}", e, a));
    v["ticket"].as_str().expect("a ticket").to_string()
}

fn page(srv: &Srv) -> TcpStream {
    let t = srv.ticket();
    let mut s = connect(srv.port);
    let head = upgrade(&mut s, &t);
    assert!(head.starts_with(b"HTTP/1.1 101"), "{}", String::from_utf8_lossy(&head));
    s
}

impl Srv {
    fn ticket(&self) -> String {
        let a = support::send(self.port, "POST", "/_quillo/wt-ticket", &[], "");
        assert_eq!(a.status, 200, "{}", a.body);
        let v: Value = serde_json::from_str(&a.body).unwrap();
        v["ticket"].as_str().unwrap().to_string()
    }
}

/// A text frame from a page.
fn say(s: &mut TcpStream, text: &str) {
    let mask = [1u8, 2, 3, 4];
    assert!(text.len() < 126);
    let mut f = vec![0x81, 0x80 | text.len() as u8];
    f.extend_from_slice(&mask);
    f.extend(text.bytes().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    s.write_all(&f).unwrap();
}

// ---- a debugger --------------------------------------------------------------------------

struct Dap {
    child: Child,
    msgs: Receiver<Value>,
    seq: u64,
    held: Vec<Value>,
}

impl Dap {
    fn start() -> Dap {
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("dap").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let (tx, msgs) = channel();
        std::thread::spawn(move || loop {
            let mut len = 0;
            loop {
                let mut line = String::new();
                if out.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                if let Some(v) = line.strip_prefix("Content-Length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut buf = vec![0; len];
            out.read_exact(&mut buf).unwrap();
            if tx.send(serde_json::from_slice::<Value>(&buf).unwrap()).is_err() {
                return;
            }
        });
        Dap { child, msgs, seq: 1, held: Vec::new() }
    }

    fn wait(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        if let Some(i) = self.held.iter().position(&pred) {
            return self.held.remove(i);
        }
        let end = Instant::now() + Duration::from_secs(20);
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            let Ok(m) = self.msgs.recv_timeout(left) else { break };
            if pred(&m) {
                return m;
            }
            self.held.push(m);
        }
        panic!("no {}", what);
    }

    fn ask(&mut self, command: &str, args: Value) -> Value {
        let seq = self.seq;
        self.seq += 1;
        let body = json!({ "seq": seq, "type": "request", "command": command, "arguments": args }).to_string();
        let stdin = self.child.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
        self.wait(command, |m| m["type"] == "response" && m["request_seq"] == seq)
    }

    fn call(&mut self, command: &str, args: Value) -> Value {
        let r = self.ask(command, args);
        assert_eq!(r["success"], true, "{} failed: {}", command, r);
        r["body"].clone()
    }

    fn launch(dir: &Path, port: u16, breakpoints: Value) -> Dap {
        let mut d = Dap::start();
        d.call("initialize", json!({ "adapterID": "quillo", "linesStartAt1": true }));
        d.wait("initialized", |m| m["event"] == "initialized");
        d.call("launch", json!({ "program": dir.join("main.qwe"), "args": ["--port", port.to_string()], "cwd": dir }));
        d.call("setBreakpoints", json!({ "source": { "path": dir.join("main.qwe") }, "breakpoints": breakpoints }));
        d.call("setExceptionBreakpoints", json!({ "filters": ["errors"] }));
        d.call("configurationDone", json!({}));
        d
    }

    fn stopped(&mut self) -> Value {
        self.wait("stop", |m| m["event"] == "stopped")["body"].clone()
    }
}

impl Drop for Dap {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// GET `path` on a thread of its own (the program may stop while answering it).
fn get_aside(port: u16, path: &str) -> std::thread::JoinHandle<support::Answer> {
    let path = path.to_string();
    assert!(until(|| support::listening(port)), "the program did not start");
    std::thread::spawn(move || support::send(port, "GET", &path, &[], ""))
}

/// A statement in a route's pattern runs before the route has a frame: there is nothing
/// to show for it yet, and the program stops at its error all the same.
#[test]
fn a_default_in_a_routes_pattern_runs_outside_any_frame() {
    let src = "route GET /fine ? { n: int = if true {\n    let k = 2\n    k + 1\n} else {\n    0\n} } = n\n\
               route GET /bad ? { n: int = if true {\n    let k = 1 / (len(\"a\") - 1)\n    k\n} else {\n    0\n} } = n\n";
    let dir = folder("default-frame", &[("main.qwe", src)]);
    let port = support::free_port();
    let mut d = Dap::launch(&dir, port, json!([]));
    assert_eq!(get_aside(port, "/fine").join().unwrap().body, "3");

    let bad = get_aside(port, "/bad");
    let stop = d.stopped();
    assert_eq!(stop["reason"], "exception", "{}", stop);
    assert!(stop["text"].as_str().unwrap().contains("zero"), "{}", stop);
    assert_eq!(d.call("stackTrace", json!({ "threadId": 1 }))["stackFrames"], json!([]));
    d.call("continue", json!({ "threadId": 1 }));
    assert_eq!(bad.join().unwrap().status, 500);
}

/// The editor clears a breakpoint while its condition is running: the program does not stop.
#[test]
fn a_breakpoint_cleared_while_its_condition_runs_does_not_stop() {
    let src = "fn f() {\n    let a = 1\n    a + 1\n}\nroute GET / = f()\n";
    let dir = folder("cleared", &[("main.qwe", src)]);
    let port = support::free_port();
    let source = json!({ "path": dir.join("main.qwe") });
    // The condition holds: left alone, this breakpoint stops.
    let mut d = Dap::launch(&dir, port, json!([{ "line": 2, "condition": "sleep(1) == null" }]));
    let answer = get_aside(port, "/");
    assert_eq!(d.stopped()["reason"], "breakpoint");
    d.call("continue", json!({ "threadId": 1 }));
    assert_eq!(answer.join().unwrap().body, "2");

    d.call("setBreakpoints", json!({ "source": source, "breakpoints": [{ "line": 2, "condition": "sleep(1500) == null" }] }));
    let answer = get_aside(port, "/");
    std::thread::sleep(Duration::from_millis(500));
    d.call("setBreakpoints", json!({ "source": source, "breakpoints": [] }));
    assert_eq!(answer.join().unwrap().body, "2");
    d.call("threads", json!({}));
    assert!(!d.held.iter().any(|m| m["event"] == "stopped"), "{:?}", d.held);
}

/// A long session: an editor newer than this adapter asks for what it does not know,
/// again and again, and sets breakpoints in a long file.
#[test]
fn requests_it_does_not_know_are_each_refused_and_long_files_take_breakpoints() {
    let mut src = String::new();
    for i in 0..4000 {
        src.push_str(&format!("let a{} = {}\n", i, i));
    }
    src.push_str("route GET / = a7\n");
    let dir = folder("long-session", &[("main.qwe", &src)]);
    let mut d = Dap::start();
    d.call("initialize", json!({ "adapterID": "quillo" }));
    for i in 0..3000 {
        let name = format!("stepBack{}", i);
        let r = d.ask(&name, json!({}));
        assert_eq!(r["success"], false, "{}", r);
        assert_eq!(r["message"], format!("`{}` is not supported", name), "{}", r);
    }
    for line in [1, 2000, 4000] {
        let set = d.call("setBreakpoints", json!({ "source": { "path": dir.join("main.qwe") }, "breakpoints": [{ "line": line }] }));
        assert_eq!(set["breakpoints"][0]["verified"], true, "{}", set);
        assert_eq!(set["breakpoints"][0]["line"], line, "{}", set);
    }
}

/// `quillo dap` as a process of its own, closing or using up descriptors first: this test
/// binary run again, since neither can be done to a program from outside.
fn run_alone(mode: &str) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "alone", "--nocapture"])
        .env("QUILLO_COVER_ALONE", mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Runs only as `run_alone` asks.
#[test]
fn alone() {
    match std::env::var("QUILLO_COVER_ALONE").as_deref() {
        Ok("no-stdout") => {
            // SAFETY: nothing else in this process is using it.
            unsafe { libc::close(1) };
            let code = quillo::debug::serve(|_| 0);
            eprintln!("gave {}", code);
        }
        Ok("few-descriptors") => {
            // Every descriptor below 40 taken, and one left beyond them: enough for
            // the protocol's own, not for a pipe.
            while unsafe { libc::dup(2) } < 40 {}
            let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) }, 0);
            let was = limit;
            limit.rlim_cur = 42;
            assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
            quillo::debug::serve(move |_| {
                unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &was) };
                0
            });
        }
        _ => {}
    }
}

#[test]
fn the_debugger_with_no_stdout_to_speak_on_says_so() {
    let out = run_alone("no-stdout").wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("quillo dap: Bad file descriptor"), "{}", err);
    assert!(err.contains("gave 1"), "{}", err);
}

#[test]
fn the_debugger_says_when_it_cannot_show_the_programs_output() {
    let mut child = run_alone("few-descriptors");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let reading = {
        let seen = seen.clone();
        std::thread::spawn(move || {
            let mut b = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut b) {
                if n == 0 {
                    return;
                }
                seen.lock().unwrap().extend_from_slice(&b[..n]);
            }
        })
    };
    let text = || String::from_utf8_lossy(&seen.lock().unwrap()).into_owned();
    let mut ask = |seq: u32, command: &str, args: Value| {
        let body = json!({ "seq": seq, "type": "request", "command": command, "arguments": args }).to_string();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
    };
    ask(1, "launch", json!({ "program": "main.qwe" }));
    ask(2, "configurationDone", json!({}));
    assert!(until(|| text().contains("\"terminated\"")), "{}", text());
    assert!(text().contains("the program's output is not shown here: Too many open files"), "{}", text());
    assert!(text().contains("\"exitCode\":0"), "{}", text());
    ask(3, "disconnect", json!({}));
    assert!(child.wait().unwrap().success());
    reading.join().unwrap();
}

// ---- a server out of descriptors -------------------------------------------------------

/// A change to the program when no descriptor is left: the system cannot be asked to
/// watch, so the server looks for changes itself, and finds the next one.
#[test]
fn a_server_out_of_descriptors_watches_files_by_looking() {
    let d = folder("watch-fds", &[("app.qwe", "route GET / = \"one\"\n")]);
    let srv = run(&d, &["--watch"], Some(64));
    assert_eq!(srv.get("/").body, "one");
    let held = take_every_descriptor(&srv);
    std::fs::write(d.join("app.qwe"), "route GET / = \"two\"\n").unwrap();
    assert!(until(|| srv.log().contains("looking for changed files every 50 ms (the system cannot tell of them: Too many open files")), "{}", srv.log());
    drop(held);
    std::fs::write(d.join("app.qwe"), "route GET / = \"three\"\n").unwrap();
    assert!(until(|| srv.get("/").body.contains("three")), "{}", srv.log());
}

/// A page's upgrade that cannot get the second descriptor its reader needs: it is
/// hung up on, and that is all.
#[test]
fn a_page_connecting_when_no_descriptor_is_left_is_hung_up_on() {
    let d = folder("ws-fds", &[("app.qwe", "route GET / = \"hi\"\n")]);
    let srv = run(&d, &["--no-wt"], Some(64));
    let mut c = connect(srv.port);
    c.write_all(b"POST /_quillo/wt-ticket HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n").unwrap();
    let a = read_until(&mut c, b"}");
    let t = ticket(&String::from_utf8_lossy(&a));
    let held = take_every_descriptor(&srv);
    let head = upgrade(&mut c, &t);
    assert!(head.starts_with(b"HTTP/1.1 101"), "{}", String::from_utf8_lossy(&head));
    let mut b = [0u8; 16];
    assert_eq!(c.read(&mut b).unwrap(), 0, "the page was kept");
    drop(held);
    assert!(until(|| srv.get("/").body == "hi"), "{}", srv.log());
}

// ---- pages and clients that go ----------------------------------------------------------

/// A client gone by the time an answer that failed part way ends: the server finds it
/// cannot even shut the connection, and lets it go.
#[test]
fn an_answer_that_fails_after_its_client_went_is_let_go() {
    let src = "route GET /s = stream((send) => {\n    send(\"first\\n\")\n    sleep(400)\n    1 / (len(\"a\") - 1)\n})\nroute GET /ok = \"ok\"\n";
    let d = folder("gone-then-failed", &[("app.qwe", src)]);
    let srv = run(&d, &["--metrics"], None);
    let mut c = connect(srv.port);
    c.write_all(b"GET /s HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let got = read_until(&mut c, b"first");
    assert!(String::from_utf8_lossy(&got).contains("first"), "{:?}", got);
    resets_when_closed(&c);
    drop(c);
    assert!(until(|| srv.log().contains("error while streaming an answer")), "{}", srv.log());
    assert_eq!(srv.get("/ok").body, "ok");
    assert!(until(|| srv.connections() == 1), "{} open: {}", srv.connections(), srv.log());
}

/// A page that stopped reading and then left: what the server was sending it fails, and
/// it is taken out of its channels.
#[test]
fn a_page_that_stops_reading_and_leaves_is_noticed_by_what_is_sent_to_it() {
    let src = r#"route GET /flood {
    let big = "x".repeat(1000000)
    let sent = 0
    for i in 0..40 {
        sent = sent + channel.broadcast("chat", big)
    }
    sent
}
route GET /peers = len(channel.peers("chat"))
"#;
    let d = folder("page-left", &[("app.qwe", src)]);
    let srv = run(&d, &["--no-wt"], None);
    let mut p = page(&srv);
    say(&mut p, "[\"j\",\"chat\"]");
    assert!(until(|| srv.get("/peers").body == "1"), "{}", srv.log());
    assert_eq!(srv.get("/flood").body, "40");
    // More than the sockets between them hold: the server is in the middle of a write.
    std::thread::sleep(Duration::from_millis(300));
    resets_when_closed(&p);
    drop(p);
    assert!(until(|| srv.get("/peers").body == "0"), "{}", srv.log());
}

/// A server stopping, with a request still being answered, and a page that asks to join
/// a channel: the request is answered, and the channel's thread does not try for long.
#[test]
fn a_page_joining_as_the_server_stops_does_not_hold_it_up() {
    let src = "route GET /slow {\n    sleep(3000)\n    \"done\"\n}\nroute GET /ok = \"ok\"\n";
    let d = folder("join-stopping", &[("app.qwe", src)]);
    let mut srv = run(&d, &["--no-wt"], None);
    let mut p = page(&srv);
    let slow = get_aside(srv.port, "/slow");
    std::thread::sleep(Duration::from_millis(200));
    assert!(Command::new("kill").args(["-TERM", &srv.child.id().to_string()]).status().unwrap().success());
    assert!(until(|| !support::listening(srv.port)), "the server goes on listening: {}", srv.log());
    say(&mut p, "[\"j\",\"later\"]");
    assert_eq!(slow.join().unwrap().body, "done");
    assert!(until(|| srv.child.try_wait().unwrap().is_some()), "the server did not stop: {}", srv.log());
    assert!(srv.child.wait().unwrap().success(), "{}", srv.log());
}

/// A request's waits, two at once and then one: both threads that did the two look for
/// more, and only one of them can have the next. It is done, and the request answered.
#[test]
fn waits_one_after_another_are_all_done() {
    let src = "route GET /w/:n {\n    parallel(() => sleep(0), () => sleep(0))\n    sleep(0)\n    n\n}\n";
    let d = folder("waiting", &[("app.qwe", src)]);
    let srv = run(&d, &[], None);
    let mut s = connect(srv.port);
    for i in 0..1000 {
        let n = format!("n{}x", i);
        write!(s, "GET /w/{} HTTP/1.1\r\nHost: x\r\n\r\n", n).unwrap();
        let got = read_until(&mut s, n.as_bytes());
        assert!(got.starts_with(b"HTTP/1.1 200"), "{}", String::from_utf8_lossy(&got));
    }
}

// ---- a page that cannot be made ----------------------------------------------------------

/// In development a component that cannot run in the browser fails where it is used: its
/// page gets the program's whole script, whose first line says what is wrong.
#[test]
fn a_page_whose_bundle_cannot_be_made_loads_the_whole_programs_script() {
    let src = "component Home() {\n    <p>{ env(\"HOME\") }</p>\n}\nroute GET / = <html><body><Home/></body></html>\n";
    let d = folder("no-bundle", &[("app.qwe", src)]);
    let srv = run(&d, &["--watch", "--no-check"], None);
    let a = srv.get("/");
    assert_eq!(a.status, 200, "{}", a.body);
    assert!(a.body.contains("/_quillo/quillo.js"), "{}", a.body);
    assert!(!a.body.contains("/_quillo/b/"), "{}", a.body);
    assert!(srv.get("/_quillo/quillo.js").body.contains("`env` is not available in the browser"));
}

// ---- a panic, in a builtin of the test's own ---------------------------------------------

mod panics {
    use quillo::interp::Ctrl;
    use quillo::server::{serve_with, App, Loaded};
    use quillo::value::define;
    use quillo::{Interp, Value};
    use std::io::{Read, Write};
    use std::sync::mpsc::channel;
    use std::sync::Once;
    use std::time::Duration;

    fn boom(_: &mut Interp, _: Vec<Value>, _: u32) -> Result<Value, Ctrl> {
        panic!("boom in a stream");
    }

    /// This panic is expected; any other is shown as usual.
    fn quietly() {
        static SET: Once = Once::new();
        SET.call_once(|| {
            let shown = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |p| {
                if p.payload().downcast_ref::<&str>() != Some(&"boom in a stream") {
                    shown(p);
                }
            }));
        });
    }

    /// Past the head of a streamed answer, no status can say what went wrong: the
    /// connection ends without the stream's last chunk, and the server goes on.
    #[test]
    fn a_panic_in_a_stream_costs_that_connection_only() {
        quietly();
        let d = super::support::temp_dir("cover-server-panic");
        let (tx, rx) = channel();
        let (dir, main) = (d.clone(), d.join("main.qwe"));
        std::thread::spawn(move || {
            let mut it = Interp::new(&dir);
            define(&it.globals, "boom", Value::Builtin("boom", boom));
            let src = "route GET /s = stream((send) => {\n    send(\"first\\n\")\n    boom()\n})\nroute GET /fine = \"fine\"\n";
            it.load_source(src, main.to_str().unwrap()).unwrap();
            let shared = it.shared.clone();
            let app = App::new(Loaded::Ready(it), shared.clone(), &main, &dir, false);
            assert!(std::sync::Arc::ptr_eq(&app.shared(), &shared));
            serve_with(app, "127.0.0.1:0", 1, move |a| tx.send(a.port()).unwrap()).unwrap();
        });
        let port = rx.recv_timeout(Duration::from_secs(20)).expect("the server did not start");
        let mut c = super::connect(port);
        c.write_all(b"GET /s HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut got = String::new();
        c.read_to_string(&mut got).unwrap();
        assert!(got.starts_with("HTTP/1.1 200"), "{}", got);
        assert!(got.contains("first"), "{}", got);
        assert!(!got.ends_with("0\r\n\r\n"), "the stream looks complete: {}", got);
        assert_eq!(super::support::send(port, "GET", "/fine", &[], "").body, "fine");
    }
}

// ---- the window --------------------------------------------------------------------------

/// Run with QUILLO_TEST_WINDOW set, as it puts a window on the screen. There is no `open`
/// on the PATH here, so a link to another site can only be reported.
#[cfg(all(feature = "app", target_os = "macos"))]
#[test]
fn a_link_with_no_browser_to_open_it_is_reported() {
    if std::env::var("QUILLO_TEST_WINDOW").is_err() {
        return;
    }
    let src = "route GET / = <html><body><script src=\"/assets/go.js\"></script></body></html>\nroute GET /assets/*path = file(\"public/\" + path)\n";
    let d = folder("no-open", &[("app.qwe", src)]);
    std::fs::create_dir_all(d.join("public")).unwrap();
    std::fs::write(d.join("public/go.js"), "location.href = \"https://example.test/away\";\n").unwrap();
    let log = d.join("stderr.log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["app", "app.qwe"])
        .current_dir(&d)
        .env("HOME", &d)
        .env("PATH", d.join("nothing-here"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let _window = support::Guard::new(child);
    let out = || std::fs::read_to_string(&log).unwrap_or_default();
    assert!(until(|| out().contains("quillo: cannot open https://example.test/away in the browser: ")), "{}", out());
}

/// Closing the window ends the program, with nothing wrong. Pressing the window's close
/// button takes macOS's accessibility permission: without it, nothing can be said here.
#[cfg(all(feature = "app", target_os = "macos"))]
#[test]
fn closing_the_window_ends_the_program() {
    if std::env::var("QUILLO_TEST_WINDOW").is_err() {
        return;
    }
    let d = folder("close-window", &[("app.qwe", "route GET / = <html><body><p>hi</p></body></html>\n")]);
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["app", "app.qwe"])
        .current_dir(&d)
        .env("HOME", &d)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut window = support::Guard::new(child);
    let press = format!(
        "tell application \"System Events\" to tell (first process whose unix id is {}) to click (first button of window 1 whose subrole is \"AXCloseButton\")",
        window.0.id()
    );
    let mut refused = String::new();
    assert!(
        until(|| {
            let out = Command::new("osascript").args(["-e", &press]).output().unwrap();
            refused = String::from_utf8_lossy(&out.stderr).into_owned();
            out.status.success() || refused.contains("assistive")
        }),
        "the window was never there to close: {}",
        refused
    );
    if refused.contains("assistive") {
        eprintln!("not run: {}", refused);
        return;
    }
    assert!(until(|| window.0.try_wait().unwrap().is_some()), "the program went on after its window closed");
    assert!(window.0.wait().unwrap().success());
}
