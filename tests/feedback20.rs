//! Database and tenants (FEEDBACK20.md; branch feedback20-fixes): what a visitor sees of a
//! database's error, a tenant that is not there, a signup that cannot land in a customer's
//! data, and a global that crosses tenants.

mod support;

use quillo::check::check_file;
use std::path::{Path, PathBuf};
use support::{send, serve};

fn app(name: &str, src: &str) -> (PathBuf, PathBuf) {
    let d = support::temp_dir(&format!("feedback20-{}", name));
    let f = d.join("main.qwe");
    std::fs::write(&f, src).unwrap();
    (d, f)
}

fn warnings(main: &Path) -> Vec<String> {
    check_file(main).diagnostics.iter().map(|d| d.message.clone()).collect()
}

// ---- Q-161: a database's error is for the log ------------------------------------------------

const USERS: &str = r#"
db.open(":memory:")
db.exec("create table if not exists users (id integer primary key, email text unique)")
"#;

#[test]
fn a_visitor_is_not_told_the_sql() {
    let src = format!(
        "{}{}",
        USERS,
        r#"
route GET /value = db.query("select nope from users")
route GET /try {
    let rows = db.query("select nope from users")?
    rows
}
route GET /nested = { rows: db.query("select nope from users"), ok: true }
route GET /list = [db.query("select nope from users")]
route GET /page = <p>{db.query("select nope from users")}</p>
route GET /unique {
    db.exec("insert into users (email) values ('a')")
    db.exec("insert into users (email) values ('a')")
}
route GET /reply {
    reply 500 db.query("select nope from users")
}
"#
    );
    let (_d, f) = app("hidden", &src);
    let s = serve(&f, &[]);
    for path in ["/value", "/try", "/unique", "/reply"] {
        let r = send(s.port, "GET", path, &[], "");
        assert_eq!((r.status, r.body.as_str()), (500, r#"{"error":"internal error"}"#), "{}", path);
    }
    for path in ["/nested", "/list", "/page"] {
        let r = send(s.port, "GET", path, &[], "");
        assert!(r.body.contains("internal error"), "{}: {}", path, r.body);
        assert!(!r.body.contains("nope") && !r.body.contains("no such column"), "{}: {}", path, r.body);
    }
}

#[test]
fn a_server_fn_does_not_hand_it_to_the_page() {
    let src = format!("{}{}", USERS, "server fn broken() = db.query(\"select nope from users\")\nroute GET / = \"up\"\n");
    let (_d, f) = app("rpc", &src);
    let s = serve(&f, &[]);
    let r = send(s.port, "POST", "/_quillo/rpc/broken", &[("content-type", "application/json"), ("X-Quillo-Live", "1")], "[]");
    assert!(r.body.contains("internal error") && !r.body.contains("nope"), "{} {}", r.status, r.body);
}

#[test]
fn what_the_program_wrote_is_for_the_visitor_and_the_program_still_reads_the_kind() {
    let src = format!(
        "{}{}",
        USERS,
        r#"
route GET /mine = err("That name is taken")
route GET /kind {
    db.exec("insert into users (email) values ('b')")
    let r = db.exec("insert into users (email) values ('b')")
    db.error_kind(r)
}
route GET /text {
    let r = db.query("select nope from users")
    is_err(r) ? "failed" : "worked"
}
"#
    );
    let (_d, f) = app("own", &src);
    let s = serve(&f, &[]);
    assert_eq!(send(s.port, "GET", "/mine", &[], "").body, r#"{"error":"That name is taken"}"#);
    assert_eq!(send(s.port, "GET", "/kind", &[], "").body, "unique");
    assert_eq!(send(s.port, "GET", "/text", &[], "").body, "failed");
}

#[test]
fn the_log_has_what_the_visitor_does_not() {
    use std::process::{Command, Stdio};
    let src = format!("{}{}", USERS, "route GET /value = db.query(\"select nope from users\")\n");
    let (d, f) = app("log", &src);
    let port = support::free_port();
    let log = d.join("log.txt");
    let out = std::fs::File::create(&log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", f.to_str().unwrap(), "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(out)
        .spawn()
        .unwrap();
    let g = support::Guard::new(child);
    for _ in 0..200 {
        if support::listening(port) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert_eq!(send(port, "GET", "/value", &[], "").body, r#"{"error":"internal error"}"#);
    drop(g);
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("quillo: error: GET /value (rid ") && text.contains(": db: no such column: nope"), "{}", text);
    // Where it was made, ahead of it (FEEDBACK21.md, Q-174).
    assert!(text.contains("main.qwe:"), "{}", text);
}

#[test]
fn its_author_sees_it_under_watch() {
    use std::process::{Command, Stdio};
    let src = format!("{}{}", USERS, "route GET /value = db.query(\"select nope from users\")\n");
    let (_d, f) = app("watch", &src);
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", f.to_str().unwrap(), "--port", &port.to_string(), "--watch"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    for _ in 0..200 {
        if support::listening(port) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(send(port, "GET", "/value", &[], "").body.contains("no such column: nope"));
}

// ---- Q-167: a host that names no customer --------------------------------------------------

const TENANTS: &str = r#"
tenant_from("example.com")
db.open("data/{tenant}.db")
route GET /landing = "welcome"
route GET /count = db.query("select 1 as one")
route GET /own {
    let r = db.query("select 1 as one")
    is_err(r) ? "no such customer here" : r
}
route GET /who = str(tenant())
route POST /make/:name = new_tenant(name, () => "made")
"#;

#[test]
fn a_made_up_subdomain_is_not_found_only_where_it_needs_a_database() {
    let (d, f) = app("ghost", TENANTS);
    let s = serve(&f, &[]);
    let ghost = [("host", "ghost.example.com")];
    let r = send(s.port, "GET", "/count", &ghost, "");
    assert_eq!((r.status, r.body.trim()), (404, "not found"));
    // A page that needs no database, as www's would, and a route that answers for itself.
    assert_eq!(send(s.port, "GET", "/landing", &ghost, "").body, "welcome");
    assert_eq!(send(s.port, "GET", "/who", &ghost, "").body, "ghost");
    assert_eq!(send(s.port, "GET", "/own", &ghost, "").body, "no such customer here");
    assert!(!d.join("data/ghost.db").exists(), "nothing is made");
    // And it is one request's: a real tenant after it is answered.
    assert_eq!(send(s.port, "POST", "/make/acme", &[("host", "example.com")], "").body, "made");
    assert_eq!(send(s.port, "GET", "/count", &[("host", "acme.example.com")], "").status, 200);
    assert_eq!(send(s.port, "GET", "/count", &ghost, "").status, 404);
}

// ---- Q-165: a signup ---------------------------------------------------------------------

#[test]
fn a_second_signup_does_not_land_in_the_first_customers_data() {
    let src = r#"
tenant_from("example.com")
db.open("data/{tenant}.db")
db.migrate()
route POST /signup/:name {
    let r = new_tenant(name, () => db.exec("insert into settings (plan) values ('free')"))
    if is_err(r) { reply 409 "taken" }
    "made"
}
route GET /rows = len(db.query("select * from settings"))
"#;
    let (d, f) = app("signup", src);
    std::fs::create_dir_all(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_settings.sql"), "create table settings (plan text);\n").unwrap();
    let s = serve(&f, &[]);
    let bare = [("host", "example.com")];
    assert_eq!(send(s.port, "POST", "/signup/acme", &bare, "").body, "made");
    let again = send(s.port, "POST", "/signup/acme", &bare, "");
    assert_eq!((again.status, again.body.as_str()), (409, "taken"));
    assert_eq!(send(s.port, "GET", "/rows", &[("host", "acme.example.com")], "").body, "1");
}

#[test]
fn of_many_signups_at_once_for_one_name_one_makes_it() {
    let (d, f) = app("race", "db.open(\"data/{tenant}.db\")\nroute POST /make = new_tenant(\"acme\", () => \"made\")\n");
    let s = serve(&f, &[]);
    let port = s.port;
    let tries: Vec<_> = (0..12).map(|_| std::thread::spawn(move || send(port, "POST", "/make", &[], "").body)).collect();
    let made = tries.into_iter().map(|t| t.join().unwrap()).filter(|b| b == "made").count();
    assert_eq!(made, 1);
    assert!(d.join("data/acme.db").is_file());
}

#[test]
fn new_tenant_says_what_it_needs() {
    let (_d, f) = app("needs", "db.open(\"data/{tenant}.db\")\nlet a = new_tenant(\"Bad Name\", () => 1)\n");
    let mut it = quillo::Interp::new(f.parent().unwrap());
    assert!(it.load_file(&f).err().unwrap().msg.contains("cannot be a tenant's name"));
    let (_d, f) = app("needs2", "db.open(\"data/{tenant}.db\")\nlet a = new_tenant(\"acme\")\n");
    let mut it = quillo::Interp::new(f.parent().unwrap());
    assert!(it.load_file(&f).err().unwrap().msg.contains("expects a function second"));
    let (_d, f) = app("needs3", "db.open(\"one.db\")\nlet a = new_tenant(\"acme\", () => 1)\n");
    let mut it = quillo::Interp::new(f.parent().unwrap());
    assert!(it.load_file(&f).err().unwrap().msg.contains("needs a database per tenant"));
}

// ---- Q-166: a global crosses tenants ---------------------------------------------------------

#[test]
fn a_global_a_route_writes_is_told_where_there_are_tenants() {
    let say = "`last_seen` is one copy for every tenant: keep a tenant's data in its database, or in a map keyed by tenant()";
    let (_d, f) = app("global", "tenant_from(\"example.com\")\nlet last_seen = \"nobody\"\nroute GET /peek {\n    last_seen = str(tenant())\n    \"x\"\n}\nfn note() {\n    last_seen = \"y\"\n}\nroute GET /n = note()\n");
    assert_eq!(warnings(&f).iter().filter(|m| *m == say).count(), 2, "a tenant's name in a global is the leak itself");
    let (_d, f) = app("global2", "tenant_from(\"example.com\")\nlet last_seen = \"nobody\"\nroute GET /peek {\n    last_seen = request.ip\n    \"x\"\n}\nfn note() {\n    last_seen = \"y\"\n}\nroute GET /n = note()\n");
    assert_eq!(warnings(&f).iter().filter(|m| *m == say).count(), 2, "{:?}", warnings(&f));
    // With no tenants, or at the top of a program, or keyed by the tenant, it is fine.
    let (_d, f) = app("global3", "let n = 0\nroute GET /peek {\n    n = n + 1\n    \"x\"\n}\n");
    assert_eq!(warnings(&f), Vec::<String>::new());
    let (_d, f) = app("global4", "tenant_from(\"example.com\")\nlet cache = {}\nlet n = 0\nn = 1\nroute GET /peek {\n    cache = set(cache, tenant(), 1)\n    \"x\"\n}\n");
    assert_eq!(warnings(&f), Vec::<String>::new());
}

// ---- Q-168: past 16 tenants ------------------------------------------------------------------

/// Each tenant's connection, by the server's number for it: the same one is still open
/// after the others have had a turn.
fn pids(port: u16, tenants: usize) -> Vec<String> {
    (0..tenants).map(|i| send(port, "GET", "/pid", &[("host", &format!("c{}.example.com", i))], "").body).collect()
}

#[test]
fn how_many_tenants_stay_open_is_the_programs_to_say() {
    let Some(pg) = support::postgres("feedback20-open") else { return };
    let src = |query: &str| format!(
        "tenant_from(\"example.com\")\ndb.open(\"{}?schema=t_{{tenant}}&pool_size=1{}\")\nroute POST /make/:name = new_tenant(name, () => \"made\")\nroute GET /pid = str(db.one(\"select pg_backend_pid() as p\").p)\n",
        pg.url, query
    );
    let (_d, f) = app("open-20", &src("&open_tenants=20"));
    let s = serve(&f, &[]);
    for i in 0..20 {
        assert_eq!(send(s.port, "POST", &format!("/make/c{}", i), &[("host", "example.com")], "").body, "made");
    }
    let (first, second) = (pids(s.port, 20), pids(s.port, 20));
    assert_eq!(first, second, "all 20 stayed connected");
    drop(s);
    let (_d, f) = app("open-default", &src(""));
    let s = serve(&f, &[]);
    let (first, second) = (pids(s.port, 20), pids(s.port, 20));
    assert_ne!(first, second, "past 16, each connects again");
    drop(s);
    let (_d, f) = app("open-bad", &src("&open_tenants=none"));
    let s = serve(&f, &[]);
    let r = send(s.port, "POST", "/make/c99", &[("host", "example.com")], "");
    assert_eq!(r.status, 500, "{}", r.body);
}

#[test]
fn a_signup_on_postgres_is_made_once_and_a_made_up_one_is_not_found() {
    let Some(pg) = support::postgres("feedback20-signup") else { return };
    let src = format!(
        "tenant_from(\"example.com\")\ndb.open(\"{}?schema=t_{{tenant}}\")\nroute POST /make/:name = new_tenant(name, () => \"made\")\nroute GET /one = db.one(\"select 1 as one\").one\n",
        pg.url
    );
    let (_d, f) = app("pg-signup", &src);
    let s = serve(&f, &[]);
    let bare = [("host", "example.com")];
    assert_eq!(send(s.port, "POST", "/make/acme", &bare, "").body, "made");
    let again = send(s.port, "POST", "/make/acme", &bare, "");
    assert_eq!((again.status, again.body.as_str()), (500, r#"{"error":"there is already a tenant \"acme\""}"#));
    assert_eq!(send(s.port, "GET", "/one", &[("host", "acme.example.com")], "").body, "1");
    assert_eq!(send(s.port, "GET", "/one", &[("host", "ghost.example.com")], "").status, 404);
}

#[test]
fn a_signup_on_mariadb_is_made_once_and_a_made_up_one_is_not_found() {
    let Some(m) = support::mariadb("feedback20-signup") else { return };
    let src = format!(
        "tenant_from(\"example.com\")\ndb.open(\"{}\")\nroute POST /make/:name = new_tenant(name, () => \"made\")\nroute GET /one = db.one(\"select 1 as one\").one\n",
        m.url("app_{tenant}")
    );
    let (_d, f) = app("my-signup", &src);
    let s = serve(&f, &[]);
    let bare = [("host", "example.com")];
    assert_eq!(send(s.port, "POST", "/make/acme", &bare, "").body, "made");
    assert!(send(s.port, "POST", "/make/acme", &bare, "").body.contains("already a tenant"));
    assert_eq!(send(s.port, "GET", "/one", &[("host", "acme.example.com")], "").body, "1");
    assert_eq!(send(s.port, "GET", "/one", &[("host", "ghost.example.com")], "").status, 404);
}

// ---- one request cannot stop a server (Q-162, Q-163, Q-164) ------------------------------------

const STUCK: &str = r#"
route GET /ping = "pong"
route GET /forever {
    let i = 0
    while i >= 0 { i = i + 1 }
    "never"
}
route GET /wait {
    sleep(1200)
    "waited"
}
fn spin(n: int) -> int {
    let i = n
    while i >= 0 { i = i + 1 }
    i
}
route GET /compiled = spin(1)
route GET /each = sum(range(0, 100000000) | map((n) => n + 1))
"#;

/// A server of its own, with its log in a file.
fn started(name: &str, args: &[&str]) -> (std::process::Child, u16, PathBuf) {
    let (d, f) = app(name, STUCK);
    let port = support::free_port();
    let log = d.join("log.txt");
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", f.to_str().unwrap(), "--port", &port.to_string()])
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    for _ in 0..400 {
        if support::listening(port) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    (child, port, log)
}

#[test]
fn a_request_past_the_limit_is_ended_and_the_server_goes_on() {
    let (mut c, port, log) = started("limit", &["--max-compute", "300ms"]);
    for path in ["/forever", "/compiled", "/each"] {
        let began = std::time::Instant::now();
        let r = send(port, "GET", path, &[], "");
        assert_eq!((r.status, r.body.trim()), (500, "internal error"), "{}", path);
        assert!(began.elapsed() < std::time::Duration::from_secs(5), "{}: {:?}", path, began.elapsed());
        assert_eq!(send(port, "GET", "/ping", &[], "").body, "pong");
    }
    // Waiting is not computing: this takes longer than the limit, and lets others go.
    assert_eq!(send(port, "GET", "/wait", &[], "").body, "waited");
    let _ = c.kill();
    let _ = c.wait();
    let text = std::fs::read_to_string(log).unwrap();
    assert!(text.contains("computed for longer than --max-compute"), "{}", text);
}

#[test]
fn the_limit_needs_a_length_of_time() {
    let (d, f) = app("limit-bad", STUCK);
    for bad in ["soon", "0s", "5", "5h", "s"] {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_quillo")).args(["run", f.to_str().unwrap(), "--max-compute", bad]).current_dir(&d).output().unwrap();
        assert_eq!(o.status.code(), Some(2), "{}", bad);
        assert!(String::from_utf8_lossy(&o.stderr).contains("--max-compute needs a length of time"), "{}", bad);
    }
    for good in ["250ms", "2s", "1m"] {
        let (mut c, port, _) = started("limit-good", &["--max-compute", good]);
        assert_eq!(send(port, "GET", "/ping", &[], "").body, "pong", "{}", good);
        let _ = c.kill();
        let _ = c.wait();
    }
}

#[cfg(unix)]
#[test]
fn a_stuck_instance_stops_nothing_and_a_signal_ends_it_in_time() {
    let (mut c, port, log) = started("instances", &["--instances", "2"]);
    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(|| send(port, "GET", "/forever", &[], ""));
    });
    std::thread::sleep(std::time::Duration::from_millis(600));
    // New connections do not wait for it, and do not land on it.
    for _ in 0..8 {
        assert_eq!(send(port, "GET", "/ping", &[], "").body, "pong");
    }
    std::thread::sleep(std::time::Duration::from_millis(5000));
    let began = std::time::Instant::now();
    unsafe { libc::kill(c.id() as i32, libc::SIGTERM) };
    let status = c.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    assert!(began.elapsed() < std::time::Duration::from_secs(13), "{:?}", began.elapsed());
    let text = std::fs::read_to_string(log).unwrap();
    assert!(text.contains("computing for 5 s without letting go: GET /forever"), "{}", text);
    assert!(text.contains("stopping with requests in progress: GET /forever"), "{}", text);
}

#[test]
fn a_while_that_cannot_end_is_told() {
    let say = "this `while` never ends: nothing in it can stop it, and it holds its instance (see --max-compute)";
    let (_d, f) = app("endless", "route GET / {\n    let i = 0\n    while true { i = i + 1 }\n    i\n}\n");
    assert!(warnings(&f).iter().any(|m| m == say), "{:?}", warnings(&f));
    for body in ["sleep(10)", "if i > 3 { return i }", "reply 200 \"x\""] {
        let (_d, f) = app("ends", &format!("route GET / {{\n    let i = 0\n    while true {{\n        i = i + 1\n        {}\n    }}\n    i\n}}\n", body));
        assert!(!warnings(&f).iter().any(|m| m == say), "{}: {:?}", body, warnings(&f));
    }
    let (_d, f) = app("ends2", "route GET / {\n    let i = 0\n    while i < 3 { i = i + 1 }\n    i\n}\n");
    assert_eq!(warnings(&f), Vec::<String>::new());
}

// ---- Q-171: every error a builtin makes; Q-175: which request it was --------------------------

const BUILTIN_ERRORS: &str = r#"
route GET /billing = http.get("http://127.0.0.1:1/internal/billing?key=abc")
route GET /keys = read_file("/srv/app/secrets/keys.json")
route GET /number = int("x")
route GET /helper {
    let n = parse("x")?
    n
}
fn parse(s: str) = int(s)
route GET /mine = err("Out of stock")
route GET /mine/nested = { problem: err("Out of stock"), hidden: int("x") }
route GET /caught {
    let r = int("x")
    is_err(r) ? "not a number" : r
}
"#;

#[test]
fn what_a_builtin_says_is_not_for_the_visitor_and_is_in_the_log() {
    let (d, f) = app("builtins", BUILTIN_ERRORS);
    let port = support::free_port();
    let log = d.join("log.txt");
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", f.to_str().unwrap(), "--port", &port.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let g = support::Guard::new(child);
    for _ in 0..200 {
        if support::listening(port) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    for path in ["/billing", "/keys", "/number", "/helper"] {
        let r = send(port, "GET", path, &[], "");
        assert_eq!((r.status, r.body.as_str()), (500, r#"{"error":"internal error"}"#), "{}", path);
    }
    let r = send(port, "GET", "/mine", &[], "");
    assert_eq!((r.status, r.body.as_str()), (500, r#"{"error":"Out of stock"}"#));
    let nested = send(port, "GET", "/mine/nested", &[], "").body;
    assert!(nested.contains("Out of stock") && nested.contains("internal error") && !nested.contains("not an int"), "{}", nested);
    assert_eq!(send(port, "GET", "/caught", &[], "").body, "not a number");
    drop(g);
    let text = std::fs::read_to_string(&log).unwrap();
    for line in ["GET /billing (rid", "internal/billing?key=abc", "GET /keys (rid", "secrets/keys.json", "GET /number (rid", "GET /helper (rid", "GET /mine (rid"] {
        assert!(text.contains(line), "{}: {}", line, text);
    }
}

#[test]
fn an_error_line_is_an_object_where_the_log_is_objects() {
    let (d, f) = app("json-log", "route GET /boom = 1 / 0\nroute GET /value = int(\"x\")\n");
    let port = support::free_port();
    let log = d.join("log.txt");
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", f.to_str().unwrap(), "--port", &port.to_string(), "--log", "json", "--no-check"])
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let g = support::Guard::new(child);
    for _ in 0..200 {
        if support::listening(port) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let r = send(port, "GET", "/boom", &[("x-request-id", "abc123")], "");
    assert_eq!(r.status, 500);
    assert_eq!(send(port, "GET", "/value", &[], "").status, 500);
    drop(g);
    let text = std::fs::read_to_string(&log).unwrap();
    let errors: Vec<serde_json::Value> = text.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()).filter(|j| j["level"] == "error").collect();
    assert_eq!(errors.len(), 2, "{}", text);
    assert_eq!(errors[0]["request"], "GET /boom (rid abc123)", "{}", text);
    assert!(errors[0]["error"].as_str().unwrap().contains("division by zero"), "{}", text);
    assert!(errors[1]["error"].as_str().unwrap().contains("not an int"), "{}", text);
}

#[test]
fn a_services_own_error_reaches_the_visitor_and_its_databases_does_not() {
    let (d, f) = app("service-errors", "service stock = \"stock.qwe\"\nroute GET /own = stock.out()\nroute GET /sql = stock.sql()\n");
    std::fs::write(d.join("stock.qwe"), "db.open(\":memory:\")\nserver fn out() = err(\"Out of stock\")\nserver fn sql() = db.query(\"select nope from nowhere\")\n").unwrap();
    let s = serve(&f, &[]);
    assert_eq!(send(s.port, "GET", "/own", &[], "").body, r#"{"error":"Out of stock"}"#);
    let r = send(s.port, "GET", "/sql", &[], "");
    assert_eq!(r.body, r#"{"error":"internal error"}"#, "the caller's visitor");
}
