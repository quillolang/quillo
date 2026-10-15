//! Runs the `wisp` binary as a server and talks to it over HTTP.

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
    let d = std::env::temp_dir().join(format!("wisp-test-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(file: &Path, extra: &[&str]) -> Server {
    let port = port();
    let child = Command::new(env!("CARGO_BIN_EXE_wisp"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(extra)
        .stderr(Stdio::null())
        .spawn()
        .expect("cannot start wisp");
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
    let f = d.join("app.wisp");
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
fn watch_reloads_changed_files_and_keeps_the_last_good_version() {
    let d = temp_dir("watch");
    let main = d.join("app.wisp");
    let lib = d.join("lib.wisp");
    std::fs::write(&lib, "fn greet() = \"one\"\n").unwrap();
    std::fs::write(&main, "use \"lib.wisp\"\nroute GET / = greet()\n").unwrap();
    let srv = start(&main, &["--watch"]);
    assert_eq!(get(srv.port, "/").1, "one");
    let v1 = get(srv.port, "/_wisp/dev").1;

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
    assert_ne!(get(srv.port, "/_wisp/dev").1, v1);

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
    let f = d.join("app.wisp");
    std::fs::write(&f, "route GET / = <html><body><p>\"hi\"</p></body></html>\nroute GET /t = \"text\"\n").unwrap();
    let srv = start(&f, &["--watch"]);
    let page = get(srv.port, "/").1;
    assert!(page.contains("/_wisp/events") && page.ends_with("</body></html>"), "{}", page);
    assert_eq!(get(srv.port, "/t").1, "text");
}

#[test]
fn http_client_talks_to_another_server() {
    let d = temp_dir("http-client");
    let api = d.join("api.wisp");
    std::fs::write(
        &api,
        "route GET /users/:id(int) = { id, name: \"user {id}\" }\n\
         route POST /echo { json: { n: int } } = reply 201 { doubled: n * 2, agent: request.headers[\"user-agent\"] }\n\
         route GET /teapot = reply 418 \"short and stout\"\n",
    )
    .unwrap();
    let srv = start(&api, &[]);
    let client = d.join("client.wisp");
    std::fs::write(
        &client,
        format!(
            "let base = \"http://127.0.0.1:{}\"\n\
             fn load(id: int) with http = http.get(\"{{base}}/users/{{id}}\").json\n\
             let r = http.post(\"{{base}}/echo\", {{ n: 21 }})\n\
             let t = http.get(\"{{base}}/teapot\")\n\
             let down = http.get(\"http://127.0.0.1:1/nothing\")\n\
             print(json.stringify([load(7), r.status, r.json.doubled, starts_with(r.json.agent, \"wisp/\"), t.status, t.ok, t.body, is_err(down)]))\n",
            srv.port
        ),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_wisp")).arg("run").arg(&client).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        r#"[{"id":7,"name":"user 7"},201,42,true,418,false,"short and stout",true]"#
    );
}

#[test]
fn watch_tells_waiting_pages_about_changes() {
    let d = temp_dir("events");
    let f = d.join("app.wisp");
    std::fs::write(&f, "route GET / = \"one\"\n").unwrap();
    let srv = start(&f, &["--watch", "--workers", "1"]);
    assert_eq!(get(srv.port, "/_wisp/events").1, "1");
    // A page waits for the version to change from 1...
    let port = srv.port;
    let waiting = std::thread::spawn(move || {
        let t0 = Instant::now();
        (get(port, "/_wisp/events?v=1").1, t0.elapsed())
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
