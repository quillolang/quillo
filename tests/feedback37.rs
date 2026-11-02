//! FEEDBACK37.md: Rust beside a program as `.rs`, built together and beside the start, a CSS
//! engine of the program's own, and a file's style for its components (Q-452 to Q-457; branch
//! feedback36-39).

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use support::send;

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback37-{}", name));
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

fn cache() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-cache")
}

fn quillo(d: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quillo")).args(args).current_dir(d).env("QUILLO_NATIVE_CACHE", cache()).stdin(Stdio::null()).output().unwrap()
}

fn out(o: &Output) -> (String, String) {
    (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}

#[test]
#[cfg(unix)]
fn a_server_whose_stderr_has_closed_still_stops_on_a_signal() {
    let d = dir("pipe", &[("main.qwe", "route GET / = \"hi\"\n")]);
    let port = support::free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--port", &port.to_string()])
        .current_dir(&d)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The pipe's other end goes away, as `| head -1` does.
    drop(child.stderr.take());
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(send(port, "GET", "/", &[], "").body, "hi", "a request's log line goes nowhere, and the server goes on");
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success() || status.code().is_some(), "{:?}", status);
            break;
        }
        assert!(Instant::now() < deadline, "Q-452: SIGTERM did not stop it");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn rust_beside_the_program_is_a_rs_file_built_together() {
    let d = dir(
        "rs",
        &[
            ("a.rs", "//! A.\npub fn a() -> String { \"a\".into() }\n"),
            ("b.qwer", "//! B.\npub fn b(n: i64) -> i64 { n * 2 }\n"),
            ("main.qwe", "use \"a.rs\"\nimport b.qwer use b as twice\nprint(a(), twice(21))\n"),
        ],
    );
    let check = quillo(&d, &["check", "main.qwe"]);
    assert!(check.status.success(), "{}", out(&check).1);
    let run = quillo(&d, &["run", "main.qwe"]);
    let (o, e) = out(&run);
    assert_eq!(o, "a 42\n", "Q-453: {}", e);
    assert!(!e.contains("building ./a.rs\n") || e.contains("together") || !e.contains("building"), "built in one call when both were new: {}", e);
    let build = quillo(&d, &["build", "main.qwe"]);
    let (o, _) = out(&build);
    assert!(o.contains("liba-") && o.contains("libb-"), "{}", o);
    // A crate's own source beside the program, used by nothing, is left alone.
    let lone = dir("rs-lone", &[("src/lib.rs", "fn x() {}"), ("main.qwe", "print(1)\n")]);
    assert_eq!(out(&quillo(&lone, &["run", "main.qwe"])).0, "1\n");
}

#[test]
fn files_new_to_the_cache_are_built_in_one_cargo_call() {
    let d = dir(
        "prebuild",
        &[
            ("x.rs", "//! X.\npub fn x() -> i64 { 1 }\n"),
            ("y.rs", "//! Y.\npub fn y() -> i64 { 2 }\n"),
            ("main.qwe", "use \"x.rs\"\nuse \"y.rs\"\nprint(x() + y())\n"),
        ],
    );
    let own = d.join("cache");
    let run = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["run", "main.qwe"]).current_dir(&d).env("QUILLO_NATIVE_CACHE", &own).output().unwrap();
    let (o, e) = out(&run);
    assert_eq!(o, "3\n", "{}", e);
    assert!(e.contains("together"), "Q-454: one cargo call for both: {}", e);
    assert_eq!(e.matches("quillo: building").count(), 1, "{}", e);
    assert!(own.join("workspaces").is_dir());
}

#[test]
fn a_build_runs_once_the_port_is_open_and_a_request_waits_for_it() {
    let d = dir(
        "build",
        &[(
            "main.qwe",
            "fn compile() {\n    sleep(1500)\n    \".p-4{padding:1rem}\"\n}\nbuild \"/app.css\" = compile()\nbuild \"/data.json\" = { ok: true }\nbuild \"/page.html\" = <p>hi</p>\nroute GET / = \"page\"\nlet build = 3\nprint(build)\n",
        )],
    );
    let t = Instant::now();
    let s = support::serve(&d.join("main.qwe"), &[]);
    assert!(t.elapsed() < Duration::from_millis(1400), "the port opened before the build ended: {:?}", t.elapsed());
    assert_eq!(send(s.port, "GET", "/", &[], "").body, "page");
    let css = send(s.port, "GET", "/app.css", &[], "");
    assert_eq!((css.status, css.body.as_str(), css.header("content-type")), (200, ".p-4{padding:1rem}", Some("text/css; charset=utf-8")), "Q-454");
    let etag = css.header("etag").unwrap().to_string();
    assert_eq!(send(s.port, "GET", "/app.css", &[("If-None-Match", &etag)], "").status, 304);
    assert_eq!(send(s.port, "GET", "/data.json", &[], "").body, "{\"ok\":true}");
    assert_eq!(send(s.port, "GET", "/page.html", &[], "").header("content-type"), Some("text/html; charset=utf-8"));
    // A build that fails at the first start stops the program.
    let bad = dir("build-bad", &[("main.qwe", "fn fails() = err(\"no engine\")\nbuild \"/app.css\" = fails()\nroute GET / = 1\n")]);
    let port = support::free_port();
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["run", "main.qwe", "--port", &port.to_string()]).current_dir(&bad).stderr(Stdio::piped()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(s) = c.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > deadline {
            let _ = c.kill();
            panic!("a failed build did not stop it");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut e = String::new();
    std::io::Read::read_to_string(c.stderr.as_mut().unwrap(), &mut e).unwrap();
    assert_eq!(status.code(), Some(1));
    assert!(e.contains("build /app.css failed: no engine"), "{}", e);
    // What a build declaration is, and is not.
    let wrong = dir("build-wrong", &[("a.qwe", "build \"app.css\" = 1\n"), ("b.qwe", "build \"/a/\" = 1\n"), ("c.qwe", "build \"/a?x\" = 1\n")]);
    for f in ["a.qwe", "b.qwe", "c.qwe"] {
        assert!(out(&quillo(&wrong, &["check", f])).1.contains("`build` names the path it is served at"), "{}", f);
    }
}

#[test]
fn style_with_takes_a_function() {
    let d = dir(
        "style-fn",
        &[(
            "main.qwe",
            "fn engine(css) { replace(css, \"@apply m-2;\", \"margin: .5rem;\") }\nstyle_with(engine)\ncomponent Card() {\n    style css`.c { @apply m-2; }`\n    <div.c>card</div>\n}\nroute GET / = <html><body><Card/></body></html>\n",
        )],
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let page = send(s.port, "GET", "/", &[], "").body;
    let sheet = page.split("href=\"").find(|p| p.starts_with("/_quillo/s/")).unwrap().split('"').next().unwrap().to_string();
    let css = send(s.port, "GET", &sheet, &[], "").body;
    assert!(css.contains("margin: .5rem;") && !css.contains("@apply"), "Q-455: {}", css);
    // A function that gives no CSS stops the load, as a command that fails does.
    let bad = dir("style-fn-bad", &[("main.qwe", "fn engine(css) = 3\nstyle_with(engine)\ncomponent C() {\n    style css`.c { x: 1 }`\n    <p.c/>\n}\nroute GET / = <C/>\n")]);
    let (_, e) = out(&quillo(&bad, &["run", "main.qwe", "--port", &support::free_port().to_string()]));
    assert!(e.contains("style_with `engine` failed") && e.contains("not CSS text"), "{}", e);
}

#[test]
fn the_classes_a_program_writes() {
    let d = dir(
        "classes",
        &[(
            "main.qwe",
            "let color = \"red\"\ncomponent Card(on) {\n    <div.card.p-4 class={on ? \"ring-2 shadow\" : \"opacity-50\"} class:active={on}>\n        <span class=\"text-sm md:text-lg\">{`bg-${color} keep-me`}</span>\n        <i class={`w-1/3 bg-${color} mid${color}x`}/>\n    </div>\n}\nprint(classes())\n",
        )],
    );
    let want = "[\"active\", \"card\", \"md:text-lg\", \"opacity-50\", \"p-4\", \"ring-2\", \"shadow\", \"text-sm\", \"w-1/3\"]\n";
    assert_eq!(out(&quillo(&d, &["run", "main.qwe"])).0, want, "Q-456");
    let listed = out(&quillo(&d, &["classes", "main.qwe"])).0;
    assert_eq!(listed, "active\ncard\nmd:text-lg\nopacity-50\np-4\nring-2\nshadow\ntext-sm\nw-1/3\n");
    assert!(out(&quillo(&d, &["classes", "nothing.qwe"])).1.contains("cannot read"));
}

#[test]
fn a_files_style_styles_its_components_only() {
    let d = dir(
        "file-style",
        &[
            (
                "lib/cards.qwe",
                "style css`.title { color: darkred } .title:hover { text-decoration: underline }`\nstyle css`.extra { margin: 0 }`\ncomponent Card(t) {\n    style css`.title { color: navy }`\n    <div.card><h2.title>{t}</h2><span.extra/></div>\n}\ncomponent Badge(t) { <span.title>{t}</span> }\nfn helper() { <em class=\"title\">x</em> }\n",
            ),
            ("main.qwe", "import lib/cards.qwe use Card, Badge, helper\nroute GET / = <html><body><h2.title>page</h2><Card t=\"a\"/><Badge t=\"b\"/>{helper()}</body></html>\n"),
            ("nocomp.qwe", "style css`.x{}`\nlet a = 1\n"),
            ("late.qwe", "component A() { <p/> }\nstyle css`.x{}`\n"),
        ],
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let page = send(s.port, "GET", "/", &[], "").body;
    assert!(page.contains("<h2 class=\"title\">page</h2>"), "the page's own is left alone: {}", page);
    assert!(page.contains("<em class=\"title\">x</em>"), "a top-level fn's markup too");
    let badge = page.split("data-c=\"Badge\"").nth(1).unwrap();
    let mark = badge.split("data-q-").nth(1).unwrap()[..8].to_string();
    assert!(page.matches(&format!("data-q-{}", mark)).count() >= 4, "every element of the file's components: {}", page);
    let sheet = page.split("href=\"").find(|p| p.starts_with("/_quillo/s/")).unwrap().split('"').next().unwrap().to_string();
    let css = send(s.port, "GET", &sheet, &[], "").body;
    let file_rule = css.find(&format!(".title[data-q-{}] {{ color: darkred }}", mark)).expect(&css);
    let extra = css.find(&format!(".extra[data-q-{}]", mark)).expect(&css);
    let own = css.find("color: navy").expect(&css);
    assert!(file_rule < extra && extra < own, "the file's, in order, before the component's own: {}", css);
    assert_eq!(css.matches("darkred").count(), 1, "once for the file");
    // The browser makes the same marks.
    let js = send(s.port, "GET", "/_quillo/quillo.js", &[], "").body;
    assert!(js.contains(&format!("data-q-{}", mark)), "Q-457: the bundle marks them too");
    assert!(out(&quillo(&d, &["check", "nocomp.qwe"])).1.contains("a file's `style` styles the components declared in it, and this file has none"));
    assert!(out(&quillo(&d, &["check", "late.qwe"])).1.contains("a file's `style` comes before its components"));
}
