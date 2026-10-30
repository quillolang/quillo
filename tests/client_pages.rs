//! Component forms in a real browser: headless Chrome loads a program from
//! tests/client_pages, whose script clicks and records what it saw. Skipped
//! when no Chrome is found (set QUILLO_CHROME to point at one).

mod support;

use serde_json::{json, Value};
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

fn node_ready() -> bool {
    Command::new("node").args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"]).output().map(|o| o.status.success()).unwrap_or(false)
}

/// Serves `app` from tests/client_pages, loads `path`, runs `script` in the
/// page and gives what it stored in `data-result`.
fn run_page(chrome: &Path, app: &str, path: &str, script: &str) -> Value {
    // One Chrome at a time: several at once overload small machines.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let port = support::free_port();
    let server = support::Guard::new(
        Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(root.join("client_pages").join(app))
            .args(["--port", &port.to_string()])
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
    let profile = std::env::temp_dir().join(format!("quillo-chrome-pages-{}-{}", std::process::id(), port));
    let out = Command::new("node")
        .arg(root.join("browser/run.js"))
        .arg(chrome)
        .arg(format!("http://127.0.0.1:{}{}", port, path))
        .arg(&profile)
        .arg("60000")
        .arg(root.join("client_pages").join(script))
        .output()
        .expect("node");
    drop(server);
    let _ = std::fs::remove_dir_all(&profile);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

#[test]
fn the_rarer_component_forms_work_in_a_page() {
    let Some(chrome) = chrome().filter(|_| node_ready()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let r = run_page(&chrome, "forms.qwe", "/", "forms.js");
    assert_eq!(r["exception"], Value::Null, "{}", r);
    assert_eq!(r["errors"], json!([]), "{}", r);
    assert_eq!(r["late"], "late 0");
    assert_eq!(r["lateAfter"], "late 1");
    assert_eq!(r["earlyAfter"], "early 2");
    assert_eq!(r["lateGone"], true);
    assert_eq!(r["tried"], "42");
    assert_eq!(r["triedAfter"], Value::Null);
    assert_eq!(r["counted"], "2");
    assert_eq!(r["countedAfter"], "3");
    assert_eq!(r["parts"], "1-2");
    assert_eq!(r["partsAfter"], "2-2");
    // Here, the span itself: for what it holds, see the test after this one.
    assert_ne!(r["flag"], Value::Null);
    assert_eq!(r["bare"], true);
    assert_eq!(r["noclass"], "");
    assert_eq!(r["inblock"], "single");
    assert_eq!(r["guarded"], "one");
    assert_eq!(r["guardedAfter"], "many 2");
    assert_eq!(r["rows"], 1);
    assert_eq!(r["rowsAfter"], json!(["one:Mam", "two:MbmcmM"]));
    assert_eq!(r["counts"], json!(["1.M", "1.M"]));
    assert_eq!(r["nested"], json!(["big.M.M"]));
    assert_eq!(r["titles"], json!(["am.m", "bm.m", "cm.m"]));
    assert_eq!(r["fieldfn"], "A");
    assert_eq!(r["propfn"], "b?");
    assert_eq!(r["localfn"], "11");
    assert_eq!(r["itemfn"], "2");
    assert_eq!(r["letcalls"], "6");
}

#[test]
#[ignore = "finding: `{true}` in markup is the text true on the server and nothing in the browser (runtime.js, put)"]
fn true_as_a_child_is_the_same_text_in_the_browser() {
    let Some(chrome) = chrome().filter(|_| node_ready()) else { return };
    let r = run_page(&chrome, "forms.qwe", "/", "forms.js");
    assert_eq!(r["flag"], "true");
}
