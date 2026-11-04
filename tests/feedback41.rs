//! FEEDBACK41.md: what a program on a shared server may reach. Q-475: a link inside the
//! program's folder does not take the file builtins out of it.

mod support;

use std::path::{Path, PathBuf};
use support::{send, serve, temp_dir};

/// A program folder `p` with `data` linking to a folder outside it and `alias` to one inside.
fn linked(name: &str, main: &str) -> (PathBuf, PathBuf) {
    let d = temp_dir(name);
    let outside = d.join("outside");
    let p = d.join("p");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(p.join("real")).unwrap();
    std::fs::write(outside.join("s.txt"), "secret-outside").unwrap();
    std::fs::write(p.join("real/in.txt"), "inner").unwrap();
    std::os::unix::fs::symlink(&outside, p.join("data")).unwrap();
    std::os::unix::fs::symlink("real", p.join("alias")).unwrap();
    std::fs::write(p.join("main.qwe"), main).unwrap();
    (p, outside)
}

fn multipart(field: &str, file: &str, bytes: &str) -> (String, String) {
    let b = "XqbOUNDARYq";
    let body = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{file}\"\r\nContent-Type: text/plain\r\n\r\n{bytes}\r\n--{b}--\r\n"
    );
    (body, format!("multipart/form-data; boundary={}", b))
}

const FILES: &str = r#"route GET /read ? { p: str } = read_file(p) ?? "refused"
route GET /write ? { p: str } = is_err(write_file(p, "written")) ? "refused" : "written"
route GET /delete ? { p: str } = is_err(delete_file(p)) ? "refused" : "deleted"
route GET /file ? { p: str } = file(p)
route GET /static/*path = files("data", path)
route POST /up ? { to: str } = save_upload(request.files.doc, to) ?? "refused"
"#;

#[test]
fn a_link_out_of_the_folder_takes_no_file_builtin_out() {
    let (p, outside) = linked("f41-links", FILES);
    let s = serve(&p.join("main.qwe"), &[]);
    assert_eq!(send(s.port, "GET", "/read?p=data/s.txt", &[], "").body, "refused");
    assert_eq!(send(s.port, "GET", "/read?p=data/../data/s.txt", &[], "").body, "refused");
    assert_eq!(send(s.port, "GET", "/write?p=data/w.txt", &[], "").body, "refused");
    assert_eq!(send(s.port, "GET", "/write?p=data/new/deeper/w.txt", &[], "").body, "refused", "a folder not made yet, under the link");
    assert_eq!(send(s.port, "GET", "/delete?p=data/s.txt", &[], "").body, "refused");
    assert_eq!(send(s.port, "GET", "/delete?p=data", &[], "").body, "refused", "judged where it leads");
    assert_eq!(send(s.port, "GET", "/file?p=data/s.txt", &[], "").status, 404);
    assert_eq!(send(s.port, "GET", "/static/s.txt", &[], "").status, 404);
    let (body, ctype) = multipart("doc", "up.txt", "the visitor's file");
    assert_eq!(send(s.port, "POST", "/up?to=data/up.txt", &[("Content-Type", &ctype)], &body).body, "refused");
    // Nothing reached the folder outside, and its file is still there.
    let mut there: Vec<String> = std::fs::read_dir(&outside).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    there.sort();
    assert_eq!(there, ["s.txt"]);
    assert_eq!(std::fs::read_to_string(outside.join("s.txt")).unwrap(), "secret-outside");
}

#[test]
fn a_link_that_stays_inside_works_as_before() {
    let (p, _) = linked("f41-inner", FILES);
    let s = serve(&p.join("main.qwe"), &[]);
    assert_eq!(send(s.port, "GET", "/read?p=alias/in.txt", &[], "").body, "inner");
    assert_eq!(send(s.port, "GET", "/write?p=alias/w.txt", &[], "").body, "written");
    assert_eq!(std::fs::read_to_string(p.join("real/w.txt")).unwrap(), "written");
    assert_eq!(send(s.port, "GET", "/file?p=alias/in.txt", &[], "").body, "inner");
    let (body, ctype) = multipart("doc", "up.txt", "kept");
    assert_eq!(send(s.port, "POST", "/up?to=alias/up.txt", &[("Content-Type", &ctype)], &body).body, "alias/up.txt");
    assert_eq!(std::fs::read_to_string(p.join("real/up.txt")).unwrap(), "kept");
    assert_eq!(send(s.port, "GET", "/delete?p=alias/w.txt", &[], "").body, "deleted");
    assert!(!p.join("real/w.txt").exists());
}

#[test]
fn a_program_reached_through_a_link_is_inside_itself() {
    // As a deploy does: `current` links to the release, and the program runs from the link.
    let d = temp_dir("f41-current");
    let release = d.join("releases/1");
    std::fs::create_dir_all(release.join("pages")).unwrap();
    std::fs::write(release.join("pages/a.txt"), "page a").unwrap();
    std::fs::write(release.join("main.qwe"), FILES).unwrap();
    std::os::unix::fs::symlink(&release, d.join("current")).unwrap();
    let s = serve(&d.join("current/main.qwe"), &[]);
    assert_eq!(send(s.port, "GET", "/read?p=pages/a.txt", &[], "").body, "page a");
    assert_eq!(send(s.port, "GET", "/file?p=pages/a.txt", &[], "").body, "page a");
    assert_eq!(send(s.port, "GET", "/write?p=pages/b.txt", &[], "").body, "written");
    assert!(Path::new(&release.join("pages/b.txt")).exists());
}

#[test]
fn allow_path_still_opens_a_folder_on_purpose() {
    let (p, outside) = linked("f41-allow", FILES);
    let s = support::serve_with(&p.join("main.qwe"), &["--allow-path", outside.to_str().unwrap()]);
    assert_eq!(send(s.port, "GET", "/read?p=data/s.txt", &[], "").body, "secret-outside");
    assert_eq!(send(s.port, "GET", "/write?p=data/w.txt", &[], "").body, "written");
    assert_eq!(std::fs::read_to_string(outside.join("w.txt")).unwrap(), "written");
}

// ---- Q-477: a library is loaded only if it is the one built ----------------------------------

/// `quillo` with `args` in `dir`, given at most 300 s (a build may run): exit code and output.
fn quillo(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> (Option<i32>, String) {
    let log = dir.join("run.log");
    let file = std::fs::File::create(&log).unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(dir)
        .envs(env.iter().copied())
        .stdin(std::process::Stdio::null())
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    (code, std::fs::read_to_string(&log).unwrap_or_default())
}

fn libs(dir: &Path, stem: &str) -> Vec<PathBuf> {
    std::fs::read_dir(dir.join(".quillo/native"))
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(&format!("lib{}-", stem))).collect())
        .unwrap_or_default()
}

#[test]
fn a_library_is_loaded_only_if_it_is_the_one_built() {
    let d = temp_dir("f41-lock");
    std::fs::write(d.join("hi.qwer"), "//! effects: none\npub fn hi(name: &str) -> String { format!(\"hi {}\", name) }\n").unwrap();
    std::fs::write(d.join("home.qwer"), "//! effects: env\npub fn home(_name: &str) -> String { std::env::var(\"HOME\").unwrap_or_default() }\n").unwrap();
    std::fs::write(d.join("main.qwe"), "use \"hi.qwer\"\nuse \"home.qwer\"\nprint(hi(\"ana\"))\n").unwrap();
    let (code, out) = quillo(&d, &["build", "main.qwe"], &[]);
    assert_eq!(code, Some(0), "{}", out);
    // The lock: a line for each, its library's name and SHA-256.
    let lock = std::fs::read_to_string(d.join(".quillo/plugins.lock")).unwrap();
    let (hi, home) = (libs(&d, "hi"), libs(&d, "home"));
    assert_eq!((hi.len(), home.len()), (1, 1));
    for lib in [&hi[0], &home[0]] {
        let name = lib.file_name().unwrap().to_string_lossy().into_owned();
        let line = lock.lines().find(|l| l.starts_with(&name)).unwrap_or_else(|| panic!("{} not in {}", name, lock));
        assert_eq!(line.len(), name.len() + 1 + 64, "{}", line);
    }
    let gone = [("CARGO", "/nonexistent/cargo")];
    assert_eq!(quillo(&d, &["run", "main.qwe", "--no-build"], &gone), (Some(0), "hi ana\n".to_string()));
    // Another library under hi's name, as the report did: not loaded, built or not.
    std::fs::copy(&home[0], &hi[0]).unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-check"], &gone);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("is not the library that was built"), "{}", out);
    assert!(!out.contains(std::env::var("HOME").unwrap().as_str()), "the other library ran: {}", out);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-build"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("will not load the library of") && out.contains("quillo build"), "{}", out);
    // Where cargo may run, it is built again, and loads.
    let (code, out) = quillo(&d, &["run", "main.qwe"], &[]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.ends_with("hi ana\n") && out.contains("building it again"), "{}", out);
    // A lock with no line for it is the same as a wrong one.
    std::fs::write(d.join(".quillo/plugins.lock"), "").unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-build"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("has no line in .quillo/plugins.lock"), "{}", out);
}

#[test]
fn no_build_never_builds() {
    let d = temp_dir("f41-no-build");
    std::fs::write(d.join("hi.qwer"), "pub fn hi(name: &str) -> String { format!(\"hi {}\", name) }\n").unwrap();
    std::fs::write(d.join("main.qwe"), "use \"hi.qwer\"\nprint(hi(\"ana\"))\n").unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-build"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("is not built, and --no-build says not to build it here"), "{}", out);
    assert!(!out.contains("quillo: building"), "{}", out);
    assert!(libs(&d, "hi").is_empty());
}

#[test]
fn a_link_that_leads_nowhere_is_not_written_through() {
    let (p, outside) = linked("f41-dangling", FILES);
    std::os::unix::fs::symlink(outside.join("pwned.txt"), p.join("dang")).unwrap();
    let s = serve(&p.join("main.qwe"), &[]);
    assert_eq!(send(s.port, "GET", "/write?p=dang", &[], "").body, "refused");
    let (body, ctype) = multipart("doc", "x.txt", "the visitor's file");
    assert_eq!(send(s.port, "POST", "/up?to=dang", &[("Content-Type", &ctype)], &body).body, "refused");
    assert!(!outside.join("pwned.txt").exists());
}

#[test]
fn files_says_nothing_of_a_folder_outside() {
    let (p, outside) = linked("f41-files", FILES);
    std::fs::create_dir_all(outside.join("sub")).unwrap();
    let s = serve(&p.join("main.qwe"), &[]);
    // There or not, the same 404: no 301 for a folder that is there.
    assert_eq!(send(s.port, "GET", "/static/sub", &[], "").status, 404);
    assert_eq!(send(s.port, "GET", "/static/nope", &[], "").status, 404);
}

#[test]
fn a_program_checked_for_a_visitor_reads_nothing_through_a_link() {
    let (p, outside) = linked("f41-check-program", "");
    std::fs::write(outside.join("x.qwe"), "fn f() = TOPSECRET_TOKEN_123\n").unwrap();
    std::fs::write(p.join("main.qwe"), "route GET / = json.stringify(check_program(\"probe.qwe\", { \"probe.qwe\": \"use \\\"data/x.qwe\\\"\\nprint(f())\" }))\n").unwrap();
    let s = serve(&p.join("main.qwe"), &[]);
    let said = send(s.port, "GET", "/", &[], "").body;
    assert!(said.contains("path must stay inside the program's folder: data/x.qwe"), "{}", said);
    assert!(!said.contains("TOPSECRET"), "{}", said);
}

#[test]
fn a_folder_built_before_locks_loads_as_before_but_not_under_no_build() {
    let d = temp_dir("f41-lockless");
    std::fs::write(d.join("hi.qwer"), "//! effects: none\npub fn hi(name: &str) -> String { format!(\"hi {}\", name) }\n").unwrap();
    std::fs::write(d.join("main.qwe"), "use \"hi.qwer\"\nprint(hi(\"ana\"))\n").unwrap();
    assert_eq!(quillo(&d, &["build", "main.qwe"], &[]).0, Some(0));
    std::fs::remove_file(d.join(".quillo/plugins.lock")).unwrap();
    let gone = [("CARGO", "/nonexistent/cargo")];
    let (code, out) = quillo(&d, &["run", "main.qwe"], &gone);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("plugins.lock is not there") && out.ends_with("hi ana\n"), "{}", out);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-build"], &gone);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("has no line in .quillo/plugins.lock"), "{}", out);
}

// ---- Q-479: a Unix socket of the program's own -------------------------------------------------

fn on_socket(sock: &Path, path: &str) -> String {
    use std::io::{Read, Write};
    let mut s = std::os::unix::net::UnixStream::connect(sock).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    write!(s, "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", path).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or(out)
}

#[test]
fn a_program_listens_on_a_unix_socket_of_its_own() {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    let d = temp_dir("f41-unix");
    std::fs::write(d.join("main.qwe"), "route GET / = `hi from ${request.ip}`\nlet n = 0\nfn tally() { n += 1 }\nschedule(\"1s\", tally)\nroute GET /n = str(n)\n").unwrap();
    let log = std::fs::File::create(d.join("unix.log")).unwrap();
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--listen", "unix:app.sock"])
        .current_dir(&d)
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let mut guard = support::Guard::new(child);
    let sock = d.join("app.sock");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::os::unix::net::UnixStream::connect(&sock).is_err() {
        assert!(std::time::Instant::now() < deadline, "{}", std::fs::read_to_string(d.join("unix.log")).unwrap());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let m = std::fs::symlink_metadata(&sock).unwrap();
    assert!(m.file_type().is_socket());
    assert_eq!(m.permissions().mode() & 0o777, 0o600, "its owner's alone");
    assert_eq!(on_socket(&sock, "/"), "hi from 127.0.0.1");
    // Its jobs reach it through the socket.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while on_socket(&sock, "/n") == "0" {
        assert!(std::time::Instant::now() < deadline, "the job never ran");
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    // A second server is not given the socket another answers on.
    let (code, out) = quillo(&d, &["run", "main.qwe", "--listen", "unix:app.sock"], &[]);
    assert_eq!(code, Some(1));
    assert!(out.contains("a server is listening there"), "{}", out);
    // Stopped, it takes its socket away.
    let _ = std::process::Command::new("kill").args(["-INT", &guard.0.id().to_string()]).status();
    let _ = guard.0.wait();
    assert!(!sock.exists());
}

#[test]
fn a_socket_left_behind_is_taken_and_a_file_is_not() {
    let d = temp_dir("f41-unix-left");
    std::fs::write(d.join("main.qwe"), "route GET / = \"hi\"\n").unwrap();
    // Left by a server that did not stop cleanly: a socket no one answers on.
    drop(std::os::unix::net::UnixListener::bind(d.join("left.sock")).unwrap());
    std::fs::write(d.join("plain.txt"), "a file").unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe", "--listen", "unix:plain.txt"], &[]);
    assert_eq!(code, Some(1));
    assert!(out.contains("it is there, and not a socket"), "{}", out);
    assert_eq!(std::fs::read_to_string(d.join("plain.txt")).unwrap(), "a file");
    for (args, said) in [
        (vec!["--listen", "tcp:80"], "--listen takes unix:PATH"),
        (vec!["--listen", "unix:x.sock", "--socket-mode", "400"], "--socket-mode is octal"),
        (vec!["--listen", "unix:x.sock", "--split"], "is for one process"),
        (vec!["--listen", &format!("unix:{}", "x".repeat(120))], "at most 100 bytes"),
    ] {
        let mut a = vec!["run", "main.qwe"];
        a.extend(args.iter().map(|s| &**s));
        let (code, out) = quillo(&d, &a, &[]);
        assert!(code == Some(1) || code == Some(2), "{:?} {}", code, out);
        assert!(out.contains(said), "{:?}: {}", args, out);
    }
    // The one left behind is taken, with the mode asked for.
    let log = std::fs::File::create(d.join("left.log")).unwrap();
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--listen", "unix:left.sock", "--socket-mode", "660"])
        .current_dir(&d)
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let _guard = support::Guard::new(child);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::os::unix::net::UnixStream::connect(d.join("left.sock")).is_err() {
        assert!(std::time::Instant::now() < deadline, "{}", std::fs::read_to_string(d.join("left.log")).unwrap());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(d.join("left.sock")).unwrap().permissions().mode() & 0o777, 0o660);
    assert_eq!(on_socket(&d.join("left.sock"), "/"), "hi");
}

// ---- Q-480: a process limit too low for the server's threads is said ---------------------------

#[test]
fn a_process_limit_too_low_for_the_threads_is_said_at_start() {
    use std::os::unix::process::CommandExt;
    let d = temp_dir("f41-nproc");
    std::fs::write(d.join("main.qwe"), "route GET / = \"hi\"\n").unwrap();
    let port = support::free_port();
    let log = std::fs::File::create(d.join("nproc.log")).unwrap();
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"));
    cmd.args(["run", "main.qwe", "--port", &port.to_string(), "--workers", "64"]).current_dir(&d).stdout(log.try_clone().unwrap()).stderr(log);
    // SAFETY: only setrlimit, which is async-signal-safe, between fork and exec. The limit is
    // checked when a process is made, so the exec itself is not refused.
    unsafe {
        cmd.pre_exec(|| {
            let mut r = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            libc::getrlimit(libc::RLIMIT_NPROC, &mut r);
            r.rlim_cur = 30;
            libc::setrlimit(libc::RLIMIT_NPROC, &r);
            Ok(())
        });
    }
    let child = cmd.spawn().unwrap();
    let mut guard = support::Guard::new(child);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !support::listening(port) {
        // An account that already has 30 processes and threads (a CI runner's) cannot start
        // even the first: said so, with the limit, and not a panic (found by CI).
        if let Ok(Some(status)) = guard.0.try_wait() {
            let said = std::fs::read_to_string(d.join("nproc.log")).unwrap();
            assert_eq!(status.code(), Some(1), "{}", said);
            assert!(said.contains("quillo: cannot start: no thread can be made (") && said.contains("; the process limit (ulimit -u) is 30, which counts every thread of this account's processes: raise it"), "{}", said);
            assert!(!said.contains("panicked"), "{}", said);
            return;
        }
        assert!(std::time::Instant::now() < deadline, "{}", std::fs::read_to_string(d.join("nproc.log")).unwrap());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let said = std::fs::read_to_string(d.join("nproc.log")).unwrap();
    assert!(said.contains("the process limit (ulimit -u) is 30, and this server may use 73 threads"), "{}", said);
    assert_eq!(send(port, "GET", "/", &[], "").body, "hi");
}

// ---- Q-479: quillo fcgi -------------------------------------------------------------------------

/// A FastCGI request on `s`, as a web server sends it: its STDOUT and the END_REQUEST's statuses.
fn fcgi_ask(s: &mut std::os::unix::net::UnixStream, id: u16, params: &[(&str, &str)], body: &[u8], keep: bool) -> (String, (u32, u8)) {
    use std::io::{Read, Write};
    let rec = |t: u8, c: &[u8]| {
        let pad = (8 - c.len() % 8) % 8;
        let mut r = vec![1, t];
        r.extend_from_slice(&id.to_be_bytes());
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
    let mut m = rec(1, &[0, 1, keep as u8, 0, 0, 0, 0, 0]);
    m.extend(rec(4, &p));
    m.extend(rec(4, &[]));
    if !body.is_empty() {
        m.extend(rec(5, body));
    }
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
        assert_eq!(u16::from_be_bytes([h[2], h[3]]), id);
        match h[1] {
            6 => out.extend_from_slice(&c),
            3 => return (String::from_utf8_lossy(&out).into_owned(), (u32::from_be_bytes([c[0], c[1], c[2], c[3]]), c[4])),
            t => panic!("record {}", t),
        }
    }
}

#[test]
fn quillo_fcgi_answers_a_web_servers_requests() {
    let d = temp_dir("f41-fcgi");
    std::fs::write(
        d.join("main.qwe"),
        "route GET / = `hi from ${request.ip} at ${request.headers.host}`\nroute POST /echo = `got ${request.body}`\nroute GET /none = respond(404, \"nothing\")\nlet n = 0\nfn tally() { n += 1 }\nschedule(\"1s\", tally)\nroute GET /n = str(n)\n",
    )
    .unwrap();
    let log = std::fs::File::create(d.join("fcgi.log")).unwrap();
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["fcgi", "main.qwe", "--listen", "unix:f.sock"])
        .current_dir(&d)
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let mut guard = support::Guard::new(child);
    let sock = d.join("f.sock");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let connect = || std::os::unix::net::UnixStream::connect(&sock);
    while connect().is_err() {
        assert!(std::time::Instant::now() < deadline, "{}", std::fs::read_to_string(d.join("fcgi.log")).unwrap());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let get = |uri: &str| {
        let mut s = connect().unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        fcgi_ask(&mut s, 1, &[("REQUEST_METHOD", "GET"), ("REQUEST_URI", uri), ("HTTP_HOST", "example.com"), ("REMOTE_ADDR", "203.0.113.9"), ("REMOTE_PORT", "5555")], b"", false)
    };
    let (out, end) = get("/");
    assert!(out.starts_with("Status: 200 OK\r\n"), "{}", out);
    assert!(out.ends_with("\r\n\r\nhi from 203.0.113.9 at example.com"), "{}", out);
    assert!(!out.to_ascii_lowercase().contains("connection:"), "{}", out);
    assert_eq!(end, (0, 0));
    let (out, _) = get("/none");
    assert!(out.starts_with("Status: 404 Not Found\r\n") && out.ends_with("nothing"), "{}", out);
    // Two on one connection kept open, a body with the first.
    let mut s = connect().unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let (out, _) = fcgi_ask(&mut s, 1, &[("REQUEST_METHOD", "POST"), ("REQUEST_URI", "/echo"), ("CONTENT_TYPE", "text/plain")], b"hello body", true);
    assert!(out.ends_with("got hello body"), "{}", out);
    let (out, _) = fcgi_ask(&mut s, 2, &[("REQUEST_METHOD", "GET"), ("REQUEST_URI", "/")], b"", true);
    assert!(out.ends_with("hi from 127.0.0.1 at localhost"), "{}", out);
    // Its jobs reach it, though it answers no HTTP of its own.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while get("/n").0.ends_with("\r\n\r\n0") {
        assert!(std::time::Instant::now() < deadline, "the job never ran");
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let _ = std::process::Command::new("kill").args(["-INT", &guard.0.id().to_string()]).status();
    let _ = guard.0.wait();
    assert!(!sock.exists());
}

#[test]
fn quillo_fcgi_needs_a_socket() {
    let d = temp_dir("f41-fcgi-none");
    std::fs::write(d.join("main.qwe"), "route GET / = \"hi\"\n").unwrap();
    // Run from a terminal, descriptor 0 is no socket.
    let (code, out) = quillo(&d, &["fcgi", "main.qwe"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("file descriptor 0 is not a socket"), "{}", out);
    let (code, out) = quillo(&d, &["fcgi", "main.qwe", "--split"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("quillo fcgi is for one process"), "{}", out);
}

// ---- Q-478: a policy from whoever runs it -----------------------------------------------------

const POLICY: &str = "[effects]\nallow = [\"db\", \"http\", \"time\", \"env\"]\n[http]\nprivate = false\n[env]\nnames = [\"APP_NAME\"]\n[plugins]\nallow = [\"hi@1\"]\n";

const POLICED: &str = r#"route GET /w = write_file("x.txt", "y") ?? "refused write"
route GET /env = `${env("APP_NAME") ?? "none"} ${env("HOME") ?? "no home"}`
route GET /local = str(http.get("http://127.0.0.1:9/").error)
route GET /meta = str(http.get("http://169.254.169.254/latest").error)
route GET /fine = "fine"
"#;

#[test]
fn a_policy_is_what_check_says_and_what_run_holds_to() {
    let d = temp_dir("f41-policy");
    std::fs::write(d.join("host.toml"), POLICY).unwrap();
    std::fs::write(d.join("main.qwe"), POLICED).unwrap();
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "host.toml"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("quillo: policy host.toml (effects: db, http, env, time; no private addresses)"), "{}", out);
    assert!(out.contains("main.qwe:1: error: `write_file` needs the fs effect, which host.toml does not allow"), "{}", out);
    assert_eq!(out.matches("error:").count(), 1, "{}", out);
    // Run under it all the same: refused as said.
    let s = support::serve_with(&d.join("main.qwe"), &["--no-check", "--policy", &d.join("host.toml").display().to_string()]);
    assert_eq!(send(s.port, "GET", "/w", &[], "").body, "refused write");
    assert!(!d.join("x.txt").exists());
    assert_eq!(send(s.port, "GET", "/fine", &[], "").body, "fine");
    let local = send(s.port, "GET", "/local", &[], "").body;
    assert!(local.contains("127.0.0.1 is an address of this machine or its network, which") && local.contains("host.toml does not let a program reach"), "{}", local);
    assert!(send(s.port, "GET", "/meta", &[], "").body.contains("169.254.169.254 is an address"));
    // `env` reads the names it gives, and nothing else: as if not set.
    let e = send(s.port, "GET", "/env", &[], "").body;
    assert!(e.ends_with(" no home"), "{}", e);
}

#[test]
fn the_programs_own_policy_only_narrows() {
    let d = temp_dir("f41-policy-narrow");
    std::fs::write(d.join("host.toml"), POLICY).unwrap();
    std::fs::write(d.join("main.qwe"), "route GET / = str(now() > 0)\nroute GET /h = http.get(\"https://example.com/\").status\n").unwrap();
    // It asks for fs, which the host does not give, and gives up http.
    std::fs::write(d.join("quillo.policy"), "[effects]\nallow = [\"db\", \"fs\", \"time\"]\n").unwrap();
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "host.toml"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("policy host.toml + ") && out.contains("quillo.policy (effects: db, time; no private addresses)"), "{}", out);
    assert!(out.contains("main.qwe:2: error: `http.get` needs the http effect"), "{}", out);
    // A policy that does not parse stops it, saying where.
    std::fs::write(d.join("quillo.policy"), "[effects]\nallow = [\"disk\"]\n").unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("quillo.policy:2: `disk` is not an effect"), "{}", out);
}

#[test]
fn a_policy_names_the_plugins_and_versions_that_may_load() {
    let d = temp_dir("f41-policy-plugins");
    std::fs::write(d.join("host.toml"), POLICY).unwrap();
    std::fs::write(d.join("hi.qwer"), "//! version: 2.0.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n").unwrap();
    std::fs::write(d.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy=host.toml"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("main.qwe:1: error: the plugin `hi` 2.0.0 is not in the plugins host.toml allows: hi@1"), "{}", out);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-check", "--policy", "host.toml"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("is not in the plugins host.toml allows") && !out.contains("quillo: building"), "refused before any build: {}", out);
    // At the version it names, it loads; with `build = false`, only as built.
    std::fs::write(d.join("hi.qwer"), "//! version: 1.3.0\n//! effects: none\npub fn hi() -> String { \"hi\".into() }\n").unwrap();
    std::fs::write(d.join("nobuild.toml"), "[plugins]\nbuild = false\n").unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe", "--policy", "host.toml", "--policy", "nobuild.toml"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("--no-build says not to build it here"), "{}", out);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--policy", "host.toml"], &[]);
    assert_eq!((code, out.ends_with("hi\n")), (Some(0), true), "{}", out);
}

#[test]
fn a_server_started_while_another_drains_keeps_its_socket() {
    let d = temp_dir("f41-unix-restart");
    std::fs::write(d.join("main.qwe"), "route GET / = \"new\"\nroute GET /slow { sleep(1500)\n \"old\" }\n").unwrap();
    let start = |log: &str| {
        let f = std::fs::File::create(d.join(log)).unwrap();
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
            .args(["run", "main.qwe", "--listen", "unix:app.sock", "--trust-proxy", "127.0.0.1"])
            .current_dir(&d)
            .stdout(f.try_clone().unwrap())
            .stderr(f)
            .spawn()
            .unwrap();
        support::Guard::new(child)
    };
    let sock = d.join("app.sock");
    let wait_up = || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::os::unix::net::UnixStream::connect(&sock).is_err() {
            assert!(std::time::Instant::now() < deadline, "did not start");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    let mut a = start("a.log");
    wait_up();
    // A request A is still answering when it is told to stop.
    let s2 = sock.clone();
    let slow = std::thread::spawn(move || on_socket(&s2, "/slow"));
    std::thread::sleep(std::time::Duration::from_millis(200));
    let _ = std::process::Command::new("kill").args(["-TERM", &a.0.id().to_string()]).status();
    std::thread::sleep(std::time::Duration::from_millis(300));
    // B, started while A drains: A has let the socket go.
    let _b = start("b.log");
    wait_up();
    assert_eq!(slow.join().unwrap(), "old", "A finished what it had");
    let _ = a.0.wait();
    // A, gone, did not take B's socket with it.
    assert!(sock.exists(), "{}", std::fs::read_to_string(d.join("a.log")).unwrap());
    assert_eq!(on_socket(&sock, "/"), "new");
    assert!(!std::fs::read_to_string(d.join("b.log")).unwrap().contains("--trust-proxy 127.0.0.1 so that"), "no warning when given");
}

#[test]
fn a_socket_server_does_not_join_and_says_what_its_visitors_look_like() {
    let d = temp_dir("f41-unix-join");
    std::fs::write(d.join("main.qwe"), "route GET / = \"hi\"\n").unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe", "--listen", "unix:j.sock", "--as-service", "--join", "http://127.0.0.1:9"], &[]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("not with --split, --instances, --register or --join"), "{}", out);
    let log = std::fs::File::create(d.join("w.log")).unwrap();
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--listen", "unix:w.sock"])
        .current_dir(&d)
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::os::unix::net::UnixStream::connect(d.join("w.sock")).is_err() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let said = std::fs::read_to_string(d.join("w.log")).unwrap();
    assert!(said.contains("is 127.0.0.1 to the program") && said.contains("give --trust-proxy 127.0.0.1"), "{}", said);
}

#[test]
fn a_link_out_of_the_folder_is_said_once() {
    let (p, _) = linked("f41-said", FILES);
    let log = std::fs::File::create(p.join("said.log")).unwrap();
    let port = support::free_port();
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--port", &port.to_string()])
        .current_dir(&p)
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !support::listening(port) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    send(port, "GET", "/read?p=data/s.txt", &[], "");
    send(port, "GET", "/file?p=data/s.txt", &[], "");
    std::thread::sleep(std::time::Duration::from_millis(200));
    let said = std::fs::read_to_string(p.join("said.log")).unwrap();
    assert_eq!(said.matches("leads out of the program's folder through a link").count(), 1, "{}", said);
    assert!(said.contains("data/s.txt leads out") && said.contains("--allow-path FOLDER"), "{}", said);
}

#[test]
fn a_redirect_is_held_to_the_policy_too() {
    let d = temp_dir("f41-policy-redirect");
    let port = support::free_port();
    std::fs::write(d.join("hosts.toml"), "[http]\nhosts = [\"127.0.0.1\"]\n").unwrap();
    std::fs::write(
        d.join("main.qwe"),
        format!(
            "route GET /go = redirect(\"http://localhost:{port}/x\")\nroute GET /here = redirect(\"/x\")\nroute GET /x = \"reached\"\nroute GET /out = str(http.get(\"http://127.0.0.1:{port}/go\").error)\nroute GET /in = http.get(\"http://127.0.0.1:{port}/here\").body\n"
        ),
    )
    .unwrap();
    let log = std::fs::File::create(d.join("r.log")).unwrap();
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--port", &port.to_string(), "--policy", "hosts.toml"])
        .current_dir(&d)
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !support::listening(port) {
        assert!(std::time::Instant::now() < deadline, "{}", std::fs::read_to_string(d.join("r.log")).unwrap());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(send(port, "GET", "/in", &[], "").body, "reached", "a redirect to an allowed host is followed");
    let out = send(port, "GET", "/out", &[], "").body;
    assert!(out.contains("localhost is not among the hosts") && out.contains("hosts.toml allows: 127.0.0.1"), "{}", out);
}

#[test]
fn opening_a_port_is_the_net_effect() {
    let d = temp_dir("f41-net");
    std::fs::write(d.join("only.toml"), "[effects]\nallow = [\"time\"]\n").unwrap();
    std::fs::write(d.join("main.qwe"), "fn got(p) = null\nudp.listen(47611, got)\nprint(\"listening\")\n").unwrap();
    let (code, out) = quillo(&d, &["check", "main.qwe", "--policy", "only.toml"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("`udp.listen` needs the net effect, which only.toml does not allow"), "{}", out);
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-check", "--policy", "only.toml"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(!out.contains("listening\n"), "{}", out);
}

#[test]
fn a_plugin_the_policy_refuses_is_never_built() {
    let d = temp_dir("f41-policy-prebuild");
    std::fs::write(d.join("only.toml"), "[plugins]\nallow = [\"csv\"]\n").unwrap();
    // Two, so that they would be built together before the program loads.
    std::fs::write(d.join("evil.qwer"), "pub fn e() -> bool { true }\n").unwrap();
    std::fs::write(d.join("worse.qwer"), "pub fn w() -> bool { true }\n").unwrap();
    std::fs::write(d.join("main.qwe"), "use \"evil.qwer\"\nuse \"worse.qwer\"\nprint(e())\n").unwrap();
    let (code, out) = quillo(&d, &["run", "main.qwe", "--no-check", "--policy", "only.toml"], &[]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("the plugin `evil` is not in the plugins only.toml allows: csv"), "{}", out);
    assert!(!out.contains("quillo: building"), "{}", out);
    assert!(!d.join(".quillo/native").exists());
}
