//! What a saga leans on (FEEDBACK23.md, Q-182 to Q-188): `.later` that goes with its transaction
//! on every database, is delivered wherever it was made, is kept until the function has really
//! run, and can be asked after; and calls that stay safe across a crash.

mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::{send, serve, Server};

const SECRET: &str = "a secret both sides of these calls share";

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback23-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

fn eventually(secs: u64, what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn get(s: &Server, path: &str) -> String {
    send(s.port, "GET", path, &[], "").body
}

const MAILER: &str = "let seen = []\nserver fn send(who: str) {\n    seen = [...seen, who]\n    who\n}\nserver fn seen_so_far() = seen | join(\",\")\n";

/// Q-182: a program whose transactions call `.later`, on database `target`.
fn transactions(name: &str, target: &str) -> PathBuf {
    let front = format!(
        r#"
db.open("{}")
db.exec("create table if not exists orders (id bigint primary key)")
service mailer = "mailer.qwe"

route GET /rollback/:id {{
    let n = int(id)
    let r = db.transaction(() => {{
        db.exec("insert into orders (id) values (?)", n)
        mailer.later.send("rolled back")
        db.exec("insert into orders (id) values (?)", n + 1)
        err("change of mind")
    }})
    is_err(r)
}}

route GET /commit/:id {{
    db.transaction(() => {{
        later.status("nothing like it")
        db.exec("insert into orders (id) values (?)", int(id))
        mailer.later.send(id)
    }})
    "committed"
}}

route GET /orders = len(db.query("select id from orders"))
route GET /sent = mailer.seen_so_far()
"#,
        target
    );
    dir(name, &[("mailer.qwe", MAILER), ("front.qwe", &front)]).join("front.qwe")
}

fn goes_with_its_transaction(name: &str, target: &str) {
    let s = serve(&transactions(name, target), &[]);
    // The first `.later` this database sees, its table not there yet.
    assert_eq!(get(&s, "/rollback/1"), "true", "{}", name);
    assert_eq!(get(&s, "/orders"), "0", "{}: the rollback undoes what came before and after `.later`", name);
    get(&s, "/commit/10");
    eventually(15, "a committed call was not delivered", || get(&s, "/sent") == "10");
    assert_eq!(get(&s, "/orders"), "1", "{}", name);
    // With the table there, the same.
    assert_eq!(get(&s, "/rollback/20"), "true", "{}", name);
    assert_eq!(get(&s, "/orders"), "1", "{}", name);
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(get(&s, "/sent"), "10", "{}: a call rolled back is not delivered", name);
}

#[test]
fn later_goes_with_its_transaction_on_sqlite() {
    goes_with_its_transaction("tx-sqlite", "orders.db");
}

#[test]
fn later_goes_with_its_transaction_on_postgres() {
    let Some(pg) = support::postgres("feedback23-tx") else { return };
    goes_with_its_transaction("tx-postgres", &pg.url);
}

#[test]
fn later_goes_with_its_transaction_on_mariadb() {
    let Some(m) = support::mariadb("feedback23-tx") else { return };
    goes_with_its_transaction("tx-mariadb", &m.url("later_tx"));
}

/// Q-183: a call made by a service, in one process, whichever of them has a database.
fn delivered_in_one_process(name: &str, front_db: bool, svc_db: bool) {
    let inner = "let seen = []\nserver fn hit(x: str) {\n    seen = [...seen, x]\n    \"ok\"\n}\nserver fn hits() = seen | join(\",\")\n";
    let open = |file: &str, on: bool| if on { format!("db.open(\"{}\")\n", file) } else { String::new() };
    let svc = format!("{}service inner = \"inner.qwe\"\nserver fn go() = inner.later.hit(\"x\")\nserver fn seen_there() = inner.hits()\n", open("svc.db", svc_db));
    let front = format!("{}service svc = \"svc.qwe\"\nroute GET /go = svc.go()\nroute GET /seen = svc.seen_there()\n", open("main.db", front_db));
    let d = dir(name, &[("inner.qwe", inner), ("svc.qwe", &svc), ("main.qwe", &front)]);
    let s = serve(&d.join("main.qwe"), &[]);
    assert_eq!(get(&s, "/go").len(), 24, "{}", name);
    eventually(15, &format!("{}: a service's call was not delivered", name), || get(&s, "/seen") == "x");
}

#[test]
fn a_call_a_service_makes_is_delivered_in_one_process() {
    delivered_in_one_process("one-both", true, true);
    delivered_in_one_process("one-service", false, true);
    delivered_in_one_process("one-app", true, false);
    delivered_in_one_process("one-neither", false, false);
}

/// Q-184: a stand-in for a service, answering each function as told.
struct Stand {
    port: u16,
    healed: Arc<AtomicBool>,
    hits: Arc<AtomicUsize>,
}

fn stand_in() -> Stand {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let healed = Arc::new(AtomicBool::new(false));
    let hits = Arc::new(AtomicUsize::new(0));
    let (h, n) = (healed.clone(), hits.clone());
    std::thread::spawn(move || {
        for c in listener.incoming().flatten() {
            let (h, n) = (h.clone(), n.clone());
            std::thread::spawn(move || {
                let mut r = BufReader::new(c);
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    let f = line.split(' ').nth(1).unwrap_or("").rsplit('/').next().unwrap_or("").to_string();
                    let mut length = 0;
                    loop {
                        let mut l = String::new();
                        r.read_line(&mut l).unwrap();
                        if l.trim().is_empty() {
                            break;
                        }
                        if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
                            length = v.trim().parse().unwrap();
                        }
                    }
                    r.read_exact(&mut vec![0; length]).unwrap();
                    n.fetch_add(1, Ordering::SeqCst);
                    let (status, body) = match f.as_str() {
                        "bad" => (400, r#"{"error":"not its arguments"}"#.to_string()),
                        "said_no" => (200, r#"{"err":"declined","own":true}"#.to_string()),
                        _ if h.load(Ordering::SeqCst) => (200, format!(r#"{{"ok":"{} done"}}"#, f)),
                        f => (f[1..].parse().unwrap_or(500), r#"{"error":"not now"}"#.to_string()),
                    };
                    let head = format!("HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n", status, body.len());
                    let _ = r.get_mut().write_all(format!("{}{}", head, body).as_bytes());
                }
            });
        }
    });
    Stand { port, healed, hits }
}

#[test]
fn only_an_answer_finishes_a_call_made_later() {
    let stand = stand_in();
    let mailer = "server fn s401() = 1\nserver fn s404() = 1\nserver fn s409() = 1\nserver fn s500() = 1\nserver fn bad() = 1\nserver fn said_no() = 1\n";
    let front = r#"
db.open("front.db")
service mailer = "mailer.qwe"
route GET /send/:f = match f {
    "s401" => mailer.later.s401()
    "s404" => mailer.later.s404()
    "s409" => mailer.later.s409()
    "s500" => mailer.later.s500()
    "bad" => mailer.later.bad()
    _ => mailer.later.said_no()
}
route GET /status/:id = json.stringify(later.status(id))
route GET /kept = len(db.query("select id from _quillo_later"))
"#;
    let d = dir("answers", &[("mailer.qwe", mailer), ("front.qwe", front)]);
    let url = format!("http://127.0.0.1:{}", stand.port);
    let s = serve(&d.join("front.qwe"), &[("QUILLO_SERVICE_MAILER", &url), ("QUILLO_SERVICE_SECRET", SECRET)]);
    let status = |id: &str| serde_json::from_str::<serde_json::Value>(&get(&s, &format!("/status/{}", id))).unwrap();
    assert_eq!(status("nosuchcall"), serde_json::Value::Null);

    let waits: Vec<(&str, String)> = ["s401", "s404", "s409", "s500"].iter().map(|f| (*f, get(&s, &format!("/send/{}", f)))).collect();
    let bad = get(&s, "/send/bad");
    let no = get(&s, "/send/said_no");
    eventually(15, "the calls were not tried", || waits.iter().all(|(_, id)| status(id)["tries"].as_i64().unwrap_or(0) >= 1));
    for (f, id) in &waits {
        let got = status(id);
        assert_eq!(got["state"], "waiting", "{}: {}", f, got);
        assert!(got["error"]["error"].as_str().unwrap().contains(&f[1..]), "{}: {}", f, got);
    }
    // It cannot succeed: marked, not tried again, and its row kept.
    eventually(15, "a 400 was not marked", || status(&bad)["state"] == "failed");
    assert!(status(&bad)["error"]["error"].as_str().unwrap().contains("not its arguments"), "{}", status(&bad));
    assert_eq!(get(&s, "/kept"), "5");
    // The function answered, with a no: finished, and the caller can learn of it.
    eventually(15, "an error value did not finish the call", || status(&no)["state"] == "done");
    assert_eq!(status(&no)["result"], serde_json::json!({ "error": "declined" }), "{}", status(&no));

    stand.healed.store(true, Ordering::SeqCst);
    let hits = stand.hits.load(Ordering::SeqCst);
    eventually(15, "a call was not delivered once the service was mended", || waits.iter().all(|(_, id)| status(id)["state"] == "done"));
    assert_eq!(status(&waits[0].1)["result"], "s401 done");
    assert!(stand.hits.load(Ordering::SeqCst) >= hits + 4);
    assert_eq!(get(&s, "/kept"), "1", "only the call that can never work stays");
}

/// A service that keeps refusing is given up on after about a day, its row kept to be sent again.
#[test]
fn a_call_refused_for_a_day_is_marked_and_can_be_sent_again() {
    let stand = stand_in();
    let front = r#"
db.open("front.db")
service mailer = "mailer.qwe"
route GET /send = mailer.later.s500()
route GET /old = db.exec("update _quillo_later set tries = 289, at = 0")
route GET /again = db.exec("update _quillo_later set at = 0, tries = 0 where tries < 0")
route GET /status/:id = later.status(id)
route GET /seen/:id = json.stringify(later.status(id))
"#;
    let d = dir("given-up", &[("mailer.qwe", "server fn s500() = 1\n"), ("front.qwe", front)]);
    let url = format!("http://127.0.0.1:{}", stand.port);
    let s = serve(&d.join("front.qwe"), &[("QUILLO_SERVICE_MAILER", &url), ("QUILLO_SERVICE_SECRET", SECRET)]);
    let id = get(&s, "/send");
    let status = || serde_json::from_str::<serde_json::Value>(&get(&s, &format!("/seen/{}", id))).unwrap();
    eventually(15, "the call was not tried", || status()["tries"].as_i64().unwrap_or(0) >= 1);
    get(&s, "/old");
    eventually(15, "the call was not given up on", || status()["state"] == "failed");
    let hits = stand.hits.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(2200));
    assert_eq!(stand.hits.load(Ordering::SeqCst), hits, "a call given up on is not tried");
    // Why it failed names where the service runs: no visitor is told.
    let shown = get(&s, &format!("/status/{}", id));
    assert!(shown.contains("failed") && !shown.contains("127.0.0.1"), "{}", shown);
    stand.healed.store(true, Ordering::SeqCst);
    get(&s, "/again");
    eventually(15, "a call sent again was not delivered", || status()["state"] == "done");
}

fn as_service(file: &Path, port: u16) -> Child {
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string(), "--as-service", "--service-name", "mailer"])
        .env("QUILLO_SECRET", "test secret for the tests")
        .env("QUILLO_SERVICE_SECRET", SECRET)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "the service did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    child
}

/// The report's case: the fix for a bad deploy is another deploy, and the calls are still there.
/// A 409 is not remembered as this call's answer, or the next deploy would get it too.
#[test]
fn a_call_outlives_a_deploy_that_does_not_fit_it() {
    let right = "db.open(\"mailer.db\")\ndb.exec(\"create table if not exists sent (who text)\")\nserver fn send(who: str) {\n    db.exec(\"insert into sent (who) values (?)\", who)\n    who\n}\n";
    let wrong = right.replace("who: str)", "who: str, urgent: bool)");
    let d = dir("deploy", &[("right.qwe", right), ("mailer.qwe", right), ("deployed.qwe", &wrong), ("front.qwe", "db.open(\"front.db\")\nservice mailer = \"mailer.qwe\"\nroute GET /send/:who = mailer.later.send(who)\nroute GET /status/:id = json.stringify(later.status(id))\n")]);
    let port = support::free_port();
    let url = format!("http://127.0.0.1:{}", port);
    let mut service = as_service(&d.join("deployed.qwe"), port);
    let s = serve(&d.join("front.qwe"), &[("QUILLO_SERVICE_MAILER", &url), ("QUILLO_SERVICE_SECRET", SECRET), ("QUILLO_LOG", "off")]);
    let id = get(&s, "/send/ana");
    let status = || serde_json::from_str::<serde_json::Value>(&get(&s, &format!("/status/{}", id))).unwrap();
    eventually(15, "the call was not tried", || status()["tries"].as_i64().unwrap_or(0) >= 1);
    assert!(status()["error"]["error"].as_str().unwrap().contains("409"), "{}", status());
    let _ = service.kill();
    let _ = service.wait();
    let mut service = as_service(&d.join("right.qwe"), port);
    eventually(25, "the call was not delivered after the next deploy", || status()["state"] == "done");
    assert_eq!(status()["result"], "ana");
    let _ = service.kill();
    let _ = service.wait();
}

/// In one process too: a runtime error in the step is not the end of the call.
#[test]
fn a_step_that_stops_with_an_error_runs_again() {
    let work = "let broken = true\nlet ran = 0\nserver fn mend() { broken = false }\nserver fn step(x: str) {\n    if broken {\n        json.parse(\"[]\").no.such\n    }\n    ran = ran + 1\n    x\n}\nserver fn times() = ran\n";
    let front = "db.open(\"front.db\")\nservice work = \"work.qwe\"\nroute GET /go = work.later.step(\"x\")\nroute GET /mend = work.mend()\nroute GET /status/:id = json.stringify(later.status(id))\nroute GET /times = work.times()\n";
    let d = dir("stops", &[("work.qwe", work), ("front.qwe", front)]);
    let s = serve(&d.join("front.qwe"), &[]);
    let id = get(&s, "/go");
    let status = || serde_json::from_str::<serde_json::Value>(&get(&s, &format!("/status/{}", id))).unwrap();
    eventually(15, "the step was not tried", || status()["tries"].as_i64().unwrap_or(0) >= 1);
    assert!(status()["error"]["error"].as_str().unwrap().contains("internal error in `step`"), "{}", status());
    get(&s, "/mend");
    eventually(15, "the step did not run again", || status()["state"] == "done");
    assert_eq!(get(&s, "/times"), "1");
}

#[test]
fn a_call_given_up_on_is_counted() {
    let d = dir("counted", &[("work.qwe", "server fn step(x: int) = x\n"), ("front.qwe", "db.open(\"front.db\")\nservice work = \"work.qwe\"\nlet wrong = json.parse(\"[\\\"not a number\\\"]\")\nroute GET /go = work.later.step(wrong[0])\nroute GET /status/:id = json.stringify(later.status(id))\n")]);
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("front.qwe"))
        .args(["--port", &port.to_string(), "--metrics"])
        .env("QUILLO_SECRET", "test secret for the tests")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let _guard = support::Guard::new(child);
    eventually(20, "the server did not start", || support::listening(port) && send(port, "GET", "/status/x", &[], "").status == 200);
    let id = send(port, "GET", "/go", &[], "").body;
    eventually(15, "the call was not marked", || send(port, "GET", &format!("/status/{}", id), &[], "").body.contains("failed"));
    let metrics = send(port, "GET", "/_quillo/metrics", &[], "").body;
    assert!(metrics.contains("quillo_later_total{result=\"failed\"} 1"), "{}", metrics);
}

/// Q-185: with a database per tenant, and beside `db.system`, a call commits with the work.
fn tenant_app(name: &str, open: &str) -> (PathBuf, PathBuf) {
    let front = format!(
        r#"
{}
service mailer = "mailer.qwe"
route GET /rollback/:who = with_tenant("acme", () => {{
    db.exec("create table if not exists orders (id bigint primary key, who text)")
    let r = db.transaction(() => {{
        db.exec("insert into orders (id, who) values (?, ?)", 1, who)
        mailer.later.send(who)
        err("change of mind")
    }})
    `tx: ${{r}}`
}})
route GET /commit/:who = with_tenant("acme", () => {{
    db.exec("create table if not exists orders (id bigint primary key, who text)")
    db.transaction(() => {{
        db.exec("insert into orders (id, who) values (?, ?)", 2, who)
        mailer.later.send(who)
    }})
}})
route GET /status/:id = with_tenant("acme", () => json.stringify(later.status(id)))
route GET /orders = with_tenant("acme", () => len(db.query("select id from orders")))
route GET /sent = mailer.seen_so_far()
"#,
        open
    );
    let d = dir(name, &[("mailer.qwe", MAILER), ("front.qwe", &front)]);
    (d.join("front.qwe"), d)
}

fn a_call_goes_with_the_work(name: &str, open: &str) {
    let (f, _d) = tenant_app(name, open);
    let s = serve(&f, &[]);
    assert!(get(&s, "/rollback/bob").contains("change of mind"), "{}", name);
    assert_eq!(get(&s, "/orders"), "0", "{}", name);
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(get(&s, "/sent"), "", "{}: a call rolled back is not delivered", name);
    let id = get(&s, "/commit/ana");
    eventually(15, &format!("{}: a committed call was not delivered", name), || get(&s, "/sent") == "ana");
    eventually(5, "its answer was not kept", || get(&s, &format!("/status/{}", id)).contains("done"));
}

#[test]
fn a_call_goes_with_a_tenants_transaction() {
    a_call_goes_with_the_work("tenant-sqlite", "db.open(\"data/{tenant}.db\")");
}

#[test]
fn a_call_goes_with_the_transaction_beside_a_system_database() {
    a_call_goes_with_the_work("system", "db.open(\"work.db\")\ndb.system(\"system.db\")");
}

#[test]
fn a_call_goes_with_a_tenants_transaction_on_postgres() {
    let Some(pg) = support::postgres("feedback23-tenant") else { return };
    a_call_goes_with_the_work("tenant-postgres", &format!("db.open(\"{}?schema=t_{{tenant}}\")", pg.url));
}

#[test]
fn a_call_goes_with_a_tenants_transaction_on_mariadb() {
    let Some(m) = support::mariadb("feedback23-tenant") else { return };
    let (f, _d) = tenant_app("tenant-mariadb", &format!("db.open(\"{}\")\nroute GET /new = new_tenant(\"acme\", () => null)", m.url("t23_{tenant}")));
    let s = serve(&f, &[]);
    get(&s, "/new");
    assert!(get(&s, "/rollback/bob").contains("change of mind"));
    assert_eq!(get(&s, "/orders"), "0");
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(get(&s, "/sent"), "", "a call rolled back is not delivered");
    get(&s, "/commit/ana");
    eventually(15, "a committed call was not delivered", || get(&s, "/sent") == "ana");
}

#[test]
fn a_tenants_calls_outlive_a_restart() {
    let (f, _d) = tenant_app("tenant-restart", "db.open(\"data/{tenant}.db\")");
    // No one is there to take it, the first time.
    let nobody = format!("http://127.0.0.1:{}", support::free_port());
    let env = [("QUILLO_SERVICE_MAILER", nobody.as_str()), ("QUILLO_SERVICE_SECRET", SECRET)];
    let mut s = serve(&f, &env);
    let id = get(&s, "/commit/ana");
    assert_eq!(id.len(), 24, "{}", id);
    s.kill();
    let s = serve(&f, &[]);
    eventually(15, "a tenant's call was not delivered after a restart", || get(&s, "/sent") == "ana");
}

fn signed(port: u16, f: &str, call: &str, body: &str) -> support::Answer {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let sig = quillo::service::signature(SECRET, t, "mailer", f, "", call, body.as_bytes());
    let auth = format!("t={},sig={}", t, sig);
    send(port, "POST", &format!("/_quillo/service/{}", f), &[("x-quillo-service", &auth), ("x-quillo-to", "mailer"), ("x-quillo-call", call), ("content-type", "application/json")], body)
}

/// Q-186 and Q-188: a service stopped in the middle of a step.
const STEP: &str = r#"
db.open("mailer.db")
db.exec("create table if not exists entries (who text)")

server fn post(who: str) -> str {
    let r = idempotent(() => {
        db.exec("insert into entries (who) values (?)", who)
        "posted"
    })
    sleep(4000)
    r
}

server fn age() = db.exec("update _quillo_calls set at = at - 700000")
server fn entries() = len(db.query("select who from entries"))
"#;

#[test]
fn a_step_cut_short_runs_again_and_idempotent_keeps_it_to_once() {
    let d = dir("crash", &[("mailer.qwe", STEP)]);
    let port = support::free_port();
    let mut service = as_service(&d.join("mailer.qwe"), port);
    let body = "[\"ana\"]";
    let t = std::thread::spawn(move || signed(port, "post", "call1", "[\"ana\"]").status);
    eventually(5, "the step did not start", || signed(port, "entries", &format!("c{}", fastrand::u32(..)), "[]").body.contains("{\"ok\":1}"));
    // `kill -9` in the middle of it, the work committed.
    service.kill().unwrap();
    service.wait().unwrap();
    assert_eq!(t.join().unwrap(), 0, "the caller got no answer");
    let mut service = as_service(&d.join("mailer.qwe"), port);
    // The claim it left stops the call, for a while.
    assert_eq!(signed(port, "post", "call1", body).status, 503);
    assert_eq!(signed(port, "post", "call1", body).status, 503);
    // Then it is free; `idempotent` found the work done.
    assert!(signed(port, "age", "age1", "[]").body.contains("ok"));
    let r = signed(port, "post", "call1", body);
    assert_eq!((r.status, r.body.as_str()), (200, "{\"ok\":\"posted\"}"));
    assert!(signed(port, "entries", "last", "[]").body.contains("{\"ok\":1}"), "the work ran once");
    let _ = service.kill();
    let _ = service.wait();
}

/// Q-188: `request.call`, the same for each try; `idempotent` needs no header in a step.
#[test]
fn a_step_sees_its_call() {
    let work = r#"
db.open("work.db")
db.exec("create table if not exists done (id text)")
let calls = []
let times = 0
server fn plain() = request.call
server fn remembered() {
    calls = [...calls, request.call]
    "ok"
}
server fn strict() = idempotent({ required: true }, () => {
    times = times + 1
    db.exec("insert into done (id) values ('one')")
    { times: times }
})
server fn two() {
    let a = idempotent(() => {
        db.exec("insert into done (id) values ('two')")
        "first"
    })
    let b = idempotent(() => {
        db.exec("insert into done (id) values ('two')")
        "second"
    })
    a + " " + b
}
server fn seen_calls() = calls | join(",")
server fn done() = len(db.query("select id from done"))
"#;
    let front = r#"
db.open("front.db")
service work = "work.qwe"
route GET /plain = work.plain()
route GET /later = work.later.remembered()
route GET /calls = work.seen_calls()
route GET /strict = json.stringify(work.strict())
route GET /two = work.two()
route GET /done = work.done()
"#;
    let d = dir("call", &[("work.qwe", work), ("front.qwe", front)]);
    let s = serve(&d.join("front.qwe"), &[]);
    let (a, b) = (get(&s, "/plain"), get(&s, "/plain"));
    assert_eq!(a.len(), 16, "{}", a);
    assert_ne!(a, b, "each call has its own");
    let id = get(&s, "/later");
    eventually(15, "the call was not delivered", || get(&s, "/calls") == id);
    assert_eq!(get(&s, "/strict"), "{\"times\":1}", "a key was there, so `required` is met");
    assert_eq!(get(&s, "/two"), "first second", "two in one function are two");
    assert_eq!(get(&s, "/done"), "3");
}

/// Across the wire, a step answered with `idempotent` gives the same value again and again.
#[test]
fn a_step_kept_by_idempotent_answers_as_it_did() {
    let work = "db.open(\"work.db\")\ndb.exec(\"create table if not exists done (id text)\")\nserver fn charge(cents: int) = idempotent(() => {\n    db.exec(\"insert into done (id) values ('x')\")\n    { charged: cents }\n})\nserver fn no(cents: int) = idempotent({ transaction: false }, () => err(\"declined\"))\nserver fn age() = db.exec(\"delete from _quillo_calls\")\nserver fn done() = len(db.query(\"select id from done\"))\n";
    let d = dir("kept", &[("mailer.qwe", work)]);
    let port = support::free_port();
    let mut service = as_service(&d.join("mailer.qwe"), port);
    assert_eq!(signed(port, "charge", "c1", "[5]").body, "{\"ok\":{\"charged\":5}}");
    assert!(signed(port, "no", "c2", "[5]").body.contains("declined"));
    assert_eq!(signed(port, "age", "a1", "[]").status, 200);
    let _ = service.kill();
    let _ = service.wait();
    // The service restarted and the calls forgotten: only `idempotent` remembers.
    let mut service = as_service(&d.join("mailer.qwe"), port);
    assert_eq!(signed(port, "charge", "c1", "[5]").body, "{\"ok\":{\"charged\":5}}");
    let again = signed(port, "no", "c2", "[5]");
    assert!(again.body.contains("declined") && again.body.contains("\"own\":true"), "{}", again.body);
    assert!(signed(port, "done", "d1", "[]").body.contains("{\"ok\":1}"));
    let _ = service.kill();
    let _ = service.wait();
}

/// Q-187: a full go asks for the next at once.
#[test]
fn a_long_queue_is_not_held_to_fifty_a_second() {
    let work = "let n = 0\nserver fn hit(i: int) {\n    n = n + 1\n    n\n}\nserver fn count() = n\n";
    let front = "db.open(\"front.db\")\nservice work = \"work.qwe\"\nroute GET /many/:n = db.transaction(() => {\n    for i in range(0, int(n)) {\n        work.later.hit(i)\n    }\n    \"kept\"\n})\nroute GET /count = work.count()\n";
    let d = dir("queue", &[("work.qwe", work), ("front.qwe", front)]);
    let s = serve(&d.join("front.qwe"), &[]);
    assert_eq!(get(&s, "/many/300"), "kept");
    let start = Instant::now();
    eventually(20, "the calls were not delivered", || get(&s, "/count") == "300");
    // At fifty a second this takes six.
    assert!(start.elapsed() < Duration::from_millis(4500), "{:?}", start.elapsed());
}
