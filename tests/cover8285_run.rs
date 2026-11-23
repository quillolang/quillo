//! Coverage of feedback82-85 (branch feedback82-85): the lines added since main that no test ran in
//! the full run at 7e5a9af, in src/native.rs (a Rust plugin pinned or confirmed by its SHA-256),
//! src/fetch.rs (`save:` and `max_bytes:` on the http calls, FEEDBACK85.md Q-1342), src/head.rs and
//! src/builtins.rs (FEEDBACK84.md's `page_head`, `page_url`, `navigate`), src/server.rs and
//! src/client.rs (a page that moves in place, Q-1323), src/check.rs (Q-965's GET that writes,
//! Q-1348's operator's setting), src/imaging.rs and src/pg.rs (FEEDBACK83.md's hold, Q-1300) and
//! src/httpd.rs (`--trust-proxy`, Q-1280).

mod support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use support::{port_in_log, send, Guard};

/// A test's own folder, its path as the system names it, removed (made writable first) when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = Command::new("chmod").arg("-R").arg("u+rwx").arg(&self.0).status();
        let _ = std::fs::remove_dir_all(&self.0);
        let _ = std::fs::remove_dir_all(record_dir(&self.0));
    }
}

impl Dir {
    fn new(name: &str) -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = support::temp_dir(&format!("cov8285run-{}-{}", name, N.fetch_add(1, Ordering::SeqCst)));
        Dir(d.canonicalize().unwrap())
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, name: &str, text: &str) {
        let p = self.0.join(name);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(p, text).unwrap();
    }
}

/// Where a test's program keeps the record of what its machine built (`native::built_record`):
/// beside its folder, never the user's own cache.
fn record_dir(dir: &Path) -> PathBuf {
    dir.with_file_name(format!("{}-built", dir.file_name().unwrap().to_string_lossy()))
}

/// `quillo` with `args` in `dir`, ended after `secs`, with `vars` set: its exit code and what it
/// wrote (stdout and stderr).
fn quillo_env(dir: &Path, args: &[&str], secs: u64, vars: &[(&str, &str)]) -> (Option<i32>, String) {
    let log = dir.join(format!("cmd-{}.log", std::process::id()));
    let file = std::fs::File::create(&log).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        // `cargo test` gives its tests these; a program's run has them only where a test says so.
        .env_remove("CARGO")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_BUILT_RECORD_DIR", record_dir(dir))
        .env("QUILLO_SECRET", "test secret for the tests, 32 bytes or more")
        .envs(vars.iter().copied())
        .stdin(Stdio::null())
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(secs);
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let out = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    (code, out)
}

fn quillo(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    quillo_env(dir, args, 60, &[])
}

/// `quillo check` of `file` in `dir`, which passes: what it said.
fn checked(dir: &Path, file: &str) -> String {
    let (code, out) = quillo(dir, &["check", file]);
    assert_eq!(code, Some(0), "quillo check {}:\n{}", file, out);
    out
}

/// `quillo run <file> --port 0` in `dir`, with `opts` and `vars`: the guard, the port, the log.
fn serve_env(dir: &Path, file: &str, opts: &[&str], vars: &[(&str, &str)], secs: u64) -> (Guard, u16, PathBuf) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let log = dir.join(format!("run-{}.log", N.fetch_add(1, Ordering::SeqCst)));
    let out = std::fs::File::create(&log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", file, "--port", "0"])
        .args(opts)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("CARGO")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_BUILT_RECORD_DIR", record_dir(dir))
        .env("QUILLO_SECRET", "test secret for the tests, 32 bytes or more")
        .envs(vars.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(out)
        .spawn()
        .unwrap();
    let mut g = Guard::new(child);
    let port = port_in_log(&mut g.0, &log, Duration::from_secs(secs));
    (g, port, log)
}

fn serve(dir: &Path, file: &str, opts: &[&str]) -> (Guard, u16, PathBuf) {
    serve_env(dir, file, opts, &[], 60)
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn get(port: u16, path: &str) -> support::Answer {
    send(port, "GET", path, &[], "")
}

/// The files of `dir` whose names end with `.part`: what a save that stopped would leave.
fn parts_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".part")).collect()).unwrap_or_default()
}

// ---- src/check.rs: Q-965's helper write said once, Q-1348's operator's setting ---------------------

/// A helper's write that five GET routes and four reach is said once, at the write, with the
/// first three routes and how many more; three are named whole (FEEDBACK70.md, Q-965, decided
/// 2026-11-23). An address from `env()` and literals is the operator's setting only so deep: 31
/// methods on it are, 32 are not, and `env()` of a name in a variable is no setting's name
/// (FEEDBACK85.md, Q-1348). (src/check.rs; coverage of feedback82-85; branch feedback82-85)
#[test]
fn check_says_how_many_more_routes_and_how_deep_a_setting_is_read() {
    let d = Dir::new("check");
    let routes = |n: usize| -> String {
        let mut s = String::from("db.open(\":memory:\")\ndb.exec(\"create table hits (n int)\")\nfn bump() {\n    db.exec(\"insert into hits values (1)\")\n}\n");
        for name in ["a", "b", "c", "d", "e"].iter().take(n) {
            s.push_str(&format!("route GET /{} = {{ bump()\n \"{}\" }}\n", name, name));
        }
        s
    };
    let said = |out: &str| out.lines().filter(|l| l.contains("not declared `route writes`")).map(str::to_string).collect::<Vec<_>>();
    d.write("five.qwe", &routes(5));
    let out = checked(d.path(), "five.qwe");
    assert_eq!(
        said(&out),
        ["five.qwe:4: warning: this write (`db.exec(\"insert into hits values (1)\")`, in `bump`) is done by 5 GET routes not declared `route writes`: `GET /a` (line 6), `GET /b` (line 8), `GET /c` (line 10) and 2 more. A hover's prefetch, or a move in place, runs them until that write is refused. Tag them, `route writes GET ...`, `routes writes /...`, or make them `POST`s"],
        "{}",
        out
    );
    d.write("four.qwe", &routes(4));
    let out = checked(d.path(), "four.qwe");
    assert!(said(&out).len() == 1 && said(&out)[0].contains("is done by 4 GET routes not declared `route writes`: `GET /a` (line 6), `GET /b` (line 8), `GET /c` (line 10) and 1 more."), "{}", out);
    d.write("three.qwe", &routes(3));
    let out = checked(d.path(), "three.qwe");
    assert!(said(&out).len() == 1 && said(&out)[0].contains("is done by 3 GET routes not declared `route writes`: `GET /a` (line 6), `GET /b` (line 8) and `GET /c` (line 10)."), "{}", out);

    let src = format!(
        "let NAME = \"TUNO_URL\"\nlet deep = env(\"TUNO_URL\", \"http://127.0.0.1:8940\"){}\nlet edge = env(\"TUNO_URL\", \"http://127.0.0.1:8940\"){}\nroute GET /d = http.get(`${{deep}}/health`).status ?? 0\nroute GET /e = http.get(`${{edge}}/health`).status ?? 0\nroute GET /v = http.get(env(NAME, \"http://127.0.0.1:8940\")).status ?? 0\nroute GET /w = http.get(`${{env(NAME, \"http://127.0.0.1:8940\")}}/x`).status ?? 0\n",
        ".trim()".repeat(32),
        ".trim()".repeat(31)
    );
    d.write("env.qwe", &src);
    let out = checked(d.path(), "env.qwe");
    let visitor = "warning: the host `http.get` goes to comes from a value: if a visitor can choose it, they can reach this server's own network";
    let operator = "warning: the host `http.get` goes to comes from `TUNO_URL`, the operator's setting: say { public_only: false }";
    assert!(out.contains(&format!("env.qwe:4: {}", visitor)), "{}", out);
    assert!(out.contains(&format!("env.qwe:5: {}", operator)), "{}", out);
    assert!(out.contains(&format!("env.qwe:6: {}", visitor)), "{}", out);
    assert!(out.contains(&format!("env.qwe:7: {}", visitor)), "{}", out);
    assert_eq!(out.matches("the operator's setting").count(), 1, "{}", out);
}

// ---- src/builtins.rs: page_url, page_head and navigate where no page is made -------------------

/// Outside a request `page_url` is this machine's address, `http://localhost` and the path, and
/// `page_head` says nothing; `page_head` of no map and `navigate` with options that are no map
/// stop the program, naming what they take (FEEDBACK84.md, Q-1328, Q-1320, Q-1326).
/// (src/builtins.rs page_url, whole_address, page_head, navigate; coverage of feedback82-85;
/// branch feedback82-85)
#[test]
fn page_url_and_page_head_outside_a_page() {
    let d = Dir::new("top");
    d.write("top.qwe", "print(page_url())\nprint(page_url(\"/img/card.png\"))\nprint(page_url(\"/caf\u{e9}/\u{6771}\"))\nprint(page_head({ title: \"t\" }))\n");
    checked(d.path(), "top.qwe");
    let (code, out) = quillo(d.path(), &["run", "top.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    assert_eq!(out, "http://localhost/\nhttp://localhost/img/card.png\nhttp://localhost/caf\u{e9}/\u{6771}\nnull\n");
    for (src, said) in [
        ("print(page_head(json.parse(\"\\\"x\\\"\")))\n", "top.qwe:1: `page_head` takes a map: page_head({ title: \"Home\", description: \"...\" })"),
        ("print(page_head(json.parse(\"[]\")))\n", "top.qwe:1: `page_head` takes a map"),
        ("print(navigate(\"/x\", json.parse(\"3\")))\n", "top.qwe:1: `navigate`'s options are a map, `{ replace: true, scroll: false }`, got 3"),
        ("print(navigate(\"/x\", json.parse(\"[true]\")))\n", "`navigate`'s options are a map, `{ replace: true, scroll: false }`, got [true]"),
    ] {
        d.write("top.qwe", src);
        checked(d.path(), "top.qwe");
        let (code, out) = quillo(d.path(), &["run", "top.qwe"]);
        assert_eq!(code, Some(1), "{}: {}", src, out);
        assert!(out.contains(said), "{}: {}", src, out);
    }
}

// ---- src/head.rs: the page's own tags, however their attributes are written ---------------------

/// The page's own tags of a key `page_head` says go, however their attributes are written:
/// spaces around `=`, a value in single quotes or none, an attribute with no value before the name;
/// a tag whose name only starts with `meta` (`<meta="`), whose value's quote never ends, is no
/// meta and stays (FEEDBACK84.md, Q-1320). Where the tags go inside the `<html>` tag (a page with
/// no head and no body, its `lang` holding a `>`), what `page_head` says is still escaped: no
/// attribute of its own. (src/head.rs attr_of, apply, tag_close; coverage of feedback82-85;
/// branch feedback82-85)
#[test]
fn page_head_replaces_the_pages_tags_however_their_attributes_are_written() {
    let d = Dir::new("head");
    d.write(
        "main.qwe",
        r#"route GET / {
    page_head({ title: "New", description: "Said" })
    return raw("<!doctype html><html><head><meta name = \"description\" content=\"spaced\"><meta property=  'og:title' content=x><meta itemprop name=description content=bare><meta data-x name=\"keep\"><meta=\" a='\"><title>Old</title></head><body>b</body></html>")
}
route GET /attr {
    page_head({ title: "x\" onload=\"alert(1)" })
    return raw("<html lang=\"a>b\"><p>no head, no body</p></html>")
}
"#,
    );
    checked(d.path(), "main.qwe");
    let (_g, port, log) = serve(d.path(), "main.qwe", &[]);
    let a = get(port, "/");
    assert_eq!(a.status, 200, "{}", read(&log));
    // `<meta=" a='">`'s `a` opens a value with `'` that never ends: the browser drops it and all
    // after it (parse5: the head ends at `<meta data-x name="keep">`, the body is empty), so the
    // tags go before it, and the title after it, never read, stays (slice Z's fix: FEEDBACK84.md,
    // Q-1320, found by the coverage tests; branch feedback82-85). They went after it, unseen.
    assert_eq!(
        a.body,
        "<!doctype html><html><head><meta data-x name=\"keep\"><title>New</title><meta property=\"og:title\" content=\"New\"><meta name=\"description\" content=\"Said\"><meta property=\"og:description\" content=\"Said\"><meta=\" a='\"><title>Old</title></head><body>b</body></html>"
    );
    let a = get(port, "/attr");
    assert_eq!(a.status, 200, "{}", read(&log));
    assert!(a.body.contains("x&quot; onload=&quot;alert(1)") && !a.body.contains("onload=\"") && !a.body.contains("<script"), "{}", a.body);
    assert!(a.body.ends_with("<p>no head, no body</p></html>"), "{}", a.body);
}

// ---- src/server.rs: page_head on what is no page, and the key of a sign-out ---------------------

/// A HEAD request and an answer that is no HTML take nothing of `page_head`, and what they said
/// is not left for the next request of the same worker: the next page has its own title
/// (FEEDBACK84.md, Q-1320). (src/server.rs page_head_into; coverage of feedback82-85; branch
/// feedback82-85)
#[test]
fn a_head_request_or_json_takes_no_page_head_and_leaves_none_behind() {
    let d = Dir::new("headreq");
    d.write(
        "main.qwe",
        r#"route GET / {
    page_head({ title: "Said" })
    return raw("<html><head><title>Own</title></head><body>p</body></html>")
}
route GET /plain = raw("<html><head><title>Plain</title></head><body>q</body></html>")
route GET /json {
    page_head({ title: "J" })
    return { a: 1 }
}
"#,
    );
    checked(d.path(), "main.qwe");
    let (_g, port, log) = serve(d.path(), "main.qwe", &["--workers", "1"]);
    let h = send(port, "HEAD", "/", &[], "");
    assert_eq!(h.status, 200, "{}", read(&log));
    assert!(h.body.is_empty(), "{}", h.body);
    assert!(h.header("content-type").is_some_and(|t| t.starts_with("text/html")), "{:?}", h.headers);
    let p = get(port, "/plain");
    assert_eq!(p.body, "<html><head><title>Plain</title></head><body>q</body></html>");
    let j = get(port, "/json");
    assert_eq!((j.status, j.body.as_str()), (200, "{\"a\":1}"));
    let p = get(port, "/plain");
    assert_eq!(p.body, "<html><head><title>Plain</title></head><body>q</body></html>");
    let s = get(port, "/");
    assert!(s.body.contains("<title>Said</title>") && !s.body.contains("Own"), "{}", s.body);
}

/// The key a page that moves in place carries (`w.ik`), or none.
fn key_of(page: &str) -> Option<String> {
    let at = page.find("w.ik=\"")? + 6;
    Some(page[at..at + page[at..].find('"')?].to_string())
}

/// The page that signs out (its answer takes the session's cookie away) has the key of a visitor
/// with no session, not the signed-in one's: the next page moves in place from it (FEEDBACK84.md,
/// Q-1323, review of slice H). (src/server.rs in_place_key; coverage of feedback82-85; branch
/// feedback82-85)
#[test]
fn the_page_that_signs_out_has_the_key_of_no_one() {
    let d = Dir::new("signout");
    d.write(
        "main.qwe",
        r#"moves_in_place()
route GET / = <html><head><title>A</title></head><body>a</body></html>
route GET /in {
    set_session({ user: "ann" })
    return <html><head><title>In</title></head><body>in</body></html>
}
route GET /out {
    end_session()
    return <html><head><title>Out</title></head><body>out</body></html>
}
"#,
    );
    checked(d.path(), "main.qwe");
    let (_g, port, log) = serve(d.path(), "main.qwe", &[]);
    let no_one = key_of(&get(port, "/").body).expect("a key");
    let signed = get(port, "/in");
    let cookie = signed.headers.iter().find(|(k, _)| k == "set-cookie").map(|(_, v)| v.split(';').next().unwrap().to_string()).expect("a session cookie");
    let with = key_of(&send(port, "GET", "/", &[("cookie", cookie.as_str())], "").body).unwrap();
    assert_ne!(with, no_one);
    let out = send(port, "GET", "/out", &[("cookie", cookie.as_str())], "");
    assert_eq!(out.status, 200, "{}", read(&log));
    let gone = out.headers.iter().find(|(k, _)| k == "set-cookie").map(|(_, v)| v.clone()).unwrap_or_default();
    assert!(gone.contains("quillo_session=;") && gone.contains("Max-Age=0"), "{:?}", out.headers);
    assert_eq!(key_of(&out.body).as_deref(), Some(no_one.as_str()), "{}", out.body);
    // Another cookie the program never reads leaves it as it is.
    let theme = format!("{}; theme=dark", cookie);
    assert_eq!(key_of(&send(port, "GET", "/out", &[("cookie", theme.as_str())], "").body).as_deref(), Some(no_one.as_str()));
}

/// `quillo mcp --serve` takes no `--watch`, so a program it loads is loaded or the door does not
/// start: a load that failed gives no interpreter to answer its tools, one that loaded does
/// (FEEDBACK85.md, Q-1347). (src/server.rs fork_loaded; coverage of feedback82-85; branch
/// feedback82-85)
#[test]
fn a_failed_load_gives_no_interpreter_for_the_door() {
    let d = Dir::new("fork");
    d.write("main.qwe", "route GET / = \"x\"\n");
    let (code, out) = quillo(d.path(), &["mcp", "main.qwe", "--serve", "--watch"]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("`--watch` is not for `quillo mcp`"), "{}", out);
    let it = quillo::interp::Interp::new(d.path());
    let shared = it.shared.clone();
    let failed = quillo::server::App::new(quillo::server::Loaded::Failed("boom".into()), shared.clone(), &d.path().join("main.qwe"), d.path(), true);
    assert!(failed.fork_loaded().is_none());
    let ready = quillo::server::App::new(quillo::server::Loaded::Ready(it), shared, &d.path().join("main.qwe"), d.path(), true);
    assert!(ready.fork_loaded().is_some());
}

// ---- src/client.rs: the development bundle of a program that moves in place --------------------

/// The script of `path` the development bundle's loader (`/_quillo/quillo.js` under --watch) names.
fn dev_file(port: u16, path: &str) -> String {
    let loader = get(port, "/_quillo/quillo.js").body;
    let at = loader.find(&format!("/{}\"", path)).unwrap_or_else(|| panic!("no {} in:\n{}", path, loader));
    let from = loader[..at].rfind('"').unwrap() + 1;
    let file = &loader[from..at + 1 + path.len()];
    get(port, &format!("/_quillo/src/{}", file)).body
}

/// Under --watch, a program that moves in place has its development bundle's boot turn that on,
/// as its built bundle does; one that does not, none (FEEDBACK84.md, Q-1323). (src/client.rs
/// bundle; coverage of feedback82-85; branch feedback82-85)
#[test]
fn the_development_bundle_moves_in_place_when_the_program_does() {
    let d = Dir::new("devbundle");
    let comp = "component C() {\n    state n = 0\n    <button onclick={() => { n = n + 1 }}>{n}</button>\n}\nroute GET / = <html><head><title>A</title></head><body><C/></body></html>\n";
    d.write("moves.qwe", &format!("moves_in_place()\n{}", comp));
    d.write("stays.qwe", comp);
    checked(d.path(), "moves.qwe");
    checked(d.path(), "stays.qwe");
    let (g, port, log) = serve(d.path(), "moves.qwe", &["--watch"]);
    assert_eq!(get(port, "/").status, 200, "{}", read(&log));
    let boot = dev_file(port, "boot.js");
    assert!(boot.contains("\nW.inPlace();\n") && boot.contains("boot"), "{}", boot);
    drop(g);
    let (_g, port, log) = serve(d.path(), "stays.qwe", &["--watch"]);
    assert_eq!(get(port, "/").status, 200, "{}", read(&log));
    let boot = dev_file(port, "boot.js");
    assert!(!boot.contains("W.inPlace") && boot.contains("boot"), "{}", boot);
}

// ---- src/httpd.rs: --trust-proxy's list -----------------------------------------------------------

/// `trust_proxies` takes a list of addresses and networks once: a list with a part that is
/// neither is refused and sets nothing, a good one is taken, and a second is not (FEEDBACK82.md,
/// Q-1280). (src/httpd.rs trust_proxies; coverage of feedback82-85; branch feedback82-85)
#[test]
fn trust_proxies_takes_one_good_list_once() {
    assert!(!quillo::httpd::trust_proxies("10.0.0.0/8,not an address"));
    assert!(!quillo::httpd::trust_proxies(""));
    assert!(quillo::httpd::trust_proxies("10.0.0.0/8,127.0.0.1,::1"));
    assert!(!quillo::httpd::trust_proxies("192.168.0.0/16"));
}

// ---- src/fetch.rs: save's edges ---------------------------------------------------------------------

/// A stand-in for another server, for `secs`: `/short` says 1000 bytes and sends 10 before it
/// closes, `/big404` is a 404 of 2 MiB, anything else `hello`.
fn stand_in() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut c = c;
                c.set_read_timeout(Some(Duration::from_secs(5))).ok();
                let mut buf = [0u8; 4096];
                let n = c.read(&mut buf).unwrap_or(0);
                let path = String::from_utf8_lossy(&buf[..n]).split_whitespace().nth(1).unwrap_or("/").to_string();
                match path.as_str() {
                    "/short" => {
                        let _ = c.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 1000\r\nconnection: close\r\n\r\nxxxxxxxxxx");
                    }
                    "/big404" => {
                        let n = 2usize << 20;
                        let _ = c.write_all(format!("HTTP/1.1 404 Not Found\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", n).as_bytes());
                        let _ = c.write_all(&vec![b'y'; n]);
                    }
                    "/small404" => {
                        let _ = c.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 7\r\nconnection: close\r\n\r\nmissing");
                    }
                    _ => {
                        let _ = c.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nhello");
                    }
                }
            });
        }
    });
    port
}

/// An answer cut short is an error and writes nothing, no part left; a file's name longer than a
/// part's name keeps, its cut made at a character's start (a 2-byte and a 4-byte one across byte
/// 180); a folder that cannot be written is said; and under --max-memory a long answer that is
/// not saved (a 404) stops the program as any value that long does, one under it is read
/// (FEEDBACK85.md, Q-1342). (src/fetch.rs write_answer, send_saved; coverage of feedback82-85;
/// branch feedback82-85)
#[test]
fn save_cut_short_long_names_a_closed_folder_and_max_memory() {
    let d = Dir::new("save");
    let u = format!("http://127.0.0.1:{}", stand_in());
    std::fs::create_dir_all(d.path().join("cache")).unwrap();
    std::fs::create_dir_all(d.path().join("ro")).unwrap();
    let two = format!("{}\u{e9}.bin", "a".repeat(179));
    let four = format!("{}\u{1F600}.bin", "b".repeat(178));
    let src = format!(
        "print(http.get(\"{u}/short\", {{}}, {{ save: \"cache/s.bin\", public_only: false }}))\nprint(http.get(\"{u}/ok\", {{}}, {{ save: \"cache/{two}\", public_only: false }}).bytes)\nprint(http.get(\"{u}/ok\", {{}}, {{ save: \"cache/{four}\", public_only: false }}).file)\nprint(http.get(\"{u}/ok\", {{}}, {{ save: \"ro/a.bin\", public_only: false }}))\n"
    );
    d.write("t.qwe", &src);
    checked(d.path(), "t.qwe");
    // A folder its owner cannot write (not so for root, who writes anywhere).
    let ro = d.path().join("ro");
    let _ = Command::new("chmod").arg("555").arg(&ro).status();
    let closed = std::fs::write(ro.join("probe"), "x").is_err();
    let (code, out) = quillo(d.path(), &["run", "t.qwe"]);
    let _ = Command::new("chmod").arg("755").arg(&ro).status();
    assert_eq!(code, Some(0), "{}", out);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with("err(\"http: ") && lines[0].contains("closed before all bytes were read"), "{}", out);
    assert!(!d.path().join("cache/s.bin").exists());
    assert_eq!(lines[1], "5", "{}", out);
    assert_eq!(std::fs::read(d.path().join("cache").join(&two)).unwrap(), b"hello");
    assert_eq!(lines[2], format!("cache/{}", four), "{}", out);
    assert_eq!(std::fs::read(d.path().join("cache").join(&four)).unwrap(), b"hello");
    if closed {
        assert_eq!(lines[3], "err(\"http: save: ro/a.bin: Permission denied (os error 13)\")", "{}", out);
        assert!(!ro.join("a.bin").exists());
    }
    assert!(parts_in(&d.path().join("cache")).is_empty() && parts_in(&ro).is_empty(), "{:?}", parts_in(&d.path().join("cache")));

    d.write("m.qwe", &format!("let r = http.get(\"{u}/big404\", {{}}, {{ save: \"cache/m.bin\", public_only: false }})\nprint([r.status, r.file, len(r.body)])\n"));
    checked(d.path(), "m.qwe");
    let (code, out) = quillo(d.path(), &["run", "m.qwe", "--max-memory", "1MB"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("m.qwe:1: this request's value would be bigger than --max-memory allows (1048576 bytes)"), "{}", out);
    let (code, out) = quillo(d.path(), &["run", "m.qwe"]);
    assert_eq!((code, out.as_str()), (Some(0), format!("[404, null, {}]\n", 2 << 20).as_str()));
    d.write("s.qwe", &format!("let r = http.get(\"{u}/small404\", {{}}, {{ save: \"cache/m.bin\", public_only: false }})\nprint([r.status, r.file, r.body])\n"));
    checked(d.path(), "s.qwe");
    let (code, out) = quillo(d.path(), &["run", "s.qwe", "--max-memory", "1MB"]);
    assert_eq!((code, out.as_str()), (Some(0), "[404, null, \"missing\"]\n"));
    assert!(!d.path().join("cache/m.bin").exists());
}

// ---- src/fetch.rs, src/imaging.rs, src/pg.rs: what a held program reaches ------------------------

/// Held to its `quillo.confirmed`, a save to a path worked out at run time that is not confirmed
/// is refused as the file builtins refuse one, and nothing is written; so is `image.convert`'s
/// output; an argument of `image.convert` that is no path is not judged as one, and is said as
/// before (FEEDBACK83.md, Q-1300; FEEDBACK85.md, Q-1342). (src/fetch.rs save_to, src/imaging.rs
/// unconfirmed, convert; coverage of feedback82-85; branch feedback82-85)
#[test]
fn a_held_save_or_conversion_writes_only_where_confirmed() {
    let d = Dir::new("held");
    let u = format!("http://127.0.0.1:{}", stand_in());
    std::fs::create_dir_all(d.path().join("data")).unwrap();
    d.write(
        "main.qwe",
        &format!(
            "let other = join([\"other\", \"b.bin\"], \"/\")\nlet out = join([\"other\", \"out.png\"], \"/\")\nprint(http.get(\"{u}/ok\", {{}}, {{ save: \"data/a.bin\", public_only: false }}).bytes)\nprint(http.get(\"{u}/ok\", {{}}, {{ save: other, public_only: false }}))\nprint(image.convert(\"data/in.png\", out))\nprint(image.convert(json.parse(\"5\"), \"data/out.png\"))\n"
        ),
    );
    d.write("quillo.confirmed", "[\"main.qwe\"]\nhttp = [\"127.0.0.1\"]\nfiles = [\"data/\"]\nwrites = [\"data/\"]\n");
    checked(d.path(), "main.qwe");
    let (code, out) = quillo(d.path(), &["run", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    let printed: Vec<&str> = out.lines().filter(|l| !l.starts_with("quillo:")).collect();
    assert_eq!(
        printed,
        ["5", "err(\"http.get: save: other/b.bin is not confirmed for main.qwe (quillo confirm)\")", "err(\"image: other/out.png is not confirmed for main.qwe (quillo confirm)\")"],
        "{}",
        out
    );
    assert!(out.contains("quillo: main.qwe: the path other/b.bin was refused: it is not confirmed for this file"), "{}", out);
    assert!(out.contains("quillo: main.qwe:6: `image.convert` expects a path, got int"), "{}", out);
    assert_eq!(read(&d.path().join("data/a.bin")), "hello");
    assert!(!d.path().join("other").exists());
}

/// A PostgreSQL address with no host is held as `localhost`, where libpq would go: refused where
/// it is not confirmed, and where it is, the driver's own refusal of an address with no host
/// (FEEDBACK83.md, Q-1300, review 3). (src/pg.rs hosts; coverage of feedback82-85; branch
/// feedback82-85)
#[test]
fn a_database_with_no_host_is_held_as_localhost() {
    assert_eq!(quillo::confirm::db_hosts("postgres:///app").unwrap(), vec!["localhost"]);
    assert_eq!(quillo::confirm::db_hosts("postgresql://u@/app?sslmode=disable").unwrap(), vec!["localhost"]);
    assert_eq!(quillo::confirm::db_hosts("postgres:///app?hostaddr=10.0.0.1").unwrap(), vec!["10.0.0.1"]);
    let d = Dir::new("pg");
    d.write("db.qwe", "let d = db.open(\"postgres:///app?sslmode=disable\")\nprint(d)\n");
    d.write("quillo.confirmed", "[\"db.qwe\"]\nhttp = [\"db.example.invalid\"]\n");
    let (code, out) = quillo(d.path(), &["check", "db.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("db.qwe:1: error: reaches localhost: not confirmed for this file"), "{}", out);
    let (code, out) = quillo(d.path(), &["run", "db.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("quillo: db.qwe: localhost was refused: it is not confirmed for this file") && out.contains("err(\"db: localhost is not confirmed for db.qwe (quillo confirm)\")"), "{}", out);
    d.write("quillo.confirmed", "[\"db.qwe\"]\nhttp = [\"localhost\"]\n");
    checked(d.path(), "db.qwe");
    let (code, out) = quillo(d.path(), &["run", "db.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("err(\"db: invalid configuration: both host and hostaddr are missing\")"), "{}", out);
}

// ---- src/native.rs: Rust plugins pinned or confirmed by their SHA-256 ------------------------------

/// The SHA-256 of `bytes`, in hexadecimal.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{:02x}", b)).collect()
}

/// A program in `dir` that uses the plugin `hi.qwer` of `src`, and `pin.toml`, a policy that holds
/// it to its source's SHA-256 (with `more` after it).
fn pinned_program(dir: &Path, src: &str, more: &str) {
    std::fs::write(dir.join("hi.qwer"), src).unwrap();
    std::fs::write(dir.join("main.qwe"), "use \"hi.qwer\"\nprint(hi())\n").unwrap();
    std::fs::write(dir.join("pin.toml"), format!("[plugins]\nallow = [\"hi@sha256:{}\"]\n{}", sha256_hex(src.as_bytes()), more)).unwrap();
}

const HI: &str = "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { \"hi from the pinned source\".into() }\n";

/// The folders a pinned build made in `tmp` and left there.
fn pinned_dirs(tmp: &Path) -> Vec<String> {
    std::fs::read_dir(tmp).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("quillo-pinned-")).collect()).unwrap_or_default()
}

/// A pinned source that does not compile says cargo's errors, without its `Compiling` lines, and
/// leaves no private folder behind in the temporary folder (FEEDBACK82.md, Q-1285). (src/native.rs
/// build_private; coverage of feedback82-85; branch feedback82-85)
#[test]
fn a_pinned_source_that_does_not_compile_says_why_and_leaves_nothing() {
    let d = Dir::new("pin-broken");
    let app = d.path().join("app");
    let tmp = d.path().join("tmp");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    // The same program, compiling, passes quillo check; this one is run unchecked, to build it.
    pinned_program(&app, HI, "");
    checked(&app, "main.qwe");
    pinned_program(&app, "//! version: 1.0.0\n//! effects: none\npub fn hi() -> String { 1 }\n", "");
    let t = tmp.display().to_string();
    let (code, out) = quillo_env(&app, &["run", "main.qwe", "--no-check", "--policy", "pin.toml"], 600, &[("TMPDIR", &t)]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("building ./hi.qwer (held to its SHA-256: from its source, apart)"), "{}", out);
    assert!(out.contains("cannot build ./hi.qwer:\n") && out.contains("error[E0308]: mismatched types"), "{}", out);
    assert!(!out.contains("Compiling ") && !out.contains("hi from the pinned source"), "{}", out);
    assert!(pinned_dirs(&tmp).is_empty(), "left in the temporary folder: {:?}", pinned_dirs(&tmp));
    assert!(!app.join(".quillo/native").exists(), "built where the program is");
}

/// A pinned build is never made in a temporary folder the program can reach: inside a folder the
/// policy's `[fs] paths` names (or one reached through a link into it), inside the program's
/// folder, or inside one `--allow-path` names; it is refused, naming both (FEEDBACK82.md,
/// Q-1285, found in review). (src/native.rs private_dir; coverage of feedback82-85; branch
/// feedback82-85)
#[cfg(unix)]
#[test]
fn a_pinned_build_is_refused_a_temporary_folder_the_program_reaches() {
    let d = Dir::new("pin-reach");
    let app = d.path().join("app");
    let shared = d.path().join("shared");
    let other = d.path().join("other");
    for f in [app.join("tmp"), shared.join("tmp"), other.join("tmp")] {
        std::fs::create_dir_all(f).unwrap();
    }
    std::os::unix::fs::symlink(shared.join("tmp"), d.path().join("via")).unwrap();
    pinned_program(&app, HI, &format!("[fs]\npaths = [\"{}\", \"{}\"]\n", shared.display(), other.display()));
    let refused = |tmp: &Path, extra: &[&str], inside: &Path| {
        let t = tmp.display().to_string();
        let mut args = vec!["run", "main.qwe", "--no-check", "--policy", "pin.toml"];
        args.extend_from_slice(extra);
        let (code, out) = quillo_env(&app, &args, 120, &[("TMPDIR", &t)]);
        assert_eq!(code, Some(1), "{}: {}", t, out);
        let real = tmp.canonicalize().unwrap();
        assert!(
            out.contains(&format!("a pinned plugin is built apart, and the temporary folder {} is inside {}, which the program can reach", real.display(), inside.display())),
            "{}: {}",
            t,
            out
        );
        assert!(!out.contains("building ") && pinned_dirs(&real).is_empty(), "{}", out);
    };
    refused(&shared.join("tmp"), &[], &shared);
    // Named another way, through a link: judged where it is.
    refused(&d.path().join("via"), &[], &shared);
    refused(&app.join("tmp"), &[], &app);
    let o = other.display().to_string();
    refused(&other.join("tmp"), &["--allow-path", &o], &other);
}

/// A pinned plugin a second instance loads is the library already built and loaded, not built
/// again: both instances answer with it, and it was built once (FEEDBACK82.md, Q-1285).
/// (src/native.rs build_private; coverage of feedback82-85; branch feedback82-85)
#[test]
fn a_pinned_plugin_loaded_again_is_not_built_again() {
    let d = Dir::new("pin-again");
    let app = d.path().join("app");
    let tmp = d.path().join("tmp");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    pinned_program(&app, HI, "");
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nroute GET / = hi()\n").unwrap();
    checked(&app, "main.qwe");
    let t = tmp.display().to_string();
    let (_g, port, log) = serve_env(&app, "main.qwe", &["--no-check", "--policy", "pin.toml", "--instances", "2"], &[("TMPDIR", &t)], 600);
    for _ in 0..6 {
        let a = get(port, "/");
        assert_eq!((a.status, a.body.as_str()), (200, "hi from the pinned source"), "{}", read(&log));
    }
    assert_eq!(read(&log).matches("held to its SHA-256: from its source, apart").count(), 1, "{}", read(&log));
    assert!(pinned_dirs(&tmp).is_empty(), "{:?}", pinned_dirs(&tmp));
}

/// A confirmed Rust file is built apart and loads where its library cannot be kept in
/// `.quillo/native`, or the record of what was built cannot be written; then the next start with
/// building off has no library this machine is known to have built, and is refused, saying which
/// (FEEDBACK83.md, Q-1300, found in review). (src/native.rs build_confirmed, record_put; coverage
/// of feedback82-85; branch feedback82-85)
#[test]
fn a_confirmed_plugin_that_cannot_be_kept_or_recorded_loads_and_is_built_again() {
    let d = Dir::new("confirmed-kept");
    d.write("hi.qwer", "//! version: 1.0.0\n//! effects: none\npub fn hi(n: &str) -> String { format!(\"hi {}\", n) }\n");
    d.write("main.qwe", "import hi.qwer use hi\nroute GET / = hi(\"Ana\")\n");
    let (code, out) = quillo(d.path(), &["confirm", "--yes"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(read(&d.path().join("quillo.confirmed")).contains(&format!("sha256 = \"{}\"", sha256_hex(read(&d.path().join("hi.qwer")).as_bytes()))));
    checked(d.path(), "main.qwe");
    let record = record_dir(d.path());
    let native = d.path().join(".quillo/native");
    let libs = || std::fs::read_dir(&native).map(|r| r.flatten().filter(|e| e.file_name().to_string_lossy().contains("hi")).count()).unwrap_or(0);
    // The record's folder, this user's alone, that no one may write: nothing is recorded.
    std::fs::create_dir_all(&record).unwrap();
    let _ = Command::new("chmod").arg("500").arg(&record).status();
    let closed = std::fs::write(record.join("probe"), "x").is_err();
    let (g, port, log) = serve_env(d.path(), "main.qwe", &[], &[], 600);
    assert_eq!(get(port, "/").body, "hi Ana", "{}", read(&log));
    drop(g);
    let _ = Command::new("chmod").arg("700").arg(&record).status();
    assert_eq!(libs(), 1, "kept in .quillo/native: {}", read(&log));
    if closed {
        assert!(!record.join("built.txt").exists());
        assert!(std::fs::read_dir(&record).unwrap().next().is_none(), "a part of the record left behind");
        let (code, out) = quillo(d.path(), &["run", "main.qwe", "--no-build", "--port", "0"]);
        assert_eq!(code, Some(1), "{}", out);
        assert!(out.contains("is confirmed by its SHA-256, so only a library this machine built from it loads, and building is off") && out.contains("the one in .quillo/native was not built here"), "{}", out);
    }
    // `.quillo/native` no one may write: built apart, loaded, not kept, not recorded.
    for e in std::fs::read_dir(&native).unwrap().flatten() {
        let _ = std::fs::remove_file(e.path());
    }
    let _ = Command::new("chmod").arg("555").arg(&native).status();
    let closed = std::fs::write(native.join("probe"), "x").is_err();
    let (g, port, log) = serve_env(d.path(), "main.qwe", &[], &[], 600);
    assert_eq!(get(port, "/").body, "hi Ana", "{}", read(&log));
    drop(g);
    let _ = Command::new("chmod").arg("755").arg(&native).status();
    if closed {
        assert_eq!(libs(), 0, "{}", read(&log));
        assert!(!read(&record.join("built.txt")).contains(&sha256_hex(read(&d.path().join("hi.qwer")).as_bytes())), "recorded what was not kept");
        let (code, out) = quillo(d.path(), &["run", "main.qwe", "--no-build", "--port", "0"]);
        assert_eq!(code, Some(1), "{}", out);
        assert!(out.contains("building is off") && out.contains("none is built"), "{}", out);
    }
}

/// A plugin function whose own `/// effects:` line names an effect its file's line does not, or
/// that says its effects twice, does not load as the program runs unchecked, the error naming the
/// file and line (FEEDBACK85.md, Q-1344). (src/native.rs use_file; coverage of feedback82-85;
/// branch feedback82-85)
#[test]
fn a_plugin_function_wider_than_its_file_does_not_load() {
    let d = Dir::new("wide");
    let cache = d.path().join("cache").display().to_string();
    d.write("hi.qwer", "//! version: 1.0.0\n//! effects: none\n/// Says hi.\n/// effects: fs\npub fn hi() -> String { \"hi\".into() }\n");
    d.write("main.qwe", "use \"hi.qwer\"\nprint(hi())\n");
    let (code, out) = quillo(d.path(), &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("names fs, which the file's `//! effects: none` does not"), "{}", out);
    let (code, out) = quillo_env(d.path(), &["run", "main.qwe", "--no-check"], 600, &[("QUILLO_NATIVE_CACHE", &cache)]);
    assert_eq!(code, Some(1), "{}", out);
    // Said once, at the plugin's line, with the `use` it was taken by: it was `./hi.qwer:1:
    // hi.qwer:4:` (found by the coverage tests; the expected text changed with that fix).
    assert!(out.contains("./hi.qwer:4: `hi`'s `/// effects: fs` names fs, which the file's `//! effects: none` does not: a function's line narrows its file's, never widens it (the `use` at main.qwe:1)"), "{}", out);
    assert!(!out.contains("\nhi\n"), "{}", out);
    d.write("hi.qwer", "//! version: 1.0.0\n//! effects: fs, net\n/// effects: fs\n/// effects: net\npub fn hi() -> String { \"hi\".into() }\n");
    let (code, out) = quillo_env(d.path(), &["run", "main.qwe", "--no-check"], 600, &[("QUILLO_NATIVE_CACHE", &cache)]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("hi.qwer:4: `hi` says its effects twice: one `/// effects:` line"), "{}", out);
}

/// Under --watch, a pinned plugin refused once it was built (a function's effects line wider than
/// its file's) leaves the page saying why, and a reload builds it again, as nothing of it was
/// loaded, and says why again (FEEDBACK82.md, Q-1285; FEEDBACK85.md, Q-1344). (src/native.rs
/// build_private; coverage of feedback82-85; branch feedback82-85)
#[test]
fn a_pinned_plugin_refused_at_its_load_is_built_again_at_a_reload() {
    let d = Dir::new("pin-reload");
    let app = d.path().join("app");
    let tmp = d.path().join("tmp");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    pinned_program(&app, "//! version: 1.0.0\n//! effects: none\n/// effects: fs\npub fn hi() -> String { \"hi\".into() }\n", "");
    std::fs::write(app.join("main.qwe"), "use \"hi.qwer\"\nroute GET / = hi()\n").unwrap();
    let t = tmp.display().to_string();
    let said = "hi.qwer:3: `hi`'s `/// effects: fs` names fs, which the file's `//! effects: none` does not";
    // A first load that fails under --watch says no port: the test names one, as support's
    // `serve_with` does for a port given.
    let port = support::free_port();
    let log = app.join("watch.log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--port", &port.to_string(), "--no-check", "--policy", "pin.toml", "--watch"])
        .current_dir(&app)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("CARGO")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_SECRET", "test secret for the tests, 32 bytes or more")
        .env("TMPDIR", &t)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let mut g = Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(600);
    while !support::listening(port) {
        assert!(g.0.try_wait().unwrap().is_none(), "quillo stopped: {}", read(&log));
        assert!(Instant::now() < deadline, "not listening: {}", read(&log));
        std::thread::sleep(Duration::from_millis(50));
    }
    let a = get(port, "/");
    assert_eq!(a.status, 500, "{}", read(&log));
    assert!(a.body.contains("The program did not load") && a.body.contains("hi.qwer:3: `hi`&#39;s `/// effects: fs` names fs"), "{}", a.body);
    // Saved once the watcher is there, and again until a reload is seen (a save in the same
    // second as the last may look unchanged).
    while !read(&log).contains("quillo: watching for changes") {
        assert!(Instant::now() < deadline, "not watching: {}", read(&log));
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut saves = 0;
    let mut saved = Instant::now() - Duration::from_secs(10);
    while read(&log).matches(said).count() < 2 {
        assert!(Instant::now() < deadline, "not loaded again: {}", read(&log));
        if saved.elapsed() > Duration::from_secs(3) {
            saves += 1;
            std::fs::write(app.join("main.qwe"), format!("use \"hi.qwer\"\nroute GET / = hi()\n// saved again ({})\n", saves)).unwrap();
            saved = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(read(&log).matches("building ./hi.qwer (held to its SHA-256: from its source, apart)").count() >= 2, "{}", read(&log));
    assert_eq!(get(port, "/").status, 500, "{}", read(&log));
    drop(g);
}

// ---- src/head.rs, src/fetch.rs, src/check.rs: what the coverage tests found (slice Z) -----------

/// A head that says `pairs`, each at depth 0 in no component, in their order.
fn said_head(pairs: &[(&str, &str)]) -> quillo::head::Head {
    let mut h = quillo::head::Head::default();
    for (k, v) in pairs {
        h.say(quillo::head::KEYS.iter().position(|x| x == k).unwrap(), 0, 0, v.to_string());
        h.called();
    }
    h
}

/// `page` with `h`'s tags in its head.
fn with_head(page: &str, h: &quillo::head::Head) -> String {
    let mut b = page.as_bytes().to_vec();
    quillo::head::apply(&mut b, h);
    String::from_utf8(b).unwrap()
}

/// What `page_head({ title: "T" })` writes.
const TT: &str = "<title>T</title><meta property=\"og:title\" content=\"T\">";

/// The repro: a page with no head and no body whose `<html>` tag's `lang` holds a `>` takes
/// `page_head`'s tags after that tag, not inside it (which lost the title and showed `b">` as
/// text); so does one after a comment that says `<html>` and `>`, and one whose doctype comes
/// after a comment that says one (src/head.rs apply, marks, tag_close, FEEDBACK84.md Q-1320, found
/// by the coverage tests; branch feedback82-85)
#[test]
fn page_heads_tags_go_after_an_html_tag_whose_value_holds_a_gt() {
    let d = Dir::new("headgt");
    d.write(
        "main.qwe",
        r#"route GET / {
    page_head({ title: "T" })
    return raw("<html lang=\"a>b\"><p>x</p></html>")
}
route GET /c {
    page_head({ title: "T" })
    return raw("<!-- <html> > --><html lang='a>b'><p>x</p></html>")
}
route GET /d {
    page_head({ title: "T" })
    return raw("<!-- <!doctype x> --><!doctype html><p>x</p>")
}
route GET /u {
    page_head({ title: "Café · 東京" })
    return raw("<html lang=\"日本>語\"><p>x</p></html>")
}
"#,
    );
    checked(d.path(), "main.qwe");
    let (_g, port, log) = serve(d.path(), "main.qwe", &[]);
    for (path, want) in [
        ("/", format!("<html lang=\"a>b\">{TT}<p>x</p></html>")),
        ("/c", format!("<!-- <html> > --><html lang='a>b'>{TT}<p>x</p></html>")),
        ("/d", format!("<!-- <!doctype x> --><!doctype html>{TT}<p>x</p>")),
        ("/u", "<html lang=\"日本>語\"><title>Café · 東京</title><meta property=\"og:title\" content=\"Café · 東京\"><p>x</p></html>".to_string()),
    ] {
        let a = get(port, path);
        assert_eq!((a.status, a.body.as_str()), (200, want.as_str()), "{} {}", path, read(&log));
    }
}

/// Each tag `page_head`'s tags go after ends where the browser ends it (checked against parse5, the
/// HTML standard's tokenizer): a quote opens a value only after `=` (and spaces), single or
/// double; an unquoted value, a value or a name that starts with `=`, a quote in a name, end at
/// their first `>`; any case. A comment ends at its first `-->` or `--!>`, `<!-->` and `<!--->`
/// whole, `<!---!>` not (one short); a doctype and a `<?...>` at their first `>`, quoted or not.
/// What a comment or a script says is no tag. A tag, comment or doctype that never ends, or a
/// page that is no document, is left as it is (src/head.rs apply, marks, token, comment_end,
/// tag_close, FEEDBACK84.md Q-1320, found by the coverage tests; branch feedback82-85)
#[test]
fn the_tag_page_head_goes_after_ends_where_the_browser_ends_it() {
    let h = said_head(&[("title", "T")]);
    let after = |tag: &str, rest: &str| format!("{tag}{TT}{rest}");
    for (page, want) in [
        ("<html lang=\"a>b\"><p>x</p>", after("<html lang=\"a>b\">", "<p>x</p>")),
        ("<html lang='a>b'><p>x</p>", after("<html lang='a>b'>", "<p>x</p>")),
        ("<HTML LANG = \"a>b\" ><p>x</p>", after("<HTML LANG = \"a>b\" >", "<p>x</p>")),
        ("<html\nlang=\n'a>b'><p>x</p>", after("<html\nlang=\n'a>b'>", "<p>x</p>")),
        ("<html/ lang=\"a>b\"><p>x</p>", after("<html/ lang=\"a>b\">", "<p>x</p>")),
        ("<html lang=a>b><p>x</p>", after("<html lang=a>", "b><p>x</p>")),
        ("<html lang=a\"b>c\"><p>x</p>", after("<html lang=a\"b>", "c\"><p>x</p>")),
        ("<html a==\">\"><p>x</p>", after("<html a==\">", "\"><p>x</p>")),
        ("<html =\">\"><p>x</p>", after("<html =\">", "\"><p>x</p>")),
        ("<html b'c><p>x</p>", after("<html b'c>", "<p>x</p>")),
        ("<html a=\"\"><p>x</p>", after("<html a=\"\">", "<p>x</p>")),
        ("<html a=><p>x</p>", after("<html a=>", "<p>x</p>")),
        ("<!-- > --><html lang=\"x>y\"><p>x</p>", after("<!-- > --><html lang=\"x>y\">", "<p>x</p>")),
        ("<!-- <html lang=\"no\"> --><html lang=\"yes\"><p>x</p>", after("<!-- <html lang=\"no\"> --><html lang=\"yes\">", "<p>x</p>")),
        ("<!--><html lang=\"a>b\"><p>x</p>", after("<!--><html lang=\"a>b\">", "<p>x</p>")),
        ("<!---><html lang=\"a>b\"><p>x</p>", after("<!---><html lang=\"a>b\">", "<p>x</p>")),
        ("<!----><html lang=\"a>b\"><p>x</p>", after("<!----><html lang=\"a>b\">", "<p>x</p>")),
        ("<!----!><html lang=\"a>b\"><p>x</p>", after("<!----!><html lang=\"a>b\">", "<p>x</p>")),
        ("<!-- --!><html><p>x</p>", after("<!-- --!><html>", "<p>x</p>")),
        ("<?xml version=\"1.0\"?><html lang=\"a>b\"><p>x</p>", after("<?xml version=\"1.0\"?><html lang=\"a>b\">", "<p>x</p>")),
        ("<!doctype html><p>x</p>", after("<!doctype html>", "<p>x</p>")),
        ("<!-- <!doctype x> --><!DOCTYPE html><p>x</p>", after("<!-- <!doctype x> --><!DOCTYPE html>", "<p>x</p>")),
        ("<!doctype html \"a>b\"><p>x</p>", after("<!doctype html \"a>", "b\"><p>x</p>")),
        ("<!DOCTYPE html PUBLIC \"-//W3C//DTD HTML 4.01//EN\"><html lang=\"a>b\"><p>x</p>", after("<!DOCTYPE html PUBLIC \"-//W3C//DTD HTML 4.01//EN\"><html lang=\"a>b\">", "<p>x</p>")),
        // Left as they are: no end, or no document outside a comment or a script.
        ("<html lang=\"a>b\"", "<html lang=\"a>b\"".to_string()),
        ("<html lang=\"a><p>x</p>", "<html lang=\"a><p>x</p>".to_string()),
        ("<!--!><html lang=\"a>b\"><p>x</p>", "<!--!><html lang=\"a>b\"><p>x</p>".to_string()),
        ("<!---!><html><p>x</p>", "<!---!><html><p>x</p>".to_string()),
        ("<!doctype html", "<!doctype html".to_string()),
        ("<script>\"<html>\"</script><p>x</p>", "<script>\"<html>\"</script><p>x</p>".to_string()),
        ("<p>1 < 2</p><", "<p>1 < 2</p><".to_string()),
        ("</", "</".to_string()),
        ("", String::new()),
    ] {
        assert_eq!(with_head(page, &h), want, "{:?}", page);
    }
}

/// The head's own `<head ...>`, `</head ...>` and `<body ...>` are read as tags too: a `</head>`
/// in a quoted value is none, an end tag's own attributes are passed over, a `<body>` whose value
/// holds a `>` is where a page with no head ends it; a title's text runs to `</title>`, its name
/// whole (`</titles>` is text). Where the head's rest is one tag, script or comment that never
/// ends (the browser drops it, or takes it whole), the tags go before it, the head's own of the
/// keys said before it taken away (src/head.rs marks, text_end, FEEDBACK84.md Q-1320, found by the
/// coverage tests; branch feedback82-85)
#[test]
fn the_heads_own_tags_are_read_as_the_browser_reads_them() {
    let h = said_head(&[("title", "T")]);
    for (page, want) in [
        ("<head data-x=\"</head>\"><title>Old</title></head><body>b</body>", format!("<head data-x=\"</head>\">{TT}</head><body>b</body>")),
        ("<head><title>Old</title></head x=\">\"><body>b</body>", format!("<head>{TT}</head x=\">\"><body>b</body>")),
        ("<html><body class=\"a>b\">x</body></html>", format!("<html>{TT}<body class=\"a>b\">x</body></html>")),
        ("<html><BODY\tclass='a>b'>x</BODY></html>", format!("<html>{TT}<BODY\tclass='a>b'>x</BODY></html>")),
        ("<html><head><title>a</titles>b</title></head><body>c</body>", format!("<html><head>{TT}</head><body>c</body>")),
        ("<html><head><title data-x=\"</title>\">a</title ></head>", format!("<html><head>{TT}</head>")),
        ("<head><title>Old</title><script>let a = \"</head>\"", format!("<head>{TT}<script>let a = \"</head>\"")),
        ("<head><title>Old</title><!-- </head>", format!("<head>{TT}<!-- </head>")),
        ("<head><title>Old</title><meta x='y></head><body>b</body>", format!("<head>{TT}<meta x='y></head><body>b</body>")),
    ] {
        assert_eq!(with_head(page, &h), want, "{:?}", page);
    }
    // Two keys: the description before the tag that never ends taken away, the title after it
    // (which the browser never reads) left.
    let h = said_head(&[("title", "T"), ("description", "D")]);
    let page = "<html><head><meta name=\"description\" content=\"old\"><meta x='y><title>Old</title></head><body>b</body></html>";
    assert_eq!(
        with_head(page, &h),
        format!("<html><head>{TT}<meta name=\"description\" content=\"D\"><meta property=\"og:description\" content=\"D\"><meta x='y><title>Old</title></head><body>b</body></html>")
    );
    // A custom element whose name starts with `meta` or `title` is neither. Changed with the
    // review of 1a4bb9b (FEEDBACK84.md, Q-1320; branch feedback82-85): no head holds a custom
    // element, so the browser ends the head at `<meta-x>` (parse5: it and all after it are the
    // body's); the tags go before it and the title after it, in the body, stays: nothing after the
    // head's end is cut. The test asked the tags before `</head>` and that title taken away.
    let page = "<html><head><meta-x name=\"description\"></meta-x><title-bar>t</title-bar><title>Old</title></head><body>b</body></html>";
    assert_eq!(
        with_head(page, &h),
        format!("<html><head>{TT}<meta name=\"description\" content=\"D\"><meta property=\"og:description\" content=\"D\"><meta-x name=\"description\"></meta-x><title-bar>t</title-bar><title>Old</title></head><body>b</body></html>")
    );
}

/// `save: null` and `max_bytes: null` are no options on `http.stream`, as `http.get` reads them:
/// the stream runs. Written with a value (a text, an empty one, a template, a number) they stop
/// the program, and `quillo check` now says so first; a value worked out is judged as it runs, a
/// null passing (src/fetch.rs opts_of, src/check.rs, FEEDBACK85.md Q-1342, found by the coverage
/// tests; branch feedback82-85)
#[test]
fn a_null_save_is_no_option_on_http_stream() {
    let d = Dir::new("streamsave");
    let u = format!("http://127.0.0.1:{}", stand_in());
    d.write(
        "s.qwe",
        &format!(
            "print(http.stream(\"{u}/ok\", (p) => print(p), {{ save: null }}).status)\nprint(http.stream(\"{u}/ok\", (p) => print(p), {{ max_bytes: null, save: null, public_only: false }}).status)\nprint(http.get(\"{u}/ok\", {{}}, {{ save: null }}).body)\nlet none = json.parse(\"null\")\nprint(http.stream(\"{u}/ok\", (p) => print(p), {{ save: none }}).status)\n"
        ),
    );
    checked(d.path(), "s.qwe");
    let (code, out) = quillo(d.path(), &["run", "s.qwe"]);
    assert_eq!((code, out.as_str()), (Some(0), "hello\n200\nhello\n200\nhello\nhello\n200\n"));
    for (opts, key) in [("{ save: \"a.bin\" }", "save"), ("{ save: \"\" }", "save"), ("{ save: `${n}.bin` }", "save"), ("{ max_bytes: 5 }", "max_bytes"), ("{ save: null, max_bytes: 0 }", "max_bytes"), ("{ save: \"caf\u{e9}.bin\" }", "save")] {
        d.write("t.qwe", &format!("let n = \"a\"\nprint(http.stream(\"{u}/ok\", (p) => print(p), {opts}))\n"));
        let (code, out) = quillo(d.path(), &["check", "t.qwe"]);
        assert_eq!(code, Some(1), "{}: {}", opts, out);
        assert!(out.contains(&format!("t.qwe:2: error: `http.stream`: `{}` is for the calls that read an answer whole", key)), "{}: {}", opts, out);
        let (code, out) = quillo(d.path(), &["run", "t.qwe", "--no-check"]);
        assert_eq!(code, Some(1), "{}: {}", opts, out);
        assert!(out.contains(&format!("quillo: t.qwe:2: `http.stream`: `{}` is for the calls that read an answer whole", key)), "{}: {}", opts, out);
        assert!(!out.contains("hello"), "{}: {}", opts, out);
    }
    // Worked out: check says nothing; the run stops on a text.
    d.write("w.qwe", &format!("let f = json.parse(\"\\\"a.bin\\\"\")\nprint(http.stream(\"{u}/ok\", (p) => print(p), {{ save: f }}))\n"));
    checked(d.path(), "w.qwe");
    let (code, out) = quillo(d.path(), &["run", "w.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("quillo: w.qwe:2: `http.stream`: `save` is for the calls that read an answer whole"), "{}", out);
    assert!(!d.path().join("a.bin").exists());
}

/// A path confirmed under `writes` is read as well, as the hold reads it as the program runs
/// (`writes` is read and write, `files` read only: f98c76f, CHANGELOG's "files is read-only"):
/// `quillo check` and the start's count ask no `files` entry for it; a path confirmed only in
/// `files` is still not written (src/confirm.rs missing, FEEDBACK83.md Q-1300, found by the
/// coverage tests; branch feedback82-85)
#[test]
fn a_path_confirmed_under_writes_is_read_without_a_files_entry() {
    let d = Dir::new("writesread");
    d.write("data/a.txt", "hi");
    d.write("main.qwe", "print(read_file(\"data/a.txt\"))\nprint(image.convert(\"data/in.png\", \"data/out.png\"))\n");
    d.write("quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/\"]\n");
    let (code, out) = quillo(d.path(), &["run", "main.qwe", "--no-check"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.starts_with("hi\n") && !out.contains("not confirmed"), "{}", out);
    checked(d.path(), "main.qwe");
    d.write("quillo.confirmed", "[\"main.qwe\"]\nfiles = [\"data/\"]\n");
    let (code, out) = quillo(d.path(), &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("main.qwe:2: error: writes data/out.png: not confirmed for this file"), "{}", out);
}

/// `save` with `cache` on `http.get`, written so, is an error of `quillo check`, as it is when the
/// program runs and as the changelog says: `cache: 1` and the longest, a day, are; `cache: 0`,
/// none, and `save: null` or `save: ""` (said otherwise when it runs) are not this error (src/check.rs
/// http_rules, FEEDBACK85.md Q-1342, found by the coverage tests; branch feedback82-85)
#[test]
fn save_with_cache_is_an_error_before_it_runs() {
    let d = Dir::new("savecache");
    let u = format!("http://127.0.0.1:{}", stand_in());
    for opts in ["{ save: \"a.bin\", cache: 60 }", "{ cache: 1, save: \"a.bin\" }", "{ save: `${n}.bin`, cache: 86400 }", "{ save: \"\u{6771}.bin\", cache: 5 }"] {
        d.write("t.qwe", &format!("let n = \"a\"\nprint(http.get(\"{u}/ok\", {{}}, {opts}))\n"));
        let (code, out) = quillo(d.path(), &["check", "t.qwe"]);
        assert_eq!(code, Some(1), "{}: {}", opts, out);
        assert!(out.contains("t.qwe:2: error: `http.get`: `cache` keeps answers in memory and `save` writes one to its file: one or the other"), "{}: {}", opts, out);
        let (code, out) = quillo(d.path(), &["run", "t.qwe", "--no-check"]);
        assert_eq!(code, Some(1), "{}: {}", opts, out);
        assert!(out.contains("quillo: t.qwe:2: `http.get`: `cache` keeps answers in memory and `save` writes one to its file: one or the other"), "{}: {}", opts, out);
    }
    d.write("s.qwe", &format!("print(http.get(\"{u}/ok\", {{}}, {{ save: \"a.bin\", cache: 0 }}).bytes)\nprint(http.get(\"{u}/ok\", {{}}, {{ save: null, cache: 60 }}).body)\n"));
    checked(d.path(), "s.qwe");
    let (code, out) = quillo(d.path(), &["run", "s.qwe"]);
    assert_eq!((code, out.as_str()), (Some(0), "5\nhello\n"));
    assert_eq!(read(&d.path().join("a.bin")), "hello");
    d.write("e.qwe", &format!("print(http.get(\"{u}/ok\", {{}}, {{ save: \"\", cache: 60 }}))\n"));
    let (_, out) = quillo(d.path(), &["check", "e.qwe"]);
    assert!(!out.contains("one or the other"), "{}", out);
}

// ---- src/head.rs: what the review of 1a4bb9b found (slice F2) --------------------------------------

/// What `page_head({ title: "T", description: "D" })` writes.
const TD: &str = "<title>T</title><meta property=\"og:title\" content=\"T\"><meta name=\"description\" content=\"D\"><meta property=\"og:description\" content=\"D\">";

/// The repro: a script that says `<!--` and then `<script>` is double escaped, so its first
/// `</script>` ends nothing but that and the second ends it, as the browser reads it (parse5): the
/// tags no longer go inside its string, a title it says is no tag, and the head's title after it
/// is replaced; one that says `-->` first is back to plain, one that says `<!--` and no `<script>`
/// ends at its first (its title, after, the head's). Each checked with parse5 (src/head.rs
/// script_end, FEEDBACK84.md Q-1320, found by the review of 1a4bb9b; branch feedback82-85)
#[test]
fn a_script_that_says_script_inside_a_comment_ends_where_the_browser_ends_it() {
    let d = Dir::new("headesc");
    d.write(
        "main.qwe",
        r#"route GET / {
    page_head({ title: "T", description: "D" })
    return raw("<head><script><!--\nvar s = \"<script></script></head>\";\n--></script><title>Old</title></head><body>b</body>")
}
route GET /w {
    page_head({ title: "T", description: "D" })
    return raw("<html><head><script><!--\ndocument.write(\"<script>x()</script><title>in js</title>\");\n--></script><title>Old</title></head><body>b</body></html>")
}
"#,
    );
    checked(d.path(), "main.qwe");
    // Without the script a page with one is given to fetch the links ahead: the page as written.
    let (_g, port, log) = serve(d.path(), "main.qwe", &["--no-prefetch"]);
    let a = get(port, "/");
    assert_eq!(
        (a.status, a.body.as_str()),
        (200, format!("<head><script><!--\nvar s = \"<script></script></head>\";\n--></script>{TD}</head><body>b</body>").as_str()),
        "{}",
        read(&log)
    );
    let a = get(port, "/w");
    assert_eq!(
        (a.status, a.body.as_str()),
        (200, format!("<html><head><script><!--\ndocument.write(\"<script>x()</script><title>in js</title>\");\n--></script>{TD}</head><body>b</body></html>").as_str()),
        "{}",
        read(&log)
    );
    let h = said_head(&[("title", "T")]);
    let page = |script: &str| format!("<html><head><script>{script}</script><title>Old</title></head><body>b</body></html>");
    let kept = |script: &str| format!("<html><head><script>{script}</script>{TT}</head><body>b</body></html>");
    for script in [
        "<!--<script></script>-->",
        // `-->` while double escaped is back to plain: the next `</script>` ends it.
        "<!--<script>-->",
        // `<scripts>` and `<script1>` start nothing: escaped, the next `</script>` ends it.
        "<!--<scripts>",
        "<!--<script1>",
        // Any case, `/` or a line feed after the name, the end's attributes passed over.
        "<!--<SCRIPT/></script x>-->",
        "<!--<script\n></script\n>-->",
        "<!--<script></script\t>-->",
        // `<!-->` and `<!--->` end at once; `<!-` and `<!x` start nothing.
        "<!-->x",
        "<!--->x",
        "a<!-b",
        "<!x<script>",
        // Dashes: `-->` and `--->` end the escape, a `<!--` after it starts one again.
        "<!-- <script> -->",
        "<!--<script>--><!--",
        "<!--<script>-</script>--->",
        "<!--<script></></x></script-->",
        "<!--<script></scriptx></script>-->",
        "<!--<script></script<-->",
        "<!--<script>\"東京 · café</script>\"-->",
        "",
    ] {
        assert_eq!(with_head(&page(script), &h), kept(script), "{:?}", script);
    }
    // Escaped with no `<script>`: its first `</script>` ends it, so what it says after is the
    // page's, a title of the head's (replaced) and text (which ends the head: the tags go before it
    // and the title after it, in the body, stays).
    let page = "<html><head><script><!-- var a = \"</script><title>t</title>\"; --></script><title>Old</title></head><body>b</body></html>";
    assert_eq!(with_head(page, &h), format!("<html><head><script><!-- var a = \"</script>{TT}\"; --></script><title>Old</title></head><body>b</body></html>"));
    // One that runs to the page's end, escaped or double escaped: the tags before it.
    for rest in ["<script><!--<script></script>", "<script><!--<script>", "<script><!--", "<script><!--<script></script>--"] {
        assert_eq!(with_head(&format!("<head><title>Old</title>{rest}"), &h), format!("<head>{TT}{rest}"), "{:?}", rest);
    }
}

/// The head ends where the browser's tree builder ends it (parse5): at an `<svg>` or `<math>`
/// (their titles, a CDATA's too, are kept and the head's after them, now the body's, stay), at
/// text that is not spaces (`&#32;`, `&#x20`, `&Tab;`, `&NewLine;` are spaces; `&#33;`, `&tab;`,
/// `é` are not), at any element no head holds (a custom one), at `</body>`, `</html>` or
/// `</br>`, not at `</div>`, `</>`, `</ x>` or a second `<head>` or `<html>`; the tags go before
/// what ends it. A template's content is never cut, read as the browser reads it to the
/// `</template>` that closes it: a template inside it, a script saying `</template>`, a `</head>`
/// in it; in SVG a `<style>` holds tags and a CDATA text, at an integration point or after an
/// element that breaks out of foreign content it is raw text again. After `</head>` a title or
/// meta still goes in the head and is replaced, until anything else. Each checked with parse5
/// (src/head.rs scan, walk, not_space, space_ref, FEEDBACK84.md Q-1320, found by the review of
/// 1a4bb9b; branch feedback82-85)
#[test]
fn the_head_ends_where_the_browsers_tree_builder_ends_it() {
    let h = said_head(&[("title", "T"), ("description", "D")]);
    let doc = |head: &str| format!("<html><head>{head}</head><body>b</body></html>");
    for (head, want) in [
        // Foreign content ends the head.
        ("<svg><title>Icon</title></svg><title>Old</title>", format!("{TD}<svg><title>Icon</title></svg><title>Old</title>")),
        ("<svg><![CDATA[ > <title>cdata</title> ]]></svg><title>Old</title>", format!("{TD}<svg><![CDATA[ > <title>cdata</title> ]]></svg><title>Old</title>")),
        ("<title>Old</title><math><mi><title>m</title></mi></math>", format!("{TD}<math><mi><title>m</title></mi></math>")),
        // Text, a custom element, `</br>`, `</body>`, `</html>`.
        ("<title>Old</title>Hello<meta name=\"description\" content=\"old\">", format!("{TD}Hello<meta name=\"description\" content=\"old\">")),
        ("<meta name=\"description\" content=\"old\">é<title>Old</title>", format!("{TD}é<title>Old</title>")),
        ("<title>Old</title>&#33;<title>x</title>", format!("{TD}&#33;<title>x</title>")),
        ("<title>Old</title>&tab;<title>x</title>", format!("{TD}&tab;<title>x</title>")),
        ("<title>Old</title>&#;<title>x</title>", format!("{TD}&#;<title>x</title>")),
        ("<title>Old</title>&#99999999999999999999;<title>x</title>", format!("{TD}&#99999999999999999999;<title>x</title>")),
        ("<title>Old</title>< title>x</title>", format!("{TD}< title>x</title>")),
        ("<meta-x name=\"description\"></meta-x><title>Old</title>", format!("{TD}<meta-x name=\"description\"></meta-x><title>Old</title>")),
        ("<title>Old</title></br><title>x</title>", format!("{TD}</br><title>x</title>")),
        ("<title>Old</title></body><title>x</title>", format!("{TD}</body><title>x</title>")),
        ("<title>Old</title></html><title>x</title>", format!("{TD}</html><title>x</title>")),
        // Spaces, written as references too, and end tags the head passes over.
        ("&#32;<title>Old</title>&#x20<meta name=\"description\" content=\"o\">", format!("&#32;&#x20{TD}")),
        ("&Tab;&NewLine;&#0000032;&#X9;&#10;&#12;<title>Old</title>", format!("&Tab;&NewLine;&#0000032;&#X9;&#10;&#12;{TD}")),
        ("</div></p></></ x></template><title>Old</title>", format!("</div></p></></ x></template>{TD}")),
        ("<head><html lang=\"x\"><title>Old</title>", format!("<head><html lang=\"x\">{TD}")),
        // The tags before a noscript, and its description, the head's with scripting off, taken away
        // (FEEDBACK84.md, Q-1320, found by the review of a6988b4: it was kept, the tags after it).
        ("<base href=\"/\"><basefont><bgsound><noframes><title>nf</title></noframes><noscript><meta name=\"description\" content=\"ns\"></noscript><title>Old</title>", format!("<base href=\"/\"><basefont><bgsound><noframes><title>nf</title></noframes>{TD}<noscript></noscript>")),
        // Templates: never cut inside, read to the `</template>` that closes them.
        ("<template><template></template><title>tpl</title></template><title>Old</title>", format!("<template><template></template><title>tpl</title></template>{TD}")),
        ("<template><script>var t = \"</template>\";</script><meta name=\"description\" content=\"tpl\"></template><meta name=\"description\" content=\"old\">", format!("<template><script>var t = \"</template>\";</script><meta name=\"description\" content=\"tpl\"></template>{TD}")),
        ("<template><div></head></div></template><title>Old</title>", format!("<template><div></head></div></template>{TD}")),
        ("<template><svg><![CDATA[</template>]]></svg><title>tpl</title></template><title>Old</title>", format!("<template><svg><![CDATA[</template>]]></svg><title>tpl</title></template>{TD}")),
        ("<template><svg><style></template><title>Old</title>", format!("<template><svg><style></template>{TD}")),
        ("<template><svg><title></template><title>Old</title>", format!("<template><svg><title></template>{TD}")),
        ("<template><svg><foreignObject><title></template></title></foreignObject></svg></template><title>Old</title>", format!("<template><svg><foreignObject><title></template></title></foreignObject></svg></template>{TD}")),
        ("<template><svg><desc><style></template></style></desc></svg></template><title>Old</title>", format!("<template><svg><desc><style></template></style></desc></svg></template>{TD}")),
        ("<template><math><mi><style></template></style></mi></math></template><title>Old</title>", format!("<template><math><mi><style></template></style></mi></math></template>{TD}")),
        ("<template><math><mi><mglyph><style></template><title>Old</title>", format!("<template><math><mi><mglyph><style></template>{TD}")),
        ("<template><math><annotation-xml encoding=\"Text/HTML\"><style></template></style></annotation-xml></math></template><title>Old</title>", format!("<template><math><annotation-xml encoding=\"Text/HTML\"><style></template></style></annotation-xml></math></template>{TD}")),
        ("<template><math><annotation-xml encoding=\"application/xhtml+xml\"><style></template></style></annotation-xml></math></template><title>Old</title>", format!("<template><math><annotation-xml encoding=\"application/xhtml+xml\"><style></template></style></annotation-xml></math></template>{TD}")),
        ("<template><math><annotation-xml><style></template><title>Old</title>", format!("<template><math><annotation-xml><style></template>{TD}")),
        ("<template><math><annotation-xml><svg><title><style></template></style></title></svg></annotation-xml></math></template><title>Old</title>", format!("<template><math><annotation-xml><svg><title><style></template></style></title></svg></annotation-xml></math></template>{TD}")),
        ("<template><svg><p><style></template></style></template><title>Old</title>", format!("<template><svg><p><style></template></style></template>{TD}")),
        ("<template><svg><font color=red><style></template></style></template><title>Old</title>", format!("<template><svg><font color=red><style></template></style></template>{TD}")),
        ("<template><svg><font><style></template><title>Old</title>", format!("<template><svg><font><style></template>{TD}")),
        ("<template><svg></p><style></template></style></template><title>Old</title>", format!("<template><svg></p><style></template></style></template>{TD}")),
        ("<template><div><svg><circle></div><title></template></title></template><title>Old</title>", format!("<template><div><svg><circle></div><title></template></title></template>{TD}")),
        ("<template><svg><g></g><template></svg><style></template></style></template><title>Old</title>", format!("<template><svg><g></g><template></svg><style></template></style></template>{TD}")),
        ("<template><svg/><title></template></title></template><title>Old</title>", format!("<template><svg/><title></template></title></template>{TD}")),
        ("<template><SVG><circle/><style></template><title>Old</title>", format!("<template><SVG><circle/><style></template>{TD}")),
        ("<template><textarea></template></textarea><plaintext></template><title>Old</title>", format!("{TD}<template><textarea></template></textarea><plaintext></template><title>Old</title>")),
        // A second head, and what follows `</head>` the browser puts in the head.
        ("<title>Old</title></head><head><title>Second</title>", format!("{TD}</head><head>")),
        ("<title>Old</title></head>\n<!-- c --><!doctype x><html></head></x></template><meta name=\"description\" content=\"after\"><title>after</title>", format!("{TD}</head>\n<!-- c --><!doctype x><html></head></x></template>")),
        ("<title>Old</title></head><noscript><title>ns</title></noscript>", format!("{TD}</head><noscript><title>ns</title></noscript>")),
        ("<title>Old</title></head></html><title>after</title>", format!("{TD}</head></html><title>after</title>")),
        ("<title>Old</title></head>x<title>after</title>", format!("{TD}</head>x<title>after</title>")),
        ("</head><template><title>t</title></template><title>after</title> x <title>body</title>", format!("{TD}</head><template><title>t</title></template> x <title>body</title>")),
    ] {
        assert_eq!(with_head(&doc(head), &h), doc(&want), "{:?}", head);
    }
    // A template, a title or one after `</head>` that runs to the page's end: the tags before it,
    // a title cut to there.
    for (page, want) in [
        ("<html><head><title>Old</title", format!("<html><head>{TD}")),
        ("<html><head><title>Old</title ", format!("<html><head>{TD}")),
        ("<html><head><title>Old</title a=\"", format!("<html><head>{TD}")),
        ("<html><head></head><title>after", format!("<html><head>{TD}</head>")),
        ("<html><head><title>Old</title><template><title>t", format!("<html><head>{TD}<template><title>t")),
        ("<html><head><template><svg><![CDATA[", format!("<html><head>{TD}<template><svg><![CDATA[")),
        ("<html><head><template><plaintext></template><title>Old</title>", format!("<html><head>{TD}<template><plaintext></template><title>Old</title>")),
        ("<html><head><template><x a=\"", format!("<html><head>{TD}<template><x a=\"")),
        ("<html><head><style>a</style", format!("<html><head>{TD}<style>a</style")),
    ] {
        assert_eq!(with_head(page, &h), want, "{:?}", page);
    }
}

/// A head the page leaves out (no `<head>`) the browser starts at its first element: where that is
/// one of a key said, the tags take its place, so the spaces, comment or doctype after it stay in
/// the head (a doctype after a title is passed over there; first, it would make a quirks page a
/// standard one); where it is another, the tags go where the head ends. A page that starts
/// `<head>` with no `</head>`, `<body>`, `<html>` or doctype is a document; so is one with a
/// `<frameset>`, and one with a `<body>` or `<html>` after its part's elements; a `<body>` in a
/// template or a CDATA, a `<html>` in a script, makes none. The byte order mark stays first.
/// Each checked with parse5 (src/head.rs apply, scan, walk, FEEDBACK84.md Q-1320, found by the
/// review of 1a4bb9b; branch feedback82-85)
#[test]
fn a_head_left_out_or_never_ended_takes_the_tags_where_the_browser_has_it() {
    let h = said_head(&[("title", "T"), ("description", "D")]);
    for (page, want) in [
        ("<title>Old</title><!doctype html><p>x</p>", format!("{TD}<!doctype html><p>x</p>")),
        ("<meta name=\"description\" content=\"o\">&Tab;<!-- c --><head lang=\"x\"><link rel=\"icon\"><frame>", format!("{TD}&Tab;<!-- c --><head lang=\"x\"><link rel=\"icon\"><frame>")),
        ("<!doctype html><title>Old</title>\n<p>x</p>", format!("<!doctype html>{TD}\n<p>x</p>")),
        ("<!doctype html><script>s</script><title>Old</title><p>x</p>", format!("<!doctype html><script>s</script>{TD}<p>x</p>")),
        ("<html><script>s</script>  <p>x</p>", format!("<html><script>s</script>  {TD}<p>x</p>")),
        ("<title>Old</title></head><p>x</p>", format!("{TD}</head><p>x</p>")),
        ("<title>Old</title><frameset><frame></frameset>", format!("{TD}<frameset><frame></frameset>")),
        // `<head>` and nothing else that says a document.
        ("<head><script>\"</head>\"</script></body></html>", format!("<head><script>\"</head>\"</script>{TD}</body></html>")),
        ("<head>", format!("<head>{TD}")),
        ("<HEAD class=\"a>b\"><title>Old</title>", format!("<HEAD class=\"a>b\">{TD}")),
        ("<head><title>Old</title><meta name=\"description\" content=\"old\">", format!("<head>{TD}")),
        ("<head>x", format!("<head>{TD}x")),
        // What says a document after the part's elements.
        ("<p>x</p><body>", format!("{TD}<p>x</p><body>")),
        ("<p>x</p><!doctype html>", format!("{TD}<p>x</p><!doctype html>")),
        ("<li>x</li><HTML>", format!("{TD}<li>x</li><HTML>")),
        ("<li>x</li></head>", format!("{TD}<li>x</li></head>")),
        ("<li><frameset>", format!("{TD}<li><frameset>")),
        ("<svg><![CDATA[ > ]]></svg><body>", format!("{TD}<svg><![CDATA[ > ]]></svg><body>")),
        // At an integration point a `<![CDATA[` is a comment, to its first `>`.
        ("<svg><desc><![CDATA[ > ]]><body>", format!("{TD}<svg><desc><![CDATA[ > ]]><body>")),
        ("<math><mi><![CDATA[ > ]]><body>", format!("{TD}<math><mi><![CDATA[ > ]]><body>")),
        ("\u{feff}<head><title>Old</title></head>", format!("\u{feff}<head>{TD}</head>")),
        ("\u{feff}<title>Old</title><body>", format!("\u{feff}{TD}<body>")),
    ] {
        assert_eq!(with_head(page, &h), want, "{:?}", page);
    }
    // No document: left as they are.
    for page in [
        "<template><body></template>",
        "<template><template></template><head></template>",
        "<li>x</li>",
        "<svg><![CDATA[<html>]]></svg>",
        "<svg><![CDATA[ > <html> ]]></svg>",
        "<svg><![CDATA[ > <html>",
        "<p><script>\"<html>\"</script></p>",
        "<p><script>\"<html>\"",
        "<p><title><body></title></p>",
        "<p><plaintext><body>",
        "<p><svg><title><style><body>",
        "<title>Old</title><p>x</p>",
        "<title>Old</title><!-- <body> -->",
        "<p a=\"<body>",
        "\u{feff}<title>Old</title>",
        "",
    ] {
        assert_eq!(with_head(page, &h), page, "{:?}", page);
    }
}

/// The time `apply` takes for a page, its best of three.
fn apply_time(page: &str, h: &quillo::head::Head) -> Duration {
    (0..3)
        .map(|_| {
            let mut b = page.as_bytes().to_vec();
            let t = Instant::now();
            quillo::head::apply(&mut b, h);
            let e = t.elapsed();
            assert!(b.len() < page.len());
            e
        })
        .min()
        .unwrap()
}

/// A head of many titles among tags it keeps is made again in one pass: each title taken away, the
/// kept tags in their order and the tags once, before `</head>`; eight times the titles take about
/// eight times as long, not sixty-four (each was a move of the page's rest: quadratic, a 5 MB page
/// with 20,000 seconds 1.9 s). Measured against itself, so a slow machine is no failure
/// (src/head.rs apply, FEEDBACK84.md Q-1320, found by the review of 1a4bb9b; branch feedback82-85)
#[test]
fn a_head_of_many_titles_is_made_again_in_one_pass() {
    let h = said_head(&[("title", "T")]);
    let meta = format!("<meta name=\"x\" content=\"{}\">", "k".repeat(80));
    let page = |n: usize| format!("<!doctype html><html><head>{}</head><body>b</body></html>", format!("<title>t</title>{meta}").repeat(n));
    let want = |n: usize| format!("<!doctype html><html><head>{}{TT}</head><body>b</body></html>", meta.repeat(n));
    assert_eq!(with_head(&page(3), &h), want(3));
    assert_eq!(with_head(&page(2000), &h), want(2000));
    let (small, big) = (apply_time(&page(2000), &h), apply_time(&page(16000), &h));
    assert!(big < small * 24, "2,000 titles {:?}, 16,000 {:?}", small, big);
}

/// The scripts the head is written with, as the browser has them in its head (parse5): a script
/// double escaped whole, those after `</head>` that go in the head, not a template's nor one after
/// the head's end; one that runs to the page's end to there; none of a page that is no document
/// (src/head.rs head_scripts, FEEDBACK84.md Q-1320, found by the review of 1a4bb9b; branch
/// feedback82-85)
#[test]
fn the_heads_scripts_are_those_the_browser_puts_in_it() {
    let s = |page: &str| quillo::head::head_scripts(page.as_bytes());
    assert_eq!(
        s("<html><head><script><!--<script></script>--></script><template><script>t</script></template></head>\n<script src=\"/a.js\"></script><body><script>b</script></body></html>"),
        ["<script><!--<script></script>--></script>", "<script src=\"/a.js\"></script>"]
    );
    assert_eq!(s("<html><head><script>a</script>x<script>b</script></head></html>"), ["<script>a</script>"]);
    assert_eq!(s("<html><head><svg><script>a</script></svg></head></html>"), Vec::<String>::new());
    assert_eq!(s("<!doctype html><script>a</script><p><script>b</script>"), ["<script>a</script>"]);
    assert_eq!(s("<head><script>\"東京\"</script><script>let a = 1"), ["<script>\"東京\"</script>", "<script>let a = 1"]);
    assert_eq!(s("<script>a</script><p>part</p>"), Vec::<String>::new());
}

/// A template's content where many list items, paragraphs and cells are left open, as the body
/// ends them, still has its SVG followed (its `<style>` holds tags, so the `</template>` in it
/// closes the template); and a part of many elements that never end, with as many end tags that
/// close none, is read in one pass: eight times as many take about eight times as long (src/head.rs
/// walk, ends_before, FEEDBACK84.md Q-1320, found by the review of 1a4bb9b; branch feedback82-85)
#[test]
fn elements_left_open_are_ended_as_the_body_ends_them_and_read_in_one_pass() {
    let h = said_head(&[("title", "T")]);
    for open in ["<ul><li>a".to_string() + &"<li>b".repeat(100) + "</ul>", "<p>a".repeat(100) + "<div>d</div>", "<dl>".to_string() + &"<dt>t<dd>d".repeat(60) + "</dl>", "<table>".to_string() + &"<tr><td>a<th>b".repeat(60) + "</table>", "<select><optgroup>".to_string() + &"<option>o".repeat(70) + "<optgroup><option>x</select>"] {
        let page = format!("<html><head><template>{open}<svg><style></template><title>Old</title></head><body>b</body></html>");
        assert_eq!(with_head(&page, &h), format!("<html><head><template>{open}<svg><style></template>{TT}</head><body>b</body></html>"), "{:?}", &open[..20]);
    }
    let part = |n: usize| format!("{}{}", "<span>".repeat(n), "</x>".repeat(n));
    let time = |n: usize| {
        let page = part(n);
        (0..3)
            .map(|_| {
                let mut b = page.as_bytes().to_vec();
                let t = Instant::now();
                quillo::head::apply(&mut b, &h);
                let e = t.elapsed();
                assert_eq!(b, page.as_bytes());
                e
            })
            .min()
            .unwrap()
    };
    let (small, big) = (time(2000), time(16000));
    assert!(big < small * 24, "2,000 elements {:?}, 16,000 {:?}", small, big);
}

// ---- src/head.rs: what the review of a6988b4 found (slice N) ---------------------------------------

/// What `page_head({ title: "T", description: "D", canonical: "/c" })` writes.
const TDC: &str = "<title>T</title><meta property=\"og:title\" content=\"T\"><meta name=\"description\" content=\"D\"><meta property=\"og:description\" content=\"D\"><link rel=\"canonical\" href=\"/c\"><meta property=\"og:url\" content=\"/c\">";

/// The repro: a tracking pixel's `<noscript><img></noscript>` in the head, which a crawler (scripting
/// off: a search engine's, a social card's) reads as an image that ends the head, so the tags,
/// written before `</head>`, were the body's there; and the noscript's canonical, a second one to
/// that crawler, first. Now the tags go before the head's first noscript and its canonical is
/// taken away, so the head holds one title, description and canonical, the new ones, read with
/// scripting on or off (parse5 both ways) (src/head.rs scan, hidden, FEEDBACK84.md Q-1320, found
/// by the review of a6988b4; branch feedback82-85)
#[test]
fn a_heads_noscript_gets_the_tags_before_it_and_its_own_are_taken_away() {
    let d = Dir::new("headnos");
    d.write(
        "main.qwe",
        r#"route GET / {
    page_head({ title: "T", description: "D", canonical: "https://example.com/c" })
    return raw("<!doctype html><html><head><meta charset=\"utf-8\"><noscript><link rel=\"canonical\" href=\"/ns\"><img height=\"1\" width=\"1\" src=\"https://px.example/tr\"/></noscript><title>Old</title><meta name=\"description\" content=\"old\"><link rel=\"canonical\" href=\"/old\"></head><body>b</body></html>")
}
"#,
    );
    checked(d.path(), "main.qwe");
    let (_g, port, log) = serve(d.path(), "main.qwe", &["--no-prefetch"]);
    let a = get(port, "/");
    assert_eq!(
        (a.status, a.body.as_str()),
        (200, format!("<!doctype html><html><head><meta charset=\"utf-8\">{}<noscript><img height=\"1\" width=\"1\" src=\"https://px.example/tr\"/></noscript></head><body>b</body></html>", TDC.replace("\"/c\"", "\"https://example.com/c\"")).as_str()),
        "{}",
        read(&log)
    );
}

/// What a head's noscript holds, read as a browser with scripting off reads it, up to what ends the
/// head there: its titles, descriptions, og: metas and canonicals are taken away (with scripting
/// on they are its text, no element) and what else it holds is kept: a stylesheet, a style, a
/// comment, a template, a script, a base, spaces; any case, with attributes, `/>`, unicode, empty.
/// What follows an `<img>`, text, an `<svg>` or `</br>` in it (`</body>` once a title closed it, a
/// `<noscript>` after `</head>`) is the body's with scripting off and is left; one whose element
/// runs past the noscript's `</noscript` (as scripting on reads it) is left whole, as is a title
/// that closes it where a `</head>`, `</body>` or `<noscript>` follows (read in the noscript, which
/// passes over them, once it is taken away), not where a script closes it before them or the
/// head ends after them either way. The tags go before the first noscript of the head, one left
/// out too, not before one after `</head>` (the body's) or a template's. Each checked with parse5,
/// scripting on and off (src/head.rs scan, hidden, holds, moves, FEEDBACK84.md Q-1320, found by
/// the review of a6988b4; branch feedback82-85)
#[test]
fn what_a_heads_noscript_holds_is_read_as_scripting_off_reads_it() {
    let h = said_head(&[("title", "T"), ("description", "D"), ("canonical", "/c")]);
    let old = "<title>Old</title><meta name=\"description\" content=\"old\"><link rel=\"canonical\" href=\"/old\">";
    let doc = |head: &str| format!("<!doctype html><html><head>{head}</head><body>b</body></html>");
    for (head, want) in [
        // The pixel, first, last; a stylesheet: the tags before the noscript.
        (format!("<meta charset=\"utf-8\"><noscript><img height=\"1\" width=\"1\" src=\"https://px.example/tr\"/></noscript>{old}"), format!("<meta charset=\"utf-8\">{TDC}<noscript><img height=\"1\" width=\"1\" src=\"https://px.example/tr\"/></noscript>")),
        (format!("{old}<noscript><img src=\"px\"></noscript>"), format!("{TDC}<noscript><img src=\"px\"></noscript>")),
        (format!("<noscript><link rel=\"stylesheet\" href=\"a.css\"></noscript>{old}"), format!("{TDC}<noscript><link rel=\"stylesheet\" href=\"a.css\"></noscript>")),
        (format!("<title>Old</title><noscript><iframe src=\"https://gtm.example/ns.html\" height=\"0\" width=\"0\"></iframe></noscript><meta name=\"description\" content=\"old\">"), format!("{TDC}<noscript><iframe src=\"https://gtm.example/ns.html\" height=\"0\" width=\"0\"></iframe></noscript>")),
        // Its own of the keys said, taken away; what else it holds, kept.
        (format!("<noscript><link rel=\"canonical\" href=\"/ns\"></noscript>{old}"), format!("{TDC}<noscript></noscript>")),
        (format!("<noscript><title>ns</title></noscript>{old}"), format!("{TDC}<noscript></noscript>")),
        (format!("<NOSCRIPT><META NAME=DESCRIPTION CONTENT=ns></NOSCRIPT >{old}"), format!("{TDC}<NOSCRIPT></NOSCRIPT >")),
        (format!("<noscript class=\"a>b\"><meta name=\"description\" content=\"ns\"/></noscript>{old}"), format!("{TDC}<noscript class=\"a>b\"></noscript>")),
        (format!("<noscript><link rel=\"stylesheet\" href=\"a.css\"><meta property=\"og:title\" content=\"ns\"><style>/* <title>s</title> */</style></noscript>{old}"), format!("{TDC}<noscript><link rel=\"stylesheet\" href=\"a.css\"><style>/* <title>s</title> */</style></noscript>")),
        (format!("<noscript><!-- <title>c</title> --><meta name=\"description\" content=\"ns\"></noscript>{old}"), format!("{TDC}<noscript><!-- <title>c</title> --></noscript>")),
        (format!("<noscript><template><title>t</title></template><script>document.write(\"<title>x</title>\")</script><title>ns</title></noscript>{old}"), format!("{TDC}<noscript><template><title>t</title></template><script>document.write(\"<title>x</title>\")</script></noscript>")),
        (format!("<noscript><link rel=\"Canonical\" href=\"/ns\"><link rel=alternate href=/alt><meta property=\"og:url\" content=\"/ns\"></noscript>{old}"), format!("{TDC}<noscript><link rel=alternate href=/alt></noscript>")),
        (format!("<noscript><link rel=\"canonical alternate\" href=\"/ns\"></noscript>{old}"), format!("{TDC}<noscript></noscript>")),
        (format!("<noscript><meta name=\"description\" content=\"東京 · café\"></noscript>{}", old.replace("Old", "Ünï")), format!("{TDC}<noscript></noscript>")),
        (format!("<noscript></noscript>{old}"), format!("{TDC}<noscript></noscript>")),
        (format!("<noscript>\n  &#32;<title>ns</title>\n</noscript>{old}"), format!("{TDC}<noscript>\n  &#32;\n</noscript>")),
        (format!("<noscript><base href=\"/b/\"><title>ns</title></noscript>{old}"), format!("{TDC}<noscript><base href=\"/b/\"></noscript>")),
        (format!("<noscript><noscript><meta name=\"description\" content=\"ns\"></noscript></noscript>{old}"), format!("{TDC}<noscript><noscript></noscript></noscript>")),
        (format!("<noscript><noframes><title>nf</title></noframes><meta name=\"description\" content=\"ns\"></noscript>{old}"), format!("{TDC}<noscript><noframes><title>nf</title></noframes></noscript>")),
        (format!("<noscript></div><head><html lang=x><title>a</title><title>b</title></noscript>{old}"), format!("{TDC}<noscript></div><head><html lang=x></noscript>")),
        // In the noscript `</head>`, `</body>` and `</html>` are passed over; once a title closed it,
        // `</head>` is after the head, where a title or meta is still the head's.
        (format!("<noscript></head></body></html><title>ns</title></noscript>{old}"), format!("{TDC}<noscript></head></body></html></noscript>")),
        (format!("<noscript><title>ns</title><script>s()</script></head><meta name=\"description\" content=\"ns\"></noscript>{old}"), format!("{TDC}<noscript><script>s()</script></head></noscript>")),
        // A title that closes it, then `</head>`, `</body>` or `<noscript>`, read in it once the
        // title is taken away (it passes over them): left, so what follows is read as before.
        (format!("<noscript><title>ns</title></head><meta name=\"description\" content=\"ns\"></noscript>{old}"), format!("{TDC}<noscript><title>ns</title></head></noscript>")),
        (format!("<noscript><title>a</title></body><title>ns</title></noscript>{old}"), format!("{TDC}<noscript><title>a</title></body><title>ns</title></noscript>")),
        (format!("<noscript><title>ns</title></head><noscript><title>b</title></noscript>{old}"), format!("{TDC}<noscript><title>ns</title></head><noscript><title>b</title></noscript>")),
        (format!("<noscript><title>a</title><title>b</title><noscript><meta name=\"description\" content=\"ns\"></noscript>{old}"), format!("{TDC}<noscript><title>a</title><noscript></noscript>")),
        (format!("<noscript><template><noscript></template><meta name=\"description\" content=\"ns\"></noscript>{old}"), format!("{TDC}<noscript><template><noscript></template></noscript>")),
        (format!("<noscript><title>ns</title>x</noscript>{old}"), format!("{TDC}<noscript>x</noscript>")),
        // Two noscripts: the tags before the first, the second's own taken away too.
        (format!("<noscript><link rel=\"stylesheet\" href=\"a.css\"></noscript>{old}<noscript><meta name=\"description\" content=\"ns2\"></noscript>"), format!("{TDC}<noscript><link rel=\"stylesheet\" href=\"a.css\"></noscript><noscript></noscript>")),
        // What ends the head with scripting off: what follows is the body's there, left.
        (format!("<noscript><meta name=\"description\" content=\"ns\"><img src=px><meta name=\"description\" content=\"ns2\"></noscript>{old}"), format!("{TDC}<noscript><img src=px><meta name=\"description\" content=\"ns2\"></noscript>")),
        (format!("<noscript>Turn on JavaScript<title>ns</title></noscript>{old}"), format!("{TDC}<noscript>Turn on JavaScript<title>ns</title></noscript>")),
        (format!("<noscript><svg><title>s</title></svg><title>ns</title></noscript>{old}"), format!("{TDC}<noscript><svg><title>s</title></svg><title>ns</title></noscript>")),
        (format!("<noscript></br><title>ns</title></noscript>{old}"), format!("{TDC}<noscript></br><title>ns</title></noscript>")),
        // A style that holds `</noscript>`: scripting on ends the noscript there, so the description
        // after it is the head's, and taken away as one.
        (format!("<noscript><style></noscript></style><meta name=\"description\" content=\"ns\"></noscript>{old}"), format!("{TDC}<noscript><style></noscript></style></noscript>")),
        (format!("<noscript><title>ns</title></noscript>"), format!("{TDC}<noscript></noscript>")),
        // Not the head's: after `</head>`, in a template, in a script.
        (format!("{old}</head><noscript><title>ns</title></noscript>"), format!("{TDC}</head><noscript><title>ns</title></noscript>")),
        (format!("<template><noscript><title>t</title></noscript></template><script>var a = \"<noscript>\";</script>{old}"), format!("<template><noscript><title>t</title></noscript></template><script>var a = \"<noscript>\";</script>{TDC}")),
    ] {
        assert_eq!(with_head(&doc(&head), &h), doc(&want), "{:?}", head);
    }
    for (page, want) in [
        // A head left out: the tags before its first noscript, or in place of its first element.
        ("<!doctype html><noscript><img src=\"px\"></noscript><title>Old</title><p>x</p>", format!("<!doctype html>{TDC}<noscript><img src=\"px\"></noscript><p>x</p>")),
        ("<!doctype html><title>Old</title><noscript><img src=\"px\"></noscript><p>x</p>", format!("<!doctype html>{TDC}<noscript><img src=\"px\"></noscript><p>x</p>")),
        ("<!doctype html><noscript><title>ns</title></noscript><p>x</p>", format!("<!doctype html>{TDC}<noscript></noscript><p>x</p>")),
        // A noscript that runs to the page's end: read to there.
        ("<html><head><title>Old</title><noscript><meta name=\"description\" content=\"ns\">", format!("<html><head>{TDC}<noscript>")),
        ("<html><head><title>Old</title><noscript><title>ns", format!("<html><head>{TDC}<noscript>")),
        ("<html><head><title>Old</title><noscript><title>ns</title", format!("<html><head>{TDC}<noscript>")),
        ("<head><noscript><img src=px></noscript>", format!("<head>{TDC}<noscript><img src=px></noscript>")),
        ("<html><head><title>Old</title><noscript><", format!("<html><head>{TDC}<noscript><")),
        ("<html><head><title>Old</title><noscript><img src=", format!("<html><head>{TDC}<noscript><img src=")),
        ("<html><head><title>Old</title><noscript>", format!("<html><head>{TDC}<noscript>")),
        // There a title, then `</head>` and an element no head holds: read alike with it or without.
        ("<html><head><noscript><title>Old</title><meta name=\"description\" content=\"old\"></head><body>b</body></html>", format!("<html><head>{TDC}<noscript></head><body>b</body></html>")),
    ] {
        assert_eq!(with_head(page, &h), want, "{:?}", page);
    }
    // The limit and one past it: a title whose end tag is the noscript's last, cut; one whose end
    // tag is after `</noscript>` (its text holds it with scripting off), left, as is a meta whose
    // value says `</noscript>` (the text after it, `">`, ends the head with scripting on).
    let d = said_head(&[("title", "T"), ("description", "D")]);
    assert_eq!(with_head(&doc("<noscript><title>ns</title></noscript><title>Old</title>"), &d), doc(&format!("{TD}<noscript></noscript>")));
    assert_eq!(with_head(&doc("<noscript><title></noscript></title><title>Old</title>"), &d), doc(&format!("{TD}<noscript><title></noscript></title>")));
    assert_eq!(
        with_head(&doc("<noscript><meta name=\"description\" content=\"</noscript>\"></noscript><title>Old</title>"), &d),
        doc(&format!("{TD}<noscript><meta name=\"description\" content=\"</noscript>\"></noscript><title>Old</title>"))
    );
    // The scripts the head is written with: none of a noscript's (text, with scripting on).
    let s = quillo::head::head_scripts(b"<html><head><noscript><script>ns()</script></noscript><script>a()</script></head></html>");
    assert_eq!(s, ["<script>a()</script>"]);
}
