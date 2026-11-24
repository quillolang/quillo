//! Coverage of the lines of src/confirm.rs, src/check.rs, src/modules.rs and src/policy.rs that the
//! four fix rounds after the first coverage round added and the full run at 0038a97 left unrun
//! (FEEDBACK83.md, Q-1300, Q-1301 and Q-1305: the `quillo.confirmed` hold, `quillo confirm`, the
//! reading of a Rust file; coverage of feedback82-85; branch feedback82-85). Each test asserts what
//! a developer sees: the listing, the file written, the message and exit code, the answer a held
//! program gives; each refusal is tried another way round too.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::{port_in_log, send, temp_dir, Guard};

// ---- helpers, as tests/cover8285_confirm.rs has them --------------------------------------------

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
    Dir(temp_dir(&format!("cov8285b-{}", name)))
}

/// Where a test's program keeps the record of what its machine built: beside its folder.
fn record_dir(dir: &Path) -> PathBuf {
    dir.with_file_name(format!("{}-built", dir.file_name().unwrap().to_string_lossy()))
}

const SECRET: &str = "test secret for the tests, 32 bytes or more";

/// Waits for `child` up to `secs`, killing it past them: its exit code.
fn ended(child: &mut std::process::Child, secs: u64) -> Option<i32> {
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

/// Held while a test starts a process, and while the terminal test opens and closes its
/// terminal's other end: a process another test started in between took that end with it, kept
/// it open, and `quillo fix` printed with no error (seen once in a dozen runs, more with the
/// plugin builds of the tests for the review of 152bc1b).
static SPAWNING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `Command::spawn` while no other test opens a terminal.
trait SpawnHeld {
    fn spawn_held(&mut self) -> std::process::Child;
}

impl SpawnHeld for Command {
    fn spawn_held(&mut self) -> std::process::Child {
        let _one = SPAWNING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.spawn().unwrap()
    }
}

/// The command `quillo` with `args` in `dir`, as every test here starts it.
fn quillo_cmd(dir: &Path, program: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(program);
    c.args(args)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_BUILT_RECORD_DIR", record_dir(dir))
        .env("QUILLO_SECRET", SECRET)
        .stdin(Stdio::null());
    c
}

/// `quillo` with `args` in `dir`, ended after 60 s: its exit code and what it wrote.
fn quillo(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    quillo_logged(dir, &dir.join("cmd.log"), args)
}

/// `quillo`, what it writes kept in `log` until it ends.
fn quillo_logged(dir: &Path, log: &Path, args: &[&str]) -> (Option<i32>, String) {
    let log = log.to_path_buf();
    let file = std::fs::File::create(&log).unwrap();
    let mut child = quillo_cmd(dir, Path::new(env!("CARGO_BIN_EXE_quillo")), args).stdout(file.try_clone().unwrap()).stderr(file).spawn_held();
    let code = ended(&mut child, 60);
    let out = read(&log);
    let _ = std::fs::remove_file(&log);
    (code, out)
}

/// `quillo run main.qwe --port 0` in `dir` with `opts`: the guard, the port, the log.
fn serve(dir: &Path, opts: &[&str]) -> (Guard, u16, PathBuf) {
    let log = dir.join("run.log");
    let file = std::fs::File::create(&log).unwrap();
    let child = quillo_cmd(dir, Path::new(env!("CARGO_BIN_EXE_quillo")), &["run", "main.qwe", "--port", "0"]).args(opts).stdout(Stdio::null()).stderr(file).spawn_held();
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

/// Asks `path` until its body is `want`, up to 30 s (a `--watch` reload): whether it came.
fn body_becomes(port: u16, path: &str, want: &str) -> bool {
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        if send(port, "GET", path, &[], "").body == want {
            return true;
        }
        if Instant::now() > until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A line of `quillo confirm --list`: the words, the item, its lines.
fn list_line(word: &str, item: &str, lines: &str) -> String {
    format!("  {:<13} {:<40} {}\n", word, item, lines)
}

/// A cross-site form post to `path` (the CSRF gate's case): its status.
fn cross_site_post(port: u16, path: &str) -> u16 {
    send(port, "POST", path, &[("origin", "https://evil.example"), ("sec-fetch-site", "cross-site")], "").status
}

// ---- site_headers under the `site` key ---------------------------------------------------------

/// The kit's `site_headers` calls, each with its own header, so an answer says which rules hold.
const SITE_KIT: &str = "component Badge() {
  return <b>b</b>
}
let hs = { \"x-kit-var\": \"1\" }
let x = \"\"
site_headers(\"/admin/\", { \"x-kit-admin\": \"1\" })
site_headers(\"//\", { \"x-kit-all\": \"1\" })
site_headers(\"/pub\" + x, { \"x-kit-pub\": \"1\" })
site_headers(hs)
\"/pipe\" | site_headers({ \"x-kit-pipe\": \"1\" })
site_headers(\"/ü/\", { \"x-kit-u\": \"1\" })
site_headers(\"/admin/..\", { \"x-kit-up\": \"1\" })
site_headers(\"/%61dmin\", { \"x-kit-pct\": \"1\" })
site_headers(\"/admin\" + x, { \"x-kit-built\": \"1\" })
";

/// The kit's headers on an answer to `path`, by name, sorted.
fn kit_headers(port: u16, path: &str) -> Vec<String> {
    let mut hs: Vec<String> = send(port, "GET", path, &[], "").headers.into_iter().filter(|(k, _)| k.starts_with("x-kit-") || k == "x-own").map(|(k, _)| k).collect();
    hs.sort();
    hs
}

/// `site_headers(...)` in a file taken for some of its names is listed under `site` by the path the
/// builtin keeps (`site-wide site_headers /admin` for "/admin/", `/` for "//" and for a map alone,
/// `*` for a path worked out at run time or a map held in a name, the piped path, a Unicode path
/// without its `/`), and a run is held to it: with `site_headers /admin` and `/ü` confirmed by hand
/// those rules are applied and the others not, said once each; a path worked out as "/admin" is
/// applied (asked at the call, where the value is the one used), "/admin/.." and "/%61dmin" are
/// refused (not the item confirmed, though the second reads as /admin when applied). `quillo
/// confirm --yes` writes every item, and then each rule is applied, "/%61dmin" under /admin. The
/// main file's own rule is the program's throughout (src/confirm.rs site_prefix, site_item,
/// site_ok; FEEDBACK83.md Q-1301; coverage of feedback82-85; branch feedback82-85).
#[test]
fn site_headers_is_listed_by_the_path_the_builtin_keeps_and_held_to_it() {
    let d = dir("site-headers");
    write(&d, "main.qwe", "import kit/k.qwe use Badge\nsite_headers(\"/own/\", { \"x-own\": \"1\" })\nroute GET / = <html><body><Badge/></body></html>\nroute GET /admin/x = \"admin\"\nroute GET /adminx = \"adminx\"\nroute GET /ü/x = \"u\"\nroute GET /pub/y = \"pub\"\nroute GET /pipe = \"pipe\"\nroute GET /own/z = \"own\"\n");
    write(&d, "kit/k.qwe", SITE_KIT);
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let (code, out) = quillo(&d, &["confirm", "--list"]);
    assert_eq!(code, Some(0), "{}", out);
    let items = [("*", "8, 9, 14"), ("/", "7"), ("/%61dmin", "13"), ("/admin", "6"), ("/admin/..", "12"), ("/pipe", "10"), ("/ü", "11")];
    let want: String = items.iter().map(|(i, l)| list_line("site-wide", &format!("site_headers {}", i), l)).collect();
    assert!(out.starts_with(&format!("kit/k.qwe\n{}", want)), "{}", out);
    assert!(!out.contains("main.qwe\n"), "the main file's own rule is listed: {}", out);
    // Two confirmed by hand.
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n\n[\"kit/k.qwe\"]\nsite = [\"site_headers /admin\", \"site_headers /ü\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    for (i, l) in [("*", "8"), ("/", "7"), ("/%61dmin", "13"), ("/admin/..", "12"), ("/pipe", "10")] {
        assert!(out.contains(&format!("kit/k.qwe:{}: error: site-wide site_headers {}: not confirmed for this file (quillo.confirmed; `quillo confirm` lists it)", l, i)), "{}: {}", i, out);
    }
    assert!(!out.contains("site_headers /admin:") && !out.contains("site_headers /ü:"), "{}", out);
    let (g, port, log) = serve(&d, &["--no-check"]);
    assert_eq!(kit_headers(port, "/admin/x"), ["x-kit-admin", "x-kit-built"], "{}", read(&log));
    assert_eq!(kit_headers(port, "/admin"), ["x-kit-admin", "x-kit-built"]);
    assert_eq!(kit_headers(port, "/adminx"), Vec::<String>::new());
    assert_eq!(kit_headers(port, "/%C3%BC/x"), ["x-kit-u"]);
    for p in ["/", "/pub/y", "/pipe"] {
        assert_eq!(kit_headers(port, p), Vec::<String>::new(), "{}", p);
    }
    assert_eq!(kit_headers(port, "/own/z"), ["x-own"]);
    for i in ["/", "/pub", "/pipe", "/admin/..", "/%61dmin"] {
        assert!(log_has(&log, &format!("quillo: kit/k.qwe: site_headers {} is not applied: it is not confirmed for this file (`quillo confirm` lists it)\n", i), 30), "{}: {}", i, read(&log));
    }
    let said = read(&log);
    assert_eq!(said.matches("site_headers / is not applied").count(), 1, "said once for both of its calls: {}", said);
    assert!(!said.contains("site_headers /admin is not applied") && !said.contains("site_headers /ü is not applied"), "{}", said);
    drop(g);
    // Confirmed, all of them: the developer's.
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    let conf = read(&d.join("quillo.confirmed"));
    assert!(conf.contains("[\"kit/k.qwe\"]\nsite = [\"site_headers *\", \"site_headers /\", \"site_headers /%61dmin\", \"site_headers /admin\", \"site_headers /admin/..\", \"site_headers /pipe\", \"site_headers /ü\"]\n"), "{}", conf);
    assert_eq!(quillo(&d, &["check", "main.qwe"]).0, Some(0));
    let (g, port, log) = serve(&d, &[]);
    assert_eq!(kit_headers(port, "/"), ["x-kit-all", "x-kit-var"], "{}", read(&log));
    assert_eq!(kit_headers(port, "/admin/x"), ["x-kit-admin", "x-kit-all", "x-kit-built", "x-kit-pct", "x-kit-var"]);
    assert_eq!(kit_headers(port, "/pub/y"), ["x-kit-all", "x-kit-pub", "x-kit-var"]);
    assert_eq!(kit_headers(port, "/pipe"), ["x-kit-all", "x-kit-pipe", "x-kit-var"]);
    assert!(!read(&log).contains("is not applied"), "{}", read(&log));
    drop(g);
}

// ---- a file no start read, under --watch -------------------------------------------------------

/// Under `--watch`, a file the main file takes since the start, with no block in quillo.confirmed:
/// its `site_headers` is not applied and its `db.open` gives an error value naming it, each said
/// with "its file was not read as the program started (restart it once it is confirmed)"; its
/// route is not served; the main file's own rule and routes are. After a restart with it confirmed,
/// all of it is (src/confirm.rs site_ok, held_name, held_taken; FEEDBACK83.md Q-1301; coverage of
/// feedback82-85; branch feedback82-85).
#[test]
fn a_site_wide_call_of_a_file_taken_under_watch_is_not_applied_until_a_restart() {
    let d = dir("watch-site");
    write(&d, "main.qwe", "site_headers({ \"x-own\": \"1\" })\nroute GET / = \"one\"\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nsite_headers(\"/\", { \"x-late\": \"1\" })\nlet opened = db.open(\":memory:\")\nroute GET /k = \"k\"\n");
    let (g, port, log) = serve(&d, &["--watch"]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "one");
    write(&d, "main.qwe", "import kit/k.qwe use Badge, opened\nsite_headers({ \"x-own\": \"1\" })\nroute GET / = \"two\"\nroute GET /db = str(opened)\n");
    assert!(body_becomes(port, "/", "two"), "{}", read(&log));
    let page = send(port, "GET", "/", &[], "");
    assert_eq!((page.header("x-own"), page.header("x-late")), (Some("1"), None), "{:?}", page.headers);
    assert_eq!(send(port, "GET", "/db", &[], "").body, "err(\"db: db.open sets every file's database, and is not confirmed for kit/k.qwe (quillo confirm)\")");
    assert_eq!(send(port, "GET", "/k", &[], "").status, 404);
    for what in ["site_headers / is not applied", "db.open is not applied", "GET /k is not served"] {
        assert!(log_has(&log, &format!("quillo: kit/k.qwe: {}: its file was not read as the program started (restart it once it is confirmed)\n", what), 30), "{}: {}", what, read(&log));
    }
    drop(g);
    // Restarted with it confirmed: applied.
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(read(&d.join("quillo.confirmed")).contains("[\"kit/k.qwe\"]\nroutes = [\"GET /k\"]\nsite = [\"db.open\", \"site_headers /\"]\n"), "{}", read(&d.join("quillo.confirmed")));
    let (g, port, log) = serve(&d, &["--watch"]);
    assert_eq!(send(port, "GET", "/", &[], "").header("x-late"), Some("1"), "{}", read(&log));
    assert_eq!(send(port, "GET", "/db", &[], "").body, "null");
    assert_eq!(send(port, "GET", "/k", &[], "").body, "k");
    drop(g);
}

/// BUG (found by this coverage work; FEEDBACK83.md Q-1301): a file that has a block in
/// quillo.confirmed but that the program did not take at the start (a block left from before, or
/// written for a file not used yet) is held as the program's own once `--watch` takes it for a
/// name: its routes are served, its `site_headers` and `allow_cross_site("/")` applied, though its
/// block confirms none of them (only `files = ["kit/"]`). A file with no block is refused all three
/// ("its file was not read as the program started"). `Hold::id` gives it its block, and
/// `routes_held` holds only files the start found taken, so `route_ok`, `site_ok` and
/// `held_taken` take it for the program's own. What it should do: refuse them as for a file with no
/// block, until a restart reads it (coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_file_with_a_block_taken_under_watch_is_held_as_one_no_start_read() {
    let d = dir("watch-block");
    write(&d, "main.qwe", "route GET / = \"one\"\nroute POST /transfer = \"transferred\"\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n\n[\"kit/k.qwe\"]\nfiles = [\"kit/\"]\n");
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nsite_headers(\"/\", { \"x-left\": \"1\" })\nallow_cross_site(\"/\")\nroute GET /k = \"k\"\n");
    let (g, port, log) = serve(&d, &["--watch"]);
    assert_eq!(cross_site_post(port, "/transfer"), 403);
    write(&d, "main.qwe", "import kit/k.qwe use Badge\nroute GET / = <html><body><Badge/></body></html>\nroute POST /transfer = \"transferred\"\n");
    let until = Instant::now() + Duration::from_secs(30);
    while !send(port, "GET", "/", &[], "").body.contains("<b>b</b>") && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(send(port, "GET", "/", &[], "").body.contains("<b>b</b>"), "{}", read(&log));
    assert_eq!(send(port, "GET", "/", &[], "").header("x-left"), None);
    assert_eq!(send(port, "GET", "/k", &[], "").status, 404);
    assert_eq!(cross_site_post(port, "/transfer"), 403);
    drop(g);
}

// ---- `quillo confirm` writing its file ---------------------------------------------------------

/// `quillo confirm --yes` where its file cannot be written: in a folder it may not write, the temp
/// file is not made and it says so with exit 1, nothing left behind; with something that is not a
/// file at the temp file's name (a folder, as a run of the same process number would not leave),
/// it says the name is taken and leaves that alone; `quillo.confirmed` is not written either way.
/// `-h` is answered by `quillo` itself before the command runs (src/confirm.rs write; FEEDBACK83.md
/// Q-1300; coverage of feedback82-85; branch feedback82-85).
#[cfg(unix)]
#[test]
fn confirm_says_why_its_file_cannot_be_written() {
    use std::os::unix::fs::PermissionsExt;
    let d = dir("write-fails");
    write(&d, "main.qwe", "import kit/k.qwe use Badge\nroute GET / = <html><body><Badge/></body></html>\n");
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nroute GET /k = \"k\"\n");
    // A folder at the temp file's name: `exec` keeps the shell's process number for quillo.
    let sh = format!("mkdir .quillo.confirmed.$$.part && echo $$ > pid.txt && exec '{}' confirm --yes", env!("CARGO_BIN_EXE_quillo"));
    let log = d.join("sh.log");
    let file = std::fs::File::create(&log).unwrap();
    let mut child = quillo_cmd(&d, Path::new("/bin/sh"), &["-c", &sh]).stdout(file.try_clone().unwrap()).stderr(file).spawn_held();
    assert_eq!(ended(&mut child, 60), Some(1), "{}", read(&log));
    let pid = read(&d.join("pid.txt")).trim().to_string();
    let part = format!(".quillo.confirmed.{}.part", pid);
    assert!(read(&log).contains(&format!("quillo confirm: ./{}: File exists (os error 17)\n", part)), "{}", read(&log));
    assert!(d.join(&part).is_dir(), "the folder there is left alone");
    assert!(!d.join("quillo.confirmed").exists());
    std::fs::remove_dir(d.join(&part)).unwrap();
    // A folder it may not write (skipped where the test runs as a user every folder lets write);
    // what quillo says is kept outside it.
    let outside = dir("write-fails-log");
    std::fs::set_permissions(&*d, std::fs::Permissions::from_mode(0o555)).unwrap();
    let writable = std::fs::write(d.join("probe"), "").is_ok();
    let (code, out) = quillo_logged(&d, &outside.join("cmd.log"), &["confirm", "--yes"]);
    let left: Vec<String> = std::fs::read_dir(&*d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".part")).collect();
    std::fs::set_permissions(&*d, std::fs::Permissions::from_mode(0o755)).unwrap();
    if !writable {
        assert_eq!(code, Some(1), "{}", out);
        assert!(out.contains("quillo confirm: ./.quillo.confirmed.") && out.contains(".part: Permission denied (os error 13)\n"), "{}", out);
        assert!(left.is_empty(), "{:?}", left);
        assert!(!d.join("quillo.confirmed").exists());
    }
    // Written once it may.
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(read(&d.join("quillo.confirmed")).contains("[\"kit/k.qwe\"]\nroutes = [\"GET /k\"]\n"));
    let (code, out) = quillo(&d, &["confirm", "--yes", "--help"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.starts_with("usage:\n  quillo confirm [<file.qwe>] [PATH...]"), "{}", out);
}

// ---- quillo.confirmed's text -------------------------------------------------------------------

/// A text in quillo.confirmed cut at a backslash, with nothing after it, is a list that does not
/// end: `quillo check` says it with its line, `quillo confirm` stops with 2 and writes nothing, a
/// run does not start; a SHA-256 cut so is no text; the same text closed is read (src/confirm.rs
/// unquote, parse; FEEDBACK83.md
/// Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_text_cut_at_its_backslash_is_a_list_that_does_not_end() {
    let d = dir("backslash");
    write(&d, "main.qwe", "route GET / = read_file(\"data\\\\x\") ?? \"none\"\n");
    let cut = "[\"main.qwe\"]\nfiles = [\"data\\";
    write(&d, "quillo.confirmed", cut);
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("./quillo.confirmed: error: ./quillo.confirmed: line 2: a list that does not end"), "{}", out);
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!((code, out.trim()), (Some(2), "quillo confirm: ./quillo.confirmed: line 2: a list that does not end"));
    assert_eq!(read(&d.join("quillo.confirmed")), cut);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0", "--no-check"]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("quillo: ./quillo.confirmed: line 2: a list that does not end"), "{}", out);
    // A text on its own (a SHA-256) cut the same way is no text.
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nfiles = [\"data\\\\x\"]\n\n[\"hi.qwer\"]\nsha256 = \"00\\\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("./quillo.confirmed: error: ./quillo.confirmed: line 5: `sha256` is a text: \"...\""), "{}", out);
    // Closed, `\\` is one backslash, the path the code reads.
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nfiles = [\"data\\\\x\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
}

/// A pattern written by hand (`*.example.com`) stays when its file is confirmed again, and what it
/// covers is not written beside it; only what it does not cover is added (src/confirm.rs merged;
/// FEEDBACK83.md Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_hand_written_pattern_stays_when_its_file_is_confirmed_again() {
    let d = dir("pattern-kept");
    write(&d, "main.qwe", "import kit/k.qwe use Badge, go\nroute GET / = <html><body><Badge/>{go()}</body></html>\n");
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nfn go() {\n  let a = http.get(\"https://api.example.com/x\")\n  let b = http.get(\"https://other.org/y\")\n  return \"\"\n}\n");
    assert_eq!(quillo(&d, &["check", "main.qwe"]).0, Some(0));
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n\n[\"kit/k.qwe\"]\nhttp = [\"*.example.com\"]\n");
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("kit/k.qwe\n    reaches       other.org") && !out.contains("api.example.com"), "{}", out);
    assert!(read(&d.join("quillo.confirmed")).ends_with("[\"kit/k.qwe\"]\nhttp = [\"*.example.com\", \"other.org\"]\n"), "{}", read(&d.join("quillo.confirmed")));
    // Again: nothing left, nothing written beside the pattern.
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(read(&d.join("quillo.confirmed")).ends_with("http = [\"*.example.com\", \"other.org\"]\n"));
}

// ---- the scan's listing ------------------------------------------------------------------------

/// Paths as the listing shows them: an absolute path with `..` after a name kept as written, `/`
/// first (the system takes the `..` from where the name leads), `./` taken away; `translations()`
/// with no folder lists the call and no folder (src/confirm.rs listed, read_calls; FEEDBACK83.md
/// Q-1300; coverage of feedback82-85; branch feedback82-85).
#[test]
fn paths_are_listed_as_the_system_takes_them() {
    let d = dir("listed");
    write(&d, "main.qwe", "import kit/k.qwe use Badge\nroute GET / = <html><body><Badge/></body></html>\n");
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\ntranslations()\nlet a = read_file(\"/tmp/qa/../qb\")\nlet b = read_file(\"./kit/./x.txt\")\nlet c = read_file(\"kit/sub/../y.txt\")\nlet e = read_file(\"/ü/../ä\")\n");
    let (code, out) = quillo(&d, &["confirm", "--list"]);
    assert_eq!(code, Some(0), "{}", out);
    let want = [list_line("reads", "/tmp/qa/../qb", "5"), list_line("reads", "/ü/../ä", "8"), list_line("reads", "kit/sub/../y.txt", "7"), list_line("reads", "kit/x.txt", "6"), list_line("site-wide", "translations", "4")].concat();
    assert!(out.starts_with(&format!("kit/k.qwe\n{}", want)), "{}", out);
    // translations() itself is check's error: it needs its folder.
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("kit/k.qwe:4: error: missing argument `dir` in call to `translations`"), "{}", out);
}

/// What an import hands over, as `quillo confirm --list` and check under a hold say it: through a
/// global set again at the top (`r = read_file` after `let r = now`), through a member of another
/// global (`helper.tools`, `api.get`, both members of one in a list), each name said once though it
/// is taken twice (`r, r as r2`); a plain value and a member of a module that reaches nothing
/// (`math.pi`) hand nothing (src/confirm.rs carried, scan_from's grants; FEEDBACK83.md Q-1300;
/// coverage of feedback82-85; branch feedback82-85).
#[test]
fn what_an_import_hands_over_follows_assignments_and_members() {
    let d = dir("grants");
    write(&d, "main.qwe", "import kit/k.qwe use Badge, go\nlet api = { get: http.get, put: http.post }\nlet g = api.get\nlet r = now\nr = read_file\nlet helper = { tools: { load: read_file } }\nlet deep = helper.tools\nlet plain = { n: 1 }\nlet pi = math.pi\nlet both = [api.get, api.put]\nroute GET / = <html><body><Badge/>{go()}</body></html>\n");
    write(&d, "kit/k.qwe", "import main.qwe use g, r, r as r2, deep, plain, pi, both\ncomponent Badge() {\n  return <b>b</b>\n}\nfn go() {\n  return str(plain.n) + str(pi > 3) + str(len(both)) + str(is_err(r2(\"nothing-here\")))\n}\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let (code, out) = quillo(&d, &["confirm", "--list"]);
    assert_eq!(code, Some(0), "{}", out);
    let want = "kit/k.qwe\n  note: kit/k.qwe takes main.qwe's g (http.get, http.post, as main.qwe reaches it)\n  note: kit/k.qwe takes main.qwe's r (read_file, as main.qwe reaches it)\n  note: kit/k.qwe takes main.qwe's deep (read_file, as main.qwe reaches it)\n  note: kit/k.qwe takes main.qwe's both (http.get, http.post, as main.qwe reaches it)\n";
    assert_eq!(out, want);
    // `math.pi` (a module that reaches nothing) and `plain` hand nothing.
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("kit/k.qwe:1: warning: kit/k.qwe takes main.qwe's r (read_file, as main.qwe reaches it)"), "{}", out);
}

/// A mounted program's main file, taken by name by a file of its own program, stays the mounted
/// program's own (its route is not listed) while that program also takes a Rust file, which the
/// scan reads apart: the Rust file is listed by its SHA-256 alone (src/confirm.rs scan_from's
/// own_program; FEEDBACK83.md Q-1301; coverage of feedback82-85; branch feedback82-85).
#[test]
fn a_mounted_programs_own_main_stays_its_own_beside_a_rust_file() {
    let d = dir("own-with-rust");
    write(&d, "main.qwe", "mount /sub = \"sub/main.qwe\"\nroute GET / = \"main\"\n");
    write(&d, "sub/main.qwe", "use part.qwe\nimport hi.qwer use hi\nfn own() {\n  return \"own\"\n}\nroute GET / = part() + hi()\n");
    write(&d, "sub/part.qwe", "import main.qwe use own\nfn part() {\n  return own()\n}\n");
    let rust = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n";
    write(&d, "sub/hi.qwer", rust);
    let (code, out) = quillo(&d, &["confirm", "--list"]);
    assert_eq!(code, Some(0), "{}", out);
    use sha2::Digest;
    let sha: String = sha2::Sha256::digest(rust.as_bytes()).iter().map(|x| format!("{:02x}", x)).collect();
    assert_eq!(out, format!("sub/hi.qwer\n  sha256        {}\n  says effects  none\n  reads as      none\n", sha));
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("sub/hi.qwer: error: sub/hi.qwer is not confirmed") && !out.contains("GET /"), "{}", out);
}

// ---- paths held where they really are ----------------------------------------------------------

/// A path confirmed for a file through a link that leads nowhere covers nothing, and the file's
/// other confirmed paths still hold: `ok.txt` is read, `gone/x.txt` refused (the link named, where
/// it leads), `main.qwe` refused as not confirmed (src/confirm.rs real_under, path_in;
/// FEEDBACK83.md Q-1300; coverage of feedback82-85; branch feedback82-85).
#[cfg(unix)]
#[test]
fn a_confirmed_path_through_a_link_to_nowhere_covers_nothing() {
    let d = dir("dangling-item");
    write(&d, "main.qwe", "import kit/k.qwe use Badge, rd\nroute GET / = <html><body><Badge/></body></html>\nroute GET /r = str(rd(request.query.p ?? \"\"))\n");
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nfn rd(p) {\n  return read_file(p)\n}\n");
    write(&d, "ok.txt", "ok");
    std::os::unix::fs::symlink("nowhere/at/all", d.join("gone")).unwrap();
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n\n[\"kit/k.qwe\"]\nfiles = [\"gone/x.txt\", \"ok.txt\"]\n");
    let (g, port, log) = serve(&d, &["--no-check"]);
    assert_eq!(send(port, "GET", "/r?p=ok.txt", &[], "").body, "ok", "{}", read(&log));
    assert_eq!(send(port, "GET", "/r?p=gone/x.txt", &[], "").body, "err(\"internal error\")");
    assert!(log_has(&log, "read_file: path must stay inside the program's folder: gone/x.txt; gone is a link that leads nowhere (to nowhere/at/all)", 30), "{}", read(&log));
    assert_eq!(send(port, "GET", "/r?p=main.qwe", &[], "").body, "err(\"internal error\")");
    assert!(log_has(&log, "quillo: kit/k.qwe: the path main.qwe was refused: it is not confirmed for this file (`quillo confirm` lists it)", 30), "{}", read(&log));
    drop(g);
}

// ---- names a file uses without taking them -----------------------------------------------------

/// The program `depth` mounts down, its last a main file with `let reader = read_file`, confirmed
/// to read its `.env`, and a kit confirmed for nothing that calls `reader(".env")` without taking
/// it: the path of the last program's folder from `d`.
fn mounted_down(d: &Path, depth: usize) -> String {
    let mut at = String::new();
    for i in 0..depth {
        write(d, &format!("{}main.qwe", at), &format!("mount /m = \"m/main.qwe\"\nroute GET / = \"level {}\"\n", i));
        at.push_str("m/");
    }
    write(d, &format!("{}main.qwe", at), "import kit.qwe use Badge, go\nlet reader = read_file\nroute GET / = <html><body><Badge/>{go()}</body></html>\n");
    write(d, &format!("{}kit.qwe", at), "component Badge() {\n  return <b>b</b>\n}\nfn go() {\n  return str(reader(\".env\"))\n}\n");
    write(d, &format!("{}.env", at), "SECRET=hunter2\n");
    write(d, "quillo.confirmed", &format!("[\"main.qwe\"]\n\n[\"{}main.qwe\"]\nfiles = [\".env\"]\n", at));
    at
}

/// Under a quillo.confirmed a file using a name another file defines without taking it is refused
/// as the program loads, `--no-check` or not: a mounted program's kit that calls its main file's
/// `reader` (`let reader = read_file`, the main confirmed for `.env`) does not start, said with
/// the file and line; the program checked stops on the same error (src/check.rs untaken_at_load,
/// src/interp.rs load_file; FEEDBACK83.md Q-1300, the user's decision of 2026-11-24; coverage of
/// feedback82-85; branch feedback82-85).
#[test]
fn a_name_not_taken_stops_a_held_start_unchecked() {
    let d = dir("untaken-mounted");
    mounted_down(&d, 1);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0", "--no-check"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("quillo: a quillo.confirmed holds the program, and 1 of its files uses a name another file defines without taking it, so that it would reach what that file was confirmed for: the program is not started\n  ./m/kit.qwe:5: error: `reader` comes from main.qwe, which this file does not take"), "{}", out);
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("m/kit.qwe:5: error: `reader` comes from main.qwe, which this file does not take"), "{}", out);
}

/// BUG (found by this coverage work; FEEDBACK83.md Q-1300): past 16 programs mounted one in
/// another, the check `untaken_at_load` runs stops reading ("programs mount or call each other
/// more than 16 deep", a diagnostic it passes over: it refuses only on TOO_COMPLEX, which a plain
/// check never says, and on the names it found), so under `--no-check` the program starts and the
/// 17th program's kit, confirmed for nothing, reads `.env` through its main file's `reader`:
/// `SECRET=hunter2` in the page. One program down, the start is refused (the test above). What it
/// should do: refuse a held start whose check could not read every program (coverage of
/// feedback82-85; branch feedback82-85).
#[test]
fn a_name_not_taken_stops_a_held_start_however_deep_its_program_is() {
    let d = dir("untaken-deep");
    let at = mounted_down(&d, 17);
    let log = d.join("run.log");
    let child = quillo_cmd(&d, Path::new(env!("CARGO_BIN_EXE_quillo")), &["run", "main.qwe", "--port", "0", "--no-check"]).stdout(Stdio::null()).stderr(std::fs::File::create(&log).unwrap()).spawn_held();
    let mut g = Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(60);
    while !read(&log).contains("listening on") && g.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if let Some(port) = support::port_said_in(&log) {
        let page = send(port, "GET", &format!("/{}", at), &[], "").body;
        assert!(!page.contains("hunter2"), "the kit read the .env: {}", page);
    }
    drop(g);
    assert!(read(&log).contains("the program is not started"), "{}", read(&log));
}

// ---- quillo fix under a hold -------------------------------------------------------------------

/// Under a quillo.confirmed, a browser library a file uses without its own `client` line is an
/// error, which `quillo fix` exists to mend: it writes the line and check passes after. Its
/// terminal gone before it prints (an I/O error, not a closed pipe), the files are written and the
/// failure is said as Rust says it, exit 1 (src/modules.rs fix; FEEDBACK83.md Q-1300; coverage of
/// feedback82-85; branch feedback82-85).
#[cfg(unix)]
#[test]
fn fix_under_a_hold_writes_a_browser_librarys_line() {
    let d = dir("fix-browser");
    write(&d, "main.qwe", "use lib/a.qwe\nclient /assets/chart.js use Chart\nroute GET / = <html><body><C/></body></html>\n");
    write(&d, "lib/a.qwe", "component C() = <p>{Chart.version}</p>\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("./lib/a.qwe:1: error: `Chart` is a browser library this file does not take, and under quillo.confirmed a file uses only the names it takes"), "{}", out);
    let (code, out) = quillo(&d, &["fix", "main.qwe"]);
    assert_eq!((code, out.as_str()), (Some(0), "./lib/a.qwe: client /assets/chart.js use Chart\n"));
    assert_eq!(read(&d.join("lib/a.qwe")), "client /assets/chart.js use Chart\n\ncomponent C() = <p>{Chart.version}</p>\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    // Again, printing to a terminal whose other end is closed.
    write(&d, "lib/a.qwe", "component C() = <p>{Chart.version}</p>\n");
    let (mut master, mut slave) = (0, 0);
    let one = SPAWNING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: openpty fills in two descriptors; no names or settings are given.
    assert_eq!(unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) }, 0);
    use std::os::fd::FromRawFd;
    // SAFETY: each descriptor is this test's, and owned once.
    let (m, s) = unsafe { (std::fs::File::from_raw_fd(master), std::fs::File::from_raw_fd(slave)) };
    drop(m);
    drop(one);
    let log = d.join("fix.log");
    let mut child = quillo_cmd(&d, Path::new(env!("CARGO_BIN_EXE_quillo")), &["fix", "main.qwe"]).stdout(s).stderr(std::fs::File::create(&log).unwrap()).spawn_held();
    assert_eq!(ended(&mut child, 60), Some(1), "{}", read(&log));
    assert!(read(&log).contains("failed printing to stdout: Input/output error (os error 5)"), "{}", read(&log));
    assert_eq!(read(&d.join("lib/a.qwe")), "client /assets/chart.js use Chart\n\ncomponent C() = <p>{Chart.version}</p>\n");
}

// ---- the reading of a Rust file ----------------------------------------------------------------

/// `quillo confirm --effects` on Rust files: `use` inside a word (`user`, `reuse`, `fuse`) is no
/// declaration, `extern crate core;` and `extern crate ;` name nothing, and a glob (`use std::fs::*;`) is read as
/// std::fs, so its bare calls say fs (src/confirm.rs rust_use_names, rust_use_tree; FEEDBACK83.md
/// Q-1305; coverage of feedback82-85; branch feedback82-85).
#[test]
fn the_rust_reading_passes_words_and_reads_a_glob() {
    let d = dir("rust-words");
    write(&d, "words.qwer", "//! version: 1.0.0\n//! effects: none\nextern crate core;\n// the user's reuse of a fuse: no `use` here\npub fn user() -> String { \"reused\".into() }\n");
    write(&d, "glob.qwer", "//! version: 1.0.0\n//! effects: none\nuse std::fs::*;\npub fn hosts() -> String { read_to_string(\"/etc/hosts\").unwrap_or_default() }\n");
    write(&d, "named.qwer", "//! version: 1.0.0\n//! effects: none\nextern crate std as s;\nuse std::{fs::read_to_string};\npub fn hosts() -> String { read_to_string(\"/etc/hosts\").unwrap_or_default() }\n");
    // An `extern crate` with no name: nothing named, nothing read into it.
    write(&d, "empty.qwer", "//! version: 1.0.0\n//! effects: none\nextern crate ;\npub fn a() -> String { \"a\".into() }\n");
    let (code, out) = quillo(&d, &["confirm", "--effects", "words.qwer", "empty.qwer"]);
    assert_eq!((code, out.as_str()), (Some(0), "words.qwer: effects: none\nempty.qwer: effects: none\n"));
    let (code, out) = quillo(&d, &["confirm", "--effects", "glob.qwer", "named.qwer"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("glob.qwer: effects: fs\n  but it says `effects: none`, and calls std::fs (line 3)"), "{}", out);
    assert!(out.contains("named.qwer: effects: fs\n  but it says `effects: none`, and calls fs::read_to_string (line 4), std::fs::read_to_string (line 5)"), "{}", out);
}

/// BUG (found by this coverage work; FEEDBACK83.md Q-1305): a glob inside a group, `use
/// std::{fs::*};` (or `use std::{fs::*, io};`, `use std::{fs::{*}};`), then a bare
/// `read_to_string("/etc/hosts")`, reads as `effects: none`, so a plugin that says `effects: none`
/// and reads any file passes `quillo check` and `quillo confirm --effects`: rust_use_tree returns
/// at the `*` and names nothing, and no pattern sees `std::fs::` in `std::{fs::*}`. `use
/// std::fs::*;` and `use std::fs::{*};` read as fs. What it should do: read each as fs (coverage of
/// feedback82-85; branch feedback82-85).
#[test]
fn a_glob_in_a_group_is_read_as_what_it_takes() {
    let d = dir("rust-glob-group");
    for (i, u) in ["use std::{fs::*};", "use std::{fs::*, io};", "use std::{fs::{*}};", "use std :: { fs :: * } ;"].iter().enumerate() {
        let f = format!("g{}.qwer", i);
        write(&d, &f, &format!("//! version: 1.0.0\n//! effects: none\n{}\npub fn hosts() -> String {{ read_to_string(\"/etc/hosts\").unwrap_or_default() }}\n", u));
        let (code, out) = quillo(&d, &["confirm", "--effects", &f]);
        assert_eq!(code, Some(1), "{}: {}", u, out);
        assert!(out.starts_with(&format!("{}: effects: fs\n", f)), "{}: {}", u, out);
    }
}

// ---- edge tests of the fixes of Q-1301, Q-1305 and Q-1300 ------------------------------------

/// Under `--watch`, files no start read that have blocks in quillo.confirmed confirming what they
/// do are held as files with none, however the main file takes them since: a kit taken whole has
/// its route not served, its `site_headers` not applied and the path its block confirms refused; a
/// program mounted since is not served; a service called since has its `server fn` not offered;
/// each said with "its file was not read as the program started". After a restart, each is held
/// to its block (src/confirm.rs, FEEDBACK83.md Q-1301, found by the coverage tests; branch feedback82-85).
#[test]
fn files_no_start_read_are_held_as_ones_with_no_block_however_they_are_taken() {
    let d = dir("watch-block-ways");
    write(&d, "main.qwe", "route GET / = \"one\"\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n\n[\"kit/k.qwe\"]\nfiles = [\"kit/\"]\nroutes = [\"GET /k\"]\nsite = [\"site_headers /\"]\n\n[\"sub/main.qwe\"]\nfiles = [\"sub/\"]\n\n[\"svc/billing.qwe\"]\nfiles = [\"svc/\"]\n");
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nfn rd() {\n  return str(read_file(\"kit/data.txt\"))\n}\nsite_headers(\"/\", { \"x-left\": \"1\" })\nroute GET /k = \"k\"\n");
    write(&d, "kit/data.txt", "kit-data");
    write(&d, "sub/main.qwe", "site_headers(\"/\", { \"x-sub\": \"1\" })\nroute GET / = \"sub\"\n");
    write(&d, "svc/billing.qwe", "server fn data() {\n  return \"svc-data\"\n}\n");
    let program = "use kit/k.qwe\nmount /s = \"sub/main.qwe\"\nservice billing = \"svc/billing.qwe\"\nroute GET / = <html><body><Badge/></body></html>\nroute GET /r = rd()\nroute GET /d = billing.data()\n";
    assert_eq!(quillo(&d, &["check", "main.qwe"]).0, Some(0));
    let (g, port, log) = serve(&d, &["--watch"]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "one");
    write(&d, "main.qwe", program);
    let until = Instant::now() + Duration::from_secs(30);
    while !send(port, "GET", "/", &[], "").body.contains("<b>b</b>") && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    let page = send(port, "GET", "/", &[], "");
    assert!(page.body.contains("<b>b</b>"), "{}", read(&log));
    assert_eq!((page.header("x-left"), page.header("x-sub")), (None, None), "{:?}", page.headers);
    assert_eq!(send(port, "GET", "/k", &[], "").status, 404);
    assert_eq!(send(port, "GET", "/s/", &[], "").status, 404);
    assert_ne!(send(port, "GET", "/r", &[], "").body, "kit-data");
    let data = send(port, "GET", "/d", &[], "");
    assert!(data.status == 500 && !data.body.contains("svc-data"), "{} {}", data.status, data.body);
    for (file, what) in [("kit/k.qwe", "site_headers / is not applied"), ("kit/k.qwe", "GET /k is not served"), ("sub/main.qwe", "GET / is not served"), ("svc/billing.qwe", "server fn data is not served")] {
        assert!(log_has(&log, &format!("quillo: {}: {}: its file was not read as the program started (restart it once it is confirmed)\n", file, what), 30), "{}: {}", what, read(&log));
    }
    assert!(log_has(&log, "quillo: kit/k.qwe: the path kit/data.txt was refused: it is not confirmed for this file", 30), "{}", read(&log));
    drop(g);
    // Restarted: each file read, each held to its block (the kit, taken whole, is the program's).
    let (g, port, log) = serve(&d, &["--no-check"]);
    assert_eq!(send(port, "GET", "/", &[], "").header("x-left"), Some("1"), "{}", read(&log));
    assert_eq!(send(port, "GET", "/k", &[], "").body, "k");
    assert_eq!(send(port, "GET", "/s/", &[], "").body, "sub");
    assert_eq!(send(port, "GET", "/r", &[], "").body, "kit-data");
    assert_eq!(send(port, "GET", "/d", &[], "").body, "svc-data", "{}", read(&log));
    drop(g);
}

/// A file the start read is held to its block as the start read it: under `--watch`, a kit taken
/// for `Badge` whose block is given `routes = ["GET /k"]` has its route still not served after
/// the kit and the file are written again, said as not confirmed; a restart reads the block and
/// serves it (src/confirm.rs, FEEDBACK83.md Q-1301, found by the coverage tests; branch feedback82-85).
#[test]
fn a_block_edited_under_watch_holds_from_the_next_start() {
    let d = dir("watch-block-edit");
    write(&d, "main.qwe", "import kit/k.qwe use Badge\nroute GET / = <html><body><Badge/></body></html>\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n\n[\"kit/k.qwe\"]\n");
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>b</b>\n}\nroute GET /k = \"k\"\n");
    let (g, port, log) = serve(&d, &["--watch"]);
    assert_eq!(send(port, "GET", "/k", &[], "").status, 404, "{}", read(&log));
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n\n[\"kit/k.qwe\"]\nroutes = [\"GET /k\"]\n");
    write(&d, "kit/k.qwe", "component Badge() {\n  return <b>bb</b>\n}\nroute GET /k = \"k\"\n");
    let until = Instant::now() + Duration::from_secs(30);
    while !send(port, "GET", "/", &[], "").body.contains("<b>bb</b>") && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(send(port, "GET", "/", &[], "").body.contains("<b>bb</b>"), "{}", read(&log));
    assert_eq!(send(port, "GET", "/k", &[], "").status, 404);
    assert!(log_has(&log, "quillo: kit/k.qwe: the route GET /k is not served: it is not confirmed for this file (`quillo confirm` lists it)\n", 30), "{}", read(&log));
    drop(g);
    let (g, port, log) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/k", &[], "").body, "k", "{}", read(&log));
    drop(g);
}

/// What `use` declarations take, each path read as it would be written alone, however the tree
/// is written: a glob or a name in nested groups, `self` and `as` inside a group, a glob of
/// std::env (its `var` called bare), of a module named `process` (std's, tokio's), through a name
/// another `use` gives (Unicode too), `use std::fs;` then a function the reading did not list,
/// `use std::*;` then a module it gives; `env!` and `UnixListener`. What takes nothing reads as
/// nothing: `use std::*;` alone, a glob of std::io, an empty group, `use ::*;`, `env!=` compared;
/// and `quillo check` says the line too (src/confirm.rs, FEEDBACK83.md Q-1305, found by the coverage tests; branch feedback82-85).
#[test]
fn what_a_use_tree_takes_is_read_however_it_is_written() {
    let d = dir("rust-use-trees");
    let read = "pub fn r() -> String { read_to_string(\"/etc/hosts\").unwrap_or_default() }";
    let cases: Vec<(&str, String, &str, &str)> = vec![
        ("nested", format!("use std::{{io, fs::{{self, *}}}};\n{}", read), "fs", "std::fs (line 3)"),
        ("selfas", "use std::{fs::{self as f}};\npub fn r() { f::hard_link(\"/a\", \"/b\").ok(); }".into(), "fs", "std::fs::hard_link (line 4)"),
        ("renamed", "use std::{env::{var as v}};\npub fn r() -> String { v(\"K\").unwrap_or_default() }".into(), "env", "std::env::var (line 4)"),
        ("envglob", "use std::{env::{*}};\npub fn r() -> String { var(\"K\").unwrap_or_default() }".into(), "env", "std::env::* (line 3)"),
        ("envalone", "use std::env::*;\npub fn r() -> String { var(\"K\").unwrap_or_default() }".into(), "env", "std::env::* (line 3)"),
        ("process", "use std::{process::*};\npub fn r() { Command::new(\"ls\"); }".into(), "run", "Command::new (line 4)"),
        ("tokio", "use tokio::process::*;\npub fn r() { Command::new(\"ls\"); }".into(), "run", "Command::new (line 4)"),
        ("alias", format!("use std::fs as ü;\nuse ü::*;\n{}", read), "fs", "std::fs (line 3)"),
        ("named", format!("extern crate std as s;\nuse s::fs::read_to_string;\n{}", read), "fs", "s::fs::read_to_string (line 4), std::fs::read_to_string (line 5)"),
        ("unlisted", "use std::fs;\npub fn r() { fs::hard_link(\"/a\", \"/b\").ok(); }".into(), "fs", "fs::hard_link (line 4)"),
        ("file", "use std::{fs::File};\npub fn r() { File::options().write(true).open(\"/x\").ok(); }".into(), "fs", "fs::File (line 3)"),
        ("root", "use std::*;\npub fn r() { fs::hard_link(\"/a\", \"/b\").ok(); }".into(), "fs", "fs::hard_link (line 4)"),
        ("rootglob", "use std::*;\nuse env::*;\npub fn r() -> String { var(\"K\").unwrap_or_default() }".into(), "env", "env::* (line 4)"),
        ("built", "pub fn r() -> String { env!(\"AWS_SECRET_ACCESS_KEY\").to_string() }".into(), "env", "env! (line 3)"),
        ("socket", "use std::os::unix::net::UnixListener;\npub fn r() { UnixListener::bind(\"/tmp/s\").ok(); }".into(), "net", "UnixListener::bind (line 4)"),
    ];
    for (name, code, effect, call) in &cases {
        let f = format!("{}.qwer", name);
        write(&d, &f, &format!("//! version: 1.0.0\n//! effects: none\n{}\n", code));
        let (code, out) = quillo(&d, &["confirm", "--effects", &f]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.starts_with(&format!("{}: effects: {}\n  but it says `effects: none`, and calls {}: a reading", f, effect, call)), "{}: {}", name, out);
    }
    for (name, code) in [("rootonly", "use std::*;\npub fn r() -> u8 { 1 }"), ("io", "use std::{io::*};\npub fn r() -> u8 { 1 }"), ("empty", "use std::{};\npub fn r() -> u8 { 1 }"), ("bare", "use ::*;\npub fn r() -> u8 { 1 }"), ("compared", "pub fn r() -> bool { let env = 1; env!=2 }")] {
        let f = format!("{}.qwer", name);
        write(&d, &f, &format!("//! version: 1.0.0\n//! effects: none\n{}\n", code));
        assert_eq!(quillo(&d, &["confirm", "--effects", &f]), (Some(0), format!("{}: effects: none\n", f)), "{}", name);
    }
    // `quillo check` of a program that takes one says it at the file's line.
    write(&d, "main.qwe", "import nested.qwer use r\nroute GET / = r()\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("nested.qwer:3: warning: nested.qwer says `effects: none`, and calls std::fs (line 3)"), "{}", out);
}

/// `depth` programs mounted one in another, the last answering "deepest", nothing taken wrong,
/// under a quillo.confirmed: the path of the last program's folder from `d`.
fn mounted_clean(d: &Path, depth: usize) -> String {
    let mut at = String::new();
    for i in 0..depth {
        write(d, &format!("{}main.qwe", at), &format!("mount /m = \"m/main.qwe\"\nroute GET / = \"level {}\"\n", i));
        at.push_str("m/");
    }
    write(d, &format!("{}main.qwe", at), "route GET / = \"deepest\"\n");
    write(d, "quillo.confirmed", "[\"main.qwe\"]\n");
    at
}

/// A held `--no-check` start reads as deep as a check does: 16 programs (15 mounted one in
/// another), the kit of the 16th using its main file's `reader` untaken is refused by that name;
/// 16 programs with nothing wrong start, the deepest served; 17 do not start, said with what the
/// check could not read and what to do, which `quillo check` says too; a chain of 64 `use`d files
/// is read through (its last file's route served), 65 is refused the same way
/// (src/check.rs, FEEDBACK83.md Q-1300, found by the coverage tests; branch feedback82-85).
#[test]
fn a_held_start_reads_as_deep_as_a_check_and_no_deeper() {
    let d = dir("untaken-16");
    let at = mounted_down(&d, 15);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0", "--no-check"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(&format!("the program is not started\n  ./{}kit.qwe:5: error: `reader` comes from main.qwe, which this file does not take", at)), "{}", out);
    // Nothing wrong: 16 start, 17 do not.
    let d = dir("clean-16");
    let at = mounted_clean(&d, 15);
    let (g, port, log) = serve(&d, &["--no-check"]);
    assert_eq!(send(port, "GET", &format!("/{}", at), &[], "").body, "deepest", "{}", read(&log));
    drop(g);
    let d = dir("clean-17");
    let at = mounted_clean(&d, 16);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0", "--no-check"]);
    assert_eq!(code, Some(1), "{}", out);
    let deep = format!("./{}main.qwe: error: programs mount or call each other more than 16 deep", at);
    assert!(out.contains(&format!("quillo: a quillo.confirmed holds the program, and its check cannot read all of it, so which names each of its files takes is not known, and one could reach what another file was confirmed for: the program is not started\n  {}\n  a check reads programs 16 deep, the main one among them: mount or call the deepest from a program nearer the main file (`quillo check` says the same)\n", deep)), "{}", out);
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(&format!("{}\n", deep)), "{}", out);
    // A chain of `use`, the last file's route the program's own.
    let chain = |name: &str, n: usize| {
        let d = dir(name);
        write(&d, "main.qwe", "use u1.qwe\nroute GET / = \"main\"\n");
        for i in 1..n {
            write(&d, &format!("u{}.qwe", i), &format!("use u{}.qwe\n", i + 1));
        }
        write(&d, &format!("u{}.qwe", n), "route GET /leaf = \"leaf\"\n");
        write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
        d
    };
    let d = chain("chain-64", 64);
    let (g, port, log) = serve(&d, &["--no-check"]);
    assert_eq!(send(port, "GET", "/leaf", &[], "").body, "leaf", "{}", read(&log));
    drop(g);
    let d = chain("chain-65", 65);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0", "--no-check"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("the program is not started\n  ./u64.qwe:1: error: this `use` is the end of a chain of files more than 64 deep") && out.contains("\n  a check follows a chain of `use` 64 files long: take some of its files from one file (`quillo check` says the same)\n"), "{}", out);
}

/// A held `--no-check` start whose files come to more than a check reads together (32 MiB): the
/// kit read last, which uses the main file's `reader` untaken to read `.env`, is not read by the
/// check, and the start is refused, said with what to do; the page never shows the `.env`
/// (src/check.rs, FEEDBACK83.md Q-1300, found by the coverage tests; branch feedback82-85).
#[test]
fn a_held_start_whose_files_pass_what_a_check_reads_does_not_start() {
    let d = dir("untaken-budget");
    let mib = 1usize << 20;
    write(&d, "main.qwe", "let reader = read_file\nuse a.qwe\nuse b.qwe\nuse kit.qwe\nroute GET / = \"main\"\n");
    write(&d, "a.qwe", &format!("fn fa() {{\n  return 1\n}}\n{}", "\n".repeat(16 * mib - 64)));
    write(&d, "b.qwe", &format!("fn fb() {{\n  return 2\n}}\n{}", "\n".repeat(16 * mib - 600)));
    write(&d, "kit.qwe", &format!("route GET /leak = str(reader(\".env\"))\n{}", "\n".repeat(600)));
    write(&d, ".env", "SECRET=hunter2\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nfiles = [\".env\"]\n");
    let log = d.join("run.log");
    let child = quillo_cmd(&d, Path::new(env!("CARGO_BIN_EXE_quillo")), &["run", "main.qwe", "--port", "0", "--no-check"]).stdout(Stdio::null()).stderr(std::fs::File::create(&log).unwrap()).spawn_held();
    let mut g = Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(120);
    while !read(&log).contains("listening on") && g.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if let Some(port) = support::port_said_in(&log) {
        assert!(!send(port, "GET", "/leak", &[], "").body.contains("hunter2"), "the kit read the .env");
    }
    drop(g);
    let said = read(&log);
    assert!(said.contains("the program is not started\n  main.qwe:4: error: cannot read kit.qwe: the program's files come to more than 32 MiB together\n  a check reads 32 MiB of a program's files together: make them smaller, or take fewer (`quillo check` says the same)\n"), "{}", said);
}

// ---- edge tests of the fixes the review of 109f6b1 asked for ------------------------------------

/// Under `--watch --no-check`, a reload reads again the names of a mounted program's and a
/// service's files: a kit of either changed to call its main file's `reader(".env")` untaken is
/// refused as the program loads again ("still serving the previous version"), and the page keeps
/// what it served; the `.env` is never in it. The record that let a start skip reading again
/// named only the main program's files (src/check.rs, FEEDBACK83.md Q-1300, found by the review
/// of 109f6b1; branch feedback82-85).
#[test]
fn a_reload_reads_again_what_a_mounted_program_or_a_service_takes() {
    for (name, main, file, program) in [
        ("mount", "mount /m = \"m/main.qwe\"\nroute GET / = \"top\"\n", "m", "import kit.qwe use go\nlet reader = read_file\nroute GET / = go()\n"),
        ("service", "service billing = \"svc/billing.qwe\"\nroute GET /m/ = billing.data()\n", "svc", "import kit.qwe use go\nlet reader = read_file\nserver fn data() {\n  return go()\n}\n"),
    ] {
        let d = dir(&format!("reload-{}", name));
        write(&d, "main.qwe", main);
        let program_file = if name == "mount" { "main.qwe" } else { "billing.qwe" };
        write(&d, &format!("{}/{}", file, program_file), program);
        write(&d, &format!("{}/kit.qwe", file), "fn go() {\n  return \"fine\"\n}\n");
        write(&d, &format!("{}/.env", file), "SECRET=hunter2\n");
        write(&d, "quillo.confirmed", &format!("[\"main.qwe\"]\n\n[\"{}/{}\"]\nfiles = [\".env\"]\n", file, program_file));
        let (g, port, log) = serve(&d, &["--watch", "--no-check"]);
        assert_eq!(send(port, "GET", "/m/", &[], "").body, "fine", "{}: {}", name, read(&log));
        write(&d, &format!("{}/kit.qwe", file), "fn go() {\n  return str(reader(\".env\"))\n}\n");
        assert!(log_has(&log, "quillo: still serving the previous version\n", 30), "{}: {}", name, read(&log));
        let said = read(&log);
        assert!(said.contains(&format!("the program is not started\n  ./{}/kit.qwe:2: error: `reader` comes from {}, which this file does not take", file, program_file)), "{}: {}", name, said);
        assert_eq!(said.matches(&format!("./{}/kit.qwe:2: error", file)).count(), 1, "said once: {}", said);
        assert_eq!(send(port, "GET", "/m/", &[], "").body, "fine", "{}", name);
        drop(g);
    }
}

/// What a name a `use` gives stands for is read wherever the name is used, after `::` too: `std::fs`
/// taken as `q` in a module and used as `a::q::`, taken as `s` and used through `self::`, `super::`
/// and `crate::`, a module's name taken again (`use self::m::store as q;`), `std::net::TcpStream` and
/// `std::env` taken in a module, a raw name (`r#fs`), each read as what it takes; a path a macro builds
/// from what it is given, a foreign block and `asm!` read as everything, said once. What takes
/// nothing still reads as nothing: `$crate::`, `$f::<T>`, a function made for C, a type of std::net,
/// a qualified path in brackets (src/confirm.rs, FEEDBACK83.md Q-1305, found by the review of 109f6b1; branch feedback82-85).
#[test]
fn what_a_name_a_use_gives_stands_for_is_read_wherever_it_is_used() {
    let d = dir("rust-names-used");
    let all = "http, fs, env, time, random, net, run";
    let cases: Vec<(&str, &str, &str, &str)> = vec![
        ("reexport", "mod a { pub use std::fs as q; }\npub fn r(p: String) -> String { a::q::read_to_string(p).unwrap_or_default() }", "fs", "std::fs (line 3)"),
        ("selfpath", "use std::fs as s;\npub fn r(p: String) -> String { self::s::read_to_string(p).unwrap_or_default() }", "fs", "std::fs (line 3)"),
        ("superpath", "use std::fs as s;\nmod inner { pub fn r(p: String) -> String { super::s::read_to_string(p).unwrap_or_default() } }", "fs", "std::fs (line 3)"),
        ("cratepath", "use std::fs as s;\npub fn r(p: String) -> String { crate::s::read_to_string(p).unwrap_or_default() }", "fs", "std::fs (line 3)"),
        ("again", "mod m { pub use std::fs as store; }\nuse self::m::store as q;\npub fn r(p: String) -> String { q::read_to_string(p).unwrap_or_default() }", "fs", "std::fs (line 3)"),
        ("tcp", "mod a { pub use std::net::TcpStream as T; }\npub fn r(c: String) -> i64 { a::T::connect(c).map(|_| 0).unwrap_or(1) }", "net", "std::net::TcpStream (line 3)"),
        ("env", "mod a { pub use std::env as e; }\npub fn r(c: String) -> String { a::e::var(c).unwrap_or_default() }", "env", "std::env (line 3)"),
        ("raw", "use std::r#fs as q;\npub fn r(p: String) -> String { q::read_to_string(p).unwrap_or_default() }", "fs", "std::fs::read_to_string (line 4)"),
        ("rawglob", "use std::{r#fs::*};\npub fn r(p: String) -> String { read_to_string(p).unwrap_or_default() }", "fs", "std::fs::* (line 3)"),
        ("macro", "macro_rules! m { ($x:ident) => { std::$x::read_to_string } }\npub fn r(p: String) -> String { m!(fs)(p).unwrap_or_default() }", all, "`::$x` (a path a macro builds from what it is given) (line 3)"),
        ("macrohead", "macro_rules! m { ($x:ident) => { $x::read_to_string } }\npub fn r(p: String) -> String { m!(fs)(p).unwrap_or_default() }", all, "`$x::` (a path a macro builds from what it is given) (line 3)"),
        ("foreign", "extern \"C\" { fn system(c: *const u8) -> i32; }\npub fn r() -> i64 { unsafe { system(std::ptr::null()) as i64 } }", all, "a foreign block (`extern \"C\" { ... }`), whose functions are not Rust's to read (line 3)"),
    ];
    for (name, code, effects, call) in &cases {
        let f = format!("{}.qwer", name);
        write(&d, &f, &format!("//! version: 1.0.0\n//! effects: none\n{}\n", code));
        let (code, out) = quillo(&d, &["confirm", "--effects", &f]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert_eq!(out, format!("{}: effects: {}\n  but it says `effects: none`, and calls {}: a reading of its text, not a proof (a crate can do what it does unseen)\n", f, effects, call), "{}", name);
    }
    // `asm!` reads as everything, and builds another file in too, by its `.include` and `.incbin`,
    // said as `include!` is (FEEDBACK83.md, Q-1305, found by the coverage tests of 3117dea; branch
    // feedback82-85).
    write(&d, "asm.qwer", "//! version: 1.0.0\n//! effects: none\npub fn r() { unsafe { core::arch::asm!(\"nop\") } }\n");
    let (code, out) = quillo(&d, &["confirm", "--effects", "asm.qwer"]);
    assert_eq!(code, Some(1), "{}", out);
    assert_eq!(out, format!("asm.qwer: effects: {}\n  but it says `effects: none`, and calls `asm!`, code that is not Rust's to read (line 3): a reading of its text, not a proof (a crate can do what it does unseen)\n  but it builds another file into it (`asm!`, assembly, whose `.include` and `.incbin` read another file in at line 3), which this reading does not see; under a quillo.confirmed it does not load\n", all));
    for (name, code) in [
        ("cratemacro", "macro_rules! m { () => { $crate::r() } }\npub fn r() -> u8 { 1 }"),
        ("turbofish", "macro_rules! m { ($f:ident) => { $f::<u8>() } }\npub fn r() -> u8 { 1 }"),
        ("forc", "pub extern \"C\" fn r() -> u8 { 1 }"),
        ("ipaddr", "use std::net::IpAddr;\npub fn r(s: String) -> bool { s.parse::<IpAddr>().is_ok() }"),
        ("qualified", "pub fn r(a: [u8; 4]) -> u8 { a[<usize as Default>::default()] }"),
        ("unicode", "mod ü { pub fn r() -> u8 { 1 } }\npub fn r() -> u8 { ü::r() }"),
    ] {
        let f = format!("{}.qwer", name);
        write(&d, &f, &format!("//! version: 1.0.0\n//! effects: none\n{}\n", code));
        assert_eq!(quillo(&d, &["confirm", "--effects", &f]), (Some(0), format!("{}: effects: none\n", f)), "{}", name);
    }
}

/// A `use` tree is read 64 groups deep: `use std::{{...{fs::*}...}};` 64 deep reads as fs, 65 deep
/// as everything, said at its line; 300,000 deep is read the same way, not brought down by its
/// depth, by `quillo confirm --effects`, `quillo check` and a held start (src/confirm.rs, FEEDBACK83.md Q-1305, found by the review of 109f6b1; branch feedback82-85).
#[test]
fn a_use_tree_deeper_than_it_is_read_reads_as_everything() {
    let d = dir("rust-use-deep");
    let nested = |n: usize| format!("//! version: 1.0.0\n//! effects: none\nuse std::{}fs::*{};\npub fn r() -> String {{ read_to_string(\"/etc/hosts\").unwrap_or_default() }}\n", "{".repeat(n), "}".repeat(n));
    write(&d, "d64.qwer", &nested(64));
    let (code, out) = quillo(&d, &["confirm", "--effects", "d64.qwer"]);
    assert_eq!((code, out.lines().next()), (Some(1), Some("d64.qwer: effects: fs")), "{}", out);
    write(&d, "d65.qwer", &nested(65));
    let (code, out) = quillo(&d, &["confirm", "--effects", "d65.qwer"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.starts_with("d65.qwer: effects: http, fs, env, time, random, net, run\n  but it says `effects: none`, and calls a `use` nested more than 64 deep, which is not read (line 3): "), "{}", out);
    write(&d, "deep.qwer", &format!("//! version: 1.0.0\n//! effects: none\nuse std::{};\npub fn r() -> i64 {{ 0 }}\n", "{".repeat(300_000)));
    let (code, out) = quillo(&d, &["confirm", "--effects", "deep.qwer"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.starts_with("deep.qwer: effects: http, fs, env, time, random, net, run\n"), "{}", out);
    // Under a hold: check says it is not confirmed, and the start stops on that, each standing.
    write(&d, "main.qwe", "import deep.qwer use r\nroute GET / = str(r())\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\n");
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("deep.qwer:3: warning: deep.qwer says `effects: none`, and calls a `use` nested more than 64 deep") && out.contains("deep.qwer: error: deep.qwer is not confirmed"), "{}", out);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0", "--no-check"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("deep.qwer is not confirmed"), "{}", out);
}

/// A hostile Rust file is read in time in proportion to its size: for each kind (a path of 250,000
/// `fs::`, 40,000 `use`s each giving a name, groups 300,000 deep, runs of spaces, `a :: ` joined,
/// `.open(` after a builder, strings, `$x::y`, distinct calls, `use` with no `;`), reading 1 MB once
/// takes less than five times as long as reading 64 KB sixteen times, the same work in
/// proportion (sixteen times as long as the square), each the best of three rounds taken in turn:
/// compared, not timed, so a slower or busier machine passes as a faster one does. And `quillo
/// confirm --effects` reads the 1 MB ones (src/confirm.rs, FEEDBACK83.md Q-1305, found by the review of 109f6b1; branch feedback82-85).
#[test]
fn a_hostile_rust_file_is_read_in_time_in_proportion_to_its_size() {
    let make = |kind: &str, size: usize| -> String {
        let mut body = String::with_capacity(size + 64);
        let mut i = 0usize;
        while body.len() < size {
            match kind {
                "fs" => body.push_str("fs::"),
                "uses" => body.push_str(&format!("use std::io::Read as r{};\n", i)),
                "deep" => body.push('{'),
                "spaces" => body.push(' '),
                "colons" => body.push_str("a :: "),
                "open" => body.push_str("Builder.open(x)"),
                "strings" => body.push_str("\"a\" "),
                "dollar" => body.push_str("$x::y "),
                "calls" => body.push_str(&format!("std::fs::a{}(p);\n", i)),
                _ => body.push_str("use "),
            }
            i += 1;
        }
        let body = if kind == "deep" { format!("use std::{};", body) } else { body };
        format!("//! version: 1.0.0\n//! effects: none\npub fn f() -> i64 {{ {} 0 }}\n", body)
    };
    let timed = |f: &dyn Fn()| {
        let t = Instant::now();
        f();
        t.elapsed()
    };
    for kind in ["fs", "uses", "deep", "spaces", "colons", "open", "strings", "dollar", "calls", "usekw"] {
        let (small, large) = (make(kind, 64 << 10), make(kind, 1 << 20));
        let (mut t_small, mut t_large) = (Duration::MAX, Duration::MAX);
        for _ in 0..3 {
            t_small = t_small.min(timed(&|| {
                for _ in 0..16 {
                    quillo::confirm::rust_effects(&small);
                }
            }));
            t_large = t_large.min(timed(&|| {
                quillo::confirm::rust_effects(&large);
            }));
        }
        let ratio = t_large.as_secs_f64() / t_small.as_secs_f64().max(1e-6);
        eprintln!("{}: 64 KB sixteen times in {:?}, 1 MB in {:?} ({:.2} times)", kind, t_small, t_large, ratio);
        assert!(ratio < 5.0, "{}: 64 KB sixteen times in {:?}, 1 MB in {:?} ({:.2} times)", kind, t_small, t_large, ratio);
    }
    let d = dir("rust-hostile");
    write(&d, "fs.qwer", &make("fs", 1 << 20));
    write(&d, "uses.qwer", &make("uses", 1 << 20));
    let (code, out) = quillo(&d, &["confirm", "--effects", "fs.qwer", "uses.qwer"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.starts_with("fs.qwer: effects: fs\n") && out.contains("\nuses.qwer: effects: none\n"), "{}", &out[..out.len().min(400)]);
}

/// Under a quillo.confirmed a Rust file that builds another file into it is refused, as its
/// SHA-256 does not cover that file: `#[path = "helper.rs"] mod helper;`, `mod helper;`, `mod ü;`,
/// `include!`, `include_str!`, `include_bytes!`. `quillo confirm` writes nothing and says why;
/// with its hash confirmed by hand, `quillo check` says it at its line, a start does not start,
/// `quillo build` builds nothing, and taken under `--watch` it does not load; with no hold,
/// `quillo confirm --effects` says it. An inline `mod helper { ... }` and the words in a comment
/// or a text are not refused (src/confirm.rs, FEEDBACK83.md Q-1305, found by the review of 109f6b1; branch feedback82-85).
#[test]
fn a_plugin_that_builds_another_file_into_it_is_refused_under_a_hold() {
    use sha2::Digest;
    let cases = [
        ("pathmod", "#[path = \"helper.rs\"]\nmod helper;", "`mod helper;`, a module in another file", 4),
        ("plainmod", "mod helper;", "`mod helper;`, a module in another file", 3),
        ("unicode", "pub mod ü ;", "`mod ü;`, a module in another file", 3),
        ("include", "include!(\"helper.rs\");", "`include!`", 3),
        ("text", "const T: &str = include_str! (\"helper.txt\");", "`include_str!`", 3),
        ("bytes", "static B: &[u8] = include_bytes!(\"helper.bin\");", "`include_bytes!`", 3),
    ];
    for (name, code, how, line) in cases {
        let d = dir(&format!("builds-in-{}", name));
        let src = format!("//! version: 1.0.0\n//! effects: fs\n{}\npub fn hi() -> String {{ \"hi\".into() }}\n", code);
        write(&d, "hi.qwer", &src);
        write(&d, "helper.rs", "pub fn hi() -> String { \"helper\".into() }\n");
        write(&d, "main.qwe", "route GET / = \"main\"\n");
        let why = format!("hi.qwer builds another file into it ({} at line {}), which its SHA-256 in quillo.confirmed would not cover: a change there would build another plugin under the one confirmed, so it is not confirmed and does not load under a quillo.confirmed (put that code in hi.qwer)", how, line);
        // No hold: said by the reading.
        let (code, out) = quillo(&d, &["confirm", "--effects", "hi.qwer"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.contains(&format!("  but it builds another file into it ({} at line {}), which this reading does not see; under a quillo.confirmed it does not load\n", how, line)), "{}: {}", name, out);
        // Taken under --watch, its hash confirmed by hand: not loaded, nothing built.
        let sha: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|x| format!("{:02x}", x)).collect();
        write(&d, "quillo.confirmed", &format!("[\"main.qwe\"]\n\n[\"hi.qwer\"]\nsha256 = \"{}\"\n", sha));
        let (g, port, log) = serve(&d, &["--watch", "--no-check", "--no-build"]);
        assert_eq!(send(port, "GET", "/", &[], "").body, "main", "{}: {}", name, read(&log));
        write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
        assert!(log_has(&log, &why, 30), "{}: {}", name, read(&log));
        drop(g);
        let (code, out) = quillo(&d, &["check", "main.qwe"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.contains(&format!("hi.qwer:{}: error: {}", line, why)), "{}: {}", name, out);
        let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0", "--no-check"]);
        assert_eq!((code, out.trim()), (Some(1), format!("quillo: {}", why).as_str()), "{}", name);
        let (code, out) = quillo(&d, &["build", "main.qwe"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.contains(&format!("{}: nothing built", why)), "{}: {}", name, out);
        // `quillo confirm` writes nothing.
        std::fs::remove_file(d.join("quillo.confirmed")).unwrap();
        let (code, out) = quillo(&d, &["confirm", "--yes"]);
        assert_eq!((code, out.as_str()), (Some(1), format!("quillo confirm: {}\nquillo confirm: nothing written\n", why).as_str()), "{}", name);
        assert!(!d.join("quillo.confirmed").exists(), "{}", name);
    }
    // Inline, or only words: confirmed as before.
    let d = dir("builds-in-not");
    write(&d, "hi.qwer", "//! version: 1.0.0\n//! effects: none\n// mod helper; include!(\"x\")\nmod helper { pub fn hi() -> String { \"mod h; include_str!()\".into() } }\npub fn hi() -> String { helper::hi() }\n");
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(read(&d.join("quillo.confirmed")).contains("[\"hi.qwer\"]\nsha256 = "), "{}", read(&d.join("quillo.confirmed")));
}

// ---- edge tests of the fixes the review of a867750 asked for ------------------------------------

/// A plugin that builds another file into it is refused under a quillo.confirmed however it is
/// written, as the reading reads Rust's tokens: `mod helper;` with a comment or any of Rust's
/// whitespace between its words, `mod $n;` in a macro, `mod` handed to a macro, `include!`,
/// `include_str!` and `include_bytes!` with any delimiter, space or comment around their `!`, by a
/// raw name or a name a `use` gives, `$m!`, paste's `[<...>]!`, and a dependency from a folder,
/// from git, with another key, of a crate whose macros build files in, or one not read as a
/// name and a version. `quillo confirm` writes nothing; with the hash written by hand, check, a
/// start and `quillo build` refuse it, and so does a `--watch` load of the name a `use` gives
/// (src/confirm.rs, FEEDBACK83.md Q-1305, found by the review of a867750; branch feedback82-85).
#[test]
fn a_plugin_that_builds_another_file_in_is_refused_however_it_is_written() {
    use sha2::Digest;
    let module = "`mod helper;`, a module in another file";
    let mut cases: Vec<(String, String, String)> = vec![
        ("comment".into(), "mod/**/helper;".into(), module.into()),
        ("nested".into(), "mod /* a /* b */ c */ helper;".into(), module.into()),
        ("macro".into(), "macro_rules! m { ($n:ident) => { mod $n; } }\nm!(helper);".into(), "`mod $n;`, a module in another file".into()),
        ("handed".into(), "macro_rules! m { ($a:tt $b:ident) => { $a $b; } }\nm!(mod helper);".into(), "`mod helper` handed to a macro, which can make a module in another file of it".into()),
        ("handedalone".into(), "macro_rules! m { ($a:tt) => { $a helper; } }\nm!(mod);".into(), "`mod` handed to a macro, which can make a module in another file of it".into()),
        ("brace".into(), "include! {\"helper.rs\"}".into(), "`include!`".into()),
        ("bracket".into(), "const T: &str = include_str! [\"/etc/hosts\"];".into(), "`include_str!`".into()),
        ("formfeed".into(), "const T: &str = include_str!\u{0C}(\"/etc/hosts\");".into(), "`include_str!`".into()),
        ("spaced".into(), "static B: &[u8] = include_bytes /* x */ ! (\"helper.rs\");".into(), "`include_bytes!`".into()),
        ("raw".into(), "r#include!(\"helper.rs\");".into(), "`include!`".into()),
        ("alias".into(), "use std::include_str as grab;\nconst T: &str = grab!(\"/etc/hosts\");".into(), "`grab!`, an include macro by another name".into()),
        ("aliasgroup".into(), "use core::{include_bytes as g};\nstatic B: &[u8] = g!(\"helper.rs\");".into(), "`g!`, an include macro by another name".into()),
        ("dollar".into(), "macro_rules! inc { ($m:ident) => { $m!(\"/etc/hosts\") } }\nconst T: &str = inc!(include_str);".into(), "`$m!`, a macro named by what a macro is given, which can be `include!`".into()),
        ("paste".into(), "paste::paste! { const T: &str = [<include _str>]!(\"/etc/hosts\"); }".into(), "`[<...>]!`, a macro whose name paste builds, which can be `include!`".into()),
    ];
    for (name, ws) in [("vt", '\u{0B}'), ("ff", '\u{0C}'), ("tab", '\t'), ("nel", '\u{85}'), ("lrm", '\u{200E}'), ("rlm", '\u{200F}'), ("ls", '\u{2028}'), ("ps", '\u{2029}')] {
        cases.push((format!("ws-{}", name), format!("mod{}helper;", ws), module.into()));
    }
    let deps = [
        ("path", "helper = { path = \"helper\" }", "the dependency `helper` from a folder (`path`)"),
        ("git", "helper = { git = \"https://example.invalid/helper.git\", branch = \"main\" }", "the dependency `helper` from git (`git`)"),
        ("registry", "helper = { version = \"1\", registry = \"elsewhere\" }", "the dependency `helper` with `registry`, which can take its code from elsewhere"),
        ("embed", "rust-embed = \"8\"", "the dependency `rust-embed`, rust-embed, whose macros build files in"),
        ("renamed", "e = { package = \"rust-embed\", version = \"8\" }", "the dependency `e`, rust-embed, whose macros build files in"),
        ("dotted", "helper.path = \"helper\"", "the dependency line `helper.path = \"helper\"`, not read as a crate's name and version"),
        ("quoted", "\"helper\" = \"1\"", "the dependency line `\"helper\" = \"1\"`, not read as a crate's name and version"),
    ];
    for (name, dep, how) in deps {
        cases.push((format!("dep-{}", name), format!("//! [dependencies]\n//! {}", dep), how.into()));
    }
    for (name, code, how) in &cases {
        let d = dir(&format!("builds-in3-{}", name));
        let (header, code) = match code.strip_prefix("//! [dependencies]\n") {
            Some(dep) => (format!("//! version: 1.0.0\n//! effects: fs\n//! [dependencies]\n{}\n", dep), String::new()),
            None => ("//! version: 1.0.0\n//! effects: fs\n".to_string(), code.clone()),
        };
        let src = format!("{}{}\npub fn hi() -> String {{ \"hi\".into() }}\n", header, code);
        let line = if name.starts_with("dep-") { 4 } else if ["alias", "aliasgroup"].contains(&name.as_str()) { 4 } else { 3 };
        let line = match name.as_str() {
            "macro" => 3,
            "handed" | "handedalone" => 4,
            _ => line,
        };
        write(&d, "hi.qwer", &src);
        write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
        let why = format!("hi.qwer builds another file into it ({} at line {}), which its SHA-256 in quillo.confirmed would not cover", how, line);
        let (code, out) = quillo(&d, &["confirm", "--yes"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.starts_with(&format!("quillo confirm: {}", why)) && out.ends_with("quillo confirm: nothing written\n"), "{}: {}", name, out);
        assert!(!d.join("quillo.confirmed").exists(), "{}", name);
        // Its hash written by hand: check, a start and `quillo build` refuse it.
        let sha: String = sha2::Sha256::digest(src.as_bytes()).iter().map(|x| format!("{:02x}", x)).collect();
        write(&d, "quillo.confirmed", &format!("[\"main.qwe\"]\n\n[\"hi.qwer\"]\nsha256 = \"{}\"\n", sha));
        let (code, out) = quillo(&d, &["check", "main.qwe"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.contains(&format!("hi.qwer:{}: error: {}", line, why)), "{}: {}", name, out);
        let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0", "--no-check"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.contains(&format!("quillo: {}", why)), "{}: {}", name, out);
        let (code, out) = quillo(&d, &["build", "main.qwe"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.contains(&why) && out.contains(": nothing built"), "{}: {}", name, out);
        // And a load under --watch, of the name a `use` gives (where the reading was another's).
        if name == "alias" {
            write(&d, "main.qwe", "route GET / = \"main\"\n");
            let (g, port, log) = serve(&d, &["--watch", "--no-check", "--no-build"]);
            assert_eq!(send(port, "GET", "/", &[], "").body, "main", "{}", read(&log));
            write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
            assert!(log_has(&log, &why, 30), "{}", read(&log));
            drop(g);
        }
    }
}

/// What builds nothing in stays confirmed: an inline `mod helper { }` (by a macro's name too),
/// `mod helper;` in a comment and in each kind of string, a character and a lifetime beside such
/// a text, a variable named `include` compared with `!=`, a macro with no `$m!`, and the
/// dependencies the catalog's plugins have (src/confirm.rs, FEEDBACK83.md Q-1305, found by the review of a867750; branch feedback82-85).
#[test]
fn what_builds_nothing_in_is_confirmed_as_before() {
    let d = dir("builds-in3-not");
    let code = [
        "mod helper { pub fn hi() -> String { \"hi\".into() } }",
        "macro_rules! m { ($n:ident) => { mod $n { } } }\nm!(inner);",
        "// mod helper; include!(\"x\")\n/* mod helper; /* include_str!(\"y\") */ */",
        "const A: &str = \"mod helper; include!(\\\"x\\\")\";",
        "const B: &[u8] = b\"mod helper;\";",
        "const C: &core::ffi::CStr = c\"mod helper;\";",
        "const D: &str = r##\"mod helper; \"# include!(\"##;",
        "const E: &[u8] = br##\"mod helper;\"#\"##;",
        "const F: &core::ffi::CStr = cr#\"mod helper; \\\"#;",
        "const G: char = '\"';\nconst H: u8 = b'\\'';\nfn l<'a>(x: &'a str) -> &'a str { x }",
        "fn n() -> bool { let include = 1; include != 2 }",
        "macro_rules! twice { ($x:expr) => { $x + $x } }\nfn t() -> i64 { twice!(1) }",
    ]
    .join("\n");
    let src = format!("//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! serde_json = {{ version = \"1\", features = [\"preserve_order\"] }}\n//! ureq = \"2\"\n//! gix = {{ version = \"0.66\", default-features = false, features = [\"revision\"] }}\n{}\npub fn hi() -> String {{ helper::hi() }}\n", code);
    write(&d, "hi.qwer", &src);
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(read(&d.join("quillo.confirmed")).contains("[\"hi.qwer\"]\nsha256 = "), "{}", read(&d.join("quillo.confirmed")));
}

/// What a Rust file calls is read past every kind of string and character, whichever spaces are
/// between a path's parts: a `std::fs::read_to_string` after `"\""`, `b"\""`, `c"\""`, `r"\"`,
/// `r#"\"#`, `br#"\"#`, `cr#"\"#`, `r##"a"#b"##`, `'\''`, `b'\''`, `'"'` and a lifetime, and
/// `std::fs::read_to_string` written with each of Rust's whitespace and a comment around its `::`,
/// reads as fs. A qualified path (`<std::net::TcpStream>::connect`, `<std::net::TcpStream as
/// Read>::`), a type's other name (`type T = std::net::TcpStream;`, `UdpSocket`) read as net;
/// `$m!` as everything; `UNIX_EPOCH.elapsed()` as time. What takes nothing reads as nothing:
/// `std::io::Result<()>` named, `a < b`, `Vec<u8>::new`, `UNIX_EPOCH` alone, the time of a file
/// (src/confirm.rs, FEEDBACK83.md Q-1305, found by the review of a867750; branch feedback82-85).
#[test]
fn what_a_rust_file_calls_is_read_past_every_kind_of_string() {
    let d = dir("rust-tokens");
    let read = "pub fn r(p: String) -> String { std::fs::read_to_string(p).unwrap_or_default() }";
    let mut cases: Vec<(String, String, &str, String)> = Vec::new();
    for (i, lit) in ["\"\\\"\"", "b\"\\\"\"", "c\"\\\"\"", "r\"\\\"", "r#\"\\\"#", "br#\"\\\"#", "cr#\"\\\"#", "r##\"a\"#b\"##", "'\\''", "b'\\''", "'\"'"].iter().enumerate() {
        cases.push((format!("lit{}", i), format!("pub const X{}: () = {{ let _ = {}; }};\n{}", i, lit, read), "fs", "std::fs::read_to_string (line 4)".into()));
    }
    cases.push(("lifetime".into(), format!("fn l<'a>(x: &'a str) -> &'a str {{ x }}\n{}", read), "fs", "std::fs::read_to_string (line 4)".into()));
    for (name, ws) in [("vt", '\u{0B}'), ("ff", '\u{0C}'), ("nel", '\u{85}'), ("lrm", '\u{200E}'), ("rlm", '\u{200F}'), ("ls", '\u{2028}'), ("ps", '\u{2029}'), ("tab", '\t')] {
        cases.push((format!("ws-{}", name), format!("pub fn r(p: String) -> String {{ std{w}::{w}fs /* x */ ::{w}read_to_string{w}(p).unwrap_or_default() }}", w = ws), "fs", "std::fs::read_to_string (line 3)".into()));
    }
    cases.push(("qualified".into(), "pub fn s() { let _ = <std::net::TcpStream>::connect(\"203.0.113.9:80\"); }".into(), "net", "std::net::TcpStream::connect (line 3)".into()));
    cases.push(("qualifiedas".into(), "pub fn s(b: &mut [u8]) { let _ = <std::net::TcpStream as std::io::Read>::read; let _ = b; }".into(), "net", "std::net::TcpStream::read (line 3)".into()));
    cases.push(("typenet".into(), "type T = std::net::TcpStream;\npub fn s() { let _ = T::connect(\"203.0.113.9:80\"); }".into(), "net", "std::net::TcpStream::connect (line 4)".into()));
    cases.push(("typeudp".into(), "pub type U<'a> = std::net::UdpSocket;\npub fn s() { let _ = U::bind(\"0.0.0.0:0\"); }".into(), "net", "std::net::UdpSocket::bind (line 4)".into()));
    cases.push(("dollar".into(), "macro_rules! go { ($m:ident) => { $m!(\"HOME\") } }\npub fn e() -> &'static str { go!(env) }".into(), "http, fs, env, time, random, net, run", "`$m!` (a macro named by what a macro is given) (line 3)".into()));
    cases.push(("epoch".into(), "pub fn t() -> u64 { std::time::UNIX_EPOCH.elapsed().unwrap().as_secs() }".into(), "time", "std::time::UNIX_EPOCH.elapsed() (line 3)".into()));
    for (name, code, effects, call) in &cases {
        let f = format!("{}.qwer", name);
        write(&d, &f, &format!("//! version: 1.0.0\n//! effects: none\n{}\n", code));
        let (code, out) = quillo(&d, &["confirm", "--effects", &f]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.starts_with(&format!("{}: effects: {}\n  but it says `effects: none`, and calls {}: a reading of its text", f, effects, call)), "{}: {}", name, out);
    }
    for (name, code) in [
        ("result", "type R = std::io::Result<()>;\npub fn r() -> R { Ok(()) }"),
        ("less", "pub fn r(a: i64, b: i64) -> bool { a < b && b > a }"),
        ("generic", "pub fn r() -> usize { let v: Vec<u8> = Vec::<u8>::new(); v.len() }"),
        ("epochalone", "pub fn r(t: std::time::SystemTime) -> u64 { t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }"),
    ] {
        let f = format!("{}.qwer", name);
        write(&d, &f, &format!("//! version: 1.0.0\n//! effects: none\n{}\n", code));
        assert_eq!(quillo(&d, &["confirm", "--effects", &f]), (Some(0), format!("{}: effects: none\n", f)), "{}", name);
    }
}

// ---- edge tests of the fixes the review of 152bc1b asked for ------------------------------------

/// A plugin held to its SHA-256 is judged by what the compiler read as it built it: one whose
/// tokens build nothing in, but whose dependency's macro does (`doc_comment::doctest!`, which
/// expands to `include_str!`), is confirmed by `quillo confirm`, and then does not load, as a start,
/// as `quillo build` and as a `--watch` load builds it: said with the file it read; its library
/// is not kept nor recorded. A plugin that reads only itself builds and loads under the same hold
/// (src/native.rs, FEEDBACK83.md Q-1305, found by the review of 152bc1b; branch feedback82-85).
#[test]
fn a_pinned_build_is_judged_by_what_the_compiler_read() {
    // Only itself: built and loaded, and recorded.
    let d = dir("read-by-rustc-plain");
    write(&d, "hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi(name: String) -> String { serde_json::json!({ \"hi\": name }).to_string() }\n");
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi(\"Ana\")\n");
    assert_eq!(quillo(&d, &["confirm", "--yes"]).0, Some(0));
    let (g, port, log) = serve(&d, &[]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "{\"hi\":\"Ana\"}", "{}", read(&log));
    drop(g);
    assert_eq!(read(&record_dir(&d).join("built.txt")).lines().count(), 1);
    // A dependency's macro reads another file in: no word of it in the plugin.
    let d = dir("read-by-rustc-doc");
    let secret = d.join("secret.txt");
    write(&d, "secret.txt", "not the plugin's\n");
    let src = format!("//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! doc-comment = \"0.3\"\ndoc_comment::doctest!({:?});\npub fn hi() -> String {{ \"hi\".into() }}\n", secret.display().to_string());
    write(&d, "hi.qwer", &src);
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "the tokens build nothing in: {}", out);
    // Named as rustc read it, as the plugin wrote it (no link resolved).
    let real = secret.clone();
    let why = format!("hi.qwer is held to its SHA-256, and building it read {}, which that hash does not cover: it does not load, and what was built is not kept", real.display());
    let (code, out) = quillo(&d, &["run", "main.qwe", "--port", "0", "--no-check"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(&why), "{}", out);
    let (code, out) = quillo(&d, &["build", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(&why), "{}", out);
    let kept: Vec<String> = std::fs::read_dir(d.join(".quillo/native")).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    assert!(kept.iter().all(|f| !f.starts_with(&format!("{}hi", std::env::consts::DLL_PREFIX))), "{:?}", kept);
    assert!(!read(&record_dir(&d).join("built.txt")).contains(&{
        use sha2::Digest;
        sha2::Sha256::digest(src.as_bytes()).iter().map(|x| format!("{:02x}", x)).collect::<String>()
    }));
    // Taken under --watch.
    write(&d, "main.qwe", "route GET / = \"main\"\n");
    let (g, port, log) = serve(&d, &["--watch", "--no-check"]);
    assert_eq!(send(port, "GET", "/", &[], "").body, "main", "{}", read(&log));
    write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
    assert!(log_has(&log, &why, 120), "{}", read(&log));
    assert_eq!(send(port, "GET", "/", &[], "").body, "main");
    drop(g);
}

/// The reading starts where rustc's does and reads each literal as rustc does: after a byte order
/// mark, after a first line `#!...` that is no inner attribute (`#!/bin/sh "`), but not after
/// `#![allow(unused)]`, `#! [..]` or `#!/* x */[..]`; an escape read up to its closing quote
/// (`'\u{4______________1}'`). Each `mod helper;` after them is refused. A byte order mark
/// elsewhere, a character and a string that do not end are refused too, said at their line;
/// a file that starts with a mark or an attribute and builds nothing in is confirmed
/// (src/confirm.rs, FEEDBACK83.md Q-1305, found by the review of 152bc1b; branch feedback82-85).
#[test]
fn the_reading_starts_and_reads_literals_as_rustc_does() {
    let module = "`mod helper;`, a module in another file";
    let refused: Vec<(&str, String, u32, &str)> = vec![
        ("shebang", "#!/bin/sh \"\n//! version: 1.0.0\n//! effects: none\nmod helper;\npub const Q: char = '\"';".into(), 4, module),
        ("shebangdoc", "#! //! not a comment rustc passes over\nmod helper;".into(), 2, module),
        ("bom", "\u{FEFF}mod helper;".into(), 1, module),
        ("underscores", "//! version: 1.0.0\n//! effects: none\npub const P: (char, char) = ('\\u{4______________1}','\"');\nmod helper;\npub const Q: &str = \"\";".into(), 4, module),
        ("bomlater", "//! effects: none\npub fn a() {}\n\u{FEFF}pub fn b() {}".into(), 3, "a byte order mark past its first character, which rustc does not read as this reading does"),
        ("charopen", "//! effects: none\npub const C: char = '\\u{41\n;".into(), 2, "a character that does not end as rustc reads one"),
        // The `"` of `"hi"` below closes it, as rustc reads it, and the one after opens another.
        ("stringopen", "//! effects: none\npub const S: &str = \"never closed;".into(), 3, "a string that does not end as rustc reads one"),
        ("rawopen", "//! effects: none\npub const S: &str = r##\"closed by one hash\"#;".into(), 2, "a string that does not end as rustc reads one"),
    ];
    for (name, code, line, how) in &refused {
        let d = dir(&format!("lex-{}", name));
        write(&d, "hi.qwer", &format!("{}\npub fn hi() -> String {{ \"hi\".into() }}\n", code));
        write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
        let (code, out) = quillo(&d, &["confirm", "--yes"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.starts_with(&format!("quillo confirm: hi.qwer builds another file into it ({} at line {}), which its SHA-256 in quillo.confirmed would not cover", how, line)), "{}: {}", name, out);
    }
    let d = dir("lex-allowed");
    let allowed = [
        ("attribute", "#![allow(unused)]\nmod helper { }"),
        ("attrspace", "#! [allow(unused)]\nmod helper { }"),
        ("attrcomment", "#!/* x */[allow(unused)]\nmod helper { }"),
        ("bomclean", "\u{FEFF}//! effects: none\npub const C: char = '\\u{1_F_6_0_0}';"),
        ("escapes", "pub const C: (char, char, u8) = ('\\'', '\\\\', b'\\'');\npub const S: &str = \"mod helper; \\\" still\";"),
    ];
    let main: String = allowed.iter().map(|(n, _)| format!("import {}.qwer use hi as {}\n", n, n)).collect::<String>() + "route GET / = \"ok\"\n";
    for (name, code) in allowed {
        write(&d, &format!("{}.qwer", name), &format!("{}\npub fn hi() -> String {{ \"hi\".into() }}\n", code));
    }
    write(&d, "main.qwe", &main);
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
}

/// A dependency line's texts are read as TOML reads them: a literal one (`'rust-embed'`) as it is
/// written, so `package = 'rust-embed'` is refused under a hold and `package = 'reqwest'` read as
/// reqwest (http), a feature in one too (`features = ['v4']`, random); a basic text with an
/// escape and one over lines are refused, as what is not read; `version = '1'` is confirmed
/// (src/confirm.rs, FEEDBACK83.md Q-1305, found by the review of 152bc1b; branch feedback82-85).
#[test]
fn a_dependency_lines_texts_are_read_as_toml_reads_them() {
    let refused = [
        ("embed", "e = { package = 'rust-embed', version = '8' }", "the dependency `e`, rust-embed, whose macros build files in"),
        ("escape", "x = \"1\\u0030\"", "the dependency line `x = \"1\\u0030\"`, not read as a crate's name and version"),
        ("escapekey", "x = { version = \"1\", package = \"r\\u0065qwest\" }", "the dependency line `x = { version = \"1\", package = \"r\\u0065qwest\" }`, not read as a crate's name and version"),
        ("multiline", "x = \"\"\"1\"\"\"", "the dependency line `x = \"\"\"1\"\"\"`, not read as a crate's name and version"),
        ("multiliteral", "x = '''1'''", "the dependency line `x = '''1'''`, not read as a crate's name and version"),
    ];
    for (name, dep, how) in refused {
        let d = dir(&format!("toml-{}", name));
        write(&d, "hi.qwer", &format!("//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! {}\npub fn hi() -> String {{ \"hi\".into() }}\n", dep));
        write(&d, "main.qwe", "import hi.qwer use hi\nroute GET / = hi()\n");
        let (code, out) = quillo(&d, &["confirm", "--yes"]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.starts_with(&format!("quillo confirm: hi.qwer builds another file into it ({} at line 4)", how)), "{}: {}", name, out);
    }
    let d = dir("toml-read");
    for (name, dep, want) in [
        ("web", "web = { package = 'reqwest', version = '0.12' }", "web.qwer: effects: http\n  but it says `effects: none`, and calls the crate reqwest: "),
        ("ids", "uuid = { version = '1', features = ['v4'] }", "ids.qwer: effects: random\n  but it says `effects: none`, and calls the crate uuid (feature:v4): "),
    ] {
        let f = format!("{}.qwer", name);
        write(&d, &f, &format!("//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! {}\npub fn hi() -> String {{ \"hi\".into() }}\n", dep));
        let (code, out) = quillo(&d, &["confirm", "--effects", &f]);
        assert_eq!(code, Some(1), "{}: {}", name, out);
        assert!(out.starts_with(want), "{}: {}", name, out);
    }
    write(&d, "lit.qwer", "//! version: 1.0.0\n//! effects: none\n//! [dependencies]\n//! itoa = '1'\npub fn hi() -> String { \"hi\".into() }\n");
    write(&d, "main.qwe", "import lit.qwer use hi\nroute GET / = hi()\n");
    let (code, out) = quillo(&d, &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(read(&d.join("quillo.confirmed")).contains("crates = [\"itoa 1\"]"), "{}", read(&d.join("quillo.confirmed")));
}
