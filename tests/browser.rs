//! Components in a real browser: headless Chrome loads tests/browser/app.qwe,
//! whose page script clicks, types and submits, then records what it saw.
//! Skipped when no Chrome is found (set QUILLO_CHROME to point at one).

mod support;

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
    run_page_args(chrome, app, port, path, script, &[])
}

/// Like `run_page_with_path`, with options for the server.
fn run_page_args(chrome: &Path, app: &Path, port: u16, path: &str, script: Option<&Path>, args: &[&str]) -> serde_json::Value {
    // One Chrome at a time: several at once overload small CI machines.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let server = support::Guard::new(
        Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(app)
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
        .arg(dir.join("run.js"))
        .arg(chrome)
        .arg(format!("http://127.0.0.1:{}{}", port, path))
        .arg(&profile)
        // Generous: CI machines can be several times slower than a laptop.
        .arg("60000")
        .args(script)
        .output()
        .expect("node");
    drop(server);
    let _ = std::fs::remove_dir_all(&profile);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

/// Node 22 or later (run.js uses its built-in WebSocket).
/// A port nothing is listening on (tests run in parallel).
fn free_port() -> u16 {
    support::free_port()
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

/// tests/programs/scroll.qwe: 100,000 rows, loaded 200 at a time, with only the
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
        &root.join("tests/programs/scroll.qwe"),
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

/// Attributes a component takes from data: a spread cannot add handlers,
/// whatever the key's case or `bind:`, nor replace an attribute written
/// out; a sandboxed frame's page follows the state; an SVG animation of an
/// href is given no javascript: URL.
#[test]
fn spread_attributes_from_data_stay_attributes() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("attrs.qwe"), free_port(), Some(&dir.join("attrs.js")));
    let want = serde_json::json!({
        "form": ["/login", "post"],
        "formAttrs": ["action", "href", "id", "method", "title"],
        "link": ["/ok", "window.clicked = 1", "kept"],
        "linkAttrs": ["action", "href", "id", "method", "onclick", "title"],
        "frame": "<p>second</p>",
        "anim": ["about:blank#blocked", "about:blank#blocked"],
        "ran": 0,
        "errors": []
    });
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

/// The ways around the checks on the page's functions, tried in a real
/// page: none runs its text, and ordinary calls still work.
#[test]
fn the_ways_around_the_pages_checks_do_not_run_text() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("guard.qwe"), free_port(), Some(&dir.join("guard.js")));
    let want = serde_json::json!({
        "reflect": "refused",
        "parse": "refused",
        "adopt": "refused",
        "script": "refused",
        "svgscript": "refused",
        "setter": "refused",
        "wrapped": "refused",
        "target": "<b>made</b>",
        "pwned": null
    });
    assert_eq!(r, want);
}

/// Fine-grained updates: a component's code runs once; changing one row's
/// text changes that text and nothing else; a child does not run again when
/// its parent's other state changes; setting the same value does nothing.
#[test]
fn changes_touch_only_what_shows_them() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("fine.qwe"), free_port(), Some(&dir.join("fine.js")));
    let want = serde_json::json!({
        "start": { "parent": 1, "child": 1 },
        "edit": ["characterData"],
        "rows": ["a", "B", "c"],
        "sameRows": true,
        "childOnOther": [],
        "other": "1",
        "same": [],
        "label": ["characterData"],
        "child": "y",
        "runs": { "parent": 1, "child": 1 },
        "remade": 0,
        "errors": []
    });
    assert_eq!(r, want);
}

/// navigate() where nothing follows url() loads the page it names.
#[test]
fn navigate_without_a_single_page_app_loads_the_page() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("goto.qwe"), free_port(), Some(&dir.join("goto.js")));
    let want = serde_json::json!({ "path": "/results", "query": { "q": "rust & go", "page": "2" }, "reloaded": true, "errors": [] });
    assert_eq!(r, want);
}

/// markdown() in a component shows what the server makes of the text, as
/// it is typed.
#[test]
fn markdown_in_a_component_is_the_servers() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("markdown.qwe"), free_port(), Some(&dir.join("markdown.js")));
    assert_eq!(r["first"], "<p><strong>hi</strong> &lt;b&gt;x&lt;/b&gt;</p>\n", "{}", r);
    assert_eq!(r["same"], serde_json::json!([true, true, true, true]), "{}", r);
    let errors = r["errors"].as_array().unwrap();
    assert!(errors.len() == 1 && errors[0].as_str().unwrap().contains("`markdown` expects options as a map, got int"), "{}", r);
}

/// Links pointed at are prefetched: the page they lead to, slow to make, is
/// there when the link is followed.
#[test]
fn a_link_pointed_at_is_there_when_followed() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("prefetch.qwe"), free_port(), Some(&dir.join("prefetch.js")));
    assert_eq!(r, serde_json::json!({ "fast": true, "kept": true, "errors": [] }));
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

/// tests/programs/compose.qwe, clicked through.
#[test]
fn composition_example_works() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with(
        &chrome,
        &root.join("tests/programs/compose.qwe"),
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

/// tests/programs/layout.qwe: a JSON-described page, edited live in the browser.
#[test]
fn json_layout_renders_and_follows_edits() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with(
        &chrome,
        &root.join("tests/programs/layout.qwe"),
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

/// tests/programs/spa.qwe: links, Back and Forward change pages without reloads,
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
        &root.join("tests/programs/spa.qwe"),
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

/// tests/programs/index.qwe mounts every example; the SPA works under /spa.
#[test]
fn mounted_single_page_app_stays_under_its_prefix() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with_path(
        &chrome,
        &root.join("tests/programs/index.qwe"),
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

/// tests/programs/index.qwe: tabs switch examples without reloading the page.
#[test]
fn example_gallery_tabs_switch_without_reloading() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with_path(&chrome, &root.join("tests/programs/index.qwe"), free_port(), "/", Some(&root.join("tests/browser/gallery.js")));
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

/// tests/programs/spa.qwe: stock is live data; the cart reserves and returns it.
#[test]
fn shop_stock_updates_live_and_the_cart_reserves_it() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with(&chrome, &root.join("tests/programs/spa.qwe"), free_port(), Some(&root.join("tests/browser/spa-live.js")));
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

/// tests/programs/life.qwe: the browser computes the same generations as the
/// server, and `every` runs the game only while it plays.
#[test]
fn game_of_life_runs_in_the_browser() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let r = run_page_with(&chrome, &root.join("tests/programs/life.qwe"), free_port(), Some(&root.join("tests/browser/life.js")));
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

/// tests/programs/storm.qwe: storms that mount and unmount components with timers,
/// live data and url() leave nothing behind: no removed component still
/// referenced, no timer running, the page back to its size.
#[test]
fn a_state_storm_leaves_nothing_behind() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut r = run_page_with(&chrome, &root.join("tests/programs/storm.qwe"), free_port(), Some(&root.join("tests/browser/storm.js")));
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

/// The program for the channel tests, listening for UDP on `udp`.
fn channels_program(udp: u16) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-channels-{}-{}", std::process::id(), udp));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/channels.qwe")).unwrap();
    std::fs::write(d.join("channels.qwe"), src.replace("UDP_PORT", &udp.to_string())).unwrap();
    d.join("channels.qwe")
}

/// Pages on a channel hear each other (through the server's handler), the
/// server's broadcasts, and UDP packets the server is sent; a channel whose
/// `join` says no gets nothing; a page that leaves is out.
fn channels_work(args: &[&str], via: &str) {
    let Some(chrome) = chrome() else { return };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let app = channels_program(udp);
    // Readings, as a device would send them, while the page runs.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sender = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            let mut n = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                n += 1;
                let _ = s.send_to(format!("reading {}", n).as_bytes(), ("127.0.0.1", udp));
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };
    let r = run_page_args(&chrome, &app, free_port(), "/", Some(&root.join("tests/browser/channels.js")), args);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    sender.join().unwrap();
    assert_eq!(r["via"], via, "{}", r);
    assert_eq!(r["viaFrame"], via, "{}", r);
    for k in ["bothGot", "bigGot", "serverGot", "udpGot", "orderKept"] {
        assert_eq!(r[k], true, "{}: {}", k, r);
    }
    assert_eq!(r["vip"], "0", "a page the channel refused got its broadcast: {}", r);
    assert_eq!(r["peersBefore"], 2, "{}", r);
    assert_eq!(r["peersAfter"], 1, "{}", r);
    assert_eq!(r["who"], "ana", "the handler did not see the page's session: {}", r);
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);
}

#[test]
fn channels_over_webtransport() {
    channels_work(&[], "webtransport");
}

#[test]
fn channels_over_a_websocket_without_webtransport() {
    channels_work(&["--no-wt"], "websocket");
}

/// Streams a test picture and a tone to `video` and `audio` as RTP, as a
/// camera would (with a keyframe every `gop` frames); `None` without ffmpeg.
fn ffmpeg_rtp(video: u16, audio: u16, gop: u32) -> Option<std::process::Child> {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-re"])
        .args(["-f", "lavfi", "-i", "testsrc=size=640x360:rate=30", "-f", "lavfi", "-i", "sine=frequency=440"])
        .args(["-map", "0:v", "-c:v", "libx264", "-tune", "zerolatency", "-profile:v", "baseline", "-pix_fmt", "yuv420p"])
        .args(["-g", &gop.to_string(), "-f", "rtp", &format!("rtp://127.0.0.1:{}", video)])
        .args(["-map", "1:a", "-c:a", "libopus", "-f", "rtp", &format!("rtp://127.0.0.1:{}", audio)])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// RTP from ffmpeg plays in `<quillo-media>`: video drawn, audio decoded,
/// and a page that comes in the middle of a long gap between keyframes
/// starts at once, from the frames the server kept.
fn media_plays(args: &[&str], via: &str) {
    let Some(chrome) = chrome() else { return };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let free_udp = || std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let (video, audio) = (free_udp(), free_udp());
    let d = std::env::temp_dir().join(format!("quillo-media-{}-{}", std::process::id(), video));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let src = std::fs::read_to_string(root.join("tests/browser/media.qwe")).unwrap();
    std::fs::write(d.join("media.qwe"), src.replace("VIDEO_PORT", &video.to_string()).replace("AUDIO_PORT", &audio.to_string())).unwrap();
    // A keyframe every 10 s, from when the server listens; the page comes
    // 3 s after it.
    let mut ff = None;
    let r = run_page_args_late(&chrome, &d.join("media.qwe"), free_port(), &root.join("tests/browser/media.js"), args, Duration::from_secs(3), || {
        ff = ffmpeg_rtp(video, audio, 300);
        ff.is_some()
    });
    let Some(mut ff) = ff else { return };
    let _ = ff.kill();
    let _ = ff.wait();
    let Some(r) = r else { return };
    assert_eq!(r["via"], via, "{}", r);
    assert_eq!(r["state"], "playing", "{}", r);
    assert!(r["video"].as_u64().unwrap_or(0) >= 45, "too few frames decoded: {}", r);
    assert!(r["liveFrames"].as_u64().unwrap_or(0) >= 20, "frames stopped coming: {}", r);
    assert!(r["audio"].as_u64().unwrap_or(0) > 0, "no audio decoded: {}", r);
    assert_eq!(r["width"], 640, "{}", r);
    assert!(r["colors"].as_u64().unwrap_or(0) > 10, "the picture is blank: {}", r);
    assert!(r["firstFrameMs"].as_u64().unwrap_or(99_999) < 2500, "the first frame took too long: {}", r);
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);
}

/// Like `run_page_args`, with `start` called once the server listens, and
/// the page loaded `late` after; `None` when `start` says it could not.
fn run_page_args_late(chrome: &Path, app: &Path, port: u16, script: &Path, args: &[&str], late: Duration, start: impl FnOnce() -> bool) -> Option<serde_json::Value> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let server = support::Guard::new(
        Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(app)
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
    if !start() {
        return None;
    }
    std::thread::sleep(late);
    let profile = std::env::temp_dir().join(format!("quillo-chrome-{}-{}", std::process::id(), port));
    let out = Command::new("node")
        .arg(dir.join("run.js"))
        .arg(chrome)
        .arg(format!("http://127.0.0.1:{}/", port))
        .arg(&profile)
        .arg("60000")
        .arg(script)
        .output()
        .expect("node");
    drop(server);
    let _ = std::fs::remove_dir_all(&profile);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    Some(serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout))))
}

#[test]
fn media_over_webtransport() {
    media_plays(&[], "webtransport");
}

#[test]
fn media_over_a_websocket_without_webtransport() {
    media_plays(&["--no-wt"], "websocket");
}

/// Decimals and ints past 2^53 cross to the page and back with their type:
/// props, state, a server fn's arguments and answer.
#[test]
fn decimals_cross_to_the_page_and_back() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_with(&chrome, &dir.join("decimals.qwe"), free_port(), Some(&dir.join("decimals.js")));
    let want = serde_json::json!({
        "server": ["25.00", "9007199254740994", "dec int"],
        "added": "25.10",
        "taxed": "2.01",
        "back": "dec 25.10 int 9007199254740993 1.50",
        "big": "9007199254740994",
        "errors": []
    });
    assert_eq!(r, want);
}

/// A strict Content-Security-Policy: components, prefetching and the
/// `--watch` reload run; an inline script without the nonce does not.
#[test]
fn pages_work_under_a_strict_content_security_policy() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let r = run_page_args(&chrome, &dir.join("csp.qwe"), free_port(), "/", Some(&dir.join("csp.js")), &["--watch"]);
    assert_eq!(r, serde_json::json!({ "count": "3", "good": true, "bad": false, "blocked": ["script-src-elem inline"], "errors": [] }));
}

#[test]
fn in_development_components_come_in_files_of_their_own_mapped_to_their_lines() {
    let Some(chrome) = chrome().filter(|_| node_available()) else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let app = dir.join("devmode/main.qwe");
    let source = dir.join("devmode/ui/counter.qwe").canonicalize().unwrap();
    let port = free_port();
    let mut server = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(&app)
        .args(["--port", &port.to_string(), "--watch"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let profile = std::env::temp_dir().join(format!("quillo-chrome-{}-{}", std::process::id(), port));
    // A breakpoint on `count += 1` (line 7), as an editor sets it, through the maps.
    let out = Command::new("node")
        .arg(dir.join("sourcemap.js"))
        .arg(&chrome)
        .arg(format!("http://127.0.0.1:{}/", port))
        .arg(&profile)
        .arg(&source)
        .arg("7")
        .output()
        .expect("node");
    let _ = server.kill();
    let _ = server.wait();
    let _ = std::fs::remove_dir_all(&profile);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let r: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let scripts: Vec<String> = r["scripts"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect();
    let files: Vec<&str> = scripts.iter().filter_map(|s| s.strip_prefix("/_quillo/src/")).map(|s| s.split_once('/').unwrap().1).collect();
    assert_eq!(files, vec!["quillo.js", "program.js", "ui/counter.qwe/label.js", "ui/counter.qwe/Counter.js", "boot.js"]);
    assert!(r["breakpointIn"].as_str().unwrap().ends_with("/ui/counter.qwe/Counter.js"), "{}", r);
    assert_eq!(r["stoppedAt"], 7, "{}", r);
    assert!(r["stops"].as_array().unwrap().iter().all(|l| l == 7), "{}", r);
    assert_eq!(r["sourcesContent"], true);
    assert_eq!(r["text"], "count: 6");
}
