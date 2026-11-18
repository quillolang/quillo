// FEEDBACK59.md: what an answer lets caches keep, and a page under a content policy. An
// answer made from who the visitor is stays `private`, whatever its route said (Q-903, the
// user's choice of 2026-11-16); Quillo's own scripts and its islands' style pass the page's
// policy (Q-904); a page made from a header or the address is the visitor's (Q-905, fixed with
// FEEDBACK55's Q-886, pinned here) (branch cache-and-migrate).

mod support;

use sha2::Digest;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("f59-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

fn check(d: &Path) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

/// A server whose lines go to a file: (guard, port, log).
/// The system picks the port and the log says which: a port picked here could be taken by another
/// process first (a shown bug in the test; branch security).
fn serve_logged(d: &Path, args: &[&str]) -> (support::Guard, u16, PathBuf) {
    let log = d.join("server.log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", "0"])
        .args(args)
        .env("QUILLO_SECRET", "a test secret for the tests, 32 bytes or more")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let port = support::port_in_log(&mut g.0, &log, Duration::from_secs(20));
    (g, port, log)
}

/// The session cookie of a sign-in, as `name=value`.
fn signed_in(port: u16) -> String {
    let a = support::send(port, "POST", "/login", &[("sec-fetch-site", "same-origin")], "");
    assert_eq!(a.status, 200, "{}", a.body);
    let c = a.headers.iter().find(|(k, _)| k == "set-cookie").map(|(_, v)| v.split(';').next().unwrap().to_string()).unwrap();
    c
}

const CACHED: &str = r#"route POST /login { set_session({ user: "ana" }); "ok" }
site_headers("/ruled", { "cache-control": "public, max-age=600" })
route GET /files/kept.pdf {
    let _u = session().user ?? reply 401 "Log in first"
    file("report.pdf", { cache: "1y" })
}
route GET /kept {
    let u = session().user ?? "nobody"
    respond(200, `Hello ${u}`, { "cache-control": "public, max-age=60" })
}
route GET /shared {
    let u = session().user ?? "nobody"
    respond(200, u, { "cache-control": "max-age=60, s-maxage=600, PUBLIC, proxy-revalidate, stale-while-revalidate=30" })
}
route GET /own {
    let u = session().user ?? "nobody"
    respond(200, u, { "cache-control": "private, max-age=5" })
}
route GET /never {
    let u = session().user ?? "nobody"
    respond(200, u, { "cache-control": "no-store" })
}
route GET /asks {
    let u = session().user ?? "nobody"
    respond(200, u, { "cache-control": "no-cache" })
}
route GET /ruled/page {
    let u = session().user ?? "nobody"
    u
}
route GET /ruled/plain = "plain"
route GET /sets {
    set_cookie("seen", "1")
    respond(200, "set", { "cache-control": "public, max-age=60" })
}
route GET /cdn {
    let u = session().user ?? "nobody"
    respond(200, u, { "cache-control": "public, max-age=60", "cdn-cache-control": "max-age=600", "cloudflare-cdn-cache-control": "max-age=600", "surrogate-control": "max-age=600", "x-accel-expires": "600" })
}
route GET /some {
    let u = session().user ?? "nobody"
    respond(200, u, { "cache-control": "public, private=\"set-cookie\", max-age=600" })
}
route GET /silent {
    let u = session().user ?? "nobody"
    respond(200, u, { "cache-control": null })
}
route GET /quoted {
    let u = session().user ?? "nobody"
    respond(200, u, { "cache-control": "public, private=\"set-cookie, x-user\", max-age=600" })
}
route GET /no-time {
    let u = session().user ?? "nobody"
    respond(200, u, { "cache-control": "must-revalidate, stale-while-revalidate=600", "akamai-cache-control": "max-age=600", "edge-control": "cache-maxage=600" })
}
fn me() {
    return session().user ?? "nobody"
}
route GET /helper = respond(200, me(), { "cache-control": "public, max-age=60" })
route GET /n/:name {
    let _u = session().user
    respond(200, name, { "cache-control": "public, max-age=60" })
}
route GET /cdn-everyone = respond(200, "same", { "cache-control": "public, max-age=60", "cdn-cache-control": "max-age=600" })
route GET /everyone = respond(200, "same for all", { "cache-control": "public, max-age=60" })
route GET /file = file("report.pdf", { cache: "1y" })
"#;

// Q-903: a route that read the session, or set a cookie, has its `public` (its own header,
// `file`'s `cache`, a `site_headers` rule) sent as `private`, its time kept for the browser and
// the directives for shared caches dropped; with or without a cookie, as both read it.
#[test]
fn an_answer_made_for_the_visitor_is_private_whatever_its_route_said() {
    let d = dir("private", CACHED);
    std::fs::write(d.join("report.pdf"), "%PDF-1.4 Ana's report").unwrap();
    let (_g, port, log) = serve_logged(&d, &[]);
    let c = signed_in(port);
    let cc = |path: &str, cookie: bool| {
        let h: Vec<(&str, &str)> = if cookie { vec![("cookie", c.as_str())] } else { vec![] };
        let a = support::send(port, "GET", path, &h, "");
        (a.status, a.header("cache-control").unwrap_or("").to_string(), a.header("vary").unwrap_or("").to_string())
    };
    assert_eq!(cc("/files/kept.pdf", true), (200, "private, max-age=31536000, immutable".into(), "Cookie".into()));
    assert_eq!(cc("/files/kept.pdf", false).0, 401);
    assert_eq!(cc("/kept", true), (200, "private, max-age=60".into(), "Cookie".into()));
    assert_eq!(cc("/kept", false).1, "private, max-age=60");
    assert_eq!(cc("/shared", true).1, "private, max-age=60, stale-while-revalidate=30");
    assert_eq!(cc("/ruled/page", true).1, "private, max-age=600");
    // The fields a CDN reads first go; a `private` naming fields is no `private` (found in
    // review); `null` is none, as decided for `null` (FEEDBACK55.md, Q-710).
    let a = support::send(port, "GET", "/cdn", &[("cookie", c.as_str())], "");
    assert_eq!(a.header("cache-control"), Some("private, max-age=60"));
    for h in ["cdn-cache-control", "cloudflare-cdn-cache-control", "surrogate-control", "x-accel-expires"] {
        assert_eq!(a.header(h), None, "{}", h);
    }
    assert_eq!(cc("/some", true).1, "private, max-age=600");
    assert_eq!(cc("/silent", true).1, "");
    assert_eq!(support::send(port, "GET", "/cdn-everyone", &[], "").header("cdn-cache-control"), Some("max-age=600"));
    assert_eq!(cc("/sets", false).1, "private, max-age=60");
    // Already the visitor's, or kept by no one without asking: as written.
    assert_eq!(cc("/own", true).1, "private, max-age=5");
    assert_eq!(cc("/never", true).1, "no-store");
    // Any other `cache-control` takes `private` too: a shared cache may keep one that says no
    // time, by `Expires` or its own reckoning (found in review).
    assert_eq!(cc("/asks", true).1, "private, no-cache");
    assert_eq!(cc("/quoted", true).1, "private, max-age=600");
    let a = support::send(port, "GET", "/no-time", &[("cookie", c.as_str())], "");
    assert_eq!(a.header("cache-control"), Some("private, must-revalidate, stale-while-revalidate=600"));
    assert_eq!((a.header("akamai-cache-control"), a.header("edge-control")), (None, None));
    assert_eq!(cc("/helper", true).1, "private, max-age=60");
    // A path with a line break in it is said on one line.
    assert_eq!(cc("/n/a%0Aquillo:%20forged", true).1, "private, max-age=60");
    // What reads nobody keeps its `public`: the same page for everyone.
    assert_eq!(cc("/everyone", true), (200, "public, max-age=60".into(), "".into()));
    assert_eq!(cc("/file", true).1, "public, max-age=31536000, immutable");
    assert_eq!(cc("/ruled/plain", true).1, "public, max-age=600");
    // Said once for each path, however often it is asked for.
    for _ in 0..3 {
        cc("/kept", true);
    }
    let said = std::fs::read_to_string(&log).unwrap();
    assert_eq!(said.matches("/kept read who the visitor is").count(), 1, "{}", said);
    assert!(said.contains("/kept read who the visitor is: `cache-control: public, max-age=60` sent as `private, max-age=60`"), "{}", said);
    assert!(said.contains("quillo: /n/a\\nquillo: forged read who") && !said.lines().any(|l| l.starts_with("quillo: forged")), "{}", said);
    let _ = std::fs::remove_dir_all(&d);
}

// Q-903: `quillo check` says so where a route that reads the visitor writes a `cache-control` a
// shared cache would keep; not where it says `private`, nor for `file`'s `cache`, which says how
// long and not who, nor in a route that reads no one.
#[test]
fn check_warns_where_a_route_both_reads_the_visitor_and_says_public() {
    let d = dir("check", CACHED);
    let out = check(&d);
    let lines: Vec<&str> = out.lines().filter(|l| l.contains("its `cache-control` lets a shared cache keep the answer")).collect();
    // /kept, /shared, /asks, /sets, /cdn, /some, /quoted, /no-time, /helper (its session read in
    // `me`) and /n; not /own, /never, /silent, /files/kept.pdf, /everyone or /file.
    assert_eq!(lines.len(), 10, "{}", out);
    // Each on its `respond`'s line.
    assert!(lines[0].contains("main.qwe:9:") && lines[1].contains("main.qwe:13:") && lines[3].contains("main.qwe:34:"), "{}", out);
    let _ = std::fs::remove_dir_all(&d);
}

fn b64(data: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// The hash of each inline `<script>` of a page, as a policy names it.
fn script_hashes(page: &str) -> Vec<String> {
    page.split("<script>").skip(1).filter_map(|s| s.split_once("</script>")).map(|(t, _)| format!("'sha256-{}'", b64(&sha2::Sha256::digest(t.as_bytes())))).collect()
}

const COUNTER: &str = r#"component Counter() {
    state n = 0
    <button id="c" @click={() => n = n + 1}>clicked {n}</button>
}
server fn slow(n: int) -> int {
    sleep(200)
    return n * 2
}
component Later() {
    state v = defer(slow(21))
    <p id="later">{v == null ? "waiting" : `got ${v}`}</p>
}
state shared = 5
view Shared() = <i id="g">{shared}</i>
"#;

fn page(route: &str, policy: &str, body: &str) -> String {
    let set = if policy.is_empty() { String::new() } else { format!("site_headers(\"{}\", {{ \"content-security-policy\": \"{}\" }})\n", route, policy) };
    format!("{}{}route GET {} = <html><head><title>p</title></head><body>{}<a href=\"/other\">other</a></body></html>\nroute GET /other = \"other\"\n", COUNTER, set, route, body)
}

/// The policy a page came with, and its body.
fn fetch(port: u16, path: &str) -> (String, String) {
    let a = support::send(port, "GET", path, &[], "");
    assert_eq!(a.status, 200, "{}", a.body);
    (a.header("content-security-policy").unwrap_or("").to_string(), a.body)
}

// Q-904: under the guide's own `default-src 'self'`, the page's policy names a hash for each of
// Quillo's inline scripts (its loader, its links' prefetch, the page's state), in a `script-src`
// made from `default-src`, and the islands' `display:contents` is a `<style>` rule, hashed in a
// `style-src` made the same way. Every hash is of a script the page holds.
#[test]
fn quillos_own_scripts_and_island_style_pass_the_pages_policy() {
    let d = dir("csp", &page("/p", "default-src 'self'", "<Counter/><Shared/>"));
    let (_g, port, _log) = serve_logged(&d, &[]);
    let (policy, body) = fetch(port, "/p");
    let hashes = script_hashes(&body);
    assert_eq!(hashes.len(), 3, "{}", body);
    let script_src = policy.split(';').map(str::trim).find(|d| d.starts_with("script-src ")).unwrap_or_else(|| panic!("{}", policy));
    assert_eq!(script_src, format!("script-src 'self' {}", hashes.join(" ")));
    let rule = "quillo-island{display:contents}";
    assert!(body.contains(&format!("<style>{}</style></head>", rule)), "{}", body);
    assert!(policy.contains(&format!("style-src 'self' 'sha256-{}'", b64(&sha2::Sha256::digest(rule.as_bytes())))), "{}", policy);
    assert!(policy.starts_with("default-src 'self'; "), "{}", policy);
    assert!(body.contains("<quillo-island data-c=\"Counter\"") && !body.contains("style=\"display:contents\""), "{}", body);
    let _ = std::fs::remove_dir_all(&d);
}

// Q-904, edges, one policy each: where the policy decides (`script-src-elem` over `script-src`
// over `default-src`), a nonce beside it (hashes added), `'unsafe-inline'` alone (nothing added,
// as a hash would turn it off), `'strict-dynamic'`, `'none'` (dropped beside a hash), several
// policies in one header, a reporting one, styles allowed while scripts are not, and no policy
// at all (the page as it always was).
#[test]
fn hashes_go_where_each_policy_decides_and_nowhere_else() {
    let cases = [
        ("script-src 'self'", "script-src 'self' #", false),
        ("script-src 'self' 'nonce-abc'", "script-src 'self' 'nonce-abc' #", false),
        ("script-src-elem 'self'; script-src 'none'", "script-src-elem 'self' #; script-src 'none'", false),
        ("default-src 'none'; img-src *", "default-src 'none'; img-src *; script-src #; style-src 's'", true),
        ("script-src 'self' 'unsafe-inline'", "script-src 'self' 'unsafe-inline'", false),
        // `'strict-dynamic'` turns `'unsafe-inline'` off for scripts alone: the program's styles
        // stay allowed, and nothing is added for them (found in review).
        ("default-src 'self' 'unsafe-inline' 'strict-dynamic'", "default-src 'self' 'unsafe-inline' 'strict-dynamic'; script-src 'self' 'unsafe-inline' 'strict-dynamic' #", false),
        ("script-src 'strict-dynamic' 'unsafe-inline' 'nonce-x'", "script-src 'strict-dynamic' 'unsafe-inline' 'nonce-x' #", false),
        ("default-src 'self'; style-src 'self' 'unsafe-inline'", "default-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'self' #", false),
        ("img-src 'self'", "img-src 'self'", false),
    ];
    for (i, (policy, want, styled)) in cases.iter().enumerate() {
        let d = dir(&format!("where{}", i), &page("/p", policy, "<Counter/>"));
        let (_g, port, _log) = serve_logged(&d, &[]);
        let (got, body) = fetch(port, "/p");
        let rule = format!("'sha256-{}'", b64(&sha2::Sha256::digest(b"quillo-island{display:contents}")));
        let want = want.replace('#', &script_hashes(&body).join(" ")).replace("'s'", &rule);
        assert_eq!(got, want, "{}", policy);
        assert_eq!(body.contains("<style>quillo-island{display:contents}</style>"), *styled || policy.contains("default-src 'self'") && !policy.contains("unsafe-inline"), "{}: {}", policy, body);
        let _ = std::fs::remove_dir_all(&d);
    }
    // Two policies in one header, and a reporting one: each gets its hashes, and the one that
    // refuses an island's attribute the rule's.
    let src = page("/p", "", "<Counter/>").replace("route GET /p =", "site_headers(\"/p\", { \"content-security-policy\": \"script-src 'self', default-src 'self'\", \"content-security-policy-report-only\": \"script-src 'self'\" })\nroute GET /p =");
    let d = dir("two", &src);
    let (_g, port, _log) = serve_logged(&d, &[]);
    let a = support::send(port, "GET", "/p", &[], "");
    let h = script_hashes(&a.body).join(" ");
    let rule = format!("'sha256-{}'", b64(&sha2::Sha256::digest(b"quillo-island{display:contents}")));
    assert_eq!(a.header("content-security-policy").unwrap(), format!("script-src 'self' {}, default-src 'self'; script-src 'self' {}; style-src 'self' {}", h, h, rule));
    assert_eq!(a.header("content-security-policy-report-only").unwrap(), format!("script-src 'self' {}", h));
    drop(_g);
    let _ = std::fs::remove_dir_all(&d);
    // No policy: nothing added, the islands' attribute as before.
    let d = dir("none", &page("/p", "", "<Counter/>"));
    let (_g, port, _log) = serve_logged(&d, &[]);
    let (got, body) = fetch(port, "/p");
    assert_eq!(got, "");
    assert!(body.contains("<quillo-island style=\"display:contents\" data-c=\"Counter\"") && !body.contains("<style>"), "{}", body);
    drop(_g);
    let _ = std::fs::remove_dir_all(&d);
}

// Q-904: a page with waiting components streams parts whose swap lines are hashed in the head
// sent before them, and whose islands come without the attribute; a page with links only
// carries the prefetch script, hashed; and under --watch the development script is too.
#[test]
fn streamed_parts_links_and_the_development_script_are_covered() {
    let d = dir("parts", &page("/p", "default-src 'self'", "<Counter/><Later/>"));
    let (_g, port, _log) = serve_logged(&d, &[]);
    let (policy, body) = fetch(port, "/p");
    assert!(body.contains("got 42") && body.contains("swap(document.currentScript"), "{}", body);
    for h in script_hashes(&body) {
        assert!(policy.contains(&h), "{} not in {}: {}", h, policy, body);
    }
    assert!(!body.contains("style=\"display:contents\""), "{}", body);
    // A prefetch works its parts out before it answers: their scripts are hashed too (found in
    // review).
    let a = support::send(port, "GET", "/p", &[("sec-purpose", "prefetch")], "");
    assert_eq!(a.status, 200, "{}", a.body);
    assert!(a.body.contains("got 42") && a.body.contains("swap(document.currentScript") && !a.body.contains("swap(\"0\")"), "{}", a.body);
    let policy = a.header("content-security-policy").unwrap();
    for h in script_hashes(&a.body) {
        assert!(policy.contains(&h), "{} not in {}: {}", h, policy, a.body);
    }
    drop(_g);
    let _ = std::fs::remove_dir_all(&d);
    let d = dir("links", &page("/p", "default-src 'self'", ""));
    let (_g, port, _log) = serve_logged(&d, &[]);
    let (policy, body) = fetch(port, "/p");
    let hashes = script_hashes(&body);
    assert_eq!(hashes.len(), 1, "{}", body);
    assert!(policy.ends_with(&format!("; script-src 'self' {}", hashes[0])), "{}", policy);
    // Its links are fetched: no speculation rules, which no policy naming a hash lets through.
    assert!(body.contains("const spec=false;") && !body.contains("supports(\"speculationrules\")"), "{}", body);
    drop(_g);
    let _ = std::fs::remove_dir_all(&d);
    let d = dir("watch", &page("/p", "default-src 'self'", "<Counter/>"));
    let (_g, port, _log) = serve_logged(&d, &["--watch"]);
    let (policy, body) = fetch(port, "/p");
    assert!(body.contains("/*quillo-dev*/"), "{}", body);
    let hashes = script_hashes(&body);
    assert!(hashes.len() >= 3, "{}", body);
    for h in hashes {
        assert!(policy.contains(&h), "{} not in {}", h, policy);
    }
    // And the style of its box for a save that does not load.
    let style = body.split("<style>:host").nth(1).and_then(|r| r.split_once("</style>")).map(|(t, _)| format!(":host{}", t)).unwrap();
    assert!(policy.contains(&format!("'sha256-{}'", b64(&sha2::Sha256::digest(style.as_bytes())))), "{}", policy);
    drop(_g);
    let _ = std::fs::remove_dir_all(&d);
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

fn in_chrome(chrome: &Path, url: &str, script: &str) -> serde_json::Value {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let d = support::temp_dir(&format!("f59-chrome-{}", url.len()));
    std::fs::write(d.join("t.js"), script).unwrap();
    let out = Command::new("node").arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(d.join("profile")).arg("30000").arg(d.join("t.js")).output().expect("node");
    let _ = std::fs::remove_dir_all(&d);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

const CLICK: &str = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const $ = (id) => document.getElementById(id);
  const refused = [];
  document.addEventListener("securitypolicyviolation", (e) => refused.push(e.violatedDirective + " " + (e.sample || "").slice(0, 30)));
  for (let i = 0; i < 100 && !/got/.test(($("later") || {}).textContent || ""); i++) await tick(30);
  $("c").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  $("c").click();
  for (let i = 0; i < 200 && $("c").textContent !== "clicked 1"; i++) await tick(30);
  const link = document.querySelector("a[href='/other']");
  link.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  link.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
  for (let i = 0; i < 30 && !performance.getEntriesByType("resource").some((r) => r.name.endsWith("/other")); i++) await tick(30);
  await tick(100);
  const island = document.querySelector("quillo-island");
  document.body.dataset.result = JSON.stringify({
    button: $("c").textContent,
    later: $("later").textContent,
    shared: $("g").textContent,
    display: getComputedStyle(island).display,
    booted: !!(window[Symbol.for("quillo")] || {}).booted,
    rules: !!document.querySelector("script[type=speculationrules]"),
    fetched: performance.getEntriesByType("resource").some((r) => r.name.endsWith("/other")),
    refused,
    errors: window.__quilloErrors,
  });
})();"#;

// Q-904 in Chrome: under `default-src 'self'` a component's click runs, its island is
// `display: contents`, a waiting component's part is swapped in, and nothing is refused after
// the page loaded; in development (each file its own, the development script) too.
#[test]
fn components_run_in_chrome_under_the_guides_policy() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    for args in [&[][..], &["--watch"][..]] {
        let d = dir(&format!("chrome{}", args.len()), &page("/p", "default-src 'self'", "<Counter/><Later/><Shared/>"));
        let (_g, port, log) = serve_logged(&d, args);
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/p", port), CLICK);
        assert_eq!(
            r,
            serde_json::json!({ "button": "clicked 1", "later": "got 42", "shared": "5", "display": "contents", "booted": true, "rules": false, "fetched": true, "refused": [], "errors": [] }),
            "{:?}: {}",
            args,
            std::fs::read_to_string(&log).unwrap_or_default()
        );
        drop(_g);
        let _ = std::fs::remove_dir_all(&d);
    }
}

// Q-905, pinned: a page made from a request header or the visitor's address is theirs, as
// FEEDBACK55's Q-886 made it; one made from the path and the query alone is not.
#[test]
fn a_page_made_from_a_header_or_the_address_is_the_visitors() {
    let d = dir(
        "marked",
        "route GET /agent = `agent ${request.headers[\"user-agent\"]}`\nroute GET /ip = `ip ${request.ip}`\nroute GET /lang = `lang ${get(request.headers, \"accept-language\")}`\nroute GET /path = `path ${request.path} ${request.query}`\n",
    );
    let (_g, port, _log) = serve_logged(&d, &[]);
    for path in ["/agent", "/ip", "/lang"] {
        let a = support::send(port, "GET", path, &[("user-agent", "agent-A"), ("accept-language", "fr")], "");
        assert_eq!((a.status, a.header("cache-control"), a.header("vary")), (200, Some("private"), Some("Cookie")), "{}", path);
    }
    let a = support::send(port, "GET", "/path?x=1", &[], "");
    assert_eq!((a.status, a.header("cache-control")), (200, None), "{}", a.body);
    let _ = std::fs::remove_dir_all(&d);
}

// Q-904, found in the second review: many waiting components share one hashed line, so the policy
// does not grow with them; a streamed page under --watch has its development script hashed; a page
// with no head or body keeps its doctype first; a player is loaded by a hashed script, which a
// policy with `'strict-dynamic'` takes; a visitor's page prefetched by fetch is kept for its click.
#[test]
fn many_parts_one_line_streams_in_development_and_odd_pages() {
    let many = format!("{}route GET /m = <html><head></head><body>{}</body></html>\n", COUNTER, "<Later/>".repeat(12));
    let d = dir("many", &format!("site_headers(\"/m\", {{ \"content-security-policy\": \"default-src 'self'\" }})\n{}", many));
    let (_g, port, _log) = serve_logged(&d, &[]);
    let (policy, body) = fetch(port, "/m");
    assert_eq!(body.matches("got 42").count(), 12, "{}", body);
    // A line for each part would be 12 hashes more, some 660 bytes.
    assert!(policy.len() < 600, "{} bytes: {}", policy.len(), policy);
    for h in script_hashes(&body) {
        assert!(policy.contains(&h), "{} not in {}", h, policy);
    }
    // Prefetched, its parts worked out first: the same one line.
    let a = support::send(port, "GET", "/m", &[("sec-purpose", "prefetch")], "");
    let policy = a.header("content-security-policy").unwrap();
    assert!(policy.len() < 600 && !a.body.contains("swap(\"3\")"), "{}", policy);
    for h in script_hashes(&a.body) {
        assert!(policy.contains(&h), "{} not in {}", h, policy);
    }
    drop(_g);
    let _ = std::fs::remove_dir_all(&d);
    // Streamed under --watch.
    let d = dir("stream-dev", &page("/p", "default-src 'self'", "<Counter/><Later/>"));
    let (_g, port, _log) = serve_logged(&d, &["--watch"]);
    let (policy, body) = fetch(port, "/p");
    assert!(body.contains("/*quillo-dev*/") && body.contains("got 42"), "{}", body);
    for h in script_hashes(&body) {
        assert!(policy.contains(&h), "{} not in {}", h, policy);
    }
    let style = body.split("<style>:host").nth(1).and_then(|r| r.split_once("</style>")).map(|(t, _)| format!(":host{}", t)).unwrap();
    assert!(policy.contains(&format!("'sha256-{}'", b64(&sha2::Sha256::digest(style.as_bytes())))), "{}", policy);
    drop(_g);
    let _ = std::fs::remove_dir_all(&d);
    // No head, no body: the rule after `<html>`, the doctype first.
    let d = dir("nohead", &format!("{}site_headers(\"/x\", {{ \"content-security-policy\": \"default-src 'self'\" }})\nroute GET /x = <html><main><Counter/></main></html>\n", COUNTER));
    let (_g, port, _log) = serve_logged(&d, &[]);
    let (_, body) = fetch(port, "/x");
    assert!(body.starts_with("<!doctype html>"), "{}", body);
    assert!(body.contains("<style>quillo-island{display:contents}</style>"), "{}", body);
    drop(_g);
    let _ = std::fs::remove_dir_all(&d);
    // A player, under `'strict-dynamic'`: loaded by a hashed script, no `<script src>` in the page.
    let d = dir("media", "site_headers(\"/v\", { \"content-security-policy\": \"script-src 'strict-dynamic' 'unsafe-inline'\" })\nroute GET /v = <html><head></head><body><quillo-media stream=\"cam\" controls muted/></body></html>\n");
    let (_g, port, _log) = serve_logged(&d, &[]);
    let (policy, body) = fetch(port, "/v");
    assert!(body.contains("/_quillo/media.js") && !body.contains("<script src=") && body.contains("s.async=false"), "{}", body);
    for h in script_hashes(&body) {
        assert!(policy.contains(&h), "{} not in {}", h, policy);
    }
    drop(_g);
    let _ = std::fs::remove_dir_all(&d);
    // A visitor's page prefetched by fetch: kept 30 s by that browser, as a page for everyone is.
    let d = dir("prefetch-me", "route POST /login { set_session({ user: \"ana\" }); \"ok\" }\nroute GET /me = `hi ${session().user ?? \"nobody\"}`\n");
    let (_g, port, _log) = serve_logged(&d, &[]);
    let c = signed_in(port);
    let a = support::send(port, "GET", "/me", &[("cookie", c.as_str()), ("x-quillo-prefetch", "1")], "");
    assert_eq!((a.header("cache-control"), a.header("vary")), (Some("private, max-age=30"), Some("Cookie")), "{:?}", a.headers);
    let a = support::send(port, "GET", "/me", &[("cookie", c.as_str())], "");
    assert_eq!(a.header("cache-control"), Some("private"));
    drop(_g);
    let _ = std::fs::remove_dir_all(&d);
}

// Lines 0.8.6's coverage run did not reach: with --metrics, a page with links and no component
// carries the landing script, hashed into the policy; a page with a <body> and no head, and a
// fragment, get the island rule where the browser starts the page; and past 16 different scripts
// on a thread (a page state's values differ per page) each page's own hash is still the one named.
#[test]
fn metrics_pages_without_a_head_and_many_different_scripts() {
    let src = "state n = 0\ncomponent Counter() {\n    state k = 0\n    <button id=\"c\" @click={() => k = k + 1}>{k} {n}</button>\n}\nsite_headers(\"/\", { \"content-security-policy\": \"default-src 'self'\" })\nroute GET /p ? { m: int = 0 } {\n    n = m\n    return <html><head><title>p</title></head><body><Counter/></body></html>\n}\nroute GET /bodyonly = <html><body><Counter/></body></html>\nroute GET /frag = <div><Counter/></div>\nroute GET /links = <html><head><title>l</title></head><body><a href=\"/p\">p</a></body></html>\nroute GET /plain = <html><head><title>x</title></head><body>hi</body></html>\n";
    let d = dir("metrics", src);
    let (_g, port, _log) = serve_logged(&d, &["--metrics", "--workers", "1"]);
    // No link and no component: the landing script alone.
    let (policy, body) = fetch(port, "/plain");
    let hashes = script_hashes(&body);
    assert_eq!(hashes.len(), 1, "{}", body);
    assert!(policy.contains(&hashes[0]), "{}", policy);
    let (policy, body) = fetch(port, "/links");
    let hashes = script_hashes(&body);
    assert_eq!(hashes.len(), 2, "{}", body);
    for h in &hashes {
        assert!(policy.contains(h), "{} not in {}", h, policy);
    }
    for path in ["/bodyonly", "/frag"] {
        let (policy, body) = fetch(port, path);
        let at = body.find("<style>quillo-island{display:contents}</style>").unwrap_or_else(|| panic!("{}: {}", path, body));
        let island = body.find("<quillo-island").unwrap();
        assert!(at < island, "{}: {}", path, body);
        assert!(!body[..at].contains("<quillo-island") && (body.starts_with("<!doctype") || body.starts_with("<html") || at == 0), "{}: {}", path, body);
        assert!(policy.contains("style-src"), "{}", policy);
    }
    for m in 0..40 {
        let (policy, body) = fetch(port, &format!("/p?m={}", m));
        for h in script_hashes(&body) {
            assert!(policy.contains(&h), "m={}: {} not in {}", m, h, policy);
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

// The same: a session read two helpers deep still makes check warn of a public cache-control.
#[test]
fn check_follows_a_helper_that_calls_another() {
    let d = dir("helpers", "fn inner() {\n    return session().user ?? \"nobody\"\n}\nfn me() {\n    return inner()\n}\nroute GET /two = respond(200, me(), { \"cache-control\": \"public, max-age=60\" })\n");
    let out = check(&d);
    assert!(out.contains("main.qwe:7:") && out.contains("its `cache-control` lets a shared cache keep the answer"), "{}", out);
    let _ = std::fs::remove_dir_all(&d);
}
