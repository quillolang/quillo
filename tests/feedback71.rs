//! FEEDBACK71.md, the security audit of every entry point (2026-11-16): the fixes made on branch
//! feedback71. Q-968: a path of the site's own whose dot segments hid a second `/` (`/.//evil.com`)
//! was sent as `Location: //evil.com`, another host, through the guide's own
//! `redirect(local_path(next) ?? "/")`; `local_path` let such a path through, on the server and in
//! the browser.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("feedback71-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

/// What `quillo run` prints for a script, under a deadline.
fn run_script(d: &Path) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the script did not end");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let o = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

// ---- Q-968: redirect and local_path ----------------------------------------------------------

const REDIRECTS: &str = r#"route GET /to ? { u: str } = redirect(u)
route GET /go ? { next: str = "/" } = redirect(local_path(next) ?? "/")
route GET /safe ? { u: str } = starts_with(u, "/") && !starts_with(u, "//") ? redirect(u) : "no"
"#;

#[test]
fn a_path_whose_dots_hide_a_second_slash_stays_on_the_site() {
    let d = dir("dots", REDIRECTS);
    for (base, opts) in [("", &[][..]), ("/app", &["--base", "/app"][..])] {
        let s = support::serve_with(&d.join("main.qwe"), opts);
        let to = |route: &str, u: &str| {
            let a = support::send(s.port, "GET", &format!("/{}?{}={}", route, if route == "go" { "next" } else { "u" }, enc(u)), &[], "");
            assert_eq!(a.status, 303, "{} {} under {:?}: {}", route, u, base, a.body);
            a.header("location").unwrap_or_default().to_string()
        };
        for (u, want) in [
            // Each was `//evil.com` (or `///evil.com`), which a browser takes to evil.com.
            ("/.//evil.com", "/evil.com"),
            ("/%2e//evil.com", "/evil.com"),
            ("/%2E//evil.com", "/evil.com"),
            ("/a/..//evil.com", "/evil.com"),
            ("/a/b/../..//evil.com", "/evil.com"),
            ("/..//evil.com", "/evil.com"),
            ("/.%2e//evil.com", "/evil.com"),
            ("/.///evil.com", "/evil.com"),
            ("/./\\evil.com", "/evil.com"),
            ("/.\\/evil.com", "/evil.com"),
            ("/.//evil.com/x?y=//z#//w", "/evil.com/x?y=//z#//w"),
            ("/.//", "/"),
            ("/.//é", "/%C3%A9"),
            // `//` and a host IDNA cannot write once the dots were out: an error value before, a
            // path of the site's own now (found in the second review).
            ("/..//\u{85}", "/%C2%85"),
            // Not at the start, a `//` is the site's own path, as before.
            ("/a/.//b", "/a//b"),
            ("/a//b", "/a//b"),
            ("/./x", "/x"),
            ("/a/../b", "/b"),
        ] {
            assert_eq!(to("to", u), format!("{}{}", base, want), "redirect({:?}) under {:?}", u, base);
            assert_eq!(to("safe", u), format!("{}{}", base, want), "a checked redirect({:?}) under {:?}", u, base);
        }
        // The guide's recipe: each of them is no address of the site's, so `/`.
        for u in ["/.//evil.com", "/%2e//evil.com", "/a/..//evil.com", "/.///evil.com", "/./\\evil.com", "/./%2F/evil.com", "/./%5c/evil.com", "//evil.com"] {
            assert_eq!(to("go", u), format!("{}/", base), "redirect(local_path({:?})) under {:?}", u, base);
        }
        assert_eq!(to("go", "/a/.//b"), format!("{}/a//b", base));
        // What a program wrote with a host is its own: `//evil.com` as given, the base not before it.
        assert_eq!(to("to", "//evil.com"), "//evil.com");
    }
}

#[test]
fn local_path_is_null_for_a_path_that_dots_take_off_the_site() {
    let d = dir("local-path", &format!("for n in {} {{ print(json.stringify(local_path(n))) }}\n", PATHS));
    let want = [
        "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null", "null",
        // Paths of the site's own, as they were given.
        "\"/a/../b\"", "\"/./x\"", "\"/a/.//b\"", "\"/..\"", "\"/.\"", "\"/x?next=/.//evil\"", "\"/x#/.//evil\"", "\"/...//x\"", "\"/.a//x\"", "\"/./é\"",
    ];
    assert_eq!(run_script(&d), want.join("\n") + "\n");
    // Control characters past ASCII (C1, U+0080 to U+009F) are refused, as before; a letter past
    // ASCII at the start is a path's own.
    let d = dir("local-path-c1", "print(json.stringify([local_path(\"/a\u{85}b\"), local_path(\"/é\u{9f}\"), local_path(\"/é/./x\"), local_path(\"/éé\"), local_path(\"/%2Fé\")]))\n");
    assert_eq!(run_script(&d), "[null,null,\"/é/./x\",\"/éé\",null]\n");
}

/// What `local_path` is given in the tests of both sides, as a Quillo list.
const PATHS: &str = r#"["/.//evil.example", "/%2e//evil.example", "/%2E%2e//evil.example", "/a/..//evil.example", "/a/b/../..//evil.example", "/.%2e//evil.example", "/.///evil.example", "/./\\evil.example", "/.\t//evil.example", "/./%2F/evil", "/./%5C/evil", "/.\r//evil.example", "/%2E/%2F/x", " /.//evil.example ", "/%2e.//evil.example", "/a/../%2F/x", "/./%2f/evil", "/.//evil.example?x=1", "/.//", "/a/../b", "/./x", "/a/.//b", "/..", "/.", "/x?next=/.//evil", "/x#/.//evil", "/...//x", "/.a//x", "/./é"]"#;

/// What the server's `local_path` says for each of `PATHS`.
const PATHS_SAID: &str = r#"[null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,"/a/../b","/./x","/a/.//b","/..","/.","/x?next=/.//evil","/x#/.//evil","/...//x","/.a//x","/./é"]"#;

// The browser's `local_path` says what the server's says: a component that runs it on a click.
#[test]
fn in_chrome_local_path_says_what_the_servers_says() {
    let d = dir(
        "local-path-chrome",
        &format!(
            "component Paths() {{\n    state out = \"\"\n    <div><button #go onclick={{() => {{ out = json.stringify(map({}, (n) => local_path(n))) }}}}>go</button><p #out>{{out}}</p></div>\n}}\nroute GET / = <html><head><title>p</title></head><body><Paths/></body></html>\n",
            PATHS
        ),
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  const b = document.getElementById("go");
  b.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); b.click(); await tick(300);
  document.body.dataset.result = JSON.stringify({ out: document.getElementById("out").textContent });
})();"#;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), script);
    let said: serde_json::Value = serde_json::from_str(r["out"].as_str().unwrap_or_else(|| panic!("{}", r))).unwrap();
    assert_eq!(said, serde_json::from_str::<serde_json::Value>(PATHS_SAID).unwrap(), "{}", r);
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
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let d = support::temp_dir("feedback71-chrome");
    std::fs::write(d.join("t.js"), script).unwrap();
    let out = Command::new("node").arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(d.join("profile")).arg("30000").arg(d.join("t.js")).output().expect("node");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}
