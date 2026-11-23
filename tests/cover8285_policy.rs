//! Coverage of feedback82-85 (branch feedback82-85): the lines of the provider's policy
//! (FEEDBACK82.md) and of the commands that read it that no test ran at 7e5a9af. src/policy.rs:
//! the layers read from a host's own folder (`include`, `policy.d`), a layer that cannot be looked
//! at, `quillo policy`'s report of every setting in force and the start's summary of `[run]`;
//! src/main.rs: `quillo policy --policy=FILE`, `[run]`'s ceilings, forced log, socket mode and
//! proxies put in force, `--checked` in a service its app started, `quillo mcp` held to its
//! quillo.confirmed (FEEDBACK83.md) and `quillo mcp --serve`'s ends (FEEDBACK85.md, Q-1347);
//! src/install.rs: a catalog file that cannot be fetched, a quillo.confirmed that cannot be kept
//! (FEEDBACK83.md, Q-1306 and Q-1307); src/desktop.rs: `quillo app` held to the policy and to its
//! quillo.confirmed (FEEDBACK82.md, Q-1284).

mod support;

use quillo::policy::{self, Kind};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use support::{port_in_log, send, Guard};

/// A folder for one test, removed when the test ends.
struct Tmp(PathBuf);

impl Tmp {
    fn new(name: &str) -> Tmp {
        Tmp(support::temp_dir(&format!("cov8285-policy-{}", name)))
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Tmp {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
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

/// `quillo` with `args` in `dir`, `vars` set, its stdin closed: its exit code and what it wrote,
/// ended after 60 s.
fn quillo(dir: &Path, args: &[&str], vars: &[(&str, &str)]) -> (Option<i32>, String) {
    let log = dir.join("cmd.log");
    let file = std::fs::File::create(&log).unwrap();
    let mut child = command(dir, args, vars).stdin(Stdio::null()).stdout(file.try_clone().unwrap()).stderr(file).spawn().unwrap();
    let code = wait_exit(&mut child, 60);
    (code, read(&log))
}

/// The command, with none of the variables of `cargo test` or of a host's layer but `vars`.
fn command(dir: &Path, args: &[&str], vars: &[(&str, &str)]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(args)
        .current_dir(dir)
        .env_remove(policy::PATH_VAR)
        .env_remove("QUILLO_ENV_FROM_FILE")
        .env_remove("QUILLO_STOP_WITH_APP")
        .env_remove("CARGO")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_TARGET_DIR")
        .env("HOME", dir)
        .env("QUILLO_BUILT_RECORD_DIR", dir.join(".built"))
        .env("QUILLO_SECRET", "test secret for the tests, 32 bytes or more")
        .envs(vars.iter().copied());
    c
}

/// A server or door started in `dir`, stderr in `log`, stdin piped (taken by the test to keep it
/// open or to close it).
fn start(dir: &Path, args: &[&str], vars: &[(&str, &str)], log: &str) -> Guard {
    let child = command(dir, args, vars)
        .stdin(Stdio::piped())
        .stdout(std::fs::File::create(dir.join(format!("{}.out", log))).unwrap())
        .stderr(std::fs::File::create(dir.join(log)).unwrap())
        .spawn()
        .unwrap();
    Guard::new(child)
}

fn wait_exit(child: &mut Child, secs: u64) -> Option<i32> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return s.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Waits until `log` holds `what`, or panics with it after `secs`.
fn wait_for(log: &Path, what: &str, secs: u64) -> String {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let text = read(log);
        if text.contains(what) {
            return text;
        }
        assert!(Instant::now() < deadline, "no {:?} in:\n{}", what, text);
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn sigterm(child: &Child) {
    // SAFETY: a signal to the process this test started, which it still holds.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
}

const HOME: &str = "route GET / = \"home\"\nroute GET /ip = request.ip\nroute GET /slow with time = { sleep(20000); return \"slow done\" }\n";

// ---- src/policy.rs: the layers, the report and the summary --------------------------------------

/// Every layer a host can have, each there: the host's file, a file it includes, two drop-ins of
/// policy.d (a file of another kind beside them not read), QUILLO_POLICY_PATH's, a `--policy` and the
/// program's; the report names each with what it is, has no "not there", and gives every setting in
/// force with the file it came from, a later layer narrowing an earlier one; the start's summary
/// says the ceilings and the socket mode (src/policy.rs Kind::said, Policy::report and
/// Policy::summary, FEEDBACK82.md Q-1281 and Q-1286; coverage of feedback82-85; branch feedback82-85).
#[test]
fn every_layer_there_is_reported_with_its_settings_in_force() {
    let d = Tmp::new("layers");
    let a = d.canonicalize().unwrap();
    let at = |p: &str| a.join(p).display().to_string();
    write(&a, "host/policy.toml", &format!("include = [\"{}\"]\nrequire_path = true\n\n[plugins]\nbuild = false\n", at("inc/extra.toml")));
    write(&a, "inc/extra.toml", "[http]\nprivate = false\n\n[limits]\nmax_compute = \"2s\"\n");
    write(&a, "host/policy.d/10-run.toml", "[run]\ncheck = true\ninstances = 3\ndrain = \"30s\"\n");
    write(&a, "host/policy.d/20-sock.toml", "[run]\nsocket_mode = \"640\"\nlog = \"json\"\nbase = \"/shop/\"\n");
    // Not a .toml: no layer, though it is no policy at all.
    write(&a, "host/policy.d/README.txt", "this = [ is not a policy\n");
    write(&a, "pv.toml", "[limits]\nmax_body = \"1MB\"\nmax_memory = \"64MB\"\nmax_tool_calls = 30\n\n[effects]\nallow = [\"db\", \"time\"]\n");
    // Later and looser: none of it widens what came before.
    write(&a, "named.toml", "[run]\ndrain = \"5s\"\ninstances = 9\nsocket_mode = \"777\"\ncheck = false\n\n[limits]\nmax_compute = \"1m\"\n");
    write(&a, "app/quillo.policy", "[run]\ninstances = 2\n");
    let p = policy::load_from(&a.join("host/policy.toml"), &a.join("app/main.qwe"), &[a.join("named.toml")], Some(at("pv.toml").into())).unwrap();
    assert!(p.absent.is_empty(), "{:?}", p.absent);
    assert_eq!(p.kinds, vec![Kind::Host, Kind::Included, Kind::Dropin, Kind::Dropin, Kind::PathVar, Kind::Named, Kind::Program]);
    let want = [
        "layers, in the order read (each only narrows the ones before it):".to_string(),
        format!("  {} (the host's file)", at("host/policy.toml")),
        format!("  {} (included by the host's file)", at("inc/extra.toml")),
        format!("  {} (policy.d)", at("host/policy.d/10-run.toml")),
        format!("  {} (policy.d)", at("host/policy.d/20-sock.toml")),
        format!("  {} (QUILLO_POLICY_PATH)", at("pv.toml")),
        format!("  {} (--policy)", at("named.toml")),
        format!("  {} (the program's)", at("app/quillo.policy")),
        "in force:".to_string(),
        format!("  plugins.build = false ({})", at("host/policy.toml")),
        format!("  effects.allow = db, time ({})", at("pv.toml")),
        format!("  http.private = false ({})", at("inc/extra.toml")),
        format!("  limits.max_compute = 2000ms ({})", at("inc/extra.toml")),
        format!("  limits.max_body = 1048576 bytes ({})", at("pv.toml")),
        format!("  limits.max_memory = 67108864 bytes ({})", at("pv.toml")),
        format!("  limits.max_tool_calls = 30 a minute ({})", at("pv.toml")),
        format!("  run.check = true ({})", at("host/policy.d/10-run.toml")),
        format!("  run.instances = 2 ({})", at("app/quillo.policy")),
        format!("  run.drain = 5000ms ({})", at("named.toml")),
        format!("  run.socket_mode = 640 ({})", at("host/policy.d/20-sock.toml")),
        format!("  run.log = \"json\" ({})", at("host/policy.d/20-sock.toml")),
        format!("  run.base = \"/shop\" ({})", at("host/policy.d/20-sock.toml")),
        format!("  require_path = true ({})", at("host/policy.toml")),
    ];
    assert_eq!(p.report(), want.join("\n"));
    let summary = p.summary();
    assert!(summary.contains("instances 2; drain 5000ms; socket_mode 640; log json; base /shop"), "{}", summary);
    assert!(summary.starts_with(&format!("policy {} + {}", at("host/policy.toml"), at("inc/extra.toml"))), "{}", summary);
    // require_path asks for the variable: without it, or naming no file, the load stops, said
    // as missing (quillo policy's exit 1).
    for var in [None, Some(std::ffi::OsString::from(""))] {
        let e = policy::load_from(&a.join("host/policy.toml"), &a.join("app/main.qwe"), &[], var.clone()).unwrap_err();
        assert!(e.missing && e.message.contains("says require_path = true, and QUILLO_POLICY_PATH"), "{:?}: {}", var, e.message);
    }
}

/// A layer that is there but cannot be looked at stops the load, naming it: a file `include`
/// names under a path whose folder is a file, and a policy.d whose path is too long to look at;
/// and each way of making the layers go round: the host's file including itself (no loop: an
/// included file may not include), a drop-in that includes or says require_path, a drop-in that is
/// a folder or a link to nothing, a `--policy` or the program's file setting the environment
/// (src/policy.rs there and load_from, FEEDBACK82.md Q-1281 and Q-1283;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_layer_that_cannot_be_looked_at_stops_the_load() {
    let d = Tmp::new("broken");
    let a = d.canonicalize().unwrap();
    let main = a.join("app/main.qwe");
    let host = a.join("host/policy.toml");
    let load = |extra: &[PathBuf]| policy::load_from(&host, &main, extra, None);
    // A file where a folder should be: not "not there", an error.
    write(&a, "plain", "a file\n");
    write(&a, "host/policy.toml", &format!("include = [\"{}\"]\n", a.join("plain/inner.toml").display()));
    let e = load(&[]).unwrap_err();
    assert!(!e.missing && e.message.starts_with(&format!("cannot read the policy {}: ", a.join("plain/inner.toml").display())), "{}", e.message);
    // An include that is not there is missing (exit 1 from quillo policy), not broken.
    write(&a, "host/policy.toml", &format!("include = [\"{}\"]\n", a.join("nowhere.toml").display()));
    let e = load(&[]).unwrap_err();
    assert!(e.missing && e.message.contains("includes") && e.message.contains("which is not there"), "{}", e.message);
    // Itself: read a second time as an included file, which may not include.
    write(&a, "host/policy.toml", &format!("include = [\"{}\"]\n", host.display()));
    let e = load(&[]).unwrap_err();
    assert!(e.message.contains("`include` is said only by the host's file"), "{}", e.message);
    // A drop-in says neither; nor may one that is a folder or a link that leads nowhere be skipped.
    write(&a, "host/policy.toml", "[limits]\nworkers = 4\n");
    write(&a, "host/policy.d/10.toml", "require_path = false\n");
    assert!(load(&[]).unwrap_err().message.contains("`require_path` is said only by the host's file"));
    std::fs::remove_file(a.join("host/policy.d/10.toml")).unwrap();
    std::fs::create_dir_all(a.join("host/policy.d/20.toml")).unwrap();
    assert!(load(&[]).unwrap_err().message.contains("it is a folder, not a policy file"));
    std::fs::remove_dir(a.join("host/policy.d/20.toml")).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(a.join("gone.toml"), a.join("host/policy.d/30.toml")).unwrap();
        assert!(load(&[]).unwrap_err().message.contains("which is not there"));
        std::fs::remove_file(a.join("host/policy.d/30.toml")).unwrap();
    }
    // The environment is the host's to set, not a --policy's nor the program's.
    write(&a, "named.toml", "[env]\nset = { APP_ENV = \"dev\" }\n");
    assert!(load(&[a.join("named.toml")]).unwrap_err().message.contains("`env.set` is the host's to say"));
    write(&a, "app/quillo.policy", "[env.set]\nAPP_ENV = \"dev\"\n");
    let e = load(&[]).unwrap_err();
    assert!(e.message.contains("`env.set` is the host's to say") && e.message.contains("the program's"), "{}", e.message);
    std::fs::remove_file(a.join("app/quillo.policy")).unwrap();
    // Nor a host's layer the names that choose the policy or what a process loads.
    write(&a, "host/policy.toml", "[env]\nset = { QUILLO_POLICY_PATH = \"/dev/null\" }\n");
    assert!(load(&[]).unwrap_err().message.contains("may not set QUILLO_POLICY_PATH"));
    write(&a, "host/policy.toml", "[env.set]\ndyld_insert_libraries = \"/tmp/x.dylib\"\n");
    assert!(load(&[]).unwrap_err().message.contains("may not set dyld_insert_libraries"));
    // policy.d where its path is too long to look at: an error naming it, not "not there". The
    // host's file beside it is short enough to be looked for and is not there.
    let most = if cfg!(target_os = "macos") { 1024 } else { 4096 };
    let target = most - 8;
    let mut long = a.join("no-such-folder");
    while long.as_os_str().len() + 201 < target - 1 {
        long.push("x".repeat(200));
    }
    long.push("y".repeat(target - 1 - long.as_os_str().len()));
    assert_eq!(long.as_os_str().len(), target);
    let anchor = long.join("p");
    let e = policy::load_from(&anchor, &main, &[], None).unwrap_err();
    assert!(!e.missing && e.message.starts_with(&format!("cannot read the policies in {}: ", long.join("policy.d").display())), "{}", e.message);
}

/// What a switch turns off: each of `SWITCHES` names the options it refuses, and a name that is
/// no switch refuses none (src/policy.rs switch_options, FEEDBACK82.md Q-1280;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_name_that_is_no_switch_turns_off_nothing() {
    for k in policy::SWITCHES {
        let o = policy::switch_options(k);
        assert!(!o.is_empty() && o.iter().all(|f| f.starts_with('-')), "{}: {:?}", k, o);
    }
    assert!(policy::switch_options("check").is_empty());
    assert!(policy::switch_options("").is_empty());
    assert!(policy::switch_options("WITH").is_empty());
}

// ---- src/main.rs: quillo policy --policy=FILE ----------------------------------------------------

/// `quillo policy --policy=FILE` reads FILE as `--policy FILE` does, with QUILLO_POLICY_PATH's layer
/// and the program's: the report lists them and each setting in force with its file; a
/// `--policy=` that is not there is broken (exit 2), a QUILLO_POLICY_PATH file that is not there
/// missing (exit 1) (src/main.rs policy_command, src/policy.rs report, FEEDBACK82.md Q-1286;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn quillo_policy_takes_the_option_with_its_file_after_equals() {
    let d = Tmp::new("policy-cmd");
    write(&d, "main.qwe", "route GET / = \"home\"\n");
    write(
        &d,
        "rich.toml",
        "[plugins]\nbuild = false\n[effects]\nallow = [\"db\", \"time\"]\n[http]\nprivate = false\n[limits]\nmax_compute = \"2s\"\nmax_body = \"1MB\"\nmax_memory = \"64MB\"\nmax_tool_calls = 30\n[run]\ncheck = true\ninstances = 3\ndrain = \"30s\"\nsocket_mode = \"640\"\nlog = \"json\"\nbase = \"/shop/\"\n",
    );
    write(&d, "quillo.policy", "[run]\ndrain = \"5s\"\ninstances = 2\n");
    write(&d, "pv.toml", "[limits]\nmax_compute = \"500ms\"\n");
    let pv = d.join("pv.toml").display().to_string();
    let (code, out) = quillo(&d, &["policy", "main.qwe", "--policy=rich.toml"], &[(policy::PATH_VAR, &pv)]);
    assert_eq!(code, Some(0), "{}", out);
    let want = [
        "the policy for main.qwe".to_string(),
        "layers, in the order read (each only narrows the ones before it):".to_string(),
        format!("  {} (QUILLO_POLICY_PATH)", pv),
        "  rich.toml (--policy)".to_string(),
        "  quillo.policy (the program's)".to_string(),
        "not there:".to_string(),
    ];
    let in_force = [
        "in force:".to_string(),
        "  plugins.build = false (rich.toml)".to_string(),
        "  effects.allow = db, time (rich.toml)".to_string(),
        "  http.private = false (rich.toml)".to_string(),
        format!("  limits.max_compute = 500ms ({})", pv),
        "  limits.max_body = 1048576 bytes (rich.toml)".to_string(),
        "  limits.max_memory = 67108864 bytes (rich.toml)".to_string(),
        "  limits.max_tool_calls = 30 a minute (rich.toml)".to_string(),
        "  run.check = true (rich.toml)".to_string(),
        "  run.instances = 2 (quillo.policy)".to_string(),
        "  run.drain = 5000ms (quillo.policy)".to_string(),
        "  run.socket_mode = 640 (rich.toml)".to_string(),
        "  run.log = \"json\" (rich.toml)".to_string(),
        "  run.base = \"/shop\" (rich.toml)".to_string(),
    ];
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(&lines[..want.len()], &want.iter().map(String::as_str).collect::<Vec<_>>()[..], "{}", out);
    // After "not there:" the host's places (this machine's own), then what is in force, last.
    let tail: Vec<&str> = lines[lines.len() - in_force.len()..].to_vec();
    assert_eq!(tail, in_force.iter().map(String::as_str).collect::<Vec<_>>(), "{}", out);
    assert!(out.contains("(the host's file)") && out.contains("(policy.d)"), "{}", out);
    // The two ways of writing it read the same.
    let (code, spaced) = quillo(&d, &["policy", "main.qwe", "--policy", "rich.toml"], &[(policy::PATH_VAR, &pv)]);
    assert_eq!((code, spaced.as_str()), (Some(0), out.as_str()));
    // Not there: a --policy file is broken, the variable's missing.
    let (code, out) = quillo(&d, &["policy", "main.qwe", "--policy=nope.toml"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.starts_with("quillo: cannot read the policy nope.toml: "), "{}", out);
    let gone = d.join("gone.toml").display().to_string();
    let (code, out) = quillo(&d, &["policy", "main.qwe"], &[(policy::PATH_VAR, &gone)]);
    assert_eq!(code, Some(1), "{}", out);
    assert_eq!(out, format!("quillo: QUILLO_POLICY_PATH names the policy {}, which is not there\n", gone));
    // `--policy=` with nothing after it names no file.
    let (code, out) = quillo(&d, &["policy", "main.qwe", "--policy="], &[]);
    assert_eq!(code, Some(2), "{}", out);
}

// ---- src/main.rs: [run] put in force ---------------------------------------------------------------

/// A provider's layer holds `quillo run`'s options to its ceilings: `--instances 3` runs 2, the
/// connections at once to 2 (a third closed as it comes, said), the stop's wait to 300 ms
/// whatever `--drain` asked (a request in progress cut off, said), and the request log off; the
/// start's line says each (src/main.rs policy_run, FEEDBACK82.md Q-1280;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn the_ceilings_and_the_log_a_provider_sets_are_held() {
    let d = Tmp::new("ceilings");
    write(&d, "main.qwe", HOME);
    write(&d, "host/instances.toml", "[run]\ninstances = 2\nlog = \"text\"\n");
    let host = d.join("host/instances.toml").display().to_string();
    let mut g = start(&d, &["run", "main.qwe", "--port", "0", "--instances", "3"], &[(policy::PATH_VAR, &host)], "i.log");
    let port = port_in_log(&mut g.0, &d.join("i.log"), Duration::from_secs(60));
    assert_eq!(send(port, "GET", "/", &[], "").body, "home");
    let log = wait_for(&d.join("i.log"), "GET / -> 200", 30);
    assert!(log.contains(&format!("policy {} (instances 2; log text)", host)), "{}", log);
    assert!(log.contains("listening on") && log.contains("(2 instances)") && !log.contains("(3 instances)"), "{}", log);
    assert!(!log.contains("\"status\""), "a text log, not JSON: {}", log);
    drop(g);

    write(&d, "host/limits.toml", "[run]\nmax_connections = 2\ndrain = \"300ms\"\nlog = \"off\"\n");
    let host = d.join("host/limits.toml").display().to_string();
    let mut g = start(&d, &["run", "main.qwe", "--port", "0", "--drain", "1m"], &[(policy::PATH_VAR, &host)], "l.log");
    let port = port_in_log(&mut g.0, &d.join("l.log"), Duration::from_secs(60));
    // Two held open: the third is closed as it comes, and the log says why.
    let held: Vec<TcpStream> = (0..2).map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(300));
    let mut third = TcpStream::connect(("127.0.0.1", port)).unwrap();
    third.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let _ = third.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let mut got = Vec::new();
    let _ = third.read_to_end(&mut got);
    assert!(!String::from_utf8_lossy(&got).contains("home"), "{}", String::from_utf8_lossy(&got));
    wait_for(&d.join("l.log"), "a connection is closed as it comes: 2 connections are open", 30);
    drop(held);
    // Once they are closed, a place again; and no request log.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let _ = s.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        let mut got = Vec::new();
        let _ = s.read_to_end(&mut got);
        if String::from_utf8_lossy(&got).ends_with("home") {
            break;
        }
        assert!(Instant::now() < deadline, "no place again");
        std::thread::sleep(Duration::from_millis(50));
    }
    // A request that takes 20 s, and a stop: cut off after the policy's 300 ms, not --drain's minute.
    let slow = std::thread::spawn(move || {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        s.write_all(b"GET /slow HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
        let mut got = Vec::new();
        let _ = s.read_to_end(&mut got);
        String::from_utf8_lossy(&got).into_owned()
    });
    std::thread::sleep(Duration::from_millis(500));
    sigterm(&g.0);
    assert_eq!(wait_exit(&mut g.0, 15), Some(0), "{}", read(&d.join("l.log")));
    let answer = slow.join().unwrap();
    assert!(!answer.contains("slow done"), "{}", answer);
    let log = read(&d.join("l.log"));
    assert!(log.contains(&format!("policy {} (max_connections 2; drain 300ms; log off)", host)), "{}", log);
    assert!(log.contains("GET /slow still running") || log.contains("stopping with requests in progress"), "{}", log);
    assert!(!log.contains("GET / -> 200"), "the log is off: {}", log);
}

/// This machine's address on its network, if it has one other than loopback.
fn own_address() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

/// A live page, as the runtime opens one: true when it is let in (101).
fn live_page(port: u16) -> Option<TcpStream> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "GET /_quillo/live?since=1&s=&t=db:nothing HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n").unwrap();
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        s.read_exact(&mut b).ok()?;
        head.push(b[0]);
    }
    String::from_utf8_lossy(&head).starts_with("HTTP/1.1 101").then_some(s)
}

/// The live pages and the connections one client may hold are the least of the option and the
/// policy: `--live-per-client 300` under 2 lets 2 pages wait and refuses the third (503, said);
/// `--connections-per-client 100` under 3 holds a client on the network to 3 (this machine is not
/// held to it) (src/main.rs policy_run, FEEDBACK82.md Q-1280;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn what_one_client_may_hold_is_the_policys_least() {
    let d = Tmp::new("per-client");
    write(&d, "main.qwe", HOME);
    write(&d, "p.toml", "[run]\nlive_per_client = 2\nconnections_per_client = 3\n");
    let mut g = start(&d, &["run", "main.qwe", "--port", "0", "--host", "0.0.0.0", "--policy", "p.toml", "--live-per-client", "300", "--connections-per-client", "100"], &[], "c.log");
    let port = port_in_log(&mut g.0, &d.join("c.log"), Duration::from_secs(60));
    let pages: Vec<TcpStream> = (0..2).map(|i| live_page(port).unwrap_or_else(|| panic!("page {}", i))).collect();
    assert!(live_page(port).is_none(), "a third page waits");
    let log = wait_for(&d.join("c.log"), "127.0.0.1 holds 2 pages waiting for changes, and more are refused", 30);
    assert!(log.contains("policy p.toml (live_per_client 2; connections_per_client 3)"), "{}", log);
    drop(pages);
    // From this machine's address on its network, not loopback: 3 open, the fourth closed.
    if let Some(ip) = own_address() {
        let held: Vec<TcpStream> = (0..3).map(|_| TcpStream::connect((ip.as_str(), port)).unwrap()).collect();
        std::thread::sleep(Duration::from_millis(300));
        let mut fourth = TcpStream::connect((ip.as_str(), port)).unwrap();
        fourth.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let _ = fourth.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        let mut got = Vec::new();
        let _ = fourth.read_to_end(&mut got);
        assert!(!String::from_utf8_lossy(&got).contains("home"), "{}", String::from_utf8_lossy(&got));
        // The policy's 3 is the one in force, and named (FEEDBACK82.md, Q-1280, found by the
        // coverage tests: it said "the most --connections-per-client allows").
        wait_for(&d.join("c.log"), &format!("{} holds 3 connections, the most the policy p.toml allows (run.connections_per_client)", ip), 30);
        drop(held);
    }
    // Loopback is not held to it.
    let local: Vec<TcpStream> = (0..5).map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap()).collect();
    assert_eq!(send(port, "GET", "/", &[], "").body, "home");
    drop(local);
}

/// The socket's mode is what the option and the policy both allow: `--socket-mode 666` under
/// `socket_mode = "640"` makes a socket of 640, and under "600" one of 600; it answers there
/// (src/main.rs policy_run, FEEDBACK82.md Q-1280; coverage of feedback82-85; branch feedback82-85).
#[cfg(unix)]
#[test]
fn the_sockets_mode_is_held_to_the_policys() {
    use std::os::unix::fs::PermissionsExt;
    let d = Tmp::new("sock");
    write(&d, "main.qwe", HOME);
    for (policy_mode, given, want) in [("640", "666", 0o640), ("600", "660", 0o600), ("660", "640", 0o640)] {
        write(&d, "p.toml", &format!("[run]\nsocket_mode = \"{}\"\n", policy_mode));
        // Beside the program: a socket's path is at most 100 bytes.
        let sock = d.join("s.sock");
        let _ = std::fs::remove_file(&sock);
        let g = start(&d, &["run", "main.qwe", "--listen", "unix:s.sock", "--socket-mode", given, "--policy", "p.toml", "--trust-proxy", "127.0.0.1"], &[], "s.log");
        wait_for(&d.join("s.log"), "listening on unix:s.sock", 60);
        let mut s = std::os::unix::net::UnixStream::connect(&sock).unwrap();
        let mode = std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, want, "policy {} and --socket-mode {}: {:o}", policy_mode, given, mode);
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        s.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
        let mut got = String::new();
        let _ = s.read_to_string(&mut got);
        assert!(got.ends_with("home"), "{}", got);
        assert!(read(&d.join("s.log")).contains(&format!("(socket_mode {})", policy_mode)), "{}", read(&d.join("s.log")));
        drop(g);
    }
}

/// `--trust-proxy` naming proxies inside the policy's is taken, and believed only from them:
/// 127.0.0.1 inside 127.0.0.0/8 makes `request.ip` the forwarded address; 127.0.0.2, also inside,
/// is not the proxy the request came from, so it is not; written as IPv6 it is the same address.
/// Outside, refused before the start: another network beside it, all of IPv4 written as IPv6 or
/// plainly, and an IPv6 address (src/main.rs policy_run, FEEDBACK82.md Q-1280;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_proxy_inside_the_policys_is_taken_and_believed() {
    let d = Tmp::new("proxy");
    write(&d, "main.qwe", HOME);
    write(&d, "host.toml", "[run]\ntrust_proxy = [\"127.0.0.0/8\"]\n");
    let host = d.join("host.toml").display().to_string();
    for (given, seen) in [("127.0.0.1", "203.0.113.9"), ("127.0.0.2", "127.0.0.1"), ("::ffff:127.0.0.1", "203.0.113.9"), ("127.0.0.0/8", "203.0.113.9")] {
        let mut g = start(&d, &["run", "main.qwe", "--port", "0", "--trust-proxy", given], &[(policy::PATH_VAR, &host)], "p.log");
        let port = port_in_log(&mut g.0, &d.join("p.log"), Duration::from_secs(60));
        assert_eq!(send(port, "GET", "/ip", &[("x-forwarded-for", "203.0.113.9")], "").body, seen, "--trust-proxy {}", given);
        drop(g);
    }
    for given in ["127.0.0.1,10.0.0.1", "::ffff:0.0.0.0/96", "0.0.0.0/0", "::1"] {
        let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0", "--trust-proxy", given], &[(policy::PATH_VAR, &host)]);
        assert_eq!(code, Some(2), "{}: {}", given, out);
        assert!(out.contains(&format!("is not among the proxies the policy {} trusts: 127.0.0.0/8", host)), "{}: {}", given, out);
    }
}

/// Under `check = true`, `--checked` given by a service's app (QUILLO_STOP_WITH_APP) is not taken:
/// the service checks the program again, and one with an error does not start; without the policy
/// the same start skips the check and serves (src/main.rs policy_run, FEEDBACK82.md Q-1280;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_service_its_app_started_checks_again_under_check_true() {
    let d = Tmp::new("checked");
    write(&d, "main.qwe", "route GET / = \"home\"\nroute GET /x = nope()\n");
    write(&d, "host.toml", "[run]\ncheck = true\n");
    let host = d.join("host.toml").display().to_string();
    let mut g = start(&d, &["run", "main.qwe", "--port", "0", "--checked"], &[(policy::PATH_VAR, &host), ("QUILLO_STOP_WITH_APP", "1")], "a.log");
    assert_eq!(wait_exit(&mut g.0, 60), Some(1), "{}", read(&d.join("a.log")));
    let log = read(&d.join("a.log"));
    assert!(log.contains("main.qwe:2: error: unknown name `nope`"), "{}", log);
    assert!(!log.contains("listening on"), "{}", log);
    drop(g);
    // By hand, without its app: refused, as before.
    let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0", "--checked"], &[(policy::PATH_VAR, &host)]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains(&format!("--checked is refused by the policy {} (run.check = true)", host)), "{}", out);
    // No policy: the app's word is taken, and the program serves unchecked.
    let mut g = start(&d, &["run", "main.qwe", "--port", "0", "--checked"], &[("QUILLO_STOP_WITH_APP", "1")], "b.log");
    let port = port_in_log(&mut g.0, &d.join("b.log"), Duration::from_secs(60));
    assert_eq!(send(port, "GET", "/", &[], "").body, "home");
    // Its app gone (stdin closed): it stops.
    drop(g.0.stdin.take());
    assert!(wait_exit(&mut g.0, 60).is_some());
}

// ---- src/main.rs: quillo mcp ----------------------------------------------------------------------

const META: &str = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}"#;

const SLOW: &str = "/// Waits `ms` milliseconds, then says so.
fn mcp slow(ms: int) -> str with time {
  print(\"started \" + str(ms))
  sleep(ms)
  return \"done after \" + str(ms)
}
/// Says hi.
fn mcp hi() -> str = \"hi\"
route GET / = \"home\"
route POST /echo = str(len(request.body))
";

/// `quillo mcp` is held to its quillo.confirmed as `quillo run` is: one that cannot be read stops
/// the door before it starts (exit 2, naming its line), as the door and as `--serve` (src/main.rs
/// mcp, FEEDBACK83.md Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn mcp_is_held_to_its_quillo_confirmed() {
    let d = Tmp::new("mcp-confirmed");
    write(&d, "m.qwe", SLOW);
    write(&d, "quillo.confirmed", "[\"m.qwe\"]\nhosts = [\"x\"]\n");
    for args in [&["mcp", "m.qwe"][..], &["mcp", "m.qwe", "--serve", "--port", "0"]] {
        let (code, out) = quillo(&d, args, &[]);
        assert_eq!(code, Some(2), "{:?}: {}", args, out);
        assert!(out.contains("quillo.confirmed: line 2: `hosts` is not something quillo.confirmed says"), "{:?}: {}", args, out);
        assert!(!out.contains("listening on"), "{}", out);
    }
    // Read as it is: once it can be read, the door starts and answers.
    write(&d, "quillo.confirmed", "[\"m.qwe\"]\n");
    let mut g = start(&d, &["mcp", "m.qwe"], &[], "m.log");
    let mut stdin = g.0.stdin.take().unwrap();
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"slow","arguments":{{"ms":1}},{}}}}}"#, META).unwrap();
    drop(stdin);
    assert_eq!(wait_exit(&mut g.0, 60), Some(0), "{}", read(&d.join("m.log")));
    assert!(read(&d.join("m.log.out")).contains(r#""structuredContent":{"result":"done after 1"}"#), "{}", read(&d.join("m.log.out")));
}

/// `quillo mcp --serve` of a program with neither routes nor tools runs its top level and ends
/// (exit 0), its stdout empty; of one whose top level fails, ends with the error (exit 1) and
/// listens on nothing (src/main.rs serve_beside, FEEDBACK85.md Q-1347;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn mcp_serve_of_nothing_to_serve_or_a_failing_program_ends() {
    let d = Tmp::new("mcp-ends");
    write(&d, "none.qwe", "let x = 1\nprint(\"top \" + str(x))\n");
    let mut g = start(&d, &["mcp", "none.qwe", "--serve", "--port", "0"], &[], "n.log");
    assert_eq!(wait_exit(&mut g.0, 60), Some(0), "{}", read(&d.join("n.log")));
    let log = read(&d.join("n.log"));
    assert!(log.contains("none.qwe has no tools") && log.contains("top 1"), "{}", log);
    assert!(!log.contains("listening on"), "{}", log);
    assert_eq!(read(&d.join("n.log.out")), "", "nothing but MCP's messages reaches stdout");
    drop(g);
    write(&d, "bad.qwe", "/// Pong.\nfn mcp ping() -> str = \"pong\"\nlet x = [1, 2]\nlet y = x[5]\nprint(y + 1)\n");
    let mut g = start(&d, &["mcp", "bad.qwe", "--serve", "--port", "0"], &[], "b.log");
    assert_eq!(wait_exit(&mut g.0, 60), Some(1), "{}", read(&d.join("b.log")));
    let log = read(&d.join("b.log"));
    assert!(log.contains("quillo: bad.qwe:5: cannot add null and int"), "{}", log);
    assert!(!log.contains("listening on"), "{}", log);
}

/// The JSON-RPC message of `len` bytes calling `hi`, padded with a key of its params that the door
/// does not read. Not `slow`: a tool that waits leaves the routes of `--serve` unanswered after it
/// (reported, not covered here).
fn call_of(id: u32, len: usize) -> String {
    let at = |pad: usize| format!(r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"hi","arguments":{{}},"pad":"{}",{}}}}}"#, id, "x".repeat(pad), META);
    let base = at(0).len();
    let m = at(len - base);
    assert_eq!(m.len(), len);
    m
}

/// Under `--serve` the policy's max_body holds the door's messages and the routes' bodies: with
/// 1KB, a message of 1024 bytes is answered and one of 1025 refused as too long, and a body of
/// 2000 bytes is refused (413) where one of 10 is read (src/main.rs serve_beside, FEEDBACK85.md
/// Q-1347; coverage of feedback82-85; branch feedback82-85).
#[test]
fn mcp_serve_holds_messages_and_bodies_to_the_policys_max_body() {
    let d = Tmp::new("mcp-body");
    write(&d, "m.qwe", SLOW);
    write(&d, "mb.toml", "[limits]\nmax_body = \"1KB\"\n");
    let mut g = start(&d, &["mcp", "m.qwe", "--serve", "--port", "0", "--policy", "mb.toml"], &[], "s.log");
    let port = port_in_log(&mut g.0, &d.join("s.log"), Duration::from_secs(60));
    let mut stdin = g.0.stdin.take().unwrap();
    for (id, len) in [(1, 1023), (2, 1024), (3, 1025), (4, 4000)] {
        writeln!(stdin, "{}", call_of(id, len)).unwrap();
    }
    stdin.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while read(&d.join("s.log.out")).lines().count() < 4 {
        assert!(Instant::now() < deadline, "{}\n{}", read(&d.join("s.log.out")), read(&d.join("s.log")));
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = read(&d.join("s.log.out"));
    let lines: Vec<&str> = out.lines().collect();
    for (i, l) in lines[..2].iter().enumerate() {
        assert!(l.starts_with(&format!(r#"{{"jsonrpc":"2.0","id":{},"#, i + 1)) && l.contains(r#""structuredContent":{"result":"hi"}"#), "{}", l);
    }
    for l in &lines[2..] {
        assert_eq!(*l, r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Invalid request: a message is at most 1 KiB"}}"#);
    }
    assert_eq!(send(port, "POST", "/echo", &[("content-type", "text/plain")], &"y".repeat(10)).body, "10");
    assert_eq!(send(port, "POST", "/echo", &[("content-type", "text/plain")], &"y".repeat(2000)).status, 413);
    drop(stdin);
    assert_eq!(wait_exit(&mut g.0, 60), Some(0), "{}", read(&d.join("s.log")));
    assert!(read(&d.join("s.log")).contains("policy mb.toml (max_body 1024 bytes)"));
}

/// A stop (SIGTERM) while a tool call runs under `--serve`: the routes stop, and the process waits
/// for the call, whose answer reaches the client, then ends (exit 0) (src/main.rs serve_beside,
/// FEEDBACK85.md Q-1347; coverage of feedback82-85; branch feedback82-85).
#[test]
fn mcp_serve_stops_once_the_call_in_progress_is_answered() {
    let d = Tmp::new("mcp-stop");
    write(&d, "m.qwe", SLOW);
    let mut g = start(&d, &["mcp", "m.qwe", "--serve", "--port", "0"], &[], "t.log");
    let port = port_in_log(&mut g.0, &d.join("t.log"), Duration::from_secs(60));
    let mut stdin = g.0.stdin.take().unwrap();
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{{"name":"slow","arguments":{{"ms":1500}},{}}}}}"#, META).unwrap();
    stdin.flush().unwrap();
    wait_for(&d.join("t.log"), "started 1500", 30);
    sigterm(&g.0);
    // The routes go first.
    let deadline = Instant::now() + Duration::from_secs(30);
    while support::listening(port) {
        assert!(Instant::now() < deadline, "{}", read(&d.join("t.log")));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(wait_exit(&mut g.0, 60), Some(0), "{}", read(&d.join("t.log")));
    let out = read(&d.join("t.log.out"));
    assert!(out.starts_with(r#"{"jsonrpc":"2.0","id":7,"#) && out.contains("done after 1500"), "{}", out);
    assert!(read(&d.join("t.log")).contains("stopping once the requests in progress are answered"), "{}", read(&d.join("t.log")));
    drop(stdin);
}

/// The server beside the door that cannot start (no descriptor left for its poll, under a low
/// `ulimit -n`) says why and ends the process (exit 1), at some limit of a sweep: each limit
/// either serves or ends with a line of its own, never a panic (src/main.rs serve_beside,
/// FEEDBACK85.md Q-1347; coverage of feedback82-85; branch feedback82-85). The descriptors a
/// process starts with differ by system: macOS only.
#[cfg(target_os = "macos")]
#[test]
fn mcp_serve_whose_server_cannot_start_says_why() {
    let d = Tmp::new("mcp-fds");
    write(&d, "m.qwe", SLOW);
    let mut failed = Vec::new();
    for n in 4..=16 {
        let log = format!("fd{}.log", n);
        let script = format!("ulimit -n {}; exec '{}' mcp m.qwe --serve --port 0", n, env!("CARGO_BIN_EXE_quillo"));
        let child = Command::new("sh")
            .args(["-c", &script])
            .current_dir(&*d)
            .env_remove(policy::PATH_VAR)
            .env("QUILLO_SECRET", "test secret for the tests, 32 bytes or more")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(d.join(&log)).unwrap())
            .spawn()
            .unwrap();
        let mut g = Guard::new(child);
        let deadline = Instant::now() + Duration::from_secs(60);
        while g.0.try_wait().unwrap().is_none() && !read(&d.join(&log)).contains("listening on") {
            assert!(Instant::now() < deadline, "{}", read(&d.join(&log)));
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(g.0.stdin.take());
        let code = wait_exit(&mut g.0, 60);
        let text = read(&d.join(&log));
        assert!(!text.contains("panicked"), "ulimit -n {}: {}", n, text);
        // What failed is named (FEEDBACK85.md, Q-1347, found by the coverage tests: the line was
        // "quillo: Too many open files (os error 24)" alone).
        if code == Some(1) && text.contains("quillo: cannot start the server for m.qwe: Too many open files") {
            failed.push(n);
        }
    }
    assert!(!failed.is_empty(), "no limit stopped the server's start");
}

// ---- src/install.rs ------------------------------------------------------------------------------

/// A server of fixed files, by path: a catalog; anything else is 404.
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

const HI: &str = "//! Says hi.\n//! version: 1.0.0\n//!\npub fn hi() -> String { \"hi\".into() }\n";

/// A catalog of one plugin, `hi`, at `text`, its file served when `served`.
fn catalog(text: &str, served: bool) -> String {
    let index = format!(
        "{{\"format\":1,\"plugins\":[{{\"name\":\"hi\",\"title\":\"Hi\",\"version\":\"1.0.0\",\"summary\":\"s\",\"group\":\"g\",\"sha256\":\"{}\",\"file\":\"plugins/hi.qwer\",\"example\":\"\",\"files\":[]}}]}}",
        sha256_hex(text.as_bytes())
    );
    let mut files = vec![("/plugins/index.json", index.into_bytes())];
    if served {
        files.push(("/plugins/hi.qwer", text.as_bytes().to_vec()));
    }
    format!("http://127.0.0.1:{}", files_server(files))
}

/// A copy here of the catalog's version, changed, with the catalog's file not to be had: kept as
/// it is and said, with how to replace it (src/install.rs old_differs_only_in_effects, FEEDBACK83.md
/// Q-1306; coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_changed_copy_is_kept_when_the_catalogs_file_cannot_be_fetched() {
    let d = Tmp::new("install-gone");
    let reg = catalog(HI, false);
    let mine = format!("{}// mine\n", HI);
    write(&d, "hi.qwer", &mine);
    let (code, out) = quillo(&d, &["install", "hi", "--registry", &reg], &[]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("kept hi 1.0.0 here differs from the catalog's: changed by hand? --force replaces it"), "{}", out);
    assert_eq!(read(&d.join("hi.qwer")), mine);
    // With --force it is fetched, and that fails: nothing written.
    let (code, out) = quillo(&d, &["install", "hi", "--registry", &reg, "--force"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert_eq!(read(&d.join("hi.qwer")), mine);
}

/// The plugin installed, its hash not kept where quillo.confirmed cannot take it: a link to a file
/// elsewhere (nothing written through it, the file elsewhere unchanged) or a file that does not read;
/// each said, and the install fails (exit 1) (src/install.rs main and keep_confirmed, FEEDBACK83.md
/// Q-1307; coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_quillo_confirmed_that_cannot_keep_the_hash_fails_the_install() {
    let d = Tmp::new("install-confirmed");
    let reg = catalog(HI, true);
    write(&d, "outside/kept.confirmed", "# someone else's file\n");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(d.join("outside/kept.confirmed"), d.join("quillo.confirmed")).unwrap();
        let (code, out) = quillo(&d, &["install", "hi", "--registry", &reg], &[]);
        assert_eq!(code, Some(1), "{}", out);
        assert!(out.contains("installed hi 1.0.0") && out.contains("quillo install: ./quillo.confirmed is a symbolic link: nothing written"), "{}", out);
        assert!(!out.contains("keeps its SHA-256"), "{}", out);
        assert_eq!(read(&d.join("outside/kept.confirmed")), "# someone else's file\n");
        assert_eq!(read(&d.join("hi.qwer")), HI);
        std::fs::remove_file(d.join("quillo.confirmed")).unwrap();
        std::fs::remove_file(d.join("hi.qwer")).unwrap();
    }
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nhosts = [\"x\"]\n");
    let (code, out) = quillo(&d, &["install", "hi", "--registry", &reg], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("quillo install: ./quillo.confirmed: line 2: `hosts` is not something quillo.confirmed says"), "{}", out);
    assert_eq!(read(&d.join("quillo.confirmed")), "[\"main.qwe\"]\nhosts = [\"x\"]\n");
    // A file that reads: the hash kept, and all is well.
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = quillo(&d, &["install", "hi", "--registry", &reg], &[]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("quillo.confirmed keeps its SHA-256"), "{}", out);
    assert!(read(&d.join("quillo.confirmed")).contains(&format!("[\"hi.qwer\"]\nsha256 = \"{}\"", sha256_hex(HI.as_bytes()))));
}

// ---- src/desktop.rs: quillo app -----------------------------------------------------------------

/// `quillo app` reads the policy as `quillo run` does and is held to it and to its quillo.confirmed,
/// each stopping it before any window: a policy that does not read (exit 2), `--no-check` and
/// `--with` refused by it (exit 2), a quillo.confirmed that does not read (exit 2)
/// (src/desktop.rs window, FEEDBACK82.md Q-1284 and FEEDBACK83.md Q-1300;
/// coverage of feedback82-85; branch feedback82-85).
#[cfg(feature = "app")]
#[test]
fn quillo_app_is_held_before_its_window() {
    let d = Tmp::new("app");
    write(&d, "main.qwe", "route GET / = \"home\"\n");
    write(&d, "bad.toml", "[run]\nnot_a_key = 1\n");
    let (code, out) = quillo(&d, &["app", "main.qwe", "--policy", "bad.toml"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("bad.toml:2: `run.not_a_key` is not something a policy says"), "{}", out);
    write(&d, "strict.toml", "[run]\ncheck = true\nwith = false\n");
    let (code, out) = quillo(&d, &["app", "main.qwe", "--policy", "strict.toml", "--no-check"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("--no-check is refused by the policy strict.toml (run.check = true)"), "{}", out);
    let (code, out) = quillo(&d, &["app", "main.qwe", "--policy", "strict.toml", "--with", "echo hi"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("--with is turned off by the policy strict.toml (run.with = false)"), "{}", out);
    // The program's own quillo.policy, read with no option.
    write(&d, "quillo.policy", "[run]\ncheck = true\n");
    let (code, out) = quillo(&d, &["app", "main.qwe", "--no-check"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("--no-check is refused by the policy quillo.policy (run.check = true)"), "{}", out);
    std::fs::remove_file(d.join("quillo.policy")).unwrap();
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nhosts = [\"x\"]\n");
    let (code, out) = quillo(&d, &["app", "main.qwe"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("quillo.confirmed: line 2: `hosts` is not something quillo.confirmed says"), "{}", out);
    assert!(!d.join(".quillo").exists(), "nothing made for a start that stops");
}

// ---- Bugs the coverage tests found, fixed (slice Y) ---------------------------------------------

/// A start whose environment says `QUILLO_ENV_FROM_FILE=QUILLO_POLICY_PATH`, with no `.env`, still
/// reads the provider's layer `QUILLO_POLICY_PATH` names: its `[effects]` refuses `env`, and
/// `quillo policy` lists it. Every spelling of the name in the marker, lower case, among others,
/// with empty entries; and in a service its app started, where the marker is believed (src/dotenv.rs
/// take_marker, src/policy.rs load_layers, FEEDBACK82.md Q-1281, found by the coverage tests; branch
/// feedback82-85).
#[test]
fn a_marker_in_the_environment_drops_no_layer_of_the_providers() {
    let d = Tmp::new("forged-marker");
    write(&d, "host/p.toml", "[effects]\nallow = [\"db\"]\n");
    write(&d, "main.qwe", "print(env(\"APP_X\") ?? \"none\")\n");
    let host = d.join("host/p.toml").display().to_string();
    let refused = format!("main.qwe:1: error: `env` needs the env effect, which {} does not allow", host);
    for marker in ["QUILLO_POLICY_PATH", "quillo_policy_path", "Quillo_Policy_Path", "APP_X,QUILLO_POLICY_PATH", ",QUILLO_POLICY_PATH,", "QUILLO_POLICY_PATH,QUILLO_POLICY", ""] {
        let (code, out) = quillo(&d, &["run", "main.qwe"], &[(policy::PATH_VAR, &host), ("QUILLO_ENV_FROM_FILE", marker)]);
        assert_eq!(code, Some(1), "{:?}: {}", marker, out);
        assert!(out.contains(&refused) && !out.contains("came from a .env file"), "{:?}: {}", marker, out);
        let (code, out) = quillo(&d, &["policy", "main.qwe"], &[(policy::PATH_VAR, &host), ("QUILLO_ENV_FROM_FILE", marker)]);
        assert_eq!(code, Some(0), "{:?}: {}", marker, out);
        assert!(out.contains(&format!("{} (QUILLO_POLICY_PATH)", host)) && out.contains("effects.allow = db"), "{:?}: {}", marker, out);
    }
    // A service its app started believes the marker, and still never takes the policy's name from it.
    let mut g = start(&d, &["run", "main.qwe", "--as-service", "--port", "0"], &[(policy::PATH_VAR, &host), ("QUILLO_ENV_FROM_FILE", "QUILLO_POLICY_PATH"), ("QUILLO_STOP_WITH_APP", "1")], "s.log");
    assert_eq!(wait_exit(&mut g.0, 60), Some(1), "{}", read(&d.join("s.log")));
    assert!(read(&d.join("s.log")).contains(&refused), "{}", read(&d.join("s.log")));
}

/// The marker is no one's word outside a service its app started: a value the environment set is
/// not replaced by `.env` because the marker names it, there or in the processes started from it
/// (`--with`'s command does not see it). In a service its app started it is believed: the app's
/// file's value gives way to the service's own (src/dotenv.rs take_marker, FEEDBACK82.md Q-1281,
/// found by the coverage tests; branch feedback82-85).
#[test]
fn the_marker_is_believed_only_in_a_service_its_app_started() {
    let d = Tmp::new("marker-trust");
    write(&d, "main.qwe", "print(env(\"REAL_X\") ?? \"none\")\n");
    write(&d, ".env", "REAL_X=from the file\n");
    let real = [("REAL_X", "real"), ("QUILLO_ENV_FROM_FILE", "REAL_X")];
    let (code, out) = quillo(&d, &["run", "main.qwe"], &real);
    assert_eq!((code, out.as_str()), (Some(0), "real\n"));
    // With `--as-service` alone (no app started it) or the app's variable alone: not believed.
    let (code, out) = quillo(&d, &["run", "main.qwe", "--as-service"], &real);
    assert_eq!(code, Some(0), "{}", out);
    assert!(!out.contains("from the file"), "{}", out);
    // Started by its app (stdin the app's pipe, held open until the line is out): believed.
    let mut g = start(&d, &["run", "main.qwe", "--as-service", "--port", "0"], &[("REAL_X", "the app's file's"), ("QUILLO_ENV_FROM_FILE", "REAL_X"), ("QUILLO_STOP_WITH_APP", "1")], "t.log");
    wait_for(&d.join("t.log.out"), "from the file\n", 60);
    drop(g.0.stdin.take());
    assert!(wait_exit(&mut g.0, 60).is_some());
    // Not passed on: the command `--with` starts sees no marker.
    write(&d, "srv.qwe", "route GET / = \"home\"\n");
    let mut g = start(&d, &["run", "srv.qwe", "--port", "0", "--with", "sh -c 'echo marker=[$QUILLO_ENV_FROM_FILE] > marker.txt'"], &real, "w.log");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !read(&d.join("marker.txt")).contains('\n') {
        assert!(Instant::now() < deadline, "{}", read(&d.join("w.log")));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(read(&d.join("marker.txt")), "marker=[]\n");
    sigterm(&g.0);
    assert!(wait_exit(&mut g.0, 60).is_some());
}

/// What the host's `[env] set` forces, no `.env` replaces, whatever the marker says: the program's
/// `.env` (with the marker and without), a service's own `.env` in one process, and under `--split`,
/// where the app's file's other names still give way to the service's own; the marker's entries no
/// `.env` gave do not reach the service either. A name written in other case is another variable on
/// Unix, read from `.env` (src/dotenv.rs load_into_process, var and set_by_file, FEEDBACK82.md
/// Q-1280, found by the coverage tests; branch feedback82-85).
#[test]
fn a_name_the_hosts_env_set_forces_is_never_a_dotenvs() {
    let d = Tmp::new("env-set-forced");
    write(&d, "host/p.toml", "[env]\nset = { APP_ENV = \"production\" }\n");
    write(&d, ".env", "APP_ENV=dev\nOTHER=x\napp_env=lower\n");
    write(&d, "p.qwe", "print(env(\"APP_ENV\") ?? \"none\")\nprint(env(\"OTHER\") ?? \"none\")\nprint(env(\"app_env\") ?? \"none\")\n");
    let host = d.join("host/p.toml").display().to_string();
    let want = if cfg!(windows) { "production\nx\nproduction\n" } else { "production\nx\nlower\n" };
    for marker in ["", "APP_ENV", "APP_ENV,OTHER", "app_env,APP_ENV"] {
        let (code, out) = quillo(&d, &["run", "p.qwe"], &[(policy::PATH_VAR, &host), ("QUILLO_ENV_FROM_FILE", marker)]);
        assert_eq!(code, Some(0), "{:?}: {}", marker, out);
        assert!(out.ends_with(want), "{:?}: {}", marker, out);
    }
    // A service its app started, which believes the marker: the forced name is still the host's.
    let mut g = start(&d, &["run", "p.qwe", "--as-service", "--port", "0"], &[(policy::PATH_VAR, &host), ("QUILLO_ENV_FROM_FILE", "APP_ENV"), ("QUILLO_STOP_WITH_APP", "1")], "a.log");
    wait_for(&d.join("a.log.out"), want, 60);
    drop(g.0.stdin.take());
    assert!(wait_exit(&mut g.0, 60).is_some());
    // A service's own .env, in one process and apart.
    write(&d, "svc/.env", "APP_ENV=svcdev\nOTHER=svc\nREAL_X=svcfile\n");
    write(&d, "svc/s.qwe", "server fn e() -> str = env(\"APP_ENV\") ?? \"none\"\nserver fn o() -> str = env(\"OTHER\") ?? \"none\"\nserver fn r() -> str = env(\"REAL_X\") ?? \"none\"\n");
    write(&d, "main.qwe", "service s = \"svc/s.qwe\"\nroute GET /e = s.e()\nroute GET /o = s.o()\nroute GET /r = s.r()\n");
    let vars = [(policy::PATH_VAR, host.as_str()), ("QUILLO_ENV_FROM_FILE", "APP_ENV,REAL_X"), ("REAL_X", "real"), ("QUILLO_SERVICE_SECRET", "another secret for the tests, 32 bytes or more")];
    for split in [false, true] {
        let mut args = vec!["run", "main.qwe", "--port", "0"];
        if split {
            args.push("--split");
        }
        let log = format!("split-{}.log", split);
        let mut g = start(&d, &args, &vars, &log);
        let port = port_in_log(&mut g.0, &d.join(&log), Duration::from_secs(60));
        let deadline = Instant::now() + Duration::from_secs(60);
        let body = |p: &str| send(port, "GET", p, &[], "").body;
        while send(port, "GET", "/e", &[], "").status != 200 {
            assert!(Instant::now() < deadline, "{}", read(&d.join(&log)));
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!((body("/e"), body("/o"), body("/r")), ("production".to_string(), "svc".to_string(), "real".to_string()), "split {}: {}", split, read(&d.join(&log)));
        sigterm(&g.0);
        assert!(wait_exit(&mut g.0, 60).is_some());
    }
}

/// `quillo mcp --serve`: after a tool that waits (`sleep` of 0, 1 and 20 ms, `http.get`, two waits
/// in `parallel`), the routes still answer; one answers while a tool sleeps; and stdin closing ends
/// the process (exit 0) once the call in progress is answered (src/mcp.rs serve_stdio, src/main.rs
/// serve_beside, FEEDBACK85.md Q-1347, found by the coverage tests; branch feedback82-85).
#[test]
fn mcp_serve_routes_answer_after_a_tool_that_waits() {
    let d = Tmp::new("mcp-lock");
    write(
        &d,
        "m.qwe",
        &format!(
            "{}/// Fetches a page.\nfn mcp fetch(url: str) -> str with http {{\n  return http.get(url, {{}}, {{ public_only: false }}).body\n}}\n/// Two waits at once.\nfn mcp both() -> str with time {{\n  let r = parallel(() => {{ sleep(5)\n return \"a\" }}, () => {{ sleep(5)\n return \"b\" }})\n  return r[0] + r[1]\n}}\n",
            SLOW
        ),
    );
    let pong = files_server(vec![("/pong", b"pong".to_vec())]);
    let mut g = start(&d, &["mcp", "m.qwe", "--serve", "--port", "0"], &[], "l.log");
    let port = port_in_log(&mut g.0, &d.join("l.log"), Duration::from_secs(60));
    let mut stdin = g.0.stdin.take().unwrap();
    assert_eq!(send(port, "GET", "/", &[], "").body, "home");
    let calls = [
        (r#""name":"slow","arguments":{"ms":0}"#.to_string(), "done after 0".to_string()),
        (r#""name":"slow","arguments":{"ms":1}"#.to_string(), "done after 1".to_string()),
        (r#""name":"slow","arguments":{"ms":20}"#.to_string(), "done after 20".to_string()),
        (format!(r#""name":"fetch","arguments":{{"url":"http://127.0.0.1:{}/pong"}}"#, pong), "pong".to_string()),
        (r#""name":"both","arguments":{}"#.to_string(), "ab".to_string()),
    ];
    for (i, (call, want)) in calls.iter().enumerate() {
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{{},{}}}}}"#, i + 1, call, META).unwrap();
        stdin.flush().unwrap();
        let text = wait_for(&d.join("l.log.out"), &format!(r#""id":{},"#, i + 1), 60);
        assert!(text.lines().last().unwrap().contains(&format!(r#""structuredContent":{{"result":"{}"}}"#, want)), "{}", text);
        assert_eq!(send(port, "GET", "/", &[], "").body, "home", "after {}: {}", call, read(&d.join("l.log")));
    }
    // A route answers while a tool sleeps: the lock is given up for the wait.
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{{"name":"slow","arguments":{{"ms":10000}},{}}}}}"#, META).unwrap();
    stdin.flush().unwrap();
    wait_for(&d.join("l.log"), "started 10000", 60);
    assert_eq!(send(port, "GET", "/", &[], "").body, "home");
    assert!(!read(&d.join("l.log.out")).contains(r#""id":9,"#), "answered before the route");
    // stdin closed: the call is answered, then the process ends.
    drop(stdin);
    assert_eq!(wait_exit(&mut g.0, 60), Some(0), "{}", read(&d.join("l.log")));
    assert!(read(&d.join("l.log.out")).contains("done after 10000"), "{}", read(&d.join("l.log.out")));
}

/// The stdio door of `quillo mcp --serve` offers no tool of a taken file that was not confirmed, as
/// the door alone does not: the main file's own is listed, the taken file's neither listed nor run;
/// a program that declares none still says it has none, and one whose only tool is held back does
/// not (src/main.rs mcp, FEEDBACK83.md Q-1301, found by the coverage tests; branch feedback82-85).
#[test]
fn mcp_serve_offers_no_unconfirmed_tool_on_stdio() {
    let d = Tmp::new("mcp-unconfirmed");
    write(&d, "main.qwe", "import kit/t.qwe use Badge\n/// Own tool.\nfn mcp own() -> str = \"own\"\nroute GET / = <html><body><Badge/></body></html>\n");
    write(&d, "kit/t.qwe", "component Badge() = <b>badge</b>\n/// Leaks.\nfn mcp leak() -> str with env = env(\"SECRET_X\") ?? \"\"\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n[\"kit/t.qwe\"]\n");
    let list = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{{}}}}}"#, META);
    let call = format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"leak","arguments":{{}},{}}}}}"#, META);
    let mut g = start(&d, &["mcp", "main.qwe", "--serve", "--port", "0"], &[("SECRET_X", "s3cr3t")], "u.log");
    let port = port_in_log(&mut g.0, &d.join("u.log"), Duration::from_secs(60));
    let mut stdin = g.0.stdin.take().unwrap();
    writeln!(stdin, "{}\n{}", list, call).unwrap();
    drop(stdin);
    assert_eq!(wait_exit(&mut g.0, 60), Some(0), "{}", read(&d.join("u.log")));
    let out = read(&d.join("u.log.out"));
    assert!(out.contains(r#""name":"own""#) && !out.contains(r#""name":"leak""#) && !out.contains("s3cr3t"), "{}", out);
    assert!(out.lines().nth(1).is_some_and(|l| l.contains(r#""id":2,"#) && l.contains("error")), "{}", out);
    assert!(!read(&d.join("u.log")).contains("has no tools"), "{}", read(&d.join("u.log")));
    let _ = port;
    // Its only tool held back: listed as none, without "has no tools", which is for a program that
    // declares none.
    write(&d, "only.qwe", "import kit/t.qwe use Badge\nroute GET / = <html><body><Badge/></body></html>\n");
    write(&d, "quillo.confirmed", "[\"only.qwe\"]\n[\"kit/t.qwe\"]\n");
    let mut g = start(&d, &["mcp", "only.qwe"], &[("SECRET_X", "s3cr3t")], "o.log");
    let mut stdin = g.0.stdin.take().unwrap();
    writeln!(stdin, "{}\n{}", list, call).unwrap();
    drop(stdin);
    assert_eq!(wait_exit(&mut g.0, 60), Some(0), "{}", read(&d.join("o.log")));
    let out = read(&d.join("o.log.out"));
    assert!(out.contains(r#""tools":[]"#) && !out.contains("s3cr3t"), "{}", out);
    assert!(!read(&d.join("o.log")).contains("has no tools"), "{}", read(&d.join("o.log")));
    write(&d, "none.qwe", "route GET / = \"home\"\n");
    write(&d, "quillo.confirmed", "[\"none.qwe\"]\n");
    let (code, out) = quillo(&d, &["mcp", "none.qwe"], &[]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("none.qwe has no tools"), "{}", out);
}

/// A developer's layer saying `[env] set` is refused in words that read: "not the program's own
/// (quillo.policy)" and "not a --policy file's", never "'s's" (src/policy.rs force_env, FEEDBACK82.md
/// Q-1280, found by the coverage tests; branch feedback82-85).
#[test]
fn env_set_in_a_developers_layer_is_refused_in_words_that_read() {
    for (kind, whose) in [(Kind::Program, "not the program's own (quillo.policy)"), (Kind::Named, "not a --policy file's")] {
        let e = policy::Policy::default().add_layer(Path::new("dev.toml"), "[env]\nset = { APP_ENV = \"dev\" }\n", kind).unwrap_err();
        assert!(e.contains("`env.set` is the host's to say") && e.ends_with(whose), "{:?}: {}", kind, e);
        assert!(!e.contains("'s's") && !e.contains("--policy's"), "{}", e);
    }
}

/// policy.d's layers are its `.toml` files whatever the case of the extension (`.TOML`, `.Toml`),
/// read in name order with the others, unicode names too; a file of another kind (`.toml.bak`,
/// `.tomlx`, one named `toml`) is not a layer (src/policy.rs load_from, FEEDBACK82.md Q-1281, found
/// by the coverage tests; branch feedback82-85).
#[test]
fn a_dropin_in_capitals_is_read() {
    let d = Tmp::new("dropin-case");
    let a = d.canonicalize().unwrap();
    let host = a.join("host/policy.toml");
    let main = a.join("app/main.qwe");
    write(&a, "host/policy.toml", "[limits]\nworkers = 8\n");
    write(&a, "host/policy.d/10.TOML", "[limits]\nworkers = 3\n");
    let p = policy::load_from(&host, &main, &[], None).unwrap();
    assert_eq!(p.workers, Some(3));
    assert_eq!(p.kinds, vec![Kind::Host, Kind::Dropin]);
    write(&a, "host/policy.d/20.Toml", "[limits]\nmax_body = \"1KB\"\n");
    write(&a, "host/policy.d/30-é.toml", "[http]\nprivate = false\n");
    write(&a, "host/policy.d/40.toml.bak", "[limits]\nworkers = 1\n");
    write(&a, "host/policy.d/50.tomlx", "[limits]\nworkers = 1\n");
    write(&a, "host/policy.d/toml", "[limits]\nworkers = 1\n");
    let p = policy::load_from(&host, &main, &[], None).unwrap();
    let names: Vec<String> = p.files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["policy.toml", "10.TOML", "20.Toml", "30-é.toml"]);
    assert_eq!((p.workers, p.max_body, p.private), (Some(3), Some(1024), false));
}

/// A connection closed under the policy's `max_connections` names the policy's file; under an
/// option as small, or smaller, the option, as before: two held open and the third closed under the
/// policy's 2 alone and with `--max-connections 2`; one and the second with `--max-connections 1`.
/// From this machine's address on its network, `--connections-per-client 2` under the policy's 3 is
/// the option's (src/httpd.rs place_for, src/main.rs policy_run, FEEDBACK82.md Q-1280, found by the
/// coverage tests; branch feedback82-85).
#[test]
fn a_connection_the_policy_closes_names_its_file() {
    let d = Tmp::new("max-conn");
    write(&d, "main.qwe", HOME);
    write(&d, "p.toml", "[run]\nmax_connections = 2\n");
    write(&d, "c.toml", "[run]\nconnections_per_client = 3\n");
    // Held open once answered (kept alive), so counted before the next comes, however slow the
    // machine: not after a fixed wait. One closed as it comes is tried again: a connection that
    // looked for the port may not be given back yet.
    let hold = |port: u16, ip: &str| {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let mut s = TcpStream::connect((ip, port)).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
            let _ = s.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
            let mut got = Vec::new();
            let mut buf = [0u8; 512];
            while !got.ends_with(b"home") {
                match s.read(&mut buf) {
                    Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]),
                    _ => break,
                }
            }
            if got.ends_with(b"home") {
                return s;
            }
            assert!(Instant::now() < deadline, "never answered: {}", String::from_utf8_lossy(&got));
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let refused = |port: u16, ip: &str| {
        let mut one_more = TcpStream::connect((ip, port)).unwrap();
        one_more.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let _ = one_more.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        let mut got = Vec::new();
        let _ = one_more.read_to_end(&mut got);
        !String::from_utf8_lossy(&got).contains("home")
    };
    for (i, (extra, held, said)) in [
        (&[][..], 2, "2 connections are open, the most the policy p.toml allows (run.max_connections)"),
        (&["--max-connections", "2"][..], 2, "2 connections are open, the most --max-connections allows"),
        (&["--max-connections", "1"][..], 1, "1 connections are open, the most --max-connections allows"),
    ]
    .into_iter()
    .enumerate()
    {
        let log = format!("m{}.log", i);
        let mut args = vec!["run", "main.qwe", "--port", "0", "--policy", "p.toml"];
        args.extend_from_slice(extra);
        let mut g = start(&d, &args, &[], &log);
        let port = port_in_log(&mut g.0, &d.join(&log), Duration::from_secs(60));
        let open: Vec<TcpStream> = (0..held).map(|_| hold(port, "127.0.0.1")).collect();
        assert!(refused(port, "127.0.0.1"), "{:?}", extra);
        let text = wait_for(&d.join(&log), said, 30);
        assert_eq!(text.contains("p.toml allows"), extra.is_empty(), "{:?}: {}", extra, text);
        drop(open);
        sigterm(&g.0);
        assert!(wait_exit(&mut g.0, 60).is_some());
    }
    if let Some(ip) = own_address() {
        let mut g = start(&d, &["run", "main.qwe", "--port", "0", "--host", "0.0.0.0", "--policy", "c.toml", "--connections-per-client", "2"], &[], "c.log");
        let port = port_in_log(&mut g.0, &d.join("c.log"), Duration::from_secs(60));
        let held: Vec<TcpStream> = (0..2).map(|_| hold(port, &ip)).collect();
        assert!(refused(port, &ip));
        let text = wait_for(&d.join("c.log"), &format!("{} holds 2 connections, the most --connections-per-client allows", ip), 30);
        assert!(!text.contains("c.toml allows"), "{}", text);
        drop(held);
        sigterm(&g.0);
        assert!(wait_exit(&mut g.0, 60).is_some());
    }
}

// ---- the marker and `--with`'s commands (fix slice F3) ------------------------------------------

/// A service its app started believes the names its app's `.env` gave (`QUILLO_ENV_FROM_FILE`),
/// and passed them on to the commands `--with` starts, which are no Quillo service: a `quillo run`
/// there was told which of its variables a file had set. The command now has no marker at all,
/// in such a service and in an app whose own `.env` set names; the service itself still believes
/// it (src/main.rs with::start, FEEDBACK82.md Q-1281, found by the review of bf40063; branch
/// feedback82-85).
#[test]
fn a_with_command_is_given_no_marker_of_what_a_dotenv_set() {
    let d = Tmp::new("with-marker");
    write(&d, "main.qwe", "print(env(\"REAL_X\") ?? \"none\")\nroute GET / = \"home\"\n");
    write(&d, ".env", "REAL_X=from the file\nOWN_Y=own\n");
    let probe = "sh -c 'echo marker=[${QUILLO_ENV_FROM_FILE-unset}] > marker.txt'";
    let wait_marker = |d: &Path, log: &str| {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !read(&d.join("marker.txt")).contains('\n') {
            assert!(Instant::now() < deadline, "{}", read(&d.join(log)));
            std::thread::sleep(Duration::from_millis(20));
        }
        let text = read(&d.join("marker.txt"));
        std::fs::remove_file(d.join("marker.txt")).unwrap();
        text
    };
    // A service its app started: it believes the marker (its own `.env` wins), its command sees none.
    let mut g = start(&d, &["run", "main.qwe", "--as-service", "--port", "0", "--with", probe], &[("REAL_X", "the app's file's"), ("QUILLO_ENV_FROM_FILE", "REAL_X"), ("QUILLO_STOP_WITH_APP", "1")], "s.log");
    assert_eq!(wait_marker(&d, "s.log"), "marker=[unset]\n");
    wait_for(&d.join("s.log.out"), "from the file\n", 60);
    drop(g.0.stdin.take());
    assert!(wait_exit(&mut g.0, 60).is_some());
    // An app whose `.env` set OWN_Y: the marker names it in the app, and its command has none.
    let mut g = start(&d, &["run", "main.qwe", "--port", "0", "--with", probe], &[], "a.log");
    assert_eq!(wait_marker(&d, "a.log"), "marker=[unset]\n");
    sigterm(&g.0);
    assert!(wait_exit(&mut g.0, 60).is_some());
}
