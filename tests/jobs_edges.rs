//! Jobs and ticks at their edges (src/jobs.rs): what `schedule` and `every`
//! refuse, cron lines, and the scheduler's threads against a server that is
//! slow, away, or running a program that changes.

mod support;

use quillo::jobs::{self, Cron, When};
use quillo::Interp;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-jobs-edges-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn fails(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => e.to_string(),
    }
}

fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A server that keeps the request lines it gets; while `hold` is set, it does
/// not answer them.
struct Fake {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<String>>>,
    hold: Arc<AtomicBool>,
}

fn fake(l: TcpListener) -> Fake {
    let addr = l.local_addr().unwrap();
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let hold: Arc<AtomicBool> = Default::default();
    let (s2, h2) = (seen.clone(), hold.clone());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let (seen, hold) = (s2.clone(), h2.clone());
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf).into_owned();
                seen.lock().unwrap().push(head);
                while hold.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nok\n");
            });
        }
    });
    Fake { addr, seen, hold }
}

impl Fake {
    fn count(&self, what: &str) -> usize {
        self.seen.lock().unwrap().iter().filter(|h| h.contains(what)).count()
    }
}

#[test]
fn a_tick_is_named_after_its_function_or_where_it_is() {
    let mut it = Interp::new(dir("ticks"));
    it.load_source("fn step() = null\nevery(50, step)\nevery(1.5, () => 1)\nevery(20, step, { name: \"other\" })\nevery(30, () => 2, { name: null })\nevery(40, len, null)\n", "t.qwe").unwrap();
    let ticks: Vec<(String, u64)> = it.site.ticks.iter().map(|t| (t.name.clone(), t.ms)).collect();
    assert_eq!(
        ticks,
        vec![("step".into(), 50), ("every-3".into(), 1), ("other".into(), 20), ("every-5".into(), 30), ("every-6".into(), 40)]
    );
}

#[test]
fn every_says_what_it_takes() {
    let f = "fn step() = null\n";
    assert!(fails("every(0, () => 1)").contains("`every` on the server takes at least 1 millisecond"));
    assert!(fails("every(0.5, () => 1)").contains("at least 1 millisecond"));
    assert!(fails("every()").contains("at least 1 millisecond"));
    assert!(fails("every(50, 1)").contains("`every` takes milliseconds and a function"));
    assert!(fails(&format!("{}every(50, step, {{ name: 1 }})", f)).contains("`every`'s name is a str, got int"));
    assert!(fails(&format!("{}every(50, step, 1)", f)).contains("`every` expects options as a map, got int"));
    assert!(fails(&format!("{}every(50, step, {{ name: \"no spaces\" }})", f)).contains("a tick's name is letters, digits"));
    assert!(fails(&format!("{}every(50, step)\nevery(60, step)", f)).contains("already a tick named \"step\""));
}

#[test]
fn schedule_says_what_it_takes() {
    let f = "fn run() = null\n";
    assert!(fails(&format!("{}schedule(\"* *\", run)", f)).contains("`schedule`: a cron line has 5 fields"));
    assert!(fails(&format!("{}schedule(\"soon\", run)", f)).contains("takes a duration such as \"5m\""));
    assert!(fails(&format!("{}schedule(0, run)", f)).contains("takes a duration such as"));
    assert!(fails(&format!("{}schedule(1.5, run)", f)).contains("takes a duration or a cron line first, got float"));
    assert!(fails("schedule()").contains("first, got nothing"));
    assert!(fails("schedule(\"5m\", 1)").contains("needs a function to run, got int"));
    assert!(fails("schedule(\"5m\")").contains("needs a function to run, got nothing"));
    assert!(fails(&format!("{}schedule(\"5m\", run, {{ name: 1 }})", f)).contains("`schedule`'s name is a str, got int"));
    assert!(fails(&format!("{}schedule(\"5m\", run, 1)", f)).contains("`schedule` expects options as a map, got int"));
    assert!(fails(&format!("{}schedule(\"5m\", run, {{ name: \"a b\" }})", f)).contains("a job's name is letters, digits"));
    assert!(fails(&format!("{}schedule(\"5m\", run)\nschedule(\"1m\", run)", f)).contains("already a job named \"run\""));

    let mut it = Interp::new(dir("schedule2"));
    it.load_source("fn run() = null\nschedule(60, run)\nschedule(\"@hourly\", () => 1)\nschedule(\"1h\", len, null)\nschedule(\"2h\", run, { name: \"b\" })\nschedule(\"3h\", () => 2, { name: null })\n", "s.qwe").unwrap();
    let jobs: Vec<(String, When)> = it.site.jobs.iter().map(|j| (j.name.clone(), j.when.clone())).collect();
    assert_eq!(jobs[0], ("run".to_string(), When::Every(60)));
    assert_eq!(jobs[1], ("job-3".to_string(), When::Cron(Cron::parse("0 * * * *").unwrap())));
    assert_eq!(jobs[2], ("job-4".to_string(), When::Every(3600)));
    assert_eq!(jobs[3], ("b".to_string(), When::Every(7200)));
    assert_eq!(jobs[4], ("job-6".to_string(), When::Every(10800)));
}

#[test]
fn cron_lines_match_their_times() {
    // 2026-10-24, a Saturday, 03:00 UTC.
    let sat_3am: u64 = 1_792_810_800;
    let m = |c: &str, t: u64| Cron::parse(c).unwrap().matches(t);
    assert!(m("0 3 * * *", sat_3am));
    assert!(!m("0 3 * * *", sat_3am + 60));
    assert!(!m("0 4 * * *", sat_3am));
    assert!(!m("0 3 * 11 *", sat_3am));
    assert!(m("*/15 * * * *", sat_3am + 900));
    assert!(!m("*/15 * * * *", sat_3am + 600));
    assert!(m("0-30/10 * * * *", sat_3am + 600));
    assert!(m("10/5 * * * *", sat_3am + 900) && !m("10/5 * * * *", sat_3am));
    assert!(m("1,2,0 3 * * *", sat_3am));
    assert!(m("0 3 * * 6", sat_3am) && !m("0 3 * * 1-5", sat_3am));
    assert!(m("0 3 24 10 *", sat_3am) && !m("0 3 25 10 *", sat_3am));
    // Both days given: either will do.
    assert!(m("0 3 1 * 6", sat_3am) && m("0 3 24 * 1", sat_3am) && !m("0 3 1 * 1", sat_3am));
    // 7 is Sunday too.
    assert!(m("0 0 * * 7", sat_3am + 21 * 3600) && m("0 0 * * 0", sat_3am + 21 * 3600));
    for line in ["@monthly", "@weekly", "@yearly", "@annually", "@midnight", "@daily", "@hourly"] {
        assert!(Cron::parse(line).is_ok(), "{}", line);
    }
    assert_eq!(Cron::parse("@midnight"), Cron::parse("@daily"));
    assert_eq!(jobs::slot(&When::Every(300), 1_000_123), Some(999_900));
    assert_eq!(jobs::slot(&When::Cron(Cron::parse("0 3 * * *").unwrap()), sat_3am + 30), Some(sat_3am));
    assert_eq!(jobs::slot(&When::Cron(Cron::parse("0 3 * * *").unwrap()), sat_3am + 90), None);
}

#[test]
fn a_cron_line_that_makes_no_sense_says_where() {
    let e = |c: &str| Cron::parse(c).unwrap_err();
    assert!(e("* * * *").contains("5 fields"));
    assert!(e("@often").contains("unknown \"@often\""));
    assert!(e("60 * * * *").contains("the minute field \"60\" is not a number from 0 to 59"));
    assert!(e("* 24 * * *").contains("the hour field"));
    assert!(e("* * 0 * *").contains("the day field"));
    assert!(e("* * * 13 *").contains("the month field"));
    assert!(e("* * * * 8").contains("the weekday field"));
    assert!(e("*/0 * * * *").contains("the minute field \"*/0\""));
    assert!(e("*/x * * * *").contains("the minute field"));
    assert!(e("a-5 * * * *").contains("the minute field \"a-5\""));
    assert!(e("1-b * * * *").contains("the minute field \"1-b\""));
    assert!(e("9-3 * * * *").contains("the minute field \"9-3\""));
    assert!(e("x * * * *").contains("the minute field \"x\""));
}

#[test]
fn a_job_is_asked_of_the_server_and_not_started_while_it_runs() {
    let f = fake(TcpListener::bind("127.0.0.1:0").unwrap());
    f.hold.store(true, Ordering::SeqCst);
    jobs::start(f.addr, || vec![("sweep".to_string(), When::Every(1)), ("rare".to_string(), When::Every(86400))]);
    let head = eventually(5, "the job was not asked for", || f.seen.lock().unwrap().first().cloned());
    assert!(head.starts_with("POST /_quillo/job/sweep?at="), "{}", head);
    assert!(head.contains(&format!("X-Quillo-Job: {}\r\n", jobs::token())), "{}", head);
    let at: u64 = head["POST /_quillo/job/sweep?at=".len()..].split(' ').next().unwrap().parse().unwrap();
    // It is due again, and again: it runs still.
    std::thread::sleep(Duration::from_millis(2300));
    assert_eq!(f.count("/job/sweep"), 1);
    f.hold.store(false, Ordering::SeqCst);
    eventually(5, "the job was not asked for again", || (f.count("/job/sweep") >= 2).then_some(()));
    let next = f.seen.lock().unwrap()[1].clone();
    let later: u64 = next["POST /_quillo/job/sweep?at=".len()..].split(' ').next().unwrap().parse().unwrap();
    assert!(later > at + 1, "{} after {}", later, at);
    assert_eq!(f.count("/job/rare"), 0);
}

#[test]
fn a_job_the_server_cannot_be_asked_for_is_asked_for_next_time() {
    let port = support::free_port();
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    jobs::start(addr, || vec![("late".to_string(), When::Every(1))]);
    // No one is there for its first time.
    std::thread::sleep(Duration::from_millis(2200));
    let f = fake(TcpListener::bind(addr).unwrap());
    eventually(5, "the job was not asked for", || (f.count("/job/late") >= 1).then_some(()));
}

#[test]
fn ticks_follow_the_list_and_stop_when_taken_off_it() {
    let f = fake(TcpListener::bind("127.0.0.1:0").unwrap());
    let list: Arc<Mutex<Vec<(String, u64)>>> = Arc::new(Mutex::new(vec![("step".to_string(), 30), ("other".to_string(), 60)]));
    let l2 = list.clone();
    jobs::start_ticks(f.addr, move || l2.lock().unwrap().clone());
    eventually(5, "the ticks did not run", || (f.count("/tick/step") >= 3 && f.count("/tick/other") >= 2).then_some(()));
    let head = f.seen.lock().unwrap().iter().find(|h| h.contains("/tick/step")).cloned().unwrap();
    let at: u64 = head["POST /_quillo/tick/step?at=".len()..].split(' ').next().unwrap().parse().unwrap();
    assert_eq!(at % 30, 0, "on the clock: {}", head);
    // Its interval changes: it starts again with the new one.
    *list.lock().unwrap() = vec![("step".to_string(), 700)];
    std::thread::sleep(Duration::from_millis(600));
    let (steps, others) = (f.count("/tick/step"), f.count("/tick/other"));
    eventually(5, "the new interval did not start", || (f.count("/tick/step") > steps).then_some(()));
    assert_eq!(f.count("/tick/other"), others, "a tick off the list stopped");
    let last = f.seen.lock().unwrap().iter().rev().find(|h| h.contains("/tick/step")).cloned().unwrap();
    let at: u64 = last["POST /_quillo/tick/step?at=".len()..].split(' ').next().unwrap().parse().unwrap();
    assert_eq!(at % 700, 0, "{}", last);
    // Gone: it stops within a tenth of a second or so, waiting or not.
    list.lock().unwrap().clear();
    std::thread::sleep(Duration::from_millis(600));
    let steps = f.count("/tick/step");
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(f.count("/tick/step"), steps);
}

#[test]
fn a_tick_goes_on_once_the_server_is_back() {
    let port = support::free_port();
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let on = Arc::new(AtomicBool::new(true));
    let o2 = on.clone();
    jobs::start_ticks(addr, move || if o2.load(Ordering::SeqCst) { vec![("away".to_string(), 10)] } else { Vec::new() });
    // It fails; and fails again, more slowly.
    std::thread::sleep(Duration::from_millis(500));
    let f = fake(TcpListener::bind(addr).unwrap());
    eventually(5, "the tick did not come back", || (f.count("/tick/away") >= 3).then_some(()));
    on.store(false, Ordering::SeqCst);
}

// ---- in a running program -----------------------------------------------------

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(file: &Path, port: u16, extra: &[&str]) -> Server {
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(extra)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("cannot start quillo");
    support::stop_with_tests(&child);
    let server = Server(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    server
}

#[test]
fn a_program_keeps_time_with_every_and_a_reload_changes_its_ticks() {
    let d = dir("program");
    let file = d.join("main.qwe");
    std::fs::write(&file, "let n = 0\nfn step() {\n n = n + 1\n}\nevery(20, step)\nroute GET /n = str(n)\n").unwrap();
    let port = support::free_port();
    let _s = start(&file, port, &["--watch"]);
    let n = |p: &str| support::send(port, "GET", p, &[], "").body.parse::<i64>().unwrap_or(-1);
    eventually(10, "the tick did not run", || (n("/n") >= 3).then_some(()));
    // A new version, without the tick: the count stays at the new version's.
    std::fs::write(&file, "let m = 0\nfn other() {\n m = m + 1\n}\nevery(25, other, { name: \"other\" })\nroute GET /m = str(m)\n").unwrap();
    eventually(15, "the new version's tick did not run", || (n("/m") >= 3).then_some(()));
}
