//! `quillo run` as a server: the options seen from outside, what runs beside the program, and
//! how it stops on a signal.

mod support;

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::{free_port, send, Guard};

/// A secret long enough that no warning is printed about it.
const SECRET: &str = "a signing secret for these tests, long enough";

fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("cli-srv-{}", name));
    for (file, text) in files {
        std::fs::write(d.join(file), text).unwrap();
    }
    d
}

/// Whether a server there answers a request: more than `listening`, as the first connection
/// to a server just started can be reset or refused.
fn answers(port: u16) -> bool {
    let Ok(mut c) = TcpStream::connect(("127.0.0.1", port)) else { return false };
    let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
    let asked = c.write_all(b"GET /_quillo/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    asked.is_ok() && c.read(&mut [0u8; 1]).map_or(false, |n| n > 0)
}

/// A running `quillo run`, what it says going to a file.
struct Running {
    server: Guard,
    port: u16,
    log: PathBuf,
}

/// Starts `quillo run` on `file`, in its folder, and waits until it answers.
fn run(dir: &Path, file: &str, extra: &[&str], env: &[(&str, &str)]) -> Running {
    let mut tries = 0;
    loop {
        match start(dir, file, extra, env) {
            Ok(r) => return r,
            // Another test's server, in this process or not, got to the port first.
            Err(said) if said.contains("Address already in use") && tries < 5 => tries += 1,
            Err(said) => panic!("quillo stopped: {}", said),
        }
    }
}

fn start(dir: &Path, file: &str, extra: &[&str], env: &[(&str, &str)]) -> Result<Running, String> {
    let port = free_port();
    let log = dir.join(format!("{}-{}.log", file, port));
    let out = File::create(&log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", file, "--port", &port.to_string()])
        .args(extra)
        .current_dir(dir)
        .env("HOME", dir)
        .env("QUILLO_SECRET", SECRET)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(out.try_clone().unwrap())
        .stderr(out)
        .spawn()
        .expect("cannot start quillo");
    let mut r = Running { server: Guard::new(child), port, log };
    let deadline = Instant::now() + Duration::from_secs(20);
    while !answers(port) {
        if let Ok(Some(status)) = r.server.0.try_wait() {
            return Err(format!("{}\n{}", status, r.said()));
        }
        assert!(Instant::now() < deadline, "the server did not start\n{}", r.said());
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(r)
}

impl Running {
    fn said(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// Waits until it has said `text`.
    fn wait_for(&self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.said().contains(text) {
            assert!(Instant::now() < deadline, "it never said {:?}:\n{}", text, self.said());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn interrupt(&self) {
        let ok = Command::new("kill").args(["-INT", &self.server.0.id().to_string()]).status().unwrap();
        assert!(ok.success());
    }

    /// Waits for it to stop by itself, and gives its exit status.
    fn exit_code(&mut self) -> Option<i32> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.server.0.try_wait().unwrap() {
                return status.code();
            }
            assert!(Instant::now() < deadline, "it did not stop:\n{}", self.said());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn get(&self, path: &str) -> support::Answer {
        answered(self.port, "GET", path, "")
    }

    /// Waits until `path` answers `status`, as a reload takes a moment.
    fn wait_until_status(&self, path: &str, status: u16) -> support::Answer {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let a = self.get(path);
            if a.status == status {
                return a;
            }
            assert!(Instant::now() < deadline, "{} stayed {}: {}\n{}", path, a.status, a.body, self.said());
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// `send`, again if the connection is reset: a first request after a server starts can be.
fn answered(port: u16, method: &str, path: &str, body: &str) -> support::Answer {
    for _ in 0..5 {
        let a = send(port, method, path, &[], body);
        if a.status != 0 {
            return a;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    send(port, method, path, &[], body)
}

fn alive(pid: &str) -> bool {
    Command::new("kill").args(["-0", pid]).stderr(Stdio::null()).status().unwrap().success()
}

#[test]
fn commands_run_beside_the_program_in_its_folder_and_stop_with_it() {
    let dir = folder("with", &[("app.qwe", "route GET / = \"hi\"\n"), ("side.txt", "next to the program\n")]);
    let mut s = run(
        &dir,
        "app.qwe",
        &["--with", "cat side.txt", "--with", "WORD=there /bin/sh -c 'echo to-stderr-$WORD >&2'", "--with", "echo $$ > side.pid; exec sleep 300"],
        &[],
    );
    s.wait_for("quillo: started `cat side.txt` beside the program");
    // Each line is marked with the command's first word, without its folder or settings.
    s.wait_for("[cat] next to the program");
    s.wait_for("[sh] to-stderr-there");
    assert_eq!(s.get("/").body, "hi");

    let deadline = Instant::now() + Duration::from_secs(20);
    let pid = loop {
        let text = std::fs::read_to_string(dir.join("side.pid")).unwrap_or_default();
        if !text.trim().is_empty() {
            break text.trim().to_string();
        }
        assert!(Instant::now() < deadline, "the command did not start");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(alive(&pid));
    s.interrupt();
    assert_eq!(s.exit_code(), Some(0), "{}", s.said());
    assert!(!alive(&pid), "the command stops with the program");
}

/// Answers every request with 200, keeping each one's request line and headers.
fn recording_server() -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let log = seen.clone();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let log = log.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(s);
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                }
                log.lock().unwrap().push(head);
                let _ = r.get_mut().write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            });
        }
    });
    (port, seen)
}

fn wait_for_request(seen: &Mutex<Vec<String>>, start: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(r) = seen.lock().unwrap().iter().find(|r| r.starts_with(start)) {
            return r.clone();
        }
        assert!(Instant::now() < deadline, "no {:?} in {:?}", start, seen.lock().unwrap());
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_registered_server_leaves_the_pool_when_it_is_stopped() {
    let dir = folder("register", &[("app.qwe", "route GET / = \"hi\"\n")]);
    let (admin, seen) = recording_server();
    let url = format!("http://127.0.0.1:{}", admin);
    let mut s = run(&dir, "app.qwe", &["--register", &url, "--service", "shop"], &[("LIMER_ADMIN_SECRET", SECRET)]);
    let joined = wait_for_request(&seen, &format!("POST /register?service=shop&port={}&ttl=", s.port));
    assert!(joined.to_lowercase().contains(&format!("authorization: bearer {}", SECRET).to_lowercase()), "{}", joined);
    s.wait_for(&format!("quillo: registered with {} as a member of `shop`", url));
    s.interrupt();
    wait_for_request(&seen, &format!("POST /deregister?service=shop&port={} ", s.port));
    assert_eq!(s.exit_code(), Some(0), "{}", s.said());
    s.wait_for(&format!("quillo: left `shop` at {}", url));
}

#[test]
fn a_copy_of_a_service_joins_the_app_and_leaves_it_when_stopped() {
    let app = "service risk = \"risk.qwe\"\nroute GET / = \"app\"\n";
    let dir = folder("join", &[("app.qwe", app), ("risk.qwe", "server fn score() = 7\n"), ("spare.qwe", "server fn score() = 8\n")]);
    let env = [("QUILLO_SERVICE_SECRET", SECRET)];
    let a = run(&dir, "app.qwe", &[], &env);
    let url = format!("http://127.0.0.1:{}", a.port);
    // By the name of its file, or by the one it is given.
    for (file, extra) in [("risk.qwe", &[][..]), ("spare.qwe", &["--service-name", "risk"])] {
        let mut args = vec!["--as-service", "--join", &url];
        args.extend_from_slice(extra);
        let mut copy = run(&dir, file, &args, &env);
        copy.wait_for(&format!("quillo: joined {} as a copy of `risk`", url));
        copy.interrupt();
        assert_eq!(copy.exit_code(), Some(0), "{}", copy.said());
        copy.wait_for(&format!("quillo: left {} (`risk`)", url));
    }
}

#[test]
fn a_second_signal_stops_at_once_without_waiting_for_requests() {
    let dir = folder("twice", &[("app.qwe", "route GET /slow {\n    print(\"slow request started\")\n    sleep(60000)\n    \"done\"\n}\n")]);
    let mut s = run(&dir, "app.qwe", &[], &[]);
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    write!(c, "GET /slow HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    s.wait_for("slow request started");
    s.interrupt();
    s.wait_for("quillo: stopping once the requests in progress are answered (again to stop now)");
    assert_eq!(s.server.0.try_wait().unwrap(), None, "the first signal waits for the request");
    s.interrupt();
    assert_eq!(s.exit_code(), Some(130), "{}", s.said());
    let mut rest = Vec::new();
    let _ = c.read_to_end(&mut rest);
    assert!(!String::from_utf8_lossy(&rest).contains("done"));
}

#[test]
fn the_first_signal_answers_what_is_in_progress_and_stops() {
    let dir = folder("once", &[("app.qwe", "route GET /slow {\n    print(\"slow request started\")\n    sleep(500)\n    \"done\"\n}\n")]);
    let mut s = run(&dir, "app.qwe", &[], &[]);
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    write!(c, "GET /slow HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    s.wait_for("slow request started");
    s.interrupt();
    let mut answer = String::new();
    c.read_to_string(&mut answer).unwrap();
    assert!(answer.ends_with("done"), "{}", answer);
    assert_eq!(s.exit_code(), Some(0), "{}", s.said());
}

#[test]
fn watch_serves_a_broken_program_and_recovers_when_it_is_fixed() {
    // Stopped by the checker: the details are in the terminal.
    let dir = folder("watch-broken", &[("app.qwe", "route GET / = nope\n")]);
    let s = run(&dir, "app.qwe", &["--watch"], &[]);
    s.wait_for("app.qwe:1: error: unknown name `nope`");
    let a = s.get("/");
    assert_eq!(a.status, 500, "{}", a.body);
    assert!(a.body.contains("the program has errors (see the terminal)"), "{}", a.body);
    std::fs::write(dir.join("app.qwe"), "route GET / = \"fixed\"\n").unwrap();
    assert_eq!(s.wait_until_status("/", 200).body, "fixed");
}

#[test]
fn watch_without_the_checker_shows_the_error_on_the_page() {
    let dir = folder("watch-run", &[("app.qwe", "fn zero() = 0\nprint(1 / zero())\nroute GET / = \"hi\"\n")]);
    let s = run(&dir, "app.qwe", &["--watch", "--no-check"], &[]);
    s.wait_for("quillo: app.qwe:2: division by zero");
    let a = s.get("/");
    assert_eq!(a.status, 500, "{}", a.body);
    assert!(a.body.contains("division by zero"), "{}", a.body);
}

#[test]
fn watch_stays_up_for_a_program_with_no_routes_yet() {
    let dir = folder("watch-empty", &[("app.qwe", "print(\"no routes yet\")\n")]);
    let s = run(&dir, "app.qwe", &["-w"], &[]);
    assert_eq!(s.get("/").status, 404);
    std::fs::write(dir.join("app.qwe"), "route GET / = \"now one\"\n").unwrap();
    assert_eq!(s.wait_until_status("/", 200).body, "now one");
}

#[test]
fn no_prefetch_leaves_the_prefetching_script_out_of_pages() {
    let dir = folder("prefetch", &[("app.qwe", "route GET / = <html><body><a href=\"/x\">x</a></body></html>\n")]);
    let on = run(&dir, "app.qwe", &[], &[]);
    assert!(on.get("/").body.contains("pointerdown"), "{}", on.get("/").body);
    let off = run(&dir, "app.qwe", &["--no-prefetch"], &[]);
    let page = off.get("/").body;
    assert!(page.contains("<a href=\"/x\">x</a>") && !page.contains("pointerdown"), "{}", page);
}

#[test]
fn plain_http_keeps_cookies_from_being_secure() {
    let dir = folder("plain", &[("app.qwe", "route GET / {\n    set_cookie(\"who\", \"me\")\n    \"hi\"\n}\n")]);
    let secure = run(&dir, "app.qwe", &[], &[]);
    let cookie = secure.get("/").header("set-cookie").unwrap().to_string();
    assert!(cookie.contains("; Secure"), "{}", cookie);
    let plain = run(&dir, "app.qwe", &["--plain-http"], &[]);
    let cookie = plain.get("/").header("set-cookie").unwrap().to_string();
    assert!(cookie.starts_with("who=me") && !cookie.contains("Secure"), "{}", cookie);
}

#[test]
fn query_stats_are_kept_when_asked_for_or_set_in_the_environment() {
    let dir = folder("stats", &[("app.qwe", "db.open(\"s.db\")\ndb.exec(\"create table if not exists t (n int)\")\nroute GET / = str(len(db.query(\"select * from t\")))\n")]);
    let figures = |s: &Running| -> serde_json::Value {
        assert_eq!(s.get("/").body, "0");
        serde_json::from_str(&s.get("/_quillo/health/queries").body).unwrap()
    };
    assert_eq!(figures(&run(&dir, "app.qwe", &[], &[]))["enabled"], false);
    for (extra, env) in [(&["--query-stats"][..], &[][..]), (&[], &[("QUILLO_QUERY_STATS", "1")]), (&[], &[("QUILLO_QUERY_STATS", "true")])] {
        let stats = figures(&run(&dir, "app.qwe", extra, env));
        assert_eq!(stats["enabled"], true, "{:?} {:?}", extra, env);
        assert!(stats["queries"]["total"].as_u64().unwrap() >= 1, "{}", stats);
    }
}

#[test]
fn describe_answers_with_the_programs_api_only_when_asked() {
    let dir = folder("describe", &[("app.qwe", "route GET /items = \"[]\"\n")]);
    assert_eq!(run(&dir, "app.qwe", &[], &[]).get("/_quillo/describe").status, 404);
    let a = run(&dir, "app.qwe", &["--describe"], &[]).get("/_quillo/describe");
    assert_eq!(a.status, 200);
    let doc: serde_json::Value = serde_json::from_str(&a.body).unwrap();
    assert!(doc["paths"]["/items"].is_object(), "{}", doc);
}

#[test]
fn the_log_option_picks_how_requests_are_logged() {
    let dir = folder("log", &[("app.qwe", "route GET /here = \"hi\"\n")]);
    let json = run(&dir, "app.qwe", &["--log", "json"], &[]);
    assert_eq!(json.get("/here").body, "hi");
    json.wait_for("\"path\":\"/here\"");
    let text = run(&dir, "app.qwe", &["--log", "off", "--log", "text"], &[]);
    assert_eq!(text.get("/here").body, "hi");
    text.wait_for("/here");
    let off = run(&dir, "app.qwe", &["--log", "off"], &[]);
    assert_eq!(off.get("/here").body, "hi");
    // The first line of the log would be out by now; a later request's is what shows it.
    assert_eq!(off.get("/here").body, "hi");
    assert!(!off.said().contains("/here"), "{}", off.said());
}

#[test]
fn a_service_url_sends_its_calls_to_that_server() {
    let ledger = "let runs = 0\nserver fn add(x: int) -> int {\n    runs += 1\n    x + 1\n}\nroute GET /runs = str(runs)\n";
    let app = "service ledger = \"ledger.qwe\"\nroute GET / = str(ledger.add(1))\n";
    let dir = folder("service-url", &[("ledger.qwe", ledger), ("app.qwe", app)]);
    let env = [("QUILLO_SERVICE_SECRET", SECRET)];
    let far = run(&dir, "ledger.qwe", &[], &env);
    let url = format!("ledger=http://127.0.0.1:{}", far.port);
    let s = run(&dir, "app.qwe", &["--service-url", &url], &env);
    assert_eq!(s.get("/").body, "2", "{}", s.said());
    assert_eq!(far.get("/runs").body, "1", "the call ran there, not here");
}

#[test]
#[ignore = "finding: `--with` commands outlive a program that fails to load, ends with nothing to serve, or cannot listen: `run` returns without with::stop_all"]
fn commands_beside_a_program_stop_when_it_fails_to_start() {
    let dir = folder("with-failed", &[("bad.qwe", "route GET / = nope\n")]);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "bad.qwe", "--with", "echo $$ > side.pid; exec sleep 30"])
        .current_dir(&dir)
        .env("HOME", &dir)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    let deadline = Instant::now() + Duration::from_secs(5);
    let pid = loop {
        let text = std::fs::read_to_string(dir.join("side.pid")).unwrap_or_default();
        if !text.trim().is_empty() {
            break text.trim().to_string();
        }
        assert!(Instant::now() < deadline, "the command did not start");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(!alive(&pid), "the command stops with the program");
}

const CHANNEL: &str = "channel.on(\"room\", (m, peer) => { })\nroute GET / = \"hi\"\n";

/// What a page is told when it asks how to connect.
fn ticket(s: &Running) -> serde_json::Value {
    let a = answered(s.port, "POST", "/_quillo/wt-ticket", "{}");
    assert_eq!(a.status, 200, "{}", a.body);
    let j: serde_json::Value = serde_json::from_str(&a.body).unwrap();
    assert!(j["ticket"].is_string(), "{}", j);
    j
}

/// From `free_port`'s range: one the system picks can be given to someone else at once.
fn free_udp_port() -> u16 {
    loop {
        let port = free_port();
        if std::net::UdpSocket::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
}

#[test]
fn wt_port_is_what_pages_are_told() {
    let dir = folder("wt-port", &[("app.qwe", CHANNEL)]);
    let port = free_udp_port();
    let s = run(&dir, "app.qwe", &["--wt-port", &port.to_string()], &[]);
    let j = ticket(&s);
    s.wait_for(&format!("quillo: WebTransport on UDP port {}", port));
    assert_eq!(j["port"], port);
    assert!(j["hash"].is_string(), "a certificate of its own is passed by its hash: {}", j);
}

#[test]
fn a_certificate_given_is_presented_and_pages_need_no_hash() {
    let dir = folder("wt-tls", &[("app.qwe", CHANNEL)]);
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap().self_signed(&key).unwrap();
    std::fs::write(dir.join("cert.pem"), cert.pem()).unwrap();
    std::fs::write(dir.join("key.pem"), key.serialize_pem()).unwrap();
    let port = free_udp_port();
    let s = run(&dir, "app.qwe", &["--wt-port", &port.to_string(), "--tls-cert", "cert.pem", "--tls-key", "key.pem"], &[]);
    let j = ticket(&s);
    assert_eq!(j["port"], port, "{}", s.said());
    assert!(j["hash"].is_null(), "{}", j);
}

#[test]
fn no_wt_leaves_pages_to_the_websocket() {
    let dir = folder("no-wt", &[("app.qwe", CHANNEL)]);
    let s = run(&dir, "app.qwe", &["--no-wt"], &[]);
    let j = ticket(&s);
    assert!(j["port"].is_null(), "no port to connect to: {}", j);
}
