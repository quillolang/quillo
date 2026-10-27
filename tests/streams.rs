//! Answers sent as they are made (`stream`, `events`: src/stream.rs), and
//! read as they arrive (`http.stream`, and `listen`/`receive` in the browser).

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const APP: &str = r#"
let sent = 0
let finished = 0

route GET /count = stream((send) => {
    for i in range(4) {
        send(`line ${i}\n`)
        sleep(300)
    }
})

route GET /progress = events((send) => {
    for i in range(3) { send({ done: i + 1, of: 3 }, "progress") }
    send("finished")
})

route GET /forever = events((send) => {
    for i in range(100000) {
        send(i)
        sent = sent + 1
        sleep(50)
    }
    finished = finished + 1
})

route GET /big = stream((send) => {
    let line = repeat("x", 1023) + "\n"
    for i in range(20000) { send(line) }
}, { type: "text/csv", headers: { "content-disposition": "attachment" } })

route GET /lines = stream((send) => {
    for i in range(3) { send({ i }) }
}, { type: "application/x-ndjson" })

route GET /broken = stream((send) => {
    send("before")
    send(int("x") + 1)
})

route GET /relay/:port = events((send) => http.stream(`http://127.0.0.1:${port}/progress`, send))

route GET /collect/:port {
    let got = []
    let r = http.stream(`http://127.0.0.1:${port}/lines`, (x) => { got = got + [x] })
    { status: r.status, got }
}

route GET /first/:port {
    let got = []
    http.stream(`http://127.0.0.1:${port}/forever`, (x) => {
        got = got + [x]
        len(got) < 3
    })
    got
}

route GET /text/:port {
    let pieces = []
    http.stream(`http://127.0.0.1:${port}/count`, (t) => { pieces = pieces + [t] })
    join(pieces, "")
}

route GET /sent = str(sent)
route GET /finished = str(finished)
route GET / = "ok"
"#;

fn get(it: &mut Interp, method: &str, url: &str) -> HttpResponse {
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: vec![("Host".into(), "localhost".into())], body: Vec::new(), remote: "127.0.0.1".into() })
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-streams-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("app.qwe"), APP).unwrap();
    d
}

#[test]
fn outside_the_server_the_answer_is_made_whole() {
    let d = dir("whole");
    let mut it = Interp::new(&d);
    it.load_file(&d.join("app.qwe")).unwrap();
    let r = get(&mut it, "GET", "/progress");
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type"), Some("text/event-stream"));
    assert_eq!(r.header("x-quillo-stream"), None);
    assert_eq!(
        String::from_utf8_lossy(&r.body),
        "event: progress\ndata: {\"done\":1,\"of\":3}\n\nevent: progress\ndata: {\"done\":2,\"of\":3}\n\nevent: progress\ndata: {\"done\":3,\"of\":3}\n\ndata: \"finished\"\n\n"
    );
    let r = get(&mut it, "GET", "/lines");
    assert_eq!(String::from_utf8_lossy(&r.body), "{\"i\":0}\n{\"i\":1}\n{\"i\":2}\n");
    assert_eq!(r.header("content-type"), Some("application/x-ndjson"));
    assert_eq!(get(&mut it, "GET", "/broken").status, 500);
    assert!(get(&mut it, "HEAD", "/lines").body.is_empty());
}

#[test]
fn misuse_is_an_error() {
    let mut it = Interp::new(".");
    for (src, needle) in [
        ("fn f() = stream(1)", "takes a function that sends"),
        ("fn f() = events((s) => s(1), 2)", "options as a map"),
        ("fn f() = stream((s) => 1, { status: 99 })", "status must be"),
    ] {
        it.load_source(&format!("{}\n", src), "case.qwe").unwrap();
        let f = quillo::value::lookup(&it.globals, "f").unwrap();
        match it.call(&f, vec![], 0) {
            Err(quillo::interp::Ctrl::Error(e)) => assert!(e.msg.contains(needle), "{}: {}", src, e.msg),
            _ => panic!("{} did not fail", src),
        }
    }
    let a = quillo::check::check_source("route GET / = stream((send) => send(\"x\"))\nroute GET /e = events((send) => send({ a: 1 }, \"tick\"))\nroute GET /h = http.stream(\"http://x\", (p) => print(p))\n", "main.qwe", Path::new("."));
    assert!(a.errors().next().is_none(), "{:?}", a.errors().map(|e| e.message.clone()).collect::<Vec<_>>());
    let a = quillo::check::check_source("component C() = <p>{str(stream((s) => 1))}</p>\nroute GET / = <C />\n", "main.qwe", Path::new("."));
    assert!(a.errors().any(|e| e.message.contains("not available in the browser")));
}

// ---- in the server --------------------------------------------------------------------------

struct Server(Child, std::path::PathBuf);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        // A test that failed shows what its server said.
        if std::thread::panicking() {
            eprintln!("--- server log ---\n{}", std::fs::read_to_string(&self.1).unwrap_or_default());
        }
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// Starts the program on a free port. Tests run side by side, so another
/// one's server may take the port first: then this one's cannot listen,
/// and is started again on another (connecting would only reach theirs).
fn start(d: &Path) -> (Server, u16) {
    let log = d.join("server.log");
    for _ in 0..5 {
        let port = free_port();
        let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(d.join("app.qwe"))
            .args(["--port", &port.to_string()])
            .stderr(std::fs::File::create(&log).unwrap())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        support::stop_with_tests(&child);
        let mut server = Server(child, log.clone());
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if std::fs::read_to_string(&log).unwrap_or_default().contains("listening on") {
                return (server, port);
            }
            if let Ok(Some(_)) = server.0.try_wait() {
                break;
            }
            assert!(Instant::now() < deadline, "server did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = TcpStream::connect(("127.0.0.1", port));
    }
    panic!("no free port for the server: {}", std::fs::read_to_string(&log).unwrap_or_default())
}

fn ask(port: u16, path: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "GET {} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n", path).unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap_or_else(|e| panic!("reading {}: {} ({:?}) after {:?}", path, e, e.kind(), String::from_utf8_lossy(&out)));
    let out = String::from_utf8_lossy(&out).into_owned();
    out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default()
}

/// A chunked body, its chunks put back together.
fn dechunk(mut b: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = b.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap();
        if n == 0 {
            break;
        }
        out.push_str(&rest[..n]);
        b = &rest[n + 2..];
    }
    out
}

#[test]
fn pieces_reach_the_client_as_they_are_sent() {
    let d = dir("live");
    let (_server, port) = start(&d);
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let asked = Instant::now();
    write!(s, "GET /count HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut seen = String::new();
    let mut buf = [0u8; 4096];
    // The head and the first line come at once, long before the last.
    while !seen.contains("line 0") {
        let n = s.read(&mut buf).unwrap();
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    let first = asked.elapsed();
    assert!(first < Duration::from_millis(250), "first line after {:?}", first);
    assert!(seen.contains("transfer-encoding: chunked"), "{}", seen);
    while !seen.ends_with("0\r\n\r\n") {
        let n = s.read(&mut buf).unwrap();
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    assert!(asked.elapsed() >= Duration::from_millis(900));
    let body = seen.split_once("\r\n\r\n").unwrap().1;
    assert_eq!(dechunk(body), "line 0\nline 1\nline 2\nline 3\n");
    // The connection carries the next request.
    write!(s, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    let mut rest = String::new();
    s.read_to_string(&mut rest).unwrap();
    assert!(rest.ends_with("\r\n\r\nok"), "{}", rest);
    // Other requests are answered while a stream runs.
    let busy = std::thread::spawn(move || ask(port, "/count"));
    std::thread::sleep(Duration::from_millis(100));
    let t = Instant::now();
    assert_eq!(ask(port, "/"), "ok");
    assert!(t.elapsed() < Duration::from_millis(200));
    busy.join().unwrap();
}

#[test]
fn a_client_that_leaves_ends_the_function() {
    let d = dir("gone");
    let (_server, port) = start(&d);
    {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "GET /forever HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        std::thread::sleep(Duration::from_millis(400));
    }
    std::thread::sleep(Duration::from_millis(400));
    let a: u32 = ask(port, "/sent").parse().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let b: u32 = ask(port, "/sent").parse().unwrap();
    assert!(a > 0 && a == b, "still sending after the client left: {} then {}", a, b);
    // It stopped at a `send`, not by running to its end.
    assert_eq!(ask(port, "/finished"), "0");
}

#[test]
fn a_client_that_does_not_read_holds_up_nobody() {
    let d = dir("slow");
    let (_server, port) = start(&d);
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "GET /big HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    // 20 MB that are not read: the server waits on the socket, elsewhere.
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    assert_eq!(ask(port, "/"), "ok");
    assert!(t.elapsed() < Duration::from_millis(300), "{:?}", t.elapsed());
    let mut all = Vec::new();
    s.read_to_end(&mut all).unwrap();
    let all = String::from_utf8_lossy(&all);
    let (head, body) = all.split_once("\r\n\r\n").unwrap();
    assert!(head.contains("content-type: text/csv") && head.contains("content-disposition: attachment"), "{}", head);
    let body = dechunk(body);
    assert_eq!(body.len(), 20000 * 1024);
    assert!(body.lines().all(|l| l.len() == 1023));
}

#[test]
fn http_stream_reads_another_server_s_stream_as_it_comes() {
    let d = dir("relay");
    let (_server, port) = start(&d);
    // Events relayed by `send`: names and values as they were.
    assert_eq!(dechunk(&ask(port, &format!("/relay/{}", port))), dechunk(&ask(port, "/progress")));
    // Lines of JSON as values; text as it comes.
    assert_eq!(ask(port, &format!("/collect/{}", port)), r#"{"status":200,"got":[{"i":0},{"i":1},{"i":2}]}"#);
    assert_eq!(ask(port, &format!("/text/{}", port)), "line 0\nline 1\nline 2\nline 3\n");
    // `false` stops reading, and the stream it read from ends too.
    let t = Instant::now();
    assert_eq!(ask(port, &format!("/first/{}", port)), "[0, 1, 2]".replace(", ", ","));
    assert!(t.elapsed() < Duration::from_secs(2));
}

#[test]
fn the_browser_receives_as_it_comes() {
    if !Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
        return;
    }
    let d = dir("browser");
    let (_server, port) = start(&d);
    let src = format!(
        "fn __t() {{\n let got = []\n let r = receive(\"http://127.0.0.1:{p}/progress\", (data, name) => {{ got = got + [[data, name]] }})\n let lines = []\n receive(\"http://127.0.0.1:{p}/lines\", (x) => {{ lines = lines + [x] }})\n let text = \"\"\n receive(\"http://127.0.0.1:{p}/count\", (t) => {{ text = text + t }})\n let gone = receive(\"http://127.0.0.1:{p}/nowhere\", (x) => x)\n [got, lines, text, r, is_err(gone)]\n}}\n",
        p = port
    );
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap();
    let js = quillo::client::test_script(&it, "__t").unwrap();
    let js = js.replace("console.log(W.repr(G[\"__t\"]()))", "G[\"__t\"]().then((v) => console.log(W.repr(v)), (e) => console.log(\"ERROR: \" + e.message))");
    let out = Command::new("node").arg("-e").arg(&js).output().unwrap();
    let got = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(
        got,
        r#"[[[{ done: 1, of: 3 }, "progress"], [{ done: 2, of: 3 }, "progress"], [{ done: 3, of: 3 }, "progress"], ["finished", null]], [{ i: 0 }, { i: 1 }, { i: 2 }], "line 0\nline 1\nline 2\nline 3\n", null, true]"#,
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
