//! Starting a new program (FEEDBACK34.md, Q-422 to Q-426; branch feedback27-35-fixes): `quillo
//! new` and its two templates, the reason a missing `migrations/` gives, `delete_cookie` over
//! plain HTTP, and `platform().server`, where the program listens.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use support::send;

fn quillo(d: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quillo")).args(args).current_dir(d).stdin(Stdio::null()).output().unwrap()
}

fn said(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

/// Serves the program in `d`, asks `/`, and gives its status and body.
fn first_page(d: &Path) -> (u16, String) {
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .args(["--port", &port.to_string()])
        .current_dir(d)
        .env("QUILLO_SECRET", "a test secret, long enough to sign sessions")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "did not start");
        std::thread::sleep(Duration::from_millis(30));
    }
    let a = send(port, "GET", "/", &[], "");
    (a.status, a.body)
}

#[test]
fn quillo_new_makes_a_program_that_checks_fixes_nothing_and_serves() {
    let d = support::temp_dir("feedback34-new");
    for (name, args) in [("todo", &[][..]), ("hello", &["--minimal"][..])] {
        let mut a = vec!["new", name];
        a.extend(args);
        let o = quillo(&d, &a);
        assert!(o.status.success(), "{}", said(&o));
        assert!(said(&o).contains(&format!("made {}: cd {} && quillo run --watch", name, name)), "{}", said(&o));
        let p = d.join(name);
        let c = quillo(&p, &["check"]);
        assert!(said(&c).contains("ok (") && !said(&c).contains("warning"), "{}: {}", name, said(&c));
        assert!(said(&quillo(&p, &["fix", "main.qwe"])).contains("nothing to fix"), "{}", name);
        let gi = std::fs::read_to_string(p.join(".gitignore")).unwrap();
        for line in ["*.db", "*.db-wal", "*.db-shm", ".env", ".quillo/"] {
            assert!(gi.lines().any(|l| l == line), "{}: {}", name, gi);
        }
        let (status, body) = first_page(&p);
        assert_eq!(status, 200, "{}: {}", name, body);
        assert!(body.contains("quillo-island") || body.contains("data-c"), "{}: a component on the page", name);
        // No empty folder anywhere (Q-423), and no half-made one left beside it.
        for e in walk(&p) {
            if e.is_dir() {
                assert!(std::fs::read_dir(&e).unwrap().next().is_some(), "{} is empty", e.display());
            }
        }
    }
    let todo = std::fs::read_to_string(d.join("todo/main.qwe")).unwrap();
    assert!(todo.contains("quillo run --watch") && !todo.contains("app/main.qwe"), "its comments say how to run it here");
    assert_eq!(std::fs::read_to_string(d.join("todo/lib/todos.qwe")).unwrap(), include_str!("../app/lib/todos.qwe"));
    assert!(std::fs::read_dir(&d).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains("quillo-new")));
}

fn walk(d: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
            out.push(p);
        }
    }
    out
}

#[test]
fn quillo_new_refuses_what_it_should_and_takes_an_empty_folder() {
    let d = support::temp_dir("feedback34-refuse");
    std::fs::create_dir_all(d.join("full")).unwrap();
    std::fs::write(d.join("full/notes.txt"), "mine").unwrap();
    let o = quillo(&d, &["new", "full"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(said(&o).contains("full is not empty: it has notes.txt"), "{}", said(&o));
    assert_eq!(std::fs::read_to_string(d.join("full/notes.txt")).unwrap(), "mine");
    for bad in ["con", "aux.txt", "COM1", "a:b", "x?", "trailing.", "line\nbreak"] {
        let o = quillo(&d, &["new", bad]);
        assert_eq!(o.status.code(), Some(1), "{:?}", bad);
        assert!(!d.join(bad).exists(), "{:?}", bad);
    }
    for (args, code, text) in [(&["new"][..], 2, "usage: quillo new"), (&["new", "x", "--minimall"][..], 2, "did you mean `--minimal`"), (&["new", "a", "b"][..], 2, "takes one folder"), (&["new", "--help"][..], 0, "usage: quillo new")] {
        let o = quillo(&d, args);
        assert_eq!(o.status.code(), Some(code), "{:?}", args);
        assert!(said(&o).contains(text), "{:?}: {}", args, said(&o));
    }
    // An empty folder, or one holding only `.git`, is used.
    std::fs::create_dir_all(d.join("cloned/.git")).unwrap();
    assert!(quillo(&d, &["new", "cloned", "--minimal"]).status.success());
    assert!(d.join("cloned/main.qwe").exists() && d.join("cloned/.git").exists());
    // A name with quotes or braces lands in the title alone, written safely.
    let o = quillo(&d, &["new", "Tom {app} & 'x'", "--minimal"]);
    assert!(o.status.success(), "{}", said(&o));
    let p = d.join("Tom {app} & 'x'");
    assert!(said(&quillo(&p, &["check"])).contains("ok ("));
    let page = first_page(&p).1;
    assert!(page.contains("<title>Tom {app} &amp; &#39;x&#39;</title>"), "{}", page);
    // Inside another program: said.
    std::fs::write(d.join("main.qwe"), "route GET / = 1\n").unwrap();
    assert!(said(&quillo(&d, &["new", "inner", "--minimal"])).contains("inside another program"));
}

#[test]
fn quillo_new_binary_copies_this_quillo() {
    let d = support::temp_dir("feedback34-binary");
    assert!(quillo(&d, &["new", "b", "--minimal", "--binary"]).status.success());
    let bin = d.join("b/.quillo/bin/quillo");
    assert!(bin.exists());
    let v = Command::new(&bin).arg("--version").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&v.stdout), String::from_utf8_lossy(&Command::new(env!("CARGO_BIN_EXE_quillo")).arg("--version").output().unwrap().stdout));
}

// ---- Q-423 ------------------------------------------------------------------------------------

#[test]
fn a_missing_migrations_folder_says_why_it_may_be_missing() {
    let d = support::temp_dir("feedback34-migrations");
    std::fs::write(d.join("main.qwe"), "db.open(\"app.db\")\ndb.migrate(\"migrations\")\nprint(\"started\")\n").unwrap();
    let o = quillo(&d, &["run", "main.qwe"]);
    assert!(said(&o).contains("there is no such folder (git keeps no empty folder: put the first migration in it, or a .gitkeep"), "{}", said(&o));
    std::fs::create_dir_all(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/.gitkeep"), "").unwrap();
    assert_eq!(String::from_utf8_lossy(&quillo(&d, &["run", "main.qwe"]).stdout), "started\n");
}

// ---- Q-425 ------------------------------------------------------------------------------------

#[test]
fn delete_cookie_over_plain_http_is_not_secure_and_is_over_https() {
    let d = support::temp_dir("feedback34-cookie");
    std::fs::write(
        d.join("main.qwe"),
        "route GET /del = { delete_cookie(\"theme\"); \"deleted\" }\nroute GET /del-secure = { delete_cookie(\"theme\", { secure: true }); \"deleted\" }\nroute GET /del-host = { delete_cookie(\"__Host-x\"); \"deleted\" }\nroute GET /set = { set_cookie(\"theme\", \"dark\"); \"set\" }\n",
    )
    .unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let cookie = |path: &str, h: &[(&str, &str)]| send(s.port, "GET", path, h, "").header("set-cookie").unwrap_or("").to_string();
    let plain = cookie("/del", &[]);
    assert!(plain.starts_with("theme=; Path=/; Max-Age=0") && !plain.contains("Secure"), "{}", plain);
    assert!(cookie("/del", &[("X-Forwarded-Proto", "https")]).contains("Secure"));
    assert!(cookie("/del-secure", &[]).contains("Secure"));
    assert!(cookie("/del-host", &[]).contains("Secure"), "a __Host- cookie needs it");
    // set_cookie keeps its default.
    assert!(cookie("/set", &[]).contains("Secure"));
}

// ---- Q-426 ------------------------------------------------------------------------------------

#[test]
fn a_program_learns_where_it_listens() {
    let d = support::temp_dir("feedback34-server");
    std::fs::write(d.join("main.qwe"), "route GET / = platform().server\nprint(platform().server)\nprint(platform().runtime)\n").unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let got: serde_json::Value = serde_json::from_str(&send(s.port, "GET", "/", &[], "").body).unwrap();
    assert_eq!(got["port"], s.port);
    assert_eq!(got["host"], "127.0.0.1");
    assert_eq!(got["url"], format!("http://127.0.0.1:{}", s.port));
    assert_eq!(got["base"], "");
    // In a script there is no server.
    std::fs::write(d.join("s.qwe"), "print(platform().server)\n").unwrap();
    assert_eq!(String::from_utf8_lossy(&quillo(&d, &["run", "s.qwe"]).stdout), "null\n");
}
