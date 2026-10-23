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

    // Announcements without the key are refused.
    let forged = ureq::post(&format!("http://127.0.0.1:{}/_quillo/announce", b))
        .send_string(r#"{"from":"x","key":"guess","topics":["db:orders"]}"#);
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
