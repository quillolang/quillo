//! Runs the `quillo` binary as a server and talks to it over HTTP.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

static NEXT_PORT: AtomicU16 = AtomicU16::new(0);

fn port() -> u16 {
    let base = 20000 + (std::process::id() % 2000) as u16 * 10;
    base + NEXT_PORT.fetch_add(1, Ordering::SeqCst)
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
    let port = port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(extra)
        .stderr(Stdio::null())
        .spawn()
        .expect("cannot start quillo");
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    Server { child, port }
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
fn large_bodies_are_refused_before_they_are_read() {
    let d = temp_dir("max-body");
    let f = d.join("app.qwe");
    std::fs::write(&f, "route POST /echo = len(request.body)\n").unwrap();
    let srv = start(&f, &["--max-body", "100"]);
    assert_eq!(request(srv.port, "POST", "/echo", &"x".repeat(100)), (200, "100".into()));
    assert_eq!(request(srv.port, "POST", "/echo", &"x".repeat(101)).0, 413);
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
