//! Components in a real browser: headless Chrome loads tests/browser/app.qwe,
//! whose page script clicks, types and submits, then records what it saw.
//! Skipped when no Chrome is found (set QUILLO_CHROME to point at one).

use std::io::Read;
use std::net::TcpStream;
use std::path::PathBuf;
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

fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&#39;", "'").replace("&amp;", "&")
}

#[test]
fn components_hydrate_react_and_call_the_server() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome found");
        return;
    };
    let port = 21000 + (std::process::id() % 1000) as u16;
    let app = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/app.qwe");
    let mut server = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(&app)
        .args(["--port", &port.to_string()])
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let profile = std::env::temp_dir().join(format!("quillo-chrome-{}", std::process::id()));
    let mut child = Command::new(&chrome)
        .args(["--headless=new", "--disable-gpu", "--no-first-run", "--no-sandbox", "--virtual-time-budget=10000", "--dump-dom"])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("http://127.0.0.1:{}/", port))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut dom = String::new();
    child.stdout.take().unwrap().read_to_string(&mut dom).unwrap();
    let _ = child.wait();
    let _ = server.kill();
    let _ = std::fs::remove_dir_all(&profile);

    let start = dom.find("data-result=\"").unwrap_or_else(|| panic!("no result in page:\n{}", dom)) + 13;
    let end = start + dom[start..].find('"').unwrap();
    let result: serde_json::Value = serde_json::from_str(&unescape(&dom[start..end])).unwrap();
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
