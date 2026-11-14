//! The `quillo` binary itself and the edge cases every web server meets (FEEDBACK31.md, Q-378 to
//! Q-394; branch feedback27-35-fixes): a log nobody can write lines into, a closed pipe, `.env`
//! lines said when skipped, options that are checked, `--with` stopped every way, links
//! followed, source files as Windows and the shell write them, a REPL fed from a pipe, unknown
//! commands, `local_path`, headers for caches and frames, files told apart by inode, `Host`, and
//! `normalize`.

mod support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};
use support::send;

fn dir(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback31-{}", name));
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

fn quillo(d: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quillo")).args(args).current_dir(d).stdin(Stdio::null()).output().unwrap()
}

fn text(o: &Output) -> (String, String) {
    (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}

struct Logged {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Logged {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Logged {
    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
    fn wait_for(&self, s: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let l = self.log();
            if l.contains(s) || Instant::now() > deadline {
                return l;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    }
}

fn serve(d: &Path, file: &str, args: &[&str]) -> Logged {
    let port = support::free_port();
    let log = d.join(format!("{}.log", port));
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join(file))
        .args(["--port", &port.to_string(), "--no-check"])
        .args(args)
        .env("QUILLO_SECRET", "test secret for the tests, long enough to sign")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let mut s = Logged { child, port, log };
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        if let Ok(Some(st)) = s.child.try_wait() {
            panic!("stopped: {}\n{}", st, s.log());
        }
        assert!(Instant::now() < deadline, "did not start:\n{}", s.log());
        std::thread::sleep(Duration::from_millis(30));
    }
    s
}

fn raw(port: u16, req: &[u8]) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(req).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

// ---- Q-378: the log ----------------------------------------------------------------------------

#[test]
fn what_a_visitor_sends_cannot_add_lines_to_the_log_or_reach_a_terminal() {
    let d = dir("log", &[("main.qwe", b"route GET /crash/:m = err(`bad \"${m}\"`)\n")]);
    for mode in ["text", "json"] {
        let s = serve(&d, "main.qwe", &["--log", mode]);
        send(s.port, "GET", "/crash/x%0Aquillo:%20error:%20GET%20%2Fadmin%20-%3E%20500", &[], "");
        send(s.port, "GET", "/crash/y%1b%5B2J", &[], "");
        raw(s.port, b"GE\x1b[2JT / HTTP/1.1\r\nHost: x\r\n\r\n");
        let log = s.wait_for("400");
        assert!(!log.contains('\x1b'), "{}: {:?}", mode, log);
        assert!(!log.lines().any(|l| l.starts_with("quillo: error: GET /admin")), "{}: {}", mode, log);
        if mode == "text" {
            assert!(log.contains("bad \"x\\nquillo: error: GET /admin -> 500\""), "{}", log);
            assert!(log.contains("bad \"y\\x1b[2J\""), "{}", log);
            assert!(log.contains("GE\\x1b[2JT / HTTP/1.1 -> 400 (bad request)"), "{}", log);
        } else {
            for l in log.lines().filter(|l| l.starts_with('{')) {
                serde_json::from_str::<serde_json::Value>(l).unwrap_or_else(|e| panic!("{}: {}", e, l));
            }
            assert!(log.contains("\"status\":400"), "{}", log);
        }
    }
}

// ---- Q-379: a closed pipe ----------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn a_closed_pipe_ends_a_script_quietly_and_a_server_goes_on() {
    let d = dir("pipe", &[("many.qwe", b"for i in 0..200000 { print(i) }\n"), ("srv.qwe", b"route GET / { print(\"visit\"); \"ok\" }\n")]);
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("many.qwe")).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut first = [0u8; 2];
    c.stdout.as_mut().unwrap().read_exact(&mut first).unwrap();
    drop(c.stdout.take());
    let o = c.wait_with_output().unwrap();
    assert_eq!(&first, b"0\n");
    assert_eq!(std::os::unix::process::ExitStatusExt::into_raw(o.status) >> 8, 141, "{:?}", o.status);
    assert!(!String::from_utf8_lossy(&o.stderr).contains("panicked"), "{}", String::from_utf8_lossy(&o.stderr));

    let port = support::free_port();
    let mut srv = support::Guard::new(
        Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(d.join("srv.qwe"))
            .args(["--port", &port.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    while !support::listening(port) {
        std::thread::sleep(Duration::from_millis(30));
    }
    drop(srv.0.stdout.take());
    let codes: Vec<u16> = (0..4).map(|_| send(port, "GET", "/", &[], "").status).collect();
    assert_eq!(codes, [200, 200, 200, 200]);
}

// ---- Q-380: .env ----------------------------------------------------------------------------

#[test]
fn a_dotenv_with_a_byte_order_mark_and_values_over_lines_is_read_and_a_skipped_line_said() {
    let d = dir(
        "dotenv",
        &[
            (".env", b"\xef\xbb\xbfFIRST=one\nMULTI=\"line1\nline2\"\nAFTER=ok\nCOLON: no\nPEM='-----BEGIN-----\nabc\n-----END-----'\nOPEN=\"never closed\n"),
            ("main.qwe", b"for k in [\"FIRST\", \"MULTI\", \"AFTER\", \"COLON\", \"PEM\", \"OPEN\"] { print(k, json.stringify(env(k))) }\n"),
        ],
    );
    let (out, err) = text(&quillo(&d, &["run", "main.qwe"]));
    // A quote never closed is skipped as it is said to be, its value not kept (FEEDBACK55.md,
    // Q-839; branch feedback55).
    assert_eq!(out, "FIRST \"one\"\nMULTI \"line1\\nline2\"\nAFTER \"ok\"\nCOLON null\nPEM \"-----BEGIN-----\\nabc\\n-----END-----\"\nOPEN null\n", "{}", err);
    assert!(err.contains(".env:5: not KEY=value, skipped"), "{}", err);
    assert!(err.contains(".env:9: its quote is not closed, skipped"), "{}", err);
}

// ---- Q-381, Q-389: the command line --------------------------------------------------------

#[test]
fn options_are_checked_and_a_stray_word_is_told_where_it_goes() {
    let d = dir("options", &[("main.qwe", b"route GET / = \"x\"\n"), ("a.qwe", b"print(args())\n"), ("tool", b"print(1)\n")]);
    for (args, said) in [
        (&["run", "main.qwe", "--service-name", "--watch"][..], "`--service-name` needs a value, and `--watch` is an option"),
        (&["run", "main.qwe", "--with", "--watch"][..], "`--with` needs a value"),
        (&["run", "main.qwe", "--port", "abc"][..], "--port is a number from 1 to 65535"),
        (&["run", "main.qwe", "--port", "70000"][..], "--port is a number"),
        (&["run", "main.qwe", "--port", "-1"][..], "--port is a number"),
        (&["run", "main.qwe", "--tls-cert", "none.pem", "--tls-key", "none.pem"][..], "--tls-cert none.pem: there is no such file"),
        (&["run", "a.qwe", "x", "y"][..], "a program's own arguments go after `--`"),
        (&["chek", "main.qwe"][..], "unknown command `chek` (did you mean `check`?)"),
        (&["frobnicate"][..], "unknown command `frobnicate`"),
        (&["tool"][..], "to run the file: quillo run tool"),
    ] {
        let o = quillo(&d, args);
        assert_eq!(o.status.code(), Some(2), "{:?}: {:?}", args, text(&o));
        assert!(text(&o).1.contains(said), "{:?}: {}", args, text(&o).1);
        assert!(!text(&o).1.contains("usage:"), "{:?}: the whole usage is not printed", args);
    }
    // `--` before the program ends the options; after it, it starts the program's arguments.
    assert_eq!(text(&quillo(&d, &["run", "--", "a.qwe", "x"])).0, "[\"x\"]\n");
    assert_eq!(text(&quillo(&d, &["run", "a.qwe", "--", "x", "--port"])).0, "[\"x\", \"--port\"]\n");
    // A program's name in capitals is still a program.
    std::fs::write(d.join("UPPER.QWE"), "print(\"upper\")\n").unwrap();
    assert_eq!(text(&quillo(&d, &["UPPER.QWE"])).0, "upper\n");
}

#[test]
fn version_names_the_commit_or_says_it_is_unknown() {
    let o = quillo(Path::new("."), &["--version"]);
    let v = text(&o).0;
    assert!(v.starts_with("quillo ") && v.contains('('), "{}", v);
}

// ---- Q-382: what runs beside the program ---------------------------------------------------

#[cfg(unix)]
#[test]
fn a_with_command_stops_with_a_quillo_stopped_while_it_loads_or_killed() {
    for sig in ["-TERM", "-KILL"] {
        let tag = format!("{}{}", 31000 + support::free_port() % 1000, &sig[1..2]);
        let d = dir(&format!("with{}", sig), &[("main.qwe", b"sleep(4000)\nroute GET / = \"up\"\n")]);
        let cmd = format!("sleep {}.{}", tag.trim_end_matches(char::is_alphabetic), if sig == "-TERM" { 1 } else { 2 });
        let mut q = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(d.join("main.qwe"))
            .args(["--port", &support::free_port().to_string(), "--no-check", "--with", &cmd])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let running = || String::from_utf8_lossy(&Command::new("pgrep").args(["-f", &format!("^{}$", cmd)]).output().unwrap().stdout).trim().to_string();
        let deadline = Instant::now() + Duration::from_secs(5);
        while running().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!running().is_empty(), "the command did not start");
        Command::new("kill").args([sig, &q.id().to_string()]).status().unwrap();
        let _ = q.wait();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !running().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(running().is_empty(), "{} left `{}` running", sig, cmd);
    }
}

// ---- Q-383: links ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn a_link_pointed_elsewhere_is_seen_by_watch_and_reload() {
    for flag in ["--watch", "--reload"] {
        let d = dir(&format!("links{}", flag), &[
            ("main.qwe", b"import lib/a.qwe use word\nroute GET / = word()\n"),
            ("v1/a.qwe", b"fn word() = \"v1\"\n"),
            ("v2/a.qwe", b"fn word() = \"v2\"\n"),
        ]);
        std::fs::create_dir_all(d.join("lib")).unwrap();
        std::os::unix::fs::symlink("../v1/a.qwe", d.join("lib/a.qwe")).unwrap();
        let s = serve(&d, "main.qwe", &[flag]);
        let get = || send(s.port, "GET", "/", &[], "").body;
        let until = |want: &str| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while get() != want && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            get()
        };
        assert_eq!(get(), "v1");
        std::fs::remove_file(d.join("lib/a.qwe")).unwrap();
        std::os::unix::fs::symlink("../v2/a.qwe", d.join("lib/a.qwe")).unwrap();
        assert_eq!(until("v2"), "v2", "{}", flag);
        std::fs::write(d.join("v2/a.qwe"), "fn word() = \"v2 edited\"\n").unwrap();
        assert_eq!(until("v2 edited"), "v2 edited", "{}", flag);
    }
}

// ---- Q-384: localhost ------------------------------------------------------------------------

#[test]
fn host_localhost_answers_127_0_0_1() {
    let d = dir("localhost", &[("main.qwe", b"route GET / = \"here\"\n")]);
    let s = serve(&d, "main.qwe", &["--host", "localhost"]);
    assert_eq!(send(s.port, "GET", "/", &[], "").body, "here");
    assert!(s.log().contains("listening on http://127.0.0.1:"), "{}", s.log());
}

// ---- Q-385, Q-386, Q-387: source files -------------------------------------------------------

#[test]
fn a_file_is_read_as_windows_and_the_shell_write_one() {
    let d = dir(
        "source",
        &[
            ("bom.qwe", b"\xef\xbb\xbfprint(\"bom\")\n"),
            ("tool.qwe", b"#!/usr/bin/env quillo\nprint(\"shebang\")\nprint(2)\n"),
            ("crlf.qwe", b"let s = `a\r\nb`\r\nprint(len(s), json.stringify(s), json.stringify(\"x\\ry\"))\r\n"),
            ("used.qwe", b"#!/usr/bin/env quillo\nimport lib/a.qwe use word\nprint(word())\n"),
            ("lib/a.qwe", b"\xef\xbb\xbffn word() = \"from a\"\r\n"),
            ("bad.qwe", b"print(\"a\")\nprint(\"\xff\")\n"),
            ("nbsp.qwe", b"print(1)\xc2\xa0\n"),
            ("zw.qwe", b"let a\xe2\x80\x8b = 1\n"),
            ("hash.qwe", b"print(1)\n#!/not/first\n"),
        ],
    );
    assert_eq!(text(&quillo(&d, &["run", "bom.qwe"])).0, "bom\n");
    assert_eq!(text(&quillo(&d, &["run", "tool.qwe"])).0, "shebang\n2\n");
    assert_eq!(text(&quillo(&d, &["run", "crlf.qwe"])).0, "3 \"a\\nb\" \"x\\ry\"\n");
    assert_eq!(text(&quillo(&d, &["run", "used.qwe"])).0, "from a\n");
    let (_, e) = text(&quillo(&d, &["run", "bad.qwe"]));
    assert!(e.contains("not UTF-8 text: byte 0xFF at line 2, column 8"), "{}", e);
    let (_, e) = text(&quillo(&d, &["run", "nbsp.qwe"]));
    assert!(e.contains("unexpected character U+00A0 (a no-break space)") && !e.contains("newer Quillo"), "{}", e);
    assert!(text(&quillo(&d, &["check", "zw.qwe"])).1.contains("U+200B (a zero-width space)"));
    // `#!` counts on the first line only.
    assert!(!quillo(&d, &["run", "hash.qwe"]).status.success());
    assert!(text(&quillo(&d, &["check", "bom.qwe"])).0.contains("ok"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.join("tool.qwe"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let bin = PathBuf::from(env!("CARGO_BIN_EXE_quillo"));
        let path = format!("{}:{}", bin.parent().unwrap().display(), std::env::var("PATH").unwrap_or_default());
        let o = Command::new(d.join("tool.qwe")).env("PATH", path).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&o.stdout), "shebang\n2\n", "{}", String::from_utf8_lossy(&o.stderr));
    }
}

#[test]
fn a_json_body_with_a_byte_order_mark_is_read() {
    let d = dir("json-bom", &[("main.qwe", b"route POST /j = request.json\n")]);
    let s = serve(&d, "main.qwe", &[]);
    assert_eq!(send(s.port, "POST", "/j", &[("Content-Type", "application/json")], "\u{feff}{\"a\": 1}").body, "{\"a\":1}");
}

// ---- Q-388: the REPL from a pipe ---------------------------------------------------------------

fn repl(input: &str) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("repl").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    c.wait_with_output().unwrap()
}

#[test]
fn the_repl_from_a_pipe_writes_the_answers_alone_and_says_what_is_left_open() {
    let o = repl("1 + 1\nlet x = [1,\n2]\nx\n");
    assert_eq!(text(&o).0, "2\n[1, 2]\n");
    assert!(o.status.success());
    let o = repl("let s = \"abc\nprint(s)\n");
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).1.contains("the input ended inside the statement that began on line 1"), "{}", text(&o).1);
    let o = repl("1\n\nlet x = [1,\n");
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).1.contains("began on line 3"), "{}", text(&o).1);
    let o = repl("len(5)\n");
    assert_eq!(o.status.code(), Some(1), "a statement that failed: {:?}", text(&o));
    assert_eq!(repl("").status.code(), Some(0));
}

// ---- Q-390: local_path ----------------------------------------------------------------------

#[test]
fn local_path_lets_only_an_address_on_this_site_through() {
    let d = dir("local", &[(
        "main.qwe",
        b"for n in [\"/a?b=1\", \"/\\\\evil.example/\", \"/\\t/evil.example\", \"/\\n/evil.example\", \"//evil.example\", \"https://evil.example\", \"javascript:alert(1)\", \"a\", \"\", \" /x \", \"/%2F/evil\", \"/%5c/evil\", \"/ok#frag\", \"/caf\xc3\xa9\"] { print(json.stringify(local_path(n))) }\nprint(local_path(3), local_path(null))\n",
    )]);
    let (out, err) = text(&quillo(&d, &["run", "main.qwe"]));
    assert_eq!(out, "\"/a?b=1\"\nnull\nnull\nnull\nnull\nnull\nnull\nnull\nnull\n\"/x\"\nnull\nnull\n\"/ok#frag\"\n\"/café\"\nnull null\n", "{}", err);
}

// ---- Q-391: headers ---------------------------------------------------------------------------

#[test]
fn a_page_made_for_one_visitor_is_private_and_no_site_frames_a_page() {
    let d = dir("headers", &[(
        "main.qwe",
        b"site_headers({ \"content-security-policy\": \"default-src 'self'\", \"x-extra\": \"1\" })\nsite_headers(\"/embed\", { \"x-frame-options\": null, \"x-extra\": \"2\" })\nroute GET /me = <html><body>{session().user ?? \"nobody\"}</body></html>\nroute GET /c = <html><body>{cookie(\"theme\") ?? \"none\"}</body></html>\nroute GET /plain = <html><body><a href=\"/x\">hi</a></body></html>\nroute GET /embed/x = <html><body>e</body></html>\nroute GET /own = respond(200, \"x\", { \"x-frame-options\": \"DENY\", \"cache-control\": \"no-store\", \"content-type\": \"text/html\", \"x-extra\": \"3\" })\nroute GET /api = { a: session().user }\nroute GET /missing = respond(404, <html><body>{session().user}</body></html>)\n",
    )]);
    let s = serve(&d, "main.qwe", &[]);
    let h = |p: &str| send(s.port, "GET", p, &[], "");
    let me = h("/me");
    assert_eq!((me.header("cache-control"), me.header("vary"), me.header("x-frame-options")), (Some("private"), Some("Cookie"), Some("SAMEORIGIN")));
    assert_eq!(me.header("content-security-policy"), Some("default-src 'self'"));
    assert_eq!(h("/c").header("cache-control"), Some("private"));
    // Not `Vary: Cookie`; a page gzipped for a client that takes it varies by that, plain too
    // (FEEDBACK55.md, Q-724; branch feedback55). It has a link, so it carries the prefetch script
    // and is large enough to be gzipped (1,024 bytes): with none it gets no script (branch
    // page-size), and a page too small to gzip does not vary.
    let plain = h("/plain");
    assert_eq!((plain.header("cache-control"), plain.header("vary"), plain.header("x-frame-options"), plain.header("x-extra")), (None, Some("accept-encoding"), Some("SAMEORIGIN"), Some("1")));
    let embed = h("/embed/x");
    assert_eq!((embed.header("x-frame-options"), embed.header("x-extra")), (None, Some("2")));
    let own = h("/own");
    assert_eq!((own.header("x-frame-options"), own.header("cache-control"), own.header("x-extra")), (Some("DENY"), Some("no-store"), Some("3")));
    let api = h("/api");
    assert_eq!((api.header("cache-control"), api.header("x-frame-options")), (Some("private"), None));
    // Whatever the status: a 404 that names the visitor is theirs (FEEDBACK55.md, Q-886; branch
    // feedback55).
    assert_eq!(h("/missing").header("cache-control"), Some("private"));
    for (bad, said) in [
        ("site_headers(\"admin\", {})", "needs a path that starts with /"),
        ("site_headers({ \"bad name\": \"1\" })", "is not a header's name"),
        ("site_headers({ \"x-a\": \"a\\nb\" })", "text on one line"),
        ("site_headers({ \"set-cookie\": \"a=1\" })", "is the answer's own"),
    ] {
        let d = dir("headers-bad", &[("main.qwe", format!("{}\n", bad).as_bytes())]);
        assert!(text(&quillo(&d, &["run", "main.qwe"])).1.contains(said), "{}", bad);
    }
}

// ---- Q-392: a file replaced by one of the same size and time ------------------------------------

#[cfg(unix)]
#[test]
fn a_file_replaced_by_one_of_the_same_size_and_time_is_served_new() {
    let d = dir("same-time", &[("main.qwe", b"route GET /static/*p = file(\"public/\" + p)\n"), ("public/app.css", b"body { color: red  }\n")]);
    let touch = |p: &Path| Command::new("touch").args(["-t", "202601010000"]).arg(p).status().unwrap();
    touch(&d.join("public/app.css"));
    let s = serve(&d, "main.qwe", &[]);
    let first = send(s.port, "GET", "/static/app.css", &[], "");
    assert_eq!(first.body, "body { color: red  }\n");
    std::fs::write(d.join("public/app.css"), "body { color: blue }\n").unwrap();
    touch(&d.join("public/app.css"));
    let second = send(s.port, "GET", "/static/app.css", &[], "");
    assert_eq!(second.body, "body { color: blue }\n");
    assert_ne!(first.header("etag"), second.header("etag"));
    // The old copy's tag no longer matches; the new one's does.
    assert_eq!(send(s.port, "GET", "/static/app.css", &[("If-None-Match", first.header("etag").unwrap())], "").status, 200);
    assert_eq!(send(s.port, "GET", "/static/app.css", &[("If-None-Match", second.header("etag").unwrap())], "").status, 304);
}

// ---- Q-393: Host and HEAD ----------------------------------------------------------------------

#[test]
fn host_must_be_one_and_head_says_the_gets_length() {
    let d = dir("host", &[("main.qwe", b"route GET /echo = request.headers.host ?? \"none\"\nroute GET /ten = \"0123456789\"\n"), ("public/a.css", b"x")]);
    let s = serve(&d, "main.qwe", &[]);
    let status = |r: &str| r.split(' ').nth(1).unwrap_or("").to_string();
    assert_eq!(status(&raw(s.port, b"GET /echo HTTP/1.1\r\nHost: a.example\r\nHost: b.example\r\n\r\n")), "400");
    assert_eq!(status(&raw(s.port, b"GET /echo HTTP/1.1\r\nConnection: close\r\n\r\n")), "400");
    assert_eq!(status(&raw(s.port, b"GET /echo HTTP/1.1\r\nHost: a b\r\nConnection: close\r\n\r\n")), "400");
    assert_eq!(status(&raw(s.port, b"get /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")), "501");
    let old = raw(s.port, b"GET /echo HTTP/1.0\r\n\r\n");
    assert!(old.starts_with("HTTP/1.1 200") && old.ends_with("none"), "{}", old);
    let abs = raw(s.port, b"GET http://site.example:8080/echo HTTP/1.1\r\nHost: other\r\nConnection: close\r\n\r\n");
    assert!(abs.ends_with("site.example:8080"), "{}", abs);
    // Read as a host is named: the default port dropped (FEEDBACK47.md, Q-545).
    for (good, read) in [("a.example", "a.example"), ("a.example:8080", "a.example:8080"), ("[::1]:80", "[::1]"), ("127.0.0.1", "127.0.0.1")] {
        let r = raw(s.port, format!("GET /echo HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", good).as_bytes());
        assert!(r.starts_with("HTTP/1.1 200") && r.ends_with(read), "{}: {}", good, r);
    }
    let head = raw(s.port, b"HEAD /ten HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    assert!(head.to_ascii_lowercase().contains("content-length: 10\r\n"), "{}", head);
    assert!(head.ends_with("\r\n\r\n"), "no body: {}", head);
}

// ---- Q-394: normalize ------------------------------------------------------------------------

#[test]
fn normalize_makes_text_that_looks_the_same_one_text() {
    let d = dir("normalize", &[(
        "main.qwe",
        "print(len(\"café\"), len(normalize(\"cafe\\u{301}\")), normalize(\"cafe\\u{301}\") == \"café\", len(normalize(\"é\", \"nfd\")), normalize(\"ﬁ\", \"NFKC\"), normalize(\"ﬁ\"), normalize(\"\"), normalize(\"Ⅻ\", \"NFKD\"))\nnormalize(\"x\", \"NFX\")\n".as_bytes(),
    )]);
    let (out, err) = text(&quillo(&d, &["run", "main.qwe"]));
    assert_eq!(out, "4 4 true 2 fi ﬁ  XII\n", "{}", err);
    assert!(err.contains("the form is \"NFC\", \"NFD\", \"NFKC\" or \"NFKD\""), "{}", err);
}
