// FEEDBACK54.md: real hot module replacement under --watch (branch hmr). Q-688 first: what did
// not change keeps its file and its address.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("f54-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

/// The development files a page loads, by path: (address, text).
fn files(port: u16) -> Vec<(String, String, String)> {
    let page = support::send(port, "GET", "/", &[], "").body;
    let b = page.split("/_quillo/b/").nth(1).and_then(|r| r.split('"').next()).unwrap_or_else(|| panic!("{}", page));
    let loader = support::send(port, "GET", &format!("/_quillo/b/{}", b), &[], "").body;
    let at = loader.split("const at = \"").nth(1).and_then(|r| r.split('"').next()).unwrap_or_else(|| panic!("{}", loader));
    let list = loader.split("for (const f of [").nth(1).and_then(|r| r.split(']').next()).unwrap();
    let named: Vec<String> = serde_json::from_str(&format!("[{}]", list)).unwrap();
    named
        .into_iter()
        .map(|f| {
            let url = format!("{}{}", at, f);
            let a = support::send(port, "GET", &url, &[], "");
            assert_eq!(a.status, 200, "{}", url);
            assert_eq!(a.header("cache-control"), Some("public, max-age=31536000, immutable"), "{}", url);
            let path = f.split_once('/').unwrap().1.to_string();
            (path, url, a.body)
        })
        .collect()
}

fn of<'a>(fs: &'a [(String, String, String)], path: &str) -> &'a (String, String, String) {
    fs.iter().find(|f| f.0 == path).unwrap_or_else(|| panic!("no {} in {:?}", path, fs.iter().map(|f| &f.0).collect::<Vec<_>>()))
}

/// Saves `src` and waits for the server to have loaded it (its page says `mark`).
fn save(port: u16, d: &Path, src: &str, mark: &str) {
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    while !support::send(port, "GET", "/", &[], "").body.contains(mark) {
        assert!(Instant::now() < end, "the save was not loaded");
        std::thread::sleep(Duration::from_millis(50));
    }
}

const TWO: &str = "component First() {\n    state a = 0\n    <p.first>{a} one</p>\n}\ncomponent Second() {\n    state b = 1\n    <div.second><b>{b}</b></div>\n}\ncomponent Third() {\n    state c = 2\n    <div.second><b>{c}</b></div>\n}\nroute GET / = <html><body><First/><Second/><Third/></body></html>\n";

// Q-688: blocks were numbered across the program and one hash named every file, so an edit to
// one component (here a block and a variable more, on its own lines) changed every later file
// and every address. The runtime's file, and the other components', now stay as they were.
#[test]
fn an_edit_to_one_component_leaves_the_others_files_and_addresses_alone() {
    let d = dir("one-edit", TWO);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let before = files(s.port);
    assert_eq!(before.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["quillo.js", "main.qwe/First.js", "main.qwe/Second.js", "main.qwe/Third.js", "boot.js"]);
    // Second's and Third's blocks are the same markup: each file declares its own copy, by one name.
    let block = |t: &str| t.lines().find(|l| l.starts_with("const $$b_")).map(|l| l.split(' ').nth(1).unwrap().to_string());
    assert_eq!(block(&of(&before, "main.qwe/Second.js").2), block(&of(&before, "main.qwe/Third.js").2));
    assert!(!of(&before, "quillo.js").2.contains("W.block("), "no block in the runtime's file");
    let edited = TWO.replace("    <p.first>{a} one</p>", "    let z = a + 1\n    <p.first><i>{a}</i>\n        {a > 0 && <b>y</b>} {z} two</p>");
    save(s.port, &d, &edited, "two");
    let after = files(s.port);
    for path in ["quillo.js", "boot.js"] {
        assert_eq!(of(&before, path).1, of(&after, path).1, "{} moved", path);
    }
    // Below the edit, a component's lines moved: its map says so, so its file is new; above it
    // nothing moved. The one that changed has a new address.
    assert_ne!(of(&before, "main.qwe/First.js").1, of(&after, "main.qwe/First.js").1);
    let unchanged = |t: &str| t.split("PLACES[").next().unwrap().to_string();
    for path in ["main.qwe/Second.js", "main.qwe/Third.js"] {
        assert_eq!(unchanged(&of(&before, path).2), unchanged(&of(&after, path).2), "{}'s code changed", path);
    }
}

// Q-688's edges: an edit on the same lines leaves every other file's address as it was; the old
// address of the changed file is still served (a page that has not reloaded yet), and an address
// with the wrong path or hash is not.
#[test]
fn an_edit_on_its_own_lines_keeps_every_other_address() {
    let d = dir("same-lines", TWO);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let before = files(s.port);
    save(s.port, &d, &TWO.replace("{a} one", "{a} uno"), "uno");
    let after = files(s.port);
    for (path, url, _) in &before {
        if path == "main.qwe/First.js" {
            assert_ne!(url, &of(&after, path).1);
            assert_eq!(support::send(s.port, "GET", url, &[], "").status, 200, "the old address is still served");
        } else {
            assert_eq!(url, &of(&after, path).1, "{} moved", path);
        }
    }
    let first = &of(&after, "main.qwe/First.js").1;
    assert_eq!(support::send(s.port, "GET", &first.replace("First.js", "Second.js"), &[], "").status, 404);
    assert_eq!(support::send(s.port, "GET", "/_quillo/src/0000000000000000/main.qwe/First.js", &[], "").status, 404);
}

fn chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium", "/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// `url` in headless Chrome, with `script` run in the page: what it stored in `data-result`.
fn in_chrome(chrome: &Path, url: &str, script: &str) -> serde_json::Value {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let d = support::temp_dir(&format!("f54-chrome-{}", script.len()));
    std::fs::write(d.join("t.js"), script).unwrap();
    let out = std::process::Command::new("node").arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(d.join("profile")).arg("30000").arg(d.join("t.js")).output().expect("node");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

const COUNTER: &str = "let clicked = false\nroute GET /clicked {\n    clicked = true\n    \"ok\"\n}\nroute GET /was-clicked = clicked\ncomponent Count() {\n    state n = 0\n    <button id=\"b\" onclick={() => { n += 1 }}>{n}</button>\n}\nroute GET / = <html><body><div><Count/></div></body></html>\n";

// Q-686: a save that did not load said nothing on the page, which went on showing the version
// before it. The open page now says so, over itself, and keeps what it holds (no reload); a page
// opened while it stands says so too; the save that loads reloads them, as before.
#[test]
fn a_save_that_does_not_load_is_said_on_the_page_which_keeps_its_state() {
    let d = dir("failed-save", COUNTER);
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let version = support::send(s.port, "GET", "/_quillo/events", &[], "").body;
    // A plain request is told too, after the version it has.
    let broken = COUNTER.replace("{n}</button>", "{nope}</button>");
    let Some(chrome) = chrome() else {
        std::fs::write(d.join("main.qwe"), &broken).unwrap();
        let said = support::send(s.port, "GET", &format!("/_quillo/events?v={}&e=0", version), &[], "").body;
        assert!(said.starts_with("err:1:") && said.contains("nope"), "{}", said);
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    let port = s.port;
    let page = std::thread::spawn(move || {
        let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const b = document.getElementById("b");
  for (let i = 0; i < 3; i++) { b.click(); await tick(30); }
  window.__kept = 1;
  await fetch("/clicked");
  let box = null;
  for (let i = 0; i < 600 && !box; i++) { await tick(30); box = document.querySelector("quillo-error"); }
  const text = box ? box.shadowRoot.querySelector("pre").textContent : null;
  document.body.dataset.result = JSON.stringify({ text, n: document.getElementById("b").textContent, kept: window.__kept === 1, errors: window.__quilloErrors });
})();"#;
        in_chrome(&chrome, &format!("http://127.0.0.1:{}/", port), script)
    });
    let end = Instant::now() + Duration::from_secs(30);
    while support::send(s.port, "GET", "/was-clicked", &[], "").body != "true" {
        assert!(Instant::now() < end && !page.is_finished(), "the page did not click");
        std::thread::sleep(Duration::from_millis(30));
    }
    std::fs::write(d.join("main.qwe"), &broken).unwrap();
    let r = page.join().unwrap();
    assert!(r["text"].as_str().is_some_and(|t| t.contains("nope")), "{}", r);
    assert_eq!((r["n"].as_str(), r["kept"].as_bool()), (Some("3"), Some(true)), "the page was reloaded: {}", r);
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);
    // Opened while the save stands: the version before, and the overlay.
    let chrome = self::chrome().unwrap();
    let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  let box = null;
  for (let i = 0; i < 300 && !box; i++) { await tick(30); box = document.querySelector("quillo-error"); }
  document.body.dataset.result = JSON.stringify({ shown: !!box, n: document.getElementById("b").textContent });
})();"#;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), script);
    assert_eq!((r["shown"].as_bool(), r["n"].as_str()), (Some(true), Some("0")), "{}", r);
    // The save that loads: pages are told the new version (and reload).
    std::fs::write(d.join("main.qwe"), COUNTER.replace("{n}</button>", "{n}!</button>")).unwrap();
    let said = support::send(s.port, "GET", &format!("/_quillo/events?v={}&e=1", version), &[], "").body;
    assert!(said.parse::<u64>().is_ok() && said != version, "{}", said);
    assert!(support::send(s.port, "GET", "/", &[], "").body.contains("0!</button>"));
}
