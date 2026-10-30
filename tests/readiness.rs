//! `/_quillo/ready`: whether what a program needs is there, its database, its
//! services wherever they run, and the checks it adds.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::{free_port, listening, send, temp_dir, Guard};

const SECRET: &str = "a-test-secret-for-readiness";

fn get(it: &mut Interp, url: &str) -> HttpResponse {
    it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers: Vec::new(), body: Vec::new(), remote: "127.0.0.1".into() })
}

fn ready(it: &mut Interp) -> (u16, serde_json::Value) {
    let r = get(it, "/_quillo/ready");
    assert_eq!(r.header("cache-control"), Some("no-store"));
    (r.status, serde_json::from_str(&r.text()).unwrap())
}

fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = temp_dir(&format!("ready-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

fn load(d: &Path) -> Interp {
    let mut it = Interp::new(d.to_str().unwrap());
    it.load_file(&d.join("app.qwe")).unwrap_or_else(|e| panic!("{}", e));
    it
}

fn start(file: &Path, port: u16, args: &[&str]) -> Guard {
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(args)
        .env("QUILLO_SERVICE_SECRET", SECRET)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let g = Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    g
}

fn until(mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn a_programs_own_checks_count() {
    let d = folder("own", &[("app.qwe", "let up = true\nready(\"ledger\", () => up)\nready(\"ledger\", () => up)\nready(\"always\", () => true)\nready(\"odd\", () => 1)\nroute GET /down { up = false; \"down\" }\n")]);
    let mut it = load(&d);
    let (status, j) = ready(&mut it);
    assert_eq!(status, 503);
    assert_eq!(j["checks"]["ledger"], "ready");
    assert_eq!(j["checks"]["odd"], "not ready");
    get(&mut it, "/down");
    assert_eq!(ready(&mut it).1["checks"]["ledger"], "not ready");
    let mut bad = Interp::new(d.to_str().unwrap());
    let Err(e) = bad.load_source("ready(\"x\")", "x.qwe") else { panic!("loaded") };
    assert!(e.to_string().contains("`ready` takes a name and a function"), "{}", e);
}

#[test]
fn a_database_that_answers_is_ready() {
    let d = folder("db", &[("app.qwe", "db.open(\"a.db\")\nroute GET / = \"hi\"\n")]);
    let mut it = load(&d);
    let (status, j) = ready(&mut it);
    assert_eq!((status, j["checks"]["database"].as_str()), (200, Some("ready")));
    assert_eq!(j["ready"], true);
}

#[test]
fn a_service_in_this_process_is_asked_as_this_one_is() {
    let d = folder("local", &[("app.qwe", "service billing = \"billing.qwe\"\n"), ("billing.qwe", "let up = false\nready(\"books\", () => up)\nserver fn open() { up = true }\n")]);
    let mut it = load(&d);
    let (status, j) = ready(&mut it);
    assert_eq!(status, 503);
    assert_eq!(j["checks"]["service billing"]["checks"]["books"], "not ready");
}

/// Services inside services are asked only so deep.
#[test]
fn a_long_chain_of_services_is_asked_only_so_deep() {
    let names: Vec<String> = (0..11).map(|i| format!("s{}", i)).collect();
    let mut files: Vec<(String, String)> = vec![("app.qwe".into(), "service s0 = \"s0.qwe\"\n".into())];
    for i in 0..10 {
        files.push((format!("{}.qwe", names[i]), format!("service {} = \"{}.qwe\"\nserver fn f() = 1\n", names[i + 1], names[i + 1])));
    }
    files.push(("s10.qwe".into(), "ready(\"deepest\", () => false)\nserver fn f() = 1\n".into()));
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let d = folder("chain", &refs);
    let mut it = load(&d);
    let (status, j) = ready(&mut it);
    // The check too deep to reach is not asked.
    assert_eq!(status, 200, "{}", j);
    let mut at = &j;
    for i in 0..8 {
        at = &at["checks"][format!("service s{}", i)];
    }
    assert_eq!(at["checks"]["service s8"], "ready", "{}", j);
}

#[test]
fn a_service_that_is_neither_here_nor_anywhere_is_not_loaded() {
    let d = folder("nowhere", &[("app.qwe", "service ghost = \"ghost.qwe\"\n"), ("ghost.qwe", "server fn f() = 1\n")]);
    std::env::set_var("QUILLO_SERVICE_GHOST", "http://127.0.0.1:9");
    let mut it = load(&d);
    std::env::remove_var("QUILLO_SERVICE_GHOST");
    let (status, j) = ready(&mut it);
    assert_eq!(status, 503);
    assert_eq!(j["checks"]["service ghost"], "not loaded");
}

#[test]
fn a_service_elsewhere_is_asked_at_each_of_its_addresses() {
    let d = folder("remote", &[
        ("app.qwe", "service /billing = \"billing.qwe\"\nroute GET / = \"hi\"\n"),
        ("billing.qwe", "route GET /page = \"from billing\"\nserver fn f() = 1\n"),
    ]);
    let (up, down, app) = (free_port(), free_port(), free_port());
    let _billing = start(&d.join("billing.qwe"), up, &["--as-service"]);
    let url = format!("billing=http://127.0.0.1:{},http://127.0.0.1:{}", up, down);
    let _app = start(&d.join("app.qwe"), app, &["--service-url", &url]);
    let a = send(app, "GET", "/_quillo/ready", &[], "");
    assert_eq!(a.status, 200, "{}", a.body);
    let j: serde_json::Value = serde_json::from_str(&a.body).unwrap();
    let at = &j["checks"]["service billing"];
    assert_eq!(at[format!("http://127.0.0.1:{}", up)], "ready", "{}", j);
    assert_eq!(at[format!("http://127.0.0.1:{}", down)], "down", "{}", j);
    // Asked again at once: from what was kept.
    assert_eq!(send(app, "GET", "/_quillo/ready", &[], "").status, 200);
    // Its readiness at its own prefix, for a gateway.
    let a = send(app, "GET", "/billing/_quillo/ready", &[], "");
    assert_eq!((a.status, a.header("cache-control")), (200, Some("no-store")));
    let j: serde_json::Value = serde_json::from_str(&a.body).unwrap();
    assert_eq!((j["ready"].as_bool(), j["service"].as_str()), (Some(true), Some("billing")), "{}", a.body);
    assert_eq!(send(app, "GET", "/billing/page", &[], "").body, "from billing");
}

#[test]
fn a_services_answer_is_kept_a_moment() {
    let d = folder("kept", &[("app.qwe", "service /billing = \"billing.qwe\"\nroute GET / = \"hi\"\n"), ("billing.qwe", "server fn f() = 1\n")]);
    let (up, app) = (free_port(), free_port());
    let _app = start(&d.join("app.qwe"), app, &["--service-url", &format!("billing=http://127.0.0.1:{}", up)]);
    assert_eq!(send(app, "GET", "/_quillo/ready", &[], "").status, 503);
    let _billing = start(&d.join("billing.qwe"), up, &["--as-service"]);
    assert!(until(|| send(app, "GET", "/_quillo/ready", &[], "").status == 200));
}

#[test]
fn no_address_answering_is_not_ready() {
    let d = folder("all-down", &[("app.qwe", "service /billing = \"billing.qwe\"\nroute GET / = \"hi\"\n"), ("billing.qwe", "server fn f() = 1\n")]);
    let (down, app) = (free_port(), free_port());
    let _app = start(&d.join("app.qwe"), app, &["--service-url", &format!("billing=http://127.0.0.1:{}", down)]);
    assert_eq!(send(app, "GET", "/_quillo/ready", &[], "").status, 503);
    let a = send(app, "GET", "/billing/_quillo/ready", &[], "");
    assert_eq!(a.status, 503);
    assert!(a.body.contains("\"ready\":false"), "{}", a.body);
}

#[test]
fn a_copy_that_joined_is_asked_with_the_service_here() {
    let d = folder("copies", &[
        ("app.qwe", "service billing = \"billing.qwe\"\nroute GET / = \"hi\"\n"),
        ("billing.qwe", "server fn f() = 1\nroute GET /page = \"copy\"\n"),
    ]);
    let (app, copy) = (free_port(), free_port());
    let _app = start(&d.join("app.qwe"), app, &[]);
    let at = format!("http://127.0.0.1:{}", app);
    let _copy = start(&d.join("billing.qwe"), copy, &["--as-service", "--join", &at]);
    let mut seen = String::new();
    assert!(
        until(|| {
            seen = send(app, "GET", "/_quillo/ready", &[], "").body;
            seen.contains("\"copies\"") && seen.contains(&format!("127.0.0.1:{}\":\"ready\"", copy))
        }),
        "{}",
        seen
    );
}
