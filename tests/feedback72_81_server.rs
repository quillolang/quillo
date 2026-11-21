//! FEEDBACK72.md, FEEDBACK79.md and FEEDBACK80.md, slice S (server and operations): a program with
//! nothing to serve over HTTP opens no port (Q-1011); a file in a form field the pattern names is
//! said (Q-1252); readiness with a database per tenant (Q-1254), not logged or counted (Q-1255);
//! `site_headers`' `server: null` (Q-1256); the closest route's miss (Q-1257); SQLite closed on a
//! clean stop (Q-1260); a 304 logged as one (Q-1262); WebTransport closed on a clean stop (Q-1265),
//! and listened for on [::1] as well (Q-1266).
//! Branch feedback72-81.

mod support;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const TOKEN: &str = "abcdefghijklmnop1234-a-health-token";

/// A folder of its own, removed when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Dir {
    fn new(name: &str, files: &[(&str, &str)]) -> Dir {
        let d = support::temp_dir(&format!("f7281s-{}", name));
        for (f, src) in files {
            if let Some(parent) = Path::new(f).parent() {
                std::fs::create_dir_all(d.join(parent)).unwrap();
            }
            std::fs::write(d.join(f), src).unwrap();
        }
        Dir(d)
    }

    fn file(&self, f: &str) -> PathBuf {
        self.0.join(f)
    }
}

/// `quillo check` takes the program (every snippet here passes it).
fn checks(file: &Path) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(file).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let status = wait_for(&mut c, Duration::from_secs(60)).unwrap_or_else(|| panic!("quillo check {} took too long", file.display()));
    let mut out = String::new();
    let _ = c.stdout.take().unwrap().read_to_string(&mut out);
    let _ = c.stderr.take().unwrap().read_to_string(&mut out);
    assert!(status.success(), "quillo check {}: {}", file.display(), out);
}

/// The process's end within `within`, or None (it is then killed).
fn wait_for(c: &mut Child, within: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + within;
    loop {
        if let Some(s) = c.try_wait().unwrap() {
            return Some(s);
        }
        if Instant::now() > deadline {
            let _ = c.kill();
            let _ = c.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// A running server, its log in a file, stopped as Ctrl-C does when dropped.
struct Run {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Run {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = Command::new("kill").args(["-INT", &self.child.id().to_string()]).stderr(Stdio::null()).status();
        }
        if wait_for(&mut self.child, Duration::from_secs(10)).is_none() {
            let _ = self.child.kill();
        }
        let _ = std::fs::remove_file(&self.log);
    }
}

impl Run {
    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn get(&self, path: &str) -> support::Answer {
        support::send(self.port, "GET", path, &[], "")
    }

    fn post(&self, path: &str, headers: &[(&str, &str)], body: &str) -> support::Answer {
        support::send(self.port, "POST", path, headers, body)
    }

    /// Signals it, and waits for its end; its exit status.
    fn stop(&mut self, signal: &str) -> std::process::ExitStatus {
        let _ = Command::new("kill").args([signal, &self.child.id().to_string()]).status();
        wait_for(&mut self.child, Duration::from_secs(60)).expect("the server did not stop")
    }
}

/// `quillo run file --port 0` (or the port in `extra`) with `extra`, its log in a file.
fn run(file: &Path, extra: &[&str], env: &[(&str, &str)]) -> Run {
    static N: AtomicUsize = AtomicUsize::new(0);
    checks(file);
    let log = file.with_extension(format!("log{}", N.fetch_add(1, Ordering::SeqCst)));
    let port_given = extra.contains(&"--port");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(if port_given { &[][..] } else { &["--port", "0"][..] })
        .args(extra)
        .current_dir(file.parent().unwrap())
        .env("QUILLO_SECRET", "a test secret for slice S, long enough")
        .env("QUILLO_HEALTH_TOKEN", TOKEN)
        .env_remove("PORT")
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .expect("cannot start quillo");
    support::stop_with_tests(&child);
    let mut r = Run { child, port: 0, log };
    r.port = support::port_in_log(&mut r.child, &r.log, Duration::from_secs(30));
    let deadline = Instant::now() + Duration::from_secs(15);
    while support::send(r.port, "GET", "/_quillo/health", &[], "").status != 200 {
        assert!(Instant::now() < deadline, "the server does not answer");
        std::thread::sleep(Duration::from_millis(20));
    }
    r
}

/// The lines of the log that name `path` as a request (`GET /x -> 200`).
fn logged(r: &Run, path: &str) -> Vec<String> {
    r.log().lines().filter(|l| l.contains(&format!(" {} -> ", path)) || l.contains(&format!(" {}? ", path))).map(str::to_string).collect()
}

/// A request counter of `--metrics`, by its class (`2xx`).
fn counted(r: &Run, class: &str) -> u64 {
    let m = support::send(r.port, "GET", "/_quillo/metrics", &[("Authorization", &format!("Bearer {}", TOKEN))], "").body;
    let key = format!("quillo_requests_total{{status=\"{}\"}} ", class);
    m.lines().find_map(|l| l.strip_prefix(&key)).and_then(|n| n.trim().parse().ok()).unwrap_or_else(|| panic!("no {} in {}", class, m))
}

fn ready(r: &Run) -> (u16, serde_json::Value) {
    let a = support::send(r.port, "GET", "/_quillo/ready", &[("Authorization", &format!("Bearer {}", TOKEN))], "");
    (a.status, serde_json::from_str(&a.body).unwrap_or_else(|e| panic!("{}: {}", e, a.body)))
}

// ---- Q-1254: readiness with a database per tenant ----------------------------------------

const TENANTS: &str = r#"tenant_from("localhost")
db.open(TARGET)
route POST /teams/:name = new_tenant(name, () => "made")
route GET / with db = `tenant ${tenant()}: ${len(db.query("select 1 as one"))} row`
"#;

fn tenants(target: &str) -> String {
    TENANTS.replace("TARGET", &serde_json::to_string(target).unwrap())
}

/// Ready, and "ready (per tenant)", while every tenant's page works, as "database: down" (503)
/// kept an orchestrator from ever sending an app with tenants traffic (FEEDBACK79.md, Q-1254;
/// branch feedback72-81).
#[test]
fn ready_with_a_database_per_tenant_says_ready_per_tenant() {
    let d = Dir::new("tenants", &[("main.qwe", &tenants("data/{tenant}.db"))]);
    std::fs::create_dir_all(d.file("data")).unwrap();
    let r = run(&d.file("main.qwe"), &[], &[]);
    // No tenant yet: a tenant's database can be made.
    assert_eq!(ready(&r), (200, serde_json::json!({ "ready": true, "checks": { "database": "ready (per tenant)" } })));
    assert_eq!(r.post("/teams/acme", &[], "").body, "made");
    assert_eq!(support::send(r.port, "GET", "/", &[("Host", "acme.localhost")], "").body, "tenant acme: 1 row");
    // One tenant, then two.
    assert_eq!(ready(&r).0, 200);
    assert_eq!(r.post("/teams/zoë-co", &[], "").status, 500, "not a tenant's name");
    assert_eq!(r.post("/teams/beta", &[], "").body, "made");
    assert_eq!(ready(&r), (200, serde_json::json!({ "ready": true, "checks": { "database": "ready (per tenant)" } })));
    // Without the token, whether it is ready, no more.
    let plain = r.get("/_quillo/ready");
    assert_eq!((plain.status, plain.body.as_str()), (200, "{\"ready\":true}"));
}

/// The tenants' folder: not there yet with its parent writable (new_tenant makes it), a folder per
/// tenant, a name in another script, a function's databases; and a folder that cannot be written
/// to, which is down (FEEDBACK79.md, Q-1254; branch feedback72-81).
#[test]
fn ready_per_tenant_reads_the_folder_tenants_need() {
    for (target, made) in [("later/{tenant}.db", false), ("by/{tenant}/app.db", true), ("données/t_{tenant}.db", true), ("{tenant}.db", false)] {
        let d = Dir::new("tfolder", &[("main.qwe", &tenants(target))]);
        if made {
            std::fs::create_dir_all(d.file(target.split('/').next().unwrap())).unwrap();
        }
        let r = run(&d.file("main.qwe"), &[], &[]);
        assert_eq!(ready(&r), (200, serde_json::json!({ "ready": true, "checks": { "database": "ready (per tenant)" } })), "{}", target);
        assert_eq!(r.post("/teams/acme", &[], "").body, "made", "{}", target);
        assert_eq!(support::send(r.port, "GET", "/", &[("Host", "acme.localhost")], "").body, "tenant acme: 1 row", "{}", target);
        assert_eq!(ready(&r).0, 200, "{}", target);
    }
    let src = "tenant_from(\"localhost\")\ndb.open((t) => `fn/${t}.db`)\nroute GET / with db = `tenant ${tenant()}`\n";
    let d = Dir::new("tfn", &[("main.qwe", src)]);
    let r = run(&d.file("main.qwe"), &[], &[]);
    assert_eq!(ready(&r).1["checks"]["database"], "ready (per tenant)");
    // A folder no file can be made in.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let d = Dir::new("tlocked", &[("main.qwe", &tenants("locked/{tenant}.db"))]);
        std::fs::create_dir_all(d.file("locked")).unwrap();
        std::fs::set_permissions(d.file("locked"), std::fs::Permissions::from_mode(0o500)).unwrap();
        // Root writes anywhere: nothing to see then.
        let root = std::fs::write(d.file("locked/probe"), "x").is_ok();
        let r = run(&d.file("main.qwe"), &[], &[]);
        if !root {
            assert_eq!(ready(&r), (503, serde_json::json!({ "ready": false, "checks": { "database": "down" } })));
            let plain = r.get("/_quillo/ready");
            assert_eq!((plain.status, plain.body.as_str()), (503, "{\"ready\":false}"));
        }
        std::fs::set_permissions(d.file("locked"), std::fs::Permissions::from_mode(0o700)).unwrap();
        // Writable again: ready again, at the next probe.
        assert_eq!(ready(&r).0, 200);
    }
}

/// A server's tenants (`postgres://host/app_{tenant}`): ready while the server takes a connection,
/// down while nothing listens there (FEEDBACK79.md, Q-1254; branch feedback72-81).
#[test]
fn ready_per_tenant_on_a_server_asks_whether_it_is_reachable() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let up = listener.local_addr().unwrap().port();
    let down = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    for (port, want) in [(up, (200, "ready (per tenant)")), (down, (503, "down"))] {
        let target = format!("postgres://quillo:secret@127.0.0.1:{}/app_{{tenant}}", port);
        let d = Dir::new("tpg", &[("main.qwe", &tenants(&target))]);
        let r = run(&d.file("main.qwe"), &[], &[]);
        let (status, body) = ready(&r);
        assert_eq!((status, body["checks"]["database"].as_str().unwrap()), want, "{}", body);
    }
    drop(listener);
}

/// A server reached by its Unix socket (PostgreSQL's folder as the host, or `?host=`; MySQL's
/// `?socket=`): ready while something listens on the socket, down when nothing does; it was dialed
/// as a host name and said down (FEEDBACK79.md, Q-1254, found in review; branch feedback72-81).
#[cfg(unix)]
#[test]
fn ready_per_tenant_on_a_server_through_its_unix_socket() {
    // A short folder: a socket's path is at most about 100 bytes.
    let dir = PathBuf::from(format!("/tmp/q1254-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pg_sock = dir.join(".s.PGSQL.5432");
    let my_sock = dir.join("mysqld.sock");
    let enc = dir.to_string_lossy().replace('/', "%2F");
    let targets = [
        format!("postgres://app@{}/app_{{tenant}}", enc),
        format!("postgres:///app_{{tenant}}?host={}", dir.display()),
        format!("mysql://app@localhost/app_{{tenant}}?socket={}", my_sock.display()),
    ];
    for target in &targets {
        let d = Dir::new("tsock", &[("main.qwe", &tenants(target))]);
        let r = run(&d.file("main.qwe"), &[], &[]);
        assert_eq!(ready(&r), (503, serde_json::json!({ "ready": false, "checks": { "database": "down" } })), "{}", target);
        let listeners = (std::os::unix::net::UnixListener::bind(&pg_sock).unwrap(), std::os::unix::net::UnixListener::bind(&my_sock).unwrap());
        assert_eq!(ready(&r), (200, serde_json::json!({ "ready": true, "checks": { "database": "ready (per tenant)" } })), "{}", target);
        drop(listeners);
        let _ = std::fs::remove_file(&pg_sock);
        let _ = std::fs::remove_file(&my_sock);
        // A socket file nothing listens on any more: down.
        let stale = std::os::unix::net::UnixListener::bind(&pg_sock).unwrap();
        drop(stale);
        assert_eq!(ready(&r).0, 503, "{}", target);
        let _ = std::fs::remove_file(&pg_sock);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// One database, as before: `select 1` (FEEDBACK79.md, Q-1254; branch feedback72-81).
#[test]
fn ready_with_one_database_is_as_it_was() {
    let d = Dir::new("tone", &[("main.qwe", "db.open(\"app.db\")\nroute GET / = \"hi\"\n")]);
    let r = run(&d.file("main.qwe"), &[], &[]);
    assert_eq!(ready(&r), (200, serde_json::json!({ "ready": true, "checks": { "database": "ready" } })));
}

// ---- Q-1255: ready is neither logged nor counted -----------------------------------------

/// `/_quillo/ready` is not logged and not in `quillo_requests_total`, as `/_quillo/health` is not,
/// and a 503 from it is no 5xx; a route's request still is, and so is a path that only looks like
/// it (FEEDBACK79.md, Q-1255; branch feedback72-81).
#[test]
fn ready_is_neither_logged_nor_counted() {
    let d = Dir::new("rlog", &[("main.qwe", "db.open(\"app.db\")\nroute GET / = \"hi\"\n")]);
    let r = run(&d.file("main.qwe"), &["--metrics"], &[]);
    for p in ["/_quillo/ready", "/_quillo/ready", "/_quillo/ready?probe=1", "/_quillo/health"] {
        assert_eq!(r.get(p).status, 200, "{}", p);
    }
    assert_eq!(ready(&r).0, 200);
    assert_eq!((counted(&r, "2xx"), counted(&r, "5xx")), (0, 0));
    assert_eq!(r.get("/").body, "hi");
    assert_eq!(r.get("/_quillo/readyx").status, 404);
    eventually(10, "the route's request logged", || logged(&r, "/").first().cloned());
    assert_eq!((counted(&r, "2xx"), counted(&r, "4xx")), (1, 1));
    assert!(logged(&r, "/_quillo/ready").is_empty(), "{}", r.log());
    assert!(!logged(&r, "/_quillo/readyx").is_empty(), "{}", r.log());
    // A program's route that a path ending so reaches is the program's: logged and counted; a
    // mounted program's own ready is Quillo's, and is not.
    let src = "let calls = 0\nroute POST /api/:thing/:verb { calls = calls + 1; `${thing} ${verb}` }\nroute GET /calls = calls\nmount /m = \"inner.qwe\"\n";
    let d = Dir::new("rlog-route", &[("main.qwe", src), ("inner.qwe", "route GET / = \"inner\"\n")]);
    let r = run(&d.file("main.qwe"), &["--metrics"], &[]);
    assert_eq!(r.post("/api/_quillo/ready", &[], "").body, "_quillo ready");
    assert_eq!(r.get("/m/_quillo/ready").status, 200);
    assert_eq!(r.get("/calls").body, "1");
    eventually(10, "the route's request logged", || logged(&r, "/api/_quillo/ready").first().cloned());
    assert_eq!(counted(&r, "2xx"), 2);
    assert!(logged(&r, "/m/_quillo/ready").is_empty(), "{}", r.log());
    // A 503 from it: no 5xx either.
    let down = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let d = Dir::new("rlog503", &[("main.qwe", &tenants(&format!("postgres://q:s@127.0.0.1:{}/app_{{tenant}}", down)))]);
    let r = run(&d.file("main.qwe"), &["--metrics"], &[]);
    assert_eq!(r.get("/_quillo/ready").status, 503);
    assert_eq!(ready(&r).0, 503);
    assert_eq!((counted(&r, "2xx"), counted(&r, "5xx")), (0, 0));
    assert!(logged(&r, "/_quillo/ready").is_empty(), "{}", r.log());
}

// ---- Q-1256: site_headers takes `server` away ---------------------------------------------

const HEADERS: &str = r#"site_headers({ "server": null, "x-frame-options": null })
site_headers("/named", { "server": "Edge" })
route GET / = <p>hi</p>
route GET /named = <p>named</p>
route GET /named/deep = "deep"
route GET /own = respond(200, "hi", { "server": "Mine" })
route GET /gone = respond(200, "hi", { "server": null })
route GET /s = stream((send) => send("s"))
route GET /missing = respond(404, "no")
"#;

/// A rule's `"server": null` takes `server: Quillo` away, on a page, a stream and a 404 alike; a
/// longer path's rule gives its own; a route's own wins over either (FEEDBACK79.md, Q-1256; branch
/// feedback72-81).
#[test]
fn site_headers_takes_away_or_replaces_server() {
    let d = Dir::new("server", &[("main.qwe", HEADERS)]);
    let r = run(&d.file("main.qwe"), &[], &[]);
    for p in ["/", "/s", "/missing", "/gone"] {
        let a = r.get(p);
        assert_eq!(a.header("server"), None, "{}: {:?}", p, a.headers);
    }
    assert_eq!(r.get("/").header("x-frame-options"), None);
    assert_eq!(r.get("/named").header("server"), Some("Edge"));
    assert_eq!(r.get("/named/deep").header("server"), Some("Edge"));
    assert_eq!(r.get("/own").header("server"), Some("Mine"));
    // Only one `server` line, never an empty one.
    let a = r.get("/named");
    assert_eq!(a.headers.iter().filter(|(k, _)| k == "server").count(), 1);
    // Without a rule, Quillo's, as before.
    let d = Dir::new("server-kept", &[("main.qwe", "site_headers({ \"x-n\": \"1\" })\nroute GET / = \"plain\"\n")]);
    let r = run(&d.file("main.qwe"), &[], &[]);
    let a = r.get("/");
    assert_eq!((a.header("server"), a.header("x-n")), (Some("Quillo"), Some("1")));
}

// ---- Q-1257: the closest route's miss ----------------------------------------------------

const CONTACT: &str = r#"route POST /contact { form: { name: str, email: str, message: str, updates: bool } } = "form"
route POST /contact { json: { name: str, email: str, message: str, updates: bool = false } } = "json"
route POST /two { json: { a: str, n: int } } = "a"
route POST /two { json: { b: str, n: int } } = "b"
route POST /q { query: { page: int } } = "q"
route POST /q { form: { page: int, size: int } } = "f"
"#;

fn misses(r: &Run, path: &str) -> Vec<String> {
    r.log().lines().filter_map(|l| l.split_once(&format!("400: POST {} (", path)).map(|(_, rest)| rest.to_string())).collect()
}

const JSON: [(&str, &str); 1] = [("Content-Type", "application/json")];
const FORM: [(&str, &str); 1] = [("Content-Type", "application/x-www-form-urlencoded")];

/// With several routes on one path, the 400 names the miss of the route whose body is of the
/// request's kind, then the one that matched the most fields; on a tie the first written, as
/// before (FEEDBACK79.md, Q-1257; branch feedback72-81).
#[test]
fn a_400_names_the_closest_routes_miss() {
    let d = Dir::new("closest", &[("main.qwe", CONTACT)]);
    let r = run(&d.file("main.qwe"), &[], &[]);
    assert_eq!(r.post("/contact", &JSON, r#"{"name":"Bo","email":"bo@example.com"}"#).status, 400);
    let utf8 = [("Content-Type", "application/json; charset=utf-8")];
    assert_eq!(r.post("/contact", &utf8, r#"{"name":"Bø","email":"bo@example.com","message":"hi ✓","updates":"yes"}"#).status, 400);
    // A form post: the form route's miss.
    assert_eq!(r.post("/contact", &FORM, "name=Bo").status, 400);
    // Of no kind, nothing matched: the first, as before.
    assert_eq!(r.post("/contact", &[("Content-Type", "text/plain")], "").status, 400);
    // One that matches still does.
    assert_eq!(r.post("/contact", &JSON, r#"{"name":"Bo","email":"e","message":"m"}"#).body, "json");
    assert_eq!(r.post("/contact", &FORM, "name=Bo&email=e&message=m&updates=true").body, "form");
    let said = eventually(10, "four 400s logged", || Some(misses(&r, "/contact")).filter(|m| m.len() == 4));
    assert_eq!(
        said,
        [
            "main.qwe:2): `json.message` is missing, and must be a str",
            "main.qwe:2): `json.updates` is text, not a bool",
            "main.qwe:1): `form.email` is missing, and must be a str",
            "main.qwe:1): `form.name` is missing, and must be a str",
        ]
    );
    // Two of one kind: the one that matched more fields; on a tie (none, one each), the first.
    assert_eq!(r.post("/two", &JSON, r#"{"b":"x","n":"no"}"#).status, 400);
    assert_eq!(r.post("/two", &JSON, r#"{}"#).status, 400);
    assert_eq!(r.post("/two", &JSON, r#"{"a":"x","b":"y","n":"no"}"#).status, 400);
    let said = eventually(10, "three 400s logged", || Some(misses(&r, "/two")).filter(|m| m.len() == 3));
    assert_eq!(said, ["main.qwe:4): `json.n` is text, not an int", "main.qwe:3): `json.a` is missing, and must be a str", "main.qwe:3): `json.n` is text, not an int"]);
    // A query pattern's miss beside a form route's: for a form post, the form's.
    assert_eq!(r.post("/q?page=x", &FORM, "page=1").status, 400);
    let said = eventually(10, "a 400 logged", || Some(misses(&r, "/q")).filter(|m| m.len() == 1));
    assert_eq!(said, ["main.qwe:6): `form.size` is missing, and must be an int"]);
}

/// The routes the other way round: each request is told its own kind's miss still (FEEDBACK79.md,
/// Q-1257; branch feedback72-81).
#[test]
fn a_400_names_the_closest_routes_miss_whichever_comes_first() {
    let mut lines: Vec<&str> = CONTACT.lines().take(2).collect();
    lines.swap(0, 1);
    let d = Dir::new("closest-swapped", &[("main.qwe", &(lines.join("\n") + "\n"))]);
    let r = run(&d.file("main.qwe"), &[], &[]);
    assert_eq!(r.post("/contact", &JSON, r#"{"name":"Bo","email":"bo@example.com"}"#).status, 400);
    assert_eq!(r.post("/contact", &FORM, "name=Bo&email=bo%40example.com&message=hi").status, 400);
    let said = eventually(10, "two 400s logged", || Some(misses(&r, "/contact")).filter(|m| m.len() == 2));
    assert_eq!(
        said,
        [
            "main.qwe:1): `json.message` is missing, and must be a str",
            "main.qwe:2): `form.updates` is missing, and must be a bool (an unchecked checkbox sends nothing: give it a default, `updates: bool = false`)"
        ]
    );
}

// ---- Q-1252: a file in a form field ------------------------------------------------------

const UPLOAD: &str = r#"route POST /a { form: { name: str = "(none)" } } = `name is ${name}`
route POST /b { form: { name: str } } = `name is ${name}`
route POST /c { form: { name: str | null } } = `name is ${name ?? "null"}`
route POST /d { form: { age: int = 0 } } = `age ${age}`
route POST /e { form.name: str = "(none)" } = `name is ${name}`
"#;

/// A multipart body of `parts`: (field, file name or none, content).
fn multipart(parts: &[(&str, Option<&str>, &str)]) -> (String, String) {
    let b = "XqUiLlObOuNdArY";
    let mut body = String::new();
    for (field, file, content) in parts {
        body.push_str(&format!("--{}\r\n", b));
        match file {
            Some(f) => body.push_str(&format!("Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: text/plain\r\n\r\n", field, f)),
            None => body.push_str(&format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", field)),
        }
        body.push_str(content);
        body.push_str("\r\n");
    }
    body.push_str(&format!("--{}--\r\n", b));
    (format!("multipart/form-data; boundary={}", b), body)
}

fn upload(r: &Run, path: &str, parts: &[(&str, Option<&str>, &str)]) -> support::Answer {
    let (ctype, body) = multipart(parts);
    r.post(path, &[("Content-Type", &ctype)], &body)
}

/// A file sent in a field the form pattern names is said to be one, where it is: in the log, with a
/// default used, and for the 400 (in the answer too under `--watch`); a field sent as text
/// beside it, or a file under another name, says nothing (FEEDBACK79.md, Q-1252; branch
/// feedback72-81).
#[test]
fn a_file_in_a_form_field_is_said_to_be_one() {
    let d = Dir::new("upload", &[("main.qwe", UPLOAD)]);
    let r = run(&d.file("main.qwe"), &[], &[]);
    assert_eq!(upload(&r, "/a", &[("name", Some("hello.txt"), "hello")]).body, "name is (none)");
    let b = upload(&r, "/b", &[("name", Some("hello.txt"), "hello")]);
    assert_eq!((b.status, b.body.as_str()), (400, "bad request: no route pattern matches this request\n"));
    assert_eq!(upload(&r, "/c", &[("name", Some("héllo ✓.txt"), "x")]).status, 400);
    assert_eq!(upload(&r, "/d", &[("age", Some("a\u{1}b.txt"), "x")]).body, "age 0");
    let long = format!("{}.txt", "n".repeat(200));
    assert_eq!(upload(&r, "/a", &[("name", Some(&long), "x")]).body, "name is (none)");
    // Text beside it, or a file under another name: nothing said.
    assert_eq!(upload(&r, "/a", &[("name", None, "Ana"), ("name", Some("x.txt"), "x")]).body, "name is Ana");
    assert_eq!(upload(&r, "/a", &[("photo", Some("p.txt"), "x")]).body, "name is (none)");
    assert_eq!(upload(&r, "/a", &[]).body, "name is (none)");
    // The pattern's field by its dotted name.
    assert_eq!(upload(&r, "/e", &[("name", Some("e.txt"), "x")]).body, "name is (none)");
    let said = eventually(10, "the six warnings logged", || {
        let l: Vec<String> = r.log().lines().filter(|l| l.contains("is a file")).map(str::to_string).collect();
        (l.len() >= 6).then_some(l)
    });
    let tail = |l: &str| l.split_once("main.qwe:").map(|(_, t)| t.to_string()).unwrap();
    assert_eq!(
        said.iter().map(|l| tail(l)).collect::<Vec<_>>(),
        [
            "1): `form.name` is a file (hello.txt), not a str; it is in request.files.name (the route ran with its default)".to_string(),
            "2): `form.name` is a file (hello.txt), not a str; it is in request.files.name".to_string(),
            "3): `form.name` is a file (héllo ✓.txt), not what its pattern takes; it is in request.files.name".to_string(),
            "4): `form.age` is a file (ab.txt), not an int; it is in request.files.age (the route ran with its default)".to_string(),
            format!("1): `form.name` is a file ({}), not a str; it is in request.files.name (the route ran with its default)", &long[..80]),
            "5): `form.name` is a file (e.txt), not a str; it is in request.files.name (the route ran with its default)".to_string(),
        ]
    );
    assert!(said[1].contains("400: POST /b"), "{}", said[1]);
    assert_eq!(r.log().lines().filter(|l| l.contains("is a file")).count(), 6, "{}", r.log());
    // A field missing and no file: as before.
    assert_eq!(upload(&r, "/b", &[("other", None, "x")]).status, 400);
    eventually(10, "the miss logged", || r.log().contains("`form.name` is missing, and must be a str").then_some(()));
    // Under --watch, in the answer too.
    let r = run(&d.file("main.qwe"), &["--watch"], &[]);
    let b = upload(&r, "/b", &[("name", Some("hello.txt"), "hello")]);
    assert_eq!(b.status, 400);
    assert!(b.body.contains("`form.name` is a file (hello.txt), not a str; it is in request.files.name"), "{}", b.body);
}

// ---- Q-1262: a 304 logged and counted as one -----------------------------------------------

const ETAG: &str = r#"route GET /help/:slug = respond(200, <html><body><h1>{slug}</h1></body></html>, { etag: `"${slug}"` })
route GET /weak = respond(200, "w", { etag: "W/\"w1\"" })
route POST /help/:slug = respond(200, "posted", { etag: `"${slug}"` })
route GET /f = file("f.txt")
"#;

/// A route's etag the browser holds: the log says 304 and the metrics count a 3xx, as the answer
/// is; one it does not hold, a POST, and `file()`'s own 304 as before (FEEDBACK80.md, Q-1262;
/// branch feedback72-81).
#[test]
fn an_etag_304_is_logged_and_counted_as_a_304() {
    let d = Dir::new("etag", &[("main.qwe", ETAG), ("f.txt", "file")]);
    let r = run(&d.file("main.qwe"), &["--metrics"], &[]);
    let inm = |v: &str| support::send(r.port, "GET", "/help/hours", &[("If-None-Match", v)], "").status;
    assert_eq!(r.get("/help/hours").status, 200);
    assert_eq!(inm("\"hours\""), 304);
    assert_eq!(inm("\"other\", \"hours\""), 304);
    assert_eq!(inm("\"other\""), 200);
    assert_eq!(inm("*"), 304);
    assert_eq!(support::send(r.port, "HEAD", "/help/hours", &[("If-None-Match", "\"hours\"")], "").status, 304);
    assert_eq!(support::send(r.port, "GET", "/weak", &[("If-None-Match", "W/\"w1\"")], "").status, 304);
    assert_eq!(support::send(r.port, "POST", "/help/hours", &[("If-None-Match", "\"hours\"")], "").status, 200);
    let tag = r.get("/f").header("etag").unwrap().to_string();
    assert_eq!(support::send(r.port, "GET", "/f", &[("If-None-Match", &tag)], "").status, 304);
    let lines = eventually(10, "nine answers logged", || {
        let l: Vec<String> = r.log().lines().filter(|l| l.contains(" -> ")).map(|l| l.split(" (").next().unwrap().to_string()).collect();
        (l.len() >= 10).then_some(l)
    });
    assert_eq!(
        lines,
        [
            "GET /help/hours -> 200",
            "GET /help/hours -> 304",
            "GET /help/hours -> 304",
            "GET /help/hours -> 200",
            "GET /help/hours -> 304",
            "HEAD /help/hours -> 304",
            "GET /weak -> 304",
            "POST /help/hours -> 200",
            "GET /f -> 200",
            "GET /f -> 304",
        ]
    );
    assert_eq!((counted(&r, "2xx"), counted(&r, "3xx")), (4, 6));
}

// ---- Q-1260: SQLite whole after a clean stop ------------------------------------------------

const WAL: &str = r#"db.open("library.db")
db.exec("create table if not exists loans (id integer primary key, book text)")
let other = db.connect("other.db")
other.exec("create table if not exists notes (id integer primary key, t text)")
route POST /loan with db { db.exec("insert into loans (book) values ('a book')")?; "ok" }
route POST /note with db { other.exec("insert into notes (t) values ('ü')")?; "ok" }
route POST /late { write_file("late.begun", "x")?; sleep(400); db.exec("insert into loans (book) values ('late')")?; "late" }
route POST /spin {
    write_file("spin.begun", "x")?
    let n = 0
    for i in 0..100000000000 { n = n + i % 7 }
    str(n)
}
route GET /count with db = db.one("select count(*) as n from loans").n
"#;

/// The rows a copy of `file` alone holds, as a restore by copying would have them: `table`'s count.
fn rows_in_copy(d: &Dir, file: &str, table: &str) -> String {
    let copy = d.file("copy.db");
    let _ = std::fs::remove_file(&copy);
    std::fs::copy(d.file(file), &copy).unwrap();
    let script = d.file("count.qwe");
    std::fs::write(&script, format!("db.open(\"copy.db\")\nprint(db.one(\"select count(*) as n from {}\").n)\n", table)).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(&script).current_dir(&d.0).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    wait_for(&mut c, Duration::from_secs(60)).expect("the count did not end");
    let mut out = String::new();
    let _ = c.stdout.take().unwrap().read_to_string(&mut out);
    let _ = c.stderr.take().unwrap().read_to_string(&mut out);
    for f in ["copy.db", "copy.db-wal", "copy.db-shm", "count.qwe"] {
        let _ = std::fs::remove_file(d.file(f));
    }
    out.trim().to_string()
}

fn journals(d: &Dir) -> Vec<String> {
    let mut left: Vec<String> = std::fs::read_dir(&d.0).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with("-wal") || n.ends_with("-shm")).collect();
    left.sort();
    left
}

/// A clean stop, by SIGINT or SIGTERM, checkpoints and closes each SQLite database, the program's
/// and one it connected to: no -wal or -shm is left, and the file alone holds every row, so a
/// backup copied over it is not undone at the next start (FEEDBACK80.md, Q-1260; branch
/// feedback72-81). A request in progress at the stop is still answered first, its write in the file.
#[test]
fn a_clean_stop_leaves_each_sqlite_database_whole() {
    for signal in ["-INT", "-TERM"] {
        let d = Dir::new("wal", &[("main.qwe", WAL)]);
        let mut r = run(&d.file("main.qwe"), &[], &[]);
        for _ in 0..3 {
            assert_eq!(r.post("/loan", &[], "").body, "ok");
        }
        assert_eq!(r.post("/note", &[], "").body, "ok");
        assert!(!journals(&d).is_empty(), "WAL mode: a -wal while it runs");
        // A request in progress at the stop: answered, its row written, before the close.
        let port = r.port;
        let late = std::thread::spawn(move || support::send(port, "POST", "/late", &[], "").body);
        eventually(60, "/late began", || d.file("late.begun").exists().then_some(()));
        assert!(r.stop(signal).success());
        assert_eq!(late.join().unwrap(), "late");
        assert_eq!(journals(&d), Vec::<String>::new(), "{}", r.log());
        assert_eq!(rows_in_copy(&d, "library.db", "loans"), "4");
        assert_eq!(rows_in_copy(&d, "other.db", "notes"), "1");
        // A backup copied over the file stays the file at the next start.
        std::fs::copy(d.file("library.db"), d.file("backup.db")).unwrap();
        let r = run(&d.file("main.qwe"), &[], &[]);
        assert_eq!(r.post("/loan", &[], "").body, "ok");
        drop(r);
        std::fs::copy(d.file("backup.db"), d.file("library.db")).unwrap();
        let r = run(&d.file("main.qwe"), &[], &[]);
        assert_eq!(r.get("/count").body, "4");
    }
}

/// Tenants' databases are closed too; and a stop `--drain` cuts short, with a request still
/// running, closes those not in a transaction (FEEDBACK80.md, Q-1260; branch feedback72-81).
#[test]
fn tenants_and_a_drain_cut_short_close_their_databases() {
    let src = "tenant_from(\"localhost\")\ndb.open(\"tdata/{tenant}.db\")\nroute POST /teams/:name = new_tenant(name, () => \"made\")\nroute POST /put with db { db.exec(\"create table if not exists t (x int)\")?; db.exec(\"insert into t values (1)\")?; \"ok\" }\n";
    let d = Dir::new("walt", &[("main.qwe", src)]);
    let mut r = run(&d.file("main.qwe"), &[], &[]);
    for t in ["a", "b"] {
        assert_eq!(r.post(&format!("/teams/{}", t), &[], "").body, "made");
        assert_eq!(support::send(r.port, "POST", "/put", &[("Host", &format!("{}.localhost", t))], "").body, "ok");
    }
    assert!(r.stop("-TERM").success());
    let left: Vec<String> = std::fs::read_dir(d.file("tdata")).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.contains("-wal") || n.contains("-shm")).collect();
    assert!(left.is_empty(), "{:?}", left);
    // `--drain` over, a request still computing: the database is closed all the same.
    let d = Dir::new("wald", &[("main.qwe", WAL)]);
    let mut r = run(&d.file("main.qwe"), &["--drain", "300ms"], &[]);
    assert_eq!(r.post("/loan", &[], "").body, "ok");
    let port = r.port;
    let slow = std::thread::spawn(move || support::send(port, "POST", "/spin", &[], "").status);
    eventually(60, "/spin began", || d.file("spin.begun").exists().then_some(()));
    r.stop("-TERM");
    let _ = slow.join();
    assert!(r.log().contains("stopping with requests in progress"), "{}", r.log());
    assert_eq!(journals(&d), Vec::<String>::new(), "{}", r.log());
    assert_eq!(rows_in_copy(&d, "library.db", "loans"), "1");
}

// ---- Q-1011: no port for a program with nothing to serve over HTTP --------------------------

/// `quillo run` on `file` with `extra` (and `env`), its output in a file; the process and the file.
fn start(file: &Path, extra: &[&str], env: &[(&str, &str)]) -> (support::Guard, PathBuf) {
    static N: AtomicUsize = AtomicUsize::new(0);
    checks(file);
    let log = file.with_extension(format!("out{}", N.fetch_add(1, Ordering::SeqCst)));
    let out = std::fs::File::create(&log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(extra)
        .current_dir(file.parent().unwrap())
        .env_remove("PORT")
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(out.try_clone().unwrap())
        .stderr(out)
        .spawn()
        .expect("cannot start quillo");
    (support::Guard::new(child), log)
}

/// The TCP ports `pid` listens on, as lsof says; None without lsof.
fn tcp_listening(pid: u32) -> Option<Vec<String>> {
    let o = Command::new("lsof").args(["-nP", "-a", "-p", &pid.to_string(), "-iTCP", "-sTCP:LISTEN", "-Fn"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    // lsof exits 1 when it finds nothing: a list of none.
    Some(String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.strip_prefix('n').map(str::to_string)).collect())
}

/// The private socket `pid` serves itself through (`quillo-self-*/self.sock`), as lsof says; None
/// without lsof, or when it has none.
fn own_socket(pid: u32) -> Option<PathBuf> {
    let o = Command::new("lsof").args(["-nP", "-a", "-p", &pid.to_string(), "-U", "-Fn"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.strip_prefix('n')).find(|n| n.contains("quillo-self-") && n.ends_with("self.sock")).map(PathBuf::from)
}

/// Stops a program started by `start` as SIGTERM does, and waits for it.
fn terminate(p: &mut support::Guard) -> std::process::ExitStatus {
    let _ = Command::new("kill").args(["-TERM", &p.0.id().to_string()]).status();
    wait_for(&mut p.0, Duration::from_secs(60)).expect("SIGTERM did not stop it")
}

/// Waits for `log` to hold `what`, the process still running.
fn says(p: &mut support::Guard, log: &Path, what: &str) {
    eventually(60, &format!("{:?} said", what), || {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if let Ok(Some(status)) = p.0.try_wait() {
            panic!("it stopped ({}): {}", status, text);
        }
        text.contains(what).then_some(())
    });
}

/// A program that only ticks, with no port asked for: it runs, says nothing of a port, listens on
/// none, serves itself through a socket in a folder of its own, and a stop removes the folder
/// (FEEDBACK72.md, Q-1011; branch feedback72-81).
#[test]
fn a_program_that_only_ticks_opens_no_port() {
    let d = Dir::new("tick", &[("main.qwe", "every(100, () => { print(\"tick\") })\n")]);
    let (mut p, log) = start(&d.file("main.qwe"), &[], &[]);
    says(&mut p, &log, "tick\ntick\ntick");
    let said = std::fs::read_to_string(&log).unwrap();
    assert!(!said.contains("listening") && !said.contains("cannot listen") && !said.contains("in use"), "{}", said);
    if let Some(ports) = tcp_listening(p.0.id()) {
        assert!(ports.is_empty(), "{:?}", ports);
    }
    let own = own_socket(p.0.id());
    assert!(terminate(&mut p).success());
    if let Some(own) = own {
        assert!(!own.parent().unwrap().exists(), "{} left behind", own.display());
    }
    let _ = std::fs::remove_file(&log);
}

/// `$PORT` asks for a port, as a platform that sets it probes `/_quillo/health` there (FEEDBACK72.md,
/// Q-1011, review; branch feedback72-81).
#[test]
fn port_in_the_environment_asks_for_one() {
    let d = Dir::new("tick-port", &[("main.qwe", "every(100, () => null)\n")]);
    let port = support::free_port();
    let (mut p, log) = start(&d.file("main.qwe"), &[], &[("PORT", &port.to_string())]);
    says(&mut p, &log, &format!("0 routes listening on http://127.0.0.1:{}", port));
    assert_eq!(support::send(port, "GET", "/_quillo/health", &[], "").status, 200);
    assert!(terminate(&mut p).success());
    let _ = std::fs::remove_file(&log);
}

/// A temporary folder too long for a socket's path: a short folder under /tmp instead, removed on
/// stop, where the program stopped at once (FEEDBACK72.md, Q-1011, review; branch feedback72-81).
#[cfg(unix)]
#[test]
fn a_long_temporary_folder_still_runs_without_a_port() {
    let d = Dir::new("tick-tmp", &[("main.qwe", "every(100, () => { print(\"tick\") })\n")]);
    let long = d.file(&"t".repeat(90));
    std::fs::create_dir_all(&long).unwrap();
    let (mut p, log) = start(&d.file("main.qwe"), &[], &[("TMPDIR", long.to_str().unwrap())]);
    says(&mut p, &log, "tick\ntick");
    if let Some(ports) = tcp_listening(p.0.id()) {
        assert!(ports.is_empty(), "{:?}", ports);
    }
    let own = own_socket(p.0.id());
    if let Some(own) = &own {
        assert!(own.starts_with("/tmp") || own.starts_with("/private/tmp"), "{}", own.display());
    }
    assert!(terminate(&mut p).success());
    if let Some(own) = own {
        assert!(!own.parent().unwrap().exists(), "{} left behind", own.display());
    }
    assert!(std::fs::read_dir(&long).unwrap().next().is_none(), "nothing made in the long folder");
    let _ = std::fs::remove_file(&log);
}

/// A UDP echo alone answers its packets with no port; given `--port`, it listens there too, and
/// `/_quillo/health` answers; a job alone takes no port either (FEEDBACK72.md, Q-1011; branch
/// feedback72-81).
#[test]
fn udp_alone_answers_without_a_port_and_with_one_given() {
    let udp_port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let src = format!("udp.listen({}, (p) => udp.reply(p, `echo ${{p.text ?? \"\"}}`))\n", udp_port);
    let d = Dir::new("udp", &[("main.qwe", &src)]);
    let echo = || {
        let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        s.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        let mut b = [0u8; 64];
        eventually(60, "an echo", || {
            s.send_to("ü ✓".as_bytes(), ("127.0.0.1", udp_port)).ok()?;
            let n = s.recv(&mut b).ok()?;
            Some(String::from_utf8_lossy(&b[..n]).into_owned())
        })
    };
    let (mut p, log) = start(&d.file("main.qwe"), &[], &[]);
    assert_eq!(echo(), "echo ü ✓");
    if let Some(ports) = tcp_listening(p.0.id()) {
        assert!(ports.is_empty(), "{:?}", ports);
    }
    assert!(!std::fs::read_to_string(&log).unwrap().contains("listening"));
    let own = own_socket(p.0.id());
    assert!(terminate(&mut p).success());
    if let Some(own) = own {
        assert!(!own.parent().unwrap().exists(), "{} left behind", own.display());
    }
    let _ = std::fs::remove_file(&log);
    // `--port` asks for one.
    let r = run(&d.file("main.qwe"), &[], &[]);
    assert_eq!(echo(), "echo ü ✓");
    assert_eq!(r.get("/_quillo/health").status, 200);
    assert!(r.log().contains("0 routes listening on http://127.0.0.1:"), "{}", r.log());
    drop(r);
    let d = Dir::new("job", &[("main.qwe", "fn work() = print(\"ran\")\nschedule(\"1h\", work)\nprint(\"up\")\n")]);
    let (mut p, log) = start(&d.file("main.qwe"), &[], &[]);
    says(&mut p, &log, "up");
    // Running, not ended: a job keeps it, with no port.
    let own = eventually(60, "its own socket", || own_socket(p.0.id()).or_else(|| tcp_listening(p.0.id()).is_none().then(PathBuf::new)));
    assert!(p.0.try_wait().unwrap().is_none(), "{}", std::fs::read_to_string(&log).unwrap());
    if let Some(ports) = tcp_listening(p.0.id()) {
        assert!(ports.is_empty(), "{:?}", ports);
    }
    assert!(terminate(&mut p).success());
    if own.parent().is_some() {
        assert!(!own.parent().unwrap().exists(), "{} left behind", own.display());
    }
    let _ = std::fs::remove_file(&log);
}

/// What does serve over HTTP still opens its port with none asked for (no `--port`, no `$PORT`): a
/// channel's handler alone (a page's WebSocket); and with `--metrics` asked; it listens on the
/// default port, or says the default port is taken. A script still ends on its own, and so does a
/// program with only a public folder, which no route serves (FEEDBACK72.md, Q-1011; branch
/// feedback72-81).
#[test]
fn what_serves_over_http_listens_and_a_script_ends() {
    for (name, src, extra) in [("chan", "channel.on(\"room\", (msg, peer) => null)\n", &[][..]), ("metrics", "every(100, () => null)\n", &["--metrics"][..])] {
        let d = Dir::new(name, &[("main.qwe", src)]);
        let (mut p, log) = start(&d.file("main.qwe"), extra, &[]);
        let said = eventually(60, "the HTTP port opened, or said taken", || {
            let t = std::fs::read_to_string(&log).unwrap_or_default();
            (t.contains("listening on http://127.0.0.1:8080") || t.contains("cannot listen on 127.0.0.1:8080")).then_some(t)
        });
        if said.contains("listening") {
            assert!(terminate(&mut p).success(), "{}", said);
        }
        let _ = std::fs::remove_file(&log);
    }
    for (name, files) in [("ends", vec![("main.qwe", "print(\"done\")\n")]), ("public", vec![("main.qwe", "print(\"done\")\n"), ("public/index.html", "<p>hi</p>")])] {
        let d = Dir::new(name, &files);
        let (mut p, log) = start(&d.file("main.qwe"), &[], &[]);
        let status = wait_for(&mut p.0, Duration::from_secs(60)).expect("the script did not end");
        let said = std::fs::read_to_string(&log).unwrap();
        assert!(status.success() && said.contains("done") && !said.contains("listening"), "{}: {}", name, said);
        let _ = std::fs::remove_file(&log);
    }
}

// ---- Q-1265 and Q-1266: WebTransport at a stop, found dead, and at localhost ----------------

const CHANNELS: &str = r#"let log = []
let GEN = uuid()
channel.on("room", (msg, peer) => {
    log = log + [msg.text]
    channel.broadcast("room", { text: msg.text }, { reliable: true })
})
route GET /log = log
route GET /gen = GEN
route GET /mark/:m { log = log + [m]; "ok" }
route GET /peers = str(len(channel.peers("room")))
route GET /bcast/:t = str(channel.broadcast("room", { text: t }, { reliable: true }))
"#;

const PAGE: &str = r#"component C() {
    state got = []
    let room = channel.open("room", (m) => { got = got + [m.text] })
    <div>
        <button id="two" @click={() => { room.send({ text: "two-a" }); room.send({ text: "two-b ✓" }) }}>two</button>
        <ul id="got">{for t in got => <li>{t}</li>}</ul>
    </div>
}
route GET / = <html><body><C/></body></html>
"#;

/// The page's side (tests/browser/run.js runs it): `MODE` is "clean" (the server stopped with
/// SIGTERM and started again) or "via" (how it connected).
const PROBE: &str = r##"(async () => {
  const out = {};
  const tick = (ms = 50) => new Promise((r) => setTimeout(r, ms));
  const until = async (f, ms = 30000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) { const v = await f(); if (v) return v; await tick(); }
    return null;
  };
  const get = async (p) => { try { const r = await fetch(p); return r.ok ? await r.text() : null; } catch (e) { return null; } };
  const items = () => [...document.querySelectorAll("#got li")].map((l) => l.textContent);
  // When the page asked for tickets, from now on.
  const tickets = [];
  const was = window.fetch;
  window.fetch = (u, i) => { if (String(u).includes("/_quillo/wt-ticket")) tickets.push(Date.now()); return was(u, i); };
  try {
    out.via = await until(() => (window[Symbol.for("quillo")] || {}).via);
    if (MODE !== "via") {
      await until(async () => (await get("/peers")) === "1");
      const g1 = await get("/gen");
      document.querySelector("#two").click();
      out.heard = !!(await until(() => items().includes("two-b ✓")));
      await get("/mark/ready");
      out.down = !!(await until(async () => (await get("/gen")) === null, 120000));
      const down = Date.now();
      out.back = !!(await until(async () => { const g = await get("/gen"); return g !== null && g !== g1; }, 120000));
      out.rejoined = !!(await until(async () => (await get("/peers")) === "1", 120000));
      // The first ticket asked for once the server stopped: the page found its session closed.
      const t = tickets.find((x) => x >= down);
      out.noticed = t !== undefined && t - down;
      await get("/bcast/after");
      out.heardAfter = !!(await until(() => items().includes("after")));
      out.log = JSON.parse(await get("/log"));
      out.via2 = (window[Symbol.for("quillo")] || {}).via;
    }
    out.errors = window.__quilloErrors || [];
  } catch (e) {
    out.exception = String((e && e.stack) || e);
  }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
"##;

fn chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ];
    let node = Command::new("node").args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"]).output().map(|o| o.status.success()).unwrap_or(false);
    let c = candidates.iter().map(PathBuf::from).find(|p| p.exists()).filter(|_| node);
    if c.is_none() {
        eprintln!("skipping: needs Chrome and Node 22 or later");
    }
    c
}

// One Chrome at a time: several at once overload small CI machines.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Headless Chrome on `url`, running the probe in `mode`, as tests/browser/run.js drives it.
struct Page {
    node: support::Guard,
    out: PathBuf,
    err: PathBuf,
    profile: PathBuf,
}

impl Page {
    fn open(chrome: &Path, d: &Dir, url: &str, mode: &str) -> Page {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let script = d.file(&format!("probe{}.js", n));
        std::fs::write(&script, PROBE.replace("MODE", &serde_json::to_string(mode).unwrap())).unwrap();
        let profile = std::env::temp_dir().join(format!("quillo-chrome-f7281s-{}-{}", std::process::id(), n));
        let (out, err) = (d.file(&format!("page{}.out", n)), d.file(&format!("page{}.err", n)));
        let child = Command::new("node")
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/run.js"))
            .arg(chrome)
            .arg(url)
            .arg(&profile)
            // Generous: CI machines can be several times slower than a laptop.
            .arg("150000")
            .arg(&script)
            .stdout(std::fs::File::create(&out).unwrap())
            .stderr(std::fs::File::create(&err).unwrap())
            .spawn()
            .expect("node");
        Page { node: support::Guard::new(child), out, err, profile }
    }

    fn result(mut self) -> serde_json::Value {
        let status = wait_for(&mut self.node.0, Duration::from_secs(200)).expect("the page did not finish");
        let out = std::fs::read_to_string(&self.out).unwrap_or_default();
        let err = std::fs::read_to_string(&self.err).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&self.profile);
        assert!(status.success(), "{}{}", out, err);
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("{}: {}{}", e, out, err))
    }
}

/// The channels' program and its page, served on a port the test names (to be started again
/// there), once the page has said it is ready.
fn channels_app(name: &str) -> (Dir, u16) {
    let d = Dir::new(name, &[("main.qwe", &format!("{}{}", CHANNELS, PAGE))]);
    (d, support::free_port())
}

fn serve_at(d: &Dir, port: u16) -> Run {
    let r = run(&d.file("main.qwe"), &["--port", &port.to_string()], &[]);
    assert!(support::listener_is(port, r.child.id()), "another process listens on {}", port);
    r
}

fn waits_for_ready(r: &Run) {
    eventually(60, "the page said it is ready", || r.get("/log").body.contains("ready").then_some(()));
}

/// A clean stop closes the page's WebTransport session saying so: the page asks for a ticket as
/// soon as the server has gone, as from a WebSocket's close, not 20 s on when the keep-alive gives
/// up; it hears the new server, and sends nothing again (FEEDBACK80.md, Q-1265; branch
/// feedback72-81).
#[test]
fn a_page_finds_the_server_again_at_once_after_a_clean_stop() {
    let Some(chrome) = chrome() else { return };
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (d, port) = channels_app("wt-clean");
    let mut first = serve_at(&d, port);
    let page = Page::open(&chrome, &d, &format!("http://127.0.0.1:{}/", port), "clean");
    waits_for_ready(&first);
    let stopping = Instant::now();
    assert!(first.stop("-TERM").success());
    // How long the stop took: the page saw it begin (its requests refused), and its session closed
    // at its end; what is left is the page's own half-second wait before it connects again, and
    // its timers, which a slower build of the server does not slow.
    let stop = stopping.elapsed();
    let second = serve_at(&d, port);
    let r = page.result();
    // Well before the keep-alive's 20 s, which is what found the session gone before.
    let noticed = r["noticed"].as_u64().unwrap_or_else(|| panic!("{}", r));
    assert!(Duration::from_millis(noticed) < stop + Duration::from_secs(8), "{} (the stop took {:?})", r, stop);
    assert_eq!(r["via"], "webtransport", "{}", r);
    for k in ["heard", "down", "back", "rejoined", "heardAfter"] {
        assert_eq!(r[k], true, "{}: {}", k, r);
    }
    assert_eq!(r["log"], serde_json::json!([]), "a line heard was sent again: {}", r);
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);
    drop(second);
}

/// A page at http://localhost takes WebTransport, which it dials at [::1] (FEEDBACK80.md, Q-1266;
/// branch feedback72-81); at 127.0.0.1 as before.
#[test]
fn a_page_at_localhost_takes_webtransport() {
    let Some(chrome) = chrome() else { return };
    if std::net::UdpSocket::bind("[::1]:0").is_err() {
        eprintln!("no IPv6 loopback here: skipped");
        return;
    }
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (d, port) = channels_app("wt-localhost");
    let r = serve_at(&d, port);
    for host in ["localhost", "127.0.0.1"] {
        let got = Page::open(&chrome, &d, &format!("http://{}:{}/", host, port), "via").result();
        assert_eq!(got, serde_json::json!({ "via": "webtransport", "errors": [] }), "{}", host);
    }
    drop(r);
}

/// Where WebTransport listens, by lsof: UDP addresses of `pid`.
fn udp_listening(pid: u32) -> Option<Vec<String>> {
    let o = Command::new("lsof").args(["-nP", "-a", "-p", &pid.to_string(), "-iUDP", "-Fn"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    let mut v: Vec<String> = String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.strip_prefix('n').map(str::to_string)).collect();
    v.sort();
    Some(v)
}

/// On 127.0.0.1, WebTransport listens on [::1] as well, and no wider (FEEDBACK80.md, Q-1266;
/// branch feedback72-81).
#[test]
fn webtransport_listens_on_both_loopbacks_and_no_wider() {
    if std::net::UdpSocket::bind("[::1]:0").is_err() {
        eprintln!("no IPv6 loopback here: skipped");
        return;
    }
    let d = Dir::new("wt-where", &[("main.qwe", CHANNELS)]);
    let r = run(&d.file("main.qwe"), &[], &[]);
    let t: serde_json::Value = serde_json::from_str(&r.post("/_quillo/wt-ticket", &[("Sec-Fetch-Site", "same-origin")], "").body).unwrap();
    let port = t["port"].as_u64().unwrap();
    if let Some(at) = udp_listening(r.child.id()) {
        assert_eq!(at, [format!("127.0.0.1:{}", port), format!("[::1]:{}", port)], "{:?}", at);
    }
    drop(r);
}

// ---- Q-1265: the server's side, with a WebTransport client of the test's own ----------------

#[cfg(feature = "realtime")]
mod with_a_client {
    use super::*;
    use wtransport::{ClientConfig, Connection, Endpoint};

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
    }

    /// A session with a ticket of the program's, at `host`, asked again only when wtransport's own
    /// driver dropped the attempt before Quillo was asked (see tests/wt_edges.rs, Q-1124).
    async fn connect(r: &Run, host: &str) -> Connection {
        let mut last = String::new();
        for _ in 0..5 {
            let t: serde_json::Value = serde_json::from_str(&r.post("/_quillo/wt-ticket", &[("Sec-Fetch-Site", "same-origin")], "").body).unwrap();
            let hex = t["hash"].as_str().unwrap();
            let bytes: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()).collect();
            let hash = wtransport::tls::Sha256Digest::new(bytes.try_into().unwrap());
            let c = ClientConfig::builder().with_bind_default().with_server_certificate_hashes([hash]).build();
            let url = format!("https://{}:{}/_quillo/wt?t={}", host, t["port"], t["ticket"].as_str().unwrap());
            match tokio::time::timeout(Duration::from_secs(10), Endpoint::client(c).unwrap().connect(url)).await {
                Ok(Ok(conn)) => return conn,
                Ok(Err(e)) => last = e.to_string(),
                Err(_) => last = "no answer within 10 s".into(),
            }
        }
        panic!("no session: {}", last)
    }

    fn framed(t: &str) -> Vec<u8> {
        let mut f = (t.len() as u32).to_be_bytes().to_vec();
        f.extend_from_slice(t.as_bytes());
        f
    }

    async fn read_framed(s: &mut wtransport::RecvStream) -> String {
        let mut len = [0u8; 4];
        s.read_exact(&mut len).await.unwrap();
        let mut b = vec![0u8; u32::from_be_bytes(len) as usize];
        s.read_exact(&mut b).await.unwrap();
        String::from_utf8(b).unwrap()
    }

    /// A clean stop closes each session with HTTP/3's "no error" (0x100) and "server stopping",
    /// where the session lived on till the keep-alive's 20 s gave up on it, a time-out
    /// (FEEDBACK80.md, Q-1265; branch feedback72-81).
    #[test]
    fn a_clean_stop_closes_each_session_saying_the_server_is_going() {
        let d = Dir::new("wtc-close", &[("main.qwe", CHANNELS)]);
        let mut r = run(&d.file("main.qwe"), &[], &[]);
        rt().block_on(async {
            let a = connect(&r, "127.0.0.1").await;
            let b = connect(&r, "127.0.0.1").await;
            let (mut out, mut inn) = a.open_bi().await.unwrap().await.unwrap();
            out.write_all(&framed("[\"j\",\"room\"]")).await.unwrap();
            assert_eq!(read_framed(&mut inn).await, "[\"j\",\"room\",true]");
            let _ = Command::new("kill").args(["-TERM", &r.child.id().to_string()]).status();
            for conn in [a, b] {
                match tokio::time::timeout(Duration::from_secs(60), conn.closed()).await.expect("never closed") {
                    wtransport::error::ConnectionError::ApplicationClosed(c) => {
                        assert_eq!((c.code().into_inner(), c.reason()), (0x100, &b"server stopping"[..]));
                    }
                    other => panic!("{:?}", other),
                }
            }
        });
        assert!(wait_for(&mut r.child, Duration::from_secs(30)).is_some_and(|s| s.success()));
    }

    /// A session at [::1] (where a page at localhost dials), and at 127.0.0.1 (FEEDBACK80.md,
    /// Q-1266; branch feedback72-81).
    #[test]
    fn a_session_opens_at_either_loopback() {
        if std::net::UdpSocket::bind("[::1]:0").is_err() {
            eprintln!("no IPv6 loopback here: skipped");
            return;
        }
        let d = Dir::new("wtc-both", &[("main.qwe", CHANNELS)]);
        let r = run(&d.file("main.qwe"), &[], &[]);
        rt().block_on(async {
            for host in ["[::1]", "127.0.0.1"] {
                let conn = connect(&r, host).await;
                let (mut out, mut inn) = conn.open_bi().await.unwrap().await.unwrap();
                out.write_all(&framed("[\"j\",\"room\"]")).await.unwrap();
                assert_eq!(read_framed(&mut inn).await, "[\"j\",\"room\",true]", "{}", host);
            }
        });
    }
}
