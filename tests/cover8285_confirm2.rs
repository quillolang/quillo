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
    let mut child = quillo_cmd(dir, Path::new(env!("CARGO_BIN_EXE_quillo")), args).stdout(file.try_clone().unwrap()).stderr(file).spawn().unwrap();
    let code = ended(&mut child, 60);
    let out = read(&log);
    let _ = std::fs::remove_file(&log);
    (code, out)
}

/// `quillo run main.qwe --port 0` in `dir` with `opts`: the guard, the port, the log.
fn serve(dir: &Path, opts: &[&str]) -> (Guard, u16, PathBuf) {
    let log = dir.join("run.log");
    let file = std::fs::File::create(&log).unwrap();
    let child = quillo_cmd(dir, Path::new(env!("CARGO_BIN_EXE_quillo")), &["run", "main.qwe", "--port", "0"]).args(opts).stdout(Stdio::null()).stderr(file).spawn().unwrap();
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
#[ignore = "BUG: a file with a block that the start did not read is unheld once --watch takes it"]
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
    let mut child = quillo_cmd(&d, Path::new("/bin/sh"), &["-c", &sh]).stdout(file.try_clone().unwrap()).stderr(file).spawn().unwrap();
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
#[ignore = "BUG: past 16 mounted programs a held start does not read untaken names"]
fn a_name_not_taken_stops_a_held_start_however_deep_its_program_is() {
    let d = dir("untaken-deep");
    let at = mounted_down(&d, 17);
    let log = d.join("run.log");
    let child = quillo_cmd(&d, Path::new(env!("CARGO_BIN_EXE_quillo")), &["run", "main.qwe", "--port", "0", "--no-check"]).stdout(Stdio::null()).stderr(std::fs::File::create(&log).unwrap()).spawn().unwrap();
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
    // SAFETY: openpty fills in two descriptors; no names or settings are given.
    assert_eq!(unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) }, 0);
    use std::os::fd::FromRawFd;
    // SAFETY: each descriptor is this test's, and owned once.
    let (m, s) = unsafe { (std::fs::File::from_raw_fd(master), std::fs::File::from_raw_fd(slave)) };
    drop(m);
    let log = d.join("fix.log");
    let mut child = quillo_cmd(&d, Path::new(env!("CARGO_BIN_EXE_quillo")), &["fix", "main.qwe"]).stdout(s).stderr(std::fs::File::create(&log).unwrap()).spawn().unwrap();
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
#[ignore = "BUG: a glob inside a use group hides std::fs from the Rust reading"]
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
