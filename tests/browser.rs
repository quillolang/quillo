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
    run_page_with(chrome, &dir.join(app), port, None)
}

/// Like `run_page`, for any program, with an optional test script that
/// run.js injects into the page.
fn run_page_with(chrome: &Path, app: &Path, port: u16, script: Option<&Path>) -> serde_json::Value {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let mut server = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(app)
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
        .arg("30000")
        .args(script)
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
        "errors": [],
        "waiting": []
    });
    assert_eq!(result, want);
}

/// examples/scroll.qwe: 100,000 rows, loaded 200 at a time, with only the
/// visible rows and a buffer of 5 on each side in the page.
#[test]
fn infinite_scroll_renders_only_a_window_of_rows() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with(
        &chrome,
        &root.join("examples/scroll.qwe"),
        23000 + (std::process::id() % 1000) as u16,
        Some(&root.join("tests/browser/scroll.js")),
    );
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);
    assert_eq!(r["start"], serde_json::json!({ "rows": 20, "loaded": 200, "ids": [1, 20] }));
    assert_eq!(r["atRow100"]["ids"], serde_json::json!([96, 120]));
    assert_eq!(r["atRow100"]["rows"], 25);
    assert_eq!(r["loadedMore"], true);
    assert!(r["deep"]["loaded"].as_u64().unwrap() > 1000, "{}", r);
    assert_eq!(r["mostRowsAtOnce"], 25, "{}", r);
    assert_eq!(r["afterFailure"], serde_json::json!({ "failed": true, "stuckLoading": false }), "{}", r);
    assert_eq!(r["recovered"], true, "{}", r);
}

/// Components inside components: callbacks from child to parent, HTML
/// props (named slots), `children`, and each component's own state.
#[test]
fn components_compose() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("compose.qwe"), 24000 + (std::process::id() % 1000) as u16, Some(&dir.join("compose.js")));
    let want = serde_json::json!({
        "counts": ["2", "4"],
        "total": "Last change: 2",
        "log": ["apples 1", "apples 2"],
        "header": "<b>Details</b>",
        "closed": true,
        "reopened": "Last change: 2",
        "errors": []
    });
    assert_eq!(r, want);
}

/// examples/compose.qwe, clicked through.
#[test]
fn composition_example_works() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with(
        &chrome,
        &root.join("examples/compose.qwe"),
        24500 + (std::process::id() % 400) as u16,
        Some(&root.join("tests/browser/compose-example.js")),
    );
    let want = serde_json::json!({
        "counts": ["2", "2", "0", "10"],
        "sum": "4",
        "log": ["Crates → 10", "Pears → 2", "Apples → 2", "Apples → 1"],
        "header": "▾ Recent changes (4)",
        "collapsed": true,
        "errors": []
    });
    assert_eq!(r, want);
}

/// examples/layout.qwe: a JSON-described page, edited live in the browser.
#[test]
fn json_layout_renders_and_follows_edits() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with(
        &chrome,
        &root.join("examples/layout.qwe"),
        24900 + (std::process::id() % 90) as u16,
        Some(&root.join("tests/browser/layout.js")),
    );
    let want = serde_json::json!({
        "serverTextarea": true,
        "stats": ["128", "4", "3021"],
        "trendClass": "stat up",
        "link": { "href": "#restock", "cls": "button", "title": "Order more", "text": "Restock" },
        "unknown": "Unknown block “chart”",
        "counter": "10",
        "afterEdit": ["Orders ▲", "Returns ▼", "Visitors", "Refunds ▼"],
        "error": true,
        "replaced": "OnlyHi",
        "errors": []
    });
    assert_eq!(r, want);
}

/// examples/spa.qwe: links, Back and Forward change pages without reloads,
/// and state (the cart) survives.
#[test]
fn single_page_app_navigates_without_reloading() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with(
        &chrome,
        &root.join("examples/spa.qwe"),
        25500 + (std::process::id() % 400) as u16,
        Some(&root.join("tests/browser/spa.js")),
    );
    let want = serde_json::json!({
        "start": ["/", "Welcome"],
        "products": ["/products", "Products", "Products"],
        "product": ["/products/2", "Notebook"],
        "cart": "Cart: 2 (€24)",
        "back": ["/products", "Products"],
        "forward": ["/products/2", "Notebook"],
        "about": ["/about", "About"],
        "cartKept": "2",
        "missing": ["/products/99", "Not found"],
        "noReload": true,
        "errors": []
    });
    assert_eq!(r, want);
}
