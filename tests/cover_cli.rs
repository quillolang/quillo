//! The command line past the usual path (src/main.rs, src/new.rs, src/policy.rs): options given
//! wrong, a port that is no number from `$PORT`, `quillo fcgi` on a TCP port, the limits a policy
//! sets on a server, the policy file's own edges, and `quillo new` where it cannot write.

mod support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::{send, Guard};

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("cover-cli-{}", name));
    for (f, text) in files {
        if let Some(p) = d.join(f).parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

/// `quillo` with these arguments, in `d`, given 60 seconds at most.
fn quillo(d: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(args).current_dir(d).env_remove("PORT").env_remove("LIMER_ADMIN_SECRET").envs(env.iter().copied()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = c.spawn().unwrap();
    support::stop_with_tests(&child);
    let id = child.id();
    let done = Arc::new(AtomicBool::new(false));
    let d2 = done.clone();
    std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(60);
        while !d2.load(Ordering::SeqCst) {
            if Instant::now() > until {
                let _ = Command::new("kill").args(["-KILL", &id.to_string()]).status();
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    let o = child.wait_with_output().unwrap();
    done.store(true, Ordering::SeqCst);
    o
}

fn said(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Exits with 2, saying `what`.
fn refused(o: &Output, what: &str) {
    assert_eq!(o.status.code(), Some(2), "{}", said(o));
    assert!(said(o).contains(what), "wanted {:?} in:\n{}", what, said(o));
}

/// A server started in `d` with these arguments, once it listens; its log in `log.txt`.
fn started(d: &Path, cmd: &str, args: &[&str]) -> (Guard, u16) {
    let port = support::free_port();
    let log = std::fs::File::create(d.join("log.txt")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args([cmd, "main.qwe", "--port", &port.to_string()])
        .args(args)
        .current_dir(d)
        .env("QUILLO_SECRET", "a secret for the cover tests, long enough")
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let g = Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "did not start: {}", std::fs::read_to_string(d.join("log.txt")).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(20));
    }
    (g, port)
}

const HELLO: &str = "route GET / = \"hi\"\n";

#[test]
fn an_option_where_the_command_goes_is_told_the_usage() {
    let d = dir("usage", &[]);
    let o = quillo(&d, &["--frobnicate"], &[]);
    refused(&o, "usage:");
    // The library's own list of what a program may be refused: this file uses it too.
    assert!(quillo::policy::Policy::default().files.is_empty());
}

#[test]
fn serve_with_a_value_option_last_says_it_needs_its_value() {
    let d = dir("serve-port", &[("site/index.html", "<p>x</p>")]);
    refused(&quillo(&d, &["serve", "site", "--port"], &[]), "`--port` needs a value");
    refused(&quillo(&d, &["serve", "site", "--port", "eighty"], &[]), "--port is a number from 1 to 65535");
}

#[test]
fn join_options_and_live_per_client_refuse_what_they_cannot_use() {
    let d = dir("join-opts", &[("main.qwe", HELLO)]);
    refused(&quillo(&d, &["run", "main.qwe", "--allow-join", " , "], &[]), "--allow-join names the services whose copies may join");
    refused(&quillo(&d, &["run", "main.qwe", "--join-as", "billing-3.internal:8443"], &[]), "--join-as is the address this copy is called at");
    refused(&quillo(&d, &["run", "main.qwe", "--live-per-client", "0"], &[]), "--live-per-client needs a positive number");
    refused(&quillo(&d, &["run", "main.qwe", "--live-per-client", "many"], &[]), "--live-per-client needs a positive number");
}

#[test]
fn a_port_from_the_environment_that_is_no_number_is_refused_for_register_and_join() {
    let d = dir("port-env", &[("main.qwe", HELLO)]);
    let env = [("PORT", "eighty"), ("LIMER_ADMIN_SECRET", "a limer secret for this test")];
    refused(&quillo(&d, &["run", "main.qwe", "--register", "http://127.0.0.1:1"], &env), "--register needs a numeric --port");
    refused(&quillo(&d, &["run", "main.qwe", "--as-service", "--join", "http://127.0.0.1:1"], &env[..1]), "--join needs a numeric --port");
}

#[test]
fn check_stops_at_a_policy_that_does_not_parse() {
    let d = dir("check-policy", &[("main.qwe", HELLO), ("bad.toml", "[limits]\nmax_compute = \"2h\"\n")]);
    let o = quillo(&d, &["check", "main.qwe", "--policy", "bad.toml"], &[]);
    assert_ne!(o.status.code(), Some(0), "{}", said(&o));
    assert!(said(&o).contains("bad.toml:2: `2h` is not a time such as \"10s\" or \"500ms\""), "{}", said(&o));
}

/// A FastCGI request on `s`: its STDOUT, once the END_REQUEST comes.
fn fcgi_ask(s: &mut TcpStream, params: &[(&str, &str)]) -> String {
    let rec = |t: u8, c: &[u8]| {
        let pad = (8 - c.len() % 8) % 8;
        let mut r = vec![1, t, 0, 1];
        r.extend_from_slice(&(c.len() as u16).to_be_bytes());
        r.extend_from_slice(&[pad as u8, 0]);
        r.extend_from_slice(c);
        r.resize(r.len() + pad, 0);
        r
    };
    let mut p = Vec::new();
    for (k, v) in params {
        p.extend_from_slice(&[k.len() as u8, v.len() as u8]);
        p.extend_from_slice(k.as_bytes());
        p.extend_from_slice(v.as_bytes());
    }
    let mut m = rec(1, &[0, 1, 0, 0, 0, 0, 0, 0]);
    m.extend(rec(4, &p));
    m.extend(rec(4, &[]));
    m.extend(rec(5, &[]));
    s.write_all(&m).unwrap();
    let mut out = Vec::new();
    loop {
        let mut h = [0u8; 8];
        s.read_exact(&mut h).unwrap();
        let n = u16::from_be_bytes([h[4], h[5]]) as usize;
        let mut c = vec![0u8; n + h[6] as usize];
        s.read_exact(&mut c).unwrap();
        c.truncate(n);
        match h[1] {
            6 => out.extend_from_slice(&c),
            3 => return String::from_utf8_lossy(&out).into_owned(),
            _ => {}
        }
    }
}

#[test]
fn quillo_fcgi_listens_on_a_tcp_port_when_given_one() {
    let d = dir("fcgi-tcp", &[("main.qwe", "route GET / = `fastcgi on tcp for ${request.headers.host}`\n")]);
    let (_g, port) = started(&d, "fcgi", &[]);
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let out = fcgi_ask(&mut s, &[("REQUEST_METHOD", "GET"), ("REQUEST_URI", "/"), ("HTTP_HOST", "example.com"), ("REMOTE_ADDR", "203.0.113.9")]);
    assert!(out.starts_with("Status: 200 OK\r\n") && out.ends_with("fastcgi on tcp for example.com"), "{}", out);
}

#[test]
fn a_policys_limits_hold_a_server_to_its_body_size_and_compute_time() {
    let d = dir(
        "policy-limits",
        &[
            ("main.qwe", "route GET / = \"hi\"\nroute POST /take = `took ${len(request.body)}`\nroute GET /forever {\n    let i = 0\n    while i >= 0 { i = i + 1 }\n    \"never\"\n}\n"),
            ("limits.toml", "[limits]\nworkers = 2\nmax_body = \"1KB\"\nmax_compute = \"300ms\"\n"),
        ],
    );
    let (_g, port) = started(&d, "run", &["--no-check", "--policy", "limits.toml"]);
    assert_eq!(send(port, "POST", "/take", &[("content-type", "text/plain")], &"x".repeat(500)).body, "took 500");
    assert_eq!(send(port, "POST", "/take", &[("content-type", "text/plain")], &"x".repeat(4000)).status, 413, "past the policy's max_body");
    let began = Instant::now();
    let r = send(port, "GET", "/forever", &[], "");
    assert_eq!(r.status, 500, "{}", r.body);
    assert!(began.elapsed() < Duration::from_secs(10), "{:?}", began.elapsed());
    assert_eq!(send(port, "GET", "/", &[], "").body, "hi", "and it goes on");
    let log = std::fs::read_to_string(d.join("log.txt")).unwrap_or_default();
    assert!(log.contains("computed for longer than"), "{}", log);
}

#[test]
fn a_policy_limit_on_compute_does_not_widen_a_tighter_one_given() {
    let d = dir("policy-tighter", &[("main.qwe", "route GET / = \"hi\"\n"), ("limits.toml", "[limits]\nmax_compute = \"5s\"\n")]);
    let (_g, port) = started(&d, "run", &["--no-check", "--max-compute", "200ms", "--policy", "limits.toml"]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "hi");
}

// ---- the policy file -----------------------------------------------------------------------

fn policy(text: &str) -> Result<quillo::policy::Policy, String> {
    let mut p = quillo::policy::Policy::default();
    p.add(Path::new("p.toml"), text).map(|_| p)
}

#[test]
fn a_policy_list_holds_text_only() {
    let e = policy("[http]\nhosts = [\"api.example.com\", 3]\n").unwrap_err();
    assert!(e.contains("p.toml: line 2: `[\"api.example.com\", 3]` is not true, false, a number, \"text\" or a list of \"text\""), "{}", e);
    let e = policy("[plugins]\nallow = [\"csv\",\n  true]\n").unwrap_err();
    assert!(e.contains("line 2:"), "{}", e);
}

#[test]
fn a_policy_time_has_a_unit_it_knows() {
    for bad in ["2h", "10 days", "x"] {
        let e = policy(&format!("[limits]\nmax_compute = \"{}\"\n", bad)).unwrap_err();
        assert!(e.contains("is not a time such as"), "{}: {}", bad, e);
    }
    let e = policy("[limits]\nmax_body = true\n").unwrap_err();
    assert!(e.contains("p.toml:2: `max_body` is a size such as \"10MB\""), "{}", e);
    let p = policy("[limits]\nmax_compute = \"2m\"\n").unwrap();
    assert_eq!(p.max_compute, Some(Duration::from_secs(120)));
}

#[test]
fn ipv6_on_the_public_internet_is_not_private_and_ipv4_in_ipv6_is_judged_as_ipv4() {
    use quillo::policy::private_ip;
    let ip = |s: &str| private_ip(s.parse().unwrap());
    for public in ["2606:4700::1111", "2001:4860::8888", "::ffff:8.8.8.8"] {
        assert!(!ip(public), "{}", public);
    }
    for private in ["::ffff:10.0.0.1", "fe80::1", "fd00::1", "::1", "10.1.2.3", "100.64.0.1"] {
        assert!(ip(private), "{}", private);
    }
}

// ---- quillo new ----------------------------------------------------------------------------

#[test]
fn new_says_why_it_cannot_make_the_folder_above_or_move_the_program_in() {
    use std::os::unix::fs::PermissionsExt;
    let d = dir("new-fails", &[("a-file", "not a folder")]);
    // The folder above is a file: it cannot be made.
    let o = quillo(&d, &["new", "a-file/inside/app"], &[]);
    assert_ne!(o.status.code(), Some(0));
    assert!(said(&o).contains("cannot make") && said(&o).contains("a-file/inside"), "{}", said(&o));
    // A folder there, empty, that cannot be written into: nothing is moved in, and nothing is left.
    let locked = d.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let o = quillo(&d, &["new", "locked"], &[]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_ne!(o.status.code(), Some(0));
    assert!(said(&o).contains("cannot move the program into locked"), "{}", said(&o));
    assert_eq!(std::fs::read_dir(&locked).unwrap().count(), 0);
    assert!(!std::fs::read_dir(&d).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("quillo-new")), "no folder of its own left beside it");
    // A folder that cannot be read.
    let hidden = d.join("hidden");
    std::fs::create_dir(&hidden).unwrap();
    std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o000)).unwrap();
    let o = quillo(&d, &["new", "hidden"], &[]);
    std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_ne!(o.status.code(), Some(0));
    assert!(said(&o).to_ascii_lowercase().contains("permission denied"), "{}", said(&o));
}

#[test]
fn new_takes_the_name_of_a_folder_written_with_dots() {
    let d = dir("new-dots", &[]);
    std::fs::create_dir_all(d.join("shop/tmp")).unwrap();
    // `shop/tmp/..` is shop, which is not empty (it has tmp): the name is read from the folder itself.
    let o = quillo(&d, &["new", "shop/tmp/.."], &[]);
    assert_ne!(o.status.code(), Some(0));
    assert!(said(&o).contains("is not empty: it has tmp"), "{}", said(&o));
}
