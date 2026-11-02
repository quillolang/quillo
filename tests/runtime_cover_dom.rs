//! The browser runtime's DOM side, channels and events, in a real browser:
//! headless Chrome loads the tests/browser/cover-*.qwe pages, whose scripts
//! (tests/browser/cover-*.js) act on them and record what they saw. What a
//! handler reads of its event, HTML as a request's body, `range` with a step,
//! the dialog's keys, lists emptied beside other elements, rows moved without
//! moveBefore, the idempotency key without crypto.randomUUID, timers and
//! @mount that fail, a component gone mid-timer, `listen` opening again,
//! navigate() with odd addresses, components made again by themselves, and
//! channels as components come and go and the server stops and starts.
//! Skipped when no Chrome is found (set QUILLO_CHROME to point at one) or
//! Node is older than 22.

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

/// `quillo run` on `app` at `port` with `args`, killed when dropped.
fn serve(app: &Path, port: u16, args: &[&str]) -> support::Guard {
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
    server
}

/// run.js loading `path` of the server at `port`, with `script`.
fn run_js(chrome: &Path, port: u16, path: &str, script: &Path) -> (Command, PathBuf) {
    let profile = std::env::temp_dir().join(format!("quillo-chrome-{}-{}", std::process::id(), port));
    let mut cmd = Command::new("node");
    cmd.arg(dir().join("run.js"))
        .arg(chrome)
        .arg(format!("http://127.0.0.1:{}{}", port, path))
        .arg(&profile)
        // Generous: CI machines can be several times slower than a laptop.
        .arg("60000")
        .arg(script);
    (cmd, profile)
}

fn result_of(out: std::process::Output) -> serde_json::Value {
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

// One Chrome at a time: several at once overload small CI machines.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Serves `tests/browser/<app>` with `args`, loads `path` in headless Chrome
/// (driven by run.js over the DevTools protocol), runs `script` in it and
/// returns what the script recorded in `data-result`.
fn run_page(chrome: &Path, app: &str, path: &str, script: &str, args: &[&str]) -> serde_json::Value {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let port = support::free_port();
    let server = serve(&dir().join(app), port, args);
    let (mut cmd, profile) = run_js(chrome, port, path, &dir().join(script));
    let out = cmd.output().expect("node");
    drop(server);
    let _ = std::fs::remove_dir_all(&profile);
    result_of(out)
}

/// A handler reads its event (`e.checked`, `e.value`, `e.type`, `e.form` of a
/// form with a field given twice and a file, `e.form` of a button in no form,
/// a field neither the event nor the element has); HTML made by raw() or
/// markdown() goes as a request's body as text/html unless the call names a
/// type; `range` with a step counts up, down, and not at all, and a step of 0
/// is an error; Tab in the dialog goes between its buttons and brings focus
/// back from the page; a list beside other elements in its parent is emptied
/// and filled again, all its keys new, and moved without moveBefore with the
/// same elements; an idempotent call without crypto.randomUUID gets a key of
/// 32 hex digits, one for two clicks and another for the next payment.
#[test]
fn events_bodies_ranges_dialog_lists_and_keys_in_the_browser() {
    let Some(chrome) = browser() else { return };
    let r = run_page(&chrome, "cover-dom.qwe", "/", "cover-dom.js", &[]);
    assert_eq!(r["exception"], serde_json::Value::Null, "{}", r);
    assert_eq!(r["checked"], json!([true, "on", "change"]), "{}", r);
    assert_eq!(r["form"], json!({ "who": "ana", "tag": "last", "pic": "photo.png" }), "{}", r);
    assert_eq!(r["stayed"], "/", "a submit reloaded the page: {}", r);
    assert_eq!(r["looseForm"], json!({}), "{}", r);
    assert_eq!(r["fields"], json!([null, 0, null, "loose"]), "{}", r);
    assert_eq!(r["bodies"][0], json!({ "type": "text/html; charset=utf-8", "body": "<b>bold</b>" }), "{}", r);
    assert_eq!(r["bodies"][1], json!({ "type": "text/x-mine", "body": "<i>kept</i>" }), "{}", r);
    assert_eq!(r["bodies"][2]["type"], "text/html; charset=utf-8", "{}", r);
    assert!(r["bodies"][2]["body"].as_str().unwrap_or("").contains("<strong>md</strong>"), "{}", r);
    assert_eq!(r["ranges"], json!([[1, 4, 7], [10, 6, 2], []]), "{}", r);
    assert_eq!(r["zeroError"], json!(["`range`: a step of 0 never gets anywhere"]), "{}", r);
    assert_eq!(r["zeroLeft"], "[[1,4,7],[10,6,2],[]]", "a step of 0 ran its loop: {}", r);
    assert_eq!(r["tabs"], json!(["q-ok", "q-cancel", "q-ok", "q-ok"]), "{}", r);
    assert_eq!(r["stillOpen"], true, "{}", r);
    assert_eq!(r["answer"], "false", "{}", r);
    assert_eq!(r["rowsStart"], json!(["head", "a", "b", "c", "tail"]), "{}", r);
    assert_eq!(r["rowsCleared"], json!(["head", "tail"]), "{}", r);
    assert_eq!(r["rowsBack"], json!(["head", "a", "b", "c", "tail"]), "{}", r);
    assert_eq!(r["rowsNew"], json!(["head", "x", "y", "tail"]), "{}", r);
    assert_eq!(r["oldGone"], true, "{}", r);
    assert_eq!(r["rowsReversed"], json!(["head", "c", "b", "a", "tail"]), "{}", r);
    assert_eq!(r["sameElements"], json!(["c", "b", "a"]), "rows were made again, not moved: {}", r);
    assert_eq!(r["rowsFive"], json!(["head", "e", "d", "c", "b", "a", "tail"]), "{}", r);
    assert_eq!(r["keptMarks"], json!([null, null, "c", "b", "a"]), "{}", r);
    assert_eq!((&r["paidOnce"], &r["paidTwice"]), (&json!("1"), &json!("2")), "{}", r);
    assert_eq!(r["keys"], json!(["hex32", "hex32"]), "{}", r);
    assert_eq!(r["keysDiffer"], true, "{}", r);
    assert_eq!(r["errors"], json!([]), "{}", r);
}

/// `run_after` and `every` whose function fails say so and go on (the next
/// timer runs, the interval ticks on until stopped); a component gone before
/// its `run_after` fires takes the timer and its `every` with it, and one
/// left on the page runs both; an @mount that fails is said and the one
/// beside it runs; `listen` refused with a 503 opens the stream again after
/// a second, and one whose function fails stops (no more values, not opened
/// again).
#[test]
fn timers_mounts_and_listen_survive_failures_and_components_leaving() {
    let Some(chrome) = browser() else { return };
    let r = run_page(&chrome, "cover-timers.qwe", "/", "cover-timers.js", &[]);
    let want = json!({
        "after": "ran",
        "runAfterError": ["division by zero"],
        "ticks": "5",
        "everyError": ["division by zero"],
        "pingsAfterHide": true,
        "afterNeverRan": true,
        "afterWhenKept": true,
        "mounted": "good-mount",
        "mountError": ["division by zero"],
        "flaky": "2",
        "flakyWaited": true,
        "bad": "1,2",
        "hits": { "flaky": 2, "bad": 1 },
        "listenError": ["division by zero"],
        "errors": []
    });
    assert_eq!(r, want);
}

/// navigate() on a page where nothing reads url(): to a spot on the page, the
/// browser goes there and the page stays; to another address, it is loaded as
/// the browser reads it: a space and an accented letter in the path, a key
/// given twice (the last), a bad escape kept as written, a spot with a space.
#[test]
fn navigate_without_url_loads_odd_addresses_as_written() {
    let Some(chrome) = browser() else { return };
    let r = run_page(&chrome, "cover-nav.qwe", "/", "cover-nav.js", &[]);
    let want = json!({
        "name": "café x",
        "path": "/landed/caf%C3%A9%20x",
        "query": { "q": "last", "e": "%zz" },
        "before": { "hash": "#middle", "scrolled": true, "same": true },
        "hash": "#spot%20two",
        "host": "127.0.0.1",
        "reloaded": true,
        "errors": []
    });
    assert_eq!(r, want);
}

/// On a page that reads url() and was opened at its #spot: scrolled away,
/// navigate("#spot") goes there again (scrolled into view, :target) with no
/// new history entry; navigate("?a=1&a=2") changes the query alone, the last
/// of a key given twice.
#[test]
fn navigate_in_a_single_page_app_goes_to_the_spot_again() {
    let Some(chrome) = browser() else { return };
    let r = run_page(&chrome, "cover-nav.qwe", "/spa#spot", "cover-nav-spa.js", &[]);
    let want = json!({
        "atTop": true,
        "backAtSpot": true,
        "target": "spot",
        "sameHistory": true,
        "query": "/spa?{\"a\":\"2\"}",
        "errors": []
    });
    assert_eq!(r, want);
}

/// BUG (src/client/runtime.js, navigate, line 5202): on a page that reads
/// url(), navigate() to another site (here localhost, from 127.0.0.1) calls
/// history.pushState with the other origin, which the browser refuses: the
/// page stays, and the handler's error is the browser's "Failed to execute
/// 'pushState' on 'History' ...". Expected: the other site loaded, as on a
/// page where nothing reads url(), and as a link to it is followed.
#[test]
#[ignore = "bug: navigate() to another origin in a single-page app throws from pushState (runtime.js:5202)"]
fn navigate_to_another_site_from_a_single_page_app_loads_it() {
    let Some(chrome) = browser() else { return };
    let r = run_page(&chrome, "cover-nav.qwe", "/spa", "cover-nav-away.js", &[]);
    assert_eq!((&r["name"], &r["host"], &r["query"]), (&json!("away"), &json!("localhost"), &json!({ "from": "spa" })), "{}", r);
    assert_eq!(r["errors"], json!([]), "{}", r);
}

/// Components inside another, reached (clicked) after the page loaded: one
/// taken up as written keeps its elements; one whose <b> a script made an
/// <em> is made again by itself, with its own working state; a table cell
/// that is its own island is taken up where it stands in its row; raw HTML is
/// kept, in its <quillo-raw>. Under --watch the console says what differed;
/// without it, nothing is said.
#[test]
fn components_inside_another_are_taken_up_or_made_again_by_themselves() {
    let Some(chrome) = browser() else { return };
    for dev in [true, false] {
        let args: &[&str] = if dev { &["--watch"] } else { &[] };
        let r = run_page(&chrome, "cover-remade.qwe", "/", "cover-remade.js", args);
        let warned = if dev {
            json!(["quillo: <Inner> was made again, not taken up from the server's HTML: where it wrote <em>, the browser makes <b>"])
        } else {
            json!([])
        };
        let want = json!({
            "resumedEarly": 0,
            "clicks": "1",
            "inners": ["b:kept", "b:changed"],
            "keptSame": true,
            "bumped": ["1", "2"],
            "cellSame": true,
            "cellIsland": "Cell",
            "cellParent": "tr",
            "cell": "x 1",
            "rawSame": true,
            "rawWrapped": "quillo-raw",
            "saidSame": true,
            "remade": 1,
            "warned": warned,
            "errors": []
        });
        assert_eq!(r, want, "dev: {}", dev);
    }
}

/// BUG (src/client/runtime.js, runCalc, line 4492): a component whose text a
/// script changed (same elements, other words) is not made again: the text
/// is a dynamic part, an effect, and runCalc reports the Mismatch thrown while
/// taking it up ("text", as a page error) and empties the place, instead of
/// letting it reach island()/resume(), which would make the component again.
/// The same happens to a component that shows url().hash on a page opened at
/// a #hash, which the guide says is made again in the browser. Expected:
/// "as written", made again once, no errors; actual: "", 0, ["text"].
#[test]
#[ignore = "bug: a Mismatch in a dynamic text part is reported and emptied, not made again (runtime.js:4492)"]
fn a_changed_text_inside_a_component_makes_it_again() {
    let Some(chrome) = browser() else { return };
    let r = run_page(&chrome, "cover-remade.qwe", "/", "cover-remade-text.js", &[]);
    assert_eq!(r, json!({ "clicks": "1", "said": "as written", "remade": 1, "errors": [] }));
}

/// Two components on one channel in a page: one whose function fails on a
/// message says so, the other still has it, and the first hears on; one
/// removed, the page stays in (and hears); both removed, it leaves (a
/// broadcast reaches no one); one back, it joins again with a fresh list,
/// hears and sends. A message sent from @mount, before the page was let in,
/// arrives once.
fn channels_follow_components(args: &[&str], via: &str) {
    let Some(chrome) = browser() else { return };
    let r = run_page(&chrome, "cover-channels.qwe", "/", "cover-channels.js", args);
    let want = json!({
        "via": via,
        "joined": true,
        "early": 1,
        "bothGotOne": true,
        "boomBoth": true,
        "aHearsOn": true,
        "boomErrors": ["division by zero"],
        "peersWithB": "1",
        "bGotTwo": true,
        "left": true,
        "sentToNone": "0",
        "rejoined": true,
        "aAgain": ["four"],
        "sentAfterRejoin": true,
        "errors": []
    });
    assert_eq!(r, want);
}

#[test]
fn channels_follow_components_over_webtransport() {
    channels_follow_components(&[], "webtransport");
}

#[test]
fn channels_follow_components_over_a_websocket() {
    channels_follow_components(&["--no-wt"], "websocket");
}

/// What stands between the page and the server, played by the page's script:
/// a channel ticket refused (502, as a proxy says while the program restarts)
/// is asked for again after a wait, and no longer once nothing on the page
/// listens; with UDP blocked (the ticket's WebTransport port answers nothing)
/// the page gives up on WebTransport and uses a WebSocket, joins and hears.
#[test]
fn a_refused_ticket_is_asked_again_and_blocked_webtransport_falls_back() {
    let Some(chrome) = browser() else { return };
    let r = run_page(&chrome, "cover-channels.qwe", "/plain", "cover-channels-ticket.js", &[]);
    let want = json!({
        "askedAgain": true,
        "waited": true,
        "stoppedAsking": true,
        "noVia": true,
        "via": "websocket",
        "joined": true,
        "heard": true,
        "errors": []
    });
    assert_eq!(r, want);
}

/// The server stops (killed) and starts again on the same port: the page's
/// channel connection drops, a message sent while it is down is kept and
/// sent once the new server lets the page in, and the page hears the new
/// server. Over a WebSocket, whose end the page sees at once (a WebTransport
/// session waits out QUIC's idle timeout).
#[test]
fn a_channel_is_found_again_when_the_server_restarts() {
    let Some(chrome) = browser() else { return };
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let app = dir().join("cover-channels.qwe");
    let port = support::free_port();
    let first = serve(&app, port, &["--no-wt"]);
    let (mut cmd, profile) = run_js(&chrome, port, "/", &dir().join("cover-channels-restart.js"));
    // What run.js prints, in files, so that the page is a Guard (stopped if this fails).
    let said = (profile.with_extension("out"), profile.with_extension("err"));
    let mut page = support::Guard::new(
        cmd.stdout(std::fs::File::create(&said.0).unwrap()).stderr(std::fs::File::create(&said.1).unwrap()).spawn().expect("node"),
    );
    // The page has sent its first message: stop the server.
    let deadline = Instant::now() + Duration::from_secs(30);
    while !support::send(port, "GET", "/log", &[], "").body.contains("a says") {
        assert!(Instant::now() < deadline, "the page never sent its first message");
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(first);
    std::thread::sleep(Duration::from_millis(1500));
    let second = serve(&app, port, &["--no-wt"]);
    let status = page.0.wait().expect("node");
    drop(second);
    let out = std::process::Output { status, stdout: std::fs::read(&said.0).unwrap(), stderr: std::fs::read(&said.1).unwrap() };
    let _ = std::fs::remove_dir_all(&profile);
    let _ = std::fs::remove_file(&said.0);
    let _ = std::fs::remove_file(&said.1);
    let r = result_of(out);
    let want = json!({
        "via": "websocket",
        "heardBefore": true,
        "down": true,
        "back": true,
        "rejoined": true,
        "keptArrived": ["a says"],
        "heardTwice": true,
        "heardNew": true,
        "items": ["a says", "a says", "after"],
        "errors": []
    });
    assert_eq!(r, want);
}
