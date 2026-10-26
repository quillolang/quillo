//! Two Quillo servers behind limer (https://limer.sh), sharing one database:
//! a change made through one wakes pages waiting on the other.
//!
//! Needs the limer binary: `$LIMER`, or `../limer/limer` beside this
//! repository. Skipped without it.

use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const SECRET: &str = "cluster-test-secret-0123456789abcdef";

struct Proc(Child);

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// limer, stopped with `limer stop` so that its workers stop too (killing
/// the main process would leave them running).
struct Limer {
    bin: PathBuf,
    config: PathBuf,
    child: Child,
}

impl Drop for Limer {
    fn drop(&mut self) {
        let _ = Command::new(&self.bin).args(["stop", "-c"]).arg(&self.config).stdout(Stdio::null()).stderr(Stdio::null()).status();
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

fn limer() -> Option<PathBuf> {
    let p = std::env::var_os("LIMER")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../limer/limer"));
    p.is_file().then_some(p)
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn wait_tcp(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "nothing listening on {}", port);
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn json(r: ureq::Response) -> serde_json::Value {
    serde_json::from_str(&r.into_string().unwrap()).unwrap()
}

fn post(url: &str, body: &str, live: bool) -> serde_json::Value {
    let mut rq = ureq::post(url).set("content-type", "application/json");
    if live {
        rq = rq.set("x-quillo-live", "1");
    }
    json(rq.send_string(body).unwrap())
}

fn get(url: &str) -> serde_json::Value {
    json(ureq::get(url).call().unwrap())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

#[test]
fn a_change_through_one_server_reaches_pages_on_the_other() {
    let Some(limer) = limer() else {
        eprintln!("skipping: no limer binary (set $LIMER, or build ../limer)");
        return;
    };
    let dir = std::env::temp_dir().join(format!("quillo-cluster-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let app = dir.join("app.qwe");
    std::fs::write(
        &app,
        format!(
            r#"
db.open("{}")
db.exec("create table if not exists orders (id integer primary key, item text)")
let visits = 0
server fn add(item: str) with db = db.one(sql`insert into orders (item) values (?) returning id`, item).id
server fn count() with db = db.one("select count(*) as n from orders").n
server fn visit() {{ visits = visits + 1; visits }}
server fn seen() = visits
route GET / = <p>hello</p>
"#,
            dir.join("shared.db").display()
        ),
    )
    .unwrap();

    let (admin, site) = (free_port(), free_port());
    let limerfile = dir.join("Limerfile");
    std::fs::write(
        &limerfile,
        format!(
            "{{\n    admin 127.0.0.1:{admin} {{\n        secret {SECRET}\n    }}\n    log_level warn\n    storage {dir}/storage\n    pidfile {dir}/limer.pid\n}}\n\
             127.0.0.1:{site} {{\n    reverse_proxy {{\n        discover app {{\n            allow 127.0.0.0/8\n            broadcast /_quillo/announce\n        }}\n    }}\n}}\n",
            dir = dir.display()
        ),
    )
    .unwrap();
    let _lb = Limer {
        child: Command::new(&limer)
            .args(["run", "-w", "1", "-c"])
            .arg(&limerfile)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("cannot start limer"),
        bin: limer.clone(),
        config: limerfile.clone(),
    };
    wait_tcp(admin);

    let start = |port: u16| {
        let p = Proc(
            Command::new(env!("CARGO_BIN_EXE_quillo"))
                .arg("run")
                .arg(&app)
                .args(["--port", &port.to_string(), "--register", &format!("http://127.0.0.1:{}", admin)])
                .env("LIMER_ADMIN_SECRET", SECRET)
                .stderr(Stdio::null())
                .spawn()
                .expect("cannot start quillo"),
        );
        wait_tcp(port);
        p
    };
    let (a, b) = (free_port(), free_port());
    let _a = start(a);
    let _b = start(b);

    // Both join the pool (the service name is the file's: `app`).
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let reg = json(
            ureq::get(&format!("http://127.0.0.1:{}/registry", admin))
                .set("authorization", &format!("Bearer {}", SECRET))
                .call()
                .unwrap(),
        );
        if reg["members"].as_array().map_or(0, |m| m.len()) == 2 {
            break;
        }
        assert!(Instant::now() < deadline, "the servers did not register: {}", reg);
        std::thread::sleep(Duration::from_millis(100));
    }

    // A page on B shows the count, and waits for it to change.
    let rpc = |port: u16, name: &str| format!("http://127.0.0.1:{}/_quillo/rpc/{}", port, name);
    let shown = post(&rpc(b, "count"), "[]", true);
    assert_eq!(shown["ok"], 0);
    assert_eq!(shown["topics"], serde_json::json!(["db:orders"]));
    let (seq, server_b) = (shown["seq"].as_u64().unwrap(), shown["server"].as_str().unwrap().to_string());
    let wait_url = format!("http://127.0.0.1:{}/_quillo/live?since={}&s={}&t=db%3Aorders", b, seq, server_b);
    let waiting = std::thread::spawn(move || {
        let t0 = Instant::now();
        (get(&wait_url), t0.elapsed())
    });
    std::thread::sleep(Duration::from_millis(300));

    // An order is placed through A.
    assert_eq!(post(&rpc(a, "add"), r#"["lamp"]"#, false)["ok"], 1);
    let (woke, after) = waiting.join().unwrap();
    assert_eq!(woke["changed"], serde_json::json!(["db:orders"]), "{}", woke);
    assert!(after < Duration::from_secs(5), "B heard about it only after {:?}", after);
    assert_eq!(post(&rpc(b, "count"), "[]", false)["ok"], 1);

    // A sequence number from A means nothing to B: everything counts as changed.
    let from_a = post(&rpc(a, "count"), "[]", true);
    assert_ne!(from_a["server"], server_b);
    let asked = get(&format!(
        "http://127.0.0.1:{}/_quillo/live?since={}&s={}&t=db%3Aorders",
        b,
        from_a["seq"],
        from_a["server"].as_str().unwrap()
    ));
    assert_eq!(asked["changed"], serde_json::json!(["db:orders"]));
    assert_eq!(asked["server"], server_b.as_str());

    // Variables belong to each server: a change to one is not announced.
    let shown = post(&rpc(b, "seen"), "[]", true);
    let seq = shown["seq"].as_u64().unwrap();
    post(&rpc(a, "visit"), "[]", false);
    std::thread::sleep(Duration::from_millis(500));
    let now = post(&rpc(b, "seen"), "[]", true);
    assert_eq!((now["ok"].as_i64(), now["seq"].as_u64()), (Some(0), Some(seq)), "B's numbers moved: {}", now);

    // Announcements not signed with the key are refused.
    let forged = ureq::post(&format!("http://127.0.0.1:{}/_quillo/announce", b))
        .send_string(r#"{"from":"x","key":"guess","topics":["db:orders"]}"#);
    assert!(matches!(forged, Err(ureq::Error::Status(400 | 403, _))), "{:?}", forged.map(|r| r.status()));
    let forged = quillo::cluster::announcement(&quillo::cluster::announce_key("guess"), "x", now_secs(), &["db:orders".to_string()]);
    let forged = ureq::post(&format!("http://127.0.0.1:{}/_quillo/announce", b)).send_string(&forged);
    assert!(matches!(forged, Err(ureq::Error::Status(403, _))), "{:?}", forged.map(|r| r.status()));

    // And through limer, the app works as one.
    wait_tcp(site);
    let deadline = Instant::now() + Duration::from_secs(10);
    let through = loop {
        match ureq::post(&rpc(site, "count")).set("content-type", "application/json").send_string("[]") {
            Ok(r) => break json(r),
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => panic!("limer did not pass calls on: {}", e),
        }
    };
    assert_eq!(through["ok"], 1);
    // limer stops by its configuration, so before the folder goes.
    drop(_lb);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- announcements, without limer ------------------------------------------------------------

/// Stands in for limer's admin API: says yes to everything.
fn fake_admin() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            use std::io::{BufRead, BufReader, Read, Write};
            let mut r = BufReader::new(s);
            let mut len = 0;
            let mut line = String::new();
            while r.read_line(&mut line).map_or(false, |n| n > 0) && line != "\r\n" {
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
                line.clear();
            }
            let _ = r.by_ref().take(len).read_to_end(&mut Vec::new());
            let _ = r.get_mut().write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    port
}

fn announce(port: u16, body: &str) -> u16 {
    match ureq::post(&format!("http://127.0.0.1:{}/_quillo/announce", port)).send_string(body) {
        Ok(r) => r.status(),
        Err(ureq::Error::Status(code, _)) => code,
        Err(e) => panic!("{}", e),
    }
}

#[test]
fn announcements_are_signed_fresh_once_and_about_known_topics() {
    let dir = std::env::temp_dir().join(format!("quillo-announce-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let app = dir.join("app.qwe");
    std::fs::write(&app, "server fn orders() { watch(\"orders\"); 1 }\nroute GET / = \"up\"\n").unwrap();
    let admin = format!("http://127.0.0.1:{}", fake_admin());

    // Without limer's secret, a server does not join.
    let refused = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(&app)
        .args(["--port", &free_port().to_string(), "--register", &admin])
        .env_remove("LIMER_ADMIN_SECRET")
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("LIMER_ADMIN_SECRET"));

    let port = free_port();
    let _server = Proc(
        Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(&app)
            .args(["--port", &port.to_string(), "--register", &admin])
            .env("LIMER_ADMIN_SECRET", SECRET)
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_tcp(port);
    // A page shows the orders: the server now knows the topic.
    let rpc = format!("http://127.0.0.1:{}/_quillo/rpc/orders", port);
    let seq = || post(&rpc, "[]", true)["seq"].as_u64().unwrap();
    let start = seq();
    let key = quillo::cluster::announce_key(SECRET);
    let now = now_secs();
    let topics = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    // The old form: no signature, and a key that was the same for everyone
    // without a secret (the review's attack).
    let constant = quillo::crypto::hex(&quillo::ws::sha1(b"quillo-announce:"));
    assert_eq!(announce(port, &format!(r#"{{"from":"x","key":"{}","topics":["orders"]}}"#, constant)), 400);
    // Signed with another secret, or too old.
    let other = quillo::cluster::announce_key("not-the-secret");
    assert_eq!(announce(port, &quillo::cluster::announcement(&other, "x", now, &topics(&["orders"]))), 403);
    assert_eq!(announce(port, &quillo::cluster::announcement(&key, "x", now - 3600, &topics(&["orders"]))), 403);
    // Tampered with: the topics changed after signing.
    let good = quillo::cluster::announcement(&key, "x", now, &topics(&["orders"]));
    assert_eq!(announce(port, &good.replace("\"orders\"", "\"other\"")), 403);
    assert_eq!(seq(), start);

    // Signed: taken, once.
    assert_eq!(announce(port, &good), 204);
    let after = seq();
    assert_eq!(after, start + 1);
    assert_eq!(announce(port, &good), 403, "a message sent again is refused");

    // Topics the server does not know are dropped, not remembered.
    let unknown: Vec<String> = (0..200).map(|i| format!("junk-{}", i)).collect();
    assert_eq!(announce(port, &quillo::cluster::announcement(&key, "x", now, &unknown)), 204);
    assert_eq!(seq(), after);
    // At most so many topics in one message.
    let many: Vec<String> = (0..quillo::cluster::MAX_TOPICS + 1).map(|i| format!("t{}", i)).collect();
    assert_eq!(announce(port, &quillo::cluster::announcement(&key, "x", now, &many)), 413);
    let _ = std::fs::remove_dir_all(&dir);
}
