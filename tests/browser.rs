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
    run_page_with_path(chrome, app, port, "/", script)
}

fn run_page_with_path(chrome: &Path, app: &Path, port: u16, path: &str, script: Option<&Path>) -> serde_json::Value {
    // One Chrome at a time: several at once overload small CI machines.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
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
        .arg(format!("http://127.0.0.1:{}{}", port, path))
        .arg(&profile)
        // Generous: CI machines can be several times slower than a laptop.
        .arg("60000")
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
/// A port nothing is listening on (tests run in parallel).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

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
    let result = run_page(&chrome, "live.qwe", free_port());
    let want = serde_json::json!({
        "before": ["first"],
        "noteArrived": true,
        "after": ["first", "from elsewhere"],
        "hitsArrived": true,
        "hits": "2",
        "note": "first",
        "noteFollowsId": true,
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
    let result = run_page(&chrome, "app.qwe", free_port());
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
        free_port(),
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
    let r = run_page_with(&chrome, &dir.join("compose.qwe"), free_port(), Some(&dir.join("compose.js")));
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
        free_port(),
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
        free_port(),
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
        free_port(),
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

/// examples/index.qwe mounts every example; the SPA works under /spa.
#[test]
fn mounted_single_page_app_stays_under_its_prefix() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with_path(
        &chrome,
        &root.join("examples/index.qwe"),
        free_port(),
        "/spa",
        Some(&root.join("tests/browser/mounted.js")),
    );
    let want = serde_json::json!({
        "list": ["/spa/products", "Products"],
        "product": ["/spa/products/2", "Notebook"],
        "cart": "1",
        "back": "/spa/products",
        "noReload": true,
        "errors": []
    });
    assert_eq!(r, want);
}

/// examples/index.qwe: tabs switch examples without reloading the page.
#[test]
fn example_gallery_tabs_switch_without_reloading() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with_path(&chrome, &root.join("examples/index.qwe"), free_port(), "/", Some(&root.join("tests/browser/gallery.js")));
    let want = serde_json::json!({
        "start": ["/", "Todo", "/todo"],
        "shop": ["/?ex=spa", "Shop", "/spa"],
        "layout": ["/?ex=layout", "JSON layout", "/layout"],
        "about": "A page described as JSON, with a live editor.",
        "back": ["/?ex=spa", "Shop", "/spa"],
        "external": "_blank",
        "noReload": true,
        "errors": []
    });
    assert_eq!(r, want);
}

/// examples/spa.qwe: stock is live data; the cart reserves and returns it.
#[test]
fn shop_stock_updates_live_and_the_cart_reserves_it() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with(&chrome, &root.join("examples/spa.qwe"), free_port(), Some(&root.join("tests/browser/spa-live.js")));
    let want = serde_json::json!({
        "before": "10 in stock",
        "liveArrived": true,
        "penBefore": "Only 1 left",
        "penAfter": ["Sold out", true, true],
        "cartLink": "Cart: 1 (€58)",
        "penReturned": 1,
        "penPage": "Only 1 left",
        "checkout": ["Thank you! 1 item ordered.", "0"],
        "callWhileManyWait": true,
        "allFramesLive": true,
        "errors": []
    });
    assert_eq!(r, want);
}

/// examples/life.qwe: the browser computes the same generations as the
/// server, and `every` runs the game only while it plays.
#[test]
fn game_of_life_runs_in_the_browser() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with(&chrome, &root.join("examples/life.qwe"), free_port(), Some(&root.join("tests/browser/life.js")));
    let want = serde_json::json!({
        "start": ["0", "36"],
        "one": "39",
        "thirty": ["30", "41"],
        "speed": "25/s",
        "played": true,
        "stopped": true,
        "flipped": true,
        "cleared": ["0", true, true],
        "cells": 1536,
        "errors": []
    });
    assert_eq!(r, want);
}

/// examples/storm.qwe: storms that mount and unmount components with timers,
/// live data and url() leave nothing behind: no removed component still
/// referenced, no timer running, the page back to its size.
#[test]
fn a_state_storm_leaves_nothing_behind() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut r = run_page_with(&chrome, &root.join("examples/storm.qwe"), free_port(), Some(&root.join("tests/browser/storm.js")));
    // Reported for information; varies from run to run.
    r.as_object_mut().unwrap().remove("heapGrowthKB");
    let want = serde_json::json!({
        "start": ["12", 12],
        "stormed": true,
        "componentsSeen": true,
        "removedButAlive": 0,
        "timersRunning": 0,
        "sameNodes": true,
        "heapGrowthUnder1MB": true,
        "errors": []
    });
    assert_eq!(r, want);
}
