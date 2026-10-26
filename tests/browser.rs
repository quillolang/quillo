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

/// Keyed rows moved, removed and inserted in place (the rows kept are the
/// same elements), classes and content switching, handlers innermost first,
/// and rows that follow what they read.
#[test]
fn keyed_lists_move_only_what_changed() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("lists.qwe"), free_port(), Some(&dir.join("lists.js")));
    let want = serde_json::json!({
        "swapped": ["item 5, item 2, item 3, item 4, item 1, item 6", [], true],
        "swapMoves": [2, 2],
        "removed": ["item 5, item 2, item 4, item 1, item 6", [], true],
        "removeMoves": [0, 1],
        "inserted": ["item 5, item 10, item 2, item 4, item 1, item 6", ["item 10"], true],
        "reversed": ["item 6, item 1, item 4, item 2, item 10, item 5", ["item 10"], true],
        "picked": ["", "picked", "", "", "", ""],
        "clicks": "row 1,list",
        "rich": ["<b>item 6</b>", "<b>item 1</b>"],
        "plain": ["item 6", "item 1"],
        "clicksAfter": "row 1,list,x:row 6,list",
        "errors": []
    });
    assert_eq!(r, want);
}

/// SVG inside other markup: what a component draws is SVG's, also what
/// loops and conditions add later; a <foreignObject> holds HTML again.
#[test]
fn svg_inside_html_draws() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("svg.qwe"), free_port(), Some(&dir.join("svg.js")));
    let want = serde_json::json!({ "svg": true, "allSvg": true, "rects": 4, "circle": true, "html": true, "cls": "on", "drawn": true, "errors": [] });
    assert_eq!(r, want);
}

/// A page loads its components' code when it is needed: not before someone
/// reaches for one (what they typed and clicked meanwhile is kept), and at
/// once for one that runs by itself. It resumes on the server's HTML: the
/// same elements, not made again.
#[test]
fn islands_load_their_code_when_it_is_needed() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("lazy.qwe"), free_port(), Some(&dir.join("lazy.js")));
    let want = serde_json::json!({ "idleLoads": false, "loadedOnFocus": true, "echo": "Ada", "clicked": "Go 1", "value": "Ada",
        "sameNodes": true, "remade": 0, "errors": [] });
    assert_eq!(r, want);
    let r = run_page_with_path(&chrome, &dir.join("lazy.qwe"), free_port(), "/clock", Some(&dir.join("lazy.js")));
    assert_eq!(r, serde_json::json!({ "clockRuns": true, "errors": [] }));
}

/// `ask` waits for an answer in a dialog on the page (a dangerous question
/// starting on "Cancel", Escape saying no); `notify` shows a note that goes.
#[test]
fn ask_waits_for_an_answer_and_notify_goes_by_itself() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("notices.qwe"), free_port(), Some(&dir.join("notices.js")));
    let want = serde_json::json!({ "dialog": "alertdialog", "title": "Delete", "focus": "q-cancel", "waits": "3", "after": "2", "gone": true,
        "toast": "Deleted", "toastGone": true, "escaped": "2", "focusBack": "del", "errors": [] });
    assert_eq!(r, want);
}

/// `highlight` colours code the same in the browser (as an editor types) as
/// on the server (posts' code blocks), for every language it knows.
#[test]
fn highlight_is_the_same_on_both_sides() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("highlight.qwe"), free_port(), Some(&dir.join("highlight.js")));
    let want = serde_json::json!({ "samples": 10, "same": true, "differ": [], "coloured": 9, "errors": [] });
    assert_eq!(r, want);
}

/// Components with the page's JavaScript: `@mount` and its cleanup, `js`
/// and `js_new`, and methods of a library's objects.
#[test]
fn components_call_the_pages_javascript() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("interop.qwe"), free_port(), Some(&dir.join("interop.js")));
    let want = serde_json::json!({
        "mounted": ["gauge 1", 1],
        "called": ["gauge 2", "4"],
        "again": ["gauge 3", 1],
        "removed": [false, ["gauge"]],
        "back": ["gauge 3", 2],
        "errors": []
    });
    assert_eq!(r, want);
}

/// A component's translations: the server's locale and text, then the same
/// plural rules in the browser.
#[test]
fn components_translate_in_the_browser() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with_path(&chrome, &dir.join("i18n.qwe"), free_port(), "/?lang=ru", Some(&dir.join("i18n.js")));
    let want = serde_json::json!({
        "seen": ["1 товар", "2 товара", "3 товара", "4 товара", "5 товаров"],
        "button": "Добавить",
        "locale": "ru",
        "errors": []
    });
    assert_eq!(r, want);
}

/// Composition where the fast paths are: a layout with named slots, views
/// with children around keyed loops, components keeping their state in
/// kept rows, nested loops, spread attributes and SVG.
#[test]
fn composition_survives_kept_rows() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("slots.qwe"), free_port(), Some(&dir.join("slots.js")));
    let want = serde_json::json!({
        "layout": ["Groups", "<a href=\"/about\">About</a>"],
        "start": [
            { "title": "fruit", "footer": "<em>2 items</em>", "open": "tallies open", "tallies": ["apple=0/1", "pear=0/3"], "badges": ["1", "3!"], "radii": ["1", "3"] },
            { "title": "veg", "footer": "<em>1 items</em>", "open": "tallies", "tallies": ["leek=0/2"], "badges": ["2"], "radii": ["2"] }
        ],
        "after": [
            { "title": "fruits", "footer": "<em>2 items</em>", "open": "tallies", "tallies": ["apple=0/3", "pear=2/3"], "badges": ["3!", "3!"], "radii": ["3", "3"] },
            { "title": "veg", "footer": "<em>2 items</em>", "open": "tallies open", "tallies": ["leek=0/2", "kale=0/1"], "badges": ["2", "1"], "radii": ["2", "1"] }
        ],
        "summary": ["fruits=0", "veg=1"],
        "samePear": true,
        "shared": [["apple=0/3", "pear=2/3"], ["apple=0/3", "pear=0/3"]],
        "sharedAfter": [[["apple=0/5", "pear=3/3"], ["5!", "3!"]], [["apple=1/5", "pear=0/3"], ["5!", "3!"]]],
        "unshared": [[["apple=0/5"], ["5!"]], [["leek=0/2"], ["2"]]],
        "spread": ["extra", "1"],
        "svg": "http://www.w3.org/2000/svg",
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

#[test]
fn pages_read_streams_as_they_come() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let result = run_page(&chrome, "streams.qwe", free_port());
    let want = serde_json::json!({
        "progressDone": true,
        "names": "progress,progress,progress",
        "replyDone": true,
        "replySteps": true,
        "ticking": true,
        "tickerStopped": true,
        "errors": []
    });
    assert_eq!(result, want);
}
