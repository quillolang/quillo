//! Sessions and cookies across restarts: a server started again with the same `QUILLO_SECRET`
//! keeps who is signed in; without one, or with another, an old cookie reads as signed out,
//! never as an error; a `--watch` reload keeps them; and an app launched again (a new process,
//! its cookie jar and secret on disk) is still signed in.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROGRAM: &str = r#"
route GET /login {
    set_session({ user: "ada" })
    set_cookie("theme", "dark")
    "in"
}
route GET /me = session()?.user ?? "nobody"
route GET /theme = request.cookies.theme ?? "none"
route GET /logout {
    end_session()
    "out"
}
route GET /v = "1"
"#;

struct Running {
    _guard: support::Guard,
    port: u16,
}

fn start(dir: &Path, secret: Option<&str>, args: &[&str]) -> Running {
    let port = support::free_port();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_quillo"));
    cmd.arg("run").arg(dir.join("main.qwe")).args(["--port", &port.to_string()]).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    cmd.env_remove("QUILLO_SECRET");
    if let Some(s) = secret {
        cmd.env("QUILLO_SECRET", s);
    }
    let child = cmd.spawn().unwrap();
    support::stop_with_tests(&child);
    let guard = support::Guard::new(child);
    let until = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < until, "the server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    Running { _guard: guard, port }
}

fn folder(name: &str) -> PathBuf {
    let d = support::temp_dir(&format!("sessions-{}", name));
    std::fs::write(d.join("main.qwe"), PROGRAM).unwrap();
    d
}

/// The cookies an answer set, as a `Cookie` header sends them back.
fn cookies_of(a: &support::Answer) -> String {
    a.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie")).map(|(_, v)| v.split(';').next().unwrap_or("").to_string()).collect::<Vec<_>>().join("; ")
}

fn signed_in(dir: &Path, secret: Option<&str>) -> String {
    let s = start(dir, secret, &[]);
    let login = support::send(s.port, "GET", "/login", &[], "");
    assert_eq!(login.body, "in");
    let jar = cookies_of(&login);
    assert!(jar.contains("theme=dark"), "{}", jar);
    assert_eq!(support::send(s.port, "GET", "/me", &[("cookie", &jar)], "").body, "ada");
    jar
}

const SECRET: &str = "a secret for these tests, long enough to sign sessions with";

#[test]
fn the_same_secret_keeps_a_session_across_a_restart() {
    let d = folder("same");
    let jar = signed_in(&d, Some(SECRET));
    // The first server is gone; a new one, with the same secret.
    let s = start(&d, Some(SECRET), &[]);
    assert_eq!(support::send(s.port, "GET", "/me", &[("cookie", &jar)], "").body, "ada");
    assert_eq!(support::send(s.port, "GET", "/theme", &[("cookie", &jar)], "").body, "dark");
    // Signing out ends it there too.
    let out = support::send(s.port, "GET", "/logout", &[("cookie", &jar)], "");
    assert_eq!(out.body, "out");
    let after = cookies_of(&out);
    assert_eq!(support::send(s.port, "GET", "/me", &[("cookie", &after)], "").body, "nobody");
}

#[test]
fn without_a_secret_or_with_another_an_old_session_is_signed_out_not_an_error() {
    let d = folder("none");
    let jar = signed_in(&d, None);
    let s = start(&d, None, &[]);
    let me = support::send(s.port, "GET", "/me", &[("cookie", &jar)], "");
    assert_eq!((me.status, me.body.as_str()), (200, "nobody"));
    // A plain cookie is the browser's, whatever the key.
    assert_eq!(support::send(s.port, "GET", "/theme", &[("cookie", &jar)], "").body, "dark");

    let d = folder("other");
    let jar = signed_in(&d, Some(SECRET));
    let s = start(&d, Some("another secret, as long, after a rotation of keys"), &[]);
    let me = support::send(s.port, "GET", "/me", &[("cookie", &jar)], "");
    assert_eq!((me.status, me.body.as_str()), (200, "nobody"));
    // Its edges: a session cookie changed by hand, cut short, or empty is signed out too.
    let s = start(&d, Some(SECRET), &[]);
    // `__Host-quillo_session` when the cookie is `Secure`, as here.
    let session = jar.split("; ").find(|c| c.starts_with("quillo_session=") || c.starts_with("__Host-quillo_session=")).expect(&jar).to_string();
    let (name, value) = session.split_once('=').unwrap();
    let flipped: String = value.chars().rev().collect();
    for bad in [format!("{}={}", name, flipped), format!("{}={}", name, &value[..value.len() / 2]), format!("{}=", name), format!("{}=x.y", name)] {
        let me = support::send(s.port, "GET", "/me", &[("cookie", &bad)], "");
        assert_eq!((me.status, me.body.as_str()), (200, "nobody"), "{}", bad);
    }
    assert_eq!(support::send(s.port, "GET", "/me", &[("cookie", &jar)], "").body, "ada");
}

#[test]
fn a_watch_reload_keeps_who_is_signed_in() {
    let d = folder("watch");
    let s = start(&d, None, &["--watch"]);
    let jar = cookies_of(&support::send(s.port, "GET", "/login", &[], ""));
    assert_eq!(support::send(s.port, "GET", "/me", &[("cookie", &jar)], "").body, "ada");
    std::fs::write(d.join("main.qwe"), PROGRAM.replace("route GET /v = \"1\"", "route GET /v = \"2\"")).unwrap();
    let until = Instant::now() + Duration::from_secs(20);
    while support::send(s.port, "GET", "/v", &[], "").body != "2" {
        assert!(Instant::now() < until, "the program was not reloaded");
        std::thread::sleep(Duration::from_millis(50));
    }
    // A new key would sign everyone out at every save.
    assert_eq!(support::send(s.port, "GET", "/me", &[("cookie", &jar)], "").body, "ada");
}

// An app launched again is a new process: its signing key is the one in its folder's
// `.quillo-secret`, and its cookies those in `.quillo/cookies.json`. Each launch runs in a
// process of its own (this test binary, run again for `app_launch`), as the key is kept for a
// process's life.
#[test]
fn an_app_launched_again_is_still_signed_in() {
    let root = support::temp_dir("sessions-app");
    let (bundle, data) = (root.join("bundle"), root.join("data"));
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::write(bundle.join("main.qwe"), PROGRAM).unwrap();
    let launch = |step: &str| {
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "app_launch", "--include-ignored", "--nocapture", "--test-threads", "1"])
            .env("SESSIONS_STEP", step)
            .env("SESSIONS_BUNDLE", &bundle)
            .env("SESSIONS_DATA", &data)
            .env_remove("QUILLO_SECRET")
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{}: {}", step, said);
    };
    launch("sign in");
    assert!(data.join(".quillo-secret").exists());
    launch("still signed in");
    launch("sign out");
    launch("still signed out");
}

#[test]
#[ignore = "run by an_app_launched_again_is_still_signed_in, in a process of its own"]
fn app_launch() {
    let Ok(step) = std::env::var("SESSIONS_STEP") else { return };
    let bundle = PathBuf::from(std::env::var("SESSIONS_BUNDLE").unwrap());
    let data = PathBuf::from(std::env::var("SESSIONS_DATA").unwrap());
    let p = quillo::embed::Program::open(&bundle, &data, "", r#"{ "model": "Test1,1" }"#, "mobile").unwrap();
    let get = |path: &str| String::from_utf8_lossy(&p.request_whole(quillo::embed::request_of("GET", &format!("quillo://app{}", path), vec![], vec![])).body).into_owned();
    match step.as_str() {
        "sign in" => {
            assert_eq!(get("/me"), "nobody");
            assert_eq!(get("/login"), "in");
            assert_eq!(get("/me"), "ada");
        }
        "still signed in" => {
            assert_eq!(get("/me"), "ada");
            assert_eq!(get("/theme"), "dark");
        }
        "sign out" => {
            assert_eq!(get("/logout"), "out");
            assert_eq!(get("/me"), "nobody");
        }
        "still signed out" => assert_eq!(get("/me"), "nobody"),
        other => panic!("no step {}", other),
    }
    // As the app does when it goes to the background: what it holds is written down.
    p.suspend();
    p.stop();
}
