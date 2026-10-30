//! `quillo app` with a real window: the page loads in the web view, its title reaches the
//! window, and links go where they should. A stub `open` on the PATH stands in for the
//! system's browser. It puts a window on the screen, so it runs only with
//! QUILLO_TEST_WINDOW set.

#![cfg(all(feature = "app", target_os = "macos"))]

mod support;

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROGRAM: &str = r#"route GET / = <html><head><title>First</title></head><body><script src="/assets/first.js"></script></body></html>
route GET /second = <html><body><p>second</p><script src="/assets/second.js"></script></body></html>
route GET /assets/*path = file("public/" + path)
route GET /seen/:what {
    print(`seen ${what}`)
    "ok"
}
"#;

const FIRST: &str = r#"(async () => {
    const seen = (what) => fetch("/seen/" + what);
    await seen("page");
    document.title = "Second";
    await new Promise((r) => setTimeout(r, 300));
    document.title = "  ";
    await new Promise((r) => setTimeout(r, 300));
    await seen("titled");
    window.open("https://example.test/new-window");
    await new Promise((r) => setTimeout(r, 300));
    window.open("/second");
})();
"#;

const SECOND: &str = r#"(async () => {
    const seen = (what) => fetch("/seen/" + what);
    await seen("own-window");
    location.href = "https://example.test/navigated";
    await new Promise((r) => setTimeout(r, 300));
    await seen("stayed");
    location.href = "javascript-free:nothing";
    await new Promise((r) => setTimeout(r, 300));
    await seen("done");
})();
"#;

fn until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// An `open` command that writes down what it was given.
fn stub_open(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let stub = dir.join("open");
    std::fs::write(&stub, format!("#!/bin/sh\necho \"$1\" >> '{}'\n", dir.join("opened").display())).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_page_runs_in_the_window_and_outside_links_go_to_the_browser() {
    if std::env::var("QUILLO_TEST_WINDOW").is_err() {
        return;
    }
    let d = support::temp_dir("app-window");
    std::fs::create_dir_all(d.join("public")).unwrap();
    std::fs::write(d.join("app.qwe"), PROGRAM).unwrap();
    std::fs::write(d.join("public/first.js"), FIRST).unwrap();
    std::fs::write(d.join("public/second.js"), SECOND).unwrap();
    stub_open(&d);
    let log = d.join("out.log");
    let path = format!("{}:{}", d.display(), std::env::var("PATH").unwrap());
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["app", "app.qwe"])
        .current_dir(&d)
        .env("HOME", &d)
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&log).unwrap())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _window = support::Guard::new(child);
    let out = || std::fs::read_to_string(&log).unwrap_or_default();
    let opened = || std::fs::read_to_string(d.join("opened")).unwrap_or_default();

    assert!(until(|| out().contains("seen titled")), "the page did not run: {:?}", out());
    assert!(until(|| out().contains("seen own-window")), "the program's own link stayed out of the window: {:?}", out());
    assert!(until(|| out().contains("seen stayed")), "an outside link took the window away: {:?}", out());
    assert!(until(|| out().contains("seen done")), "{:?}", out());
    assert!(until(|| opened().contains("https://example.test/navigated")), "{:?}", opened());
    assert!(opened().contains("https://example.test/new-window"), "{:?}", opened());
    assert!(!opened().contains("javascript-free"), "only web and mail links go to the browser: {:?}", opened());
    let _ = std::fs::remove_dir_all(&d);
}
