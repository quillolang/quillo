//! Every kind of address a `use` can name, each its own test (branch library-hints).
//!
//! All the browser cases share one page in headless Chrome, so the whole table costs
//! one load: each case is a component that calls its library and writes down what it
//! got, and each test reads its own row. Addresses go to a small server here, which
//! answers as a CDN would, or fails as one can. Skipped when no Chrome is found.
//!
//! The tests named `cdn::*` load real libraries from the public CDNs, so they are
//! ignored unless asked for: `cargo test --test urls -- --ignored`.

mod support;

use quillo::check::{check_source, Severity};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
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

fn browser() -> Option<PathBuf> {
    let node = Command::new("node").args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"]).output();
    chrome().filter(|_| node.map(|o| o.status.success()).unwrap_or(false))
}

const MODULE: &str = "export const go = () => \"ran\"; export default () => \"ran default\";\n";

/// What a CDN answers, by the path: a module, or a plain script that sets the global
/// its own folder is named for, and the ways an answer can be wrong.
fn answer(path: &str) -> (String, Vec<(&'static str, String)>, String) {
    let file = path.split(['?', '#']).next().unwrap_or(path);
    let mut headers = vec![("access-control-allow-origin", "*".to_string()), ("cache-control", "public, max-age=3600".to_string())];
    let mut kind = "text/javascript";
    if file.contains("/nocors/") {
        headers.remove(0);
    }
    if file.contains("/wrongtype/") {
        kind = "text/plain";
    }
    if file.contains("/slow/") {
        std::thread::sleep(Duration::from_millis(400));
    }
    headers.push(("content-type", kind.to_string()));
    if file.contains("/redirect/") {
        headers.push(("location", file.replacen("/redirect/", "/", 1)));
        return ("302 Found".into(), headers, String::new());
    }
    if file.contains("/missing/") {
        return ("404 Not Found".into(), headers, "not here".into());
    }
    let body = if file.contains("/syntax/") {
        "export const = ;\n".to_string()
    } else if file.contains("/noexports/") {
        "globalThis.noexports = 1;\n".to_string()
    } else if let Some(rest) = file.split("/classic/").nth(1) {
        format!("globalThis[{:?}] = {{ go: () => \"ran\" }};\n", rest.split('/').next().unwrap_or(""))
    } else {
        MODULE.to_string()
    };
    ("200 OK".into(), headers, body)
}

fn serve_one(mut s: TcpStream, hits: &Mutex<Vec<String>>) {
    let mut req = Vec::new();
    let mut buf = [0u8; 2048];
    while !req.windows(4).any(|w| w == b"\r\n\r\n") {
        match s.read(&mut buf) {
            Ok(n) if n > 0 => req.extend_from_slice(&buf[..n]),
            _ => return,
        }
    }
    let line = String::from_utf8_lossy(&req);
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
    hits.lock().unwrap().push(path.clone());
    let (status, headers, body) = answer(&path);
    let mut out = format!("HTTP/1.1 {}\r\ncontent-length: {}\r\nconnection: close\r\n", status, body.len());
    for (k, v) in headers {
        out.push_str(&format!("{}: {}\r\n", k, v));
    }
    out.push_str("\r\n");
    out.push_str(&body);
    let _ = s.write_all(out.as_bytes());
}

fn listen(l: TcpListener, hits: Arc<Mutex<Vec<String>>>) {
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let hits = hits.clone();
            std::thread::spawn(move || serve_one(s, &hits));
        }
    });
}

/// A proxy in front of a program under a base path: it takes the prefix off what it
/// passes on, as one does in production, and returns the port it listens on.
fn proxy(prefix: &str, to: u16) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let prefix = prefix.to_string();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let prefix = prefix.clone();
            std::thread::spawn(move || {
                let mut req = Vec::new();
                let mut buf = [0u8; 2048];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut buf) {
                        Ok(n) if n > 0 => req.extend_from_slice(&buf[..n]),
                        _ => return,
                    }
                }
                let text = String::from_utf8_lossy(&req).replacen(&format!(" {}/", prefix), " /", 1);
                let head: Vec<&str> = text.split("\r\n").filter(|l| !l.to_ascii_lowercase().starts_with("connection:") && !l.is_empty()).collect();
                let Ok(mut up) = TcpStream::connect(("127.0.0.1", to)) else { return };
                let _ = up.write_all(format!("{}\r\nconnection: close\r\n\r\n", head.join("\r\n")).as_bytes());
                let _ = std::io::copy(&mut up, &mut s);
            });
        }
    });
    port
}

/// A case: what it is called, its `use` lines, what it calls, and what the page must say.
#[derive(Clone)]
struct Case {
    id: String,
    uses: String,
    call: String,
    want: String,
}

/// What one load of the page said.
struct Page {
    rows: HashMap<String, String>,
    links: Vec<String>,
    scripts: Vec<String>,
    hits: Vec<String>,
}

/// Fills in where a case's files are.
fn place(text: &str, id: &str, kind: &str, cdn: u16, v6: u16, closed: u16) -> String {
    let file = if kind == "classic" { "c.js" } else { "m.js" };
    text.replace("{id}", id)
        .replace("{kind}", kind)
        .replace("{file}", file)
        .replace("{cdn}", &format!("127.0.0.1:{}", cdn))
        .replace("{v6}", &format!("[::1]:{}", v6))
        .replace("{host}", &format!("LocalHost:{}", cdn))
        .replace("{closed}", &format!("127.0.0.1:{}", closed))
}

/// Loads a program of all these cases in Chrome, once.
fn load(cases: &[(Case, &str)], args: &[&str], at: &str, wait_ms: u32) -> Page {
    let prefix = at.trim_end_matches('/');
    let chrome = browser().expect("Chrome");
    let dir = support::temp_dir(&format!("urls-{}", std::process::id()));
    let dir = dir.join(format!("{}", cases.len()));
    let hits = Arc::new(Mutex::new(Vec::new()));
    let four = TcpListener::bind("127.0.0.1:0").unwrap();
    let cdn = four.local_addr().unwrap().port();
    listen(four, hits.clone());
    let six = TcpListener::bind("[::1]:0").ok();
    let v6 = six.as_ref().map_or(0, |l| l.local_addr().unwrap().port());
    if let Some(l) = six {
        listen(l, hits.clone());
    }
    let closed = support::free_port();
    let mut uses = String::new();
    let mut components = String::new();
    let mut tags = String::new();
    for (i, (c, kind)) in cases.iter().enumerate() {
        if v6 == 0 && c.uses.contains("{v6}") {
            continue;
        }
        let public = dir.join("public").join(&c.id);
        std::fs::create_dir_all(public.join("sub")).unwrap();
        let classic = format!("globalThis[{:?}] = {{ go: () => \"ran\" }};\n", c.id);
        for (name, text) in [("m.js", MODULE), ("sub/m.js", MODULE), ("a m.js", MODULE), ("m.mjs", MODULE), ("M.MJS", MODULE), ("c.js", classic.as_str()), ("sub/c.js", classic.as_str()), ("a c.js", classic.as_str())] {
            std::fs::write(public.join(name), text).unwrap();
        }
        uses.push_str(&place(&c.uses, &c.id, kind, cdn, v6, closed));
        uses.push('\n');
        let call = place(&c.call, &c.id, kind, cdn, v6, closed);
        components.push_str(&format!("component Case{i}() {{\n    <p class=\"case\" id=\"{}\" @mount={{(el) => {{ el.textContent = str({}) }}}}>waiting</p>\n}}\n", c.id, call));
        tags.push_str(&format!("<Case{i}/>"));
    }
    let program = format!(
        "{uses}route GET /assets/*path = file(\"public/\" + path)\n{components}route GET / = <html><head><title>urls</title></head><body>{tags}</body></html>\n"
    );
    std::fs::write(dir.join("main.qwe"), &program).unwrap();
    let script = dir.join("page.js");
    std::fs::write(
        &script,
        format!(
            r#"(async () => {{
  const rows = () => [...document.querySelectorAll(".case")];
  const end = Date.now() + {wait_ms};
  while (Date.now() < end && rows().some((p) => p.textContent === "waiting")) await new Promise((r) => setTimeout(r, 50));
  const out = {{ links: [...document.querySelectorAll("link")].map((l) => l.outerHTML), scripts: [...document.scripts].map((s) => s.getAttribute("src")).filter(Boolean), rows: {{}} }};
  for (const p of rows()) out.rows[p.id] = p.textContent;
  document.body.setAttribute("data-result", JSON.stringify(out));
}})();"#
        ),
    )
    .unwrap();
    let port = support::free_port();
    let server = support::Guard::new(
        Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(dir.join("main.qwe"))
            .args(["--port", &port.to_string()])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start:\n{}", program);
        std::thread::sleep(Duration::from_millis(20));
    }
    let front = if prefix.is_empty() { port } else { proxy(prefix, port) };
    let profile = dir.join("profile");
    let out = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/browser/run.js"))
        .arg(&chrome)
        .arg(format!("http://127.0.0.1:{}{}", front, at))
        .arg(&profile)
        .arg((wait_ms + 60000).to_string())
        .arg(&script)
        .output()
        .expect("node");
    drop(server);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)));
    let strings = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let rows = v["rows"].as_object().map(|m| m.iter().map(|(k, s)| (k.clone(), s.as_str().unwrap_or("").to_string())).collect()).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    let hits = hits.lock().unwrap().clone();
    Page { rows, links: strings("links"), scripts: strings("scripts"), hits }
}

fn says(page: &Page, id: &str, want: &str) {
    let got = page.rows.get(id).map(String::as_str).unwrap_or("(no row)");
    assert!(got.contains(want), "{}: expected {:?}, got {:?}", id, want, got);
}

/// `use` lines that are refused, with what they say.
fn refused(src: &str, want: &str) {
    let d: Vec<String> = check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| d.message.clone()).collect();
    assert!(d.len() == 1 && d[0].contains(want), "{}: expected {:?}, got {:?}", src, want, d);
}

fn accepted(src: &str) {
    let d: Vec<String> = check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| d.message.clone()).collect();
    assert!(d.is_empty(), "{}: {:?}", src, d);
}

/// Each address, written each way, as a module and as a plain script.
macro_rules! forms {
    ($($name:ident: $url:expr;)*) => {
        fn form_cases() -> Vec<(Case, &'static str)> {
            let mut out = Vec::new();
            $(
                out.push((Case { id: concat!("module_", stringify!($name)).into(), uses: format!("use go as {{id}} with module from {}", $url), call: "{id}()".into(), want: "ran".into() }, "module"));
                out.push((Case { id: concat!("classic_", stringify!($name)).into(), uses: format!("use {{id}} from {}", $url), call: "{id}.go()".into(), want: "ran".into() }, "classic"));
            )*
            out
        }
        mod module_address {
            use super::*;
            $(#[test] fn $name() { row(concat!("module_", stringify!($name))); })*
        }
        mod classic_address {
            use super::*;
            $(#[test] fn $name() { row(concat!("classic_", stringify!($name))); })*
        }
    };
}

forms! {
    root_path_quoted: r#""/assets/{id}/{file}""#;
    root_path_bare: "/assets/{id}/{file}";
    relative_quoted: r#""assets/{id}/{file}""#;
    relative_bare: "assets/{id}/{file}";
    dot_slash_quoted: r#""./assets/{id}/{file}""#;
    dot_slash_bare: "./assets/{id}/{file}";
    nested_folder: r#""/assets/{id}/sub/{file}""#;
    nested_folder_bare: "/assets/{id}/sub/{file}";
    query_quoted: r#""/assets/{id}/{file}?v=2""#;
    query_bare: "/assets/{id}/{file}?v=2";
    two_params_quoted: r#""/assets/{id}/{file}?a=1&b=2""#;
    two_params_bare: "/assets/{id}/{file}?a=1&b=2";
    fragment_quoted: r#""/assets/{id}/{file}#x""#;
    fragment_bare: "/assets/{id}/{file}#x";
    encoded_space: r#""/assets/{id}/a%20{file}""#;
    encoded_space_bare: "/assets/{id}/a%20{file}";
    raw_space: r#""/assets/{id}/a {file}""#;
    http_quoted: r#""http://{cdn}/{kind}/{id}/lib.js""#;
    http_bare: "http://{cdn}/{kind}/{id}/lib.js";
    protocol_relative_quoted: r#""//{cdn}/{kind}/{id}/lib.js""#;
    protocol_relative_bare: "//{cdn}/{kind}/{id}/lib.js";
    no_extension_quoted: r#""http://{cdn}/{kind}/{id}/lib""#;
    no_extension_bare: "http://{cdn}/{kind}/{id}/lib";
    version_quoted: r#""http://{cdn}/{kind}/{id}/pkg@1.2.3/lib.js""#;
    version_bare: "http://{cdn}/{kind}/{id}/pkg@1.2.3/lib.js";
    plus_quoted: r#""http://{cdn}/{kind}/{id}/pkg@1/+esm""#;
    plus_bare: "http://{cdn}/{kind}/{id}/pkg@1/+esm";
    address_query_quoted: r#""http://{cdn}/{kind}/{id}/lib.js?bundle&target=es2022""#;
    address_query_bare: "http://{cdn}/{kind}/{id}/lib.js?bundle&target=es2022";
    address_fragment_quoted: r#""http://{cdn}/{kind}/{id}/lib.js#h""#;
    address_fragment_bare: "http://{cdn}/{kind}/{id}/lib.js#h";
    uppercase_scheme: r#""HTTP://{cdn}/{kind}/{id}/lib.js""#;
    uppercase_scheme_bare: "HTTP://{cdn}/{kind}/{id}/lib.js";
    mixed_case_scheme: r#""Http://{cdn}/{kind}/{id}/lib.js""#;
    mixed_case_host: r#""http://{host}/{kind}/{id}/lib.js""#;
    ipv6_quoted: r#""http://{v6}/{kind}/{id}/lib.js""#;
    ipv6_bare: "http://{v6}/{kind}/{id}/lib.js";
    markup_in_query: r#""http://{cdn}/{kind}/{id}/lib.js?x=</script><b>""#;
    quote_in_query: r#""http://{cdn}/{kind}/{id}/lib.js?x=\"y""#;
    apostrophe_in_query: r#""http://{cdn}/{kind}/{id}/lib.js?x='y""#;
    unicode_path: r#""http://{cdn}/{kind}/{id}/ライブラリ.js""#;
    tilde_and_percent: r#""http://{cdn}/{kind}/{id}/~u/a%2Bb.js""#;
    redirect: r#""http://{cdn}/redirect/{kind}/{id}/lib.js""#;
}

/// One more case of the shared page: its `use`, what it calls and what it must say.
macro_rules! cases {
    ($mod:ident, $fn:ident, $kind:expr; $($name:ident: $uses:expr, $call:expr => $want:expr;)*) => {
        fn $fn() -> Vec<(Case, &'static str)> {
            vec![$((Case { id: concat!(stringify!($mod), "_", stringify!($name)).into(), uses: $uses.into(), call: $call.into(), want: $want.into() }, $kind),)*]
        }
        mod $mod {
            use super::*;
            $(#[test] fn $name() { row(concat!(stringify!($mod), "_", stringify!($name))); })*
        }
    };
}

// A `.mjs` file is a module by its name.
cases! { told_by_name, told_by_name_cases, "module";
    quoted: r#"use go as {id} from "/assets/{id}/m.mjs""#, "{id}()" => "ran";
    bare: "use go as {id} from /assets/{id}/m.mjs", "{id}()" => "ran";
    relative: "use go as {id} from assets/{id}/m.mjs", "{id}()" => "ran";
    query: r#"use go as {id} from "/assets/{id}/m.mjs?v=1""#, "{id}()" => "ran";
    query_bare: "use go as {id} from /assets/{id}/m.mjs?v=1", "{id}()" => "ran";
    fragment: r#"use go as {id} from "/assets/{id}/m.mjs#a""#, "{id}()" => "ran";
    address: r#"use go as {id} from "http://{cdn}/module/{id}/lib.mjs""#, "{id}()" => "ran";
    uppercase: r#"use go as {id} from "/assets/{id}/M.MJS""#, "{id}()" => "ran";
}

// What a module gives, every way of asking for it.
cases! { names, names_cases, "module";
    named: r#"use go as {id} with module from "/assets/{id}/m.js""#, "{id}()" => "ran";
    default_export: r#"use default as {id} with module from "/assets/{id}/m.js""#, "{id}()" => "ran default";
    whole: r#"use * as {id} with module from "/assets/{id}/m.js""#, "{id}.go()" => "ran";
    whole_default: r#"use * as {id} with module from "/assets/{id}/m.js""#, "{id}.default()" => "ran default";
    whole_as_a_value: r#"use * as {id} with module from "/assets/{id}/m.js""#, "{id}.default" => "<fn";
    two_from_one: "use default as {id}, go as {id}_go with module from \"/assets/{id}/m.js\"", "{id}() + {id}_go()" => "ran defaultran";
    two_uses_of_one: "use go as {id} with module from \"/assets/{id}/m.js\"\nuse default as {id}_d with module from \"/assets/{id}/m.js\"", "{id}() + {id}_d()" => "ranran default";
    with_preload: r#"use go as {id} with module, preload from "/assets/{id}/m.js""#, "{id}()" => "ran";
    preload_first: r#"use go as {id} with preload, module from "/assets/{id}/m.js""#, "{id}()" => "ran";
    slow: r#"use go as {id} with module from "http://{cdn}/slow/{id}/lib.js""#, "{id}()" => "ran";
}

// What goes wrong at an address says so, where the name is used, and leaves the
// rest of the page running: every row of this table shares a page with the others.
cases! { module_fails, module_fails_cases, "module";
    nothing_listening: r#"use go as {id} with module from "http://{closed}/lib.js""#, "{id}()" => "did not load";
    not_found: r#"use go as {id} with module from "http://{cdn}/missing/lib.js""#, "{id}()" => "did not load";
    no_cors: r#"use go as {id} with module from "http://{cdn}/nocors/lib.js""#, "{id}()" => "did not load";
    wrong_type: r#"use go as {id} with module from "http://{cdn}/wrongtype/lib.js""#, "{id}()" => "did not load";
    syntax_error: r#"use go as {id} with module from "http://{cdn}/syntax/lib.js""#, "{id}()" => "did not load";
    no_such_export: r#"use nope as {id} with module from "http://{cdn}/module/lib.js""#, "{id}()" => "has no export `nope`";
    no_such_default: r#"use default as {id} with module from "http://{cdn}/noexports/a.js""#, "{id}()" => "exports nothing";
    plain_script: r#"use go as {id} with module from "http://{cdn}/noexports/b.js""#, "{id}()" => "leave out `with module`";
}

cases! { classic_fails, classic_fails_cases, "classic";
    nothing_listening: r#"use {id} from "http://{closed}/lib.js""#, "{id}.go()" => "is not defined on this page";
    not_found: r#"use {id} from "http://{cdn}/missing/{id}.js""#, "{id}.go()" => "is not defined on this page";
    syntax_error: r#"use {id} from "http://{cdn}/syntax/{id}.js""#, "{id}.go()" => "is not defined on this page";
    no_cors_is_no_matter: r#"use {id} from "http://{cdn}/classic/{id}/nocors/lib.js""#, "{id}.go()" => "ran";
}

/// What `with` writes in the page's head.
macro_rules! links {
    ($($name:ident: $uses:expr => $want:expr;)*) => {
        fn link_cases() -> Vec<(Case, &'static str)> {
            vec![$((Case { id: concat!("link_", stringify!($name)).into(), uses: $uses.into(), call: if $uses.contains("with module") { "{id}()" } else { "{id}.go()" }.into(), want: $want.into() }, if $uses.contains("with module") { "module" } else { "classic" }),)*]
        }
        mod link {
            use super::*;
            $(#[test] fn $name() {
                let Some(page) = table() else { return };
                let (cdn, v6) = (page.1, page.2);
                let want = place($want, concat!("link_", stringify!($name)), "", cdn, v6, 0);
                if v6 == 0 && $want.contains("{v6}") { return; }
                assert!(page.0.links.iter().any(|l| *l == want), "expected {} in {:#?}", want, page.0.links);
            })*
        }
    };
}

links! {
    module_preload: r#"use go as {id} with module, preload from "/assets/{id}/m.js""# => r#"<link rel="modulepreload" href="/assets/{id}/m.js">"#;
    module_preload_address: r#"use go as {id} with module, preload from "http://{cdn}/module/{id}/lib.js""# => r#"<link rel="modulepreload" href="http://{cdn}/module/{id}/lib.js">"#;
    module_preload_escaped: r#"use go as {id} with module, preload from "http://{cdn}/module/{id}/lib.js?a=1&b=2""# => r#"<link rel="modulepreload" href="http://{cdn}/module/{id}/lib.js?a=1&amp;b=2">"#;
    module_preconnect: r#"use go as {id} with module, preconnect from "http://{cdn}/module/{id}/lib.js""# => r#"<link rel="preconnect" href="http://{cdn}" crossorigin="">"#;
    module_preconnect_relative: r#"use go as {id} with module, preconnect from "//{cdn}/module/{id}/lib.js""# => r#"<link rel="preconnect" href="//{cdn}" crossorigin="">"#;
    module_preconnect_ipv6: r#"use go as {id} with module, preconnect from "http://{v6}/module/{id}/lib.js""# => r#"<link rel="preconnect" href="http://{v6}" crossorigin="">"#;
    module_preconnect_query_only: r#"use go as {id} with module, preconnect from "http://{cdn}?x=1""# => r#"<link rel="preconnect" href="http://{cdn}" crossorigin="">"#;
    module_preconnect_uppercase: r#"use go as {id} with module, preconnect from "HTTP://{cdn}/module/{id}/lib.js""# => r#"<link rel="preconnect" href="HTTP://{cdn}" crossorigin="">"#;
    classic_preload: r#"use {id} with preload from "http://{cdn}/classic/{id}/lib.js""# => r#"<link rel="preload" as="script" href="http://{cdn}/classic/{id}/lib.js">"#;
    classic_preload_local: r#"use {id} with preload from "/assets/{id}/c.js""# => r#"<link rel="preload" as="script" href="/assets/{id}/c.js">"#;
    classic_preconnect: r#"use {id} with preconnect from "http://{cdn}/classic/{id}/lib.js""# => r#"<link rel="preconnect" href="http://{cdn}">"#;
}

/// The whole table, loaded once: its page, and the ports it was served from.
fn table() -> Option<&'static (Page, u16, u16)> {
    static PAGE: OnceLock<Option<(Page, u16, u16)>> = OnceLock::new();
    PAGE.get_or_init(|| {
        browser()?;
        let mut all = form_cases();
        for more in [told_by_name_cases(), names_cases(), module_fails_cases(), classic_fails_cases(), link_cases()] {
            all.extend(more);
        }
        let mut page = load(&all, &[], "/", 25000);
        let port_of = |want: &str| page.links.iter().find_map(|l| l.split(want).nth(1)?.split(['/', '"']).next()?.parse::<u16>().ok()).unwrap_or(0);
        let (cdn, v6) = (port_of("href=\"http://127.0.0.1:"), port_of("href=\"http://[::1]:"));
        page.hits.sort();
        Some((page, cdn, v6))
    })
    .as_ref()
}

fn row(id: &str) {
    let Some((page, _, v6)) = table() else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let want = [form_cases(), told_by_name_cases(), names_cases(), module_fails_cases(), classic_fails_cases()].concat();
    let (case, _) = want.iter().find(|(c, _)| c.id == id).expect("a case");
    if *v6 == 0 && case.uses.contains("{v6}") {
        return;
    }
    says(page, id, &case.want);
}

/// A module's preload and its import are one request (branch library-hints): the
/// same for a plain script's preload and the `<script>` the page makes.
#[test]
fn a_preload_is_fetched_once() {
    let Some((page, ..)) = table() else { return };
    let count = |id: &str| page.hits.iter().filter(|h| h.contains(id)).count();
    assert_eq!(count("/module/link_module_preload_address/lib.js"), 1, "{:#?}", page.hits);
    assert_eq!(count("/classic/link_classic_preload/lib.js"), 1, "{:#?}", page.hits);
}

/// A module is imported, never a `<script>`.
#[test]
fn a_module_is_no_script_tag() {
    let Some((page, ..)) = table() else { return };
    assert!(page.scripts.iter().any(|s| s.contains("/classic/classic_http_quoted/lib.js")), "{:#?}", page.scripts);
    assert!(!page.scripts.iter().any(|s| s.contains("/module/") || s.contains(".mjs") || s.contains("m.js")), "{:#?}", page.scripts);
}

/// Under a base path, a path on the program's own address goes with it, and an
/// address does not. `--base` is for a proxy that takes the prefix off, so one stands
/// in front of the program here.
mod under_a_base {
    use super::*;

    fn cases() -> Vec<(Case, &'static str)> {
        let m = |id: &str, url: &str| (Case { id: id.into(), uses: format!("use go as {{id}} with module, preload from {}", url), call: "{id}()".into(), want: "ran".into() }, "module");
        vec![
            m("base_root", r#""/assets/{id}/m.js""#),
            m("base_bare", "/assets/{id}/m.js"),
            m("base_relative", "assets/{id}/m.js"),
            m("base_dot_slash", r#""./assets/{id}/m.js""#),
            m("base_query", r#""/assets/{id}/m.js?v=1""#),
            m("base_address", r#""http://{cdn}/module/{id}/lib.js""#),
            (Case { id: "base_mjs".into(), uses: r#"use go as {id} from "/assets/{id}/m.mjs""#.into(), call: "{id}()".into(), want: "ran".into() }, "module"),
            (Case { id: "base_classic".into(), uses: r#"use {id} with preload from "/assets/{id}/c.js""#.into(), call: "{id}.go()".into(), want: "ran".into() }, "classic"),
        ]
    }

    fn page() -> Option<&'static Page> {
        static PAGE: OnceLock<Option<Page>> = OnceLock::new();
        PAGE.get_or_init(|| browser().map(|_| load(&cases(), &["--base", "/shop"], "/shop/", 15000))).as_ref()
    }

    #[test]
    fn every_path_goes_with_the_base() {
        let Some(page) = page() else { return };
        for (c, _) in cases() {
            says(page, &c.id, &c.want);
        }
    }

    #[test]
    fn its_preloads_go_with_the_base() {
        let Some(page) = page() else { return };
        assert!(page.links.iter().any(|l| l == r#"<link rel="modulepreload" href="/shop/assets/base_root/m.js">"#), "{:#?}", page.links);
        assert!(page.links.iter().any(|l| l == r#"<link rel="preload" as="script" href="/shop/assets/base_classic/c.js">"#), "{:#?}", page.links);
    }
}

/// What a `use` cannot name, and what it says.
mod refused {
    use super::*;

    macro_rules! refused_cases {
        ($($name:ident: $src:expr => $want:expr;)*) => { $(#[test] fn $name() { refused($src, $want); })* };
    }

    refused_cases! {
        ftp_module: r#"use go with module from "ftp://cdn.example/lib.js""# => "not a `ftp:` address";
        ftp_script: r#"use go from "ftp://cdn.example/lib.js""# => "not a `ftp:` address";
        ftp_bare: "use go from ftp://cdn.example/lib.js" => "not a `ftp:` address";
        file_module: r#"use go with module from "file:///tmp/lib.js""# => "not a `file:` address";
        file_script: r#"use go from "file:///tmp/lib.js""# => "not a `file:` address";
        data_module: r#"use go with module from "data:text/javascript,export const go=1""# => "not a `data:` address";
        data_script: r#"use go from "data:text/javascript,x""# => "not a `data:` address";
        blob_module: r#"use go with module from "blob:http://x/y""# => "not a `blob:` address";
        blob_script: r#"use go from "blob:http://x/y""# => "not a `blob:` address";
        javascript_scheme: r#"use go from "javascript:alert(1)""# => "not a `javascript:` address";
        ws_scheme: r#"use go from "wss://cdn.example/lib.js""# => "not a `wss:` address";
        uppercase_ftp: r#"use go from "FTP://cdn.example/lib.js""# => "not a `FTP:` address";
        empty_module: r#"use go with module from """# => "`use` expects a file";
        empty_script: r#"use go from """# => "`use` expects a file";
        spaces_only: r#"use go from "   ""# => "`use` expects a file";
        user_and_password: r#"use go with module from "https://u:p@cdn.example/lib.js""# => "has a user name in it";
        user_only: r#"use go from "https://u@cdn.example/lib.js""# => "has a user name in it";
        user_bare: "use go from https://u:p@cdn.example/lib.js" => "has a user name in it";
        user_protocol_relative: r#"use go from "//u:p@cdn.example/lib.js""# => "has a user name in it";
        preconnect_root_path: "use go with preconnect from /assets/lib.js" => "`preconnect` is for a script at another address";
        preconnect_relative: "use go with preconnect from assets/lib.js" => "`preconnect` is for a script at another address";
        preconnect_module_path: "use go with module, preconnect from /assets/lib.js" => "`preconnect` is for a script at another address";
        preconnect_mjs_path: "use go with preconnect from /assets/lib.mjs" => "`preconnect` is for a script at another address";
        whole_without_module: r#"use * as THREE from "https://cdn.example/three.js""# => "write client module";
        whole_of_a_qwe: "use * as Auth from lib/auth.qwe" => "takes a whole ES module, not `lib/auth.qwe`";
        default_unnamed: r#"use default with module from "https://cdn.example/lib.js""# => "default export needs a name of yours";
        default_unnamed_mjs: "use default from /assets/lib.mjs" => "default export needs a name of yours";
        module_of_a_plugin: "use go with module from plugin.qwer" => "are for a browser library";
        preload_of_a_plugin: "use go with preload from plugin.qwer" => "are for a browser library";
        unknown_option: r#"use go with prefetch from "https://cdn.example/lib.js""# => "not `prefetch`";
        nothing_after_with: r#"use go with from "https://cdn.example/lib.js""# => "not `from`";
        both_hints: r#"use go with preload, preconnect from "https://cdn.example/lib.js""# => "not both";
        no_names: r#"use "https://cdn.example/lib.js""# => "a script gives the page names of its own";
        every_name_of_a_script: r#"use * from "https://cdn.example/lib.js""# => "a script gives the page names of its own";
    }

    #[test]
    fn a_name_with_a_colon_in_a_path_is_still_a_path() {
        accepted(r#"use go from "/assets/a:b.js""#);
        accepted(r#"use go from "/assets/lib.js?next=http://x.example""#);
        accepted(r#"use go from "https://cdn.example/a:b@1/lib.js""#);
        accepted(r#"use go from "https://cdn.example:8443/lib.js""#);
    }
}

/// Real libraries on the public CDNs, each CDN's own way of serving one. They need
/// the network, so `cargo test --test urls -- --ignored` asks for them.
mod cdn {
    use super::*;

    macro_rules! cdn_cases {
        ($($name:ident: $uses:expr, $call:expr => $want:expr;)*) => {
            fn cases() -> Vec<(Case, &'static str)> {
                vec![$((Case { id: stringify!($name).into(), uses: $uses.into(), call: $call.into(), want: $want.into() }, "module"),)*]
            }
            $(#[test] #[ignore = "needs the public CDNs"] fn $name() { row(stringify!($name)); })*
        };
    }

    cdn_cases! {
        esm_sh_default: r#"use default as {id} with module from "https://esm.sh/dayjs@1.11.13""#, "{id}().year() > 2000" => "true";
        esm_sh_bare: r#"use default as {id} with module from https://esm.sh/dayjs@1.11.13"#, "{id}().year() > 2000" => "true";
        esm_sh_bundle_query: r#"use default as {id} with module from "https://esm.sh/dayjs@1.11.13?bundle""#, "{id}().year() > 2000" => "true";
        esm_sh_bundle_query_bare: r#"use default as {id} with module from https://esm.sh/dayjs@1.11.13?bundle"#, "{id}().year() > 2000" => "true";
        esm_sh_target_query: r#"use default as {id} with module from "https://esm.sh/dayjs@1.11.13?target=es2022""#, "{id}().year() > 2000" => "true";
        esm_sh_named: r#"use chunk as {id} with module from "https://esm.sh/lodash-es@4.17.21""#, "{id}([1, 2, 3, 4], 2).len()" => "2";
        esm_sh_preload: r#"use chunk as {id} with module, preload from "https://esm.sh/lodash-es@4.17.21""#, "{id}([1, 2, 3, 4], 2).len()" => "2";
        esm_sh_preconnect: r#"use chunk as {id} with module, preconnect from "https://esm.sh/lodash-es@4.17.21""#, "{id}([1, 2, 3, 4], 2).len()" => "2";
        esm_sh_whole: r#"use * as {id} with module from "https://esm.sh/three@0.170.0""#, "{id}.REVISION" => "170";
        esm_sh_nested_class: r#"use * as {id} with module from "https://esm.sh/three@0.170.0""#, "{id}.Scene.new().type" => "Scene";
        esm_sh_class: r#"use Chart as {id} with module from "https://esm.sh/chart.js@4.4.1""#, "{id}.version" => "4.4.1";
        esm_sh_no_such_export: r#"use nope as {id} with module from "https://esm.sh/dayjs@1.11.13""#, "{id}()" => "has no export `nope`";
        esm_sh_no_such_package: r#"use x as {id} with module from "https://esm.sh/this-package-does-not-exist-zz9@1.0.0""#, "{id}()" => "did not load";
        jsdelivr_esm: r#"use default as {id} with module from "https://cdn.jsdelivr.net/npm/dayjs@1.11.13/+esm""#, "{id}().year() > 2000" => "true";
        jsdelivr_esm_bare: r#"use default as {id} with module from https://cdn.jsdelivr.net/npm/dayjs@1.11.13/+esm"#, "{id}().year() > 2000" => "true";
        jsdelivr_esm_preload: r#"use default as {id} with module, preload from "https://cdn.jsdelivr.net/npm/dayjs@1.11.13/+esm""#, "{id}().year() > 2000" => "true";
        jsdelivr_esm_class: r#"use Chart as {id} with module from "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/+esm""#, "{id}.version" => "4.4.1";
        jsdelivr_module_file: r#"use * as {id} with module from "https://cdn.jsdelivr.net/npm/three@0.170.0/build/three.module.js""#, "{id}.REVISION" => "170";
        jsdelivr_protocol_relative: r#"use default as {id} with module from "//cdn.jsdelivr.net/npm/dayjs@1.11.13/+esm""#, "{id}().year() > 2000" => "true";
        jsdelivr_plain_http: r#"use default as {id} with module from "http://cdn.jsdelivr.net/npm/dayjs@1.11.13/+esm""#, "{id}().year() > 2000" => "true";
        esm_run: r#"use default as {id} with module from "https://esm.run/dayjs@1.11.13""#, "{id}().year() > 2000" => "true";
        unpkg_mjs: r#"use default as {id} from "https://unpkg.com/canvas-confetti@1.9.3/dist/confetti.module.mjs""#, "{id} != null" => "true";
        unpkg_module_query: r#"use chunk as {id} with module from "https://unpkg.com/lodash-es@4.17.21/lodash.js?module""#, "{id}([1, 2], 1).len()" => "2";
        unpkg_file: r#"use chunk as {id} with module from "https://unpkg.com/lodash-es@4.17.21/lodash.js""#, "{id}([1, 2], 1).len()" => "2";
        unpkg_redirect: r#"use chunk as {id} with module from "https://unpkg.com/lodash-es@4/lodash.js""#, "{id}([1, 2], 1).len()" => "2";
        skypack: r#"use default as {id} with module from "https://cdn.skypack.dev/dayjs@1.11.13""#, "{id}().year() > 2000" => "true";
        skypack_not_found: r#"use default as {id} with module from "https://cdn.skypack.dev/dayjs@1.11.13?min""#, "{id}()" => "did not load";
        cdnjs_module: r#"use * as {id} with module from "https://cdnjs.cloudflare.com/ajax/libs/three.js/0.170.0/three.module.min.js""#, "{id}.REVISION" => "170";
        global_build_as_module: r#"use Chart as {id} with module from "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js""#, "{id}.version" => "with module";
        classic_jsdelivr: r#"use Chart as {id} from "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js""#, "{id}.version" => "4.4.1";
        classic_jsdelivr_bare: r#"use Chart as {id} from https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js"#, "{id}.version" => "4.4.1";
        classic_preload: r#"use Chart as {id} with preload from "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js""#, "{id}.version" => "4.4.1";
        classic_preconnect: r#"use Chart as {id} with preconnect from "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js""#, "{id}.version" => "4.4.1";
        classic_unpkg: r#"use dayjs as {id} from "https://unpkg.com/dayjs@1.11.13/dayjs.min.js""#, "{id}().year() > 2000" => "true";
        classic_cdnjs: r#"use L as {id} from "https://cdnjs.cloudflare.com/ajax/libs/leaflet/1.9.4/leaflet.min.js""#, "{id}.version" => "1.9.4";
        classic_protocol_relative: r#"use L as {id} from "//cdnjs.cloudflare.com/ajax/libs/leaflet/1.9.4/leaflet.min.js""#, "{id}.version" => "1.9.4";
    }

    fn page() -> Option<&'static Page> {
        static PAGE: OnceLock<Option<Page>> = OnceLock::new();
        PAGE.get_or_init(|| browser().map(|_| load(&cases(), &[], "/", 45000))).as_ref()
    }

    fn row(id: &str) {
        let Some(page) = page() else {
            eprintln!("skipping: needs Chrome and Node 22 or later");
            return;
        };
        let (case, _) = cases().into_iter().find(|(c, _)| c.id == id).expect("a case");
        says(page, id, &case.want);
    }
}
