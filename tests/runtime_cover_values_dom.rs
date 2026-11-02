//! The browser runtime's values as the page holds them when a component is
//! taken up, in a real browser: headless Chrome loads
//! tests/browser/cover-values.qwe, whose script (tests/browser/cover-values.js)
//! changes the page before the components are reached and records what they
//! did. Selects the browser chose an option for, one a person changed, an
//! element a script added inside a component, and list rows that show
//! nothing. Skipped when no Chrome is found (set QUILLO_CHROME to point at
//! one) or Node is older than 22.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;

fn chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Node 22 or later (run.js uses its built-in WebSocket).
fn node_available() -> bool {
    Command::new("node")
        .args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Chrome and Node, or None (and the test says it is skipped).
fn browser() -> Option<PathBuf> {
    let c = chrome().filter(|_| node_available());
    if c.is_none() {
        eprintln!("skipping: needs Chrome and Node 22 or later");
    }
    c
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser")
}

/// Serves `tests/browser/<app>` with `args`, loads `/` in headless Chrome
/// (driven by run.js over the DevTools protocol), runs `script` in it and
/// returns what the script recorded in `data-result`.
fn run_page(chrome: &Path, app: &str, script: &str, args: &[&str]) -> serde_json::Value {
    let port = support::free_port();
    let _server = support::Guard::new(
        Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(dir().join(app))
            .args(["--port", &port.to_string()])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let profile = std::env::temp_dir().join(format!("quillo-chrome-{}-{}", std::process::id(), port));
    let out = Command::new("node")
        .arg(dir().join("run.js"))
        .arg(chrome)
        .arg(format!("http://127.0.0.1:{}/", port))
        .arg(&profile)
        // Generous: CI machines can be several times slower than a laptop.
        .arg("60000")
        .arg(dir().join(script))
        .output()
        .expect("node");
    let _ = std::fs::remove_dir_all(&profile);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

/// Taken up when reached: a select the browser chose its first option for, or
/// that has an option `selected`, or a multiple one with none, is as the page
/// gave it, and its component hears nothing; one a person changed first is
/// heard once. An element a script put inside a component has it made again
/// without it (said under --watch only), and the new one works. Rows that show nothing stay so as
/// rows are added, dropped and all taken away.
#[test]
fn fields_and_children_are_read_as_the_page_has_them_when_taken_up() {
    let Some(chrome) = browser() else { return };
    for dev in [true, false] {
        let args: &[&str] = if dev { &["--watch"] } else { &[] };
        let r = run_page(&chrome, "cover-values.qwe", "cover-values.js", args);
        let warned = if dev {
            json!(["quillo: <Extra> was made again, not taken up from the server's HTML: where it wrote <i>, the browser makes nothing"])
        } else {
            json!([])
        };
        let want = json!({
            "changes": "typed b,wake",
            "values": ["a", "b", "b"],
            "woken": "0",
            "bumped": "1",
            "extra": "<button id=\"bump\">1</button>",
            "rows": ["2", "2,4", "2,4", "2,4,4", "", "1"],
            "warned": warned,
            "errors": []
        });
        assert_eq!(r, want, "dev: {}", dev);
    }
}
