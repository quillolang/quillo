//! FEEDBACK40.md (Q-471 to Q-474): what an app needs of the services it calls, under `--split`
//! and when a service runs elsewhere.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use support::{free_port, listening, send, temp_dir};

const SECRET: &str = "a-test-secret-for-feedback40-of-32-bytes";

const MAIN: &str = "service billing = \"billing.qwe\"\nroute GET / = str(billing.add(1, 2))\nroute GET /g = billing.greet(\"Ana\")\n";

/// A `quillo` process a test started, stopped with Ctrl-C (so `--split` stops its services) when
/// dropped; its output in `log`.
struct Run {
    child: Child,
    log: PathBuf,
}

impl Run {
    fn output(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = Command::new("kill").args(["-INT", &self.child.id().to_string()]).status();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn quillo(dir: &Path, args: &[&str], log: &str) -> Run {
    quillo_with(dir, args, log, SECRET)
}

fn quillo_with(dir: &Path, args: &[&str], log: &str, secret: &str) -> Run {
    let log = dir.join(log);
    let file = std::fs::File::create(&log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(dir)
        .env("QUILLO_SERVICE_SECRET", secret)
        .stdin(Stdio::null())
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .spawn()
        .expect("cannot start quillo");
    support::stop_with_tests(&child);
    Run { child, log }
}

fn up(run: &mut Run, port: u16) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !listening(port) {
        if let Ok(Some(status)) = run.child.try_wait() {
            panic!("quillo stopped ({}): {}", status, run.output());
        }
        assert!(Instant::now() < deadline, "did not start: {}", run.output());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Runs quillo until it ends, for at most `secs`: its exit code (None when it had to be stopped)
/// and what it said.
fn to_the_end(dir: &Path, args: &[&str], secs: u64) -> (Option<i32>, String) {
    to_the_end_with(dir, args, secs, SECRET)
}

fn to_the_end_with(dir: &Path, args: &[&str], secs: u64, secret: &str) -> (Option<i32>, String) {
    let mut run = quillo_with(dir, args, "end.log", secret);
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Ok(Some(status)) = run.child.try_wait() {
            return (status.code(), run.output());
        }
        if Instant::now() > deadline {
            return (None, run.output());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn eventually(secs: u64, what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn names(path: &Path) -> Vec<String> {
    quillo::service::offered(path).unwrap().iter().map(|d| format!("{}/{}", d.name, d.params.len())).collect()
}

// ---- Q-471: a service is read for its headers; one not here is asked ---------------------------

#[test]
fn a_service_that_does_not_parse_is_read_for_its_server_fn_headers() {
    let d = temp_dir("f40-headers");
    std::fs::write(
        d.join("billing.qwe"),
        r#"use "tax.qwe"
server fn add(a: int, b: int) -> int = a + b
server fn invoice(
    customer: str,
    cents: int,
    note: str = "none",
) -> map ! str {
    { customer, cents, note }
}
server fn sizes(xs: list<map>) -> list<int> = xs | map((x) => len(x))
server fn same(a: int, b: int) -> bool = a == b
fn helper() = 1
route GET /report = {
"#,
    )
    .unwrap();
    std::fs::write(d.join("tax.qwe"), "server fn rate(country: str) -> dec = 0.21\nroute GET /x = {\n").unwrap();
    // Each header, its own and its `use`d file's; not the plain fn, nor the broken route.
    assert_eq!(names(&d.join("billing.qwe")), ["rate/1", "add/2", "invoice/3", "sizes/1", "same/2"]);
    let defs = quillo::service::offered(&d.join("billing.qwe")).unwrap();
    let invoice = defs.iter().find(|f| f.name == "invoice").unwrap();
    assert!(invoice.params[2].default.is_some(), "a default is kept as a default");
    assert!(invoice.ret.is_some() && invoice.err.is_some(), "and the return and error types");
    // `public`, and a shape returned, whose `{` is not a body's.
    std::fs::write(
        d.join("shapes.qwe"),
        "public server fn pub1(a: int) -> int = a\nserver fn shape() -> { title: str } = { title: \"x\" }\nserver fn shape2(a: int) -> { title: str } {\n  { title: \"y\" }\n}\nserver fn cmp(a: int, b: int) -> bool = a >= b\nroute GET /x = {\n",
    )
    .unwrap();
    assert_eq!(names(&d.join("shapes.qwe")), ["pub1/1", "shape/0", "shape2/1", "cmp/2"]);
    // A file with no header at all still says why it does not parse.
    std::fs::write(d.join("broken.qwe"), "route GET / = {\n").unwrap();
    let e = quillo::service::offered(&d.join("broken.qwe")).unwrap_err();
    assert!(e.contains("broken.qwe") && e.contains("missing `}`"), "{}", e);
}

#[test]
fn an_app_without_the_services_file_asks_the_running_service_for_its_functions() {
    let svc = temp_dir("f40-svc");
    let app = temp_dir("f40-app");
    std::fs::write(svc.join("billing.qwe"), "server fn add(a: int, b: int, c: int = 0) -> int = a + b + c\nserver fn greet(name: str) = \"hello \" + name\n").unwrap();
    std::fs::write(app.join("main.qwe"), MAIN).unwrap();
    let sport = free_port();
    let mut billing = quillo(&svc, &["run", "billing.qwe", "--port", &sport.to_string(), "--as-service"], "billing.log");
    up(&mut billing, sport);
    let url = format!("billing=http://127.0.0.1:{}", sport);

    // Its list is signed: unsigned, or for another, it is refused.
    assert_eq!(send(sport, "POST", "/_quillo/service/_contracts", &[], "").status, 401);

    // The check says the file is not here and does not stop on it, nor on its calls.
    let check = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["check", "main.qwe"]).current_dir(&app).output().unwrap();
    let said = String::from_utf8_lossy(&check.stderr).into_owned() + &String::from_utf8_lossy(&check.stdout);
    assert!(check.status.success(), "{}", said);
    assert!(said.contains("warning: service billing: billing.qwe is not here, so its calls are not checked"), "{}", said);

    // Checked and run, the app asks billing what it offers, and calls it.
    let port = free_port();
    let mut main = quillo(&app, &["run", "main.qwe", "--port", &port.to_string(), "--service-url", &url], "main.log");
    up(&mut main, port);
    assert_eq!(send(port, "GET", "/", &[], "").body, "3");
    assert_eq!(send(port, "GET", "/g", &[], "").body, "hello Ana");
    drop(main);

    // Its addresses in turn: one that is down is gone past, as for any call.
    let two = format!("billing=http://127.0.0.1:{},http://127.0.0.1:{}", free_port(), sport);
    let port = free_port();
    let mut main = quillo(&app, &["run", "main.qwe", "--port", &port.to_string(), "--service-url", &two], "two.log");
    up(&mut main, port);
    assert_eq!(send(port, "GET", "/", &[], "").body, "3");
    drop(main);

    // Asked of a service that is not running: it says so, and why.
    let gone = format!("billing=http://127.0.0.1:{}", free_port());
    let (code, said) = to_the_end(&app, &["run", "main.qwe", "--port", &free_port().to_string(), "--no-check", "--service-url", &gone], 30);
    assert_eq!(code, Some(1), "{}", said);
    assert!(said.contains("billing.qwe is not here, and service billing did not give its functions"), "{}", said);

    // With another secret, the service does not give them.
    let other = "not-the-secret-but-as-long-as-the-secret";
    let (code, said) = to_the_end_with(&app, &["run", "main.qwe", "--port", &free_port().to_string(), "--no-check", "--service-url", &url], 30, other);
    assert_eq!(code, Some(1), "{}", said);
    assert!(said.contains("did not give its functions"), "{}", said);
}

// ---- Q-473: the tree is checked once, before anything starts ----------------------------------

#[test]
fn split_checks_the_whole_tree_once_and_says_each_warning_once() {
    let d = temp_dir("f40-once");
    std::fs::write(d.join("main.qwe"), "service billing = \"billing.qwe\"\nroute GET / = str(billing.total(2))\n").unwrap();
    std::fs::write(d.join("billing.qwe"), "service tax = \"tax.qwe\"\nserver fn total(n: int) -> dec = tax.rate(\"es\") * n\n").unwrap();
    std::fs::write(d.join("tax.qwe"), "server fn rate(country: str) -> dec {\n    let unused = 1\n    0.21m\n}\n").unwrap();
    let port = free_port();
    let mut run = quillo(&d, &["run", "main.qwe", "--port", &port.to_string(), "--split"], "once.log");
    up(&mut run, port);
    eventually(20, "the app did not answer", || send(port, "GET", "/", &[], "").body == "0.42");
    let out = run.output();
    assert_eq!(out.matches("unused variable `unused`").count(), 1, "{}", out);
}

#[test]
fn split_stops_at_once_on_an_error_in_a_service() {
    let d = temp_dir("f40-error");
    std::fs::write(d.join("main.qwe"), MAIN).unwrap();
    std::fs::write(d.join("billing.qwe"), "server fn add(a: int, b: int) -> int = a + b\nserver fn greet(name: str) = no_such_function(name)\n").unwrap();
    let t = Instant::now();
    let (code, said) = to_the_end(&d, &["run", "main.qwe", "--port", &free_port().to_string(), "--split"], 30);
    assert_eq!(code, Some(1), "{}", said);
    assert!(said.contains("unknown name `no_such_function`"), "{}", said);
    assert!(!said.contains("quillo: service billing (billing.qwe) on"), "found before any service started: {}", said);
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
}

// ---- Q-474: a service that ends at start is not waited for ------------------------------------

#[test]
fn a_service_that_ends_at_start_is_said_at_once() {
    let d = temp_dir("f40-ended");
    std::fs::write(d.join("main.qwe"), MAIN).unwrap();
    std::fs::write(d.join("billing.qwe"), "let rows = no_such_function(1)\nserver fn add(a: int, b: int) -> int = a + b\n").unwrap();
    let t = Instant::now();
    let (code, said) = to_the_end(&d, &["run", "main.qwe", "--port", &free_port().to_string(), "--split", "--no-check"], 30);
    assert_eq!(code, Some(1), "{}", said);
    assert!(said.contains("service billing stopped at start ("), "{}", said);
    assert!(t.elapsed() < Duration::from_secs(10), "waited {:?}: {}", t.elapsed(), said);
}

// ---- Q-472: under --split --watch, a change to a service reaches the app -----------------------

fn a_function_added_to_a_service_reaches_the_app(flags: &[&str], name: &str) {
    let d = temp_dir(name);
    std::fs::write(d.join("main.qwe"), MAIN).unwrap();
    std::fs::write(d.join("billing.qwe"), "server fn add(a: int, b: int) -> int = a + b\nserver fn greet(name: str) = \"hi \" + name\n").unwrap();
    let port = free_port();
    let p = port.to_string();
    let mut args = vec!["run", "main.qwe", "--port", &p, "--split", "--watch"];
    args.extend(flags);
    let mut run = quillo(&d, &args, "watch.log");
    up(&mut run, port);
    eventually(20, "the app did not answer", || send(port, "GET", "/g", &[], "").body == "hi Ana");
    // A parameter more and a function more, in one edit.
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(
        d.join("billing.qwe"),
        "server fn add(a: int, b: int, c: int = 10) -> int = a + b + c\nserver fn greet(name: str) = \"hello \" + name\nserver fn more() = 1\n",
    )
    .unwrap();
    eventually(20, "the app did not take billing's change", || send(port, "GET", "/g", &[], "").body == "hello Ana" && send(port, "GET", "/", &[], "").body == "13");
    let out = run.output();
    assert!(out.contains("reloaded main.qwe"), "the app reloaded too: {}", out);
}

#[test]
fn under_split_watch_a_change_to_a_service_reaches_the_app() {
    a_function_added_to_a_service_reaches_the_app(&[], "f40-watch");
}

#[test]
fn under_split_watch_unchecked_a_change_to_a_service_reaches_the_app() {
    a_function_added_to_a_service_reaches_the_app(&["--no-check"], "f40-watch-unchecked");
}
