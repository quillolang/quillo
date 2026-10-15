//! Components in a real browser: headless Chrome loads tests/browser/app.qwe,
//! whose page script clicks, types and submits, then records what it saw.
//! Skipped when no Chrome is found (set QUILLO_CHROME to point at one).

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

/// Serves `tests/browser/<app>`, loads it in headless Chrome (driven by
/// run.js over the DevTools protocol) and returns what the page script
/// recorded in `data-result`.
fn run_page(chrome: &Path, app: &str, port: u16) -> serde_json::Value {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let mut server = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(dir.join(app))
        .args(["--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let profile = std::env::temp_dir().join(format!("quillo-chrome-{}-{}", std::process::id(), port));
    let out = Command::new("node")
        .arg(dir.join("run.js"))
        .arg(chrome)
        .arg(format!("http://127.0.0.1:{}/", port))
        .arg(&profile)
        .output()
        .expect("node");
    let _ = server.kill();
    let _ = server.wait();
    let _ = std::fs::remove_dir_all(&profile);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

/// Node 22 or later (run.js uses its built-in WebSocket).
fn node_available() -> bool {
    Command::new("node")
        .args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn live_state_follows_changes_made_elsewhere() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let result = run_page(&chrome, "live.qwe", 22000 + (std::process::id() % 1000) as u16);
    let want = serde_json::json!({
        "before": ["first"],
        "noteArrived": true,
        "after": ["first", "from elsewhere"],
        "hitsArrived": true,
        "hits": "2",
        "errors": []
    });
    assert_eq!(result, want);
}

#[test]
fn components_hydrate_react_and_call_the_server() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let result = run_page(&chrome, "app.qwe", 21000 + (std::process::id() % 1000) as u16);
    let want = serde_json::json!({
        "count": "7",
        "nestedStart": "1",
        "draftShown": "1 items, draft: second",
        "saved": true,
        "items": ["first", "second"],
        "inputCleared": "",
        "sameInput": true,
        "nestedAfter": "1",
        "errors": []
    });
    assert_eq!(result, want);
}
