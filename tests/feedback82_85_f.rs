//! FEEDBACK85.md (slice F of FEEDBACK82-85; branch feedback82-85): what moving Candy Studio onto
//! Quillo found. Q-1340: `files()` through a link, logged and let out by `--allow-path` as
//! `file()` is; Q-1341: `read_file`'s refusal names `--allow-path`; Q-1342: `save:` on the http
//! calls; Q-1344: a plugin function's own `/// effects:` line; Q-1345: `"""`; Q-1346: the MCP
//! door's `_meta` refusal; Q-1347: `quillo mcp --serve`; Q-1348: a host from the operator's
//! settings.

mod support;

use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use support::{port_in_log, send, temp_dir, Guard};

/// `quillo` with `args` in `dir`, ended after `secs`: its exit code and what it wrote.
fn quillo_for(dir: &Path, args: &[&str], secs: u64) -> (Option<i32>, String) {
    let log = dir.join("run.log");
    let file = std::fs::File::create(&log).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_SECRET", "test secret for the tests, 32 bytes or more")
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
    (code, std::fs::read_to_string(&log).unwrap_or_default())
}

fn quillo(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    quillo_for(dir, args, 60)
}

/// `quillo run` of `main` in `dir` on a port the system picks, its stderr in `log`: the guard and
/// the port.
fn serve(dir: &Path, main: &str, opts: &[&str], log: &str) -> (Guard, u16) {
    let log = dir.join(log);
    let file = std::fs::File::create(&log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", main, "--port", "0", "--workers", "8"])
        .args(opts)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        .env("QUILLO_SECRET", "test secret for the tests, 32 bytes or more")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(file)
        .spawn()
        .unwrap();
    let mut g = Guard::new(child);
    let port = port_in_log(&mut g.0, &log, Duration::from_secs(30));
    (g, port)
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

// ---- Q-1340: files() through a link ----------------------------------------------------------

/// An app with `engine -> ../shared`, as the report's two apps had.
fn linked_app(name: &str) -> std::path::PathBuf {
    let d = temp_dir(name);
    std::fs::create_dir_all(d.join("shared/web/docs")).unwrap();
    std::fs::create_dir_all(d.join("other")).unwrap();
    std::fs::create_dir_all(d.join("app/own")).unwrap();
    std::fs::write(d.join("shared/lib.qwe"), "public fn hello() = \"from shared\"\n").unwrap();
    std::fs::write(d.join("shared/web/x.js"), "x").unwrap();
    std::fs::write(d.join("shared/web/ñ é.js"), "unicode").unwrap();
    std::fs::write(d.join("shared/web/.env"), "SECRET=1").unwrap();
    std::fs::write(d.join("shared/web/lib.qwe"), "let a = 1").unwrap();
    std::fs::write(d.join("shared/web/docs/index.html"), "<p>docs</p>").unwrap();
    std::fs::write(d.join("shared/web/404.html"), "<p>missing</p>").unwrap();
    std::fs::write(d.join("other/y.js"), "y").unwrap();
    std::fs::write(d.join("app/own/z.js"), "z").unwrap();
    std::os::unix::fs::symlink("../shared", d.join("app/engine")).unwrap();
    // A link inside the allowed folder that leads to a folder not allowed.
    std::os::unix::fs::symlink("../../other", d.join("shared/web/elsewhere")).unwrap();
    std::fs::write(
        d.join("app/main.qwe"),
        "use \"engine/lib.qwe\"\nroute GET / = hello()\nroute GET /e/*path = files(\"engine/web\", path)\nroute GET /own/*path = files(\"own\", path)\nroute GET /one = file(\"engine/web/x.js\")\n",
    )
    .unwrap();
    d
}

#[test]
fn files_through_a_link_says_why_and_takes_allow_path() {
    let d = linked_app("f85-files-link");
    let app = d.join("app");
    // No option: refused, as before (Q-475), and now said in the log with the way out.
    let (g, port) = serve(&app, "main.qwe", &[], "a.log");
    assert_eq!(send(port, "GET", "/", &[], "").body, "from shared");
    assert_eq!(send(port, "GET", "/e/x.js", &[], "").status, 404);
    assert_eq!(send(port, "GET", "/own/z.js", &[], "").body, "z", "a folder of the program's own is served as it was");
    drop(g);
    let log = read(&app.join("a.log"));
    assert!(log.contains("engine/web/x.js leads out of the program's folder through a link") && log.contains("`--allow-path FOLDER` lets the program reach a folder outside"), "{}", log);
    // With --allow-path: served, as `file()` serves it; its hidden files stay hidden.
    let (g, port) = serve(&app, "main.qwe", &["--allow-path", "../shared"], "b.log");
    assert_eq!(send(port, "GET", "/e/x.js", &[], "").body, "x");
    assert_eq!(send(port, "GET", "/one", &[], "").body, "x");
    assert_eq!(send(port, "GET", "/e/%C3%B1%20%C3%A9.js", &[], "").body, "unicode");
    assert_eq!(send(port, "GET", "/e/docs/", &[], "").body, "<p>docs</p>", "an index");
    let miss = send(port, "GET", "/e/none.js", &[], "");
    assert_eq!((miss.status, miss.body.as_str()), (404, "<p>missing</p>"), "the folder's page for a miss");
    for kept in ["/e/.env", "/e/lib.qwe", "/e/elsewhere/y.js", "/e/../lib.qwe", "/e/%2e%2e/lib.qwe"] {
        assert_eq!(send(port, "GET", kept, &[], "").status, 404, "{}", kept);
    }
    drop(g);
    // A folder named that is not the one the link leads to lets nothing out.
    let (g, port) = serve(&app, "main.qwe", &["--allow-path", "../other"], "c.log");
    assert_eq!(send(port, "GET", "/e/x.js", &[], "").status, 404);
    drop(g);
    assert!(read(&app.join("c.log")).contains("leads out of the program's folder through a link"));
}

// ---- Q-1341: the refusal names --allow-path --------------------------------------------------

#[test]
fn a_path_outside_names_the_way_out() {
    let d = temp_dir("f85-way-out");
    std::fs::create_dir_all(d.join("app")).unwrap();
    std::fs::create_dir_all(d.join("tuno")).unwrap();
    std::fs::write(d.join("tuno/.env"), "KEY=1\n").unwrap();
    std::fs::write(
        d.join("app/t.qwe"),
        "print(read_file(\"../tuno/.env\"))\nprint(write_file(\"../tuno/x.txt\", \"x\"))\nprint(delete_file(\"../tuno/x.txt\"))\nprint(read_file(\"/etc/hosts-none\"))\nprint(read_file(\"..\"))\nprint(read_file(\"\"))\nprint(delete_file(\".\"))\nprint(read_file(\"../ñ/é.txt\"))\nprint(read_file(\"sub/../../tuno/.env\"))\n",
    )
    .unwrap();
    let app = d.join("app");
    let (code, out) = quillo(&app, &["run", "t.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let lines: Vec<&str> = out.lines().collect();
    let way = |dir: &str, verb: &str| format!("; `--allow-path {}` (on `quillo run` or `quillo mcp`) lets the program {} it\")", dir, verb);
    assert_eq!(lines[0], format!("err(\"read_file: path must stay inside the program's folder: ../tuno/.env{}", way("../tuno", "read")));
    assert_eq!(lines[1], format!("err(\"write_file: path must stay inside the program's folder: ../tuno/x.txt{}", way("../tuno", "write")));
    assert_eq!(lines[2], format!("err(\"delete_file: path must be inside the program's folder: ../tuno/x.txt{}", way("../tuno", "delete")));
    assert_eq!(lines[3], format!("err(\"read_file: path must stay inside the program's folder: /etc/hosts-none{}", way("/etc", "read")));
    assert_eq!(lines[4], format!("err(\"read_file: path must stay inside the program's folder: ..{}", way("..", "read")));
    // Nothing to name: no way out said.
    assert_eq!(lines[5], "err(\"read_file: path must stay inside the program's folder: \")");
    assert_eq!(lines[6], "err(\"delete_file: path must be inside the program's folder: .\")");
    assert_eq!(lines[7], format!("err(\"read_file: path must stay inside the program's folder: ../ñ/é.txt{}", way("../ñ", "read")));
    assert!(lines[8].contains("`--allow-path sub/../../tuno`"), "{}", lines[8]);
    // The way it names works.
    let (code, out) = quillo(&app, &["run", "t.qwe", "--allow-path", "../tuno"]);
    assert_eq!(code, Some(0), "{}", out);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(&lines[..4], ["KEY=1", "", "null", "null"], "{}", out);
    assert!(!d.join("tuno/x.txt").exists());
}

// ---- Q-1342: save: on the http calls -----------------------------------------------------------

/// A server of bytes: a binary file, a miss, a long text, a redirect, an empty answer.
fn bytes_server(d: &Path) -> (Guard, u16, Vec<u8>) {
    let src = d.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let blob: Vec<u8> = (0..3000u32).map(|i| (i * 7 % 256) as u8).collect();
    std::fs::write(src.join("blob.bin"), &blob).unwrap();
    std::fs::write(
        src.join("main.qwe"),
        "route GET /blob = file(\"blob.bin\")\nroute POST /blob = file(\"blob.bin\")\nroute GET /missing = respond(404, \"no such clip\")\nroute GET /big = repeat(\"x\", 5000)\nroute GET /moved = redirect(\"/blob\")\nroute GET /none = respond(204, \"\")\n",
    )
    .unwrap();
    let (g, port) = serve(&src, "main.qwe", &[], "srv.log");
    (g, port, blob)
}

#[test]
fn save_writes_the_answer_to_a_file() {
    let d = temp_dir("f85-save");
    let (_srv, port, blob) = bytes_server(&d);
    let cli = d.join("cli");
    std::fs::create_dir_all(cli.join("cache")).unwrap();
    std::fs::write(cli.join("cache/keep.bin"), "old").unwrap();
    let u = format!("http://127.0.0.1:{}", port);
    let o = "public_only: false";
    let src = format!(
        r#"let r = http.get("{u}/blob", {{}}, {{ save: "cache/a.bin", {o} }})
print([r.status, r.ok, r.file, r.bytes, r.body, r.headers["content-type"]])
let m = http.get("{u}/missing", {{}}, {{ save: "cache/keep.bin", {o} }})
print([m.status, m.ok, m.file, m.bytes, m.body])
print(http.get("{u}/big", {{}}, {{ save: "cache/keep.bin", max_bytes: 4999, {o} }}))
print(http.get("{u}/big", {{}}, {{ save: "cache/big.bin", max_bytes: 5000, {o} }}).bytes)
print(http.get("{u}/blob", {{}}, {{ save: "cache/one.bin", max_bytes: 1, {o} }}))
let mv = http.get("{u}/moved", {{}}, {{ save: "cache/mv.bin", {o} }})
print([mv.status, mv.bytes])
let nr = http.get("{u}/moved", {{}}, {{ save: "cache/nr.bin", redirects: 0, {o} }})
print([nr.status, nr.file, nr.bytes])
print(http.get("{u}/none", {{}}, {{ save: "cache/empty.bin", {o} }}).bytes)
print(http.get("{u}/blob", {{}}, {{ save: "new/ñ é.bin", {o} }}).file)
print(http.post("{u}/blob", "hi", {{}}, {{ save: "cache/p.bin", {o} }}).bytes)
print(http.request("GET", "{u}/blob", null, {{}}, {{ save: "cache/q.bin", {o} }}).bytes)
print(http.get("{u}/blob", {{}}, {{ save: "../x.bin", {o} }}))
print(http.get("{u}/blob", {{}}, {{ save: "t.qwe", {o} }}))
print(http.get("{u}/blob", {{}}, {{ save: "cache", {o} }}))
print(http.get("http://127.0.0.1:1/x", {{}}, {{ save: "cache/dead.bin", {o} }}).error)
print(http.get("{u}/blob", {{}}, {{ save: null, {o} }}).body == null)
"#
    );
    std::fs::write(cli.join("t.qwe"), &src).unwrap();
    let (code, out) = quillo(&cli, &["check", "t.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let (code, out) = quillo(&cli, &["run", "t.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "[200, true, \"cache/a.bin\", 3000, null, \"application/octet-stream\"]", "{}", out);
    assert_eq!(std::fs::read(cli.join("cache/a.bin")).unwrap(), blob, "the bytes as sent, none replaced");
    // Not a 2xx: not saved, the file there kept, the answer's text given.
    assert_eq!(lines[1], "[404, false, null, 0, \"no such clip\"]");
    // Past max_bytes by one: an error, the file there kept; at it: written.
    assert!(lines[2].contains("the answer is longer than 4999 bytes, the most `save` writes"), "{}", lines[2]);
    assert_eq!(read(&cli.join("cache/keep.bin")), "old");
    assert_eq!(lines[3], "5000");
    assert!(lines[4].contains("longer than 1 bytes"), "{}", lines[4]);
    assert!(!cli.join("cache/one.bin").exists());
    // A redirect followed, the last answer saved; one not followed is not a 2xx.
    assert_eq!(lines[5], "[200, 3000]");
    assert_eq!(lines[6], "[303, null, 0]");
    assert!(!cli.join("cache/nr.bin").exists());
    assert_eq!(lines[7], "0");
    assert_eq!(std::fs::read(cli.join("cache/empty.bin")).unwrap(), b"");
    assert_eq!(lines[8], "new/ñ é.bin");
    assert_eq!(std::fs::read(cli.join("new/ñ é.bin")).unwrap(), blob);
    assert_eq!(lines[9], "3000");
    assert_eq!(lines[10], "3000");
    assert_eq!(lines[11], "err(\"http.get: save: path must stay inside the program's folder: ../x.bin; `--allow-path ..` (on `quillo run` or `quillo mcp`) lets the program write it\")");
    assert_eq!(lines[12], "err(\"http.get: save: will not write t.qwe: it is one of the program's own files\")");
    assert_eq!(lines[13], "err(\"http.get: save: cache is a folder\")");
    assert!(lines[14].starts_with("http: http://127.0.0.1:1/x:"), "{}", lines[14]);
    assert!(!cli.join("cache/dead.bin").exists());
    assert_eq!(lines[15], "false", "save: null reads the answer as before");
    // No file half-written is left beside them.
    for dir in ["cache", "new"] {
        let left: Vec<String> = std::fs::read_dir(cli.join(dir)).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".part")).collect();
        assert!(left.is_empty(), "{:?}", left);
    }
    // --allow-path lets it write outside, as write_file.
    std::fs::create_dir_all(d.join("out")).unwrap();
    std::fs::write(cli.join("o.qwe"), format!("print(http.get(\"{u}/blob\", {{}}, {{ save: \"../out/a.bin\", {o} }}).bytes)\n")).unwrap();
    let (code, out) = quillo(&cli, &["run", "o.qwe", "--allow-path", "../out"]);
    assert_eq!((code, out.trim()), (Some(0), "3000"), "{}", out);
    assert_eq!(std::fs::read(d.join("out/a.bin")).unwrap(), blob);
}

#[test]
fn save_is_checked_and_held_to_the_policy() {
    let d = temp_dir("f85-save-check");
    // fs as well as http, written in the call; a variable's options are held as the call runs.
    std::fs::write(
        d.join("e.qwe"),
        "fn a() with http, fs = http.get(\"https://example.com/a\", {}, { save: \"a.bin\" })\nfn b() with http = http.get(\"https://example.com/a\", {}, { save: \"a.bin\" })\nfn c() with http = http.get(\"https://example.com/a\", {}, { save: null })\nfn d() with http = http.post(\"https://example.com/a\", \"x\", {}, { save: \"a.bin\" })\n",
    )
    .unwrap();
    let (code, out) = quillo(&d, &["check", "e.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("e.qwe:2: error: `http.get` needs the fs effect, but `b` is declared `with http`"), "{}", out);
    assert!(out.contains("e.qwe:4: error: `http.post` needs the fs effect, but `d` is declared `with http`"), "{}", out);
    assert!(!out.contains("e.qwe:1:") && !out.contains("e.qwe:3:"), "{}", out);
    // At run time too, with the options in a variable.
    std::fs::write(d.join("r.qwe"), "fn b(o: map) with http = http.get(\"http://127.0.0.1:1/a\", {}, o)\nprint(b({ public_only: false }).error)\nprint(b({ save: \"a.bin\", public_only: false }))\n").unwrap();
    let (code, out) = quillo(&d, &["run", "r.qwe", "--no-check"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("http: http://127.0.0.1:1/a:") && out.contains("`http.get` needs the fs effect"), "{}", out);
    // Mistakes written in the options.
    std::fs::write(
        d.join("m.qwe"),
        "let a = http.get(\"https://example.com\", {}, { max_bytes: 10 })\nlet b = http.get(\"https://example.com\", {}, { save: \"a\", cache: 60 })\nlet c = http.get(\"https://example.com\", {}, { save: 5 })\nlet e = http.get(\"https://example.com\", {}, { save: \"a\", max_bytes: 0 })\n",
    )
    .unwrap();
    for (line, said) in [
        (1, "`max_bytes` goes with `save`"),
        (2, "`cache` keeps answers in memory and `save` writes one to its file"),
        (3, "`save` is the path of the file to write the answer to, got 5"),
        (4, "`max_bytes` is the most bytes `save` writes, a whole number more than 0, got 0"),
    ] {
        let src = read(&d.join("m.qwe")).lines().nth(line - 1).unwrap().to_string();
        std::fs::write(d.join("one.qwe"), src).unwrap();
        let (code, out) = quillo(&d, &["run", "one.qwe", "--no-check"]);
        assert_eq!(code, Some(1), "{}", out);
        assert!(out.contains(said), "{}: {}", line, out);
    }
    std::fs::write(d.join("s.qwe"), "http.stream(\"https://example.com\", (p) => true, { save: \"a\" })\n").unwrap();
    let (_, out) = quillo(&d, &["run", "s.qwe", "--no-check"]);
    assert!(out.contains("`http.stream`: `save` is for the calls that read an answer whole"), "{}", out);
    // In a component, the browser writes no file of the server's.
    std::fs::write(d.join("c.qwe"), "component C() {\n  state s = 0\n  <button onclick={() => http.get(\"/a\", {}, { save: \"a.bin\" })}>go</button>\n}\nroute GET / = <C/>\n").unwrap();
    let (code, out) = quillo(&d, &["check", "c.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("which writes no file of the server's: `save` is for a `server fn`"), "{}", out);
    // The policy's hosts, and its effects.
    std::fs::write(d.join("hosts.toml"), "[http]\nhosts = [\"api.example.com\"]\n").unwrap();
    std::fs::write(d.join("h.qwe"), "print(http.get(\"http://127.0.0.1:1/a\", {}, { save: \"h.bin\", public_only: false }).error)\n").unwrap();
    let (code, out) = quillo(&d, &["run", "h.qwe", "--policy", "hosts.toml"]);
    assert_eq!(code, Some(0), "{}", out);
    assert!(out.contains("127.0.0.1") && !d.join("h.bin").exists(), "{}", out);
    std::fs::write(d.join("fx.toml"), "[effects]\nallow = [\"http\"]\n").unwrap();
    let (code, out) = quillo(&d, &["check", "h.qwe", "--policy", "fx.toml"]);
    assert_ne!(code, Some(0), "{}", out);
    assert!(out.contains("`http.get` needs the fs effect, which"), "{}", out);
}

// ---- Q-1344: a function's own effects line ---------------------------------------------------

const PLUGIN: &str = "//! effects: fs, net
/// Only reads a file.
/// effects: fs
pub fn size_of(path: &str) -> i64 { std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(-1) }
/// Reaches out.
pub fn fetch_it(url: &str) -> String { url.to_string() }
/// Pure.
/// effects: none
pub fn plus_one(x: i64) -> i64 { x + 1 }
";

#[test]
fn a_function_line_narrows_the_files() {
    let p = quillo::native::parse(PLUGIN);
    let fx = |n: &str| p.exports.iter().find(|e| e.name == n).unwrap();
    let bit = quillo::ast::effect_bit;
    assert_eq!(fx("size_of").effects, bit("fs"));
    assert_eq!(fx("fetch_it").effects, bit("fs") | bit("net"), "no line: the file's");
    assert_eq!(fx("plus_one").effects, 0);
    assert_eq!(fx("size_of").doc, "Only reads a file.", "the line is not its documentation");
    assert!(p.errors.is_empty() && p.problems.is_empty(), "{:?} {:?}", p.errors, p.problems);
    // Wider: an error naming both, and the file's effects kept.
    let p = quillo::native::parse("//! effects: fs\n/// effects: fs, env, net\npub fn w(x: i64) -> i64 { x }\n");
    assert_eq!(p.exports[0].effects, bit("fs"));
    assert_eq!(p.errors, vec![(2, "`w`'s `/// effects: fs, env, net` names env, net, which the file's `//! effects: fs` does not: a function's line narrows its file's, never widens it".to_string())]);
    let p = quillo::native::parse("//! effects: none\n/// effects: fs\npub fn w(x: i64) -> i64 { x }\n");
    assert!(p.errors[0].1.contains("the file's `//! effects: none` does not"), "{:?}", p.errors);
    // A file with no line may do anything: a function's line says less.
    let p = quillo::native::parse("/// effects: time\npub fn w(x: i64) -> i64 { x }\n");
    assert_eq!((p.exports[0].effects, p.errors.len()), (bit("time"), 0));
    // Twice, and a word that is no effect.
    let p = quillo::native::parse("//! effects: fs, net\n/// effects: fs\n/// effects: net\npub fn w(x: i64) -> i64 { x }\n/// effects: disk\npub fn v(x: i64) -> i64 { x }\n");
    assert!(p.errors[0].1.contains("says its effects twice"), "{:?}", p.errors);
    assert_eq!(p.exports[0].effects, bit("fs"));
    assert!(p.problems.iter().any(|(_, m)| m.contains("`disk` is not an effect")), "{:?}", p.problems);
    // The word in a sentence is not the line.
    let p = quillo::native::parse("//! effects: fs\n/// It has no effects: none at all.\npub fn w(x: i64) -> i64 { x }\n");
    assert_eq!(p.exports[0].effects, bit("fs"));
}

#[test]
fn callers_need_only_the_functions_effects() {
    let d = temp_dir("f85-fn-effects");
    std::fs::write(d.join("fx.rs"), PLUGIN).unwrap();
    std::fs::write(
        d.join("main.qwe"),
        "import fx.rs use size_of, fetch_it, plus_one\nfn file_size() -> int with fs = size_of(\"fx.rs\")\nfn pure() -> int with time = plus_one(1)\nfn far() -> str with fs = fetch_it(\"x\")\n",
    )
    .unwrap();
    let (code, out) = quillo(&d, &["check", "main.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("main.qwe:4: error: `fetch_it` needs the net effect, but `far` is declared `with fs`"), "{}", out);
    assert!(!out.contains("main.qwe:2:") && !out.contains("main.qwe:3:"), "{}", out);
    std::fs::write(d.join("wide.rs"), "//! effects: fs\n/// effects: fs, net\npub fn w(x: i64) -> i64 { x }\n").unwrap();
    std::fs::write(d.join("w.qwe"), "import wide.rs use w\nprint(w(1))\n").unwrap();
    let (code, out) = quillo(&d, &["check", "w.qwe"]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains("wide.rs:2: error: `w`'s `/// effects: fs, net` names net, which the file's `//! effects: fs` does not"), "{}", out);
    // As it runs (built: slow), unchecked: the narrowed function from `with fs`, the other refused.
    std::fs::write(d.join("r.qwe"), "import fx.rs use size_of, fetch_it\nfn file_size() -> int with fs = size_of(\"fx.rs\")\nfn far() -> str with fs = fetch_it(\"x\")\nprint(file_size())\nprint(far())\n").unwrap();
    let (code, out) = quillo_for(&d, &["run", "r.qwe", "--no-check"], 600);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(&format!("{}\n", PLUGIN.len())), "{}", out);
    assert!(out.contains("`fetch_it` needs the net effect"), "{}", out);
}

// ---- Q-1345: """ ------------------------------------------------------------------------------

#[test]
fn triple_quotes_point_to_a_backtick_string() {
    let d = temp_dir("f85-triple");
    let said = |q: &str| format!("error: `{}` is not a string here: text over several lines is a backtick string, `line one ...` (`${{` interpolates)", q);
    for (src, q) in [
        ("fn guide() -> str = \"\"\"\nline one\n\"\"\"\n", "\"\"\""),
        ("let s = '''\nx\n'''\n", "'''"),
        ("let s = \"\"\"one line\"\"\"\n", "\"\"\""),
        ("let s = \"\"\"\"\n", "\"\"\""),
        ("let s = \"\"\"é\"\"\"\n", "\"\"\""),
    ] {
        std::fs::write(d.join("t.qwe"), src).unwrap();
        let (code, out) = quillo(&d, &["check", "t.qwe"]);
        assert_eq!(code, Some(1), "{}", out);
        assert!(out.contains(&format!("t.qwe:1: {}", said(q))), "{:?}: {}", src, out);
    }
    // What must keep working: an empty string, beside others, and a quote in one.
    std::fs::write(d.join("ok.qwe"), "let a = \"\"\nlet b = \"\" + \"x\"\nlet c = [\"\", \"\"]\nlet e = \"\\\"\"\nlet f = ''\nprint([a, b, len(c), e, f])\n").unwrap();
    let (code, out) = quillo(&d, &["run", "ok.qwe"]);
    assert_eq!((code, out.trim()), (Some(0), "[\"\", \"x\", 2, \"\\\"\", \"\"]"), "{}", out);
    // A space between: the old message.
    std::fs::write(d.join("sp.qwe"), "let s = \"\" \"x\"\n").unwrap();
    let (_, out) = quillo(&d, &["check", "sp.qwe"]);
    assert!(out.contains("expected end of statement, found a string") && !out.contains("backtick"), "{}", out);
}

// ---- Q-1346 and Q-1347: the MCP door ---------------------------------------------------------

const META: &str = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}"#;

/// `quillo mcp` with `args` in `dir`, its stdin piped: the child (stderr in `err`, stdout in `out`).
fn mcp(dir: &Path, args: &[&str]) -> Child {
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("mcp")
        .args(args)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        .env("QUILLO_SECRET", "test secret for the tests, 32 bytes or more")
        .stdin(Stdio::piped())
        .stdout(std::fs::File::create(dir.join("out")).unwrap())
        .stderr(std::fs::File::create(dir.join("err")).unwrap())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    child
}

fn wait_exit(child: &mut Child, secs: u64) -> Option<i32> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return s.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_meta_refusal_names_both_keys_and_values() {
    let d = temp_dir("f85-meta");
    std::fs::write(d.join("m.qwe"), "/// Pong.\nfn mcp ping() -> str = \"pong\"\n").unwrap();
    let mut c = mcp(&d, &["m.qwe"]);
    let mut stdin = c.stdin.take().unwrap();
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"ping","arguments":{{}}}}}}"#).unwrap();
    // The guide's line, as it is to be copied.
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"ping","arguments":{{}},{}}}}}"#, META).unwrap();
    drop(stdin);
    assert_eq!(wait_exit(&mut c, 30), Some(0), "{}", read(&d.join("err")));
    let out = read(&d.join("out"));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"Invalid params: `_meta` must carry `io.modelcontextprotocol/protocolVersion` (\"2026-07-28\") and `io.modelcontextprotocol/clientCapabilities` ({})"}}"#);
    assert!(lines[1].contains(r#""structuredContent":{"result":"pong"}"#), "{}", lines[1]);
}

const SERVED: &str = "/// Writes a note, and says how many there are.
fn mcp note(text: str) -> int with db {
  db.exec(\"create table if not exists notes (t text)\")
  db.exec(\"insert into notes values (?)\", text)
  return db.one(\"select count(*) as n from notes\").n
}
route GET /slow with time = { sleep(1500); \"slow done\" }
route GET /notes with db = {
  db.exec(\"create table if not exists notes (t text)\")
  return str(db.one(\"select count(*) as n from notes\").n)
}
";

#[test]
fn mcp_serve_serves_the_routes_beside_the_door() {
    let d = temp_dir("f85-serve");
    std::fs::write(d.join("s.qwe"), SERVED).unwrap();
    let mut c = mcp(&d, &["s.qwe", "--serve", "--port", "0"]);
    let port = port_in_log(&mut c, &d.join("err"), Duration::from_secs(30));
    let mut stdin = c.stdin.take().unwrap();
    for (i, t) in ["a", "é"].iter().enumerate() {
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"note","arguments":{{"text":"{}"}},{}}}}}"#, i + 1, t, META).unwrap();
    }
    stdin.flush().unwrap();
    // One program's data: what the tool wrote, the route reads.
    let deadline = Instant::now() + Duration::from_secs(20);
    while read(&d.join("out")).lines().count() < 2 {
        assert!(Instant::now() < deadline, "{}", read(&d.join("err")));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(read(&d.join("out")).lines().nth(1).unwrap().contains(r#""structuredContent":{"result":2}"#), "{}", read(&d.join("out")));
    assert_eq!(send(port, "GET", "/notes", &[], "").body, "2");
    // The door over HTTP is there too, for a signed-in caller only.
    assert_eq!(send(port, "POST", "/_quillo/mcp", &[("content-type", "application/json")], "{}").status, 401);
    // stdin closing stops it as SIGTERM stops `quillo run`: the request in progress answered.
    let slow = std::thread::spawn(move || send(port, "GET", "/slow", &[], ""));
    std::thread::sleep(Duration::from_millis(400));
    drop(stdin);
    let answer = slow.join().unwrap();
    assert_eq!((answer.status, answer.body.as_str()), (200, "slow done"));
    assert_eq!(wait_exit(&mut c, 30), Some(0), "{}", read(&d.join("err")));
    assert!(read(&d.join("err")).contains("stopping once the requests in progress are answered"));
    assert!(!support::listening(port));
}

#[test]
fn mcp_serve_options_and_the_policy() {
    let d = temp_dir("f85-serve-opts");
    std::fs::write(d.join("s.qwe"), SERVED).unwrap();
    let (code, out) = quillo(&d, &["mcp", "s.qwe", "--port", "0"]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("`--port` goes with `--serve`"), "{}", out);
    let (code, out) = quillo(&d, &["mcp", "s.qwe", "--serve", "--watch"]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("`--watch` is not for `quillo mcp`") && out.contains("--serve [--port N]"), "{}", out);
    // Served as `quillo run` serves it: a policy's Unix socket only is refused here.
    std::fs::write(d.join("u.toml"), "[run]\nlisten = \"unix\"\n").unwrap();
    let (code, out) = quillo(&d, &["mcp", "s.qwe", "--serve", "--port", "0", "--policy", "u.toml"]);
    assert_eq!(code, Some(2), "{}", out);
    assert!(out.contains("has the program listen on a Unix socket only") && out.contains("quillo run --listen unix:PATH"), "{}", out);
    // The door alone still takes no port (and stdin at its end: it ends).
    let (code, out) = quillo(&d, &["mcp", "s.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    // A port taken: said, and no door.
    let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let p = held.local_addr().unwrap().port().to_string();
    let (code, out) = quillo(&d, &["mcp", "s.qwe", "--serve", "--port", &p]);
    assert_eq!(code, Some(1), "{}", out);
    assert!(out.contains(&format!("cannot listen on 127.0.0.1:{}", p)), "{}", out);
}

// ---- Q-1348: the operator's setting ----------------------------------------------------------

#[test]
fn a_host_from_env_is_the_operators() {
    let d = temp_dir("f85-operator");
    std::fs::write(
        d.join("e.qwe"),
        "let TUNO = env(\"TUNO_URL\", \"http://127.0.0.1:8940\").trim()
let BOTH = env(\"TUNO_URL\") ?? env(\"TTS_URL\") ?? \"http://127.0.0.1:8940\"
let H = { base: env(\"TUNO_URL\", \"http://127.0.0.1:8940\").lower() }
fn a1() with http = http.get(`${TUNO}/health`)
fn a2() with http = http.get(`${BOTH.trim()}/health`)
fn a3() with http = http.get(`${H.base}/x`)
fn a4(p: str) with http = http.get(`${TUNO}${p}`)
fn a5() with http = http.get(`${TUNO}/health`, {}, { public_only: false })
fn a6() with http {
  let TUNO = \"x\"
  return http.get(TUNO.trim())
}
let PLAIN = env(\"TUNO_URL\", \"http://127.0.0.1:8940\")
fn a7() with http = http.get(`${PLAIN}/health`)
let ÉTÉ = env(\"ÉTÉ_URL\", \"http://127.0.0.1:1\").trim()
fn a8() with http = http.get(ÉTÉ)
",
    )
    .unwrap();
    let (code, out) = quillo(&d, &["check", "e.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let operator = |line: u32, names: &str, s: &str| format!("e.qwe:{}: warning: the host `http.get` goes to comes from {}, the operator's setting{}: say {{ public_only: false }} as its options if it is meant to reach this machine or its network (or public_only: true, if not)", line, names, s);
    assert!(out.contains(&operator(4, "`TUNO_URL`", "")), "{}", out);
    assert!(out.contains(&operator(5, "`TUNO_URL` and `TTS_URL`", "s")), "{}", out);
    assert!(out.contains(&operator(6, "`TUNO_URL`", "")), "{}", out);
    assert!(out.contains(&operator(16, "`ÉTÉ_URL`", "")), "{}", out);
    // A visitor's value may be in it: as before.
    for line in [7, 11] {
        assert!(out.contains(&format!("e.qwe:{}: warning: the host `http.get` goes to comes from a value: if a visitor can choose it", line)), "{}: {}", line, out);
    }
    // Said either way, or fixed: nothing.
    for line in [8, 14] {
        assert!(!out.contains(&format!("e.qwe:{}:", line)), "{}: {}", line, out);
    }
}

// ---- Q-1342, found in review: a save is a write; the file's mode; parts left by a crash --------

#[test]
fn a_save_is_a_write_that_a_prefetch_or_a_move_does_not_make() {
    let d = temp_dir("f85-save-write");
    let (_srv, port, blob) = bytes_server(&d);
    let app = d.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(
        app.join("main.qwe"),
        format!(
            "moves_in_place()\nfn grab() with http, fs = http.get(\"http://127.0.0.1:{p}/blob\", {{}}, {{ save: \"out.bin\", public_only: false }})\nfn grab2() with http, fs = http.request(\"GET\", \"http://127.0.0.1:{p}/blob\", null, {{}}, {{ save: \"out2.bin\", public_only: false }})\nroute GET /p = <p>{{grab().bytes}}</p>\nroute GET /q = <p>{{grab2().bytes}}</p>\n",
            p = port
        ),
    )
    .unwrap();
    let (g, port) = serve(&app, "main.qwe", &[], "a.log");
    for (path, file) in [("/p", "out.bin"), ("/q", "out2.bin")] {
        let pre = send(port, "GET", path, &[("Sec-Purpose", "prefetch")], "");
        assert_ne!(pre.status, 200, "{}: {}", path, pre.body);
        let mv = send(port, "GET", path, &[("x-quillo-move", "1")], "");
        assert_eq!(mv.status, 503, "{}: {}", path, mv.body);
        assert!(!app.join(file).exists(), "{}: written by a prefetch or a move", path);
        // The real load saves it, once.
        let real = send(port, "GET", path, &[], "");
        assert!(real.status == 200 && real.body.starts_with("<p>3000</p>"), "{}: {} {}", path, real.status, real.body);
        assert_eq!(std::fs::read(app.join(file)).unwrap(), blob);
    }
    drop(g);
    assert!(read(&app.join("a.log")).contains("prefetch"), "{}", read(&app.join("a.log")));
    // `quillo check` reads it as a write, as write_file is, on a GET that reads who the visitor is.
    std::fs::write(app.join("c.qwe"), "route GET /s with http, fs {\n    let u = session().user\n    http.get(\"https://example.com/a\", {}, { save: \"s.bin\", public_only: true })\n    return \"ok\"\n}\n").unwrap();
    let (_, out) = quillo(&app, &["check", "c.qwe"]);
    assert!(out.contains("`http.get` with `save`") && out.contains("`session()`"), "{}", out);
    std::fs::write(app.join("n.qwe"), "route GET /s with http {\n    let u = session().user\n    http.get(\"https://example.com/a\", {}, { public_only: true })\n    return \"ok\"\n}\n").unwrap();
    let (_, out) = quillo(&app, &["check", "n.qwe"]);
    assert!(!out.contains("with `save`") && !out.contains("GET routes that read who the visitor is"), "a GET without save is no write: {}", out);
}

#[test]
fn a_saved_file_keeps_its_mode_and_old_parts_go() {
    use std::os::unix::fs::PermissionsExt;
    let d = temp_dir("f85-save-mode");
    let (_srv, port, blob) = bytes_server(&d);
    let cli = d.join("cli");
    std::fs::create_dir_all(&cli).unwrap();
    // A sticky (or setuid, setgid) target: the downloaded bytes keep only its read, write and run
    // bits (found in the second review).
    for (f, mode) in [("a.bin", 0o600), ("b.sh", 0o755), ("s.sh", 0o1755)] {
        std::fs::write(cli.join(f), "old").unwrap();
        std::fs::set_permissions(cli.join(f), std::fs::Permissions::from_mode(mode)).unwrap();
    }
    // Parts a crash left: an old one of this name goes; a new one, another name's, and one not of
    // the form stay.
    let old = cli.join(".a.bin.0123456789abcdef.part");
    let new = cli.join(".a.bin.fedcba9876543210.part");
    let other = cli.join(".c.bin.0123456789abcdef.part");
    let odd = cli.join(".a.bin.xyz.part");
    for f in [&old, &new, &other, &odd] {
        std::fs::write(f, "partial").unwrap();
    }
    let hour_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
    for f in [&old, &other, &odd] {
        std::fs::File::options().write(true).open(f).unwrap().set_modified(hour_ago).unwrap();
    }
    let u = format!("http://127.0.0.1:{}", port);
    std::fs::write(
        cli.join("t.qwe"),
        format!("print(http.get(\"{u}/blob\", {{}}, {{ save: \"a.bin\", public_only: false }}).bytes)\nprint(http.get(\"{u}/blob\", {{}}, {{ save: \"b.sh\", public_only: false }}).bytes)\nprint(http.get(\"{u}/blob\", {{}}, {{ save: \"fresh.bin\", public_only: false }}).bytes)\nprint(http.get(\"{u}/blob\", {{}}, {{ save: \"s.sh\", public_only: false }}).bytes)\n"),
    )
    .unwrap();
    let (code, out) = quillo(&cli, &["run", "t.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let mode = |f: &str| std::fs::metadata(cli.join(f)).unwrap().permissions().mode() & 0o777;
    assert_eq!((mode("a.bin"), mode("b.sh")), (0o600, 0o755));
    let full = |f: &str| std::fs::metadata(cli.join(f)).unwrap().permissions().mode() & 0o7777;
    assert_eq!(full("s.sh"), 0o755, "sticky, setuid and setgid are not copied");
    assert_eq!(std::fs::read(cli.join("a.bin")).unwrap(), blob);
    assert!(!old.exists());
    assert!(new.exists() && other.exists() && odd.exists());
    // A new file is made as write_file makes one.
    assert_eq!(mode("fresh.bin") & 0o600, 0o600);
}
