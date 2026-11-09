//! A stop (`kill <pid>`, SIGTERM, Ctrl-C) takes no new requests and waits for every request and
//! transaction in progress, however long they take, unless `--drain` gives it a time; the
//! services `--split` started keep serving the app until it has answered its own, then finish
//! what they are doing (the user's graceful stop, 2026-11-09; branch feedback56). And, decided
//! with it, `quillo check`'s warning of an `idempotent` that changes data in another program.
#![cfg(unix)]

mod support;

use quillo::server::HttpRequest;
use quillo::Interp;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A `quillo run` a test started, its stderr in a file; killed when dropped.
struct Run {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Run {
    fn start(d: &Path, file: &str, args: &[&str]) -> Run {
        Run::start_as(d, file, args, false)
    }

    /// `group`: in a process group of its own, as a shell runs a command in a terminal, which a
    /// Ctrl-C there reaches whole.
    fn start_as(d: &Path, file: &str, args: &[&str], group: bool) -> Run {
        use std::os::unix::process::CommandExt;
        let port = support::free_port();
        let log = d.join(format!("{}.log", file));
        let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
        c.arg("run").arg(d.join(file)).args(["--port", &port.to_string()]).args(args);
        c.env("QUILLO_SECRET", "test secret for the tests").current_dir(d);
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(std::fs::File::create(&log).unwrap());
        if group {
            c.process_group(0);
        }
        let child = c.spawn().unwrap();
        support::stop_with_tests(&child);
        let mut r = Run { child, port, log };
        let deadline = Instant::now() + Duration::from_secs(30);
        while !support::listening(port) {
            if let Ok(Some(st)) = r.child.try_wait() {
                panic!("stopped: {}\n{}", st, r.log());
            }
            assert!(Instant::now() < deadline, "did not start:\n{}", r.log());
            std::thread::sleep(Duration::from_millis(30));
        }
        r
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn signal(&self, sig: i32) {
        unsafe { libc::kill(self.child.id() as i32, sig) };
    }

    /// Its log, once it says `what`.
    fn says(&self, what: &str, within: Duration) -> String {
        let deadline = Instant::now() + within;
        loop {
            let log = self.log();
            if log.contains(what) {
                return log;
            }
            assert!(Instant::now() < deadline, "{:?} not said:\n{}", what, log);
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    /// Its exit status and how long it took to end, from now, within `within`.
    fn ends(&mut self, within: Duration) -> (Option<i32>, Duration) {
        let at = Instant::now();
        loop {
            if let Ok(Some(st)) = self.child.try_wait() {
                return (st.code(), at.elapsed());
            }
            assert!(at.elapsed() < within, "still running after {:?}:\n{}", within, self.log());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The processes it started (`--split`'s services): those of the tests' own server, and no
    /// other.
    fn children(&self) -> Vec<i32> {
        let o = Command::new("pgrep").args(["-P", &self.child.id().to_string()]).output().unwrap();
        String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect()
    }
}

/// Whether process `pid` is there (a zombie no longer counts: its parent is gone or waited).
fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

fn gone_within(pid: i32, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while alive(pid) {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("graceful-{}", name));
    for (f, src) in files {
        std::fs::write(d.join(f), src).unwrap();
    }
    d
}

/// A request on a connection of its own, answered in another thread: its whole answer, as read
/// until the server closes the connection.
fn asked(port: u16, path: &str) -> std::thread::JoinHandle<String> {
    let path = path.to_string();
    std::thread::spawn(move || {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        write!(s, "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", path).unwrap();
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    })
}

/// What `select` gives in the database `db` of folder `d`, read by a program of its own once the
/// server has stopped.
fn rows(d: &Path, db: &str, select: &str) -> String {
    let check = d.join("check.qwe");
    std::fs::write(&check, format!("db.open(\"{}\")\nroute GET / = str(db.query(\"{}\"))\n", db, select)).unwrap();
    let mut it = Interp::new(d);
    it.load_file(&check).unwrap();
    let r = it.handle(&HttpRequest { method: "GET".into(), url: "/".into(), headers: vec![("Host".into(), "localhost".into())], body: Vec::new(), remote: "127.0.0.1".into() });
    String::from_utf8_lossy(&r.body).into_owned()
}

const PAY: &str = r#"
db.open("app.db")
db.exec("create table if not exists paid (n int)")
route GET /ping = "pong"
route GET /pay {
    let _ = db.transaction(() => {
        db.exec("insert into paid values (1)")
        sleep(12500)
        db.exec("insert into paid values (2)")
    })
    "paid"
}
route GET /six {
    sleep(6000)
    "six"
}
route GET /short {
    sleep(1500)
    "short"
}
route GET /feed = stream((send) => {
    send("a\n")
    sleep(60000)
    send("b\n")
})
route GET /feed_tx = stream((send) => {
    send("a\n")
    let _ = db.transaction(() => {
        db.exec("insert into paid values (7)")
        sleep(1500)
        db.exec("insert into paid values (8)")
    })
    send("b\n")
})
route GET /big = file("big.bin")
"#;

#[test]
fn a_stop_waits_for_a_transaction_longer_than_ten_seconds_and_takes_no_new_requests() {
    let d = dir("pay", &[("main.qwe", PAY)]);
    let mut s = Run::start(&d, "main.qwe", &[]);
    let paying = asked(s.port, "/pay");
    std::thread::sleep(Duration::from_millis(500));
    s.signal(libc::SIGTERM);
    s.says("stopping once the requests in progress are answered", Duration::from_secs(5));
    // No new connection is taken while it waits.
    let deadline = Instant::now() + Duration::from_secs(2);
    while TcpStream::connect(("127.0.0.1", s.port)).is_ok() {
        assert!(Instant::now() < deadline, "a new connection was taken while stopping");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Every 5 s it says what it still waits for; before, the request was cut off at 10 s.
    s.says("quillo: still answering GET /pay", Duration::from_secs(8));
    let answer = paying.join().unwrap();
    assert!(answer.starts_with("HTTP/1.1 200"), "{}", answer);
    assert!(answer.ends_with("paid"), "{}", answer);
    let (code, _) = s.ends(Duration::from_secs(5));
    assert_eq!(code, Some(0), "{}", s.log());
    assert!(!s.log().contains("stopping with requests in progress"), "{}", s.log());
    // The transaction committed whole.
    assert_eq!(rows(&d, "app.db", "select n from paid order by n"), "[{ n: 1 }, { n: 2 }]");
}

#[test]
fn a_second_stop_ends_it_at_once_and_leaves_no_half_transaction() {
    let d = dir("again", &[("main.qwe", PAY)]);
    let mut s = Run::start(&d, "main.qwe", &[]);
    let paying = asked(s.port, "/pay");
    std::thread::sleep(Duration::from_millis(500));
    s.signal(libc::SIGTERM);
    s.says("(again to stop now)", Duration::from_secs(5));
    s.signal(libc::SIGTERM);
    let (code, took) = s.ends(Duration::from_secs(3));
    assert_eq!(code, Some(130), "{}", s.log());
    assert!(took < Duration::from_secs(2), "{:?}", took);
    let _ = paying.join();
    assert_eq!(rows(&d, "app.db", "select n from paid"), "[]");
}

#[test]
fn drain_gives_the_stop_a_time_after_which_what_runs_is_cut_off() {
    let d = dir("drain", &[("main.qwe", PAY)]);
    // A length of time, more than none.
    for bad in ["0s", "soon", "5", "5h", ""] {
        let o = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["run", "main.qwe", "--drain", bad]).current_dir(&d).output().unwrap();
        assert_eq!(o.status.code(), Some(2), "{:?}", bad);
        assert!(String::from_utf8_lossy(&o.stderr).contains("--drain needs a length of time: 500ms, 30s or 2m"), "{:?}: {}", bad, String::from_utf8_lossy(&o.stderr));
    }
    let mut s = Run::start(&d, "main.qwe", &["--drain", "1s"]);
    let six = asked(s.port, "/six");
    std::thread::sleep(Duration::from_millis(400));
    s.signal(libc::SIGTERM);
    let (code, took) = s.ends(Duration::from_secs(5));
    assert_eq!(code, Some(0), "{}", s.log());
    assert!(took < Duration::from_secs(3), "{:?}", took);
    // Cut off as a request is at the end of the drain, said so.
    assert!(s.log().contains("quillo: stopped with GET /six still running"), "{}", s.log());
    assert!(!six.join().unwrap().contains("six"));
    // Under its time, a request in progress is answered as without one.
    let mut s = Run::start(&d, "main.qwe", &["--drain", "1m"]);
    let short = asked(s.port, "/short");
    std::thread::sleep(Duration::from_millis(300));
    s.signal(libc::SIGTERM);
    assert!(short.join().unwrap().ends_with("short"));
    assert_eq!(s.ends(Duration::from_secs(5)).0, Some(0));
}

#[test]
fn an_answer_given_while_stopping_closes_its_connection_and_says_so() {
    let d = dir("close", &[("main.qwe", PAY)]);
    let mut s = Run::start(&d, "main.qwe", &[]);
    // A connection kept for more: the first answer comes after the stop is asked for.
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    c.write_all(b"GET /short HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    s.signal(libc::SIGTERM);
    let mut out = Vec::new();
    let _ = c.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out).into_owned();
    let head = out.split("\r\n\r\n").next().unwrap_or("").to_ascii_lowercase();
    assert!(head.starts_with("http/1.1 200"), "{}", out);
    assert!(head.contains("\r\nconnection: close"), "{}", out);
    assert!(out.ends_with("short"), "{}", out);
    assert_eq!(s.ends(Duration::from_secs(5)).0, Some(0));
    // Before a stop, the same request keeps its connection.
    let s = Run::start(&d, "main.qwe", &[]);
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    c.write_all(b"GET /ping HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    let mut buf = [0u8; 512];
    let n = c.read(&mut buf).unwrap();
    assert!(!String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase().contains("connection: close"));
}

#[test]
fn a_stream_asleep_ends_at_a_stop_as_a_client_gone() {
    let d = dir("feed", &[("main.qwe", PAY)]);
    let mut s = Run::start(&d, "main.qwe", &[]);
    let feed = asked(s.port, "/feed");
    std::thread::sleep(Duration::from_millis(500));
    s.signal(libc::SIGTERM);
    // Not after its minute's sleep (nor at 10 s, as before).
    let (code, took) = s.ends(Duration::from_secs(20));
    assert_eq!(code, Some(0), "{}", s.log());
    assert!(took < Duration::from_secs(3), "{:?}", took);
    let got = feed.join().unwrap();
    assert!(got.contains("a\n") && !got.contains("b\n"), "{}", got);
    assert!(!s.log().contains("error while streaming"), "{}", s.log());
    // In a transaction, a stream's sleep is waited out: the transaction commits whole (found in
    // review).
    let mut s = Run::start(&d, "main.qwe", &[]);
    let feed = asked(s.port, "/feed_tx");
    std::thread::sleep(Duration::from_millis(500));
    s.signal(libc::SIGTERM);
    assert_eq!(s.ends(Duration::from_secs(10)).0, Some(0), "{}", s.log());
    assert!(feed.join().unwrap().contains("a\n"));
    assert_eq!(rows(&d, "app.db", "select n from paid where n >= 7 order by n"), "[{ n: 7 }, { n: 8 }]");
}

#[test]
fn a_stop_finishes_sending_a_large_file() {
    let d = dir("big", &[("main.qwe", PAY)]);
    // Larger than what the sockets' buffers take in, and than what is sent from memory.
    let big: Vec<u8> = (0..24u32 << 20).map(|i| (i % 251) as u8).collect();
    std::fs::write(d.join("big.bin"), &big).unwrap();
    let mut s = Run::start(&d, "main.qwe", &[]);
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    c.write_all(b"GET /big HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    let mut first = [0u8; 4096];
    let n = c.read(&mut first).unwrap();
    // The download is under way, and waits for the reader: the stop is asked for then.
    std::thread::sleep(Duration::from_millis(500));
    s.signal(libc::SIGTERM);
    std::thread::sleep(Duration::from_millis(1500));
    let mut rest = Vec::new();
    let _ = c.read_to_end(&mut rest);
    let mut got = first[..n].to_vec();
    got.extend(rest);
    let body = got.windows(4).position(|w| w == b"\r\n\r\n").map(|at| got[at + 4..].to_vec()).unwrap_or_default();
    assert_eq!(body.len(), big.len(), "the download was cut off\n{}", s.log());
    assert!(body == big);
    assert_eq!(s.ends(Duration::from_secs(5)).0, Some(0), "{}", s.log());
}

const SPLIT_APP: &str = r#"
service billing = "billing.qwe" { timeout: "1s", retries: 0 }
route GET /ping = "pong"
route GET /start = billing.slow(3000) ?? "gave up"
route GET /later {
    sleep(1500)
    billing.hello()
}
"#;

const BILLING: &str = r#"
db.open("billing.db")
db.exec("create table if not exists done (n int)")
server fn slow(ms: int) -> str {
    sleep(ms)
    db.exec("insert into done values (?)", ms)
    "slept"
}
server fn hello() -> str = "hello"
"#;

#[test]
fn split_services_finish_what_they_do_after_the_app_stops() {
    let d = dir("split-finish", &[("main.qwe", SPLIT_APP), ("billing.qwe", BILLING)]);
    let mut s = Run::start(&d, "main.qwe", &["--split"]);
    let services = s.children();
    assert_eq!(services.len(), 1, "{:?}\n{}", services, s.log());
    // The app gave up on the call after its second; billing goes on with it.
    assert!(asked(s.port, "/start").join().unwrap().ends_with("gave up"));
    s.signal(libc::SIGTERM);
    let (code, _) = s.ends(Duration::from_secs(10));
    assert_eq!(code, Some(0), "{}", s.log());
    // Before, billing was killed once the app had nothing in progress, and its call cut off.
    assert!(gone_within(services[0], Duration::from_secs(2)));
    assert_eq!(rows(&d, "billing.db", "select n from done"), "[{ n: 3000 }]");
}

#[test]
fn a_split_services_first_signal_after_its_app_told_it_to_stop_is_a_first() {
    // The app has nothing in progress and tells billing to stop before billing's own signal is
    // seen (systemd signals both; or `kill app; sleep 1; kill billing`): billing still finishes
    // its call. It had taken its first signal for a second, and ended (found in review).
    let d = dir("split-late-signal", &[("main.qwe", SPLIT_APP), ("billing.qwe", BILLING)]);
    let mut s = Run::start(&d, "main.qwe", &["--split"]);
    let services = s.children();
    assert!(asked(s.port, "/start").join().unwrap().ends_with("gave up"));
    s.signal(libc::SIGTERM);
    s.says("[billing] quillo: stopping once the requests in progress are answered", Duration::from_secs(5));
    unsafe { libc::kill(services[0], libc::SIGTERM) };
    assert_eq!(s.ends(Duration::from_secs(10)).0, Some(0), "{}", s.log());
    assert!(gone_within(services[0], Duration::from_secs(2)));
    assert_eq!(rows(&d, "billing.db", "select n from done"), "[{ n: 3000 }]", "{}", s.log());
}

#[test]
fn a_split_service_signalled_with_its_app_serves_the_apps_requests_in_progress() {
    // As systemd stops a unit: every process of it gets SIGTERM at once.
    let d = dir("split-unit", &[("main.qwe", SPLIT_APP), ("billing.qwe", BILLING)]);
    let mut s = Run::start(&d, "main.qwe", &["--split"]);
    let services = s.children();
    assert_eq!(services.len(), 1, "{:?}", services);
    let later = asked(s.port, "/later");
    std::thread::sleep(Duration::from_millis(400));
    s.signal(libc::SIGTERM);
    unsafe { libc::kill(services[0], libc::SIGTERM) };
    let answer = later.join().unwrap();
    assert!(answer.starts_with("HTTP/1.1 200") && answer.ends_with("hello"), "{}\n{}", answer, s.log());
    assert_eq!(s.ends(Duration::from_secs(10)).0, Some(0), "{}", s.log());
    assert!(gone_within(services[0], Duration::from_secs(2)));
    assert!(s.log().contains("[billing] quillo: stopping when the app stops"), "{}", s.log());
}

#[test]
fn a_ctrl_c_in_the_terminal_reaches_the_app_which_stops_its_services_after() {
    let d = dir("split-ctrl-c", &[("main.qwe", SPLIT_APP), ("billing.qwe", BILLING)]);
    let mut s = Run::start_as(&d, "main.qwe", &["--split"], true);
    let services = s.children();
    assert_eq!(services.len(), 1, "{:?}", services);
    let later = asked(s.port, "/later");
    std::thread::sleep(Duration::from_millis(400));
    // The terminal's Ctrl-C: SIGINT to the whole process group in front.
    unsafe { libc::kill(-(s.child.id() as i32), libc::SIGINT) };
    let answer = later.join().unwrap();
    assert!(answer.starts_with("HTTP/1.1 200") && answer.ends_with("hello"), "{}\n{}", answer, s.log());
    assert_eq!(s.ends(Duration::from_secs(10)).0, Some(0), "{}", s.log());
    assert!(gone_within(services[0], Duration::from_secs(2)));
    // The terminal's signal did not reach billing: the app told it to stop, after.
    assert!(!s.log().contains("[billing] quillo: stopping when the app stops"), "{}", s.log());
}

#[test]
fn split_services_take_the_apps_drain_and_a_second_stop_ends_them_too() {
    let d = dir("split-drain", &[("main.qwe", SPLIT_APP), ("billing.qwe", BILLING)]);
    // Billing's call cut off at the app's time: billing has it too.
    let mut s = Run::start(&d, "main.qwe", &["--split", "--drain", "1s"]);
    let services = s.children();
    let args = Command::new("ps").args(["-o", "args=", "-p", &services[0].to_string()]).output().unwrap();
    assert!(String::from_utf8_lossy(&args.stdout).contains("--drain 1000ms"), "{}", String::from_utf8_lossy(&args.stdout));
    assert!(asked(s.port, "/start").join().unwrap().ends_with("gave up"));
    s.signal(libc::SIGTERM);
    let (code, took) = s.ends(Duration::from_secs(10));
    assert_eq!(code, Some(0), "{}", s.log());
    assert!(took < Duration::from_secs(4), "{:?}\n{}", took, s.log());
    assert!(gone_within(services[0], Duration::from_secs(2)));
    assert_eq!(rows(&d, "billing.db", "select n from done"), "[]");
    // A second stop while the app waits for billing to finish its call: billing ends with it,
    // at once, its call cut off.
    let _ = std::fs::remove_file(d.join("billing.db"));
    let mut s = Run::start(&d, "main.qwe", &["--split"]);
    let services = s.children();
    assert!(asked(s.port, "/start").join().unwrap().ends_with("gave up"));
    s.signal(libc::SIGTERM);
    s.says("(again to stop now)", Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(200));
    s.signal(libc::SIGTERM);
    assert_eq!(s.ends(Duration::from_secs(3)).0, Some(130), "{}", s.log());
    assert!(gone_within(services[0], Duration::from_millis(500)), "billing was left running");
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(rows(&d, "billing.db", "select n from done"), "[]");
}

// ---- `quillo check`: an `idempotent` that changes data in another program -------------------
//
// Lost answers between services are what a program of services lives with: the one that runs
// again is not re-designed, but where it matters, under `idempotent`, the check says so (the
// user's decision, 2026-11-09; branch feedback56).

const IDEMPOTENT_APP: &str = r#"db.open("app.db")
service billing = "billing.qwe"
site shop = "https://shop.example.com"
fn pay(ref: str) = billing.charge(ref, 100)
fn paid() = billing.charge("h", 1)
route POST /a = idempotent(() => billing.charge("a", 1))
route POST /b = idempotent(() => pay("b"))
route POST /c = idempotent(() => billing.total())
route POST /d = idempotent({ transaction: false }, () => billing.charge("d", 1))
route POST /e = idempotent(() => {
    billing.charge("e", 1)
    billing.refund("e")
})
route POST /f = idempotent(() => shop.order("f"))
route POST /g = idempotent(() => billing.later.charge("g", 1))
route POST /h = idempotent(paid)
route POST /i = idempotent({ max_age: 60 }, () => billing.relay())
route POST /j = idempotent({ transaction: true }, () => billing.charge("j", 1))
route POST /o = order()
fn order() = idempotent(() => billing.charge("o", 1))
"#;

const IDEMPOTENT_BILLING: &str = r#"db.open("billing.db")
service tax = "tax.qwe"
db.exec("create table if not exists charges (ref text, cents int)")
server fn charge(ref: str, cents: int) -> int {
    db.exec("insert into charges values (?, ?)", ref, cents)
    cents
}
server fn refund(ref: str) -> int {
    db.exec("delete from charges where ref = ?", ref)
    0
}
server fn total() -> int = db.one("select count(*) as n from charges").n
server fn relay() -> int = tax.go()
"#;

const IDEMPOTENT_TAX: &str = r#"db.open("tax.db")
db.exec("create table if not exists due (n int)")
server fn go() -> int {
    db.exec("insert into due values (1)")
    1
}
"#;

#[test]
fn check_warns_where_idempotent_changes_data_in_another_program() {
    let d = dir("idempotent-check", &[("main.qwe", IDEMPOTENT_APP), ("billing.qwe", IDEMPOTENT_BILLING), ("tax.qwe", IDEMPOTENT_TAX)]);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["check", "main.qwe"]).current_dir(&d).output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    let said: Vec<(u32, String)> = err
        .lines()
        .filter_map(|l| {
            let (at, rest) = l.split_once(": warning: `idempotent` reaches `")?;
            Some((at.rsplit(':').next()?.parse().ok()?, rest.split('`').next()?.to_string()))
        })
        .collect();
    // Directly; through the program's function; two (the first named, the other counted); a
    // site's; a function named, not written there; a service's that changes data in another
    // service; `transaction: true` written out. Not: a function that only reads, one run without
    // the transaction, and one made later, which waits in the transaction's own database.
    let want: Vec<(u32, String)> = [(6, "billing.charge"), (7, "billing.charge"), (10, "billing.charge"), (14, "shop.order"), (16, "billing.charge"), (17, "billing.relay"), (18, "billing.charge"), (20, "billing.charge")]
        .iter()
        .map(|(l, c)| (*l, c.to_string()))
        .collect();
    assert_eq!(said, want, "{}", err);
    let line = err.lines().find(|l| l.contains("main.qwe:10: warning:")).unwrap();
    assert!(line.contains("`idempotent` reaches `billing.charge`, which may change data in another program, outside its transaction (and 1 more such call): when that call's answer is lost, the key is given up and a retry with it runs the call there again, under a new call id. Give `idempotent` `{ transaction: false }`, which keeps every answer, failures too, or send an id the other program's change is keyed by, so that a second run changes nothing"), "{}", line);
    // A warning: the program still starts.
    assert!(o.status.success(), "{}", err);
    // In another file, said once, at its own line: walked again for its type where it is called,
    // it had been said a second time at the calling file's name (found in review).
    let d = dir("idempotent-check-use", &[
        ("lib.qwe", "site shop = \"https://shop.example.com\"\n\n\npublic fn buy() = idempotent(() => shop.order(\"x\"))\n"),
        ("main.qwe", "db.open(\"app.db\")\nuse lib.qwe\nroute POST /k = buy()\n"),
    ]);
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["check", "main.qwe"]).current_dir(&d).output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    let said: Vec<&str> = err.lines().filter(|l| l.contains("warning: `idempotent` reaches `shop.order`")).map(|l| l.split(": warning").next().unwrap_or("")).collect();
    assert_eq!(said, ["./lib.qwe:4"], "{}", err);
}
