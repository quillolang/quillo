//! Runs the `quillo` binary as a server and talks to it over HTTP.

mod support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn port() -> u16 {
    support::free_port()
}

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-test-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(file: &Path, extra: &[&str]) -> Server {
    start_with_env(file, extra, &[])
}

fn start_with_env(file: &Path, extra: &[&str], env: &[(&str, &str)]) -> Server {
    let port = port();
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
    let server = Server { child, port };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    server
}

fn request(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(
        s,
        "{} {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        method,
        path,
        body.len(),
        body
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let body = out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
    (status, body)
}

fn get(port: u16, path: &str) -> (u16, String) {
    request(port, "GET", path, "")
}

#[test]
fn requests_waiting_on_io_run_concurrently() {
    let d = temp_dir("concurrent");
    let f = d.join("app.qwe");
    std::fs::write(
        &f,
        "let hits = 0\nroute GET /slow { sleep(400); hits += 1; { hits } }\nroute GET /count = hits\n",
    )
    .unwrap();
    let srv = start(&f, &["--workers", "4"]);
    let port = srv.port;
    let t0 = Instant::now();
    let threads: Vec<_> = (0..4).map(|_| std::thread::spawn(move || get(port, "/slow"))).collect();
    for t in threads {
        assert_eq!(t.join().unwrap().0, 200);
    }
    let took = t0.elapsed();
    assert!(took < Duration::from_millis(1200), "4 sleeping requests took {:?}", took);
    // Handlers run one at a time between waits, so `hits += 1` never loses an update.
    assert_eq!(get(srv.port, "/count").1, "4");
}

#[test]
fn parallel_calls_wait_at_the_same_time() {
    let d = temp_dir("parallel");
    let f = d.join("app.qwe");
    std::fs::write(
        &f,
        "let n = 0
fn slow(x) { sleep(400); n += 1; x }
         route GET /both = parallel(() => slow(1), () => slow(2), () => slow(3))
         route GET /n = n
",
    )
    .unwrap();
    let srv = start(&f, &[]);
    let t0 = Instant::now();
    let (status, body) = get(srv.port, "/both");
    let took = t0.elapsed();
    assert_eq!((status, body.as_str()), (200, "[1,2,3]"));
    assert!(took < Duration::from_millis(900), "three 400 ms waits took {:?}", took);
    // Each runs alone between waits, so `n += 1` never loses an update.
    assert_eq!(get(srv.port, "/n").1, "3");
}

#[test]
fn parallel_takes_a_list_and_nests() {
    let d = temp_dir("parallel-list");
    let f = d.join("app.qwe");
    std::fs::write(
        &f,
        "fn slow(x) { sleep(300); x * 10 }\n\
         route GET /list = parallel(range(0, 8) | map((i) => () => slow(i)))\n\
         route GET /nested = parallel(() => parallel(() => slow(1), () => slow(2)), () => slow(3))\n",
    )
    .unwrap();
    let srv = start(&f, &[]);
    let t0 = Instant::now();
    assert_eq!(get(srv.port, "/list").1, "[0,10,20,30,40,50,60,70]");
    assert_eq!(get(srv.port, "/nested").1, "[[10,20],30]");
    // Eight waits of 300 ms at once, then two levels of them: about 600 ms,
    // where one after another would take 3.3 s. The bound leaves room for a
    // loaded machine (a CI runner took 1.2 s).
    assert!(t0.elapsed() < Duration::from_millis(2000), "took {:?}", t0.elapsed());
}

#[test]
fn instances_take_connections_in_turn_and_share_the_database() {
    let d = temp_dir("instances");
    let f = d.join("app.qwe");
    std::fs::write(
        &f,
        format!(
            "db.open(\"{}\")\ndb.exec(\"create table if not exists notes (t text)\")\nlet hits = 0\n\
             route GET /hits {{ hits += 1; hits }}\n\
             route POST /note {{ db.exec(\"insert into notes values (?)\", request.body); \"ok\" }}\n\
             route GET /notes = db.one(\"select count(*) as n from notes\").n\n\
             route POST /ping {{ publish(\"ping\"); \"ok\" }}\n",
            d.join("shared.db").display()
        ),
    )
    .unwrap();
    let srv = start(&f, &["--instances", "3"]);
    // Each request is a new connection; they go to the instances in turn,
    // and each instance has a counter of its own.
    let hits: Vec<String> = (0..6).map(|_| get(srv.port, "/hits").1).collect();
    assert_eq!(hits, ["1", "1", "1", "2", "2", "2"]);
    // The database is one, whichever instance writes or reads.
    for _ in 0..3 {
        assert_eq!(request(srv.port, "POST", "/note", "x").1, "ok");
    }
    let counts: Vec<String> = (0..3).map(|_| get(srv.port, "/notes").1).collect();
    assert_eq!(counts, ["3", "3", "3"]);
    // A page waiting on one instance hears about a change announced by
    // another: the wait goes to the next instance, the ping to the one after.
    let port = srv.port;
    let waiting = std::thread::spawn(move || {
        let t0 = Instant::now();
        (get(port, "/_quillo/live?since=1&t=ping").1, t0.elapsed())
    });
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(request(srv.port, "POST", "/ping", "").1, "ok");
    let (woke, after) = waiting.join().unwrap();
    assert!(woke.contains("ping") && after >= Duration::from_millis(350), "{} after {:?}", woke, after);
}

#[test]
fn watch_reloads_changed_files_and_keeps_the_last_good_version() {
    let d = temp_dir("watch");
    let main = d.join("app.qwe");
    let lib = d.join("lib.qwe");
    std::fs::write(&lib, "fn greet() = \"one\"\n").unwrap();
    std::fs::write(&main, "use \"lib.qwe\"\nroute GET / = greet()\n").unwrap();
    let srv = start(&main, &["--watch"]);
    assert_eq!(get(srv.port, "/").1, "one");
    let v1 = get(srv.port, "/_quillo/dev").1;

    let wait_for = |want: &str| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let got = get(srv.port, "/").1;
            if got == want {
                return;
            }
            assert!(Instant::now() < deadline, "expected {:?}, still getting {:?}", want, got);
            std::thread::sleep(Duration::from_millis(50));
        }
    };

    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(&lib, "fn greet() = \"two\"\n").unwrap();
    wait_for("two");
    assert_ne!(get(srv.port, "/_quillo/dev").1, v1);

    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(&main, "route GET / = (\n").unwrap();
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(get(srv.port, "/").1, "two");

    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(&main, "route GET / = \"three\"\n").unwrap();
    wait_for("three");
}

#[test]
fn watch_injects_the_reload_script_into_pages() {
    let d = temp_dir("inject");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET / = <html><body><p>hi</p></body></html>\nroute GET /t = \"text\"\n").unwrap();
    let srv = start(&f, &["--watch"]);
    let page = get(srv.port, "/").1;
    // The page knows the version it was rendered from, so a change made
    // before its first request is not missed.
    assert!(page.contains("const v='1'") && page.contains("/_quillo/events?v=") && page.ends_with("</body></html>"), "{}", page);
    assert_eq!(get(srv.port, "/t").1, "text");
}

#[test]
fn http_client_talks_to_another_server() {
    let d = temp_dir("http-client");
    let api = d.join("api.qwe");
    std::fs::write(
        &api,
        "route GET /users/:id(int) = { id, name: `user ${id}` }\n\
         route POST /echo { json: { n: int } } = reply 201 { doubled: n * 2, agent: request.headers[\"user-agent\"] }\n\
         route GET /teapot = reply 418 \"short and stout\"\n",
    )
    .unwrap();
    let srv = start(&api, &[]);
    let client = d.join("client.qwe");
    std::fs::write(
        &client,
        format!(
            "let base = \"http://127.0.0.1:{}\"\n\
             fn load(id: int) with http = http.get(`${{base}}/users/${{id}}`).json\n\
             let r = http.post(`${{base}}/echo`, {{ n: 21 }})\n\
             let t = http.get(`${{base}}/teapot`)\n\
             let down = http.get(\"http://127.0.0.1:1/nothing\")\n\
             print(json.stringify([load(7), r.status, r.json.doubled, starts_with(r.json.agent, \"quillo/\"), t.status, t.ok, t.body, is_err(down)]))\n",
            srv.port
        ),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(&client).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        r#"[{"id":7,"name":"user 7"},201,42,true,418,false,"short and stout",true]"#
    );
}

#[test]
fn watch_tells_waiting_pages_about_changes() {
    let d = temp_dir("events");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET / = \"one\"\n").unwrap();
    let srv = start(&f, &["--watch", "--workers", "1"]);
    assert_eq!(get(srv.port, "/_quillo/events").1, "1");
    // A page waits for the version to change from 1...
    let port = srv.port;
    let waiting = std::thread::spawn(move || {
        let t0 = Instant::now();
        (get(port, "/_quillo/events?v=1").1, t0.elapsed())
    });
    std::thread::sleep(Duration::from_millis(300));
    // ...without using up the only worker.
    assert_eq!(get(srv.port, "/").1, "one");
    std::thread::sleep(Duration::from_millis(900));
    std::fs::write(&f, "route GET / = \"two\"\n").unwrap();
    let (v, took) = waiting.join().unwrap();
    assert_eq!(v, "2");
    assert!(took > Duration::from_millis(1000) && took < Duration::from_secs(5), "{:?}", took);
    assert_eq!(get(srv.port, "/").1, "two");
}

#[test]
fn text_responses_are_gzipped_when_the_client_accepts_it() {
    let d = temp_dir("gzip");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET /big = repeat(\"quillo \", 1000)\nroute GET /small = \"tiny\"\n").unwrap();
    let srv = start(&f, &[]);
    let raw = |path: &str, gzip: bool| {
        let mut s = TcpStream::connect(("127.0.0.1", srv.port)).unwrap();
        let ae = if gzip { "Accept-Encoding: gzip, br\r\n" } else { "" };
        write!(s, "GET {} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n{}\r\n", path, ae).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        out
    };
    let head = |b: &[u8]| String::from_utf8_lossy(&b[..b.windows(4).position(|w| w == b"\r\n\r\n").unwrap()]).to_lowercase();
    let gz = raw("/big", true);
    assert!(head(&gz).contains("content-encoding: gzip"), "{}", head(&gz));
    assert!(gz.len() < 500, "{} bytes", gz.len());
    let plain = raw("/big", false);
    assert!(!head(&plain).contains("content-encoding") && plain.len() > 5000);
    assert!(!head(&raw("/small", true)).contains("content-encoding"));
}

#[test]
fn a_files_gzipped_copy_is_kept_and_ranges_are_not_gzipped() {
    let d = temp_dir("gzip-file");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET /a/*p = file(p)\n").unwrap();
    std::fs::write(d.join("big.css"), "body { color: red }\n".repeat(500)).unwrap();
    let srv = start(&f, &[]);
    let raw = |extra: &str| {
        let mut s = TcpStream::connect(("127.0.0.1", srv.port)).unwrap();
        write!(s, "GET /a/big.css HTTP/1.1\r\nHost: x\r\nConnection: close\r\nAccept-Encoding: gzip\r\n{}\r\n", extra).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        let at = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (String::from_utf8_lossy(&out[..at]).to_lowercase(), out[at + 4..].to_vec())
    };
    let (h1, b1) = raw("");
    let (_, b2) = raw("");
    assert!(h1.contains("content-encoding: gzip") && h1.contains("etag: w/"), "{}", h1);
    assert_eq!(b1, b2);
    let (h, b) = raw("Range: bytes=0-3\r\n");
    assert!(h.starts_with("http/1.1 206") && !h.contains("content-encoding"), "{}", h);
    assert_eq!(b, b"body");
}

#[test]
fn large_bodies_are_refused_before_they_are_read() {
    let d = temp_dir("max-body");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route POST /echo = len(request.body)\n").unwrap();
    let srv = start(&f, &["--max-body", "100"]);
    assert_eq!(request(srv.port, "POST", "/echo", &"x".repeat(100)), (200, "100".into()));
    assert_eq!(request(srv.port, "POST", "/echo", &"x".repeat(101)).0, 413);
    // The answer comes before the body is sent, and is not lost when the
    // body arrives after it (closing at once would reset the connection).
    let mut s = TcpStream::connect(("127.0.0.1", srv.port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100000\r\n\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let _ = s.write_all(&[b'x'; 100000]);
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 413"), "{}", out);
    // Still serving afterwards.
    assert_eq!(request(srv.port, "POST", "/echo", "abc"), (200, "3".into()));
}

#[test]
fn responses_tell_browsers_not_to_guess_their_type() {
    let d = temp_dir("nosniff");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET / = \"hi\"\n").unwrap();
    let srv = start(&f, &[]);
    let mut s = TcpStream::connect(("127.0.0.1", srv.port)).unwrap();
    write!(s, "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.to_ascii_lowercase().contains("x-content-type-options: nosniff"), "{}", out);
}

#[test]
fn a_sqlite_transaction_is_not_shared_with_other_requests() {
    // While /a waits inside its transaction, /b must not write into it:
    // /a rolls back, and /b's row has to survive.
    let d = temp_dir("sqlite-tx");
    let f = d.join("app.qwe");
    std::fs::write(
        &f,
        "db.open(\"t.db\")\ndb.exec(\"create table if not exists t (v text)\")\n\
         route POST /a = db.transaction(() => { db.exec(\"insert into t values ('a')\"); sleep(500); err(\"undo\") })\n\
         route POST /b { db.exec(\"insert into t values ('b')\"); \"ok\" }\n\
         route GET /rows = db.query(\"select v from t order by v\") | map((r) => r.v)\n",
    )
    .unwrap();
    let srv = start(&f, &["--workers", "4"]);
    let port = srv.port;
    let a = std::thread::spawn(move || request(port, "POST", "/a", ""));
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(request(port, "POST", "/b", "").0, 200);
    assert_eq!(a.join().unwrap().0, 500);
    assert_eq!(get(port, "/rows").1, r#"["b"]"#);
}

/// Sends `head` (request line and headers, without the blank line) and
/// returns the whole response.
fn raw(port: u16, head: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "{}\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n", head).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn a_desktop_apps_server_answers_only_its_window() {
    let d = temp_dir("launch");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET /hello = `hi`\n").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(quillo::server::WORKER_STACK)
        .spawn(move || {
            let mut it = quillo::Interp::new(&d);
            let shared = it.shared.clone();
            it.load_file(&f).unwrap();
            let mut app = quillo::server::App::new(quillo::server::Loaded::Ready(it), shared, &f, &d, false);
            app.launch_token = Some("t0ken".into());
            quillo::server::serve_with(app, "127.0.0.1:0", 2, move |a| tx.send(a.port()).unwrap()).unwrap();
        })
        .unwrap();
    let port = rx.recv_timeout(Duration::from_secs(10)).expect("server did not start");

    // Other programs, and pages of other sites, get nothing.
    assert_eq!(get(port, "/hello").0, 403);
    assert_eq!(get(port, "/hello?_quillo_launch=t0kem").0, 403);
    assert_eq!(get(port, "/_quillo/live").0, 403);
    assert!(raw(port, "GET /hello HTTP/1.1\r\nCookie: quillo_launch=t0ke").starts_with("HTTP/1.1 403"));

    // The window's first request trades the token for a cookie, and is sent
    // on to the same page without it.
    let first = raw(port, "GET /hello?a=1&_quillo_launch=t0ken&b=2 HTTP/1.1").to_ascii_lowercase();
    assert!(first.starts_with("http/1.1 303"), "{}", first);
    assert!(first.contains("location: /hello?a=1&b=2\r\n"), "{}", first);
    assert!(first.contains("set-cookie: quillo_launch=t0ken; path=/; httponly; samesite=strict"), "{}", first);

    let page = raw(port, "GET /hello HTTP/1.1\r\nCookie: theme=dark; quillo_launch=t0ken");
    assert!(page.starts_with("HTTP/1.1 200"), "{}", page);
    assert!(page.ends_with("hi"), "{}", page);
}

#[test]
fn health_says_whether_to_send_requests_here() {
    let d = temp_dir("health");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET / = \"hi\"\n").unwrap();
    let srv = start(&f, &[]);
    assert_eq!(get(srv.port, "/_quillo/health"), (200, "ok\n".into()));
}

/// On SIGTERM, a request in progress gets its answer, no new connections
/// are taken, and the server exits cleanly.
#[cfg(unix)]
#[test]
fn stopping_finishes_requests_in_progress() {
    let d = temp_dir("stop");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET /slow { sleep(600); \"done\" }\n").unwrap();
    let mut srv = start(&f, &[]);
    let port = srv.port;
    let slow = std::thread::spawn(move || get(port, "/slow"));
    std::thread::sleep(Duration::from_millis(150));
    let ok = Command::new("kill").args(["-TERM", &srv.child.id().to_string()]).status().unwrap();
    assert!(ok.success());
    assert_eq!(slow.join().unwrap(), (200, "done".into()));
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(s) = srv.child.try_wait().unwrap() {
            break s;
        }
        assert!(Instant::now() < deadline, "the server did not stop");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "{:?}", status);
    assert!(!support::listening(port));
}

#[test]
fn metrics_count_answers_when_asked_for() {
    let d = temp_dir("metrics");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET / = \"hi\"\nroute GET /missing = respond(404, \"no\")\n").unwrap();
    let srv = start(&f, &["--metrics"]);
    for path in ["/", "/", "/missing"] {
        get(srv.port, path);
    }
    let (status, text) = get(srv.port, "/_quillo/metrics");
    assert_eq!(status, 200);
    assert!(text.contains("quillo_requests_total{status=\"2xx\"} 2\n"), "{}", text);
    assert!(text.contains("quillo_requests_total{status=\"4xx\"} 1\n"), "{}", text);
    assert!(text.contains("quillo_request_duration_seconds_count 3\n"), "{}", text);
    assert!(text.contains("quillo_requests_in_progress 0\n") && text.contains("quillo_connections "), "{}", text);
    // Off unless asked for.
    let plain = start(&f, &[]);
    assert_eq!(get(plain.port, "/_quillo/metrics").0, 404);
}

#[test]
fn the_log_can_be_json_lines() {
    let d = temp_dir("jsonlog");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET /hello = \"hi\"\n").unwrap();
    let port = port();
    // The log to a file, read once the server has stopped.
    let log_file = d.join("server.log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(&f)
        .args(["--port", &port.to_string(), "--log", "json"])
        .stderr(std::fs::File::create(&log_file).unwrap())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let server = Server { child, port };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(get(port, "/hello?x=1").1, "hi");
    std::thread::sleep(Duration::from_millis(300));
    drop(server);
    let log = std::fs::read_to_string(&log_file).unwrap_or_default();
    let line = log.lines().find(|l| l.contains("/hello")).unwrap_or_else(|| panic!("no log line in {}", log));
    let v: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("{}: {}", e, line));
    // The path without its query string, unless --log-query.
    assert_eq!((v["method"].as_str(), v["path"].as_str(), v["status"].as_u64()), (Some("GET"), Some("/hello"), Some(200)));
    assert!(v["time"].as_str().map_or(false, |t| t.ends_with('Z') && t.len() == 24), "{}", line);
    assert!(v["ms"].is_number() && v["ip"].is_string(), "{}", line);
}

#[test]
fn the_log_goes_to_a_file_a_day_with_the_user_and_old_days_go() {
    let d = temp_dir("logfile");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET /in = { set_session({ user: 42 }); \"in\" }\nroute GET /pay = \"paid\"\n").unwrap();
    std::fs::create_dir_all(d.join("logs")).unwrap();
    // Days long past, and a file of someone else's.
    std::fs::write(d.join("logs/requests-2001-01-01.log"), "old\n").unwrap();
    std::fs::write(d.join("logs/requests-notes.log"), "mine\n").unwrap();
    let log = d.join("logs/requests.log");
    let srv = start(&f, &["--log", "json", "--log-file", log.to_str().unwrap(), "--log-keep", "30", "--log-query"]);
    assert_eq!(get(srv.port, "/in?x=1").1, "in");
    assert_eq!(get(srv.port, "/pay?card_number=4111111111111111&n=2").1, "paid");
    std::thread::sleep(Duration::from_millis(1500));
    let names: Vec<String> = std::fs::read_dir(d.join("logs")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert!(!names.contains(&"requests-2001-01-01.log".to_string()), "{:?}", names);
    assert!(names.contains(&"requests-notes.log".to_string()), "{:?}", names);
    let today = names.iter().find(|n| n.starts_with("requests-20") && n != &"requests-2001-01-01.log").unwrap_or_else(|| panic!("{:?}", names));
    let text = std::fs::read_to_string(d.join("logs").join(today)).unwrap();
    let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{}: {}", e, l))).collect();
    assert_eq!(lines.len(), 2, "{}", text);
    assert_eq!((lines[0]["path"].as_str(), lines[0]["user"].as_str()), (Some("/in?x=1"), Some("42")), "{}", text);
    assert!(lines[0]["rid"].is_string() && lines[0]["ip"].is_string(), "{}", text);
    assert_eq!(lines[1]["path"].as_str(), Some("/pay?card_number=[redacted]&n=2"), "{}", text);
    assert!(lines[1].get("user").is_none(), "{}", text);
}

#[test]
fn scheduled_jobs_run_on_time_and_once_across_servers() {
    let d = temp_dir("jobs");
    let f = d.join("app.qwe");
    std::fs::write(
        &f,
        r#"db.open("jobs.db")
db.exec("create table if not exists runs (server text, sec integer)")
let me = env("WHO", "?")
let ticks = 0
fn tick() {
    ticks += 1
    db.exec("insert into runs values (?, ?)", me, floor(now() / 1000))
}
schedule("1s", tick)
schedule("0 0 1 1 *", () => print("new year"), { name: "yearly" })
route GET /ticks = ticks
route GET /runs = db.query("select server, sec from runs order by sec")
"#,
    )
    .unwrap();
    let a = start_with_env(&f, &[], &[("WHO", "a")]);
    let b = start_with_env(&f, &[], &[("WHO", "b")]);
    std::thread::sleep(Duration::from_millis(4200));
    let runs: serde_json::Value = serde_json::from_str(&get(a.port, "/runs").1).unwrap();
    let secs: Vec<i64> = runs.as_array().unwrap().iter().map(|r| r["sec"].as_i64().unwrap()).collect();
    assert!(secs.len() >= 3, "{}", runs);
    let mut distinct = secs.clone();
    distinct.dedup();
    assert_eq!(secs, distinct, "a second ran twice: {}", runs);
    let (ta, tb) = (get(a.port, "/ticks").1.parse::<usize>().unwrap(), get(b.port, "/ticks").1.parse::<usize>().unwrap());
    assert_eq!(ta + tb, secs.len(), "{} {} {}", ta, tb, runs);
    // Only the scheduler can ask for a job.
    assert_eq!(request(a.port, "POST", "/_quillo/job/tick", "").0, 404);
    // Without jobs here, none run.
    let c = start(&f, &["--no-jobs"]);
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(get(c.port, "/ticks").1, "0");
}

#[test]
fn large_files_are_sent_from_disk() {
    let d = temp_dir("large-file");
    let f = d.join("app.qwe");
    std::fs::write(
        &f,
        "route GET /f/*p = file(p)\nroute GET /forged = respond(200, \"\", { \"x-quillo-send\": \"L2V0Yy9wYXNzd2Q.AAAA\" })\n",
    )
    .unwrap();
    let data: Vec<u8> = (0..5_000_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(d.join("clip.mp4"), &data).unwrap();
    let srv = start(&f, &[]);
    let raw = |method: &str, path: &str, extra: &str| {
        let mut s = TcpStream::connect(("127.0.0.1", srv.port)).unwrap();
        write!(s, "{} {} HTTP/1.1\r\nHost: x\r\nConnection: close\r\nAccept-Encoding: gzip\r\n{}\r\n", method, path, extra).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        let at = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (String::from_utf8_lossy(&out[..at]).to_lowercase(), out[at + 4..].to_vec())
    };
    let (h, b) = raw("GET", "/f/clip.mp4", "");
    assert!(h.starts_with("http/1.1 200") && h.contains("content-length: 5000000") && h.contains("etag: w/"), "{}", h);
    assert!(!h.contains("x-quillo-send") && !h.contains("content-encoding"), "{}", h);
    assert!(b == data, "{} bytes", b.len());
    let (h, b) = raw("GET", "/f/clip.mp4", "Range: bytes=2000000-2000099\r\n");
    assert!(h.starts_with("http/1.1 206") && h.contains("content-range: bytes 2000000-2000099/5000000"), "{}", h);
    assert_eq!(b, &data[2_000_000..2_000_100]);
    let (h, b) = raw("HEAD", "/f/clip.mp4", "");
    assert!(h.contains("content-length: 5000000") && b.is_empty(), "{}", h);
    // Only file() can have a file sent.
    assert_eq!(get(srv.port, "/forged").0, 500);
}

/// The server's resident memory, in KiB.
#[cfg(unix)]
fn rss_kib(srv: &Server) -> u64 {
    let out = Command::new("ps").args(["-o", "rss=", "-p", &srv.child.id().to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0)
}

#[cfg(unix)]
#[test]
fn a_client_that_sends_and_never_reads_holds_no_one_up() {
    let d = temp_dir("no-reader");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET /f/*p = file(p)\nroute GET /json = { ok: true }\n").unwrap();
    std::fs::write(d.join("small.bin"), vec![7u8; 900_000]).unwrap();
    std::fs::write(d.join("big.bin"), vec![9u8; 3_000_000]).unwrap();
    let srv = start(&f, &[]);
    let before = rss_kib(&srv);
    // Hundreds of requests for a file kept in memory, then a large one sent
    // from disk, on one connection whose answers are never read.
    let mut greedy = TcpStream::connect(("127.0.0.1", srv.port)).unwrap();
    let mut batch = String::new();
    for _ in 0..400 {
        batch.push_str("GET /f/small.bin HTTP/1.1\r\nHost: x\r\n\r\n");
    }
    batch.push_str("GET /f/big.bin HTTP/1.1\r\nHost: x\r\n\r\n");
    greedy.write_all(batch.as_bytes()).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    // Everyone else is answered at once.
    let t0 = Instant::now();
    for _ in 0..5 {
        assert_eq!(get(srv.port, "/json"), (200, r#"{"ok":true}"#.into()));
    }
    assert!(t0.elapsed() < Duration::from_secs(2), "took {:?}", t0.elapsed());
    // Its answers do not pile up: 400 of them would be 360 MB.
    let grew = rss_kib(&srv).saturating_sub(before);
    assert!(grew < 64 * 1024, "memory grew by {} KiB", grew);
    // And once it reads, it gets every answer, in order.
    greedy.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let mut got = 0usize;
    let mut buf = vec![0u8; 1 << 20];
    let want = 400 * 900_000 + 3_000_000;
    while got < want {
        match greedy.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => got += n,
        }
    }
    assert!(got > want, "read {} of at least {} bytes", got, want);
}

#[test]
fn passwords_are_hashed_off_the_event_loop() {
    let d = temp_dir("argon");
    let f = d.join("app.qwe");
    std::fs::write(
        &f,
        "route GET /check = crypto.verify_password(\"pw\", crypto.hash_password(\"pw\")) && !crypto.verify_password(\"no\", crypto.hash_password(\"pw\"))\n\
         route GET /json = { ok: true }\n",
    )
    .unwrap();
    let srv = start(&f, &[]);
    let port = srv.port;
    let logins: Vec<_> = (0..16).map(|_| std::thread::spawn(move || get(port, "/check"))).collect();
    // Meanwhile other requests are answered.
    let t0 = Instant::now();
    assert_eq!(get(port, "/json").0, 200);
    assert!(t0.elapsed() < Duration::from_millis(500), "took {:?}", t0.elapsed());
    for l in logins {
        assert_eq!(l.join().unwrap(), (200, "true".into()));
    }
}

/// `/_quillo/live` over a WebSocket: the browser lets any site open one, so
/// the server refuses pages of other sites, as it does for server fns. A
/// page of the site, and a program (no Origin), connect.
#[test]
fn live_websockets_are_only_for_the_sites_own_pages() {
    let d = temp_dir("live-origin");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET / = `hi`\n").unwrap();
    let srv = start(&f, &[]);
    // The status line of a WebSocket handshake with these headers.
    let open = |extra: &str| {
        let mut s = TcpStream::connect(("127.0.0.1", srv.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        write!(
            s,
            "GET /_quillo/live?t=db%3Anotes HTTP/1.1\r\nHost: localhost:{}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n{}\r\n",
            srv.port, extra
        )
        .unwrap();
        let mut line = Vec::new();
        let mut b = [0u8; 1];
        while !line.ends_with(b"\r\n") && s.read(&mut b).map_or(false, |n| n == 1) {
            line.push(b[0]);
        }
        String::from_utf8_lossy(&line).trim().to_string()
    };
    let other = "Origin: https://evil.example\r\n";
    assert!(open(other).starts_with("HTTP/1.1 403"), "{}", open(other));
    assert!(open("Origin: http://localhost:1\r\n").starts_with("HTTP/1.1 403"));
    assert!(open("Sec-Fetch-Site: cross-site\r\n").starts_with("HTTP/1.1 403"));
    assert!(open("Sec-Fetch-Site: same-site\r\n").starts_with("HTTP/1.1 403"));
    let own = format!("Origin: http://localhost:{}\r\n", srv.port);
    assert_eq!(open(&own), "HTTP/1.1 101 Switching Protocols");
    assert_eq!(open("Sec-Fetch-Site: same-origin\r\n"), "HTTP/1.1 101 Switching Protocols");
    assert_eq!(open(""), "HTTP/1.1 101 Switching Protocols");
    // The long wait without a WebSocket too.
    assert!(raw(srv.port, "GET /_quillo/live?since=1&t=x HTTP/1.1\r\nOrigin: https://evil.example").starts_with("HTTP/1.1 403"));
}

/// A desktop window's launch token is a secret of the machine's: whatever
/// the log's form, with query strings or not, and for a request too broken
/// to answer, the log never shows it.
#[test]
fn the_log_never_shows_a_launch_token() {
    let d = temp_dir("launchlog");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route GET /hello = \"hi\"\n").unwrap();
    let token = "5ec12e7c0ffee5ec12e7c0ffee5ec12e7";
    for flags in [&["--log", "text"][..], &["--log", "json", "--log-query"][..], &["--log", "json"][..]] {
        let port = port();
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(&f)
            .args(["--port", &port.to_string()])
            .args(flags)
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !support::listening(port) {
            assert!(Instant::now() < deadline, "server did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(get(port, &format!("/hello?a=1&_quillo_launch={}", token)).1, "hi");
        // A header line without a colon: refused before it is a request.
        let bad = raw(port, &format!("GET /hello?_quillo_launch={} HTTP/1.1\r\nno colon here", token));
        assert!(bad.starts_with("HTTP/1.1 400"), "{}", bad);
        std::thread::sleep(Duration::from_millis(300));
        let _ = child.kill();
        let out = child.wait_with_output().unwrap();
        let log = String::from_utf8_lossy(&out.stderr);
        assert!(log.contains("/hello") && log.contains("-> 400"), "{:?}: {}", flags, log);
        assert!(!log.contains(token) && !log.contains(&token[..12]), "{:?}: {}", flags, log);
    }
}
