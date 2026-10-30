//! `mobile::start`, as a phone app's launcher calls it, and what `platform()` tells the
//! program it starts.

mod support;

use quillo::mobile::{start, Started};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use support::{send, Answer};

/// `start` reads and sets QUILLO_SECRET for the process: one at a time, each from none.
static SECRET: Mutex<()> = Mutex::new(());

fn no_secret() -> MutexGuard<'static, ()> {
    let held = SECRET.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("QUILLO_SECRET");
    held
}

/// A bundle holding these files, and a folder for the app's data.
fn app(name: &str, files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
    let root = support::temp_dir(&format!("mobile-{}", name));
    let (bundle, data) = (root.join("bundle"), root.join("data"));
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    for (file, text) in files {
        std::fs::write(bundle.join(file), text).unwrap();
    }
    (bundle, data)
}

/// The page at `path`, as the app's web view gets it: the launch URL first, then with the
/// cookie that gives.
fn in_the_web_view(s: &Started, path: &str) -> Answer {
    let launch = &s.url[s.url.find("/?").unwrap()..];
    let mut first = send(s.port, "GET", launch, &[], "");
    // A first request can have its connection reset: the token is still good.
    for _ in 0..5 {
        if first.status != 0 {
            break;
        }
        first = send(s.port, "GET", launch, &[], "");
    }
    let cookie = first.header("set-cookie").expect("the launch URL gives a cookie");
    let cookie = cookie.split(';').next().unwrap();
    send(s.port, "GET", path, &[("Cookie", cookie)], "")
}

const PAGE: &str = "route GET / = \"on the phone\"\n";

#[test]
fn a_key_from_an_earlier_launch_is_used_again() {
    let _held = no_secret();
    let (bundle, data) = app("again", &[("main.qwe", PAGE)]);
    let key = "k".repeat(40);
    std::fs::write(data.join(".quillo-secret"), format!("{}\n", key)).unwrap();
    let s = start(&bundle, &data, "", "").unwrap();
    assert_eq!(std::env::var("QUILLO_SECRET").unwrap(), key);
    assert_eq!(std::fs::read_to_string(data.join(".quillo-secret")).unwrap(), format!("{}\n", key), "left as it was");
    assert_eq!(in_the_web_view(&s, "/").body, "on the phone");
}

#[test]
fn a_key_too_short_to_trust_is_replaced() {
    let _held = no_secret();
    let (bundle, data) = app("short", &[("main.qwe", PAGE)]);
    std::fs::write(data.join(".quillo-secret"), "short").unwrap();
    start(&bundle, &data, "", "").unwrap();
    let key = std::fs::read_to_string(data.join(".quillo-secret")).unwrap();
    assert!(key.len() >= 32, "{}", key);
    assert_eq!(std::env::var("QUILLO_SECRET").unwrap(), key);
}

#[test]
fn a_secret_in_the_environment_wins_and_nothing_is_written() {
    let _held = no_secret();
    let (bundle, data) = app("given", &[("main.qwe", PAGE)]);
    let given = "g".repeat(40);
    std::env::set_var("QUILLO_SECRET", &given);
    start(&bundle, &data, "", "").unwrap();
    assert_eq!(std::env::var("QUILLO_SECRET").unwrap(), given);
    assert!(!data.join(".quillo-secret").exists());
}

#[test]
fn a_key_that_cannot_be_kept_stops_the_app() {
    let _held = no_secret();
    let (bundle, data) = app("nokey", &[("main.qwe", PAGE)]);
    std::fs::create_dir_all(data.join(".quillo-secret")).unwrap();
    let e = start(&bundle, &data, "", "").err().unwrap();
    assert!(e.starts_with(&format!("{}: ", data.join(".quillo-secret").display())), "{}", e);
}

#[test]
fn another_file_may_be_the_program() {
    let _held = no_secret();
    let (bundle, data) = app("other", &[("main.qwe", "route GET / = \"main\"\n"), ("second.qwe", "route GET / = \"second\"\n")]);
    let s = start(&bundle, &data, "second.qwe", "").unwrap();
    assert_eq!(in_the_web_view(&s, "/").body, "second");
}

#[test]
fn a_program_the_bundle_does_not_have_is_named() {
    let _held = no_secret();
    let (bundle, data) = app("missing", &[("main.qwe", PAGE)]);
    let e = start(&bundle, &data, "other.qwe", "").err().unwrap();
    assert_eq!(e, "no other.qwe in the app's program");
    let (bundle, data) = app("empty", &[]);
    let e = start(&bundle, &data, "", "").err().unwrap();
    assert_eq!(e, "no main.qwe in the app's program");
}

#[test]
fn a_program_that_does_not_load_says_why() {
    let _held = no_secret();
    let (bundle, data) = app("broken", &[("main.qwe", "fn zero() = 0\nroute GET / = \"hi\"\nprint(1 / zero())\n")]);
    let e = start(&bundle, &data, "", "").err().unwrap();
    assert!(e.contains("division by zero"), "{}", e);
}

#[test]
fn a_program_with_no_routes_has_nothing_to_show() {
    let _held = no_secret();
    let (bundle, data) = app("noroutes", &[("main.qwe", "let x = 1\n")]);
    let e = start(&bundle, &data, "", "").err().unwrap();
    assert!(e.ends_with("main.qwe declares no routes, so there is nothing to show"), "{}", e);
}

#[test]
fn a_bundle_or_a_data_folder_that_cannot_be_used_is_reported() {
    let _held = no_secret();
    let (bundle, data) = app("nofolders", &[("main.qwe", PAGE)]);
    let gone = bundle.join("gone");
    let e = start(&gone, &data, "", "").err().unwrap();
    assert!(e.starts_with(&gone.display().to_string()) && e.contains("No such file"), "{}", e);
    let file = data.join("a-file");
    std::fs::write(&file, "").unwrap();
    let e = start(&bundle, &file, "", "").err().unwrap();
    assert!(e.starts_with(&file.join("app").display().to_string()), "{}", e);
}

#[test]
fn folders_of_the_bundle_come_too() {
    let (bundle, data) = app("sync", &[("main.qwe", PAGE)]);
    std::fs::create_dir_all(bundle.join("lib/deep")).unwrap();
    std::fs::write(bundle.join("lib/deep/x.qwe"), "fn x() = 1\n").unwrap();
    assert_eq!(quillo::mobile::sync(&bundle, &data).unwrap(), 2);
    assert_eq!(std::fs::read_to_string(data.join("lib/deep/x.qwe")).unwrap(), "fn x() = 1\n");
    // The same again, or a database the phone has by now, is left alone.
    std::fs::write(bundle.join("seed.sqlite"), "from the app").unwrap();
    std::fs::write(data.join("seed.sqlite"), "the phone's").unwrap();
    assert_eq!(quillo::mobile::sync(&bundle, &data).unwrap(), 0);
    assert_eq!(std::fs::read_to_string(data.join("seed.sqlite")).unwrap(), "the phone's");
}

/// `platform()`'s facts are read once a process: this is the one test here that asks.
#[test]
fn a_program_sees_what_the_launcher_says_of_the_phone() {
    let _held = no_secret();
    let (bundle, data) = app("platform", &[("main.qwe", "route GET / = platform()\n")]);
    // What is not a JSON object says nothing.
    quillo::platform::set_launcher("not json");
    quillo::platform::set_launcher("[1, 2]");
    let device = r#"{ "model": "Test1,1", "screen": { "width": 402, "scale": 3 }, "simulator": true, "app": { "version": "1.2" }, "os_version": null }"#;
    let s = start(&bundle, &data, "", device).unwrap();
    let a = in_the_web_view(&s, "/");
    assert_eq!(a.status, 200, "{}", a.body);
    let p: serde_json::Value = serde_json::from_str(&a.body).unwrap();
    assert_eq!(p["model"], "Test1,1");
    assert_eq!(p["screen"]["width"], 402);
    assert_eq!(p["simulator"], true);
    assert_eq!(p["app"]["version"], "1.2");
    assert_eq!(p["runtime"], "mobile");
    assert_eq!(p["os"], std::env::consts::OS);
    assert_eq!(p["arch"], std::env::consts::ARCH);
    #[cfg(target_os = "macos")]
    assert!(p["os_version"].is_string(), "a null from the launcher hides nothing: {}", p);
    assert!(matches!(quillo::platform::info(), quillo::Value::Map(_)));
}

#[test]
fn only_the_web_view_gets_in() {
    let _held = no_secret();
    let (bundle, data) = app("gate", &[("main.qwe", PAGE)]);
    let s = start(&bundle, &data, "", "").unwrap();
    assert_eq!(send(s.port, "GET", "/", &[], "").status, 403);
    assert_eq!(send(s.port, "GET", "/?_quillo_launch=guess", &[], "").status, 403);
    assert_eq!(in_the_web_view(&s, "/").body, "on the phone");
    assert!(s.url.starts_with(&format!("http://127.0.0.1:{}/?", s.port)), "{}", s.url);
}
