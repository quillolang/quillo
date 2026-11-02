//! What a second 100 plugins found (FEEDBACK27.md, Q-326 to Q-333; branch feedback27-35-fixes):
//! an error no statement takes is logged and rolls a transaction back, a plugin's function
//! named like a builtin is warned of, numbers and depth at a plugin's edge are said plainly,
//! `file()` refuses options it does not know, and an error handed to a builtin is named.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};
use support::send;

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback27-{}", name));
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

fn quillo() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.env("QUILLO_NATIVE_CACHE", PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-cache"));
    c
}

fn run(d: &Path) -> Output {
    quillo().arg("run").arg(d.join("main.qwe")).arg("--no-check").stdin(Stdio::null()).output().unwrap()
}

fn check(d: &Path) -> String {
    let o = quillo().arg("check").arg(d.join("main.qwe")).output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

/// A server whose log goes to a file, read with `log()`.
struct Logged {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Logged {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Logged {
    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// Waits for `text` in the log.
    fn wait_for(&self, text: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let l = self.log();
            if l.contains(text) || Instant::now() > deadline {
                return l;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn serve(d: &Path, wait: u64) -> Logged {
    let port = support::free_port();
    let log = d.join("server.log");
    let child = quillo()
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", &port.to_string(), "--no-check"])
        .env("QUILLO_SECRET", "test secret for the tests")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let mut s = Logged { child, port, log };
    let deadline = Instant::now() + Duration::from_secs(wait);
    while !support::listening(port) {
        if let Ok(Some(st)) = s.child.try_wait() {
            panic!("stopped: {}\n{}", st, s.log());
        }
        assert!(Instant::now() < deadline, "did not start:\n{}", s.log());
        std::thread::sleep(Duration::from_millis(30));
    }
    s
}

// ---- Q-332: an error no statement takes -------------------------------------------------------

const ACCOUNTS: &str = r#"db.open("app.db")
db.exec("create table if not exists accounts (id integer primary key, balance int)")
db.exec("delete from accounts")
db.exec("insert into accounts values (1, 100), (2, 0)")
fn half(n) {
    if n % 2 != 0 { return err("odd") }
    n / 2
}
route GET /fn {
    half(3)
    { ok: true }
}
route GET /taken {
    let _ = half(3)
    half(5) ?? 0
    { ok: true }
}
route GET /move {
    db.transaction(() => {
        db.exec("update accounts set balance = balance - 40 where id = 1")
        db.exec("update no_such_table set balance = balance + 40 where id = 2")
        { moved: 40 }
    })
    { ok: true }
}
route GET /inner {
    let r = db.transaction(() => {
        db.exec("update accounts set balance = balance - 10 where id = 1")
        db.transaction(() => {
            db.exec("update accounts set balance = balance + 10 where id = 2")
            db.exec("insert into no_such_table values (1)")
        })
        { moved: 10 }
    })
    { failed: is_err(r) }
}
route GET /branch ? { n: int } {
    if n > 0 { half(n) }
    for i in 0..2 { half(1) }
    { ok: true }
}
route GET /fine {
    let r = db.transaction(() => {
        db.exec("update accounts set balance = balance - 1 where id = 1")
        { moved: 1 }
    })
    r
}
route GET /balances = db.query("select balance from accounts order by id") | map((r) => r.balance)
"#;

#[test]
fn a_dropped_error_is_logged_with_its_line_and_the_request() {
    let d = dir("dropped", &[("main.qwe", ACCOUNTS)]);
    let s = serve(&d, 20);
    assert_eq!(send(s.port, "GET", "/fn", &[], "").body, r#"{"ok":true}"#);
    let log = s.wait_for("no statement took");
    assert!(log.contains("main.qwe:10: an error no statement took, made at ") && log.contains("main.qwe:6: odd"), "{}", log);
    assert!(log.contains("GET /fn (rid "), "{}", log);
}

#[test]
fn a_taken_error_is_not_logged() {
    let d = dir("taken", &[("main.qwe", ACCOUNTS)]);
    let s = serve(&d, 20);
    assert_eq!(send(s.port, "GET", "/taken", &[], "").body, r#"{"ok":true}"#);
    assert_eq!(send(s.port, "GET", "/branch?n=0", &[], "").status, 200);
    std::thread::sleep(Duration::from_millis(300));
    // `for i in 0..2 { half(1) }` drops two.
    assert_eq!(s.log().matches("no statement took").count(), 2, "{}", s.log());
    assert_eq!(send(s.port, "GET", "/branch?n=3", &[], "").status, 200);
    let log = s.wait_for("main.qwe:38");
    assert!(log.contains("main.qwe:38: an error no statement took"), "{}", log);
}

#[test]
fn a_transaction_does_not_commit_past_an_error_no_one_took() {
    let d = dir("txn", &[("main.qwe", ACCOUNTS)]);
    let s = serve(&d, 20);
    assert_eq!(send(s.port, "GET", "/move", &[], "").body, r#"{"ok":true}"#);
    assert_eq!(send(s.port, "GET", "/balances", &[], "").body, "[100,0]");
    // The transaction's own value, dropped, is the line logged: once, not once in it too.
    let log = s.wait_for("no statement took");
    assert_eq!(log.matches("no statement took").count(), 1, "{}", log);
    assert!(log.contains("no_such_table"), "{}", log);
    // A nested one rolls back, and the outer one, which dropped it, rolls back too.
    assert_eq!(send(s.port, "GET", "/inner", &[], "").body, r#"{"failed":true}"#);
    assert_eq!(send(s.port, "GET", "/balances", &[], "").body, "[100,0]");
    // One that fails nowhere commits as before.
    assert_eq!(send(s.port, "GET", "/fine", &[], "").body, r#"{"moved":1}"#);
    assert_eq!(send(s.port, "GET", "/balances", &[], "").body, "[99,0]");
}

#[test]
fn a_script_says_what_it_dropped_and_goes_on() {
    let d = dir("script", &[("main.qwe", "write_file(\"../outside.txt\", \"x\")\nprint(\"after\")\nlet _ = write_file(\"../outside.txt\", \"x\")\n")]);
    let o = run(&d);
    assert_eq!(String::from_utf8_lossy(&o.stdout), "after\n");
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(err.matches("no statement took").count(), 1, "{}", err);
    assert!(err.contains("main.qwe:1: an error no statement took: write_file: path must stay inside"), "{}", err);
    assert!(o.status.success());
}

#[test]
fn check_warns_of_a_dropped_transaction_and_a_dropped_failure() {
    let d = dir(
        "check",
        &[(
            "main.qwe",
            "db.open(\"app.db\")\nfn half(n: int) {\n    if n % 2 != 0 { return err(\"odd\") }\n    n / 2\n}\nroute GET / {\n    db.transaction(() => { db.exec(\"select 1\") })\n    half(3)\n    let _ = half(3)\n    let r = db.transaction(() => 1)\n    db.exec(\"select 1\")\n    r\n}\n",
        )],
    );
    let out = check(&d);
    assert!(out.contains("main.qwe:7: warning: the transaction's value is dropped"), "{}", out);
    assert!(out.contains("main.qwe:8: warning: `half` can give an error, and it is dropped here"), "{}", out);
    assert!(!out.contains("main.qwe:11:"), "a builtin's is logged when it happens: {}", out);
    assert_eq!(out.matches(": warning:").count(), 2, "{}", out);
}

// ---- Q-331: file()'s options -----------------------------------------------------------------

#[test]
fn file_refuses_options_it_does_not_know_and_takes_a_download_name() {
    let d = dir(
        "file",
        &[
            ("main.qwe", "route GET /bad = file(\"p.log\", { headers: { \"content-encoding\": \"gzip\" } })\nroute GET /cash = file(\"p.log\", { cash: \"1y\" })\nroute GET /dl = file(\"p.log\", { download: \"résumé \\\"final\\\".pdf\", type: \"application/pdf\" })\nroute GET /none = file(\"p.log\", { download: null, cache: null })\nroute GET /wrong = file(\"p.log\", { download: 3 })\n"),
            ("p.log", "a log\n"),
        ],
    );
    let s = serve(&d, 20);
    assert_eq!(send(s.port, "GET", "/bad", &[], "").status, 500);
    assert_eq!(send(s.port, "GET", "/cash", &[], "").status, 500);
    let log = s.wait_for("`cash`");
    assert!(log.contains("`file` has no option `headers`: it takes `cache`, `download` and `type`"), "{}", log);
    assert!(log.contains("`file` has no option `cash`"), "{}", log);
    let a = send(s.port, "GET", "/dl", &[], "");
    assert_eq!(a.body, "a log\n");
    assert_eq!(a.header("content-type"), Some("application/pdf"));
    assert_eq!(a.header("content-disposition"), Some("attachment; filename=\"r_sum_ _final_.pdf\"; filename*=UTF-8''r%C3%A9sum%C3%A9%20%22final%22.pdf"));
    let n = send(s.port, "GET", "/none", &[], "");
    assert_eq!((n.status, n.header("content-disposition")), (200, None));
    assert_eq!(send(s.port, "GET", "/wrong", &[], "").status, 500);
}

// ---- Q-333: an error given to a builtin ------------------------------------------------------

#[test]
fn a_builtin_given_an_error_says_what_it_was() {
    let d = dir("given", &[("main.qwe", "let n = int(read_file(\"missing.txt\"))\nprint(n.error)\nlet DATA = json.parse(read_file(\"does-not-exist.json\"))\n")]);
    let o = run(&d);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("cannot convert error to int (it failed before this call, ") && out.contains("main.qwe:1: read_file: missing.txt:"), "{}", out);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("`json.parse` expects a str as argument 1, got error (it failed before this call, ") && err.contains("main.qwe:3: read_file: does-not-exist.json:"), "{}", err);
    assert!(!o.status.success());
    // A builtin that takes errors takes them as before, and one given no error says what it said.
    let d = dir("given-fine", &[("main.qwe", "print(is_err(int(read_file(\"x\"))), type_of(err(\"e\")), json.parse(3) ?? \"no\")\n")]);
    let o = run(&d);
    assert_eq!(String::from_utf8_lossy(&o.stdout), "true error no\n", "{}", String::from_utf8_lossy(&o.stderr));
}

// ---- Q-326 to Q-330: a plugin's edges --------------------------------------------------------

const PLUGIN: &str = r#"//! Edges.
//! version: 1.0.0
//!
//! [dependencies]
//! serde_json = "1"

use serde_json::json;

/// Named like a builtin.
pub fn len(s: &str) -> i64 {
    s.len() as i64 * 100
}

/// A u64 past i64, inside a value.
pub fn big() -> serde_json::Value {
    json!({ "id": 18446744073709551615u64, "list": [1, 2] })
}

/// One deep in a list.
pub fn big_in_list() -> serde_json::Value {
    json!([1, { "n": 9223372036854775808u64 }])
}

/// The largest that fits.
pub fn fits() -> serde_json::Value {
    json!({ "id": 9223372036854775807u64 })
}

/// Takes a small int.
pub fn small(n: i32) -> i64 {
    n as i64
}

/// Takes any value.
pub fn deep(v: serde_json::Value) -> i64 {
    v.to_string().len() as i64
}

/// A value nested n deep.
pub fn pr_deep(n: i64) -> serde_json::Value {
    let mut v = json!(1);
    for _ in 0..n {
        v = json!([v]);
    }
    v
}

/// Sleeps.
pub fn pr_slow(ms: i64) -> i64 {
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    ms
}
"#;

const PLUGIN_USER: &str = r#"import edges.qwer use len as plugin_len, big, big_in_list, fits, small, deep, pr_deep, pr_slow
route GET /len = { v: plugin_len("abc"), builtin: len("abc") }
route GET /big = { v: big() }
route GET /big_in_list = { v: big_in_list() }
route GET /fits = { v: str(fits().id) }
route GET /small ? { n: int } = { v: small(n) }
route GET /deep ? { n: int } = { v: deep(reduce(range(0, n), 1, (acc, i) => { a: acc })) }
route GET /pr_deep ? { n: int } = { ok: type_of(pr_deep(n)) }
route GET /slow ? { ms: int } = { s: pr_slow(ms) }
"#;

#[test]
fn a_plugins_function_named_like_a_builtin_is_warned_of_at_its_line() {
    let d = dir("hides", &[("main.qwe", "use edges.qwer\nroute GET / = len(\"abc\")\n"), ("edges.qwer", PLUGIN)]);
    let out = check(&d);
    assert!(out.contains("edges.qwer:10: warning: `len` hides the builtin `len`"), "{}", out);
    assert!(out.contains("import edges.qwer use len as edges_len"), "{}", out);
    // Taken under another name, it hides nothing.
    let d = dir("hides-not", &[("main.qwe", PLUGIN_USER), ("edges.qwer", PLUGIN)]);
    assert!(!check(&d).contains("hides the builtin"), "{}", check(&d));
}

#[test]
fn numbers_and_depth_at_a_plugins_edge_are_said_plainly() {
    if Command::new("cargo").arg("--version").output().is_err() {
        return;
    }
    let d = dir("plugin", &[("main.qwe", PLUGIN_USER), ("edges.qwer", PLUGIN)]);
    let s = serve(&d, 600);
    assert_eq!(send(s.port, "GET", "/len", &[], "").body, r#"{"v":300,"builtin":3}"#);
    // Q-327: past i64 inside a value is refused, by where it is; the largest that fits is exact.
    assert_eq!(send(s.port, "GET", "/big", &[], "").status, 500);
    assert_eq!(send(s.port, "GET", "/big_in_list", &[], "").status, 500);
    assert_eq!(send(s.port, "GET", "/fits", &[], "").body, r#"{"v":"9223372036854775807"}"#);
    // Q-328: an int that does not fit, and an argument too deep, by the argument's name.
    assert_eq!(send(s.port, "GET", "/small?n=5000000000", &[], "").status, 500);
    assert_eq!(send(s.port, "GET", "/small?n=-7", &[], "").body, r#"{"v":-7}"#);
    assert_eq!(send(s.port, "GET", "/deep?n=100", &[], "").status, 200);
    assert_eq!(send(s.port, "GET", "/deep?n=200", &[], "").status, 500);
    // Q-329: an answer too deep is no panic; one just short of it is a value.
    assert_eq!(send(s.port, "GET", "/pr_deep?n=120", &[], "").body, r#"{"ok":"list"}"#);
    assert_eq!(send(s.port, "GET", "/pr_deep?n=200", &[], "").status, 500);
    let log = s.wait_for("nested too deeply: Quillo");
    assert!(log.contains("`big` (edges.qwer) gave back 18446744073709551615 at .id, which is too large for Quillo's int"), "{}", log);
    assert!(log.contains("gave back 9223372036854775808 at [1].n"), "{}", log);
    assert!(log.contains("`small`: argument `n` does not fit in i32: 5000000000"), "{}", log);
    assert!(log.contains("`deep`: argument `v` is nested too deeply"), "{}", log);
    assert!(log.contains("`pr_deep` (edges.qwer) gave back a value nested too deeply"), "{}", log);
    assert!(!log.contains("panicked"), "{}", log);
}

#[cfg(unix)]
#[test]
fn a_stop_during_a_plugins_call_is_not_called_a_bug() {
    if Command::new("cargo").arg("--version").output().is_err() {
        return;
    }
    let d = dir("slow", &[("main.qwe", PLUGIN_USER), ("edges.qwer", PLUGIN)]);
    let s = serve(&d, 600);
    let port = s.port;
    let asked = std::thread::spawn(move || send(port, "GET", "/slow?ms=14000", &[], ""));
    std::thread::sleep(Duration::from_millis(1500));
    unsafe { libc_kill(s.child.id() as i32, 15) };
    let _ = asked.join();
    let log = s.wait_for("still running");
    assert!(log.contains("quillo: stopped with GET /slow?ms=14000 still running"), "{}", log);
    assert!(!log.contains("a bug in Quillo"), "{}", log);
}

#[cfg(unix)]
extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}
