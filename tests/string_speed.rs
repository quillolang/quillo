//! Loops over texts, faster than Python's (FEEDBACK55.md, string speed; branch feedback55):
//! `s[i]`, `len(s)`, a character compared, `s += x`, and `m = m | set(k, get(m, k, 0) + 1)`.
//! Each program here runs twice, compiled (src/jitloop.rs) and with `QUILLO_JIT=0`, and must
//! print, fail and end the same; each loop goes past the rounds a loop runs before it is
//! compiled, and `text_loops_are_compiled` checks that the machine code is what runs.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("string-speed-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

/// `quillo run`, compiled or not, under a deadline.
fn run(d: &Path, jit: bool) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .arg("--no-check")
        .env("QUILLO_JIT", if jit { "1" } else { "0" })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the program did not end: {}", d.display());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}

fn shown(o: &Output) -> (String, String, Option<i32>) {
    (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned(), o.status.code())
}

/// What it printed and said, and how it ended: the same both ways, and `want` printed.
fn same(name: &str, src: &str, want: &str) {
    let d = dir(name, src);
    let (a, b) = (shown(&run(&d, true)), shown(&run(&d, false)));
    assert_eq!(a, b, "{}: compiled, then interpreted", name);
    assert_eq!(a.0, want, "{}: {}", name, a.1);
}

/// It fails, the same both ways, having printed `printed` and saying `why`.
fn fails(name: &str, src: &str, printed: &str, why: &str) {
    let d = dir(name, src);
    let (a, b) = (shown(&run(&d, true)), shown(&run(&d, false)));
    assert_eq!(a, b, "{}: compiled, then interpreted", name);
    assert_eq!(a.2, Some(1), "{}", name);
    assert_eq!(a.0, printed, "{}", name);
    assert!(a.1.contains(why), "{}: {}", name, a.1);
}

// ---- string speed ----

#[test]
fn a_text_read_by_index_in_a_compiled_loop_is_the_interpreters() {
    // ASCII, by `for` and by `while`, `==` and `!=`.
    same("ascii", "fn f(s) {\n    let c = 0\n    for i in 0..len(s) {\n        if s[i] == \"x\" {\n            c += 1\n        }\n    }\n    c\n}\nfn g(s) {\n    let i = 0\n    let c = 0\n    while i < len(s) {\n        if s[i] != \"b\" {\n            c += 1\n        }\n        i += 1\n    }\n    c\n}\nprint(f(repeat(\"xyz\", 1000)), g(repeat(\"ab\", 1000)))\n", "1000 1000\n");
    // Characters of two, three and four bytes; a text of one character, a decomposed é (two
    // characters), two characters, and the empty text, which no character is.
    same(
        "unicode",
        "fn f(s, ch) {\n    let c = 0\n    let last = 0\n    for i in 0..len(s) {\n        if s[i] == ch {\n            c += 1\n            last = i\n        }\n    }\n    [c, last, len(s)]\n}\nlet s = repeat(\"héllo wörld ✓ 😀 \", 100)\nprint(f(s, \"ö\"), f(s, \"😀\"), f(s, \"✓\"), f(s, \" \"), f(s, \"o\"), f(s, \"é\"), f(s, \"e\u{301}\"), f(s, \"😀😀\"), f(s, \"\"))\n",
        "[100, 1591, 1600] [100, 1598, 1600] [100, 1596, 1600] [400, 1599, 1600] [100, 1588, 1600] [100, 1585, 1600] [0, 0, 1600] [0, 0, 1600] [0, 0, 1600]\n",
    );
    // The empty text, one character of one byte and of two, and long ones: past either end
    // is null, and from the end when negative.
    same("empty", "fn f(s) {\n    let nulls = 0\n    let xs = 0\n    for i in -600..600 {\n        if s[i] == null {\n            nulls += 1\n        }\n        if s[i] == \"x\" {\n            xs += 1\n        }\n    }\n    [nulls, xs]\n}\nprint(f(\"\"), f(\"x\"), f(\"é\"), f(repeat(\"x\", 300)), f(repeat(\"ü\", 300)))\n", "[1200, 0] [1198, 2] [1198, 0] [600, 600] [600, 0]\n");
    same("ends", "fn f(s) {\n    let a = 0\n    let b = 0\n    for i in -2000..2000 {\n        if s[i] == \"a\" {\n            a += 1\n        } else if s[i] != null {\n            b += 1\n        }\n    }\n    [a, b]\n}\nprint(f(repeat(\"ab\", 500)), f(repeat(\"aé\", 500)), f(repeat(\"😀a\", 500)))\n", "[1000, 1000] [1000, 1000] [1000, 1000]\n");
    // The same loop for a text, a list of texts, a text of two-byte characters, and nothing.
    same("retype", "fn count(s) {\n    let c = 0\n    for i in 0..len(s) {\n        if s[i] == \"a\" {\n            c += 1\n        }\n    }\n    c\n}\nprint(count(repeat(\"ab\", 500)), count(range(0, 1000) | map((i) => \"a\")), count(repeat(\"éa\", 500)), count(\"\"))\n", "500 1000 500 0\n");
    // At a program's top, while it loads.
    same("top", "let s = repeat(\"tick tock \", 200)\nlet n = 0\nlet i = 0\nwhile i < len(s) {\n    if s[i] == \"t\" {\n        n += 1\n    }\n    i += 1\n}\nlet out = \"\"\nfor j in 0..2000 {\n    out += s[j % 10]\n}\nprint(n, len(out), out[0], out[-1])\n", "400 2000 t  \n");
}

#[test]
fn characters_compare_as_the_interpreter_compares_them() {
    // Ranges of characters, a text on either side, a text longer than a character (after the
    // character it starts with), the empty text, and two characters of the text.
    same("order", "fn f(s) {\n    let lower = 0\n    let digit = 0\n    let other = 0\n    let up = 0\n    let before = 0\n    let rising = 0\n    for i in 0..len(s) {\n        let c = s[i]\n        if c >= \"a\" && c <= \"z\" {\n            lower += 1\n        } else if \"0\" <= c && \"9\" >= c {\n            digit += 1\n        } else if c > \"Z\" {\n            other += 1\n        }\n        if c < \"ab\" {\n            before += 1\n        }\n        if c >= \"\" && c >= \"A\" && c < \"B\" {\n            up += 1\n        }\n        if i + 1 < len(s) && s[i] < s[i + 1] {\n            rising += 1\n        }\n    }\n    [lower, digit, other, up, before, rising]\n}\nprint(f(repeat(\"aZ9_bA é😀~\", 100)))\n", "[200, 100, 400, 100, 600, 400]\n");
    for (i, (op, want)) in [("<", "600 0"), ("<=", "600 600"), (">", "300 300"), (">=", "300 900")].iter().enumerate() {
        // A longer text is after the character it starts with, "a" before "ab"; "b" is after
        // both.
        same(&format!("longer-{}", i), &format!("fn f(s) {{\n    let c = 0\n    let d = 0\n    for i in 0..len(s) {{\n        if s[i] {0} \"ab\" {{\n            c += 1\n        }}\n        if s[i] {0} \"a\" {{\n            d += 1\n        }}\n    }}\n    print(c, d)\n}}\nf(repeat(\"ab\", 300) + repeat(\"a\", 300))\n", op), &format!("{}\n", want));
    }
    // A character kept, and changed, in a variable.
    same("kept", "fn f(s) {\n    let pairs = 0\n    let prev = s[0]\n    for i in 1..len(s) {\n        let c = s[i]\n        if c == prev {\n            pairs += 1\n        }\n        prev = c\n    }\n    pairs\n}\nprint(f(repeat(\"aabéé\", 300)))\n", "600\n");
    // With a number: never the same, as the interpreter has it (not compiled).
    same("number", "fn f(s) {\n    let c = 0\n    for i in 0..1000 {\n        if s[i % 3] == 5 {\n            c += 1\n        }\n        if s[i % 3] != 5 {\n            c += 2\n        }\n    }\n    c\n}\nprint(f(\"abc\"))\n", "2000\n");
}

#[test]
fn comparing_null_fails_as_the_interpreter_fails() {
    fails("null-text", "fn f(s) {\n    let c = 0\n    for i in 0..1000 {\n        if s[i] < \"m\" {\n            c += 1\n        }\n    }\n    c\n}\nprint(\"start\")\nprint(f(repeat(\"ab\", 400)))\n", "start\n", "cannot compare null with str");
    // After the round added to a text, and null on the right.
    fails("null-char", "fn f(s) {\n    let c = 0\n    let out = \"\"\n    for i in 0..1000 {\n        out += \"x\"\n        if s[i] > s[i + 300] {\n            c += 1\n        }\n    }\n    c\n}\nprint(f(repeat(\"ab\", 600)))\n", "", "cannot compare str with null");
    // The text on the left.
    fails("null-left", "fn f(s) {\n    let c = 0\n    for i in 0..1000 {\n        if \"m\" >= s[i] {\n            c += 1\n        }\n    }\n    c\n}\nprint(f(repeat(\"ab\", 400)))\n", "", "cannot compare str with null");
}

#[test]
fn a_text_added_to_in_a_loop_is_the_interpreters() {
    // Growing while its length is asked, by `+=` and by `s = s + x`, with two-byte characters;
    // and a text grown by its own characters.
    same("grow", "fn f() {\n    let s = \"\"\n    let t = \"abc\"\n    let n = 0\n    while len(s) < 5000 {\n        s += \"ab\"\n        if len(s) % 3 == 0 {\n            s = s + \"é\"\n        }\n        n += 1\n    }\n    for i in 0..1000 {\n        t += t[i % 3]\n    }\n    [len(s), n, len(t), s[0], s[4], s[-1], t[999], t[-1]]\n}\nprint(f())\n", "[5002, 1668, 1003, \"a\", \"a\", \"é\", \"a\", \"a\"]\n");
    // Characters of a text, a text in a variable, and the text read backwards.
    same("chars", "fn f(s) {\n    let out = \"\"\n    let up = \"\"\n    let sep = \"-\"\n    for i in 0..len(s) {\n        let ch = s[i]\n        if ch == \"a\" || ch == \"é\" {\n            out += ch\n        } else {\n            out += sep\n        }\n        up += s[len(s) - 1 - i]\n    }\n    [len(out), up == s, out[0], out[1], out[2], out[-1]]\n}\nprint(f(repeat(\"abé😀\", 100)))\n", "[400, false, \"a\", \"-\", \"é\", \"-\"]\n");
    // Null added is the text `null`, as `+` has it.
    same("null", "fn f(s) {\n    let out = \"\"\n    for i in 0..600 {\n        out += s[i]\n    }\n    [len(out), out[0], out[-1]]\n}\nprint(f(repeat(\"ab\", 290)))\n", "[660, \"a\", \"l\"]\n");
    // A text added to itself.
    same("itself", "fn f() {\n    let s = \"ab\"\n    let t = \"é\"\n    for i in 0..1000 {\n        if i % 100 == 0 {\n            s += s\n        }\n        t += s[0]\n    }\n    [len(s), len(t), t[0], t[1]]\n}\nprint(f())\n", "[2048, 1001, \"é\", \"a\"]\n");
    // Lengths of a text and of one added to, in characters.
    same("lengths", "fn f(s, t) {\n    let a = 0\n    for i in 0..1000 {\n        a += len(s) + len(t)\n        t += \"é\"\n    }\n    [a, len(t)]\n}\nprint(f(\"😀😀\", \"\"))\n", "[501500, 1000]\n");
    // One a million characters long, as bench/vs-python has it.
    same("million", "fn f(n) {\n    let s = \"\"\n    for i in 0..n {\n        s += \"x\"\n    }\n    len(s)\n}\nprint(f(1000000))\n", "1000000\n");
}

#[test]
fn what_else_holds_a_text_keeps_what_it_had() {
    same("alias", "fn f() {\n    let s = repeat(\"q\", 100)\n    let t = s\n    let u = s\n    for i in 0..1000 {\n        s += \"x\"\n        u = u + \"é\"\n    }\n    [len(s), len(t), len(u), t == repeat(\"q\", 100), s[99], s[100], u[100], u[-1]]\n}\nlet g = \"top\"\nlet h = g\nfor i in 0..1000 {\n    g += \"!\"\n}\nprint(f(), len(g), h, g[3], g[-1])\n", "[1100, 100, 1100, true, \"q\", \"x\", \"é\", \"é\"] 1003 top ! !\n");
    // Read while another variable that holds it is added to.
    same("read-alias", "fn f() {\n    let s = repeat(\"ab\", 300)\n    let t = s\n    let c = 0\n    for i in 0..600 {\n        t += \"a\"\n        if s[i] == \"a\" {\n            c += 1\n        }\n    }\n    [c, len(s), len(t)]\n}\nprint(f())\n", "[300, 600, 1200]\n");
}

#[test]
fn an_error_in_a_text_loop_is_the_interpreters() {
    fails("overflow", "fn f(s, k) {\n    let c = 0\n    for i in 0..1000 {\n        if s[i + k] == \"a\" {\n            c += 1\n        }\n    }\n    c\n}\nprint(f(\"abc\", 9223372036854775000))\n", "", "integer overflow");
    fails("float-index", "fn f(s) {\n    let c = 0\n    let x = 0.0\n    for i in 0..1000 {\n        x += 1.0\n        if i > 600 && s[x] == \"a\" {\n            c += 1\n        }\n    }\n    c\n}\nprint(f(\"abc\"))\n", "", "cannot index str with float");
}

#[test]
fn a_server_loop_on_a_global_text_stays_followed() {
    let src = "let text = repeat(\"ab\", 600)\nlet log = \"\"\nroute GET /count {\n    let c = 0\n    for i in 0..len(text) {\n        if text[i] == \"a\" {\n            c += 1\n        }\n    }\n    str(c)\n}\nroute GET /grow {\n    for i in 0..1000 {\n        log += \"x\"\n    }\n    str(len(log))\n}\n";
    let d = dir("served", src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    assert_eq!(support::send(s.port, "GET", "/count", &[], "").body, "600");
    assert_eq!(support::send(s.port, "GET", "/grow", &[], "").body, "1000");
    assert_eq!(support::send(s.port, "GET", "/grow", &[], "").body, "2000");
    assert_eq!(support::send(s.port, "GET", "/count", &[], "").body, "600");
}

/// The code really runs: each of these loops is compiled, on this thread.
#[test]
fn text_loops_are_compiled() {
    let progs = [
        // bench/vs-python's "s[i]", "while over a text" and "string +=".
        "fn f(s) {\n    let c = 0\n    for i in 0..len(s) {\n        if s[i] == \"x\" {\n            c += 1\n        }\n    }\n    c\n}\nlet r = f(repeat(\"x\", 3000))\n",
        "fn f(s) {\n    let i = 0\n    let c = 0\n    while i < len(s) {\n        if s[i] == \"a\" {\n            c += 1\n        }\n        i += 1\n    }\n    c\n}\nlet r = f(repeat(\"ab\", 1000))\n",
        "fn f(n) {\n    let s = \"\"\n    for i in 0..n {\n        s += \"x\"\n    }\n    len(s)\n}\nlet r = f(1000)\n",
        // A character kept and ordered, added to another text; null; a text of two-byte ones.
        "fn f(s) {\n    let out = \"\"\n    for i in 0..len(s) {\n        let c = s[i]\n        if c >= \"a\" && c <= \"z\" || c == null {\n            out = out + c\n        }\n    }\n    out\n}\nlet r = f(repeat(\"aé\", 1000))\n",
    ];
    for src in progs {
        let before = quillo::jitloop::compiled();
        let mut it = quillo::Interp::new(".");
        it.load_source(src, "main.qwe").unwrap();
        assert!(quillo::jitloop::compiled() > before, "not compiled:\n{}", src);
    }
    // A character with a number is not, nor is a text read by index where it is added to.
    for src in [
        "fn f(s) {\n    let c = 0\n    for i in 0..1000 {\n        if s[i % 3] == 5 {\n            c += 1\n        }\n    }\n    c\n}\nlet r = f(\"abc\")\n",
        "fn f() {\n    let s = \"ab\"\n    let c = 0\n    for i in 0..1000 {\n        s += \"x\"\n        if s[i] == \"x\" {\n            c += 1\n        }\n    }\n    c\n}\nlet r = f()\n",
    ] {
        let before = quillo::jitloop::compiled();
        let mut it = quillo::Interp::new(".");
        it.load_source(src, "main.qwe").unwrap();
        assert_eq!(quillo::jitloop::compiled(), before, "compiled:\n{}", src);
    }
}

#[test]
fn a_count_kept_in_a_map_is_the_long_ways() {
    // Word frequencies in all three forms, with keys of two-byte characters, in the order
    // first seen.
    same("count-forms", "let words = split(lower(\"The cat and the hat and THE bat café Café über the\"), \" \")\nlet a = {}\nlet b = {}\nlet c = {}\nfor w in words {\n    a = a | set(w, get(a, w, 0) + 1)\n    b = set(b, w, get(b, w, 0) + 1)\n    c = c.set(w, c.get(w, 0) + 1)\n}\nprint(a)\nprint(a == b, b == c)\n", "{ the: 4, cat: 1, and: 2, hat: 1, bat: 1, café: 2, über: 1 }\ntrue true\n");
    // `-` and `*`, a float and a text to start from, and a key worked out by a call.
    same("count-ops", "let m = {}\nlet n = {}\nlet p = {}\nlet q = {}\nfor i in 0..20 {\n    let k = str(i % 3)\n    m = m | set(k, get(m, k, 100) - i)\n    n = n | set(k, get(n, k, 1) * 2)\n    p = p | set(k, get(p, k, 0.5) + 1)\n    q = q | set(k, get(q, k, \"\") + str(i))\n}\nprint(m, n, p, q)\n", "{ \"0\": 37, \"1\": 30, \"2\": 43 } { \"0\": 128, \"1\": 128, \"2\": 64 } { \"0\": 7.5, \"1\": 7.5, \"2\": 6.5 } { \"0\": \"0369121518\", \"1\": \"14710131619\", \"2\": \"258111417\" }\n");
    // The same as a form written another way, which goes the long way.
    same("count-long", "let m = {}\nlet n = {}\nfor i in 0..20 {\n    let k = str(i % 3)\n    m = m | set(k, get(m, k, 100) - i)\n    n = n | set(k, 0 + get(n, k, 100) - i)\n}\nprint(m == n, m)\n", "true { \"0\": 37, \"1\": 30, \"2\": 43 }\n");
    // In a function, on a local.
    same("count-local", "fn freq(text) {\n    let counts = {}\n    for w in split(text, \" \") {\n        counts = counts | set(w, get(counts, w, 0) + 1)\n    }\n    counts\n}\nlet r = freq(\"a b a c b a\")\nprint(r, len(r), r.a)\n", "{ a: 3, b: 2, c: 1 } 3 3\n");
    // Keys that are numbers go the long way, as their text.
    same("count-int-keys", "let m = {}\nfor i in 0..10 {\n    m = m | set(i % 4, get(m, i % 4, 0) + 1)\n}\nprint(m)\n", "{ \"0\": 3, \"1\": 3, \"2\": 2, \"3\": 2 }\n");
}

#[test]
fn what_else_holds_a_counted_map_keeps_what_it_had() {
    same("count-alias", "let m = { x: 1 }\nlet kept = m\nlet mid = {}\nfor i in 0..10 {\n    m = m | set(\"x\", get(m, \"x\", 0) + 1)\n    if i == 4 {\n        mid = m\n    }\n}\nprint(m, kept, mid)\n", "{ x: 11 } { x: 1 } { x: 6 }\n");
    // A map's own `get`, and a `get` of the program's: theirs are called.
    same("count-held", "let m = { get: (k, d) => 100 }\nfor w in [\"a\", \"b\", \"a\"] {\n    m = m.set(w, m.get(w, 0) + 1)\n}\nprint(m.a, m.b)\n", "101 101\n");
    same("count-shadow", "fn tally(words) {\n    let get = (m, k, d) => 7\n    let m = {}\n    for w in words {\n        m = m | set(w, get(m, w, 0) + 1)\n    }\n    m\n}\nprint(tally([\"a\", \"b\", \"a\"]))\n", "{ a: 8, b: 8 }\n");
}

#[test]
fn a_count_that_fails_fails_as_the_long_way() {
    // No value to start from: null and an int.
    fails("count-no-start", "let m = { a: 1 }\nprint(get(m, \"a\"))\nfor w in [\"a\", \"a\", \"b\"] {\n    m = m | set(w, get(m, w) + 1)\n}\nprint(m)\n", "1\n", "main.qwe:4: cannot add null and int");
    fails("count-bool", "let m = {}\nfor w in [\"a\", \"b\"] {\n    m = m | set(w, get(m, w, true) * 2)\n}\nprint(m)\n", "", "main.qwe:3: cannot multiply bool and int");
}

#[test]
fn values_compare_as_before() {
    // Two that hold no others are compared at once; lists, maps and errors item by item.
    same("count-equal", "let pairs = [[1, 1], [1, 1.0], [1, \"1\"], [\"a\", \"a\"], [\"a\", \"b\"], [\"\", \"\"], [\"é\", \"é\"], [null, null], [null, false], [true, true], [[1, [2]], [1, [2]]], [[1], [1.0]], [{ a: 1 }, { a: 1 }], [{ a: 1 }, { a: 2 }], [[], {}], [err(\"x\"), err(\"x\")], [err(\"x\"), \"x\"], [1.5m, 1.5], [0.0 / 0.0, 0.0 / 0.0]]\nlet out = []\nfor p in pairs {\n    out = out | push(p[0] == p[1])\n    out = out | push(p[0] != p[1])\n}\nprint(out)\nlet s = \"ab\"\nlet t = s\nprint(s == t, s == \"a\" + \"b\", s != t)\n", "[true, false, true, false, false, true, true, false, false, true, true, false, true, false, true, false, false, true, true, false, true, false, true, false, true, false, false, true, false, true, true, false, false, true, true, false, false, true]\ntrue true false\n");
}

#[test]
fn an_int_keyed_count_in_a_compiled_loop_is_the_interpreters() {
    // Keys either side of 0 and near the ends of an int, `-` too, in a function.
    same("mc-keys", "fn f(n) {\n    let m = {}\n    for i in 0..n {\n        let k = i % 7 - 3\n        m = m | set(k, get(m, k, 0) + 1)\n        m = set(m, k * 1000000000000000000, get(m, k * 1000000000000000000, 10) - i)\n    }\n    m\n}\nprint(f(1000))\n", "{ \"-3\": 143, \"-3000000000000000000\": -71061, \"-2\": 143, \"-2000000000000000000\": -71204, \"-1\": 143, \"-1000000000000000000\": -71347, \"0\": -71357, \"1\": 143, \"1000000000000000000\": -71633, \"2\": 143, \"2000000000000000000\": -71776, \"3\": 142, \"3000000000000000000\": -70919 }\n");
    // A float to start from, a text and a float already there, a float added.
    same("mc-types", "fn f() {\n    let a = {}\n    let b = { \"1\": \"x\", \"2\": 2.5 }\n    let c = {}\n    for i in 0..1000 {\n        a = a | set(i % 3, get(a, i % 3, 0.5) + 1)\n        b = b | set(i % 3, get(b, i % 3, 1) + 1)\n        c = c | set(i % 4, get(c, i % 4, 0) + i * 0.5)\n    }\n    [a, b[\"0\"], len(b[\"1\"]), b[\"2\"], c]\n}\nprint(f())\n", "[{ \"0\": 334.5, \"1\": 333.5, \"2\": 333.5 }, 335, 334, 335.5, { \"0\": 62250.0, \"1\": 62375.0, \"2\": 62500.0, \"3\": 62625.0 }]\n");
    // The pipe and the call compiled; `m.set` and no value to start from go the long way.
    same("mc-forms", "fn f() {\n    let a = {}\n    let b = {}\n    let c = {}\n    let d = {}\n    for i in 0..1000 {\n        a = a | set(i % 3, get(a, i % 3, 0) + 1)\n        b = set(b, i % 3, get(b, i % 3, 0) + 1)\n        c = c.set(i % 3, c.get(i % 3, 0) + 1)\n        d = d | set(i % 3, get(d, i % 3) ?? 0)\n    }\n    [a == b, b == c, d]\n}\nprint(f())\n", "[true, true, { \"0\": 0, \"1\": 0, \"2\": 0 }]\n");
    // In a `while`, with a key read from a list.
    same("mc-while", "fn f(xs) {\n    let m = {}\n    let i = 0\n    while i < len(xs) {\n        m = m | set(xs[i] % 10, get(m, xs[i] % 10, 0) + 1)\n        i += 1\n    }\n    m\n}\nprint(f(range(0, 1000) | map((x) => x * x)))\n", "{ \"0\": 100, \"1\": 200, \"4\": 200, \"9\": 200, \"6\": 200, \"5\": 100 }\n");
    // In an inner loop.
    same("mc-nested", "fn f() {\n    let m = {}\n    let t = 0\n    for i in 0..100 {\n        for j in 0..30 {\n            m = m | set(j % 4, get(m, j % 4, 0) + i)\n        }\n        t += i\n    }\n    [m, t]\n}\nprint(f())\n", "[{ \"0\": 39600, \"1\": 39600, \"2\": 34650, \"3\": 34650 }, 4950]\n");
    // What else holds the map keeps what it had, in a function and at a program's top.
    same("mc-alias", "fn f() {\n    let m = { \"0\": 100 }\n    let kept = m\n    for i in 0..1000 {\n        m = m | set(i % 3, get(m, i % 3, 0) + 1)\n    }\n    [m, kept]\n}\nlet g = {}\nlet h = g\nfor i in 0..1000 {\n    g = g | set(i % 5, get(g, i % 5, 0) + 2)\n}\nprint(f(), g, h)\n", "[{ \"0\": 434, \"1\": 333, \"2\": 333 }, { \"0\": 100 }] { \"0\": 400, \"1\": 400, \"2\": 400, \"3\": 400, \"4\": 400 } {}\n");
}

#[test]
fn an_int_keyed_count_that_fails_fails_as_the_interpreter_fails() {
    // `*` past the last int, after the round's write.
    fails("mc-overflow", "fn f() {\n    let m = {}\n    for i in 0..1000 {\n        m = m | set(i % 2, get(m, i % 2, 1) * 3)\n    }\n    m\n}\nprint(\"start\")\nprint(f())\n", "start\n", "main.qwe:4: integer overflow");
    // A value of another type already there.
    fails("mc-bool", "fn f() {\n    let m = { \"5\": true }\n    for i in 0..1000 {\n        m = m | set(i % 10, get(m, i % 10, 0) + 1)\n    }\n    m\n}\nprint(f())\n", "", "main.qwe:4: cannot add bool and int");
    // The key's own arithmetic.
    fails("mc-key-overflow", "fn f(big) {\n    let m = {}\n    for i in 0..1000 {\n        m = m | set(big + i, get(m, big + i, 0) + 1)\n    }\n    len(m)\n}\nprint(f(9223372036854775000))\n", "", "main.qwe:4: integer overflow");
}

#[test]
fn a_count_works_out_its_key_once_and_as_the_long_way() {
    // A key or an `e` with an effect goes the long way, worked out as often as it is written: twice and once a round (400 rounds, 2,000 in all), a map's own `get` too.
    same("count-side", "let log = []\nfn key(i) {\n    log = log | push(i)\n    i % 3\n}\nfn one(i) {\n    log = log | push(i)\n    1\n}\nlet held = { get: (k, d) => 100 }\nlet m = {}\nlet n = {}\nfor i in 0..400 {\n    m = m | set(key(i), get(m, key(i), 0) + 1)\n    n = n | set(i % 3, get(n, i % 3, 0) + one(i))\n    held = held.set(key(i), held.get(key(i), 0) + 1)\n}\nprint(m, n, held[\"0\"], len(log))\n", "{ \"0\": 134, \"1\": 133, \"2\": 133 } { \"0\": 134, \"1\": 133, \"2\": 133 } 101 2000\n");
    // A `get` of the program's, the same.
    same("count-side-shadow", "let log = []\nfn key(i) {\n    log = log | push(i)\n    i % 2\n}\nfn tally(n) {\n    let get = (m, k, d) => 7\n    let m = {}\n    for i in 0..n {\n        m = m | set(key(i), get(m, key(i), 0) + 1)\n    }\n    m\n}\nprint(tally(300), len(log))\n", "{ \"0\": 8, \"1\": 8 } 600\n");
    // Keys of every type, each read as `get` and `set` read it.
    same("count-mixed-keys", "let m = {}\nlet ks = [3, -7, 0, 9223372036854775807, -9223372036854775807 - 1, 1.5, 2.0, 1.50m, true, null, [1, 2], \"3\", \"x\"]\nfor r in 0..3 {\n    for k in ks {\n        m = m | set(k, get(m, k, 0) + 1)\n    }\n}\nprint(m)\n", "{ \"3\": 6, \"-7\": 3, \"0\": 3, \"9223372036854775807\": 3, \"-9223372036854775808\": 3, \"1.5\": 6, \"2.0\": 3, true: 3, null: 3, \"[1, 2]\": 3, x: 3 }\n");
    // An error is no key: `get` gives it back, `+ "x"` of it is a text, `set` gives it back, and the map is then the error.
    fails("count-err-key", "let m = { a: 1 }\nfor w in [\"a\", \"q\"] {\n    m = m | set(int(w), get(m, int(w), \"\") + \"x\")\n}\nprint(m)\n", "", "main.qwe:3: `get` expects a map as argument 1, got error (it failed before this call, ");
    // The key fails as it is worked out.
    fails("count-key-fails", "let m = {}\nfor i in [0, 1, 2, 3] {\n    m = m | set(i % (3 - i), get(m, i % (3 - i), 0) + 1)\n}\nprint(m)\n", "", "main.qwe:3: division by zero");
    // `e` fails as it is worked out.
    fails("count-more-fails", "let m = {}\nfor i in [0, 1, 2, 3] {\n    m = m | set(i % 2, get(m, i % 2, 0) + 6 % (3 - i))\n}\nprint(m)\n", "", "main.qwe:3: division by zero");
    // A histogram over a list, in the interpreter.
    same("count-hist", "fn hist(xs) {\n    let m = {}\n    for x in xs {\n        m = m | set(x % 50, get(m, x % 50, 0) + 1)\n    }\n    m\n}\nlet h = hist(range(0, 3000))\nprint(len(h), h[\"0\"], h[\"49\"])\n", "50 60 60\n");
}

#[test]
fn a_map_set_by_an_int_key_in_a_loop_is_changed_where_it_is() {
    // 200,000 rounds: a copy each round took a second; what else holds the map keeps it.
    same("set-int-key", "fn f(xs) {\n    let m = { \"0\": -1 }\n    let kept = m\n    for x in xs {\n        m = m | set(x % 1000, x)\n    }\n    [len(m), m[\"999\"], m[\"0\"], kept]\n}\nprint(f(range(0, 200000)))\n", "[1000, 199999, 199000, { \"0\": -1 }]\n");
}

/// A count by an int in a loop over a range is compiled, as `for` or `while`, in a function or
/// at a program's top; `m.set(...)`, a function the map may hold, is not.
#[test]
fn counts_in_loops_are_compiled() {
    for src in [
        "fn f(n) {\n    let m = {}\n    for i in 0..n {\n        let k = i * 7 % 1000\n        m = m | set(k, get(m, k, 0) + 1)\n    }\n    len(m)\n}\nlet r = f(1000)\n",
        "let m = {}\nlet i = 0\nwhile i < 1000 {\n    m = set(m, i % 10, get(m, i % 10, 0.5) * 2)\n    i += 1\n}\n",
    ] {
        let before = quillo::jitloop::compiled();
        let mut it = quillo::Interp::new(".");
        it.load_source(src, "main.qwe").unwrap();
        assert!(quillo::jitloop::compiled() > before, "not compiled:\n{}", src);
    }
    let src = "fn f() {\n    let m = {}\n    for i in 0..1000 {\n        m = m.set(i % 3, m.get(i % 3, 0) + 1)\n    }\n    m\n}\nlet r = f()\n";
    let before = quillo::jitloop::compiled();
    let mut it = quillo::Interp::new(".");
    it.load_source(src, "main.qwe").unwrap();
    assert_eq!(quillo::jitloop::compiled(), before, "compiled:\n{}", src);
}
