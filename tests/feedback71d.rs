//! FEEDBACK71.md, the security audit's sixth slice (branch security): a database failure inside
//! `idempotent` tells a visitor only that the key could not be recorded, the database's words
//! going to the log (Q-978); what a tool's `///` and schemas tell a model holds no character a
//! reader of the file does not see (Q-980); `quillo install` writes through no link (Q-981); a
//! program's files are read as regular files of at most 64 MiB, so a link to `/dev/zero` no longer
//! takes the machine's memory (Q-982); and the plugin build cache is its user's alone, a
//! `build.rs` put there neither kept nor run (Q-983). Every server and registry is one this test
//! starts, on this machine.

mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::send;

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback71d-{}", name));
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

/// `quillo run` of `file` as a server, its log in `log`, once it says where it listens (20 s at
/// most), on the port the system picks.
fn serve(d: &Path, file: &str, args: &[&str], log: &str) -> (support::Guard, u16) {
    let out = std::fs::File::create(d.join(log)).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", file, "--port", "0"])
        .args(args)
        .current_dir(d)
        .env("QUILLO_SECRET", "a secret for the feedback71d tests, long enough")
        .stdin(Stdio::null())
        .stdout(out.try_clone().unwrap())
        .stderr(out)
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let text = std::fs::read_to_string(d.join(log)).unwrap_or_default();
        let said = text.lines().find_map(|l| l.split("listening on http://127.0.0.1:").nth(1)?.split(|c: char| !c.is_ascii_digit()).next()?.parse::<u16>().ok());
        if let Some(port) = said {
            if support::listening(port) {
                return (g, port);
            }
        }
        if let Ok(Some(status)) = g.0.try_wait() {
            panic!("{} stopped ({}): {}", file, status, text);
        }
        assert!(Instant::now() < deadline, "{} did not start: {}", file, text);
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The log at `path` once it holds `what`, 10 s at most: a request's line is written after its
/// answer is sent.
fn log_with(path: &Path, what: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if text.contains(what) || Instant::now() > deadline {
            return text;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// What a command did, run to its end within `secs` seconds and under `cap_mb` MB of resident
/// memory: its exit code (None when it was stopped), its stdout and its stderr, and whether the
/// memory cap stopped it. A read without end took about 550 MB in 0.14 s, so the cap stops one
/// long before it hurts the machine.
struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
    capped: bool,
}

fn run_capped(mut c: Command, secs: u64, cap_mb: u64) -> Ran {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let base = std::env::temp_dir().join(format!("quillo-feedback71d-out-{}-{}", std::process::id(), n));
    let (o, e) = (base.with_extension("out"), base.with_extension("err"));
    let child = c.stdin(Stdio::null()).stdout(std::fs::File::create(&o).unwrap()).stderr(std::fs::File::create(&e).unwrap()).spawn().unwrap();
    let mut g = support::Guard::new(child);
    let capped = cap_memory(g.0.id(), cap_mb);
    let deadline = Instant::now() + Duration::from_secs(secs);
    let code = loop {
        if let Some(status) = g.0.try_wait().unwrap() {
            break status.code();
        }
        if Instant::now() > deadline {
            let _ = g.0.kill();
            let _ = g.0.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    capped.1.store(true, Ordering::SeqCst);
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
    let ran = Ran { code, out: read(&o), err: read(&e), capped: capped.0.load(Ordering::SeqCst) };
    let _ = std::fs::remove_file(&o);
    let _ = std::fs::remove_file(&e);
    ran
}

/// Watches process `pid` every 50 ms and kills it once its resident memory passes `cap_mb`: the
/// first flag says it did, the second stops the watch. Only a process still named quillo is
/// killed.
fn cap_memory(pid: u32, cap_mb: u64) -> (Arc<AtomicBool>, Arc<AtomicBool>) {
    let (hit, done) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let (h, d) = (hit.clone(), done.clone());
    std::thread::spawn(move || {
        while !d.load(Ordering::SeqCst) {
            let Ok(o) = Command::new("ps").args(["-o", "rss=,comm=", "-p", &pid.to_string()]).output() else { return };
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            let mut words = text.split_whitespace();
            let (Some(kb), Some(name)) = (words.next().and_then(|w| w.parse::<u64>().ok()), words.next()) else { return };
            if kb / 1024 >= cap_mb && name.contains("quillo") && !d.load(Ordering::SeqCst) {
                h.store(true, Ordering::SeqCst);
                let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).status();
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    (hit, done)
}

fn quillo(d: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.current_dir(d);
    c
}

// ---- Q-978: idempotent's database failure ------------------------------------------------

/// The table of keys with a constraint every row breaks: it stands for any database failure.
const IDEM_FAILS: &str = r#"db.open("data/app.db")
db.exec("create table if not exists _quillo_idempotency (id text primary key, path text, fingerprint text, token text, done int, lease_until int, expires_at int, status int, headers text, body text, body_b64 int, owners text, bare text, check (length(path) < 3))")
route POST /pay = idempotent(() => "ok")
"#;

#[test]
fn q978_a_database_failure_in_idempotent_tells_the_visitor_only_that_the_key_was_not_recorded() {
    for (name, args, path) in [("plain", &[][..], "/pay"), ("base", &["--base", "/app"][..], "/pay"), ("instances", &["--instances", "2"][..], "/pay")] {
        let d = dir(&format!("q978-{}", name), &[("main.qwe", IDEM_FAILS)]);
        std::fs::create_dir_all(d.join("data")).unwrap();
        let (_g, port) = serve(&d, "main.qwe", args, "log.txt");
        for key in ["a1", "k"] {
            let a = send(port, "POST", path, &[("Idempotency-Key", key)], "");
            assert_eq!(a.status, 503, "{}: {}", name, a.body);
            assert_eq!(a.body, r#"{"error":"could not record the Idempotency-Key"}"#, "{}", name);
            assert_eq!(a.header("content-type"), Some("application/json"), "{}", name);
        }
        // The database's words go to the log, at the program's line.
        let log = log_with(&d.join("log.txt"), "could not record the Idempotency-Key: db: CHECK constraint failed");
        assert!(log.contains("could not record the Idempotency-Key: db: CHECK constraint failed: length(path) < 3"), "{}: {}", name, log);
        assert!(log.contains("main.qwe:3: could not record"), "{}: {}", name, log);
    }
}

#[test]
fn q978_under_watch_the_developer_reads_the_database_s_words() {
    let d = dir("q978-watch", &[("main.qwe", IDEM_FAILS)]);
    std::fs::create_dir_all(d.join("data")).unwrap();
    let (_g, port) = serve(&d, "main.qwe", &["--watch"], "log.txt");
    let a = send(port, "POST", "/pay", &[("Idempotency-Key", "a1")], "");
    assert_eq!(a.status, 503, "{}", a.body);
    assert_eq!(a.body, r#"{"error":"could not record the Idempotency-Key: db: CHECK constraint failed: length(path) < 3"}"#);
}

#[test]
fn q978_a_working_table_answers_and_replays_as_before() {
    let d = dir("q978-works", &[("main.qwe", "db.open(\"data/good.db\")\nroute POST /pay = idempotent(() => \"ok\")\n")]);
    std::fs::create_dir_all(d.join("data")).unwrap();
    let (_g, port) = serve(&d, "main.qwe", &[], "log.txt");
    let first = send(port, "POST", "/pay", &[("Idempotency-Key", "é-is-not-ascii")], "");
    assert_eq!(first.status, 400, "{}", first.body);
    let first = send(port, "POST", "/pay", &[("Idempotency-Key", "a1")], "");
    assert_eq!((first.status, first.body.as_str(), first.header("idempotent-replayed")), (200, "ok", None));
    let again = send(port, "POST", "/pay", &[("Idempotency-Key", "a1")], "");
    assert_eq!((again.status, again.body.as_str(), again.header("idempotent-replayed")), (200, "ok", Some("true")));
}

// ---- Q-980: hidden characters in what a tool tells a model -----------------------------------

/// Each kind of character `mcp::unseen` knows, and the name a message gives it.
const HIDDEN: &[(char, &str)] = &[
    ('\u{1}', "a control character"),
    ('\u{1B}', "a control character"),
    ('\u{7F}', "a control character"),
    ('\u{85}', "a control character"),
    ('\u{AD}', "soft hyphen"),
    ('\u{61C}', "Arabic letter mark"),
    ('\u{180E}', "Mongolian vowel separator"),
    ('\u{200B}', "zero width space"),
    ('\u{200E}', "left-to-right mark"),
    ('\u{200F}', "right-to-left mark"),
    ('\u{2028}', "line separator"),
    ('\u{2029}', "paragraph separator"),
    ('\u{202A}', "left-to-right embedding"),
    ('\u{202B}', "right-to-left embedding"),
    ('\u{202C}', "pop directional formatting"),
    ('\u{202D}', "left-to-right override"),
    ('\u{202E}', "right-to-left override"),
    ('\u{2060}', "word joiner"),
    ('\u{2061}', "an invisible operator"),
    ('\u{2064}', "an invisible operator"),
    ('\u{2066}', "left-to-right isolate"),
    ('\u{2067}', "right-to-left isolate"),
    ('\u{2068}', "first strong isolate"),
    ('\u{2069}', "pop directional isolate"),
    ('\u{206A}', "a deprecated format character"),
    ('\u{206F}', "a deprecated format character"),
    ('\u{FEFF}', "zero width no-break space"),
    ('\u{FFF9}', "an interlinear annotation character"),
    ('\u{FFFB}', "an interlinear annotation character"),
    ('\u{E0000}', "a tag character"),
    ('\u{E0049}', "a tag character"),
    ('\u{E007F}', "a tag character"),
];

/// `quillo check`'s errors of `src` as main.qwe in `d`: each line and message.
fn errors_of(d: &Path, src: &str) -> Vec<(u32, String)> {
    let a = quillo::check::check_source(src, &d.join("main.qwe").display().to_string(), d);
    a.diagnostics.into_iter().filter(|x| x.severity == quillo::check::Severity::Error).map(|x| (x.line, x.message)).collect()
}

#[test]
fn q980_each_hidden_character_in_a_tool_s_doc_is_an_error_at_its_line_and_column() {
    let d = dir("q980-doc", &[]);
    for (c, name) in HIDDEN {
        let src = format!("//! Notes.\n/// Lists{}the notes.\nfn mcp list_notes() -> list with time = []\n", c);
        let want = format!(
            "column 10: the `///` comment of tool `list_notes` is what AI clients are told of it, and this holds U+{:04X} ({}), which a reader of the file does not see: take it out",
            *c as u32, name
        );
        assert_eq!(errors_of(&d, &src), vec![(2, want)], "U+{:04X}", *c as u32);
    }
}

#[test]
fn q980_each_hidden_character_in_a_later_line_of_a_description_is_said_there() {
    let d = dir("q980-later", &[]);
    for (c, name) in HIDDEN {
        // A `//` line and an empty `///` among the lines, as the description reads past both.
        let src = format!("//! Notes.\n/// Sends a note.\n// not told\n///\n    /// Then: é 日本{}x.\nfn mcp send(to: str) -> str with time = to\n", c);
        let errs = errors_of(&d, &src);
        assert_eq!(errs.len(), 1, "U+{:04X}: {:?}", *c as u32, errs);
        assert_eq!(errs[0].0, 5, "U+{:04X}", *c as u32);
        assert!(errs[0].1.starts_with("column 19: the `///` comment of tool `send`"), "{}", errs[0].1);
        assert!(errs[0].1.contains(&format!("U+{:04X} ({})", *c as u32, name)), "{}", errs[0].1);
    }
}

#[test]
fn q980_each_hidden_character_in_a_schema_s_text_is_an_error_at_the_tool() {
    let d = dir("q980-schema", &[]);
    for (c, name) in HIDDEN {
        let cases = [
            (format!("//! Notes.\n/// Picks one.\nfn mcp pick(mode: str = \"fast{}\") -> str with time = mode\n", c), 3, "`inputSchema/properties/mode/default`"),
            (format!("//! Notes.\n/// Picks one.\nfn mcp pick(mode: \"a\" | \"b{}\") -> str with time = mode\n", c), 3, "`inputSchema/properties/mode/enum/1`"),
            (format!("//! Notes.\ntype Opts = {{ mode: str = \"x{}\" }}\n/// Picks one.\nfn mcp pick(o: Opts) -> str with time = o.mode\n", c), 4, "`inputSchema/$defs/Opts/properties/mode/default`"),
            (format!("//! Notes.\n/// Picks one.\nfn mcp pick() -> \"a\" | \"b{}\" with time = \"a\"\n", c), 3, "`outputSchema/properties/result/enum/1`"),
        ];
        for (src, line, at) in cases {
            let want = format!(
                "the schema of tool `pick` is what AI clients are told of its arguments and result, and {} holds U+{:04X} ({}), which a reader of the file does not see: take it out",
                at, *c as u32, name
            );
            assert_eq!(errors_of(&d, &src), vec![(line, want)], "U+{:04X} in {}", *c as u32, at);
        }
    }
}

#[test]
fn q980_a_clean_doc_passes_and_its_text_reaches_the_tool_whole() {
    let d = dir("q980-clean", &[]);
    // Joiners stay allowed, as in the `//!` block: an emoji sequence, a Persian word; a tab too.
    let doc = "Lists the notes 👩\u{200D}💻 می\u{200C}خواهم é 日本語\twith a tab.";
    let src = format!("//! Notes.\n/// {}\n// a plain line\n///\n/// Second line.\nfn mcp list_notes(mode: \"new\" | \"old\" = \"new\") -> list with time = []\nroute GET / = \"home\"\n", doc);
    assert_eq!(errors_of(&d, &src), vec![]);
    std::fs::write(d.join("main.qwe"), &src).unwrap();
    let tools = quillo::mcp::of_source(&d.join("main.qwe")).unwrap();
    assert_eq!(tools[0].description.as_deref(), Some(format!("{}\n\nSecond line.", doc).as_str()));
    // A function no model is told of keeps its doc as it is.
    assert_eq!(errors_of(&d, "/// Adds one\u{202E}.\nfn helper(n: int) -> int {\n    return n + 1\n}\nroute GET / = helper(1)\n"), vec![]);
    // An empty file and a file with no tools.
    assert_eq!(errors_of(&d, ""), vec![]);
    assert_eq!(errors_of(&d, "//! Notes.\nroute GET / = \"home\"\n"), vec![]);
}

#[test]
fn q980_a_joiner_passes_only_between_two_characters_a_reader_sees() {
    let d = dir("q980-joiners", &[]);
    let src = |doc: &str| format!("//! Notes.\n/// {}\nfn mcp list_notes() -> list with time = []\n", doc);
    // Between two seen characters: an emoji sequence, a Persian word, one with a variation selector.
    for ok in ["👩\u{200D}💻 at work", "می\u{200C}خواهم", "❤\u{FE0F}\u{200D}🔥 hot", "a\u{200C}b\u{200D}c"] {
        assert_eq!(errors_of(&d, &src(ok)), vec![], "{:?}", ok);
    }
    // A run of them, one at either end, beside a space, beside another unseen one.
    for (bad, column) in [("Lists.\u{200D}\u{200D}\u{200C} plain", 11), ("\u{200D}Lists", 5), ("Lists\u{200C}", 10), ("a \u{200D}b", 7), ("a\u{200D} b", 6), ("a\u{200D}\u{200B}b", 6)] {
        let errs = errors_of(&d, &src(bad));
        assert_eq!(errs.len(), 1, "{:?}: {:?}", bad, errs);
        assert_eq!(errs[0].0, 2);
        assert!(errs[0].1.starts_with(&format!("column {}: the `///` comment of tool `list_notes`", column)), "{:?}: {}", bad, errs[0].1);
        assert!(errs[0].1.contains("not between two characters a reader sees: take it out"), "{:?}: {}", bad, errs[0].1);
    }
    // In a schema's text too.
    let errs = errors_of(&d, "//! Notes.\n/// Picks.\nfn mcp pick(mode: str = \"a\u{200D}\u{200D}b\") -> str with time = mode\n");
    assert!(errs.len() == 1 && errs[0].1.contains("`inputSchema/properties/mode/default` holds U+200D (zero width joiner), not between two characters a reader sees"), "{:?}", errs);
}

#[test]
fn q980_a_program_whose_file_name_hides_a_character_and_has_tools_is_refused() {
    let tools = "//! Notes.\n/// Lists.\nfn mcp list_notes() -> list with time = []\n";
    let d = dir("q980-name", &[("fname\u{202E}.qwe", tools), ("plain\u{202E}.qwe", "route GET / = \"x\"\n"), ("café_日本👩\u{200D}💻.qwe", tools), ("tag\u{E0041}.qwe", tools)]);
    let errs = |f: &str| quillo::check::check_file(&d.join(f)).errors().map(|x| x.to_string()).collect::<Vec<_>>();
    let e = errs("fname\u{202E}.qwe");
    assert_eq!(e.len(), 1, "{:?}", e);
    assert!(e[0].ends_with(": error: the file's name is the program's name AI clients are told (`serverInfo`, `instructions`), and it holds U+202E (right-to-left override), which a reader of the file does not see: rename the file"), "{}", e[0]);
    assert!(errs("tag\u{E0041}.qwe")[0].contains("U+E0041 (a tag character)"));
    // A program with no tools is told to no model; a name a reader sees whole passes.
    assert_eq!(errs("plain\u{202E}.qwe"), Vec::<String>::new());
    assert_eq!(errs("café_日本👩\u{200D}💻.qwe"), Vec::<String>::new());
}

#[test]
fn q980_lines_count_as_the_lexer_counts_them_crlf_bom_and_shebang_too() {
    let d = dir("q980-lines", &[]);
    let want = |line: u32| (line, "column 10: the `///` comment of tool `list_notes` is what AI clients are told of it, and this holds U+202E (right-to-left override), which a reader of the file does not see: take it out".to_string());
    let crlf = "//! Notes.\r\n/// Lists\u{202E}x.\r\nfn mcp list_notes() -> list with time = []\r\n";
    assert_eq!(errors_of(&d, crlf), vec![want(2)]);
    let marked = "\u{FEFF}#!/usr/bin/env quillo\n//! Notes.\n/// Lists\u{202E}x.\nfn mcp list_notes() -> list with time = []\n";
    assert_eq!(errors_of(&d, marked), vec![want(3)]);
}

#[test]
fn q980_a_used_file_s_tool_and_a_service_s_tool_are_said_in_their_own_file() {
    let d = dir(
        "q980-files",
        &[
            ("tools.qwe", "/// Lists\u{E0049}\u{E0067}the notes.\nfn mcp list_notes() -> list with time = []\n"),
            ("billing.qwe", "//! Billing.\n/// Charges\u{200B}the card.\nfn mcp charge(n: int) -> int with time = n\n"),
        ],
    );
    let a = quillo::check::check_file(&{
        std::fs::write(d.join("main.qwe"), "use \"tools.qwe\"\nroute GET / = \"home\"\n").unwrap();
        d.join("main.qwe")
    });
    let errs: Vec<String> = a.errors().map(|x| x.to_string()).collect();
    assert_eq!(errs.len(), 1, "{:?}", errs);
    assert!(errs[0].contains("tools.qwe:1: error: column 10: the `///` comment of tool `list_notes`") && errs[0].contains("U+E0049 (a tag character)"), "{}", errs[0]);
    std::fs::write(d.join("main.qwe"), "//! Shop.\nservice mcp /billing = \"billing.qwe\"\nroute GET / = \"home\"\n").unwrap();
    let errs: Vec<String> = quillo::check::check_file(&d.join("main.qwe")).errors().map(|x| x.to_string()).collect();
    assert_eq!(errs.len(), 1, "{:?}", errs);
    assert!(errs[0].contains("billing.qwe:2: error: column 12: the `///` comment of tool `charge`") && errs[0].contains("U+200B (zero width space)"), "{}", errs[0]);
    // And `quillo check` stops there, as for the `//!` block.
    let r = run_capped({
        let mut c = quillo(&d);
        c.args(["check", "main.qwe"]);
        c
    }, 30, 400);
    assert_eq!(r.code, Some(1), "{}", r.err);
}

// ---- Q-981: quillo install writes through no link -----------------------------------------

/// A plugin as a file, with the version in its header, `pad` bytes of comment making it longer.
fn plugin(pad: usize) -> Vec<u8> {
    format!("//! Says hi.\n//! version: 1.0.0\n//!\n// {}\npub fn hi() -> String {{ \"hi\".into() }}\n", "x".repeat(pad)).into_bytes()
}

fn sha(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{:02x}", b)).collect()
}

/// A catalog with plugin `hi` and its example `hi.qwe`, served on a port of the test's own until
/// the test ends: its URL.
fn registry(file: &[u8]) -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let index = serde_json::json!({ "format": 1, "plugins": [{
        "name": "hi", "title": "hi", "version": "1.0.0", "summary": "Says hi.", "group": "Text",
        "sha256": sha(file), "file": "/plugins/files/hi.qwer", "example": "/plugins/files/hi.qwe", "files": [],
    }] })
    .to_string();
    let pages: Vec<(String, Vec<u8>)> = vec![
        ("/plugins/index.json".into(), index.into_bytes()),
        ("/plugins/files/hi.qwer".into(), file.to_vec()),
        ("/plugins/files/hi.qwe".into(), b"use \"hi.qwer\"\nprint(hi())\n".to_vec()),
    ];
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                match s.read(&mut byte) {
                    Ok(1) => head.push(byte[0]),
                    _ => break,
                }
            }
            let head = String::from_utf8_lossy(&head).to_string();
            let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
            let (status, body) = pages.iter().find(|(p, _)| *p == path).map_or(("404 Not Found", b"no".to_vec()), |(_, b)| ("200 OK", b.clone()));
            let _ = write!(s, "HTTP/1.1 {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", status, body.len());
            let _ = s.write_all(&body);
        }
    });
    url
}

fn install(d: &Path, url: &str, args: &[&str]) -> Ran {
    let mut c = quillo(d);
    c.arg("install").args(args).args(["--registry", url]).env("HOME", d);
    run_capped(c, 30, 400)
}

/// The names in `d`, sorted.
fn names(d: &Path) -> Vec<String> {
    let mut n: Vec<String> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    n.sort();
    n
}

#[cfg(unix)]
#[test]
fn q981_a_link_at_the_old_part_name_is_not_written_through() {
    let d = dir("q981-part", &[("home/.zshrc", "export SECRET_SETTING=1\n")]);
    let repo = d.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::os::unix::fs::symlink("../home/.zshrc", repo.join("hi.qwer.part")).unwrap();
    std::os::unix::fs::symlink("../home/notes.txt", repo.join("hi.qwe.part")).unwrap();
    let file = plugin(10);
    let url = registry(&file);
    let r = install(&repo, &url, &["hi", "--example"]);
    assert_eq!(r.code, Some(0), "{}{}", r.out, r.err);
    assert!(r.out.contains("installed hi 1.0.0 into ./hi.qwer") && r.out.contains("and ./hi.qwe"), "{}", r.out);
    // The developer's files are as they were, and none was made outside the repository.
    assert_eq!(std::fs::read_to_string(d.join("home/.zshrc")).unwrap(), "export SECRET_SETTING=1\n");
    assert!(!d.join("home/notes.txt").exists());
    // What was installed is a file of its own, and the links stay as they were.
    for (f, want) in [("hi.qwer", file.clone()), ("hi.qwe", b"use \"hi.qwer\"\nprint(hi())\n".to_vec())] {
        let m = std::fs::symlink_metadata(repo.join(f)).unwrap();
        assert!(m.file_type().is_file(), "{} is not a file", f);
        assert_eq!(std::fs::read(repo.join(f)).unwrap(), want);
    }
    assert_eq!(names(&repo), ["hi.qwe", "hi.qwe.part", "hi.qwer", "hi.qwer.part"]);
}

#[cfg(unix)]
#[test]
fn q981_a_target_that_is_a_link_is_neither_read_nor_written() {
    let d = dir("q981-target", &[("home/.zshrc", "export SECRET_SETTING=1\n")]);
    let repo = d.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let url = registry(&plugin(10));
    // To a file of the developer's.
    std::os::unix::fs::symlink("../home/.zshrc", repo.join("hi.qwer")).unwrap();
    let r = install(&repo, &url, &["hi"]);
    assert_eq!(r.code, Some(1), "{}{}", r.out, r.err);
    assert!(r.err.contains("./hi.qwer is a symbolic link: nothing written; remove it to install here"), "{}", r.err);
    assert_eq!(std::fs::read_to_string(d.join("home/.zshrc")).unwrap(), "export SECRET_SETTING=1\n");
    // To a device that never ends: refused before anything is read (the cap would stop a read).
    std::fs::remove_file(repo.join("hi.qwer")).unwrap();
    std::os::unix::fs::symlink("/dev/zero", repo.join("hi.qwer")).unwrap();
    let r = install(&repo, &url, &["hi", "--force"]);
    assert!(!r.capped && r.code == Some(1), "{:?} {}", r.code, r.err);
    assert!(r.err.contains("is a symbolic link"), "{}", r.err);
    // An example's link that leads nowhere yet: nothing is made where it points.
    std::fs::remove_file(repo.join("hi.qwer")).unwrap();
    std::os::unix::fs::symlink("../home/new.txt", repo.join("hi.qwe")).unwrap();
    let r = install(&repo, &url, &["hi", "--example"]);
    assert_eq!(r.code, Some(1), "{}{}", r.out, r.err);
    assert!(r.err.contains("./hi.qwe is a symbolic link: nothing written"), "{}", r.err);
    assert!(!d.join("home/new.txt").exists());
    assert_eq!(names(&repo), ["hi.qwe", "hi.qwer"]);
    assert!(std::fs::symlink_metadata(repo.join("hi.qwer")).unwrap().file_type().is_file());
}

#[test]
fn q981_an_install_and_an_update_leave_only_the_plugin() {
    let d = dir("q981-ok", &[]);
    let file = plugin(10);
    let url = registry(&file);
    let r = install(&d, &url, &["hi"]);
    assert_eq!(r.code, Some(0), "{}{}", r.out, r.err);
    assert_eq!(std::fs::read(d.join("hi.qwer")).unwrap(), file);
    assert_eq!(names(&d), ["hi.qwer"]);
    // An older one by hand is replaced in place.
    std::fs::write(d.join("hi.qwer"), "//! version: 0.9.0\npub fn hi() -> String { \"old\".into() }\n").unwrap();
    let r = install(&d, &url, &["--update"]);
    assert_eq!(r.code, Some(0), "{}{}", r.out, r.err);
    assert!(r.out.contains("updated hi 0.9.0 → 1.0.0"), "{}", r.out);
    assert_eq!(std::fs::read(d.join("hi.qwer")).unwrap(), file);
    assert_eq!(names(&d), ["hi.qwer"]);
}

#[cfg(unix)]
#[test]
fn q981_a_write_that_fails_part_way_or_a_rename_that_fails_leaves_nothing() {
    let d = dir("q981-fails", &[]);
    let file = plugin(4096);
    let url = registry(&file);
    let r = if cfg!(any(debug_assertions, coverage)) {
        // A debug build, or the coverage run's (a release build with `--cfg coverage`, from cargo
        // llvm-cov), fails the write half way when asked: `ulimit -f` would also stop the coverage
        // run's own file being written.
        let mut c = quillo(&d);
        c.env("HOME", &d).env("QUILLO_TEST_FAIL_WRITE", "1").args(["install", "hi", "--registry", &url]);
        run_capped(c, 30, 400)
    } else {
        // Bigger than the 512 bytes `ulimit -f 1` lets a process write to a file: the write stops
        // part way, with SIGXFSZ ignored so it is an error and not the end of the process.
        let mut c = Command::new("sh");
        c.current_dir(&d).env("HOME", &d).args(["-c", "ulimit -f 1; trap '' XFSZ; exec \"$0\" install hi --registry \"$1\"", env!("CARGO_BIN_EXE_quillo"), &url]);
        run_capped(c, 30, 400)
    };
    assert_eq!(r.code, Some(1), "{}{}", r.out, r.err);
    assert!(r.err.contains("quillo install: ./hi.qwer.") && r.err.contains(".part: "), "{}", r.err);
    assert_eq!(names(&d), Vec::<String>::new());
    // The plugin's name taken by a folder: the rename fails, and the file written is taken away.
    std::fs::create_dir_all(d.join("hi.qwer/inside")).unwrap();
    let r = install(&d, &url, &["hi"]);
    assert_eq!(r.code, Some(1), "{}{}", r.out, r.err);
    assert!(r.err.contains("quillo install: ./hi.qwer: "), "{}", r.err);
    assert_eq!(names(&d), ["hi.qwer"]);
    assert_eq!(names(&d.join("hi.qwer")), ["inside"]);
}

// ---- Q-982: a program's files are regular files of a bounded size ---------------------------

#[test]
fn q982_a_regular_file_is_read_up_to_its_limit_and_nothing_else_is() {
    let d = dir("q982-read", &[("main.qwe", "route GET / = \"héllo 日本\"\n"), ("empty.qwe", "")]);
    use quillo::error::{read_source, read_text, MOST_READ, MOST_SOURCE};
    assert_eq!((MOST_READ, MOST_SOURCE, quillo::error::MOST_PROGRAM), (64 << 20, 16 << 20, 32 << 20));
    assert_eq!(read_source(d.join("main.qwe")).unwrap(), "route GET / = \"héllo 日本\"\n");
    assert_eq!(read_text(d.join("empty.qwe")).unwrap(), "");
    // At each limit, and one byte over (sparse files: no disk written): a source (`.qwe`,
    // `.qwer`, or any file read as one), and any other file.
    let sized = |name: &str, len: u64| {
        std::fs::File::create(d.join(name)).unwrap().set_len(len).unwrap();
        d.join(name)
    };
    for ext in ["qwe", "qwer"] {
        assert_eq!(read_text(sized(&format!("at.{}", ext), MOST_SOURCE)).unwrap().len() as u64, MOST_SOURCE);
        assert_eq!(read_text(sized(&format!("over.{}", ext), MOST_SOURCE + 1)).unwrap_err().to_string(), "it is larger than 16 MiB");
    }
    assert_eq!(read_source(d.join("at.qwe")).unwrap().len() as u64, MOST_SOURCE);
    assert_eq!(read_source(d.join("over.qwe")).unwrap_err().to_string(), "it is larger than 16 MiB");
    assert_eq!(read_source(sized("big.txt", MOST_SOURCE + 1)).unwrap_err().to_string(), "it is larger than 16 MiB");
    assert_eq!(read_text(sized("at.css", MOST_READ)).unwrap().len() as u64, MOST_READ);
    assert_eq!(read_text(sized("over.css", MOST_READ + 1)).unwrap_err().to_string(), "it is larger than 64 MiB");
    // A folder, said as before.
    std::fs::create_dir_all(d.join("dir.qwe")).unwrap();
    #[cfg(unix)]
    assert_eq!(read_source(d.join("dir.qwe")).unwrap_err().to_string(), "Is a directory (os error 21)");
    assert!(read_text(d.join("dir.qwe")).is_err());
    // Not there, and not UTF-8: said as before.
    assert_eq!(read_source(d.join("none.qwe")).unwrap_err().kind(), std::io::ErrorKind::NotFound);
    std::fs::write(d.join("latin1.qwe"), b"let a = \"caf\xe9\"\n").unwrap();
    assert!(read_source(d.join("latin1.qwe")).unwrap_err().to_string().starts_with("it is not UTF-8 text: byte 0xE9 at line 1"));
    assert!(read_text(d.join("latin1.qwe")).is_err());
}

#[cfg(unix)]
#[test]
fn q982_a_fifo_with_no_writer_is_refused_at_once() {
    let d = dir("q982-fifo", &[("main.qwe", "import lib/pipe.qwe use helper\nroute GET / = helper()\n")]);
    std::fs::create_dir_all(d.join("lib")).unwrap();
    assert!(Command::new("mkfifo").arg(d.join("lib/pipe.qwe")).status().unwrap().success());
    // Opened without waiting for a writer, so this returns.
    let (tx, rx) = std::sync::mpsc::channel();
    let p = d.join("lib/pipe.qwe");
    std::thread::spawn(move || tx.send(quillo::error::read_text(&p).map_err(|e| e.to_string())));
    assert_eq!(rx.recv_timeout(Duration::from_secs(10)).expect("the read of a FIFO did not return"), Err("it is not a regular file".to_string()));
    let mut c = quillo(&d);
    c.args(["check", "main.qwe"]);
    let r = run_capped(c, 30, 400);
    assert_eq!(r.code, Some(1), "{}", r.err);
    assert!(r.err.contains("cannot read lib/pipe.qwe: it is not a regular file"), "{}", r.err);
}

#[cfg(unix)]
#[test]
fn q982_check_refuses_a_link_to_dev_zero_wherever_the_program_reads_one() {
    let d = dir("q982-zero", &[("main.qwe", "import lib/util.qwe use helper\nroute GET / = helper()\n"), ("ok.qwe", "route GET / = \"plain\"\n")]);
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::create_dir_all(d.join("public")).unwrap();
    std::os::unix::fs::symlink("/dev/zero", d.join("lib/util.qwe")).unwrap();
    std::os::unix::fs::symlink("/dev/zero", d.join("zero.qwe")).unwrap();
    std::os::unix::fs::symlink("/dev/zero", d.join("public/site.css")).unwrap();
    std::os::unix::fs::symlink("/dev/zero", d.join("public/app.js")).unwrap();
    let check = |file: &str| {
        let mut c = quillo(&d);
        c.args(["check", file]);
        run_capped(c, 30, 400)
    };
    // A file it imports.
    let r = check("main.qwe");
    assert!(!r.capped && r.code == Some(1), "{:?} {}", r.code, r.err);
    assert!(r.err.contains("main.qwe:1: error: cannot read lib/util.qwe: it is not a regular file"), "{}", r.err);
    // The program itself, named on the command line: a link a repository holds, not a pipe of
    // the person's under /dev, so held to a regular file.
    let r = check("zero.qwe");
    assert!(!r.capped && r.code == Some(1), "{:?} {}", r.code, r.err);
    assert!(r.err.contains("zero.qwe: error: cannot read file: it is not a regular file"), "{}", r.err);
    // A program beside such style sheets and scripts checks as it would without them.
    let r = check("ok.qwe");
    assert!(!r.capped && r.code == Some(0), "{:?} {}{}", r.code, r.out, r.err);
    // Its app.toml.
    std::os::unix::fs::symlink("/dev/zero", d.join("app.toml")).unwrap();
    let r = check("ok.qwe");
    assert!(!r.capped && r.code == Some(1), "{:?} {}{}", r.code, r.out, r.err);
    assert!(r.err.contains("it is not a regular file"), "{}", r.err);
}

/// A FIFO at `path` with a writer giving `text`: once, as a shell's `<(...)` does, or (`again`)
/// for each reader until `finish`, as a secrets manager does; `finish` says how many it served.
/// Writers one after another are read as one text by a reader that does not stop between them.
#[cfg(unix)]
struct FifoWriter {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    served: Arc<std::sync::atomic::AtomicUsize>,
    thread: std::thread::JoinHandle<()>,
}

#[cfg(unix)]
impl FifoWriter {
    fn new(path: &Path, text: &'static str, again: bool) -> FifoWriter {
        assert!(Command::new("mkfifo").arg(path).status().unwrap().success());
        let (stop, served) = (Arc::new(AtomicBool::new(false)), Arc::new(std::sync::atomic::AtomicUsize::new(0)));
        let (f, s, n) = (path.to_path_buf(), stop.clone(), served.clone());
        let thread = std::thread::spawn(move || {
            while !s.load(Ordering::SeqCst) {
                if std::fs::write(&f, text).is_ok() && !s.load(Ordering::SeqCst) {
                    n.fetch_add(1, Ordering::SeqCst);
                }
                if !again {
                    break;
                }
            }
        });
        FifoWriter { path: path.to_path_buf(), stop, served, thread }
    }

    /// The writer waits for a reader: one that opens and goes lets it see the stop.
    fn finish(self) -> usize {
        self.stop.store(true, Ordering::SeqCst);
        let mut o = std::fs::OpenOptions::new();
        std::os::unix::fs::OpenOptionsExt::custom_flags(o.read(true), libc::O_NONBLOCK);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.thread.is_finished() && Instant::now() < deadline {
            drop(o.open(&self.path));
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(self.thread.is_finished(), "the FIFO's writer did not stop");
        self.served.load(Ordering::SeqCst)
    }
}

/// What the person running the program names is read as it comes, as before: `--env-file
/// /dev/null`, which the messages offer, and a `.env` that is a pipe a secrets manager writes.
#[cfg(unix)]
#[test]
fn q982_env_files_the_person_names_are_read_as_they_come() {
    let d = dir("q982-env", &[("main.qwe", "print(env(\"A\") ?? \"none\")\n")]);
    let run = |args: &[&str]| {
        let mut c = quillo(&d);
        c.arg("run").arg("main.qwe").args(args);
        run_capped(c, 30, 400)
    };
    let r = run(&["--env-file", "/dev/null"]);
    assert_eq!((r.code, r.out.as_str()), (Some(0), "none\n"), "{}", r.err);
    let fifo = FifoWriter::new(&d.join(".env"), "A=from the pipe\n", true);
    let r = run(&[]);
    assert!(fifo.finish() >= 1);
    assert_eq!((r.code, r.out.as_str()), (Some(0), "from the pipe\n"), "{}", r.err);
}

/// The program file named on the command line, and a `--policy`, may be a pipe or a device of
/// the person's, under /dev or /proc: read to the size a source may have, and once (checked, then
/// run, where 0.8.6 ran what the second read found: nothing). Any other name is held to a regular
/// file, so a committed `main.qwe` that is or leads to a FIFO is refused, not waited on.
#[cfg(unix)]
#[test]
fn q982_the_program_named_on_the_command_line_may_be_a_pipe_but_not_endless() {
    let d = dir("q982-named", &[("lib/util.qwe", "fn helper() -> int {\n    return 7\n}\n"), ("ok.qwe", "route GET / = \"x\"\n")]);
    // `exec`, so the cap and the deadline are quillo's.
    let bash = |script: &str| {
        let mut c = Command::new("bash");
        c.current_dir(&d).args(["-c", script, env!("CARGO_BIN_EXE_quillo")]);
        run_capped(c, 30, 400)
    };
    let r = bash("exec \"$0\" run <(printf 'print(1 + 1)\\n')");
    assert_eq!((r.code, r.out.as_str()), (Some(0), "2\n"), "{}", r.err);
    let r = bash("exec \"$0\" run /dev/stdin < <(printf 'print(2 + 3)\\n')");
    assert_eq!((r.code, r.out.as_str()), (Some(0), "5\n"), "{}", r.err);
    let r = bash("exec \"$0\" check <(printf 'route GET / = \"x\"\\n')");
    assert_eq!(r.code, Some(0), "{}{}", r.out, r.err);
    // A `--policy` the person names: its text is read (an unknown key says so), and a good one
    // passes.
    let r = bash("exec \"$0\" check ok.qwe --policy <(printf 'effects = [\"db\"]\\n')");
    assert!(r.code == Some(2) && r.err.contains("/dev/fd/") && r.err.contains("`.effects` is not something a policy says"), "{:?} {}", r.code, r.err);
    let r = bash("exec \"$0\" check ok.qwe --policy <(printf '[effects]\\nallow = [\"db\"]\\n')");
    assert_eq!(r.code, Some(0), "{}{}", r.out, r.err);
    // A device that never ends: read to 16 MiB, then said.
    let mut c = quillo(&d);
    c.args(["check", "/dev/zero"]);
    let r = run_capped(c, 30, 400);
    assert!(!r.capped && r.code == Some(1), "{:?} {}", r.code, r.err);
    assert!(r.err.contains("/dev/zero: error: cannot read file: it is larger than 16 MiB"), "{}", r.err);
    // A FIFO named main.qwe, or a link to one, is refused at once, by every command.
    assert!(Command::new("mkfifo").arg(d.join("pipe")).status().unwrap().success());
    std::os::unix::fs::symlink(d.join("pipe"), d.join("main.qwe")).unwrap();
    assert!(Command::new("mkfifo").arg(d.join("fifo.qwe")).status().unwrap().success());
    for file in ["main.qwe", "fifo.qwe"] {
        for args in [&["check"][..], &["run"], &["docs"]] {
            let mut c = quillo(&d);
            c.args(args).arg(file);
            let r = run_capped(c, 20, 400);
            assert!(r.code.is_some_and(|c| c != 0), "{} {:?}: {:?} {}", file, args, r.code, r.err);
            assert!(r.err.contains("it is not a regular file"), "{} {:?}: {}", file, args, r.err);
        }
    }
    // What a named program imports is still held to a regular file.
    std::fs::remove_file(d.join("lib/util.qwe")).unwrap();
    std::os::unix::fs::symlink("/dev/zero", d.join("lib/util.qwe")).unwrap();
    std::fs::write(d.join("imp.qwe"), "import lib/util.qwe use helper\nroute GET / = helper()\n").unwrap();
    let mut c = quillo(&d);
    c.args(["check", "imp.qwe"]);
    let r = run_capped(c, 20, 400);
    assert!(r.code == Some(1) && r.err.contains("cannot read lib/util.qwe: it is not a regular file"), "{:?} {}", r.code, r.err);
}

/// `quillo lsp` in `d`, under the memory cap: initialize, then didOpen of `file`, and the
/// diagnostics it publishes for it (20 s at most).
fn lsp_diagnostics(d: &Path, file: &str) -> (Vec<String>, bool) {
    lsp_diagnostics_under(d, file, 400)
}

/// `lsp_diagnostics`, the server stopped past `cap_mb`.
fn lsp_diagnostics_under(d: &Path, file: &str, cap_mb: u64) -> (Vec<String>, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("lsp")
        .current_dir(d)
        .env("QUILLO_NATIVE_CACHE", PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-cache"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let capped = cap_memory(child.id(), cap_mb);
    let mut stdin = child.stdin.take().unwrap();
    let out = child.stdout.take().unwrap();
    let mut g = support::Guard::new(child);
    let path = d.join(file).canonicalize().unwrap();
    let uri = format!("file://{}", path.display());
    let send = |w: &mut std::process::ChildStdin, m: serde_json::Value| {
        let b = m.to_string();
        write!(w, "Content-Length: {}\r\n\r\n{}", b.len(), b).unwrap();
        w.flush().unwrap();
    };
    send(&mut stdin, serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "processId": null, "rootUri": format!("file://{}", d.canonicalize().unwrap().display()), "capabilities": {} } }));
    let text = std::fs::read_to_string(&path).unwrap();
    send(&mut stdin, serde_json::json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": { "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } } }));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut r = BufReader::new(out);
        loop {
            let mut len = 0;
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                if let Some(v) = line.strip_prefix("Content-Length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut buf = vec![0; len];
            if r.read_exact(&mut buf).is_err() || tx.send(serde_json::from_slice::<serde_json::Value>(&buf).unwrap_or_default()).is_err() {
                return;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut found = None;
    while found.is_none() && Instant::now() < deadline {
        if let Ok(m) = rx.recv_timeout(Duration::from_millis(100)) {
            if m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri.as_str() {
                found = Some(m["params"]["diagnostics"].as_array().cloned().unwrap_or_default().iter().map(|x| x["message"].as_str().unwrap_or("").to_string()).collect());
            }
        }
        if capped.0.load(Ordering::SeqCst) {
            break;
        }
    }
    let _ = g.0.kill();
    let _ = g.0.wait();
    capped.1.store(true, Ordering::SeqCst);
    let hit = capped.0.load(Ordering::SeqCst);
    (found.unwrap_or_else(|| panic!("no diagnostics for {} (memory cap hit: {})", file, hit)), hit)
}

#[cfg(unix)]
#[test]
fn q982_the_language_server_reads_no_link_to_dev_zero_beside_or_under_a_file() {
    // Beside the file opened: read to find the program's main file.
    let d = dir("q982-lsp", &[("main.qwe", "route GET / = \"plain\"\n")]);
    std::os::unix::fs::symlink("/dev/zero", d.join("zero.qwe")).unwrap();
    let (diags, hit) = lsp_diagnostics(&d, "main.qwe");
    assert!(!hit);
    assert_eq!(diags, Vec::<String>::new());
    // Imported by it.
    let d = dir("q982-lsp-import", &[("main.qwe", "import lib/util.qwe use helper\nroute GET / = helper()\n")]);
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::os::unix::fs::symlink("/dev/zero", d.join("lib/util.qwe")).unwrap();
    let (diags, hit) = lsp_diagnostics(&d, "main.qwe");
    assert!(!hit);
    assert!(diags.iter().any(|m| m == "cannot read lib/util.qwe: it is not a regular file"), "{:?}", diags);
    // A program that uses a regular file is found from it, as before.
    let d = dir("q982-lsp-entry", &[("main.qwe", "import lib/util.qwe use helper\nroute GET / = helper()\n"), ("lib/util.qwe", "fn helper() -> str {\n    return \"hi\"\n}\n")]);
    std::os::unix::fs::symlink("/dev/zero", d.join("lib/zero.qwe")).unwrap();
    let (diags, hit) = lsp_diagnostics(&d, "lib/util.qwe");
    assert!(!hit);
    assert_eq!(diags, Vec::<String>::new());
    // 15 MiB of line breaks beside the file opened: read a line at a time to find the main file.
    // A list of every line took 1.4 GB for 64 MiB; this stays under 150 MB.
    let d = dir("q982-lsp-lines", &[("main.qwe", "route GET / = \"plain\"\n")]);
    std::fs::write(d.join("lines.qwe"), vec![b'\n'; 15 << 20]).unwrap();
    let (diags, hit) = lsp_diagnostics_under(&d, "main.qwe", 150);
    assert!(!hit);
    assert_eq!(diags, Vec::<String>::new());
}

/// The sources of one program come to 32 MiB together at most, for `quillo check`: three files of
/// 12 MiB each are each under 16 MiB, and the third is refused. Parsing takes about seventy times
/// the text.
#[test]
fn q982_one_program_s_sources_come_to_32_mib_together_at_most() {
    let d = dir("q982-total", &[("main.qwe", "import a.qwe use a\nimport b.qwe use b\nimport c.qwe use c\nroute GET / = a() + b() + c()\n")]);
    let line = format!("// {}\n", "x".repeat(1020));
    for (f, name) in [("a.qwe", "a"), ("b.qwe", "b"), ("c.qwe", "c")] {
        let mut text = line.repeat((12 << 20) / line.len());
        text.push_str(&format!("fn {}() -> int {{\n    return 1\n}}\n", name));
        std::fs::write(d.join(f), text).unwrap();
    }
    let mut c = quillo(&d);
    c.args(["check", "main.qwe"]);
    let r = run_capped(c, 60, 2000);
    assert!(!r.capped && r.code == Some(1), "{:?} {}", r.code, r.err);
    assert!(r.err.contains("cannot read c.qwe: the program's files come to more than 32 MiB together"), "{}", r.err);
    assert!(!r.err.contains("cannot read a.qwe") && !r.err.contains("cannot read b.qwe"), "{}", r.err);
    // Two of them pass.
    std::fs::write(d.join("main.qwe"), "import a.qwe use a\nimport b.qwe use b\nroute GET / = a() + b()\n").unwrap();
    let mut c = quillo(&d);
    c.args(["check", "main.qwe"]);
    let r = run_capped(c, 60, 2000);
    assert_eq!(r.code, Some(0), "{}", r.err);
}

// ---- Q-983: the plugin build cache is the user's alone ---------------------------------------

/// A program using plugin `hi.qwer` (and `two.qwer`, with `two`), and a cargo that only says
/// where it runs, what it was asked and what the crate's folder holds, then fails: no Rust is
/// built.
#[cfg(unix)]
fn fake_cargo_program(name: &str, two: bool) -> (PathBuf, PathBuf) {
    let main = if two { "use \"hi.qwer\"\nuse \"two.qwer\"\nprint(twice(21) + 2)\n" } else { "use \"hi.qwer\"\nprint(twice(21))\n" };
    let d = dir(name, &[("app/main.qwe", main), ("app/hi.qwer", "pub fn twice(n: i64) -> i64 { n * 2 }\n"), ("app/two.qwer", "pub fn two() -> i64 { 2 }\n")]);
    let said = d.join("cargo-said.txt");
    let script = format!("#!/bin/sh\necho \"cwd=$(pwd)\" > '{0}'\nprintf '%s\\n' \"$@\" >> '{0}'\nls -a \"$(dirname \"$5\")\" >> '{0}'\nexit 1\n", said.display());
    std::fs::write(d.join("cargo"), script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(d.join("cargo"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir_all(d.join("tmp")).unwrap();
    (d, said)
}

/// `quillo run main.qwe` in `d`/app with only PATH, TMPDIR and CARGO set, the coverage run's own
/// settings kept, and `env` besides.
#[cfg(unix)]
fn run_without_home(d: &Path, env: &[(&str, &Path)]) -> Ran {
    let mut c = quillo(&d.join("app"));
    c.env_clear().env("PATH", "/usr/bin:/bin").env("TMPDIR", d.join("tmp")).env("CARGO", d.join("cargo")).args(["run", "main.qwe"]);
    for k in ["LLVM_PROFILE_FILE", "QUILLO_RUNTIME_COVERAGE"] {
        if let Some(v) = std::env::var_os(k) {
            c.env(k, v);
        }
    }
    for (k, v) in env {
        c.env(k, v);
    }
    run_capped(c, 60, 1000)
}

#[cfg(unix)]
fn uid_here(d: &Path) -> u32 {
    std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(d).unwrap())
}

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    std::os::unix::fs::MetadataExt::mode(&std::fs::metadata(p).unwrap()) & 0o777
}

#[cfg(unix)]
fn chmod(p: &Path, m: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
}

#[cfg(unix)]
#[test]
fn q983_without_home_the_cache_is_a_folder_of_the_user_s_own() {
    let (d, said) = fake_cargo_program("q983-private", false);
    let root = d.join("tmp").join(format!("quillo-{}", uid_here(&d)));
    let r = run_without_home(&d, &[]);
    assert_ne!(r.code, Some(0), "{}", r.err);
    for f in ["", "native", "native/crates", "native/target"] {
        assert_eq!(mode(&root.join(f)), 0o700, "{}", f);
    }
    let asked = std::fs::read_to_string(&said).unwrap_or_else(|_| panic!("cargo was not run: {}", r.err));
    // Cargo runs in `/`, where no other user can put a `.cargo/config.toml`.
    assert!(asked.starts_with("cwd=/\n"), "{}", asked);
    assert!(asked.contains(&format!("{}/native/crates/quillo_hi_", root.display())), "{}", asked);
    assert!(asked.contains(&format!("{}/native/target", root.display())), "{}", asked);
    let crates: Vec<PathBuf> = std::fs::read_dir(root.join("native/crates")).unwrap().flatten().map(|e| e.path()).collect();
    assert_eq!(crates.len(), 1);
    assert_eq!(mode(&crates[0]), 0o700);
    assert!(std::fs::read_to_string(crates[0].join("Cargo.toml")).unwrap().contains("publish = false\nbuild = false\n"));
    // Nothing is made in the shared folder's old place.
    assert!(!d.join("tmp/quillo").exists());
    // A second run takes the folder it made.
    std::fs::remove_file(&said).unwrap();
    let r = run_without_home(&d, &[]);
    assert!(said.exists(), "{}", r.err);
}

/// What another user could have written into a folder they could write in: one file in each of
/// the build's folders, and whether each is still there.
#[cfg(unix)]
fn plant(native: &Path) -> [PathBuf; 3] {
    let files = [native.join("crates/old/x.rs"), native.join("target/release/libx.so"), native.join("workspaces/w/Cargo.toml")];
    for f in &files {
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, "theirs").unwrap();
    }
    files
}

#[cfg(unix)]
#[test]
fn q983_a_cache_others_may_write_in_is_made_the_user_s_and_emptied_another_s_is_refused() {
    let (d, said) = fake_cargo_program("q983-tightened", false);
    // The shared folder, and the home's, made by a umask of 002 or by anyone: made 0700, and what
    // was put there taken away; one others could only read is made 0700 and kept.
    let shared = d.join("tmp").join(format!("quillo-{}", uid_here(&d)));
    let home = d.join("home");
    let home_cache = home.join(".cache/quillo");
    for (root, env, m, emptied) in [
        (&shared, None, 0o777, true),
        (&shared, None, 0o770, true),
        (&home_cache, Some(("HOME", home.as_path())), 0o775, true),
        (&home_cache, Some(("HOME", home.as_path())), 0o702, true),
        (&home_cache, Some(("HOME", home.as_path())), 0o755, false),
    ] {
        let _ = std::fs::remove_dir_all(root);
        let files = plant(&root.join("native"));
        chmod(root, m);
        chmod(&root.join("native"), m);
        let _ = std::fs::remove_file(&said);
        let r = run_without_home(&d, env.as_slice());
        assert!(said.exists(), "{:o} {}: {}", m, root.display(), r.err);
        assert_eq!((mode(root), mode(&root.join("native"))), (0o700, 0o700), "{:o}", m);
        for f in &files {
            assert_eq!(f.exists(), !emptied, "{:o}: {}", m, f.display());
        }
    }
    // A build folder others may write in, under a root that is fine: that folder alone emptied.
    let files = plant(&home_cache.join("native"));
    chmod(&home_cache.join("native/target"), 0o777);
    let _ = std::fs::remove_file(&said);
    let r = run_without_home(&d, &[("HOME", &home)]);
    assert!(said.exists(), "{}", r.err);
    assert_eq!(mode(&home_cache.join("native/target")), 0o700);
    assert_eq!(files.iter().map(|f| f.exists()).collect::<Vec<_>>(), [true, false, true]);
    // One its user did not make: refused, and nothing is built.
    if uid_here(&d) != 0 {
        let _ = std::fs::remove_file(&said);
        let r = run_without_home(&d, &[("QUILLO_NATIVE_CACHE", Path::new("/"))]);
        assert!(r.err.contains("the plugin cache / belongs to another user (uid 0, this is "), "{}", r.err);
        assert!(!said.exists());
    }
}

/// The folders above the cache, to `/`, are checked before anything is changed: one inside HOME
/// that the user owns has its group's and others' write bits taken off, and only those; HOME, or a
/// folder outside it, that others may write in is refused, unless it is sticky.
#[cfg(unix)]
#[test]
fn q983_the_folders_above_the_cache_are_checked_first() {
    let (d, said) = fake_cargo_program("q983-above", false);
    let home = d.join("home");
    let with_home = |more: &[(&str, &Path)]| {
        let _ = std::fs::remove_file(&said);
        let mut env = vec![("HOME", home.as_path())];
        env.extend_from_slice(more);
        run_without_home(&d, &env)
    };
    std::fs::create_dir_all(home.join(".cache")).unwrap();
    chmod(&home.join(".cache"), 0o775);
    let r = with_home(&[]);
    assert!(said.exists(), "{}", r.err);
    assert_eq!(mode(&home.join(".cache")), 0o755);
    // HOME itself: refused, and nothing beneath it changed or taken away.
    let quillo_dir = home.join(".cache/quillo");
    let files = plant(&quillo_dir.join("native"));
    chmod(&quillo_dir, 0o777);
    chmod(&home, 0o777);
    let r = with_home(&[]);
    assert!(!said.exists());
    assert!(r.err.contains(&format!("{}, above the plugin cache, can be written by other users (mode 777), who could put a folder of theirs in the cache's place: nothing is built there; `chmod go-w {}` makes it safe", home.display(), home.display())), "{}", r.err);
    assert!(files.iter().all(|f| f.exists()));
    assert_eq!((mode(&home), mode(&quillo_dir)), (0o777, 0o777));
    chmod(&home, 0o755);
    // Outside HOME: refused; sticky: taken as it is.
    std::fs::create_dir_all(d.join("open/xdg")).unwrap();
    chmod(&d.join("open"), 0o777);
    let r = with_home(&[("XDG_CACHE_HOME", &d.join("open/xdg"))]);
    assert!(!said.exists() && r.err.contains(&format!("{}, above the plugin cache, can be written by other users (mode 777)", d.join("open").display())), "{}", r.err);
    assert_eq!(mode(&d.join("open")), 0o777);
    chmod(&d.join("open"), 0o1777);
    let r = with_home(&[("XDG_CACHE_HOME", &d.join("open/xdg"))]);
    assert!(said.exists(), "{}", r.err);
    // A link of the user's own on the way to the cache is taken, as root's are (`/var`, `/tmp` on
    // macOS lead every test here); one another user owns is refused (unit test in src/native.rs,
    // as only root could make one).
    std::fs::create_dir_all(d.join("real")).unwrap();
    std::os::unix::fs::symlink(d.join("real"), d.join("via")).unwrap();
    let r = with_home(&[("XDG_CACHE_HOME", &d.join("via/xdg"))]);
    assert!(said.exists() && d.join("real/xdg/quillo/native").is_dir(), "{}", r.err);
    // The temporary folder without HOME: the same, sticky or refused.
    let _ = std::fs::remove_file(&said);
    chmod(&d.join("tmp"), 0o777);
    let r = run_without_home(&d, &[]);
    assert!(!said.exists() && r.err.contains(&format!("{}, above the plugin cache, can be written by other users (mode 777)", d.join("tmp").display())), "{}", r.err);
    chmod(&d.join("tmp"), 0o1777);
    let r = run_without_home(&d, &[]);
    assert!(said.exists(), "{}", r.err);
}

/// Under QUILLO_NATIVE_CACHE, a folder of the person's that may hold their own files, nothing is
/// made 0700 or taken away: one others may write in is refused, with the `chmod` that makes it
/// safe, and one they may only read is taken as it is.
#[cfg(unix)]
#[test]
fn q983_under_quillo_native_cache_nothing_is_changed_or_taken_away() {
    let (d, said) = fake_cargo_program("q983-named", false);
    let mine = d.join("mine");
    let theirs = mine.join("crates/core/src/lib.rs");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(&theirs, "pub fn core() {}\n").unwrap();
    for (at, m) in [(mine.clone(), 0o775), (mine.join("crates"), 0o777)] {
        chmod(&at, m);
        let _ = std::fs::remove_file(&said);
        let r = run_without_home(&d, &[("QUILLO_NATIVE_CACHE", &mine)]);
        assert!(!said.exists());
        assert!(r.err.contains(&format!("the plugin cache {} can be written by other users (mode {:o}): nothing is built there; `chmod go-w {}` makes it yours alone", at.display(), m, at.display())), "{}", r.err);
        assert!(theirs.exists());
        assert_eq!(mode(&at), m);
        chmod(&at, 0o755);
    }
    let r = run_without_home(&d, &[("QUILLO_NATIVE_CACHE", &mine)]);
    assert!(said.exists(), "{}", r.err);
    assert_eq!((mode(&mine), mode(&mine.join("crates"))), (0o755, 0o755));
    assert!(theirs.exists());
}

/// A bare `cargo` (or a relative `$CARGO`) is found from the program's folder before cargo runs
/// in `/`: a relative PATH entry still finds it.
#[cfg(unix)]
#[test]
fn q983_cargo_is_found_from_the_program_s_folder() {
    let (d, said) = fake_cargo_program("q983-path", false);
    std::fs::create_dir_all(d.join("app/bin")).unwrap();
    std::fs::copy(d.join("cargo"), d.join("app/bin/cargo")).unwrap();
    for cargo in [None, Some("./bin/cargo")] {
        let _ = std::fs::remove_file(&said);
        let mut c = quillo(&d.join("app"));
        c.env_clear().env("PATH", "bin:/usr/bin:/bin").env("TMPDIR", d.join("tmp")).env("HOME", d.join("home")).args(["run", "main.qwe"]);
        if let Some(v) = cargo {
            c.env("CARGO", v);
        }
        let r = run_capped(c, 60, 1000);
        let asked = std::fs::read_to_string(&said).unwrap_or_else(|_| panic!("{:?}: cargo was not run: {}", cargo, r.err));
        assert!(asked.starts_with("cwd=/\n"), "{}", asked);
    }
}

/// A FAT volume mounted so that every file is 0777 whatever is asked (as exFAT and DrvFs are):
/// the cache on it is not taken away at each build, and that it is not private is said. macOS
/// only (`hdiutil`, `mount_msdos -m 777`); passed over where the volume cannot be made.
#[cfg(target_os = "macos")]
#[test]
fn q983_on_a_filesystem_without_permissions_nothing_is_taken_away_and_it_is_said() {
    let (d, said) = fake_cargo_program("q983-fat", false);
    let home = d.join("home");
    let mount = home.join(".cache");
    std::fs::create_dir_all(&mount).unwrap();
    let ok = |c: &mut Command| c.stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    if ok(Command::new("hdiutil").args(["create", "-size", "4m", "-fs", "MS-DOS", "-volname", "QFAT", "-o"]).arg(d.join("fat.dmg"))).is_none() {
        eprintln!("no FAT image here: passed over");
        return;
    }
    let Some(attached) = ok(Command::new("hdiutil").args(["attach", "-nomount", "-nobrowse"]).arg(d.join("fat.dmg"))) else {
        eprintln!("the FAT image could not be attached: passed over");
        return;
    };
    let devs: Vec<&str> = attached.lines().filter_map(|l| l.split_whitespace().next()).filter(|w| w.starts_with("/dev/")).collect();
    struct Detach(String);
    impl Drop for Detach {
        fn drop(&mut self) {
            let _ = Command::new("hdiutil").args(["detach", &self.0, "-force"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
    }
    let _detach = Detach(devs.first().unwrap().to_string());
    let part = devs.last().unwrap().to_string();
    if ok(Command::new("mount_msdos").args(["-m", "777", &part]).arg(&mount)).is_none() {
        eprintln!("the FAT volume could not be mounted: passed over");
        return;
    }
    struct Unmount(PathBuf);
    impl Drop for Unmount {
        fn drop(&mut self) {
            let _ = Command::new("umount").arg(&self.0).stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
    }
    let _unmount = Unmount(mount.clone());
    let files = plant(&mount.join("quillo/native"));
    for _ in 0..2 {
        let _ = std::fs::remove_file(&said);
        let r = run_without_home(&d, &[("HOME", &home)]);
        assert!(said.exists(), "{}", r.err);
        assert!(r.err.contains("the cache is on a filesystem without permissions: "), "{}", r.err);
        assert!(files.iter().all(|f| f.exists()));
    }
}

#[cfg(unix)]
#[test]
fn q983_a_link_at_any_level_of_the_cache_is_refused_and_not_followed() {
    let (d, said) = fake_cargo_program("q983-links", true);
    let victim = d.join("victim");
    let crate_of = |stem: &str| {
        let abs = d.join("app").join(format!("{}.qwer", stem)).canonicalize().unwrap();
        format!("quillo_{}_{}", stem, &hex_hash(&[&abs.display().to_string()])[..8])
    };
    let (hi, two) = (crate_of("hi"), crate_of("two"));
    let home = d.join("home");
    let cache = d.join("cache");
    let levels: Vec<(&str, Option<(&str, &Path)>, PathBuf)> = vec![
        ("quillo", Some(("HOME", home.as_path())), home.join(".cache/quillo")),
        ("native", Some(("HOME", home.as_path())), home.join(".cache/quillo/native")),
        ("crates", Some(("QUILLO_NATIVE_CACHE", cache.as_path())), cache.join("crates")),
        ("crate", Some(("QUILLO_NATIVE_CACHE", cache.as_path())), cache.join("crates").join(&hi)),
        ("target", Some(("QUILLO_NATIVE_CACHE", cache.as_path())), cache.join("target")),
        ("workspaces", Some(("QUILLO_NATIVE_CACHE", cache.as_path())), cache.join("workspaces")),
        ("workspace", Some(("QUILLO_NATIVE_CACHE", cache.as_path())), cache.join("workspaces").join(hex_hash(&[&hi, &two]))),
        ("workspace, the other order", Some(("QUILLO_NATIVE_CACHE", cache.as_path())), cache.join("workspaces").join(hex_hash(&[&two, &hi]))),
    ];
    for (name, env, at) in levels {
        for p in [&home, &cache, &victim] {
            let _ = std::fs::remove_dir_all(p);
        }
        std::fs::create_dir_all(&victim).unwrap();
        std::fs::write(victim.join("keep.txt"), "mine\n").unwrap();
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&victim, &at).unwrap();
        let _ = std::fs::remove_file(&said);
        let r = run_without_home(&d, env.as_slice());
        assert_ne!(r.code, Some(0), "{}", name);
        // Nothing was written, or taken away, where the link leads.
        assert_eq!(names(&victim), ["keep.txt"], "{}: {}", name, r.err);
        assert_eq!(std::fs::read_to_string(victim.join("keep.txt")).unwrap(), "mine\n");
        assert!(std::fs::symlink_metadata(&at).unwrap().file_type().is_symlink(), "{}", name);
        if name.starts_with("workspace") {
            // The plugins built one by one instead, each in its own crate's folder.
            assert!(said.exists(), "{}: {}", name, r.err);
        } else {
            assert!(r.err.contains(&format!("the plugin cache {} is a link: nothing is built there", at.display())), "{}: {}", name, r.err);
        }
        // The cache's own folder in HOME, a link the person made: QUILLO_NATIVE_CACHE says where.
        if name == "quillo" {
            assert!(r.err.contains(&format!("; set QUILLO_NATIVE_CACHE to the folder it leads to ({}) instead", victim.display())), "{}", r.err);
        }
    }
    // QUILLO_NATIVE_CACHE itself may be a link the person made.
    for p in [&cache, &victim] {
        let _ = std::fs::remove_dir_all(p);
    }
    std::fs::create_dir_all(&victim).unwrap();
    std::os::unix::fs::symlink(&victim, &cache).unwrap();
    let _ = std::fs::remove_file(&said);
    let r = run_without_home(&d, &[("QUILLO_NATIVE_CACHE", &cache)]);
    assert!(said.exists() && victim.join("crates").is_dir(), "{}", r.err);
}

/// What `native::hex_hash` makes of these parts: the first 8 bytes of the SHA-256 of each part
/// and a zero after it, in hex.
fn hex_hash(parts: &[&str]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update([0]);
    }
    h.finalize()[..8].iter().map(|b| format!("{:02x}", b)).collect()
}

/// With real cargo, in the tests' shared cache: a `build.rs`, a `src/` and a link put in a
/// plugin's crate folder are taken away before cargo runs, and one put beside a member of the
/// workspace two plugins are built in together is not run, its manifest saying `build = false`.
#[cfg(unix)]
#[test]
fn q983_a_planted_build_script_is_not_run() {
    let d = dir(
        "q983-planted",
        &[
            ("main.qwe", "use \"hi71da.qwer\"\nuse \"hi71db.qwer\"\nprint(twice(21) + thrice(1))\n"),
            ("hi71da.qwer", "pub fn twice(n: i64) -> i64 { n * 2 }\n"),
            ("hi71db.qwer", "pub fn thrice(n: i64) -> i64 { n * 3 }\n"),
            ("victim.txt", "mine\n"),
        ],
    );
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-cache");
    let crate_of = |stem: &str| {
        let abs = d.join(format!("{}.qwer", stem)).canonicalize().unwrap();
        format!("quillo_{}_{}", stem, &hex_hash(&[&abs.display().to_string()])[..8])
    };
    let (a, b) = (crate_of("hi71da"), crate_of("hi71db"));
    let marker = |n: &str| d.join(format!("ran-{}", n));
    let script = |n: &str| format!("fn main() {{ std::fs::write({:?}, \"ran\").unwrap(); }}\n", marker(n).display().to_string());
    let crate_dir = cache.join("crates").join(&a);
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    std::fs::write(crate_dir.join("build.rs"), script("crate")).unwrap();
    std::fs::write(crate_dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    let _ = std::fs::remove_file(crate_dir.join("Cargo.toml"));
    std::os::unix::fs::symlink(d.join("victim.txt"), crate_dir.join("Cargo.toml")).unwrap();
    for (x, y) in [(&a, &b), (&b, &a)] {
        let m = cache.join("workspaces").join(hex_hash(&[x, y])).join("m");
        for (n, member) in [("first", x), ("second", y)] {
            std::fs::create_dir_all(m.join(member)).unwrap();
            std::fs::write(m.join(member).join("build.rs"), script(&format!("ws-{}-{}", n, member))).unwrap();
        }
    }
    let mut c = quillo(&d);
    c.args(["run", "main.qwe"]).env("QUILLO_NATIVE_CACHE", &cache);
    let r = run_capped(c, 900, 3000);
    assert_eq!((r.code, r.out.trim()), (Some(0), "45"), "{}", r.err);
    assert!(r.err.contains("building") && r.err.contains("together"), "{}", r.err);
    let ran: Vec<String> = names(&d).into_iter().filter(|n| n.starts_with("ran-")).collect();
    assert_eq!(ran, Vec::<String>::new());
    assert_eq!(std::fs::read_to_string(d.join("victim.txt")).unwrap(), "mine\n");
    assert!(!crate_dir.join("build.rs").exists() && !crate_dir.join("src").exists());
    let manifest = std::fs::symlink_metadata(crate_dir.join("Cargo.toml")).unwrap();
    assert!(manifest.file_type().is_file());
    assert!(std::fs::read_to_string(crate_dir.join("Cargo.toml")).unwrap().contains("build = false"));
    // The scripts left beside the members, not run, taken out of the shared cache, and the
    // workspace of the order not built with it.
    for (x, y) in [(&a, &b), (&b, &a)] {
        let ws = cache.join("workspaces").join(hex_hash(&[x, y]));
        if ws.join("Cargo.toml").exists() {
            for member in [x, y] {
                let _ = std::fs::remove_file(ws.join("m").join(member).join("build.rs"));
            }
        } else {
            let _ = std::fs::remove_dir_all(&ws);
        }
    }
}
