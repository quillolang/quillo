//! `--max-memory`'s lines of 0.8.7 that no test ran (FEEDBACK71.md, Q-977, and Q-975's pattern
//! limits): each place a value is written held to the limit, and where it would pass it, the
//! request it ends, the answer and the log line the user sees; and under the limit, what is made.
//! Every server and script is one this test starts, on this machine, on the port the system gives.

mod support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::send;

/// The limit most of these run under: `--max-memory 1MB`.
const L: usize = 1 << 20;
const STOP: &str = "this request's value would be bigger than --max-memory allows (1048576 bytes)";

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("cover-memory-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

/// Removes a test's folder when the test ends, passed or not.
struct Gone(PathBuf);

impl Drop for Gone {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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
        .env("QUILLO_SECRET", "a secret for the cover_memory tests, long enough")
        .stdin(Stdio::null())
        .stdout(out.try_clone().unwrap())
        .stderr(out)
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    // Whole lines only: a line can be written in pieces (review of coverage-087).
    let port = support::port_in_log(&mut g.0, &d.join(log), Duration::from_secs(20));
    (g, port)
}

fn get(port: u16, path: &str) -> (u16, String) {
    let a = send(port, "GET", path, &[], "");
    (a.status, a.body)
}

/// A call from the program's own page: a server fn's (`live` as a live part asks).
fn rpc(port: u16, name: &str, args: &str, live: bool) -> support::Answer {
    own_page(port, &format!("/_quillo/rpc/{}", name), args, live)
}

/// A POST of JSON to `path` from the program's own page.
fn own_page(port: u16, path: &str, body: &str, live: bool) -> support::Answer {
    let (host, origin) = (format!("localhost:{}", port), format!("http://localhost:{}", port));
    let mut h = vec![("Host", host.as_str()), ("Content-Type", "application/json"), ("Sec-Fetch-Site", "same-origin"), ("Origin", origin.as_str())];
    if live {
        h.push(("x-quillo-live", "1"));
    }
    send(port, "POST", path, &h, body)
}

/// The log once it holds `want` `n` times, 10 s at most: a request's line is written after its
/// answer.
fn log_holding(path: &Path, want: &str, n: usize) -> String {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let log = std::fs::read_to_string(path).unwrap_or_default();
        if log.matches(want).count() >= n || Instant::now() >= end {
            return log;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The line `at` starts in `src`, as the log names it.
fn line_of(src: &str, at: &str) -> usize {
    src.lines().position(|l| l.starts_with(at)).unwrap_or_else(|| panic!("no line starts with {}", at)) + 1
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

/// Each (name, program, what it prints): under 1 MB the stop, said at `stop_line`, and with a
/// limit that holds it (16 MB) and none, what it prints. Each program is checked first.
fn refused_then_given(d: &Path, cases: &[(&str, &str, usize, &str)]) {
    for (name, src, stop_line, want) in cases {
        let file = format!("{}.qwe", name);
        let (code, out, err) = script(d, &file, src, &["--max-memory", "1MB"]);
        checked(d, &file);
        assert_ne!(code, Some(0), "{} under 1MB: {}{}", name, out, err);
        assert!(err.contains(&format!("quillo: {}:{}: {}", file, stop_line, STOP)), "{} under 1MB: {}", name, err);
        for args in [&["--max-memory", "16MB"][..], &[]] {
            let (code, out, err) = script(d, &file, src, args);
            assert_eq!((code, out.trim()), (Some(0), *want), "{} {:?}: {}", name, args, err);
        }
    }
}

/// `quillo check` of each program, as the user's editor would, given 60 seconds: none has an error.
fn checked(d: &Path, file: &str) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["check", file]).current_dir(d).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    support::stop_with_tests(&child);
    let mut err = child.stderr.take().unwrap();
    let said = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("quillo check {} did not end in 60 s", file);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "{}: {}", file, said.join().unwrap());
}

// ---- JSON written held: the piece that would pass the limit -------------------------------

/// `json.stringify` held as it is written stops at the very piece that would pass the limit: the
/// list's closing `]`, a decimal written as a text, a map's key; with one byte less each is made,
/// the limit itself long (src/value.rs `json_into_as`'s `room`, `scalar_held`, `go`; FEEDBACK71.md,
/// Q-977; coverage of 0.8.7; branch coverage-087).
#[test]
fn json_text_stops_at_the_piece_that_would_pass_the_limit() {
    let d = dir("json", &[]);
    let _gone = Gone(d.clone());
    // `[` + `"a…"` + `]`: L-4 letters make L bytes; L-3 leave no room for the `]`.
    // `[` + `"a…"` + `,` + `"1.5"` + `]`: L-10 letters make L bytes; at L-8 the decimal's text has
    // no room. `{` + `"k…"` + `:"v"}`: L-8 make L bytes; L-1 leave no room for the key itself.
    // (A number is given room for 64 bytes, whatever its digits: `{"k…":1}` of L bytes is refused.)
    let cases = [
        ("bracket", L - 4, L - 3, "[\"a\".repeat(N)]"),
        ("decimal", L - 10, L - 8, "[\"a\".repeat(N), 1.5m]"),
        ("key", L - 8, L - 1, "set({}, \"k\".repeat(N), \"v\")"),
    ];
    for (name, fits, past, value) in cases {
        let src = |n: usize| format!("print(len(json.stringify({})))\n", value.replace('N', &n.to_string()));
        let (code, out, err) = script(&d, &format!("{}.qwe", name), &src(fits), &["--max-memory", "1MB"]);
        assert_eq!((code, out.trim()), (Some(0), L.to_string().as_str()), "{}: {}", name, err);
        checked(&d, &format!("{}.qwe", name));
        let (code, out, err) = script(&d, &format!("{}.qwe", name), &src(past), &["--max-memory", "1MB"]);
        assert_ne!(code, Some(0), "{}: {}", name, out);
        assert!(err.contains(&format!("quillo: {}.qwe:1: {}", name, STOP)), "{}: {}", name, err);
        // Unset, the longer one is made.
        let (code, out, err) = script(&d, &format!("{}.qwe", name), &src(past), &[]);
        assert_eq!((code, out.trim().parse::<usize>().ok()), (Some(0), Some(L + past - fits)), "{}: {}", name, err);
    }
}

/// A map's text (`str`) stops at the entry that passes the limit; a list's `\b` and `\f` are
/// written as JSON writes them (src/value.rs `repr_at`, `quoted_into`; FEEDBACK71.md, Q-977;
/// coverage of 0.8.7; branch coverage-087).
#[test]
fn a_maps_text_stops_at_the_entry_that_passes_the_limit() {
    let d = dir("repr", &[]);
    let _gone = Gone(d.clone());
    let map = "let s = \"x\".repeat(400000)\nlet m = { a: s, b: s, c: s }\nprint(len(str(m)))\n";
    // `{ a: "…", b: "…", c: "…" }`: three texts of 400,002 with their quotes, and 17 more.
    refused_then_given(&d, &[("map", map, 3, "1200023")]);
    // Two of them fit.
    let (code, out, err) = script(&d, "two.qwe", "let s = \"x\".repeat(400000)\nprint(len(str({ a: s, b: s })))\n", &["--max-memory", "1MB"]);
    assert_eq!((code, out.trim()), (Some(0), "800016"), "{}", err);
    checked(&d, "two.qwe");
    let (code, out, err) = script(&d, "esc.qwe", "let s = \"a\\u{8}b\\u{c}c\\u{1}\"\nprint(str([s]))\nprint(json.stringify([s]))\n", &["--max-memory", "1MB"]);
    assert_eq!((code, out.as_str()), (Some(0), "[\"a\\bb\\fc\\u0001\"]\n[\"a\\bb\\fc\\u0001\"]\n"), "{}", err);
    checked(&d, "esc.qwe");
}

// ---- `+`, `+=` and `split` of large values ------------------------------------------------

/// `+` of a value and a text where the limit is set writes the value's text through a held text
/// (an error's list too); two pieces of markup and two lists past 64 KB are compared with the limit
/// first; `xs += ys` asks before it grows the list where it is (src/interp.rs `text_plus`,
/// `large_plus`, `grown`; FEEDBACK71.md, Q-977; coverage of 0.8.7; branch coverage-087).
#[test]
fn texts_markup_and_lists_added_past_the_limit_end_the_script() {
    let d = dir("plus", &[]);
    let _gone = Gone(d.clone());
    let errplus = "let e = err(range(0, 30).map((i) => \"x\".repeat(100000)))\nprint(len(e + \"a\"))\n";
    let html = "let a = raw(\"a\".repeat(600000))\nprint(len(str(a + raw(\"b\".repeat(600000)))))\n";
    let extend = "fn f(n: int) -> int {\n    let xs = range(0, n)\n    xs += range(0, n)\n    return len(xs)\n}\nprint(f(20000))\n";
    refused_then_given(&d, &[("errplus", errplus, 2, "3000126"), ("html", html, 2, "1200000"), ("extend", extend, 3, "40000")]);
    // Under the limit: a number before a text, markup of 70,001 bytes, a fresh list grown where
    // it is, and a list grown by `+=` to 20,000 places (640,000 bytes).
    let small = "let n = 5\nprint(n + \" bytes\")\nprint(len(str(raw(\"a\".repeat(70000)) + raw(\"b\"))))\nprint(len(range(0, 3000) + [1]))\nfn f(n: int) -> int {\n    let xs = range(0, n)\n    xs += range(0, n)\n    return len(xs)\n}\nprint(f(10000))\n";
    let (code, out, err) = script(&d, "small.qwe", small, &["--max-memory", "1MB"]);
    assert_eq!((code, out.as_str()), (Some(0), "5 bytes\n70001\n3001\n20000\n"), "{}", err);
    checked(&d, "small.qwe");
}

/// `split` counts the list it would make before it makes it: 32,768 parts of 32 bytes are the
/// limit, one more is past it, and a text's parts by "" are its characters, not its bytes
/// (src/builtins.rs `split`; FEEDBACK71.md, Q-977; coverage of 0.8.7; branch coverage-087).
#[test]
fn split_counts_its_parts_before_it_makes_them() {
    let d = dir("split", &[]);
    let _gone = Gone(d.clone());
    for (name, src, want) in [
        ("chars", "print(len(split(\"x\".repeat(32768), \"\")))\n", Some("32768")),
        ("chars1", "print(len(split(\"x\".repeat(32769), \"\")))\n", None),
        ("sep", "print(len(split(\"x,\".repeat(32767), \",\")))\n", Some("32768")),
        ("sep1", "print(len(split(\"x,\".repeat(32768), \",\")))\n", None),
        // 30,000 characters of two bytes: 960,000 bytes of parts, where its 60,000 bytes would be
        // 1.9 MB.
        ("unicode", "print(len(split(\"é\".repeat(30000), \"\")))\n", Some("30000")),
    ] {
        let (code, out, err) = script(&d, &format!("{}.qwe", name), src, &["--max-memory", "1MB"]);
        checked(&d, &format!("{}.qwe", name));
        match want {
            Some(w) => assert_eq!((code, out.trim()), (Some(0), w), "{}: {}", name, err),
            None => {
                assert_ne!(code, Some(0), "{}: {}", name, out);
                assert!(err.contains(&format!("quillo: {}.qwe:1: {}", name, STOP)), "{}: {}", name, err);
                let (code, out, _) = script(&d, &format!("{}.qwe", name), src, &[]);
                assert_eq!((code, out.trim()), (Some(0), "32769"), "{} unset", name);
            }
        }
    }
}

/// `m = set(m, k, v)` with a key that is no text, made where the map is, asks before the map grows
/// past the limit: 7,168 entries fill its table, and the next would double it past 1 MB; one less,
/// and it is made (src/value.rs `set_by_text`; FEEDBACK71.md, Q-977; coverage of 0.8.7; branch
/// coverage-087).
#[test]
fn a_map_set_where_it_is_by_a_key_that_is_no_text_is_held() {
    let d = dir("setkey", &[]);
    let _gone = Gone(d.clone());
    let src = "fn object(n: int) -> str {\n    return \"{\" + join(range(0, n).map((i) => \"\\\"k\" + str(i) + \"\\\":1\"), \",\") + \"}\"\n}\nfn grown(n: int) -> int {\n    let m = json.parse(object(n))\n    m = set(m, 5, 1)\n    return len(keys(m))\n}\nprint(grown(7167))\nprint(grown(7168))\n";
    let (code, out, err) = script(&d, "set.qwe", src, &["--max-memory", "1MB"]);
    assert_eq!(out, "7168\n", "{}", err);
    assert_ne!(code, Some(0));
    assert!(err.contains(&format!("quillo: set.qwe:6: {}", STOP)), "{}", err);
    let (code, out, err) = script(&d, "set.qwe", src, &[]);
    assert_eq!((code, out.as_str()), (Some(0), "7168\n7169\n"), "{}", err);
    checked(&d, "set.qwe");
}

// ---- a page, a script's value and an island past the limit -------------------------------

const PAGES: &str = r#"// What a page under --max-memory 1MB writes where a value passes it (FEEDBACK71.md, Q-977).
let one = "q".repeat(100000)
fn many(n: int) -> list {
    let xs = []
    for i in range(0, n) {
        xs = xs | push(one)
    }
    return xs
}
route GET /ok = "ok"
route GET /shown ? { n: int } = <html><body><p>{many(n)}</p></body></html>
route GET /style ? { n: int } {
    let s = "x".repeat(n)
    return <html><body><p style={{ color: s, background: s, border: s }}>x</p></body></html>
}
route GET /words ? { n: int } = <html><body><p class={many(n)}>x</p></body></html>
route GET /class ? { n: int } = <html><body><p class={join(many(n), " ")}>x</p></body></html>
route GET /attr ? { n: int } = <html><body><p class="a" data-x={join(many(n), "")}>x</p></body></html>
route GET /json ? { n: int } = <html><body><p data-x={[err(many(n))]}>x</p></body></html>
route GET /key ? { k: int } = <html><body><p data-x={set({}, "k".repeat(k), 1)}>x</p></body></html>
route GET /dec = <html><body><p data-x={[1.5m]}>x</p></body></html>
route GET /script ? { n: int } = <html><body><script>let a = {[err(many(n))]};</script></body></html>
route GET /scriptkey ? { k: int } = <html><body><script>let a = {set({}, "k".repeat(k), 1)};</script></body></html>
route GET /answer = <html><body><script>let a = {[redirect("/x")]};</script></body></html>
route GET /function = <html><body><script>let a = {[len]};</script></body></html>
route GET /textarea = <html><body><textarea value={null}></textarea></body></html>
route GET /img ? { pic: str } = <html><body><img {...{ name: raw(pic), id: "cookie" }}/></body></html>
component Show(t: str) {
    <i>{len(t)}</i>
}
route GET /island ? { n: int } = <html><body><Show t={"<".repeat(n)}/></body></html>
"#;

/// Each (path, status, what the page holds) under 1 MB: past it, the page ends with a 500 and the
/// log says the stop at the route's line; under it, the page as written (src/interp.rs
/// `render_value`, `style_text`, `class_words`, `render_attrs`, `island_wrap`; src/value.rs
/// `json_into_at`, `tree_json_into`; FEEDBACK71.md, Q-977; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_page_ends_where_a_value_written_in_it_passes_the_limit() {
    let d = dir("pages", &[("pages.qwe", PAGES)]);
    let _gone = Gone(d.clone());
    checked(&d, "pages.qwe");
    let (_g, port) = serve(&d, "pages.qwe", &["--max-memory", "1MB"], "pages.log");
    let past = [
        // A list shown in a page; a style of three long properties; a class's words, and a long
        // class; a long attribute where a class is written; an error in an attribute's JSON; a
        // map's key there and in a script's JSON; an error in a script. (The first four were error
        // values, which markup now leaves out: FEEDBACK81.md, Q-1272, decided; branch
        // feedback72-81.)
        "/shown?n=20",
        "/style?n=400000",
        "/words?n=20",
        "/class?n=20",
        "/attr?n=20",
        "/json?n=20",
        &format!("/key?k={}", L - 1),
        "/script?n=20",
        &format!("/scriptkey?k={}", L - 1),
        // An island's data: 300,000 `<` are 300 KB of props, each `&lt;` in its attribute.
        "/island?n=300000",
    ];
    for path in &past {
        assert_eq!(get(port, path), (500, "internal error\n".into()), "{}", path);
    }
    let log = log_holding(&d.join("pages.log"), STOP, past.len());
    assert_eq!(log.matches(STOP).count(), past.len(), "{}", log);
    // At the line of the markup that made it: a block's `return`.
    for (path, at) in [("/shown?n=20", "route GET /shown"), ("/style?n=400000", "    return <html><body><p style="), ("/island?n=300000", "route GET /island"), ("/scriptkey", "route GET /scriptkey")] {
        let said = format!("pages.qwe:{}: {}", line_of(PAGES, at), STOP);
        assert!(log.lines().any(|l| l.contains(&format!("GET {}", path)) && l.ends_with(&said)), "{}: {}", path, log);
    }
    // Under it, each as written.
    let page = |path: &str| {
        let (status, body) = get(port, path);
        assert_eq!(status, 200, "{}: {}", path, body);
        body
    };
    let shown = page("/shown?n=1");
    assert!(shown.contains(&format!("<p>{}</p>", "q".repeat(100000))), "{}", &shown[..200]);
    let style = page("/style?n=1000");
    let x = "x".repeat(1000);
    assert!(style.contains(&format!("<p style=\"color: {x}; background: {x}; border: {x}\">x</p>")), "{}", &style[..200]);
    assert!(page("/key?k=10").contains("<p data-x=\"{&quot;kkkkkkkkkk&quot;:1}\">x</p>"));
    assert!(page("/dec").contains("<p data-x=\"[&quot;1.5&quot;]\">x</p>"));
    assert!(page("/answer").contains("<script>let a = [{\"status\":303}];</script>"));
    // A function in a script's JSON is written within the limit; how it is written is not this
    // test's (coverage of 0.8.7, Q-1107; branch coverage-087).
    assert!(page("/function").contains("<script>let a = ["));
    // A textarea's value of null is no text.
    assert!(page("/textarea").contains("<body><textarea></textarea></body>"));
    // A spread's `name` given as markup names the image, so its `id` may not take a property of
    // `document`; an empty one does not.
    assert!(page("/img?pic=x").contains("<body><img name=\"x\"></body>"));
    assert!(page("/img?pic=").contains("<body><img name=\"\" id=\"cookie\"></body>"));
    assert!(page("/island?n=10").contains("data-c=\"Show\" data-p=\"{&quot;t&quot;:&quot;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&quot;}\" data-s=\"{}\"><i>10</i>"));
    // 200,000 are 800 KB in the attribute.
    assert!(page("/island?n=200000").contains("<i>200000</i>"));
    assert_eq!(get(port, "/ok"), (200, "ok".into()));
}

// ---- server fns' answers ----------------------------------------------------------------

const FNS: &str = r#"// Server fns whose answers are held to --max-memory 1MB (FEEDBACK71.md, Q-977).
let one = "q".repeat(100000)
fn many(n: int) -> list {
    let xs = []
    for i in range(0, n) {
        xs = xs | push(one)
    }
    return xs
}
route GET /ok = "ok"
server fn decimal(k: int) -> list = ["a".repeat(k), 1.5m]
server fn dollar(k: int) -> map = set({}, "$" + "k".repeat(k), 1)
server fn dollared(n: int) -> map = set({}, "$k", many(n))
server fn failed(n: int) -> list {
    return err(many(n))
}
server fn load(n: int) -> list = [read_file("no-such-file.txt"), many(n)]
component Feed(n: int) {
    state items = live(load, n)
    <p>{len(items)}</p>
}
route GET /live ? { n: int } = <html><body><Feed n={n}/></body></html>
route GET /deep ? { n: int } {
    let v = []
    for i in range(0, n) {
        v = [v]
    }
    let r = parallel([() => len(json.stringify(v)), () => len(json.stringify([v]))])
    return `${r[0]} ${r[1]}`
}
fn nest(n: int, bottom: list) -> list {
    let v = bottom
    for i in range(0, n) {
        v = [v]
    }
    return v
}
route GET /deeper ? { n: int } {
    let v = nest(n, [])
    let r = parallel([() => error(json.stringify(v)), () => error(json.stringify([v]))])
    return `${r[0]} | ${r[1]}`
}
route GET /deeper_one ? { n: int } = `${error(json.stringify(nest(n, [])))}`
route GET /hidden ? { n: int } {
    let v = nest(n, [read_file("no-such-file.txt")])
    let r = parallel([() => json.stringify(v), () => 0])
    return r[0]
}
"#;

/// A server fn's answer is written held, an island's way: a decimal's text, a map of one `$` key
/// (its key, its value), an error value's list on a live call, and a live part whose value holds
/// a hidden error each end the call with a 500 that says the stop, and under the limit the answer
/// is the limit itself long (src/value.rs `tree_json_into`, src/server.rs `arg_of`,
/// src/builtins.rs `live`; FEEDBACK71.md, Q-977; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_server_fns_answer_ends_at_the_piece_that_would_pass_the_limit() {
    let d = dir("fns", &[("fns.qwe", FNS)]);
    let _gone = Gone(d.clone());
    checked(&d, "fns.qwe");
    let (_g, port) = serve(&d, "fns.qwe", &["--max-memory", "1MB"], "fns.log");
    let refused = format!("{{\"error\":\"{}\"}}", STOP);
    // `{"ok":["a…",{"$dec":"1.5"}]}` is the text and 26 bytes: L-26 letters are the limit.
    let a = rpc(port, "decimal", &format!("[{}]", L - 26), false);
    assert_eq!((a.status, a.body.len()), (200, L));
    assert!(a.body.starts_with("{\"ok\":[\"aaa") && a.body.ends_with("aaa\",{\"$dec\":\"1.5\"}]}"));
    // One more leaves no room for the answer's closing `}`; six more, none for the decimal's text.
    for k in [L - 25, L - 20] {
        let a = rpc(port, "decimal", &format!("[{}]", k), false);
        assert_eq!((a.status, a.body.as_str()), (500, refused.as_str()), "{}", k);
    }
    // `{"ok":{"$":["$k…",1]}}`: the key's text past the limit, and its value.
    let a = rpc(port, "dollar", &format!("[{}]", L - 6), false);
    assert_eq!((a.status, a.body.as_str()), (500, refused.as_str()));
    assert_eq!(rpc(port, "dollar", "[3]", false).body, "{\"ok\":{\"$\":[\"$kkk\",1]}}");
    let a = rpc(port, "dollared", "[20]", false);
    assert_eq!((a.status, a.body.as_str()), (500, refused.as_str()));
    let a = rpc(port, "dollared", "[1]", false);
    assert!(a.status == 200 && a.body.starts_with("{\"ok\":{\"$\":[\"$k\",[\"qqq"), "{}", a.status);
    // An error value's list, asked by a live part and not.
    for live in [true, false] {
        let a = rpc(port, "failed", "[20]", live);
        assert_eq!((a.status, a.body.as_str()), (500, refused.as_str()), "live {}", live);
    }
    let a = rpc(port, "failed", "[1]", true);
    assert!(a.status == 200 && a.body.starts_with("{\"err\":[\"qqq"), "{} {}", a.status, &a.body[..40.min(a.body.len())]);
    // A live part whose value holds a builtin's error: hidden in the page, and held as it is kept.
    let (status, body) = get(port, "/live?n=1");
    assert_eq!(status, 200, "{}", body);
    assert!(body.contains("data-s=\"{&quot;items&quot;:[{&quot;$quillo_err&quot;:&quot;internal error&quot;},[&quot;qqq") && body.contains("<p>2</p>"), "{}", &body[..300]);
    assert_eq!(get(port, "/live?n=20"), (500, "internal error\n".into()));
    let log = log_holding(&d.join("fns.log"), STOP, 7);
    assert_eq!(log.matches(STOP).count(), 7, "{}", log);
    assert!(log.contains(&format!("fns.qwe:{}: {}", line_of(FNS, "    state items = live(load, n)"), STOP)), "{}", log);
    // Lists 150, 1,000 and 5,000 deep written as JSON by a `parallel` call, on a child's stack of
    // 512 KB: deep enough, the JSON goes on to a new piece of stack (src/value.rs `deeper`), and the
    // server answers after each. In a debug build too, whose stack ran out at 150 before the first
    // look at 256 levels and stopped the server (coverage of 0.8.7, Q-1106; branch coverage-087).
    for n in [150usize, 1000, 5000] {
        assert_eq!(get(port, &format!("/deep?n={}", n)), (200, format!("{} {}", 2 * n + 2, 2 * n + 4)), "{} deep", n);
    }
    assert_eq!(get(port, "/ok"), (200, "ok".into()));
}

/// A list 20,000 or 100,000 deep given to `json.stringify` in a `parallel` call's function: the
/// walk for hidden errors before it is written goes on to a new piece of stack as the writer does,
/// and the depth's error value comes back, as outside `parallel`, where the server stopped; one with
/// a builtin's error at the bottom is copied without it the same way, 8,000 deep, where 3,000
/// stopped the server (src/server.rs `holds_hidden_at`, `without_hidden_at`; coverage of 0.8.7,
/// Q-1110; branch coverage-087). Not past that: the copy, made in the function, is dropped there,
/// and dropping a value nests as deep as it does (12,000 stops the server, as in 0.8.7).
#[test]
fn a_very_deep_value_in_parallel_is_refused_and_the_server_answers() {
    let d = dir("deeper", &[("fns.qwe", FNS)]);
    let _gone = Gone(d.clone());
    let (_g, port) = serve(&d, "fns.qwe", &[], "deeper.log");
    let too = "json.stringify: it is nested more than 10000 deep";
    // At the limit (10,000 lists around the innermost, which `json.stringify` writes), and one
    // past it; then far past it.
    assert_eq!(get(port, "/deeper?n=10000"), (200, format!("null | {}", too)));
    assert_eq!(get(port, "/deeper_one?n=10000"), (200, "null".into()));
    // A debug build to 20,000: there a list 100,000 deep stops the server where it is dropped
    // (`Arc`'s drop nests as the list does), outside `parallel` too, as before.
    let far: &[usize] = if cfg!(debug_assertions) { &[10_001, 20_000] } else { &[10_001, 20_000, 100_000] };
    for &n in far {
        assert_eq!(get(port, &format!("/deeper?n={}", n)), (200, format!("{} | {}", too, too)), "{} deep", n);
        assert_eq!(get(port, &format!("/deeper_one?n={}", n)), (200, too.to_string()), "{} deep, no parallel", n);
    }
    let n = if cfg!(debug_assertions) { 1000 } else { 8000 };
    let (status, body) = get(port, &format!("/hidden?n={}", n));
    assert_eq!(status, 200);
    assert!(body == format!("{}[{{\"error\":\"internal error\"}}]{}", "[".repeat(n), "]".repeat(n)), "{}", &body[body.len().saturating_sub(80)..]);
    let log = log_holding(&d.join("deeper.log"), "no-such-file.txt", 1);
    assert!(log.contains("read_file: no-such-file.txt"), "{}", log);
    assert_eq!(get(port, "/ok"), (200, "ok".into()));
}

// ---- markdown's endpoint, and a policy's limit -------------------------------------------

/// A markdown text of twenty reference links to one address `n` bytes long, as JSON for the page's
/// call: the links' expansion is held to about the text's length, so an address of 600,000 bytes
/// makes 1.2 MB of page from 600 KB.
fn links(n: usize) -> String {
    let text = format!("[a]: http://e.com/{}\n\n{}", "x".repeat(n), "[a] ".repeat(20));
    format!("[{}]", serde_json_string(&text))
}

fn serde_json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Markdown made for a page past `--max-memory` is a 500 that says the stop, in the log too; under
/// a policy's `max_memory` the page is told the server's policy, and the log names its file, for
/// markdown and a server fn alike (src/server.rs `handle_internal`, src/httpd.rs
/// `over_memory_said_to_pages`; FEEDBACK71.md, Q-977; coverage of 0.8.7; branch coverage-087).
#[test]
fn markdown_past_the_limit_and_a_policys_limit_are_said_to_the_page() {
    let src = "route GET /ok = \"ok\"\nserver fn big(n: int) -> list = range(0, n).map((i) => \"x\".repeat(100000))\n";
    let d = dir("markdown", &[("md.qwe", src)]);
    let _gone = Gone(d.clone());
    checked(&d, "md.qwe");
    let (g, port) = serve(&d, "md.qwe", &["--max-memory", "1MB"], "md.log");
    let a = own_page(port, "/_quillo/markdown", &links(600_000), false);
    assert_eq!((a.status, a.body.as_str()), (500, format!("{{\"error\":\"{}\"}}", STOP).as_str()));
    let a = own_page(port, "/_quillo/markdown", &links(100_000), false);
    assert_eq!(a.status, 200, "{}", &a.body[..100.min(a.body.len())]);
    assert!(a.body.starts_with("{\"ok\":\"<p><a href=\\\"http://e.com/xxx") && a.body.contains("\\\">a</a> "), "{}", a.body.len());
    let log = log_holding(&d.join("md.log"), STOP, 1);
    assert!(log.lines().any(|l| l.starts_with("quillo: error: POST /_quillo/markdown (rid ") && l.ends_with(STOP)), "{}", log);
    drop(g);
    // The same limit from the policy file.
    std::fs::write(d.join("quillo.policy"), "[limits]\nmax_memory = \"1MB\"\n").unwrap();
    let (_g, port) = serve(&d, "md.qwe", &[], "policy.log");
    let told = "{\"error\":\"this request's value would be bigger than the server's policy allows (max_memory, 1048576 bytes)\"}";
    let a = own_page(port, "/_quillo/markdown", &links(600_000), false);
    assert_eq!((a.status, a.body.as_str()), (500, told));
    let a = rpc(port, "big", "[20]", false);
    assert_eq!((a.status, a.body.as_str()), (500, told));
    assert_eq!(rpc(port, "big", "[2]", false).status, 200);
    let said = "quillo.policy allows (max_memory, 1048576 bytes)";
    let log = log_holding(&d.join("policy.log"), said, 2);
    assert_eq!(log.matches(said).count(), 2, "{}", log);
    assert!(!log.contains("the server's policy"), "the log names the file: {}", log);
    assert_eq!(get(port, "/ok"), (200, "ok".into()));
}

// ---- foreign content: what is looked at in `<svg>` ----------------------------------------

const SVG: &str = r#"// Values, a match, a select's options and a view's children inside <svg> (FEEDBACK71.md, Q-969).
route GET /list = <html><body><svg>{["a", "b"]}</svg></body></html>
route GET /match ? { k: int, t: str } = <html><body><svg>{match k { 1 => t, _ => "other" }}</svg></body></html>
route GET /select ? { a: str } = <html><body><svg><select value={"b"}><option>{a}</option><option>b</option></select></svg></body></html>
view Frame(Box, t: str) = <Box>{upper(t)}</Box>
route GET /view = <html><body><Frame Box={(children) => <svg>{children}</svg>} t="a"/></body></html>
"#;

/// Inside `<svg>`, a list of texts is written as a value, escaped, with nothing more looked at; a
/// `match` that may write a value is read again where the parser is; a select's options are written
/// where the parser is, the one its value names selected; and a view made where the page is (not a
/// global) puts its children in its `<svg>`, called again and again (that where they go is worked
/// out each time and not kept is not seen in the page: for that part this is coverage only)
/// (src/interp.rs
/// `holds_markup`, `writes_value`, `render_option_into`, `children_place`; FEEDBACK71.md, Q-969;
/// coverage of 0.8.7; branch coverage-087).
#[test]
fn values_options_and_a_views_children_inside_svg() {
    let d = dir("svg", &[("svg.qwe", SVG)]);
    let _gone = Gone(d.clone());
    checked(&d, "svg.qwe");
    let (_g, port) = serve(&d, "svg.qwe", &[], "svg.log");
    for (path, want) in [
        ("/list", "<body><svg>ab</svg></body>"),
        // A branch that may write a value is read again where the parser is: escaped, as ever.
        ("/match?k=1&t=%3Cb%3E", "<body><svg>&lt;b&gt;</svg></body>"),
        ("/match?k=2&t=x", "<body><svg>other</svg></body>"),
        ("/select?a=a", "<body><svg><select><option>a</option><option selected>b</option></select></svg></body>"),
        ("/select?a=b", "<body><svg><select><option selected>b</option><option>b</option></select></svg></body>"),
        ("/view", "<body><svg>A</svg></body>"),
    ] {
        let (status, body) = get(port, path);
        assert!(status == 200 && body.contains(want), "{}: {} {}", path, status, body);
    }
    // Asked again, the same page (coverage of `children_place` for a view that is no global's).
    for _ in 0..2 {
        assert!(get(port, "/view").1.contains("<body><svg>A</svg></body>"));
    }
}

// ---- databases' rows past the limit -----------------------------------------------------

/// SQLite's blobs are compared with the limit before their text is made, at what U+FFFD takes for
/// each byte that is not UTF-8: 30,000 bytes of 0xFF are 90,000 of text (src/db.rs `cell_room`;
/// FEEDBACK71.md, Q-977; coverage of 0.8.7; branch coverage-087).
#[test]
fn sqlite_blobs_are_held_by_the_text_they_make() {
    let d = dir("blobs", &[]);
    let _gone = Gone(d.clone());
    let both = "db.open(\"q.db\")\nlet a = db.query(\"select zeroblob(30000) as b, unhex(printf('%.*c', 60000, 'f')) as c\")\nprint(`${len(a[0].b)} ${len(a[0].c)} ${a[0].c[0] == \"\\u{fffd}\"}`)\n";
    let (code, out, err) = script(&d, "both.qwe", both, &["--max-memory", "1MB"]);
    assert_eq!((code, out.trim()), (Some(0), "30000 30000 true"), "{}", err);
    checked(&d, "both.qwe");
    // Under 64 KB: 70,000 zeros, and 30,000 bytes of 0xFF (90,000 of text) are past it; 21,000
    // of 0xFF (63,000) are not.
    for (name, sql, want) in [
        ("zeros", "select zeroblob(70000) as b", None),
        ("bad", "select unhex(printf('%.*c', 60000, 'f')) as b", None),
        ("fits", "select unhex(printf('%.*c', 42000, 'f')) as b", Some("21000")),
    ] {
        let src = format!("db.open(\"q.db\")\nlet a = db.query(\"{}\")\nprint(len(a[0].b))\n", sql);
        let (code, out, err) = script(&d, &format!("{}.qwe", name), &src, &["--max-memory", "64KB"]);
        checked(&d, &format!("{}.qwe", name));
        match want {
            Some(w) => assert_eq!((code, out.trim()), (Some(0), w), "{}: {}", name, err),
            None => {
                assert_ne!(code, Some(0), "{}: {}", name, out);
                assert!(err.contains(&format!("quillo: {}.qwe:2: this request's value would be bigger than --max-memory allows (65536 bytes)", name)), "{}: {}", name, err);
            }
        }
    }
}

/// MariaDB's rows are held as they are read: a text of 2 MB and 40,000 rows each end the script
/// under 1 MB, and under 16 MB both are made; on a server of one connection, the rest of a refused
/// result is read and dropped, and the next request's query on it answers (src/my.rs `query`;
/// FEEDBACK71.md, Q-977; coverage of 0.8.7; branch coverage-087).
#[test]
fn mariadb_rows_past_the_limit_end_the_request() {
    let Some(m) = support::mariadb("cover-memory") else { return };
    let url = m.url("cover_memory");
    let d = dir("maria", &[]);
    let _gone = Gone(d.clone());
    let text_sql = "select repeat('x', 2000000) as s";
    let rows_sql = "with recursive c(i) as (select 1 union all select i + 1 from c where i < 200) select x.i from c x, c y";
    let text = format!("db.open(\"{}\")\nlet a = db.query(\"{}\")\nprint(len(a[0].s))\n", url, text_sql);
    let rows = format!("db.open(\"{}\")\nlet b = db.query(\"{}\")\nprint(len(b))\n", url, rows_sql);
    refused_then_given(&d, &[("text", &text, 2, "2000000"), ("rows", &rows, 2, "40000")]);
    let src = format!(
        "db.open(\"{}?pool_min=1&pool_max=1\")\nroute GET /text = `${{len(db.query(\"{}\")[0].s)}}`\nroute GET /rows = `${{len(db.query(\"{}\"))}}`\nroute GET /small = `${{len(db.query(\"select repeat('x', 500000) as s\")[0].s)}}`\n",
        url, text_sql, rows_sql
    );
    std::fs::write(d.join("maria.qwe"), &src).unwrap();
    checked(&d, "maria.qwe");
    let (_g, port) = serve(&d, "maria.qwe", &["--max-memory", "1MB"], "maria.log");
    for path in ["/text", "/rows"] {
        assert_eq!(get(port, path), (500, "internal error\n".into()), "{}", path);
        assert_eq!(get(port, "/small"), (200, "500000".into()), "after {}", path);
    }
    let log = log_holding(&d.join("maria.log"), STOP, 2);
    assert!(log.contains(&format!("maria.qwe:2: {}", STOP)) && log.contains(&format!("maria.qwe:3: {}", STOP)), "{}", log);
}

// ---- answers read from elsewhere ------------------------------------------------------------

/// A server on this machine that answers each of `n` requests with `head` and `body`, and gives
/// back what it was sent.
fn answering(n: usize, head: &'static str, body: Vec<u8>) -> (u16, std::thread::JoinHandle<Vec<Vec<u8>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let t = std::thread::spawn(move || {
        let mut got = Vec::new();
        for c in l.incoming().take(n) {
            let Ok(mut c) = c else { continue };
            got.push(read_request(&mut c));
            let _ = c.write_all(format!("{}content-length: {}\r\nconnection: close\r\n\r\n", head, body.len()).as_bytes());
            let _ = c.write_all(&body);
        }
        got
    });
    (port, t)
}

/// A request's JSON body holding a function, at any depth, is an error value that says where the
/// function is, as `json.stringify`'s is, and nothing is sent, where it went out as its text
/// (`["<builtin len>",1]`); a body without one is sent as before, and its JSON is held to
/// `--max-memory`: the limit itself goes out, one byte more is the request's stop (src/fetch.rs
/// `json_body`; coverage of 0.8.7, Q-1107; branch coverage-087).
#[test]
fn a_function_in_a_requests_json_body_is_an_error_value_and_nothing_is_sent() {
    let d = dir("post", &[]);
    let _gone = Gone(d.clone());
    let (port, t) = answering(2, "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\n", b"done".to_vec());
    let src = format!(
        "let u = \"http://127.0.0.1:{port}/f\"\nlet o = {{ public_only: false }}\nprint(http.post(u, [len, 1], {{}}, o))\nprint(http.post(u, {{ a: [1, {{ f: (x) => x }}] }}, {{}}, o))\nprint(http.post(u, len, {{}}, o))\nprint(http.stream(u, (p) => p, {{ body: [print], public_only: false }}))\nprint(http.post(u, [1, \"a\"], {{}}, o).body)\nprint(http.post(u, [\"x\".repeat({fits})], {{}}, o).body)\nprint(http.post(u, [\"x\".repeat({past})], {{}}, o).body)\n",
        fits = L - 4,
        past = L - 3
    );
    let (code, out, err) = script(&d, "post.qwe", &src, &["--max-memory", "1MB"]);
    let said = |path: &str| format!("err(\"http: the body: `{}` is a function, which cannot be written as JSON\")", path);
    let want = [said("[0]"), said("a[1].f"), "err(\"http: the body: a function cannot be written as JSON\")".to_string(), said("[0]"), "done".into(), "done".into()];
    assert_eq!(out.lines().collect::<Vec<_>>(), want.iter().map(String::as_str).collect::<Vec<_>>(), "{}", err);
    // `["x…"]` of 1,048,577 bytes: the request's stop, at its line.
    assert_ne!(code, Some(0));
    assert!(err.contains(&format!("quillo: post.qwe:9: {}", STOP)), "{}", err);
    checked(&d, "post.qwe");
    // The two bodies without a function, and nothing else, reached the server.
    let got = t.join().unwrap();
    let bodies: Vec<String> = got.iter().map(|r| String::from_utf8_lossy(r).split_once("\r\n\r\n").map_or(String::new(), |(_, b)| b.to_string())).collect();
    assert!(got.iter().all(|r| r.starts_with(b"POST /f ") && String::from_utf8_lossy(r).to_ascii_lowercase().contains("content-type: application/json")));
    assert_eq!(bodies[0], "[1,\"a\"]");
    assert_eq!(bodies[1].len(), L);
    assert!(bodies[1].starts_with("[\"xxx") && bodies[1].ends_with("xxx\"]"));
}

/// A request read to the end of its body (10 s at most): answered and closed before its body was
/// read, the connection is reset, and the client may lose the answer (seen in a debug build).
fn read_request(c: &mut std::net::TcpStream) -> Vec<u8> {
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut req = Vec::new();
    let mut buf = [0u8; 65536];
    loop {
        let Ok(k) = c.read(&mut buf) else { break };
        if k == 0 {
            break;
        }
        req.extend_from_slice(&buf[..k]);
        let text = String::from_utf8_lossy(&req).to_string();
        if let Some((h, b)) = text.split_once("\r\n\r\n") {
            let len = h.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
            if b.len() >= len {
                break;
            }
        }
    }
    req
}

/// One answer of a site's `big`: `{` and `head`, then `unit` `n` times, then `tail` and `}`.
struct Answer {
    head: &'static str,
    unit: Vec<u8>,
    n: usize,
    tail: &'static str,
}

/// A text of `n` bytes `b`.
fn text_of(b: u8, n: usize) -> Answer {
    Answer { head: "\"ok\":\"", unit: vec![b], n, tail: "\"" }
}

/// A list of `n` zeros (`n` > 0).
fn zeros(n: usize) -> Answer {
    Answer { head: "\"ok\":[", unit: b"0,".to_vec(), n: n - 1, tail: "0]" }
}

/// A site on this machine whose `big` answers each request in turn with the next of `answers`,
/// written in pieces; the first line of each request it was sent is given back.
fn site_answering(answers: Vec<Answer>) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let t = std::thread::spawn(move || {
        let mut got = Vec::new();
        for a in answers {
            let Ok((mut c, _)) = l.accept() else { break };
            let req = read_request(&mut c);
            got.push(String::from_utf8_lossy(&req).lines().next().unwrap_or("").to_string());
            let len = 1 + a.head.len() + a.unit.len() * a.n + a.tail.len() + 1;
            let head = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{{{}", len, a.head);
            let _ = c.write_all(head.as_bytes());
            let per = (1 << 16) / a.unit.len().max(1);
            let piece = a.unit.repeat(per);
            let mut left = a.n;
            while left > 0 {
                let k = left.min(per);
                if c.write_all(&piece[..k * a.unit.len()]).is_err() {
                    break;
                }
                left -= k;
            }
            let _ = c.write_all(format!("{}}}", a.tail).as_bytes());
        }
        got
    });
    (port, t)
}

/// A server whose `/big` calls the site's `big`, `??` beside it, and says what it gave.
fn site_server(port: u16) -> String {
    format!("site shop = \"http://127.0.0.1:{}\"\nroute GET /big {{\n    let r = shop.big() ?? \"taken\"\n    return type_of(r) == \"list\" ? `${{len(r)}} items` : `${{len(r)}} characters`\n}}\n", port)
}

/// What `print` shows of each call of `site_answering`'s `big`: its length, or its error.
fn site_calls(n: usize, port: u16) -> String {
    format!("site shop = \"http://127.0.0.1:{}\"\nfor i in range(0, {}) {{\n    let r = shop.big()\n    print(is_err(r) ? str(r) : `${{len(r)}} characters`)\n}}\n", port, n)
}

/// A site's answer is read no further than one value may be under `--max-memory`, and its text is
/// held to it: 600 KB not UTF-8 (1.8 MB of text), where it was made whole, ends the request as
/// `http`'s answer would, a 500 and the stop in the log, which `??` does not take; at the limit
/// and one byte past it, by the text's bytes (each bad byte three) and by the answer's own
/// (src/fetch.rs `site_post`, src/sites.rs `call`; coverage of 0.8.7, Q-1105; branch coverage-087).
#[test]
fn a_sites_answer_past_the_limit_ends_the_request() {
    // `{"ok":"` and `"}` are 9 bytes: 349,522 bad bytes make 1,048,575 bytes of text, one more
    // 1,048,578; L-9 letters are an answer of L bytes, one more L+1.
    let answers = vec![text_of(0xff, 600_000), text_of(0xff, 349_522), text_of(0xff, 349_523), text_of(b'a', L - 9), text_of(b'a', L - 8)];
    let (site, t) = site_answering(answers);
    let d = dir("site", &[("site.qwe", &site_server(site))]);
    let _gone = Gone(d.clone());
    checked(&d, "site.qwe");
    let (_g, port) = serve(&d, "site.qwe", &["--max-memory", "1MB"], "site.log");
    let refused = (500, "internal error\n".to_string());
    let want = [refused.clone(), (200, "349522 characters".into()), refused.clone(), (200, format!("{} characters", L - 9)), refused];
    for (i, w) in want.iter().enumerate() {
        assert_eq!(&get(port, "/big"), w, "answer {}", i);
    }
    let said = format!("site.qwe:3: {}", STOP);
    let log = log_holding(&d.join("site.log"), &said, 3);
    assert_eq!(log.matches(&said).count(), 3, "{}", log);
    assert!(!log.contains("no answer from"), "{}", log);
    let got = t.join().unwrap();
    assert!(got.len() == 5 && got.iter().all(|l| l.starts_with("POST /_quillo/rpc/big ")), "{:?}", got);
}

/// A site's answer is read into values held to `--max-memory` as they are made, as `http`'s JSON
/// is: 698 KB of `[0,0,...]` (349,001 zeros, 11 MB of list) ends the request, 32,768 zeros (the
/// limit's room) are given and one more is refused; unset, the 349,001 are given as before
/// (src/sites.rs `call`; coverage of 0.8.7, Q-1109; branch coverage-087).
#[test]
fn a_sites_answer_is_read_into_values_held_to_the_limit() {
    let (site, t) = site_answering(vec![zeros(349_001), zeros(32_768), zeros(32_769), zeros(349_001)]);
    let d = dir("sitejson", &[("site.qwe", &site_server(site))]);
    let _gone = Gone(d.clone());
    let (g, port) = serve(&d, "site.qwe", &["--max-memory", "1MB"], "site.log");
    assert_eq!(get(port, "/big"), (500, "internal error\n".into()));
    assert_eq!(get(port, "/big"), (200, "32768 items".into()));
    assert_eq!(get(port, "/big"), (500, "internal error\n".into()));
    let said = format!("site.qwe:3: {}", STOP);
    let log = log_holding(&d.join("site.log"), &said, 2);
    assert_eq!(log.matches(&said).count(), 2, "{}", log);
    drop(g);
    let (_g, port) = serve(&d, "site.qwe", &[], "unset.log");
    assert_eq!(get(port, "/big"), (200, "349001 items".into()));
    assert_eq!(t.join().unwrap().len(), 4);
}

/// The marks of an island's JSON in a site's answer are read as before, now from values held to
/// `--max-memory`: a decimal, an int past JavaScript's, a map of one `$` key, an error value, and
/// the same marks deeper; `{"err": …}` is an error value (src/sites.rs `island_marks`; coverage of
/// 0.8.7, Q-1109; branch coverage-087).
#[test]
fn a_sites_marks_are_read_as_before() {
    let d = dir("marks", &[]);
    let _gone = Gone(d.clone());
    let body = r#""ok":[{"$dec":"12.50"},{"$int":"9007199254740993"},{"$":["$k",{"$dec":"1"}]},{"$quillo_err":{"why":[{"$int":"x"}]}},{"a":{"$dec":5}},{"$dec":"1","b":2},[[{"$quillo_err":"deep"}]]]"#;
    for args in [&["--max-memory", "1MB"][..], &[]] {
        let one = |b: &'static str| Answer { head: b, unit: Vec::new(), n: 0, tail: "" };
        let (site, t) = site_answering(vec![one(body), one(r#""err":{"$dec":"2"}"#)]);
        let src = format!("site shop = \"http://127.0.0.1:{}\"\nlet r = shop.big()\nprint(r)\nprint(r[2][\"$k\"] + 1m)\nprint(shop.big())\n", site);
        let (code, out, err) = script(&d, "marks.qwe", &src, args);
        assert_eq!(
            (code, out.as_str()),
            (Some(0), "[12.50m, 9007199254740993, { \"$k\": 1m }, err({ why: [\"x\"] }), { a: { \"$dec\": 5 } }, { \"$dec\": \"1\", b: 2 }, [[err(\"deep\")]]]\n2\nerr(2m)\n"),
            "{:?}: {}",
            args,
            err
        );
        checked(&d, "marks.qwe");
        assert_eq!(t.join().unwrap().len(), 2);
    }
}

/// Without `--max-memory`, or with one past 64 MiB, a site's answer is read to 64 MiB as before and
/// its message is unchanged; 600 KB not UTF-8 is made whole (src/fetch.rs `site_post`; coverage of
/// 0.8.7, Q-1105; branch coverage-087).
#[test]
fn without_max_memory_a_sites_answer_is_held_to_64_mib_as_before() {
    let d = dir("site64", &[]);
    let _gone = Gone(d.clone());
    for args in [&[][..], &["--max-memory", "1GB"][..]] {
        let (port, t) = site_answering(vec![text_of(b'a', 64 << 20), text_of(0xff, 600_000)]);
        let (code, out, err) = script(&d, "site.qwe", &site_calls(2, port), args);
        let long = format!("err(\"site shop: no answer from http://127.0.0.1:{0}: http://127.0.0.1:{0}/_quillo/rpc/big: the answer is longer than 64 MiB, the most a site's call reads\")", port);
        assert_eq!((code, out.lines().collect::<Vec<_>>()), (Some(0), vec![long.as_str(), "600000 characters"]), "{:?}: {}", args, err);
        checked(&d, "site.qwe");
        assert_eq!(t.join().unwrap().len(), 2);
    }
}

// ---- a pattern too deep -------------------------------------------------------------------

/// A pattern nested more than the regex crate's 250 levels is refused as too big, with its reason,
/// where 250 compile; `re.valid` says false, and `??` takes the error (src/re.rs
/// `compile_within`; FEEDBACK71.md, Q-975; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_pattern_nested_past_250_levels_is_refused_with_its_reason() {
    let d = dir("re", &[]);
    let _gone = Gone(d.clone());
    let src = "for n in [250, 251] {\n    let p = \"(\".repeat(n) + \"a\" + \")\".repeat(n)\n    print(`${n} ${re.valid(p)} ${re.test(\"a\", p) ?? \"refused\"}`)\n}\nlet p = \"(\".repeat(251) + \"a\" + \")\".repeat(251)\nprint(re.find(\"a\", p))\n";
    let (code, out, err) = script(&d, "re.qwe", src, &[]);
    assert_eq!(out, "250 true true\n251 false refused\n", "{}", err);
    assert_ne!(code, Some(0));
    assert!(err.contains("quillo: re.qwe:6: `re.find`: the pattern is too big: error: exceed the maximum number of nested parentheses/brackets (250)"), "{}", err);
    checked(&d, "re.qwe");
}
