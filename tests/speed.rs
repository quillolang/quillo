//! Faster than Python, with nothing else changed (FEEDBACK29.md; branch feedback29-fixes):
//! each thing done to be fast has to give what the slow way gave. Names that are one pointer,
//! machine code from the first call, lists and maps changed where they are inside others, text
//! added to where it is. bench/vs-python/run.py has the speed itself.

mod support;

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("speed-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

fn run(name: &str, src: &str) -> Output {
    let d = dir(name, src);
    Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).arg("--no-check").stdin(Stdio::null()).output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn said(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// What it prints, which must be the whole of it.
fn prints(name: &str, src: &str, want: &str) {
    let o = run(name, src);
    assert_eq!(out(&o), want, "{}", said(&o));
    assert!(o.status.success(), "{}", said(&o));
}

/// It fails, saying this.
fn fails(name: &str, src: &str, want: &str) {
    let o = run(name, src);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    assert!(said(&o).contains(want), "{}", said(&o));
}

/// It finishes in time: a copy each round would not, at this size.
fn quick(name: &str, src: &str, want: &str, limit: u64) {
    let start = Instant::now();
    prints(name, src, want);
    assert!(start.elapsed() < Duration::from_secs(limit), "{}: {:?}", name, start.elapsed());
}

// ---- names ---------------------------------------------------------------------------------

#[test]
fn a_name_means_what_it_meant() {
    // The nearest scope has it, and a block's own goes with the block.
    prints("shadow", "let x = 1\nfn f(x) {\n    {\n        let x = 3\n        print(x)\n    }\n    x\n}\nprint(f(2), x)\n", "3\n2 1\n");
    // A closure keeps its own scope, each call a new one.
    prints("closures", "fn counter(n) = () => n\nlet a = counter(1)\nlet b = counter(2)\nprint(a(), b(), a())\n", "1 2 1\n");
    // Names not ascii, and long ones.
    let long = "n".repeat(300);
    prints("unicode", &format!("let 名前 = 1\nlet {} = 2\nprint(名前 + {})\n", long, long), "3\n");
    // Enough variables that a scope moves to its table, and they are all still there.
    let mut src = String::from("fn f() {\n");
    for i in 0..40 {
        src += &format!("    let v{} = {}\n", i, i);
    }
    src += "    v0 + v15 + v16 + v17 + v39\n}\nprint(f())\n";
    prints("many", &src, "87\n");
}

#[test]
fn a_name_never_defined_is_still_an_error() {
    fails("undefined", "print(nowhere)\n", "nowhere");
    fails("undefined-assign", "fn f() {\n    nowhere = 1\n}\nf()\n", "never declared");
    fails("undefined-add", "fn f() {\n    nowhere += 1\n}\nf()\n", "nowhere");
}

#[test]
fn assigning_to_a_global_and_a_local_stay_apart() {
    prints("global", "let t = 0\nfn f() {\n    t = 5\n    t += 1\n}\nf()\nprint(t)\n", "6\n");
    prints("function", "fn f() = 1\nf = () => 2\nprint(f())\n", "2\n");
    // A number that does not fit in place fails as `+` fails, and a float stays one.
    fails("overflow", "fn f() {\n    let n = 9223372036854775807\n    n += 1\n    n\n}\nprint(f())\n", "overflow");
    fails("underflow", "fn f() {\n    let n = -9223372036854775807\n    n -= 2\n    n\n}\nprint(f())\n", "overflow");
    prints("assign-mixed", "fn f() {\n    let n = 1\n    n += 0.5\n    let m = 1.5\n    m += 1\n    let s = 2.5\n    s -= 0.5\n    print(n, m, s)\n}\nf()\n", "1.5 2.5 2.0\n");
    prints("assign-decimal", "fn f() {\n    let n = 1.10m\n    n += 2.20m\n    print(n)\n}\nf()\n", "3.30\n");
}

// ---- machine code from the first call --------------------------------------------------------

#[test]
fn a_loop_in_machine_code_gives_what_the_interpreter_gives() {
    prints("jit-once", "fn f(n: int) -> int {\n    let t = 0\n    for i in 0..n {\n        t += i * i\n    }\n    t\n}\nprint(f(1000))\n", "332833500\n");
    prints("no-rounds", "fn f(n: int) -> int {\n    let t = 7\n    for i in 0..n {\n        t += 1\n    }\n    t\n}\nprint(f(0), f(-5))\n", "7 7\n");
    prints("jit-floats", "fn f(n: int) -> float {\n    let s = 0.0\n    let i = 1\n    while i <= n {\n        s += 1.0 / i\n        i += 1\n    }\n    s\n}\nprint(round(f(1000), 6))\n", "7.485471\n");
    // It gives up, and the interpreter says what it says.
    fails("jit-overflow", "fn f(n: int) -> int {\n    let t = 9223372036854775000\n    for i in 0..n {\n        t += i\n    }\n    t\n}\nprint(f(100000))\n", "overflow");
    fails("jit-zero", "fn f(n: int) -> int {\n    let t = 0\n    for i in 0..n {\n        t += 10 % (5 - i)\n    }\n    t\n}\nprint(f(10))\n", "zero");
    // Not what it can compile: run as before.
    prints("not-numbers", "fn f(n) {\n    let s = \"\"\n    for i in 0..n {\n        s += \"a\"\n    }\n    s\n}\nprint(f(3))\n", "aaa\n");
}

#[test]
fn a_lambda_in_machine_code_gives_what_the_interpreter_gives() {
    prints("lambda-map", "print([1, 2, 3] | map((x) => x * 2))\n", "[2, 4, 6]\n");
    prints("lambda-filter", "print([1, 2, 3, 4] | filter((x) => x % 2 == 0))\n", "[2, 4]\n");
    prints("lambda-empty", "print([] | map((x) => x * 2), [] | filter((x) => x > 0))\n", "[] []\n");
    prints("lambda-one", "print([5] | map((x) => x + 1))\n", "[6]\n");
    // Items of more than one type, in any order: each by what it is.
    prints("lambda-mixed", "print([1, 2.5, \"a\", 3, 1.5] | map((x) => x + x))\n", "[2, 5.0, \"aa\", 6, 3.0]\n");
    prints("mixed-numbers", "let f = (x) => x + 1\nprint(f(1), f(1.5), f(2), f(2.5), f(\"a\"))\n", "2 2.5 3 3.5 a1\n");
    prints("bools", "print([true, false] | map((x) => !x))\n", "[false, true]\n");
    // What it reads from around it is each closure's own.
    prints("captured", "fn adder(k) = (x) => x + k\nlet a = adder(1)\nlet b = adder(10)\nprint([1, 2] | map(a), [1, 2] | map(b), [1, 2] | map(a))\n", "[2, 3] [11, 12] [2, 3]\n");
    // Two lambdas in turn, of different results.
    prints("turns", "let f = (x) => x * 2\nlet g = (x) => x > 1\nprint(f(1), g(1), f(2), g(2), f(3))\n", "2 false 4 true 6\n");
    // One that fails partway says so.
    fails("lambda-overflow", "print([1, 9223372036854775807] | map((x) => x + 1))\n", "overflow");
    fails("lambda-zero", "print([1, 0] | map((x) => 1 % x))\n", "zero");
}

// ---- lists and maps inside others --------------------------------------------------------------

#[test]
fn an_update_inside_keeps_every_alias() {
    prints("alias-row", "fn f() {\n    let g = [[0, 0], [0, 0]]\n    let row = g[0]\n    g = set(g, 0, set(g[0], 1, 9))\n    print(g, row)\n}\nf()\n", "[[0, 9], [0, 0]] [0, 0]\n");
    prints("alias-whole", "fn f() {\n    let g = [[0, 0], [0, 0]]\n    let h = g\n    g = set(g, 0, set(g[0], 1, 9))\n    print(g, h)\n}\nf()\n", "[[0, 9], [0, 0]] [[0, 0], [0, 0]]\n");
    prints("alias-same-row", "fn f() {\n    let row = [0]\n    let g = [row, row]\n    g = set(g, 0, set(g[0], 0, 1))\n    print(g, row)\n}\nf()\n", "[[1], [0]] [0]\n");
    prints("alias-list-in-map", "fn f() {\n    let m = { a: [1] }\n    let a = m.a\n    m = m | set(\"a\", get(m, \"a\", []) | push(2))\n    print(m, a)\n}\nf()\n", "{ a: [1, 2] } [1]\n");
    prints("alias-map-in-list", "fn f() {\n    let g = [{ n: 1 }]\n    let h = g[0]\n    g = set(g, 0, set(g[0], \"n\", 2))\n    print(g, h)\n}\nf()\n", "[{ n: 2 }] { n: 1 }\n");
    // Given to a function, kept by it.
    prints("alias-kept", "fn keep(x) = x\nfn f() {\n    let g = [[1]]\n    let k = keep(g)\n    g = set(g, 0, g[0] | push(2))\n    print(g, k)\n}\nf()\n", "[[1, 2]] [[1]]\n");
    // Pushing what it is itself part of.
    prints("alias-self", "fn f() {\n    let m = { a: [] }\n    m = set(m, \"a\", get(m, \"a\", []) | push(m))\n    print(m)\n}\nf()\n", "{ a: [{ a: [] }] }\n");
}

#[test]
fn an_update_inside_is_the_same_however_it_is_written() {
    let forms = [
        "m = m | set(\"k\", get(m, \"k\", []) | push(1))",
        "m = set(m, \"k\", get(m, \"k\", []) | push(1))",
        "m = set(m, \"k\", push(get(m, \"k\", []), 1))",
        "m = m | set(\"k\", m | get(\"k\", []) | push(1))",
        "m = m.set(\"k\", m.get(\"k\", []).push(1))",
    ];
    for (i, form) in forms.iter().enumerate() {
        prints(&format!("form-{}", i), &format!("fn f() {{\n    let m = {{}}\n    {}\n    {}\n    m\n}}\nprint(f())\n", form, form), "{ k: [1, 1] }\n");
    }
    let forms = ["g = set(g, 1, set(g[1], 0, 5))", "g = g | set(1, g[1] | set(0, 5))", "g = g.set(1, g[1].set(0, 5))"];
    for (i, form) in forms.iter().enumerate() {
        prints(&format!("grid-{}", i), &format!("fn f() {{\n    let g = [[1], [2]]\n    {}\n    g\n}}\nprint(f())\n", form), "[[1], [5]]\n");
    }
}

#[test]
fn the_edges_of_an_update_inside() {
    // A key that is not there, with and without something to start from.
    prints("no-key", "fn f() {\n    let m = {}\n    m = set(m, \"a\", get(m, \"a\", [7]) | push(8))\n    m\n}\nprint(f())\n", "{ a: [7, 8] }\n");
    prints("start-map", "fn f() {\n    let m = {}\n    m = set(m, \"a\", set(get(m, \"a\", {}), \"b\", 1))\n    m\n}\nprint(f())\n", "{ a: { b: 1 } }\n");
    // `push` on null starts a list, the long way as here.
    prints("no-start", "fn f() {\n    let m = {}\n    m = set(m, \"a\", get(m, \"a\") | push(1))\n    m\n}\nprint(f())\n", "{ a: [1] }\n");
    fails("wrong-start", "fn f() {\n    let m = {}\n    m = set(m, \"a\", get(m, \"a\", 0) | push(1))\n    m\n}\nprint(f())\n", "push");
    // What fails leaves things as they were: `??` carries on with the old value.
    prints("kept-after-failing", "fn f() {\n    let g = [[1]]\n    g = (set(g, 0, set(g[0], 5, 9)) ?? g)\n    g\n}\nprint(f())\n", "[[1]]\n");
    fails("outside", "fn f() {\n    let g = [[1]]\n    g = set(g, 0, set(g[0], 5, 9))\n    g\n}\nprint(f())\n", "outside");
    fails("outside-outer", "fn f() {\n    let g = [[1]]\n    g = set(g, 3, set(g[3], 0, 9))\n    g\n}\nprint(f())\n", "outside");
    // From the end, as an index reads.
    prints("negative", "fn f() {\n    let g = [[1, 2], [3, 4]]\n    g = set(g, -1, set(g[-1], -1, 9))\n    g\n}\nprint(f())\n", "[[1, 2], [3, 9]]\n");
    // Keys: a number is the text it is written as, and text may be anything.
    prints("int-key", "fn f() {\n    let m = {}\n    m = set(m, 1, get(m, 1, []) | push(2))\n    m = set(m, 1, get(m, \"1\", []) | push(3))\n    m\n}\nprint(f())\n", "{ \"1\": [2, 3] }\n");
    prints("unicode-key", "fn f() {\n    let m = {}\n    let k = \"日本語\"\n    m = set(m, k, get(m, k, []) | push(1))\n    m = set(m, k, get(m, k, []) | push(2))\n    m[k]\n}\nprint(f())\n", "[1, 2]\n");
    prints("empty-key", "fn f() {\n    let m = {}\n    m = set(m, \"\", get(m, \"\", 0) + 1)\n    m = set(m, \"\", get(m, \"\", 0) + 1)\n    m\n}\nprint(len(f()))\n", "1\n");
    // Keys written differently are different keys, read from one and written to another.
    prints("other-key", "fn f() {\n    let m = { a: [1], b: [2] }\n    let x = \"a\"\n    let y = \"b\"\n    m = set(m, x, get(m, y, []) | push(3))\n    m\n}\nprint(f())\n", "{ a: [2, 3], b: [2] }\n");
    // A field or an expression for a key, the same on both sides.
    prints("field-key", "fn f() {\n    let r = { k: \"a\" }\n    let m = {}\n    m = set(m, r.k, get(m, r.k, []) | push(r))\n    m\n}\nprint(f())\n", "{ a: [{ k: \"a\" }] }\n");
    prints("sum-key", "fn f(i) {\n    let g = [[0], [0]]\n    g = set(g, i + 1, set(g[i + 1], 0, 5))\n    g\n}\nprint(f(0))\n", "[[0], [5]]\n");
    // Work with an effect is done once, in its order.
    prints("effects", "fn say(x) {\n    print(x)\n    x\n}\nfn f() {\n    let m = {}\n    m = set(m, say(\"k\"), get(m, say(\"k\"), []) | push(say(1)))\n    m\n}\nprint(f())\n", "k\nk\n1\n{ k: [1] }\n");
}

#[test]
fn an_update_inside_leaves_a_function_of_that_name_alone() {
    prints("own-push", "fn push(l, x) = [\"mine\"]\nfn f() {\n    let m = {}\n    m = set(m, \"a\", get(m, \"a\", []) | push(1))\n    m\n}\nprint(f())\n", "{ a: [\"mine\"] }\n");
    prints("own-set", "fn set(m, k, v) = \"mine\"\nfn f() {\n    let m = {}\n    m = set(m, \"a\", 1)\n    m\n}\nprint(f())\n", "mine\n");
    prints("own-get", "fn get(m, k, d) = [\"mine\"]\nfn f() {\n    let m = {}\n    m = set(m, \"a\", get(m, \"a\", []) | push(1))\n    m\n}\nprint(f())\n", "{ a: [\"mine\", 1] }\n");
    // A map that holds a function of that name.
    prints("map-push", "fn f() {\n    let m = { a: [], set: (k, v) => \"held\" }\n    m = m.set(\"a\", 1)\n    m\n}\nprint(f())\n", "held\n");
}

#[test]
fn an_update_inside_does_not_copy() {
    quick("grid-big", "fn f(n) {\n    let g = range(0, 1000) | map((y) => range(0, 1000) | map((x) => 0))\n    for i in 0..n {\n        let y = i % 1000\n        let x = (i * 7) % 1000\n        g = set(g, y, set(g[y], x, i))\n    }\n    g[999][993]\n}\nprint(f(1000000))\n", "999999\n", 20);
    quick("lists-big", "fn f(n) {\n    let m = {}\n    for i in 0..n {\n        let k = str(i % 10)\n        m = m | set(k, get(m, k, []) | push(i))\n    }\n    len(m[\"3\"])\n}\nprint(f(1000000))\n", "100000\n", 20);
    // At a script's top level, and from a `reduce`.
    quick("top-big", "let g = [range(0, 100000)]\nfor i in 0..100000 {\n    g = set(g, 0, set(g[0], i, 0))\n}\nprint(sum(g[0]))\n", "0\n", 20);
    quick("reduce-big", "let m = range(0, 300000) | reduce({}, (m, i) => set(m, str(i % 10), get(m, str(i % 10), []) | push(i)))\nprint(len(m[\"3\"]))\n", "30000\n", 40);
}

// ---- text added to ---------------------------------------------------------------------------

#[test]
fn text_added_to_keeps_every_alias() {
    prints("alias", "fn f() {\n    let s = \"a\" + \"b\"\n    let t = s\n    s += \"c\"\n    print(s, t)\n}\nf()\n", "abc ab\n");
    prints("in-a-list", "fn f() {\n    let s = \"a\" + \"b\"\n    let l = [s]\n    s += \"c\"\n    print(s, l)\n}\nf()\n", "abc [\"ab\"]\n");
    // A text written in the program is not changed by a run of it.
    prints("literal", "fn f() {\n    let s = \"a\"\n    s += \"b\"\n    s\n}\nprint(f(), f(), f())\n", "ab ab ab\n");
    prints("default", "fn f(s = \"a\") {\n    s += \"b\"\n    s\n}\nprint(f(), f())\n", "ab ab\n");
    // Given to a function: the caller's stays.
    prints("parameter", "fn f(s) {\n    s += \"!\"\n    s\n}\nlet a = \"x\" + \"y\"\nprint(f(a), a)\n", "xy! xy\n");
    prints("captured", "fn f() {\n    let s = \"a\" + \"\"\n    let read = () => s\n    s += \"b\"\n    print(read(), s)\n}\nf()\n", "ab ab\n");
    // Added to itself.
    prints("itself", "fn f() {\n    let s = \"ab\" + \"c\"\n    s += s\n    print(s)\n}\nf()\n", "abcabc\n");
}

#[test]
fn the_edges_of_text_added_to() {
    prints("text-empty", "fn f() {\n    let s = \"\" + \"\"\n    s += \"\"\n    s += \"a\"\n    s += \"\"\n    print(s, len(s))\n}\nf()\n", "a 1\n");
    prints("unicode", "fn f() {\n    let s = \"\"\n    for w in [\"日\", \"本\", \"語\", \"é\", \"😀\"] {\n        s += w\n    }\n    print(s, len(s), s[1], s[4], s[-1])\n}\nf()\n", "日本語é😀 5 本 😀 😀\n");
    // Read as it grows: past what is kept for a long text, and with letters of all lengths.
    prints("read-as-it-grows", "fn f() {\n    let s = \"\"\n    let c = 0\n    for i in 0..300 {\n        s += i % 3 == 0 ? \"é\" : \"a\"\n        if s[i] == \"é\" {\n            c += 1\n        }\n        if len(s) != i + 1 {\n            print(\"wrong\", i)\n        }\n    }\n    print(c, len(s), s[299], s[298])\n}\nf()\n", "100 300 a a\n");
    prints("plain-then-not", "fn f() {\n    let s = repeat(\"a\", 100)\n    print(len(s), s[99])\n    s += \"é\"\n    print(len(s), s[100])\n    s += \"b\"\n    print(len(s), s[101], s[100])\n}\nf()\n", "100 a\n101 é\n102 b é\n");
    // Not text on the right: as `+` has it. Nor on the left.
    prints("text-number", "fn f() {\n    let s = \"a\" + \"\"\n    s += 1\n    s += 2.5\n    s += true\n    print(s)\n}\nf()\n", "a12.5true\n");
    prints("number-left", "fn f() {\n    let n = 1\n    n += \"a\"\n    print(n)\n}\nf()\n", "1a\n");
    prints("null", "fn f() {\n    let s = \"a\" + \"\"\n    s += null\n    print(s)\n}\nf()\n", "anull\n");
    // The other way of writing it, at the top of a script, and past a block's size.
    prints("text-plus", "fn f() {\n    let s = \"a\" + \"\"\n    s = s + \"b\"\n    s = s + 1\n    print(s)\n}\nf()\n", "ab1\n");
    prints("text-top", "let s = \"a\" + \"\"\ns += \"b\"\nlet t = s\ns += \"c\"\nprint(s, t)\n", "abc ab\n");
    prints("text-large", "fn f() {\n    let s = \"\"\n    for i in 0..3000000 {\n        s += \"é\"\n    }\n    print(len(s), s[2999999], s == repeat(\"é\", 3000000))\n}\nf()\n", "3000000 é true\n");
}

#[test]
fn text_added_to_is_not_quadratic() {
    quick("append", "fn f(n) {\n    let s = \"\"\n    for i in 0..n {\n        s += \"x\"\n    }\n    len(s)\n}\nprint(f(2000000))\n", "2000000\n", 10);
    quick("text-plus-big", "fn f(n) {\n    let s = \"\"\n    for i in 0..n {\n        s = s + \"x\"\n    }\n    len(s)\n}\nprint(f(2000000))\n", "2000000\n", 10);
    quick("while-len", "fn f(n) {\n    let s = \"\"\n    while len(s) < n {\n        s += \"x\"\n    }\n    len(s)\n}\nprint(f(500000))\n", "500000\n", 10);
    quick("while-len-unicode", "fn f(n) {\n    let s = \"é\"\n    while len(s) < n {\n        s += \"é\"\n    }\n    s[n - 1]\n}\nprint(f(300000))\n", "é\n", 10);
}

// ---- a sandboxed script keeps its limits -------------------------------------------------------

#[test]
fn a_script_stops_at_its_limits_however_it_grows() {
    let grow = |start: &str, body: &str| format!("let r = run_script(\"let a = {}\\nfor i in 0..40 {{\\n{}\\n}}\\nlen(a)\")\nprint(is_err(r))\n", start, body);
    for (i, (start, body)) in [("[1]", "a = a + a"), ("[1]", "a += a"), ("'aaaa'", "a = a + a"), ("'aaaa'", "a += a")].iter().enumerate() {
        let o = run(&format!("script-{}", i), &grow(start, body));
        assert_eq!(out(&o), "true\n", "{} {}", body, said(&o));
    }
}

// ---- json.parse -------------------------------------------------------------------------------

/// As `main` read it before JSON was read straight into values: each answer here is what that
/// build printed.
#[test]
fn json_is_read_as_it_was() {
    let cases = [
        ("null", "null"), ("true", "true"), ("0", "0"), ("-0", "-0.0"), ("1.5", "1.5"), ("1e3", "1000.0"),
        ("9223372036854775807", "9223372036854775807"), ("-9223372036854775808", "-9223372036854775808"),
        ("9223372036854775808", "9223372036854776000"), ("18446744073709551615", "18446744073709552000"),
        ("[]", "[]"), ("{}", "{}"), ("[1, [2, [3]]]", "[1, [2, [3]]]"),
    ];
    for (i, (text, want)) in cases.iter().enumerate() {
        prints(&format!("json-{}", i), &format!("print(json.parse({:?}))\n", text), &format!("{}\n", want));
    }
    // Keys in the order written; the last of two the same, in the first one's place.
    prints("json-order", "print(json.parse(\"{\\\"b\\\": 1, \\\"a\\\": 2, \\\"b\\\": 3}\"))\n", "{ b: 3, a: 2 }\n");
    // Escapes, and letters from outside the first plane.
    prints("json-text", "print(json.parse(\"[\\\"a\\\\n\\\\u00e9\\\\ud83d\\\\ude00\\\"]\")[0])\n", "a\né😀\n");
    prints("json-key", "print(keys(json.parse(\"{\\\"\\\\u00e9\\\": 1, \\\"\\\": 2}\")))\n", "[\"é\", \"\"]\n");
    for (i, bad) in ["", "{", "[1,]", "1 2", "{\\\"a\\\"}", "nul"].iter().enumerate() {
        prints(&format!("json-bad-{}", i), &format!("let r = json.parse(\"{}\")\nprint(is_err(r), str(r.error) != \"\")\n", bad), "true true\n");
    }
    // Deeper than it reads is an error, not a crash.
    let deep = "[".repeat(100000);
    prints("json-deep", &format!("print(is_err(json.parse(\"{}\")))\n", deep), "true\n");
}

// ---- range with a step -------------------------------------------------------------------------

#[test]
fn range_takes_a_step() {
    prints("step", "print(range(0, 10, 3), range(10, 0, -3), range(0, 9, 3), range(5, 5, 1), range(0, 5, -1), range(5, 0, 1))\n", "[0, 3, 6, 9] [10, 7, 4, 1] [0, 3, 6] [] [] []\n");
    prints("step-for", "for i in range(0, 10, 4) {\n    print(i)\n}\nfor i in range(3, -1, -2) {\n    print(i)\n}\n", "0\n4\n8\n3\n1\n");
    prints("step-keep", "fn f() = for i in range(0, 6, 2) => i * 2\nprint(f())\n", "[0, 4, 8]\n");
    prints("step-empty-for", "for i in range(0, 5, -1) {\n    print(i)\n}\nprint(\"end\")\n", "end\n");
    fails("step-zero", "print(range(0, 5, 0))\n", "step of 0");
    fails("step-zero-for", "for i in range(0, 5, 0) {\n    print(i)\n}\n", "step of 0");
    fails("step-type", "print(range(0, 5, 1.5))\n", "range");
    fails("step-big", "print(len(range(0, 1000000000, 2)))\n", "too large");
    // At the very end of the ints it stops, and does not wrap round.
    prints("step-end", "for i in range(9223372036854775806, 9223372036854775807, 5) {\n    print(i)\n}\n", "9223372036854775806\n");
    prints("step-end-down", "let n = -9223372036854775807 - 1\nfor i in range(n + 1, n, -5) {\n    print(i)\n}\n", "-9223372036854775807\n");
    // A loop that keeps nothing has no list to be too large.
    prints("step-long", "fn f() {\n    let c = 0\n    for i in range(0, 200000000, 100) {\n        c += 1\n    }\n    c\n}\nprint(f())\n", "2000000\n");
}

// ---- a visitor's script makes no names for good ------------------------------------------------

/// Names are kept for the life of the process (src/sym.rs). A script a visitor writes must not
/// grow that with each script run, or the server's memory would follow the visitors.
#[test]
fn a_scripts_names_are_its_own() {
    use quillo::sym::Sym;
    let mut it = quillo::Interp::new(".");
    it.load_source("let program_has_this = 1\n", "main.qwe").unwrap();
    let run = |it: &mut quillo::Interp, src: &str, vars: Option<&quillo::value::Map>| quillo::script::run_script(it, src, vars).repr();
    for i in 0..300 {
        let src = format!("let speed_fresh_{i} = {i}\nlet speed_other_{i} = speed_fresh_{i} * 2\nspeed_other_{i} + 1");
        assert_eq!(run(&mut it, &src, None), (i * 2 + 1).to_string());
    }
    assert!(Sym::known("speed_fresh_7").is_none() && Sym::known("speed_other_299").is_none());
    // What a message names is as the script wrote it, and the script's own text is its own.
    let said = run(&mut it, "let total_fresh = 1\ntotal_fresh + speed_missing_name", None);
    assert!(said.contains("`speed_missing_name`") && !said.contains("`$"), "{}", said);
    let said = run(&mut it, "let speed_x_fresh = 1\nerr(`costs $1 and $2, ${speed_x_fresh}`)", None);
    assert!(said.contains("costs $1 and $2, 1"), "{}", said);
    // Its variables: those it names, under its names; the rest make none.
    let mut m = quillo::value::Map::new();
    m.insert("speed_given_a".into(), quillo::Value::Int(5));
    m.insert("speed_given_unused".into(), quillo::Value::Int(9));
    m.insert("program_has_this".into(), quillo::Value::Int(7));
    assert_eq!(run(&mut it, "speed_given_a * 2 + program_has_this", Some(&m)), "17");
    assert!(Sym::known("speed_given_unused").is_none() && Sym::known("speed_given_a").is_none());
    // A lambda it makes is not given back: it would run without the script's limits
    // (FEEDBACK51.md, Q-670).
    let f = quillo::script::run_script(&mut it, "(speed_arg) => speed_arg + 1", None);
    assert!(f.repr().contains("a script gives back no function"), "{}", f.repr());
    // The same script twice gives the same, and a builtin is a builtin.
    assert_eq!(run(&mut it, "let speed_n = [3, 1, 2]\nsort(speed_n)", None), "[1, 2, 3]");
    assert_eq!(run(&mut it, "let speed_n = [3, 1, 2]\nsort(speed_n)", None), "[1, 2, 3]");
}

// ---- shortcuts in the interpreter ------------------------------------------------------------

#[test]
fn a_lambda_that_gives_a_constant_or_its_argument_is_still_checked() {
    prints("constant", "print([1, 2, 3] | map((_i) => 7), [] | map((_i) => true), [1] | map((_i) => \"a\"))\n", "[7, 7, 7] [] [\"a\"]\n");
    // `0` counts as true; `null` and `false` do not.
    prints("identity", "print([1, 0, null, 2, false] | filter((x) => x), count([true, false, true], (f) => f))\n", "[1, 0, 2] 2\n");
    // A type on the parameter is checked, as without the shortcut.
    fails("constant-typed", "print([\"a\"] | map((x: int) => 0))\n", "must be int");
    fails("identity-typed", "print([\"a\"] | map((x: int) => x))\n", "must be int");
    // Given to a function of the program's, as to a builtin.
    prints("constant-called", "fn twice(f, x) = [f(x), f(x)]\nprint(twice((_y) => 5, 1), twice((y) => y, \"b\"))\n", "[5, 5] [\"b\", \"b\"]\n");
}

#[test]
fn x_is_read_before_what_could_change_it() {
    // `f()` changes `x`: the `x` read is the one before, as `+` reads left first.
    prints("closure", "fn g() {\n    let x = 1\n    let f = () => {\n        x = 10\n        1\n    }\n    x = x + f()\n    x\n}\nprint(g())\n", "2\n");
    prints("forms", "fn g() {\n    let a = 1\n    a = a + 2\n    a = a - 5\n    let b = 1.5\n    b = b + 1\n    let c = \"x\" + \"\"\n    c = c + 1\n    let d = [1]\n    d = d + [2]\n    print(a, b, c, d)\n}\ng()\n", "-2 2.5 x1 [1, 2]\n");
    fails("forms-overflow", "fn g() {\n    let a = 9223372036854775807\n    a = a + 1\n    a\n}\nprint(g())\n", "overflow");
    fails("forms-sub-list", "fn g() {\n    let d = [1]\n    d = d - [1]\n    d\n}\nprint(g())\n", "cannot");
}

#[test]
fn a_statement_whose_value_is_not_used_makes_no_list() {
    // In an `if`'s branch, and its `else`: no list, so no list's limit.
    prints("if-branch", "fn f(n) {\n    let c = 0\n    if n > 0 {\n        for i in 0..n {\n            c += 1\n        }\n    } else {\n        for i in 0..n {\n            c -= 1\n        }\n    }\n    c\n}\nprint(f(20000000))\n", "20000000\n");
    // Used as a value, it is the list it was.
    prints("if-value", "fn f(c) = if c { for i in 0..3 => i * 2 } else { [] }\nlet r = if true {\n    for i in 0..3 {\n        i\n    }\n} else {\n    []\n}\nprint(f(true), f(false), r)\n", "[0, 2, 4] [] [0, 1, 2]\n");
    // A program run from its file: its last statement's value is no one's, a long loop too.
    prints("last-loop", "let t = 0\nfor i in 0..20000000 {\n    t += 1\n}\n", "");
    prints("last-loop-printed", "for i in 0..3 {\n    print(i)\n}\n", "0\n1\n2\n");
    // Loaded from text, the program's value is its last statement's.
    let mut it = quillo::Interp::new(".");
    let v = it.load_source("let a = 1\nfor i in 0..3 {\n    i * a\n}\n", "main.qwe").unwrap();
    assert_eq!(v.repr(), "[0, 1, 2]");
}

#[test]
fn a_key_made_by_a_builtin_that_only_computes_is_worked_out_once() {
    prints("pure-key", "fn f() {\n    let m = {}\n    for i in 0..1000 {\n        m = set(m, str(i % 3), get(m, str(i % 3), []) | push(i))\n    }\n    [len(m[\"0\"]), len(m[\"1\"]), len(m[\"2\"]), m[\"2\"][0]]\n}\nprint(f())\n", "[334, 333, 333, 2]\n");
    prints("pure-key-nested", "fn f() {\n    let m = {}\n    for w in [\"A\", \"b\", \"a\", \" B\"] {\n        m = set(m, lower(trim(w)), get(m, lower(trim(w)), 0) + 1)\n    }\n    m\n}\nprint(f())\n", "{ a: 2, b: 2 }\n");
    // The program's own `str` is called as written, twice.
    prints("own-str", "let calls = []\nfn str(x) {\n    calls = calls | push(x)\n    \"k\"\n}\nfn f() {\n    let m = {}\n    for i in 0..3 {\n        m = set(m, str(i), get(m, str(i), []) | push(i))\n    }\n    m\n}\nprint(f(), len(calls))\n", "{ k: [0, 1, 2] } 6\n");
    // A key that calls something with an effect is worked out as written.
    prints("effect-key", "fn key(i) {\n    print(\"key\", i)\n    \"k\"\n}\nfn f() {\n    let m = {}\n    m = set(m, key(1), get(m, key(1), []) | push(1))\n    m\n}\nprint(f())\n", "key 1\nkey 1\n{ k: [1] }\n");
    // Long, so a copy each round would not finish in time.
    quick("pure-key-big", "let m = range(0, 600000) | reduce({}, (m, i) => set(m, str(i % 10), get(m, str(i % 10), []) | push(i)))\nprint(len(m[\"3\"]))\n", "60000\n", 20);
}
