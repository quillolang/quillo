//! A running server's longer conversations: pages whose parts follow them,
//! pages waiting for changes, large files, `--watch` and the request log.

mod support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use support::send;

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
        send(self.port, "GET", path, &[], "")
    }

    fn write(&self, file: &str, text: &str) {
        std::fs::write(self.dir.join(file), text).unwrap();
    }

    fn connections(&self) -> i64 {
        self.get("/_quillo/metrics").body.lines().find_map(|l| l.strip_prefix("quillo_connections ")).and_then(|n| n.parse().ok()).unwrap_or(-1)
    }
}

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
    let d = support::temp_dir(&format!("running-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

fn run_in(d: &Path, main: &str, args: &[&str], env: &[(&str, &str)]) -> Srv {
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join(main))
        .args(["--port", &port.to_string()])
        .args(args)
        .envs(env.iter().copied())
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

fn run(name: &str, src: &str, args: &[&str]) -> Srv {
    let d = folder(name, &[("app.qwe", src)]);
    run_in(&d, "app.qwe", args, &[])
}

fn connect(port: u16) -> TcpStream {
    let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(40))).unwrap();
    s
}

fn ask(port: u16, path: &str) -> TcpStream {
    let mut s = connect(port);
    write!(s, "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", path).unwrap();
    s
}

fn all(s: &mut TcpStream) -> String {
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

/// Reads until `text` has come; what came.
fn read_to(s: &mut TcpStream, text: &str) -> String {
    let mut out = Vec::new();
    let mut b = [0u8; 4096];
    while !String::from_utf8_lossy(&out).contains(text) {
        match s.read(&mut b) {
            Ok(0) | Err(_) => break,
            Ok(n) => out.extend_from_slice(&b[..n]),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(unix)]
fn reset(s: TcpStream) {
    use std::os::fd::AsRawFd;
    let l = libc::linger { l_onoff: 1, l_linger: 0 };
    // SAFETY: an open socket, and a value of the size given.
    let r = unsafe { libc::setsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_LINGER, &l as *const _ as *const libc::c_void, std::mem::size_of::<libc::linger>() as u32) };
    assert_eq!(r, 0);
}

const DEFERRED: &str = r#"
server fn slow() { sleep(300); [1, 2] }
component Page() {
    state s = defer(slow())
    <div>{s == null ? <p id="wait">waiting</p> : <ul>{for v in s => <li>{v}</li>}</ul>}</div>
}
route GET / = <html><body><Page/><footer>end</footer></body></html>
"#;

#[test]
fn a_pages_waiting_parts_follow_it_in_the_same_answer() {
    let srv = run("defer", DEFERRED, &[]);
    let mut s = ask(srv.port, "/");
    let first = read_to(&mut s, "__quillo_swap");
    assert!(first.starts_with("HTTP/1.1 200") && first.contains("transfer-encoding: chunked"), "{}", first);
    assert!(first.contains("id=\"wait\"") && !first.contains("<li>"), "{}", first);
    let rest = all(&mut s);
    assert!(rest.contains("<li>1</li>") && rest.contains("__quillo_swap(\"0\")"), "{}", rest);
    assert!(rest.contains("</body></html>") && rest.ends_with("0\r\n\r\n"), "{}", rest);
}

#[test]
fn a_pages_waiting_parts_are_worked_out_for_head_as_nothing() {
    let srv = run("defer-head", DEFERRED, &[]);
    let a = send(srv.port, "HEAD", "/", &[], "");
    assert_eq!(a.status, 200);
    assert_eq!(a.body, "");
}

#[test]
fn a_page_with_no_end_has_nothing_after_its_parts() {
    let srv = run("defer-bare", &format!("{}route GET /bare = <Page/>\n", DEFERRED), &[]);
    let a = all(&mut ask(srv.port, "/bare"));
    assert!(a.contains("<li>1</li>") && a.ends_with("</script>\r\n0\r\n\r\n"), "{}", a);
}

#[test]
fn a_page_being_developed_keeps_its_reload_script_when_parts_follow_it() {
    let srv = run("defer-watch", DEFERRED, &["--watch"]);
    let a = srv.get("/").body;
    let script = a.find("/_quillo/events?v=").expect(&a);
    assert!(script < a.find("</body>").unwrap(), "{}", a);
}

#[test]
fn a_mounted_programs_waiting_parts_come_with_its_page() {
    let d = folder("defer-mount", &[("app.qwe", "mount /inner = \"inner.qwe\"\nroute GET / = \"outer\"\n"), ("inner.qwe", DEFERRED)]);
    let srv = run_in(&d, "app.qwe", &[], &[]);
    let a = srv.get("/inner");
    assert_eq!(a.status, 200);
    assert!(a.header("transfer-encoding").is_none() && a.header("content-length").is_some(), "{:?}", a.headers);
    assert!(a.body.contains("<li>1</li>"), "{}", a.body);
}

#[cfg(unix)]
#[test]
fn a_client_gone_while_a_part_is_worked_out_ends_the_page() {
    let srv = run("defer-gone", DEFERRED, &["--metrics"]);
    let mut s = ask(srv.port, "/");
    let _ = read_to(&mut s, "__quillo_swap");
    reset(s);
    assert!(until(|| srv.connections() == 1), "{}", srv.connections());
    assert_eq!(srv.get("/").status, 200);
}

#[cfg(unix)]
#[test]
fn a_client_gone_before_a_stream_ends_is_found_at_its_end() {
    let srv = run("stream-gone", "route GET / = stream((send) => { send(\"first\"); sleep(250) })\nroute GET /ok = \"ok\"\n", &["--metrics"]);
    let mut s = ask(srv.port, "/");
    let _ = read_to(&mut s, "first");
    reset(s);
    assert!(until(|| srv.connections() == 1), "{}", srv.connections());
    assert_eq!(srv.get("/ok").body, "ok");
}

#[test]
fn a_program_that_fails_in_a_stream_ends_the_connection() {
    let srv = run("stream-fails", "fn nothing() = 0\nroute GET / = stream((send) => { send(\"first\"); send(1 / nothing()) })\n", &[]);
    let a = all(&mut ask(srv.port, "/"));
    assert!(a.contains("first") && !a.ends_with("0\r\n\r\n"), "{}", a);
}

const WAITING: &str = "route POST /change { publish(\"t\"); \"ok\" }\nroute GET / = \"hi\"\n";

fn live_seq(srv: &Srv) -> u64 {
    let j: serde_json::Value = serde_json::from_str(&srv.get("/_quillo/live?since=0").body).unwrap();
    j["seq"].as_u64().unwrap()
}

#[test]
fn one_client_cannot_hold_every_waiting_place() {
    let srv = run("waiters", WAITING, &[]);
    assert_eq!(send(srv.port, "POST", "/change", &[], "").body, "ok");
    let seq = live_seq(&srv);
    assert!(seq > 0);
    let path = format!("/_quillo/live?since={}&t=t", seq);
    let held: Vec<TcpStream> = (0..256).map(|_| ask(srv.port, &path)).collect();
    // All 256 are in: the next is told to come back.
    let mut refused = None;
    assert!(until(|| {
        let mut s = ask(srv.port, &path);
        s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        let a = all(&mut s);
        let full = a.starts_with("HTTP/1.1 503");
        if full {
            refused = Some(a);
        }
        full
    }));
    let refused = refused.unwrap();
    assert!(refused.contains("retry-after: 5") && refused.contains("too many pages are waiting"), "{}", refused);
    // A change answers them all, and their places are free again.
    assert_eq!(send(srv.port, "POST", "/change", &[], "").body, "ok");
    for mut s in held {
        let a = all(&mut s);
        assert!(a.starts_with("HTTP/1.1 200") && a.contains("\"changed\":[\"t\"]"), "{}", a);
    }
    assert!(until(|| {
        let seq = live_seq(&srv);
        let mut s = ask(srv.port, &format!("/_quillo/live?since={}&t=t", seq));
        s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let mut b = [0u8; 12];
        !matches!(s.read_exact(&mut b), Ok(()) if b.starts_with(b"HTTP/1.1 503"))
    }));
}

#[test]
fn a_detached_answer_comes_after_those_before_it_on_the_connection() {
    let d = folder("pipelined", &[("app.qwe", "route GET /big = file(\"big.bin\")\nroute GET / = \"hi\"\n")]);
    std::fs::write(d.join("big.bin"), vec![b'x'; 2 << 20]).unwrap();
    let srv = run_in(&d, "app.qwe", &[], &[]);
    let mut s = connect(srv.port);
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\nGET /big HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let a = all(&mut s);
    assert!(a.starts_with("HTTP/1.1 200"), "{}", &a[..40]);
    let (first, rest) = a.split_once("\r\n\r\nhi").unwrap();
    assert!(first.contains("content-length: 2"), "{}", first);
    assert!(rest.starts_with("HTTP/1.1 200") && rest.ends_with("xxx"), "{}", &rest[..40]);
    assert_eq!(rest.matches('x').count() - rest.split("\r\n\r\n").next().unwrap().matches('x').count(), 2 << 20);
    // The same for a page's wait for changes.
    let mut s = connect(srv.port);
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\nGET /_quillo/live?since=0 HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let a = all(&mut s);
    assert!(a.matches("HTTP/1.1 200").count() == 2 && a.contains("hi") && a.contains("\"seq\":"), "{}", a);
    assert!(a.find("hi").unwrap() < a.find("\"seq\"").unwrap(), "{}", a);
}

#[test]
fn too_many_downloads_at_once_are_told_to_come_back() {
    let d = folder("downloads", &[("app.qwe", "route GET /big = file(\"big.bin\")\nroute GET / = \"hi\"\n")]);
    std::fs::File::create(d.join("big.bin")).unwrap().set_len(64 << 20).unwrap();
    let srv = run_in(&d, "app.qwe", &[], &[]);
    let mut held: Vec<TcpStream> = (0..8).map(|_| ask(srv.port, "/big")).collect();
    for s in held.iter_mut() {
        let head = read_to(s, "\r\n\r\n");
        assert!(head.starts_with("HTTP/1.1 200"), "{}", &head[..40.min(head.len())]);
    }
    let a = srv.get("/big");
    assert_eq!((a.status, a.header("retry-after")), (503, Some("5")));
    assert!(a.body.contains("too many downloads"), "{}", a.body);
    assert_eq!(srv.get("/").body, "hi");
    // Once they have gone, there is room.
    drop(held);
    assert!(until(|| {
        let mut s = ask(srv.port, "/big");
        read_to(&mut s, "\r\n").starts_with("HTTP/1.1 200")
    }));
}

/// The WebSocket a page opens at `path`, handshake done.
fn websocket(port: u16, path: &str) -> TcpStream {
    let mut s = connect(port);
    write!(s, "GET {} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: a2V5\r\nSec-WebSocket-Version: 13\r\n\r\n", path).unwrap();
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        s.read_exact(&mut b).unwrap();
        head.push(b[0]);
    }
    assert!(head.starts_with(b"HTTP/1.1 101"), "{}", String::from_utf8_lossy(&head));
    s
}

/// A frame from the server: its opcode and text.
fn frame(s: &mut TcpStream) -> (u8, String) {
    let mut head = [0u8; 2];
    s.read_exact(&mut head).unwrap();
    assert!(head[1] < 126);
    let mut data = vec![0; head[1] as usize];
    s.read_exact(&mut data).unwrap();
    (head[0] & 0x0f, String::from_utf8_lossy(&data).into_owned())
}

const WATCHED: &str = r#"
route GET /app.css = file("app.css")
route GET /data.txt = file("data.txt")
route GET /own = file("app.qwe")
route GET /fragment = <p>x</p>
route GET /page = <html><body>"page"</body></html>
"#;

fn version(srv: &Srv) -> u64 {
    srv.get("/_quillo/events").body.parse().unwrap()
}

fn watched(name: &str) -> Srv {
    let d = folder(name, &[("app.qwe", WATCHED), ("app.css", "a {}"), ("data.txt", "one")]);
    let srv = run_in(&d, "app.qwe", &["--watch", "--metrics"], &[]);
    // The watcher notes what is served as it looks round.
    for path in ["/app.css", "/data.txt", "/own"] {
        assert_eq!(srv.get(path).status, 200, "{}", path);
    }
    srv
}

/// Writes `file` until `done` says the watcher saw it: it starts to follow a
/// file a moment after the file is first served.
fn touch_until(srv: &Srv, file: &str, mut done: impl FnMut() -> bool) {
    let mut n = 0;
    assert!(until(|| {
        n += 1;
        srv.write(file, &format!("/* {} */", n));
        std::thread::sleep(Duration::from_millis(60));
        done()
    }), "{}", srv.log());
}

#[test]
fn a_stylesheet_that_changes_is_swapped_in_open_pages() {
    let srv = watched("watch-css");
    let v = version(&srv);
    let mut socket = websocket(srv.port, &format!("/_quillo/events?v={}", v));
    socket.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
    let mut got = None;
    touch_until(&srv, "app.css", || {
        let mut head = [0u8; 2];
        if socket.read_exact(&mut head).is_ok() {
            let mut data = vec![0; head[1] as usize];
            socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            socket.read_exact(&mut data).unwrap();
            got = Some(String::from_utf8_lossy(&data).into_owned());
        }
        got.is_some()
    });
    let got = got.unwrap();
    assert!(got.starts_with("css:") && got.ends_with(":[\"/app.css\"]"), "{}", got);
    assert!(srv.log().contains("/app.css changed: swapped in open pages"), "{}", srv.log());
    // The program is the same version.
    assert_eq!(version(&srv), v);
}

#[test]
fn a_page_without_a_websocket_hears_of_a_stylesheet_too() {
    let srv = watched("watch-css-plain");
    let v = version(&srv);
    let port = srv.port;
    let wait = std::thread::spawn(move || send(port, "GET", &format!("/_quillo/events?v={}", v), &[], ""));
    let mut wait = Some(wait);
    let mut body = String::new();
    touch_until(&srv, "app.css", || {
        if wait.as_ref().is_some_and(|t| t.is_finished()) {
            body = wait.take().unwrap().join().unwrap().body;
        }
        !body.is_empty()
    });
    assert!(body.starts_with("css:") && body.ends_with(":[\"/app.css\"]"), "{}", body);
    assert_eq!(srv.get(&format!("/_quillo/events?v={}&c=1", v + 100)).body, v.to_string());
}

#[test]
fn a_served_file_that_changes_refreshes_open_pages() {
    let srv = watched("watch-file");
    let v = version(&srv);
    let mut socket = websocket(srv.port, &format!("/_quillo/events?v={}", v));
    touch_until(&srv, "data.txt", || version(&srv) != v);
    let (op, text) = frame(&mut socket);
    assert_eq!((op, text.parse::<u64>().unwrap() > v), (1, true));
    // And it says goodbye.
    assert_eq!(frame(&mut socket).0, 8);
    assert!(srv.log().contains("/data.txt changed: refreshing open pages"), "{}", srv.log());
}

#[test]
fn development_pages_get_the_reload_script_wherever_they_can() {
    let srv = watched("watch-script");
    let fragment = srv.get("/fragment").body;
    assert!(fragment.starts_with("<p>x</p><script>") && fragment.ends_with("</script>"), "{}", fragment);
    let page = srv.get("/page").body;
    assert!(page.find("<script>").unwrap() < page.find("</body>").unwrap(), "{}", page);
    // In development pages load their files one by one: no shared runtime.
    let r = srv.get("/_quillo/r/anything.js");
    assert_eq!((r.status, r.body.as_str(), r.header("cache-control")), (200, "", Some("no-cache")));
}

#[test]
fn a_program_that_will_not_load_is_shown_until_it_does() {
    let srv = run("watch-broken", "route GET / = \n", &["--watch"]);
    let a = srv.get("/");
    assert_eq!(a.status, 500);
    assert!(a.body.contains("The program did not load"), "{}", a.body);
    // Broken in another way: the page says so.
    srv.write("app.qwe", "route GET / = (\n");
    assert!(until(|| srv.get("/").body.contains("expected")), "{}", srv.get("/").body);
    let broken = srv.get("/").body;
    srv.write("app.qwe", "route GET / = \"fixed\"\n");
    assert!(until(|| srv.get("/").body == "fixed"), "{}", broken);
    // Past that, one that fails to load does not replace the one that works.
    srv.write("app.qwe", "route GET / = (\n");
    assert!(until(|| srv.log().contains("still serving the previous version")), "{}", srv.log());
    assert_eq!(srv.get("/").body, "fixed");
}

#[test]
fn a_reload_checks_the_program_first() {
    let srv = run("reload-check", "fn f(a: int) -> int = a\nroute GET / = str(f(1))\n", &["--reload"]);
    assert_eq!(srv.get("/").body, "1");
    srv.write("app.qwe", "fn f(a: int) -> int = a\nroute GET / = str(f(\"text\"))\n");
    assert!(until(|| srv.log().contains("still serving the previous version")), "{}", srv.log());
    assert!(srv.log().contains("error"), "{}", srv.log());
    assert_eq!(srv.get("/").body, "1");
}

#[test]
fn no_check_reloads_what_loads() {
    let srv = run("reload-no-check", "fn f(a: int) -> int = a\nroute GET / = str(f(1))\n", &["--reload", "--no-check"]);
    srv.write("app.qwe", "fn f(a: int) -> int = a\nroute GET / = \"loaded\"\nroute GET /never = str(f(\"text\"))\n");
    assert!(until(|| srv.get("/").body == "loaded"), "{}", srv.log());
}

#[test]
fn a_reload_retires_what_the_old_version_mounted() {
    let d = folder("reload-mount", &[
        ("app.qwe", "mount /inner = \"inner.qwe\"\nroute GET / = \"one\"\n"),
        ("inner.qwe", "fn work() { sleep(100000) }\nschedule(\"1h\", work)\nroute GET / = \"inner\"\n"),
    ]);
    let srv = run_in(&d, "app.qwe", &["--reload"], &[]);
    assert_eq!(srv.get("/inner").body, "inner");
    srv.write("app.qwe", "mount /inner = \"inner.qwe\"\nroute GET / = \"two\"\n");
    assert!(until(|| srv.get("/").body == "two"), "{}", srv.log());
    assert_eq!(srv.get("/inner").body, "inner");
}

/// A client that takes nothing for 30 s holds nothing up any longer.
#[test]
fn a_client_that_stops_reading_a_stream_is_let_go() {
    let srv = run("stalled", "route GET / = stream((send) => { while true { send(repeat(\"y\", 65536)) } })\nroute GET /ok = \"ok\"\n", &["--metrics"]);
    let held = ask(srv.port, "/");
    let deadline = Instant::now() + Duration::from_secs(45);
    while srv.connections() != 1 {
        assert!(Instant::now() < deadline, "{}", srv.connections());
        std::thread::sleep(Duration::from_millis(200));
    }
    drop(held);
    assert_eq!(srv.get("/ok").body, "ok");
}

/// A page that stays open hears from the server every 20 s, so that a page
/// long gone is found out.
#[cfg(unix)]
#[test]
fn a_waiting_websocket_is_pinged() {
    let srv = run("pings", "route POST /change { publish(\"t\"); \"ok\" }\nroute GET / = \"hi\"\n", &["--watch"]);
    assert_eq!(send(srv.port, "POST", "/change", &[], "").body, "ok");
    let seq = live_seq(&srv);
    let mut live = websocket(srv.port, &format!("/_quillo/live?since={}&t=t", seq));
    let mut events = websocket(srv.port, &format!("/_quillo/events?v={}", version(&srv)));
    // Pages gone: their threads find out at the ping.
    reset(websocket(srv.port, &format!("/_quillo/live?since={}&t=t", seq)));
    reset(websocket(srv.port, &format!("/_quillo/events?v={}", version(&srv))));
    live.set_read_timeout(Some(Duration::from_secs(25))).unwrap();
    events.set_read_timeout(Some(Duration::from_secs(25))).unwrap();
    let started = Instant::now();
    assert_eq!(frame(&mut live), (9, String::new()));
    assert_eq!(frame(&mut events), (9, String::new()));
    assert!(started.elapsed() >= Duration::from_secs(19), "{:?}", started.elapsed());
    // And a change after that.
    assert_eq!(send(srv.port, "POST", "/change", &[], "").body, "ok");
    let (op, text) = frame(&mut live);
    assert!(op == 1 && text.contains("\"changed\":[\"t\"]"), "{}", text);
}

#[test]
fn a_live_websocket_tells_a_page_from_elsewhere_where_things_stand() {
    let srv = run("live-socket", WAITING, &[]);
    let mut s = websocket(srv.port, "/_quillo/live?t=t");
    let (op, text) = frame(&mut s);
    assert_eq!(op, 1);
    assert!(text.contains("\"changed\":[]"), "{}", text);
    // A page from another server, or from before a restart.
    let mut s = websocket(srv.port, "/_quillo/live?since=5&s=another&t=t");
    let (_, text) = frame(&mut s);
    assert!(text.contains("\"changed\":[\"t\"]"), "{}", text);
}

#[test]
fn a_channel_socket_needs_a_ticket() {
    let srv = run("no-ticket", "route GET / = \"hi\"\n", &[]);
    let mut s = connect(srv.port);
    s.write_all(b"GET /_quillo/wt?t=forged HTTP/1.1\r\nHost: x\r\nConnection: close\r\nUpgrade: websocket\r\nSec-WebSocket-Key: a2V5\r\n\r\n").unwrap();
    let a = all(&mut s);
    assert!(a.starts_with("HTTP/1.1 403") && a.contains("no ticket"), "{}", a);
}

#[test]
fn the_json_log_names_the_tenant() {
    let srv = run("json-log", "tenant_from((r) => r.headers[\"x-tenant\"])\nroute GET / = str(tenant())\n", &["--log", "json"]);
    assert_eq!(send(srv.port, "GET", "/", &[("x-tenant", "acme")], "").body, "acme");
    assert!(until(|| srv.log().contains("\"tenant\":\"acme\"")), "{}", srv.log());
}

#[test]
fn the_log_can_be_off_and_quillos_own_calls_are_not_counted() {
    let srv = run("no-log", "route GET / = \"hi\"\n", &["--log", "off", "--metrics"]);
    assert_eq!(srv.get("/").status, 200);
    assert!(!srv.log().contains("GET / ->"), "{}", srv.log());
    let m = srv.get("/_quillo/metrics").body;
    assert!(m.contains("quillo_requests_total{status=\"2xx\"} 1"), "{}", m);
}
