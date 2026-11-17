//! FEEDBACK71.md, the security audit's fifth slice (branch security): a pattern compiles to at
//! most 10 MiB, as before, and each thread's cache keeps 32 MB of them, where a visitor's 21
//! bytes of pattern held 11 MB a request and 255 of them 2.8 GB (Q-975); finding every match asks for
//! `--max-compute` at each search and stops a pattern whose searches read the text over and over
//! (Q-976); `--max-memory` holds each value a request makes, as the user decided on 2026-11-17
//! (Q-977): every text a builtin or an answer writes stops as it would pass the limit, and every
//! list and map as it would grow past it; `--split` gives its services `--max-compute` (Q-1001).
//! Every server is one this test starts, on this machine.

mod support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::send;

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback71c-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

fn ev(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => format!("ERROR: {}", e),
    }
}

/// `quillo run` of `file` as a server, its log in `log`, once it says where it listens (20 s at
/// most), on the port the system picks.
fn serve(d: &Path, file: &str, args: &[&str], log: &str) -> (support::Guard, u16) {
    let out = std::fs::File::create(d.join(log)).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", file, "--port", "0"])
        .args(args)
        .current_dir(d)
        .env("QUILLO_SECRET", "a secret for the feedback71c tests, long enough")
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

/// `quillo` with these arguments to the end, given 30 seconds at most: its exit code and stderr.
fn quillo(d: &Path, args: &[&str]) -> (Option<i32>, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).args(args).current_dir(d).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    support::stop_with_tests(&child);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let mut err = String::new();
            child.stderr.take().unwrap().read_to_string(&mut err).unwrap();
            return (status.code(), err);
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return (None, "still running after 30 s".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn get(port: u16, path: &str) -> (u16, String) {
    let a = send(port, "GET", path, &[], "");
    (a.status, a.body)
}

/// `send`, of a body of bytes that need not be text.
fn send_bytes(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> support::Answer {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let mut head = format!("{} {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n", method, path, body.len());
    for (k, v) in headers {
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).unwrap();
    let _ = s.write_all(body);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let status = head.lines().next().and_then(|l| l.split(' ').nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
    support::Answer { status, headers: Vec::new(), body: body.to_string() }
}

/// `quillo run` of `src` as a script, with `args`, given 60 seconds: its exit code, stdout and
/// stderr.
fn script(d: &Path, name: &str, src: &str, args: &[&str]) -> (Option<i32>, String, String) {
    std::fs::write(d.join(name), src).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(name).args(args).current_dir(d).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    support::stop_with_tests(&child);
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let o = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let e = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    let code = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (code, o.join().unwrap(), e.join().unwrap())
}

/// The resident memory of a process, in KB (`ps`, on macOS and Linux).
fn rss_kb(pid: u32) -> u64 {
    let out = Command::new("ps").args(["-o", "rss=", "-p", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0)
}

/// A JSON string of `s`.
fn js(s: &str) -> String {
    serde_json_text(s)
}

fn serde_json_text(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---- Q-975: what a pattern may compile to, and what the cache keeps --------------------------

const RE_SERVER: &str = r#"// A search box that runs the visitor's own pattern on the server (FEEDBACK71.md, Q-975, Q-976).
route GET /valid = re.valid(request.query.p ?? "") ? "ok" : "bad"
route POST /search {
    let q = request.json
    if !re.valid(q.p) {
        return "bad pattern"
    }
    let found = re.find_all(q.text, q.p)
    return is_err(found) ? "scans" : str(len(found))
}
"#;

#[test]
fn a_pattern_compiles_to_10_mib_at_most_and_says_so_past_it() {
    // The regex crate's own limit, as before the slice: `(?:\w{1000}){n}` is 96 KB a time, 109
    // is under 10 MiB, 110 past it (found in review: a limit of 4 MB refused the UI kit's
    // translation checker, and the website no longer started).
    assert!(quillo::re::compile("(?:\\w{1000}){109}", "").is_ok());
    let e = quillo::re::compile("(?:\\w{1000}){110}", "").unwrap_err();
    assert!(e.starts_with("the pattern is too big: it compiles to more than the size limit of 10 MB a pattern has"), "{}", e);
    assert!(e.contains("size limit") && e.contains("(?:\\w{1000}){100}"), "{}", e);
    // The audit's pattern, 9.6 MB, compiles, and what it held is the cache's (see below).
    assert_eq!(ev(r#"re.valid("(?:\\w{1000}){100}")"#), "true");
    let e = quillo::eval_str(r#"re.test("a", "(?:\\w{1000}){110}")"#).err().expect("refused");
    assert!(e.msg.starts_with("`re.test`: the pattern is too big: it compiles to more than the size limit of 10 MB"), "{}", e.msg);
    // With flags, the same.
    assert!(quillo::re::compile("(?:\\w{1000}){110}", "i").is_err());
    assert!(quillo::re::compile("(?:\\w{1000}){109}", "m").is_ok());
    // The UI kit's translation checker (quillo-components/ui/translation_checker.qwe), whole.
    let checker = "%%|\\{\\{\\s{0,9}[\\w.-]{1,100}\\s{0,9}\\}\\}|\\{[\\w.-]{1,100}(?:,[^{}]{0,200}(?:\\{[^{}]{0,200}\\}[^{}]{0,200}){0,20})?\\}|\\$\\{\\w{1,100}\\}|%\\([\\w.-]{1,100}\\)[sdif]|%(?:[0-9]{1,3}\\$)?[-+0#]{0,4}[0-9]{0,4}(?:\\.[0-9]{1,4})?[sdifgxXcu@]";
    assert!(quillo::re::compile(checker, "").is_ok());
    let src = format!("re.find_all(\"Hello {{name}}, {{n, plural, one {{# item}} other {{# items}}}} and %s %1$d ${{x}} {{{{ y }}}}\", {}).map((m) => m.text)", js(checker));
    assert_eq!(ev(&src), r#"["{name}", "{n, plural, one {# item} other {# items}}", "%s", "%1$d", "${x}", "{{ y }}"]"#);
}

#[test]
fn one_repeat_of_any_class_up_to_1000_still_compiles() {
    // The largest single repeats found: JavaScript's \s is many ranges, and \S its complement.
    for (p, f) in [("\\S{0,1000}", ""), ("\\S{0,1000}", "i"), ("[^\\s@]{1,1000}", ""), ("^.{0,1000}$", ""), ("^.{0,1000}$", "s"), ("[^a-z]{0,1000}", "i"), ("\\w{1000}", "")] {
        assert!(quillo::re::compile(p, f).is_ok(), "{} {}", p, f);
    }
    // A repeat of such a repeat, four times, does; five times is past 10 MiB.
    assert!(quillo::re::compile("(?:\\S{0,1000}\\s){4}", "").is_ok());
    assert!(quillo::re::compile("(?:\\S{0,1000}\\s){5}", "").is_err());
    // And matches as before: one over the cache's 64 KB is compiled again at each call, alike.
    let text = format!("{} {}", "b".repeat(1200), "c".repeat(999));
    let src = format!("[re.find_all({0}, \"\\\\S{{1000}}\").map((m) => m.start), re.find_all({0}, \"\\\\S{{1000}}\").map((m) => m.start), re.test({0}, \"^\\\\w{{1,1000}}\")]", js(&text));
    assert_eq!(ev(&src), "[[0], [0], true]");
}

#[test]
fn the_repositories_patterns_compile_and_match_as_before() {
    // The largest of the 171 patterns written in Quillo's repositories, its website and its
    // plugins (all compile to 9 KB or less), and a few longer ones a program could write.
    let cases = [
        (r#"re.test("a-b_c", "^[a-z0-9][a-z0-9_-]{0,62}$")"#, "true"),
        (r#"re.test("ann@example.org", "^[^@\\s]+@[^@\\s]+\\.[^@\\s]+$")"#, "true"),
        (r#"re.find("2026-11-17T10:20:30Z", "^(\\d{4})-(\\d{2})-(\\d{2})(?:[T ](\\d{2}):(\\d{2})(?::(\\d{2}))?(?:\\.\\d+)?)?\\s*(Z|z|[+-]\\d{2}:?\\d{2})?$").groups[0]"#, "\"2026\""),
        (r#"re.test("ZETA", "alpha|beta|gamma|delta|epsilon|zeta|eta|theta|iota|kappa", "i")"#, "true"),
        (r##"re.replace("a1b22", "\\d+", "#")"##, "\"a#b#\""),
        (r#"re.split("a, b,c", ",\\s*")"#, "[\"a\", \"b\", \"c\"]"),
        (r#"len(re.find_all("x y z", "\\w"))"#, "3"),
    ];
    for (src, want) in cases {
        assert_eq!(ev(src), want, "{}", src);
    }
    // Every pattern literal of quillo-app1, quillo-website (its plugins too) and quillo-components,
    // 569, compiles and tests as it did at c616da3 (found in review: the UI kit's translation
    // checker was refused, and the website no longer started).
    let sweep = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/feedback71c_patterns.txt")).unwrap();
    let mut n = 0;
    for line in sweep.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let (want, lit) = line.split_once('\t').unwrap();
        assert_eq!(ev(&format!("re.test(\"\", {})", lit)), want, "{}", lit);
        n += 1;
    }
    assert_eq!(n, 569);
}

#[test]
fn validators_of_any_size_allowed_are_kept_while_they_fit() {
    // The reviewer's validators (75 and 90 times slower when made again at every call): each
    // kept after its first, counted at twice the step it compiled within.
    let pats = ["^.{1,1000}$", "^.{1,255}$", "^.{0,100}$", "^[^\\s@]{1,64}@[^\\s@]{1,255}$", "^\\S{1,500}$", "^\\w{3,32}$"];
    for p in pats {
        let want = if p.starts_with("^.") { "true" } else { "false" };
        assert_eq!(ev(&format!("re.test(\"hello world\", \"{}\")", p.replace('\\', "\\\\"))), want, "{}", p);
        assert!(quillo::re::kept(p, "").is_some(), "{} not kept", p);
    }
    assert_eq!(quillo::re::kept("^\\w{3,32}$", ""), Some(32 << 10));
    assert_eq!(quillo::re::kept("^.{1,1000}$", ""), Some(8 << 20));
    // Past 32 MB in all, the oldest go first: five more of 8 MB each push the first out.
    for i in 0..5 {
        let p = format!("^.{{1,{}}}$", 990 + i);
        assert_eq!(ev(&format!("re.test(\"x\", \"{}\")", p)), "true");
    }
    assert!(quillo::re::kept("^.{1,1000}$", "").is_none());
    assert!(quillo::re::kept("^.{1,994}$", "").is_some());
}

#[test]
fn visitors_patterns_no_longer_hold_the_servers_memory() {
    // The audit's repro: 255 different patterns of its own, each 9.6 MB compiled, and 255 of 3.8
    // MB, each over what the cache keeps beside the others. Before, all were kept: 2 GB; now 60
    // MB, the cache's 32 MB and the pattern being made.
    let d = dir("re-memory", &[("re_srv.qwe", RE_SERVER)]);
    let (g, port) = serve(&d, "re_srv.qwe", &[], "re.log");
    let before = rss_kb(g.0.id());
    for (n, i) in (0..255).map(|i| (100, i)).chain((0..255).map(|i| (40, i))) {
        let p = format!("(?:\\w{{1000}}){{{}}}\\w{{{}}}", n, i);
        let (status, body) = get(port, &format!("/valid?p={}", support_url(&p)));
        assert_eq!((status, body.as_str()), (200, "ok"), "{}", p);
    }
    for i in 0..16 {
        let p = format!("(?:\\w{{1000}}){{110}}\\w{{{}}}", i);
        assert_eq!(get(port, &format!("/valid?p={}", support_url(&p))).1, "bad");
    }
    let after = rss_kb(g.0.id());
    assert!(after < before + 200_000, "held {} KB more", after.saturating_sub(before));
    // A small pattern is kept and found again.
    assert_eq!(get(port, "/valid?p=a%2Bb").1, "ok");
}

fn support_url(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

// ---- Q-976: searches that read the text over and over --------------------------------------

#[test]
fn a_pattern_that_scans_to_the_end_at_each_match_is_an_error_value() {
    // The audit's re1: 300 KB took 11 s; now an error value, once the searches have computed for
    // 1.45 s of the thread's time past 64 readings of the text.
    let t = Instant::now();
    let v = ev(r#"re.find_all("a".repeat(300000), "[a-z]+X|[a-z]")"#);
    assert!(v.starts_with("err(\"`re.find_all`: the pattern scans the text too many times: each search reads on past its match"), "{}", v);
    assert!(v.contains("over these 300000 bytes computed for more than the 1.") && v.contains(" s they may"), "{}", v);
    assert!(t.elapsed() < Duration::from_secs(15), "{:?}", t.elapsed());
    // `re.replace` and `re.split` too, and the program's own tokenizer on its worst text (re3).
    assert!(ev(r#"re.replace("a".repeat(200000), "[a-z]+X|[a-z]", "b")"#).starts_with("err(\"`re.replace`: the pattern scans the text too many times"));
    assert!(ev(r#"re.split("a".repeat(200000), "[a-z]+X|[a-z]")"#).starts_with("err(\"`re.split`: the pattern scans the text too many times"));
    assert!(ev(r#"re.find_all("\"\\".repeat(100000), "\"(?:[^\"\\\\]|\\\\.)*\"|.")"#).starts_with("err(\"`re.find_all`: the pattern scans the text too many times"));
    // `??` takes it, as any error value.
    assert_eq!(ev(r#"len(re.find_all("a".repeat(200000), "[a-z]+X|[a-z]") ?? [])"#), "0");
    assert!(t.elapsed() < Duration::from_secs(90), "{:?}", t.elapsed());
}

#[test]
fn the_same_pattern_on_a_short_text_and_one_search_are_as_before() {
    // 3,000 letters: the searches read 4.5 MB, 1,500 times the text, in about a millisecond, well
    // inside the quarter of a second they have past 64 times.
    assert_eq!(ev(r#"len(re.find_all("a".repeat(3000), "[a-z]+X|[a-z]"))"#), "3000");
    // One search is linear on any text: `re.find` and `re.test` never stop.
    assert_eq!(ev(r#"re.find("a".repeat(300000), "[a-z]+X|[a-z]").end"#), "1");
    assert_eq!(ev(r#"re.test("a".repeat(300000), "[a-z]+X")"#), "false");
    // Empty, one match, unicode, as before.
    assert_eq!(ev(r#"re.find_all("", "a")"#), "[]");
    assert_eq!(ev(r#"re.find_all("é", ".").map((m) => [m.text, m.start, m.end])"#), "[[\"é\", 0, 1]]");
    assert_eq!(ev(r#"re.find_all("ab", "").map((m) => m.start)"#), "[0, 1, 2]");
}

#[test]
fn ordinary_tokenizers_over_large_texts_stay_well_within() {
    // 1.5 MB of words, quotes, numbers and punctuation: 780,000 tokens, split and replaced too.
    let src = r#"
let text = "The quick brown fox, said \"hello \\\"world\\\"\" to 42 lazy dogs; then (it) left. ".repeat(20000)
[len(text), len(re.find_all(text, "\\w+|\\s+|[^\\w\\s]")), len(re.find_all(text, "\"(?:[^\"\\\\]|\\\\.)*\"|.")),
 len(re.split(text, "\\s+")), len(re.replace(text, "\\w+", "w")), len(re.find_all(text, "[^\\n]*\\n?"))]"#;
    assert_eq!(ev(src), "[1540000, 780000, 1220000, 280001, 780000, 2]");
}

#[test]
fn max_compute_ends_a_search_that_reads_the_text_over_and_over() {
    // The audit's repro, served: 1 MB of letters under --max-compute 200ms, where 300 KB had held
    // a worker 10.9 s past --max-compute 2s. The searches would have 4.25 s; the limit comes first.
    let d = dir("re-compute", &[("re_srv.qwe", RE_SERVER)]);
    let (_g, port) = serve(&d, "re_srv.qwe", &["--max-compute", "200ms"], "re.log");
    let body = format!("{{\"p\": \"[a-z]+X|[a-z]\", \"text\": \"{}\"}}", "a".repeat(1_000_000));
    let t = Instant::now();
    let a = send(port, "POST", "/search", &[], &body);
    assert_eq!(a.status, 500, "{}", a.body);
    assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
    let log = std::fs::read_to_string(d.join("re.log")).unwrap();
    assert!(log.contains("POST /search (rid ") && log.contains("this request computed for longer than --max-compute allows"), "{}", log);
    // The server goes on: an ordinary search, and the next pattern.
    let ok = send(port, "POST", "/search", &[], "{\"p\": \"\\\\w+\", \"text\": \"one two three\"}");
    assert_eq!((ok.status, ok.body.as_str()), (200, "3"));
    assert_eq!(get(port, "/valid?p=a").1, "ok");
}

#[test]
fn without_max_compute_the_search_ends_as_an_error_value() {
    let d = dir("re-scans", &[("re_srv.qwe", RE_SERVER)]);
    let (_g, port) = serve(&d, "re_srv.qwe", &[], "re.log");
    let body = format!("{{\"p\": \"[a-z]+X|[a-z]\", \"text\": \"{}\"}}", "a".repeat(300_000));
    let t = Instant::now();
    let a = send(port, "POST", "/search", &[], &body);
    assert_eq!((a.status, a.body.as_str()), (200, "scans"));
    assert!(t.elapsed() < Duration::from_secs(30), "{:?}", t.elapsed());
}

// ---- Q-977: --max-memory, the most one value may take -------------------------------------

const MEM: &str = r#"// What a request makes, at each place a large value is made (FEEDBACK71.md, Q-977).
fn grow(k: int) -> int {
    let s = "xxxxxxxxxxxxxxxx"
    for i in range(0, k) {
        s = s + s
    }
    return len(s)
}
fn grow_in_place(n: int) -> int {
    let s = ""
    let chunk = "y".repeat(1024)
    for i in range(0, n) {
        s += chunk
    }
    return len(s)
}
fn pushes(n: int) -> int {
    let xs = []
    for i in range(0, n) {
        xs = xs | push(i)
    }
    return len(xs)
}
fn doubled(k: int) -> int {
    let s = "zzzzzzzzzzzzzzzz"
    for i in range(0, k) {
        s = `${s}${s}`
    }
    return len(s)
}
fn counts(n: int) -> int {
    let m = {}
    for i in range(0, n) {
        m = m | set(i, get(m, i, 0) + 1)
    }
    return len(keys(m))
}
fn many(n: int) -> list {
    let one = "q".repeat(1000000)
    let xs = []
    for i in range(0, n) {
        xs = xs | push(one)
    }
    return xs
}
route GET /grow ? { k: int } = `${grow(k)} bytes`
route GET /grow_or ? { k: int } = `${grow(k) ?? -1} bytes`
route GET /inplace ? { n: int } = `${grow_in_place(n)} bytes`
route GET /push ? { n: int } = `${pushes(n)} items`
route GET /template ? { k: int } = `${doubled(k)} bytes`
route GET /counts ? { n: int } = `${counts(n)} keys`
route GET /range ? { n: int } = `${len(range(0, n))} items`
route GET /repeat ? { n: int } = `${len("ab".repeat(n))} bytes`
route GET /join ? { n: int } = `${len(join(many(n), ","))} bytes`
route GET /str ? { n: int } = `${len(str(many(n)))} bytes`
route GET /parse ? { n: int } = `${len(json.parse("[" + "1234567,".repeat(n) + "1]"))} values`
route GET /stringify ? { n: int } {
    let one = "w".repeat(100)
    return `${len(json.stringify(range(0, n).map((i) => one)))} bytes`
}
route GET /found ? { n: int } = `${len(re.find_all("a".repeat(n), "a"))} matches`
route GET /file ? { name: str } = `${len(read_file(name))} bytes`
route GET /fetch ? { port: int } = `${len(http.get("http://127.0.0.1:" + str(port) + "/", {}, { public_only: false }).body)} bytes`
route POST /body {
    print("the route ran")
    return `${len(request.json)} values`
}
route GET /two = `${len("x".repeat(40000000)) + len("y".repeat(40000000))} bytes`
route GET /one ? { n: int, ms: int } {
    let s = "z".repeat(n)
    sleep(ms)
    return `${len(s)} bytes`
}
route GET /html ? { n: int } {
    let html = ""
    for i in range(0, n) {
        html = `${html}<li>item number ${i}</li>`
    }
    return `${len(html)} bytes`
}
route GET /par ? { k: int } {
    let r = parallel([() => grow(k), () => grow(k)])
    return `${len(r)} done`
}
route GET /stream ? { n: int } = stream((send) => {
    send("start\n")
    send(many(n))
    send("end\n")
})
route GET /guarded = err("this request's value would be bigger than --max-memory allows (67108864 bytes)") ?? "taken"
route GET /keyed = json.stringify({ "this request's value would be bigger than --max-memory allows (67108864 bytes)": (x) => x }) ?? "taken"
route GET /evil ? { port: int } = http.get("http://127.0.0.1:" + str(port) + "/", {}, { public_only: false }) ?? "taken"
route GET /ok = "ok"
"#;

const STOP: &str = "this request's value would be bigger than --max-memory allows (67108864 bytes)";

/// Each (path, what a server under --max-memory 64MB answers): a value past the limit, refused
/// before it is made at each place a large value is made, and a smaller one, which is not. The
/// loops of `inplace`, `push` and `counts` go to machine code after 256 rounds, which compares as
/// the interpreter does.
const POINTS: &[(&str, u16, &str)] = &[
    // cap2's 16-byte text doubled: at k=22 it is 64 MiB, the limit itself, and at 23 past it.
    ("/grow?k=16", 200, "1048576 bytes"),
    ("/grow?k=22", 200, "67108864 bytes"),
    ("/grow?k=23", 500, "internal error\n"),
    // `??` does not take the stop.
    ("/grow_or?k=23", 500, "internal error\n"),
    ("/inplace?n=1000", 200, "1024000 bytes"),
    ("/inplace?n=70000", 500, "internal error\n"),
    ("/push?n=100000", 200, "100000 items"),
    ("/push?n=4000000", 500, "internal error\n"),
    ("/template?k=16", 200, "1048576 bytes"),
    ("/template?k=23", 500, "internal error\n"),
    ("/counts?n=10000", 200, "10000 keys"),
    ("/counts?n=1000000", 500, "internal error\n"),
    ("/range?n=100000", 200, "100000 items"),
    ("/range?n=4000000", 500, "internal error\n"),
    ("/repeat?n=500000", 200, "1000000 bytes"),
    ("/repeat?n=40000000", 500, "internal error\n"),
    ("/join?n=5", 200, "5000004 bytes"),
    ("/join?n=70", 500, "internal error\n"),
    ("/str?n=5", 200, "5000020 bytes"),
    ("/str?n=70", 500, "internal error\n"),
    // Its list as it grows: 2.5 million places of 32 bytes, 80 MB, from a 20 MB text.
    ("/parse?n=20000", 200, "20001 values"),
    ("/parse?n=2500000", 500, "internal error\n"),
    ("/stringify?n=10000", 200, "1030001 bytes"),
    ("/stringify?n=1000000", 500, "internal error\n"),
    ("/found?n=1000", 200, "1000 matches"),
    ("/found?n=3000000", 500, "internal error\n"),
    // A `parallel` function's stop is the request's.
    ("/par?k=16", 200, "2 done"),
    ("/par?k=23", 500, "internal error\n"),
    ("/ok", 200, "ok"),
];

#[test]
fn max_memory_ends_the_request_whose_value_would_be_bigger_than_it_allows() {
    let d = dir("mem", &[("mem.qwe", MEM)]);
    let (_g, port) = serve(&d, "mem.qwe", &["--max-memory", "64MB"], "mem.log");
    for (path, status, body) in POINTS {
        assert_eq!(get(port, path), (*status, body.to_string()), "{}", path);
    }
    // Each stop said in the log as `--max-compute`'s is: the request, where, and why.
    let log = std::fs::read_to_string(d.join("mem.log")).unwrap();
    assert!(log.contains("quillo: error: GET /grow?k=23 (rid "), "{}", log);
    assert!(log.contains(&format!("mem.qwe:5: {}", STOP)), "{}", log);
    // A template's, at its part's line.
    assert!(log.contains(&format!("mem.qwe:27: {}", STOP)), "{}", log);
    assert_eq!(log.matches(STOP).count(), POINTS.iter().filter(|e| e.1 == 500).count(), "{}", log);
    assert!(log.contains("GET /grow?k=23 -> 500"), "{}", log);
    // And the server answers after them all, again and again.
    for _ in 0..5 {
        assert_eq!(get(port, "/grow?k=22"), (200, "67108864 bytes".into()));
    }
}

#[test]
fn a_page_built_by_a_template_loop_is_held_to_each_text_alone() {
    // 6,000 items, 150 KB, made again at each round: 450 MB made in all, which the first
    // counting (what a request makes) refused even at 512 MB (found in review). Each is 150 KB.
    let d = dir("mem-html", &[("mem.qwe", MEM)]);
    let (_g, port) = serve(&d, "mem.qwe", &["--max-memory", "64MB"], "mem.log");
    assert_eq!(get(port, "/html?n=6000"), (200, "148890 bytes".into()));
    assert_eq!(get(port, "/html?n=20000"), (200, "508890 bytes".into()));
    assert!(!std::fs::read_to_string(d.join("mem.log")).unwrap().contains(STOP));
}

#[test]
fn each_value_is_held_to_the_limit_alone() {
    // Two values of 40 MB in one request, each under 64 MB: made. One of 70 MB: not. And eight at
    // once, each of 40 MB: all made, as the limit is a value's, not the server's (the user's third
    // decision, 2026-11-17).
    let d = dir("mem-each", &[("mem.qwe", MEM)]);
    let (_g, port) = serve(&d, "mem.qwe", &["--max-memory", "64MB"], "mem.log");
    assert_eq!(get(port, "/two"), (200, "80000000 bytes".into()));
    assert_eq!(get(port, "/one?n=70000000&ms=0"), (500, "internal error\n".into()));
    assert_eq!(get(port, "/one?n=67108864&ms=0"), (200, "67108864 bytes".into()));
    assert_eq!(get(port, "/one?n=67108865&ms=0").0, 500);
    let all: Vec<_> = (0..8).map(|_| std::thread::spawn(move || get(port, "/one?n=40000000&ms=300"))).collect();
    for t in all {
        assert_eq!(t.join().unwrap(), (200, "40000000 bytes".into()));
    }
    let log = std::fs::read_to_string(d.join("mem.log")).unwrap();
    assert_eq!(log.matches(STOP).count(), 2, "{}", log);
}

const EDGE: &str = r#"// Each counting point at --max-memory 1MB (1,048,576 bytes): the limit and one byte past it.
route GET /repeat ? { n: int } = `${len("a".repeat(n))}`
route GET /plus ? { n: int } = `${len("a".repeat(n - 1) + "b")}`
route GET /tmpl ? { n: int } {
    let a = "a".repeat(n - 1)
    return `${len(`${a}b`)}`
}
route GET /append ? { n: int } {
    let s = "a".repeat(n - 1)
    s += "b"
    return `${len(s)}`
}
route GET /join ? { n: int } = `${len(join(["a".repeat(n - 1), "b"], ""))}`
route GET /joinany ? { n: int } = `${len(join([1, "a".repeat(n - 1)], ""))}`
route GET /replace ? { n: int } = `${len(replace("a".repeat(n - 2) + "c", "c", "bb"))}`
route GET /range ? { n: int } = `${len(range(0, n))}`
route GET /push ? { n: int } {
    let xs = []
    for i in range(0, n) {
        xs = xs | push(i)
    }
    return `${len(xs)}`
}
route GET /str ? { n: int } = `${len(str(["a".repeat(n - 4)]))}`
route GET /upper ? { n: int } = `${len(upper("a".repeat(n - 6) + "ΐ"))}`
route GET /url ? { n: int } = `${len(url_encode("a".repeat(n - 6) + "é"))}`
route GET /escape ? { n: int } = `${len(escape("a".repeat(n - 6) + "\""))}`
route GET /nfkd ? { n: int } = `${len(normalize("a".repeat(n - 33) + "ﷺ", "NFKD"))}`
route GET /errplus ? { n: int } = `${len("a".repeat(n - 8) + err("x"))}`
route GET /keytext ? { n: int } = `${len(keys(set({}, ["a".repeat(n - 4)], 1))[0])}`
route GET /raw ? { n: int } = `${len(str(raw(["a".repeat(n - 4)])))}`
route GET /flatten ? { n: int } = `${len(flatten([range(0, n - 1), 1]))}`
route GET /spread ? { n: int } = `${len([...range(0, n - 1), 1])}`
route GET /parse ? { n: int } = `${len(json.parse("[" + "1,".repeat(n - 1) + "1]"))}`
fn sets(n: int) -> int {
    let m = {}
    for i in range(0, n) {
        m = set(m, "k" + str(i), i)
    }
    return len(keys(m))
}
fn counted(n: int) -> int {
    let m = {}
    for i in range(0, n) {
        m = set(m, "k" + str(i), get(m, "k" + str(i), 0) + 1)
    }
    return len(keys(m))
}
fn object(n: int) -> str {
    return "{" + join(range(0, n).map((i) => "\"k" + str(i) + "\":1"), ",") + "}"
}
route GET /sets ? { n: int } = `${sets(n)}`
route GET /counted ? { n: int } = `${counted(n)}`
route GET /object ? { n: int } = `${len(keys(json.parse(object(n))))}`
route GET /merge ? { n: int } = `${len(keys(merge(json.parse(object(n - 1)), { z: 1 })))}`
route GET /plusmap ? { n: int } = `${len(keys(json.parse(object(n - 1)) + { z: 1 }))}`
route GET /spreadmap ? { n: int } = `${len(keys({...json.parse(object(n - 1)), z: 1}))}`
route POST /form = `${len(keys(request.form))}`
route GET /stringify ? { n: int } = `${len(json.stringify("a".repeat(n - 2)))}`
route GET /esc ? { n: int } = `${len(str([json.parse("[\"" + "\\u0001".repeat(n) + "\"]")[0]]))}`
route POST /body = `${len(request.body)}`
route GET /file ? { name: str } = `${len(read_file(name))}`
"#;

#[test]
fn the_limit_and_one_byte_past_it_at_each_place() {
    let d = dir("mem-edge", &[("edge.qwe", EDGE)]);
    std::fs::write(d.join("f1.txt"), "a".repeat(1 << 20)).unwrap();
    std::fs::write(d.join("f2.txt"), "a".repeat((1 << 20) + 1)).unwrap();
    let (_g, port) = serve(&d, "edge.qwe", &["--max-memory", "1MB"], "edge.log");
    let l = 1usize << 20;
    // A text by its bytes, as written: each made the limit's length, and one byte past it. Its
    // `len` counts characters: `ΐ` is three of them in upper case (six bytes), and `ﷺ` eighteen in
    // NFKD (33 bytes).
    for (route, chars) in [("repeat", 0), ("plus", 0), ("tmpl", 0), ("append", 0), ("join", 0), ("joinany", 0), ("replace", 0), ("str", 0), ("stringify", 0), ("upper", 3), ("url", 0), ("escape", 0), ("nfkd", 15), ("errplus", 0), ("keytext", 0), ("raw", 0)] {
        assert_eq!(get(port, &format!("/{}?n={}", route, l)), (200, (l - chars).to_string()), "{}", route);
        assert_eq!(get(port, &format!("/{}?n={}", route, l + 1)).0, 500, "{}", route);
    }
    // A list by the room it grows to: 32,768 places of 32 bytes are the limit.
    for route in ["range", "push", "flatten", "spread", "parse"] {
        assert_eq!(get(port, &format!("/{}?n=32768", route)), (200, "32768".into()), "{}", route);
        assert_eq!(get(port, &format!("/{}?n=32769", route)).0, 500, "{}", route);
    }
    // A map by the room it grows to, 80 bytes an entry: 7,168 entries fill the table of 8,192
    // places, and the next grows it to 14,336, 1.1 MB.
    for route in ["sets", "counted", "object", "merge", "plusmap", "spreadmap"] {
        assert_eq!(get(port, &format!("/{}?n=7168", route)), (200, "7168".into()), "{}", route);
        assert_eq!(get(port, &format!("/{}?n=7169", route)).0, 500, "{}", route);
    }
    // A form's fields, as a map, before the route runs.
    let form = |n: usize| (0..n).map(|i| format!("k{}=1", i)).collect::<Vec<_>>().join("&");
    let a = send(port, "POST", "/form", &[("Content-Type", "application/x-www-form-urlencoded")], &form(7168));
    assert_eq!((a.status, a.body.as_str()), (200, "7168"));
    assert_eq!(send(port, "POST", "/form", &[("Content-Type", "application/x-www-form-urlencoded")], &form(7169)).status, 500);
    // A request's body and a file by their length.
    for (body, status) in [("a".repeat(l), 200), ("a".repeat(l + 1), 500)] {
        assert_eq!(send(port, "POST", "/body", &[("Content-Type", "text/plain")], &body).status, status);
    }
    assert_eq!(get(port, "/file?name=f1.txt"), (200, l.to_string()));
    assert_eq!(get(port, "/file?name=f2.txt").0, 500);
    // A text of control characters in a list is written six bytes each (`\u0001`): 174,762 of
    // them are 1,048,576 bytes written, the limit, and one more is past it.
    assert_eq!(get(port, "/esc?n=150000"), (200, "900004".into()));
    assert_eq!(get(port, "/esc?n=174762"), (200, "1048576".into()));
    assert_eq!(get(port, "/esc?n=174763").0, 500);
    let log = std::fs::read_to_string(d.join("edge.log")).unwrap();
    assert!(log.contains("this request's value would be bigger than --max-memory allows (1048576 bytes)"), "{}", log);
}

#[test]
fn without_max_memory_nothing_changes() {
    let d = dir("mem-off", &[("mem.qwe", MEM)]);
    let (_g, port) = serve(&d, "mem.qwe", &[], "mem.log");
    for path in ["/grow?k=23", "/push?n=4000000", "/join?n=70", "/range?n=4000000", "/template?k=23", "/found?n=300000", "/counts?n=300000", "/inplace?n=70000", "/one?n=70000000&ms=0"] {
        let (status, _) = get(port, path);
        assert_eq!(status, 200, "{}", path);
    }
    assert_eq!(get(port, "/grow?k=23").1, "134217728 bytes");
    assert!(!std::fs::read_to_string(d.join("mem.log")).unwrap().contains("would be bigger"));
}

#[test]
fn the_stop_is_told_by_its_kind_and_never_by_its_words() {
    // A server whose `Content-Encoding` says the stop's words, an error value with them, and a
    // map's key with them in `json.stringify`'s error: each an error value `??` takes, with the
    // limit set and not (found in review: the words were taken for the stop).
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let evil = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for c in l.incoming().take(4) {
            let Ok(mut c) = c else { continue };
            let mut buf = [0u8; 4096];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.1 200 OK\r\nContent-Encoding: this request's value would take the server's memory past --max-memory\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi");
        }
    });
    let d = dir("mem-words", &[("mem.qwe", MEM)]);
    for (log, args) in [("off.log", &[][..]), ("on.log", &["--max-memory", "64MB"][..])] {
        let (_g, port) = serve(&d, "mem.qwe", args, log);
        assert_eq!(get(port, &format!("/evil?port={}", evil)), (200, "taken".into()), "{:?}", args);
        assert_eq!(get(port, "/guarded"), (200, "taken".into()), "{:?}", args);
        assert_eq!(get(port, "/keyed"), (200, "taken".into()), "{:?}", args);
    }
}

#[test]
fn a_stream_sending_a_large_value_ends_there() {
    let d = dir("mem-stream", &[("mem.qwe", MEM)]);
    let (_g, port) = serve(&d, "mem.qwe", &["--max-memory", "64MB"], "mem.log");
    // 70 texts of 1 MB as JSON: 140 MB with its chunk's copy. What was sent stays sent; the rest
    // is not made.
    let (status, body) = get(port, "/stream?n=70");
    assert_eq!(status, 200);
    assert!(body.contains("start") && !body.contains("qqqq") && !body.contains("end"), "{}", &body[..body.len().min(200)]);
    let (status, body) = get(port, "/stream?n=2");
    assert!(status == 200 && body.contains("end") && body.len() > 2_000_000, "{}", status);
    let log = std::fs::read_to_string(d.join("mem.log")).unwrap();
    assert!(log.contains("GET /stream?n=70") && log.contains(STOP), "{}", log);
}

#[test]
fn max_memory_holds_each_value_on_every_instance() {
    // `--instances` runs its loops in one process: each value made on any is held to the limit.
    let d = dir("mem-instances", &[("mem.qwe", MEM)]);
    let (_g, port) = serve(&d, "mem.qwe", &["--max-memory", "64MB", "--instances", "2"], "mem.log");
    for _ in 0..3 {
        assert_eq!(get(port, "/grow?k=22"), (200, "67108864 bytes".into()));
        assert_eq!(get(port, "/grow?k=23").0, 500);
        assert_eq!(get(port, "/push?n=4000000").0, 500);
        assert_eq!(get(port, "/ok"), (200, "ok".into()));
    }
}

#[test]
fn a_body_too_large_to_read_ends_its_request_before_the_route_runs() {
    let d = dir("mem-body", &[("mem.qwe", MEM)]);
    let (_g, port) = serve(&d, "mem.qwe", &["--max-memory", "64MB", "--max-body", "64MB"], "mem.log");
    // What each body reads to, held as it is read (FEEDBACK71.md, Q-977, the review's probes):
    // 20 MB of JSON to 2.5 million places of 32 bytes, 80 MB; a map of a million entries of 80
    // bytes; a form of 1.1 million fields; 22 MB of bytes that are not UTF-8, three bytes of
    // U+FFFD each, 66 MB; and a multipart field of them.
    let list = format!("[{}1]", "1234567,".repeat(2_500_000));
    let map = format!("{{{}}}", (0..1_000_000).map(|i| format!("\"k{}\":1", i)).collect::<Vec<_>>().join(","));
    let form = (0..1_100_000).map(|i| format!("f{}=1", i)).collect::<Vec<_>>().join("&");
    let bad: Vec<u8> = vec![0xff; 22 << 20];
    let mut multipart = b"--b\r\nContent-Disposition: form-data; name=\"f\"\r\n\r\n".to_vec();
    multipart.extend_from_slice(&bad);
    multipart.extend_from_slice(b"\r\n--b--\r\n");
    for (ctype, body) in [("application/json", list.as_bytes()), ("application/json", map.as_bytes()), ("application/x-www-form-urlencoded", form.as_bytes()), ("text/plain", &bad[..]), ("multipart/form-data; boundary=b", &multipart[..])] {
        let a = send_bytes(port, "POST", "/body", &[("Content-Type", ctype)], body);
        assert_eq!(a.status, 500, "{}", ctype);
    }
    // 100 KB of JSON, which reads to 3.2 MB, and half the map.
    let small = format!("[{}1]", "1,".repeat(50_000));
    let a = send(port, "POST", "/body", &[], &small);
    assert_eq!((a.status, a.body.as_str()), (200, "50001 values"));
    let half = format!("{{{}}}", (0..400_000).map(|i| format!("\"k{}\":1", i)).collect::<Vec<_>>().join(","));
    let a = send(port, "POST", "/body", &[("Content-Type", "application/json")], &half);
    assert_eq!((a.status, a.body.as_str()), (200, "400000 values"));
    let log = std::fs::read_to_string(d.join("mem.log")).unwrap();
    assert_eq!(log.matches("the route ran").count(), 2, "{}", log);
    assert_eq!(log.matches(STOP).count(), 5, "{}", log);
    assert!(log.contains("quillo: error: POST /body (rid "), "{}", log);
    // Unset, all are read, and the form's fields are all there.
    let (_g2, port2) = serve(&d, "mem.qwe", &["--max-body", "64MB"], "mem2.log");
    for (ctype, body) in [("application/json", map.as_bytes()), ("text/plain", &bad[..])] {
        assert_eq!(send_bytes(port2, "POST", "/body", &[("Content-Type", ctype)], body).status, 200, "{}", ctype);
    }
}

#[test]
fn files_and_answers_are_compared_before_they_are_read() {
    let d = dir("mem-files", &[("mem.qwe", MEM)]);
    std::fs::write(d.join("f2.txt"), "x".repeat(2 << 20)).unwrap();
    std::fs::write(d.join("f80.txt"), "x".repeat(80 << 20)).unwrap();
    // A plain server that answers with as many bytes as its path says, or 30 MB.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let site = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for c in l.incoming().take(4) {
            let Ok(mut c) = c else { continue };
            let mut buf = [0u8; 4096];
            let n = c.read(&mut buf).unwrap_or(0);
            let path = String::from_utf8_lossy(&buf[..n]).split_whitespace().nth(1).unwrap_or("/").to_string();
            let len: usize = path.trim_start_matches('/').parse().unwrap_or(30 << 20);
            let _ = c.write_all(format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", len).as_bytes());
            let _ = c.write_all(&vec![b'x'; len]);
        }
    });
    let (_g, port) = serve(&d, "mem.qwe", &["--max-memory", "16MB"], "mem.log");
    // A file by its size: 2 MB and 80 MB.
    assert_eq!(get(port, "/file?name=f2.txt"), (200, format!("{} bytes", 2 << 20)));
    assert_eq!(get(port, "/file?name=f80.txt").0, 500);
    // An answer is read no further than the limit: 30 MB is past it.
    assert_eq!(get(port, &format!("/fetch?port={}", site)).0, 500);
    let log = std::fs::read_to_string(d.join("mem.log")).unwrap();
    assert!(log.contains("GET /fetch?port=") && log.matches("this request's value would be bigger than --max-memory allows (16777216 bytes)").count() == 2, "{}", log);
    let _ = std::fs::remove_file(d.join("f2.txt"));
    let _ = std::fs::remove_file(d.join("f80.txt"));
}

#[test]
fn max_memory_takes_a_size_and_refuses_anything_else_at_once() {
    let d = dir("mem-options", &[("mem.qwe", MEM)]);
    for bad in ["0", "abc", "1KB", "65535", "-5", "12XB", ""] {
        let (code, err) = quillo(&d, &["run", "mem.qwe", "--port", "0", "--max-memory", bad]);
        assert_eq!(code, Some(2), "{:?}: {}", bad, err);
        assert!(err.contains(&format!("quillo: --max-memory is a size of 64KB or more, such as 512MB or 1GB (or a number of bytes), got {:?}", bad)), "{:?}: {}", bad, err);
    }
    // `=`, plain bytes and any case are taken: the least, 64 KB, is a value of 64 KB.
    let (_g, port) = serve(&d, "mem.qwe", &["--max-memory=65536"], "mem.log");
    assert_eq!(get(port, "/repeat?n=32768"), (200, "65536 bytes".into()));
    assert_eq!(get(port, "/repeat?n=32769").0, 500);
    let (_g2, port2) = serve(&d, "mem.qwe", &["--max-memory", "1gb"], "mem2.log");
    assert_eq!(get(port2, "/grow?k=24"), (200, "268435456 bytes".into()));
}

#[test]
fn a_policy_sets_max_memory_and_its_message_names_it() {
    let d = dir("mem-policy", &[("mem.qwe", MEM), ("quillo.policy", "[limits]\nmax_memory = \"64MB\"\n")]);
    let (_g, port) = serve(&d, "mem.qwe", &[], "mem.log");
    assert_eq!(get(port, "/grow?k=22"), (200, "67108864 bytes".into()));
    assert_eq!(get(port, "/grow?k=23").0, 500);
    let log = std::fs::read_to_string(d.join("mem.log")).unwrap();
    assert!(log.contains("this request's value would be bigger than the policy ") && log.contains("quillo.policy allows (max_memory, 67108864 bytes)"), "{}", log);
    // A larger option is narrowed by it; a smaller one stays.
    let (_g2, port2) = serve(&d, "mem.qwe", &["--max-memory", "1GB"], "mem2.log");
    assert_eq!(get(port2, "/grow?k=23").0, 500);
    let (_g3, port3) = serve(&d, "mem.qwe", &["--max-memory", "16MB"], "mem3.log");
    assert_eq!(get(port3, "/grow?k=20"), (200, "16777216 bytes".into()));
    assert_eq!(get(port3, "/grow?k=21").0, 500);
    assert!(std::fs::read_to_string(d.join("mem3.log")).unwrap().contains("--max-memory allows (16777216 bytes)"));
    // A policy's nonsense is refused as it is read.
    for (bad, said) in [("\"1KB\"", "`max_memory` is a size of 64KB or more"), ("\"lots\"", "`max_memory` is a size such as \"512MB\""), ("0", "`max_memory` is a size of 64KB or more")] {
        let e = dir("mem-policy-bad", &[("mem.qwe", MEM), ("quillo.policy", &format!("[limits]\nmax_memory = {}\n", bad))]);
        let (code, err) = quillo(&e, &["run", "mem.qwe", "--port", "0"]);
        assert_ne!(code, Some(0), "{}", bad);
        assert!(err.contains(said), "{}: {}", bad, err);
    }
}

#[test]
fn the_builtins_that_ask_give_what_they_gave() {
    // join of texts, of anything, and empty; replace growing, shrinking and of ""; a list another
    // holds pushed onto; split of ""; unset, as the reviewer's probe compared them (p1.qwe).
    let cases = [
        (r#"join([], ",")"#, r#""""#),
        (r#"join(["a", "b", "c"], ", ")"#, r#""a, b, c""#),
        (r#"join([1, 2.5, null, true, [1, "x"], {a: 1, b: "y"}], "|")"#, r#""1|2.5|null|true|[1, \"x\"]|{ a: 1, b: \"y\" }""#),
        (r#"join(["é", "ü", "日本"], "→")"#, r#""é→ü→日本""#),
        (r#"join([err("boom"), "x"], ";")"#, r#""err(\"boom\");x""#),
        (r#"len(join(range(0, 30000).map((i) => "abc"), "--"))"#, "149998"),
        (r#"len(join(range(0, 30000), ""))"#, "138890"),
        (r#"replace("aaa", "a", "bb")"#, r#""bbbbbb""#),
        (r#"replace("abc", "", "-")"#, r#""-a-b-c-""#),
        (r#"len(replace("a".repeat(100000), "a", "bcd"))"#, "300000"),
        (r#"len(replace("é".repeat(40000), "", "x"))"#, "80001"),
        (r#"let xs = [1, 2]
let ys = push(xs, 3)
[xs, ys]"#, "[[1, 2], [1, 2, 3]]"),
        (r#"split("héllo", "")"#, r#"["h", "é", "l", "l", "o"]"#),
    ];
    for (src, want) in cases {
        assert_eq!(ev(src), want, "{}", src);
    }
}

/// The command line `quillo run main.qwe --split` with `opts` starts its service `work` with, once
/// the app answers through it; everything it started is stopped after.
fn split_service_line(name: &str, opts: &[&str]) -> String {
    let d = dir(name, &[("work.qwe", "server fn add(a: int, b: int) -> int = a + b\n"), ("main.qwe", "service work = \"work.qwe\"\nroute GET /sum = str(work.add(2, 3))\n")]);
    let log = std::fs::File::create(d.join("split.log")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--port", "0", "--split"])
        .args(opts)
        .current_dir(&d)
        .env("QUILLO_SECRET", "a secret for the feedback71c tests, long enough")
        .env("QUILLO_SERVICE_SECRET", "a service secret for the feedback71c tests, long enough")
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(30);
    // The system picks the app's port: its own line says it, a service's is marked `[work]`.
    loop {
        let text = std::fs::read_to_string(d.join("split.log")).unwrap_or_default();
        let port = text.lines().filter(|l| !l.starts_with('[')).find_map(|l| l.split(" listening on http://127.0.0.1:").nth(1)?.split(|c: char| !c.is_ascii_digit()).next()?.parse::<u16>().ok());
        if port.is_some_and(|p| support::listening(p) && send(p, "GET", "/sum", &[], "").body == "5") {
            break;
        }
        assert!(Instant::now() < deadline, "{}", std::fs::read_to_string(d.join("split.log")).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(100));
    }
    let children = |pid: u32| -> Vec<(u32, String)> {
        let ps = Command::new("ps").args(["-axo", "pid=,ppid=,command="]).output().unwrap();
        String::from_utf8_lossy(&ps.stdout)
            .lines()
            .filter_map(|l| {
                let mut w = l.split_whitespace();
                let (c, p) = (w.next()?.parse::<u32>().ok()?, w.next()?.parse::<u32>().ok()?);
                (p == pid).then(|| (c, w.collect::<Vec<_>>().join(" ")))
            })
            .collect()
    };
    let app = g.0.id();
    let kids = children(app);
    let service = kids.iter().find(|(_, c)| c.contains("--service-name work")).cloned();
    let _ = Command::new("kill").args(["-INT", &app.to_string()]).status();
    let until = Instant::now() + Duration::from_secs(10);
    while g.0.try_wait().ok().flatten().is_none() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    for (pid, _) in &kids {
        if children(1).iter().any(|(p, c)| p == pid && c.contains("--as-service")) {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
    }
    service.unwrap_or_else(|| panic!("no service started: {:?}", kids)).1
}

#[test]
fn split_gives_its_services_max_memory() {
    let line = split_service_line("mem-split", &["--max-memory", "64MB"]);
    assert!(line.contains("--max-memory 67108864"), "{}", line);
    assert!(!line.contains("--max-compute"), "{}", line);
}

// ---- Q-1001: --split and --max-compute ------------------------------------------------------

#[test]
fn split_gives_its_services_max_compute() {
    // Its services' requests ran with no limit where the app's had one: given as it was set, in
    // ms, `2s` and `250ms` alike, and nothing where none was.
    let line = split_service_line("compute-split", &["--max-compute", "2s"]);
    assert!(line.contains("--max-compute 2000ms"), "{}", line);
    assert!(!line.contains("--max-memory"), "{}", line);
    let line = split_service_line("compute-split-ms", &["--max-compute=250ms", "--max-memory", "1GB"]);
    assert!(line.contains("--max-compute 250ms") && line.contains("--max-memory 1073741824"), "{}", line);
    let line = split_service_line("compute-split-none", &[]);
    assert!(!line.contains("--max-compute") && !line.contains("--max-memory"), "{}", line);
}

/// The review's probes (review-sec5c, scaled to run quickly): each refused under its limit, said
/// at its line as the stop, and the same program giving what it gives with a limit that holds it,
/// or none (FEEDBACK71.md, Q-977, the bounded writers; branch security).
const PROBES: &[(&str, &str, &str, &str, &str)] = &[
    // (name, program, refused under, given under, what it prints)
    ("raw", "let s = \"x\".repeat(100000)\nlet l = range(0, 30).map((i) => s)\nprint(len(str(raw(l))))\n", "1MB", "16MB", "3000120"),
    ("escape", "let s = \"x\".repeat(100000)\nlet l = range(0, 30).map((i) => s)\nprint(len(escape(l)))\n", "1MB", "16MB", "3000420"),
    ("errdisp", "let s = \"x\".repeat(100000)\nlet e = err(range(0, 30).map((i) => s))\nprint(len(str(e)))\n", "1MB", "16MB", "3000125"),
    ("errplus", "let s = \"x\".repeat(100000)\nlet e = err(range(0, 30).map((i) => s))\nprint(len(\"a\" + e))\n", "1MB", "16MB", "3000126"),
    ("join", "let s = \"x\".repeat(100000)\nlet l = range(0, 30).map((i) => s) + [1]\nprint(len(join(l, \",\")))\n", "1MB", "16MB", "3000031"),
    ("keytext", "let s = \"x\".repeat(100000)\nlet l = range(0, 30).map((i) => s)\nlet m = set({}, l, 1)\nprint(len(keys(m)[0]))\n", "1MB", "16MB", "3000120"),
    ("flatten", "let l = range(0, 300)\nlet ll = range(0, 300).map((i) => l)\nprint(len(flatten(ll)))\n", "1MB", "16MB", "90000"),
    ("spread", "let l = range(0, 3000)\nlet x = [...l, ...l, ...l, ...l, ...l, ...l, ...l, ...l, ...l, ...l, ...l, ...l]\nprint(len(x))\n", "1MB", "16MB", "36000"),
    ("merge", "let a = {}\nlet b = {}\nfor i in range(0, 7000) { a = set(a, \"a\" + str(i), 1) }\nfor i in range(0, 7000) { b = set(b, \"b\" + str(i), 1) }\nprint(len(keys(merge(a, b))))\n", "1MB", "16MB", "14000"),
    ("mapplus", "let a = {}\nlet b = {}\nfor i in range(0, 7000) { a = set(a, \"a\" + str(i), 1) }\nfor i in range(0, 7000) { b = set(b, \"b\" + str(i), 1) }\nprint(len(keys(a + b)))\n", "1MB", "16MB", "14000"),
    ("spreadmap", "let a = {}\nlet b = {}\nfor i in range(0, 7000) { a = set(a, \"a\" + str(i), 1) }\nfor i in range(0, 7000) { b = set(b, \"b\" + str(i), 1) }\nprint(len(keys({...a, ...b, ...a})))\n", "1MB", "16MB", "14000"),
    ("md", "let url = \"http://e.com/\" + \"x\".repeat(600000)\nlet src = \"[a]: \" + url + \"\\n\\n\" + \"[a] \".repeat(20)\nprint(len(markdown(src)))\n", "1MB", "16MB", "1200139"),
    ("nfkd", "let s = \"ﷺ\".repeat(40000)\nprint(len(normalize(s, \"NFKD\")))\n", "1MB", "16MB", "720000"),
    ("upper", "let s = \"ΐ\".repeat(200000)\nprint(len(upper(s)))\n", "1MB", "16MB", "600000"),
    ("lower", "let s = \"Ⱥ\".repeat(400000)\nprint(len(lower(s)))\n", "1MB", "16MB", "400000"),
    ("urlenc", "let s = \"é\".repeat(200000)\nprint(len(url_encode(s)))\n", "1MB", "16MB", "1200000"),
    ("floats", "let l = range(0, 4000).map((i) => 1e308)\nprint(len(str(l)))\n", "1MB", "16MB", "1244000"),
    ("deep", "let v = []\nfor i in range(0, 2000) { v = [v] }\nprint(len(json.stringify(v, true)))\n", "1MB", "16MB", "8008002"),
    ("counts", "let m = {}\nfor i in range(0, 30000) {\n    m = set(m, str(i), get(m, str(i), 0) + 1)\n}\nprint(len(keys(m)))\n", "1MB", "16MB", "30000"),
    ("counts_int", "let m = {}\nfor i in range(0, 30000) {\n    m = set(m, i, get(m, i, 0) + 1)\n}\nprint(len(keys(m)))\n", "1MB", "16MB", "30000"),
    ("nested", "let m = {}\nfor i in range(0, 30000) {\n    m = set(m, str(i), get(m, str(i), []) | push(i))\n}\nprint(len(keys(m)))\n", "1MB", "16MB", "30000"),
    ("set2", "let g = { a: {} }\nfor i in range(0, 30000) {\n    g = set(g, \"a\", set(g[\"a\"], str(i), i))\n}\nprint(len(keys(g[\"a\"])))\n", "1MB", "16MB", "30000"),
    ("listlist", "let g = [[]]\nfor i in range(0, 60000) {\n    g = set(g, 0, g[0] | push(i))\n}\nprint(len(g[0]))\n", "1MB", "16MB", "60000"),
    ("groupby", "print(len(keys(group_by(range(0, 30000), (i) => i))))\n", "1MB", "16MB", "30000"),
    ("groups", "let s = \"b\".repeat(3000)\nlet p = \"(a)?\".repeat(100)\nprint(len(re.find_all(s, p)))\n", "1MB", "16MB", "3001"),
    ("groups2", "let s = \"b\".repeat(3000)\nlet p = \"(a)?\".repeat(100)\nprint(len(re.replace(s, p, \"\")))\n", "1MB", "16MB", "3000"),
    ("groups3", "let s = \"b\".repeat(3000)\nlet p = \"(a)?\".repeat(100)\nprint(len(re.split(s, p)))\n", "1MB", "16MB", "3002"),
    ("rereplace", "print(len(re.replace(\"a\".repeat(1000), \"a\", \"x\".repeat(2000))))\n", "1MB", "16MB", "2000000"),
    ("vec", "print(len(vec.zeros(40000)))\n", "1MB", "16MB", "40000"),
    ("guard", "fn big() -> str {\n    return \"x\".repeat(2000000)\n}\nprint(len([1].map((i) => big()) ?? []))\n", "1MB", "16MB", "1"),
    ("guard2", "fn big() -> str {\n    return \"x\".repeat(2000000)\n}\nlet r = parallel([() => big(), () => \"y\"]) ?? \"caught\"\nprint(len(r))\n", "1MB", "16MB", "2"),
    ("guard3", "let src = \"let s = \\\"xxxxxxxxxxxxxxxx\\\"\\nfor i in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13] {\\n    s = s + s\\n}\\nlen(s)\"\nprint(run_script(src) ?? \"caught\")\n", "64KB", "1MB", "131072"),
    ("guard4", "fn big(n: int) -> str {\n    return \"x\".repeat(n)\n}\nlet r = [3, 1, 2].sort_by((i) => big(2000000)) ?? \"caught\"\nprint(r)\n", "1MB", "16MB", "[3, 1, 2]"),
    ("ctl", "let s = json.parse(\"\\\"\\\\u0001\\\"\").repeat(200000)\nprint(len(json.stringify(s)))\n", "1MB", "16MB", "1200002"),
    ("dbtext", "db.open(\"q.db\")\nlet a = db.query(\"select printf('%.*c', 2000000, 'x') as s\")\nprint(len(a[0].s))\n", "1MB", "16MB", "2000000"),
    ("dbrows", "db.open(\"q.db\")\nlet b = db.query(\"with recursive c(x) as (select 1 union all select x + 1 from c limit 300000) select x from c\")\nprint(len(b))\n", "1MB", "16MB", "300000"),
];

#[test]
fn the_reviews_probes_are_refused_under_their_limit_and_given_above_it() {
    let d = dir("mem-probes", &[]);
    let stop = |limit: &str| format!("this request's value would be bigger than --max-memory allows ({} bytes)", quillo::policy::size(limit).unwrap());
    for (name, src, under, over, want) in PROBES {
        let file = format!("{}.qwe", name);
        let (code, out, err) = script(&d, &file, src, &["--max-memory", under]);
        assert_ne!(code, Some(0), "{} under {}: {}{}", name, under, out, err);
        assert!(err.contains(&format!("quillo: {}:", file)) && err.contains(&stop(under)), "{} under {}: {}", name, under, err);
        let (code, out, err) = script(&d, &file, src, &["--max-memory", over]);
        assert_eq!((code, out.trim()), (Some(0), *want), "{} under {}: {}", name, over, err);
        let (code, out, err) = script(&d, &file, src, &[]);
        assert_eq!((code, out.trim()), (Some(0), *want), "{} unset: {}", name, err);
    }
}

#[test]
fn texts_are_counted_as_they_are_written_and_never_taken_for_more() {
    // What a count before the writing refused, or let through, under 64 MB (found in review):
    // a list of 1.4 million zeros is 4.2 MB written, its numbers to 1,400,000 11.5 MB, as many
    // bools 8.4 MB, rows of zeros joined 3.6 MB; and a zip's list is its own 960 KB.
    let d = dir("mem-false", &[]);
    for (name, src, want) in [
        ("zeros", "let l = range(0, 1400000).map((i) => 0)\nprint(len(str(l)))\n", "4200000"),
        ("ints", "let l = range(0, 1400000)\nprint(len(str(l)))\n", "11488890"),
        ("bools", "let l = range(0, 1400000).map((i) => true)\nprint(len(`${l}`))\n", "8400000"),
        ("rows", "let rows = range(0, 2000).map((i) => range(0, 600).map((j) => 0))\nprint(len(join(rows, \"\\n\")))\n", "3601999"),
    ] {
        let (code, out, err) = script(&d, &format!("{}.qwe", name), src, &["--max-memory", "64MB"]);
        assert_eq!((code, out.trim()), (Some(0), want), "{}: {}", name, err);
    }
    let (code, out, err) = script(&d, "zip.qwe", "let l = range(0, 30000)\nprint(len(zip(l, l)))\n", &["--max-memory", "1MB"]);
    assert_eq!((code, out.trim()), (Some(0), "30000"), "{}", err);
    // A list the program keeps another copy of is that list, as before.
    let alias = "let a = [1, 2, 3, 4]\nlet b = a\nfor i in range(0, 5) {\n    a = a | push(i)\n}\nlet c = a\na = push(a, 99)\nlet m = { k: [1] }\nlet keep = m[\"k\"]\nm = set(m, \"k\", get(m, \"k\", []) | push(2))\nlet xs = []\nlet snaps = []\nfor i in range(0, 3) {\n    xs = xs | push(i)\n    snaps = snaps | push(xs)\n}\nprint(`${b} ${len(c)} ${len(a)} ${keep} ${m} ${snaps}`)\n";
    for args in [&["--max-memory", "64MB"][..], &[]] {
        let (code, out, err) = script(&d, "alias.qwe", alias, args);
        assert_eq!((code, out.trim()), (Some(0), "[1, 2, 3, 4] 9 10 [1] { k: [1, 2] } [[0], [0, 1], [0, 1, 2]]"), "{:?}: {}", args, err);
    }
}

const ANSWERS: &str = r#"route GET /ok = "ok"
route GET /ans ? { n: int } {
    let s = "x".repeat(300000)
    return range(0, n).map((i) => s)
}
component Row(t: str) {
    <li>{t}</li>
}
route GET /page ? { n: int } {
    let s = "x".repeat(300000)
    let l = range(0, n).map((i) => s)
    return <ul>{l.map((t) => <Row t={t}/>)}</ul>
}
route GET /html ? { n: int } {
    let s = "x".repeat(300000)
    let l = range(0, n).map((i) => s)
    return <ul>{l.map((t) => <li>{t}</li>)}</ul>
}
route GET /for ? { n: int } {
    let s = "x".repeat(300000)
    <ul>{for i in range(0, n) { <li>{s}</li> }}</ul>
}
route GET /attr ? { n: int } {
    let s = "x".repeat(300000)
    <div data-x={range(0, n).map((i) => s)}>a</div>
}
route GET /stream ? { n: int } = stream((send) => {
    send("start\n")
    send({ a: range(0, n).map((i) => "x".repeat(300000)) })
    send("end\n")
})
route GET /fifo = `${len(read_file("fifo"))} bytes`
route GET /ff = `${len(read_file("ff.bin"))} bytes`
route GET /fetch ? { port: int } = `${len(http.get("http://127.0.0.1:" + str(port) + "/", {}, { public_only: false }).body)}`
"#;

#[test]
fn answers_pages_and_components_are_held_as_they_are_written() {
    // A route's JSON, a page of elements, of components (whose props are in the page too), a
    // `for`'s, an attribute's JSON and a stream's piece: each a value the request makes, refused
    // as it passes 1 MB, and made under it (FEEDBACK71.md, Q-977, found in review: 180 MB of JSON
    // and a 1.26 GB page under 1 MB).
    let d = dir("mem-answers", &[("ans.qwe", ANSWERS)]);
    std::fs::write(d.join("ff.bin"), vec![0xffu8; 600_000]).unwrap();
    let (_g, port) = serve(&d, "ans.qwe", &["--max-memory", "1MB"], "ans.log");
    for path in ["/ans?n=4", "/page?n=2", "/html?n=4", "/for?n=4", "/attr?n=4"] {
        assert_eq!(get(port, path).0, 500, "{}", path);
    }
    for (path, len) in [("/ans?n=1", 300_004), ("/html?n=1", 300_018), ("/for?n=3", 900_036), ("/attr?n=1", 300_036)] {
        let (status, body) = get(port, path);
        assert_eq!((status, body.len()), (200, len), "{}", path);
    }
    let (status, body) = get(port, "/page?n=1");
    assert!(status == 200 && body.contains("data-c=\"Row\"") && body.len() > 600_000, "{} {}", status, body.len());
    let (status, body) = get(port, "/stream?n=4");
    assert!(status == 200 && body.contains("start") && !body.contains("end"), "{}", status);
    // A file read as text: 600 KB of 0xFF are 1.8 MB of U+FFFD.
    assert_eq!(get(port, "/ff").0, 500);
    assert_eq!(get(port, "/ok"), (200, "ok".into()));
    let log = std::fs::read_to_string(d.join("ans.log")).unwrap();
    assert_eq!(log.matches("this request's value would be bigger than --max-memory allows (1048576 bytes)").count(), 7, "{}", log);
    // Unset, every one is made.
    let (_g2, port2) = serve(&d, "ans.qwe", &[], "ans2.log");
    for (path, len) in [("/ans?n=4", 1_200_013), ("/html?n=4", 1_200_045), ("/for?n=4", 1_200_045)] {
        let (status, body) = get(port2, path);
        assert_eq!((status, body.len()), (200, len), "{}", path);
    }
    assert_eq!(get(port2, "/ff"), (200, "600000 bytes".into()));
}

#[test]
fn a_file_and_an_answer_are_read_no_further_than_the_limit() {
    // A FIFO says nothing of its size, and gives 3 MB; an answer of 1 MB of 0xFF is 3 MB as text.
    // Each refused under 1 MB, as it is read, and given unset (FEEDBACK71.md, Q-977, found in
    // review; branch security).
    let d = dir("mem-fifo", &[("ans.qwe", ANSWERS)]);
    let fifo = d.join("fifo");
    let _ = std::fs::remove_file(&fifo);
    assert!(Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    let feed = |fifo: PathBuf| {
        std::thread::spawn(move || {
            if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(&fifo) {
                let _ = f.write_all(&vec![b'y'; 3 << 20]);
            }
        })
    };
    let site = || {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Some(Ok(mut c)) = l.incoming().next() {
                let mut buf = [0u8; 4096];
                let _ = c.read(&mut buf);
                let _ = c.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 1000000\r\nconnection: close\r\n\r\n");
                let _ = c.write_all(&vec![0xffu8; 1_000_000]);
            }
        });
        port
    };
    for (args, fifo_status, fetch_status) in [(&["--max-memory", "1MB"][..], 500, 500), (&[][..], 200, 200)] {
        let (_g, port) = serve(&d, "ans.qwe", args, "fifo.log");
        let writer = feed(fifo.clone());
        assert_eq!(get(port, "/fifo").0, fifo_status, "{:?}", args);
        // The writer ends once nothing reads: a refused read closes the FIFO.
        let _ = writer.join();
        let (status, body) = get(port, &format!("/fetch?port={}", site()));
        assert_eq!(status, fetch_status, "{:?}: {}", args, body);
        if status == 200 {
            assert_eq!(body, "1000000");
        }
    }
    let _ = std::fs::remove_file(&fifo);
}

/// What review-sec5d found made whole before any stop: a list of `n` holds of one 1 MB text, in
/// each place a page or an answer writes it (FEEDBACK71.md, Q-977; branch security).
const WHOLE: &str = r#"let one = "q".repeat(1000000)
fn many(n: int) -> list {
    let xs = []
    for i in range(0, n) {
        xs = xs | push(one)
    }
    return xs
}
fn mmap(n: int) -> map = { a: many(n) }
component Show(items: list) {
    <ul>{len(items)}</ul>
}
server fn big(n: int) -> list = many(n)
server fn slow(n: int) -> str = str(many(n))
component Later(n: int) {
    state b = defer(slow(n))
    <i>{len(b ?? "")}</i>
}
route GET /ok = "ok"
route GET /p ? { n: int } = <html><body><p>{many(n)}</p></body></html>
route GET /href ? { n: int } = <html><body><a href={`/x/${many(n)}`}>"x"</a></body></html>
route GET /script ? { n: int } = <html><body><script>let a = {many(n)};</script></body></html>
route GET /island ? { n: int } = <html><body><Show items={many(n)}/></body></html>
route GET /print ? { n: int } {
    print(`${many(n)}`)
    return "ok"
}
route GET /json ? { n: int } = many(n)
route GET /hrefv ? { n: int } = <html><body><a href={many(n)}>"x"</a></body></html>
route GET /title ? { n: int } = <html><body><p title={many(n)}>"x"</p></body></html>
route GET /style ? { n: int } = <html><body><p style={mmap(n)}>"x"</p></body></html>
route GET /sheet ? { n: int } = <html><head><style>{many(n)}</style></head><body>"x"</body></html>
route GET /textarea ? { n: int } = <html><body><textarea value={many(n)}></textarea></body></html>
route GET /cls ? { n: int } = <html><body><p class={many(n)}>"x"</p></body></html>
route GET /spread ? { n: int } = <html><body><p {...mmap(n)}>"x"</p></body></html>
route GET /tpl ? { n: int } = <html><body><p>{`${many(n)}`}</p></body></html>
route GET /sel ? { n: int } = <html><body><select value={many(n)}><option>"a"</option></select></body></html>
route GET /data ? { n: int } = <html><body><p data-x={many(n)}>"x"</p></body></html>
route GET /later ? { n: int } = <html><body><p>"before"</p><Later n={n}/><p>"after"</p></body></html>
"#;

const WHOLE_PATHS: &[&str] = &["/p", "/href", "/script", "/island", "/print", "/json", "/hrefv", "/title", "/style", "/sheet", "/textarea", "/cls", "/spread", "/tpl", "/sel", "/data"];

/// A server fn's call from the program's own page, `live` as a live part asks.
fn rpc(port: u16, name: &str, args: &str, live: bool) -> support::Answer {
    let (host, origin) = (format!("localhost:{}", port), format!("http://localhost:{}", port));
    let mut h = vec![("Host", host.as_str()), ("Content-Type", "application/json"), ("Sec-Fetch-Site", "same-origin"), ("Origin", origin.as_str())];
    if live {
        h.push(("x-quillo-live", "1"));
    }
    send(port, "POST", &format!("/_quillo/rpc/{}", name), &h, args)
}

#[test]
fn what_a_page_or_an_answer_writes_is_held_as_it_is_written() {
    // Under 8 MB, 200 holds of a 1 MB text in each place: a 500, said in the log, and the server's
    // memory near what it started with: each was made whole first, 200 MB to 1.2 GB (found in
    // review). Its peak is read through all of them, every 20 ms.
    let d = dir("mem-whole", &[("whole.qwe", WHOLE)]);
    let (g, port) = serve(&d, "whole.qwe", &["--max-memory", "8MB"], "whole.log");
    let pid = g.0.id();
    let before = rss_kb(pid);
    let peak = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(before));
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watch = {
        let (peak, done) = (peak.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(std::sync::atomic::Ordering::SeqCst) {
                peak.fetch_max(rss_kb(pid), std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(20));
            }
        })
    };
    for path in WHOLE_PATHS {
        assert_eq!(get(port, &format!("{}?n=200", path)).0, 500, "{}", path);
    }
    for live in [false, true] {
        assert_eq!(rpc(port, "big", "[200]", live).status, 500, "live {}", live);
    }
    // A deferred part past the limit keeps its placeholder, as a part that fails does: the page
    // is a 200, and the part's stop is said in the log.
    let (status, body) = get(port, "/later?n=200");
    assert!(status == 200 && body.contains("before") && body.contains("after"), "{}", status);
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    watch.join().unwrap();
    let grew = peak.load(std::sync::atomic::Ordering::SeqCst).saturating_sub(before);
    assert!(grew < 120_000, "took {} KB more", grew);
    let log = std::fs::read_to_string(d.join("whole.log")).unwrap();
    let stop = "this request's value would be bigger than --max-memory allows (8388608 bytes)";
    assert!(log.matches(stop).count() >= WHOLE_PATHS.len() + 3, "{}", log);
    assert!(log.contains("error while rendering the deferred part of <Later>"), "{}", log);
    // Under it, each is made as before.
    for path in WHOLE_PATHS {
        assert_eq!(get(port, &format!("{}?n=2", path)).0, 200, "{}", path);
    }
    for live in [false, true] {
        let a = rpc(port, "big", "[2]", live);
        assert!(a.status == 200 && a.body.len() > 2_000_000, "live {}: {}", live, a.status);
    }
}

/// A component rendered again (FEEDBACK55.md, Q-770) because a child set its state as it started:
/// `Grows` lists `count` ints, which its child sets to `n`; `Overflows` multiplies it past an int.
const AGAIN: &str = r#"fn ints(n: int) -> int {
    let xs = []
    for i in range(0, n) {
        xs = xs | push(i)
    }
    return len(xs)
}
fn big(n: int) -> int {
    return n * 4611686018427387904
}
component Child(on_ready, n: int) {
    on_ready(n)
    <span>"child"</span>
}
component Grows(n: int) {
    state count = 0
    <div><p #g1>{ints(count)}</p><Child on_ready={(v) => { count = v }} n={n}/><p #g2>{count}</p></div>
}
component Overflows() {
    state count = 0
    <div><p #o1>{big(count)}</p><Child on_ready={(v) => { count = v }} n={2}/><p #o2>{count}</p></div>
}
route GET /grows ? { n: int } = <html><body><Grows n={n}/></body></html>
route GET /overflows = <html><body><Overflows/></body></html>
"#;

#[test]
fn a_second_render_whose_code_passes_the_limit_ends_its_request() {
    // The second render's `push` loop, made longer by the state the child set, passes the limit:
    // the request's stop, at the push's line, where the first render, showing 0, went out with a
    // 200 (FEEDBACK71.md, Q-977, found in review; branch security).
    let d = dir("again", &[("again.qwe", AGAIN)]);
    let (_g, port) = serve(&d, "again.qwe", &["--max-memory", "1MB"], "again.log");
    let (status, body) = get(port, "/grows?n=100000");
    assert_eq!(status, 500, "{}", body);
    assert!(!body.contains(r#"<p id="g1">0</p>"#), "{}", body);
    let log = std::fs::read_to_string(d.join("again.log")).unwrap();
    let stop = "again.qwe:4: this request's value would be bigger than --max-memory allows (1048576 bytes)";
    assert_eq!(log.matches(stop).count(), 1, "{}", log);
    assert!(log.contains("GET /grows?n=100000 -> 500"), "{}", log);
    // Under it, the second render is kept, as before: one value above the child and below.
    let (status, body) = get(port, "/grows?n=10");
    assert_eq!(status, 200, "{}", body);
    assert!(body.contains(r#"<p id="g1">10</p>"#) && body.contains(r#"<p id="g2">10</p>"#), "{}", body);
    // Any other error in the second render keeps the first, as before.
    let (status, body) = get(port, "/overflows");
    assert_eq!(status, 200, "{}", body);
    assert!(body.contains(r#"<p id="o1">0</p>"#) && body.contains(r#"<p id="o2">2</p>"#), "{}", body);
    // And the server answers after them.
    assert_eq!(get(port, "/grows?n=3").0, 200);
}

#[test]
fn without_max_memory_a_second_render_makes_what_it_did() {
    let d = dir("again-unset", &[("again.qwe", AGAIN)]);
    let (_g, port) = serve(&d, "again.qwe", &[], "again.log");
    let (status, body) = get(port, "/grows?n=100000");
    assert_eq!(status, 200, "{}", body);
    assert!(body.contains(r#"<p id="g1">100000</p>"#) && body.contains(r#"<p id="g2">100000</p>"#), "{}", body);
    let (status, body) = get(port, "/overflows");
    assert_eq!(status, 200, "{}", body);
    assert!(body.contains(r#"<p id="o1">0</p>"#), "{}", body);
}
