//! Loops in machine code (src/jitloop.rs; FEEDBACK29.md; branch feedback29-fixes): each program
//! here runs twice, compiled and with `QUILLO_JIT=0`, and must print, fail and end the same. Each
//! loop goes past the rounds a loop runs before it is compiled, so the machine code is what runs;
//! `compiled_loops` checks that it is.

mod support;

use std::path::PathBuf;
use std::process::{Command, Output};

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("loops-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

fn run(d: &std::path::Path, jit: bool) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .arg("--no-check")
        .env("QUILLO_JIT", if jit { "1" } else { "0" })
        .output()
        .unwrap()
}

/// What it printed and said, and how it ended: the same both ways, and `want` printed.
fn same(name: &str, src: &str, want: &str) {
    let d = dir(name, src);
    let (a, b) = (run(&d, true), run(&d, false));
    let shown = |o: &Output| (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned(), o.status.code());
    assert_eq!(shown(&a), shown(&b), "{}: compiled, then interpreted", name);
    assert_eq!(shown(&a).0, want, "{}: {}", name, shown(&a).1);
}

/// It fails, the same both ways, saying `why`.
fn fails(name: &str, src: &str, printed: &str, why: &str) {
    let d = dir(name, src);
    let (a, b) = (run(&d, true), run(&d, false));
    let shown = |o: &Output| (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned(), o.status.code());
    assert_eq!(shown(&a), shown(&b), "{}: compiled, then interpreted", name);
    assert_eq!(a.status.code(), Some(1), "{}", name);
    assert_eq!(shown(&a).0, printed, "{}", name);
    assert!(shown(&a).1.contains(why), "{}: {}", name, shown(&a).1);
}

#[test]
fn a_loop_over_a_list_gives_what_the_interpreter_gives() {
    same("read", "fn f(n) {\n    let xs = range(0, 1000) | map((i) => i * 3)\n    let t = 0\n    for i in range(0, n) {\n        t += xs[i % 1000]\n    }\n    t\n}\nprint(f(100000))\n", "149850000\n");
    // From the end, as an index reads.
    same("negative", "fn f() {\n    let xs = range(0, 10) | map((i) => i)\n    let t = 0\n    for i in 0..5000 {\n        t += xs[-1 - i % 10]\n    }\n    t\n}\nprint(f())\n", "22500\n");
    // Floats and bools, `&&`, `||`, `!`.
    same("floats", "fn f() {\n    let acc = 0.0\n    let on = range(0, 100) | map((i) => i % 3 == 0)\n    for i in 0..10000 {\n        let x = i * 0.5\n        if x > 100.0 && !(x > 4000.0) || on[i % 100] {\n            acc = acc + x\n        }\n    }\n    acc\n}\nprint(f())\n", "19051263.0\n");
    // A count down, by a step, and to the last int.
    same("step", "fn f() {\n    let xs = range(0, 10) | map((i) => i)\n    let t = 0\n    for i in range(3000, 0, -3) {\n        t += xs[i % 10]\n    }\n    let c = 0\n    for i in range(9223372036854775000, 9223372036854775807) {\n        c += 1\n    }\n    print(t, c)\n}\nf()\n", "4500 807\n");
    // `while` with `len`, growing.
    same("while-len", "fn f() {\n    let xs = []\n    while len(xs) < 5000 {\n        xs = xs | push(len(xs) * 2)\n    }\n    print(len(xs), xs[4999], sum(xs))\n}\nf()\n", "5000 9998 24995000\n");
}

#[test]
fn a_list_written_in_a_loop_is_the_interpreters() {
    let sieve = |form: &str| format!("fn sieve(n) {{\n    let flags = range(0, n + 1) | map((_i) => true)\n    let i = 2\n    while i * i <= n {{\n        if flags[i] {{\n{}\n        }}\n        i += 1\n    }}\n    count(flags, (f) => f) - 2\n}}\nprint(sieve(200000))\n", form);
    same("sieve-for", &sieve("            for j in range(i * i, n + 1, i) {\n                flags = flags | set(j, false)\n            }"), "17984\n");
    same("sieve-while", &sieve("            let j = i * i\n            while j <= n {\n                flags = set(flags, j, false)\n                j += i\n            }"), "17984\n");
    same("sieve-method", &sieve("            for j in range(i * i, n + 1, i) {\n                flags = flags.set(j, false)\n            }"), "17984\n");
    // A grid: a row read, changed and put back, in all three forms.
    for (i, form) in ["g = set(g, y, set(g[y], x, g[y][x] + i))", "g = g | set(y, g[y] | set(x, g[y][x] + i))", "g = g.set(y, g[y].set(x, g[y][x] + i))"].iter().enumerate() {
        same(&format!("grid-{}", i), &format!("fn f() {{\n    let g = range(0, 100) | map((y) => range(0, 100) | map((x) => 0))\n    for i in 0..50000 {{\n        let y = i % 100\n        let x = (i * 7) % 100\n        {}\n    }}\n    let t = 0\n    for y in 0..100 {{\n        for x in 0..len(g[y]) {{\n            t += g[y][x]\n        }}\n    }}\n    print(t, len(g), len(g[5]))\n}}\nf()\n", form), "1249975000 100 100\n");
    }
    // Pushes, in a function and at a script's top.
    same("push-fn", "fn f() {\n    let xs = []\n    for i in 0..100000 {\n        xs = xs | push(i * 2)\n    }\n    print(len(xs), xs[99999], sum(xs))\n}\nf()\n", "100000 199998 9999900000\n");
    same("push-top", "let xs = []\nfor i in 0..100000 {\n    xs = push(xs, i)\n}\nlet total = 0\nfor i in 0..len(xs) {\n    total += xs[i]\n}\nprint(len(xs), total)\n", "100000 4999950000\n");
}

#[test]
fn what_else_holds_a_list_keeps_what_it_had() {
    same("alias", "fn f() {\n    let xs = range(0, 1000) | map((i) => 0)\n    let ys = xs\n    for i in 0..1000 {\n        xs = set(xs, i, i)\n    }\n    print(sum(xs), sum(ys))\n}\nf()\n", "499500 0\n");
    same("alias-row", "fn f() {\n    let g = [range(0, 1000) | map((i) => 0)]\n    let row = g[0]\n    for i in 0..1000 {\n        g = set(g, 0, set(g[0], i, 1))\n    }\n    print(sum(g[0]), sum(row))\n}\nf()\n", "1000 0\n");
    // A list read and another written: the read one is as it was.
    same("two-lists", "fn f() {\n    let a = range(0, 500) | map((i) => i)\n    let b = range(0, 500) | map((i) => 0)\n    for i in 0..500 {\n        b = set(b, i, a[499 - i])\n    }\n    print(a[0], b[0], sum(b))\n}\nf()\n", "0 499 124750\n");
}

#[test]
fn an_error_in_a_compiled_loop_is_the_interpreters() {
    // After a round's write, as before it.
    fails("overflow", "fn f() {\n    let xs = range(0, 10) | map((i) => 0)\n    let t = 9223372036854775000\n    for i in 0..100000 {\n        xs = set(xs, i % 10, i)\n        t += i\n    }\n    t\n}\nprint(\"start\")\nprint(f())\n", "start\n", "integer overflow");
    fails("overflow-mul", "fn f() {\n    let t = 1\n    let xs = [0]\n    for i in 0..100000 {\n        xs = set(xs, 0, i)\n        t = t * 3\n    }\n    t\n}\nprint(f())\n", "", "integer overflow");
    fails("set-past", "fn f() {\n    let xs = range(0, 500) | map((i) => 0)\n    for i in 0..1000 {\n        xs = set(xs, i, 1)\n    }\n    xs\n}\nprint(f())\n", "", "`set`: index 500 is outside a list of 500");
    fails("mod-zero", "fn f() {\n    let xs = [0]\n    let t = 0\n    for i in 0..1000 {\n        xs = set(xs, 0, i)\n        t += 1000 % (700 - i)\n    }\n    t\n}\nprint(f())\n", "", "division by zero");
    fails("nan", "fn f(z) {\n    let xs = [0]\n    let c = 0\n    for i in 0..1000 {\n        xs = set(xs, 0, i)\n        if i > 500 && i * 1.0 < z {\n            c += 1\n        }\n    }\n    c\n}\nprint(f(0.0 / 0.0))\n", "", "cannot compare float with float");
    fails("neg", "fn f() {\n    let t = 0\n    let xs = [0]\n    for i in 0..1000 {\n        xs = set(xs, 0, i)\n        t = i == 900 ? -9223372036854775807 - 1 : t\n        t = -t\n    }\n    t\n}\nprint(f())\n", "", "integer overflow");
    fails("step-zero", "fn f() {\n    let xs = [0]\n    for i in 0..1000 {\n        let k = 600 - i\n        for j in range(0, 3, k > 0 ? 1 : 0) {\n            xs = set(xs, 0, j)\n        }\n    }\n    xs\n}\nprint(f())\n", "", "step of 0");
    fails("row-past", "fn f() {\n    let g = range(0, 10) | map((y) => range(0, 10) | map((x) => 0))\n    for i in 0..2000 {\n        g = set(g, i % 10, set(g[i % 10], (i - i % 100) / 100, 1))\n    }\n    g\n}\nprint(f())\n", "", "`set`: index 10 is outside a list of 10");
    // The smallest int's remainder by -1 is 0, as in the interpreter.
    same("min-mod", "fn f(m) {\n    let t = 0\n    let xs = [0]\n    for i in 0..1000 {\n        xs = set(xs, 0, i)\n        t += m % -1\n    }\n    t\n}\nprint(f(-9223372036854775807 - 1))\n", "0\n");
}

#[test]
fn a_round_the_code_cannot_take_goes_back_to_the_interpreter() {
    // An item of another type: text, then numbers again.
    same("text-item", "fn f() {\n    let xs = range(0, 1000) | map((i) => i)\n    xs = set(xs, 600, \"a\")\n    let t = 0\n    let n = 0\n    for i in 0..1000 {\n        n += 1\n        t += xs[i]\n    }\n    print(t, n)\n}\nf()\n", "179700a601602603604605606607608609610611612613614615616617618619620621622623624625626627628629630631632633634635636637638639640641642643644645646647648649650651652653654655656657658659660661662663664665666667668669670671672673674675676677678679680681682683684685686687688689690691692693694695696697698699700701702703704705706707708709710711712713714715716717718719720721722723724725726727728729730731732733734735736737738739740741742743744745746747748749750751752753754755756757758759760761762763764765766767768769770771772773774775776777778779780781782783784785786787788789790791792793794795796797798799800801802803804805806807808809810811812813814815816817818819820821822823824825826827828829830831832833834835836837838839840841842843844845846847848849850851852853854855856857858859860861862863864865866867868869870871872873874875876877878879880881882883884885886887888889890891892893894895896897898899900901902903904905906907908909910911912913914915916917918919920921922923924925926927928929930931932933934935936937938939940941942943944945946947948949950951952953954955956957958959960961962963964965966967968969970971972973974975976977978979980981982983984985986987988989990991992993994995996997998999 1000\n");
    // Past the end: null, as the interpreter reads it.
    same("past-end", "fn f() {\n    let xs = range(0, 1000) | map((i) => i % 7)\n    let c = 0\n    let nulls = 0\n    for i in 0..2000 {\n        let v = xs[i]\n        if v == null {\n            nulls += 1\n        } else if v == 5 {\n            c += 1\n        }\n    }\n    print(c, nulls)\n}\nf()\n", "143 1000\n");
    // A float where ints were: the round's numbers as it began, and on from there.
    same("float-item", "fn f() {\n    let xs = range(0, 1000) | map((i) => i)\n    xs = set(xs, 700, 0.5)\n    let t = 0\n    let k = 0\n    for i in 0..1000 {\n        k += 2\n        t = t + xs[i]\n    }\n    print(t, k)\n}\nf()\n", "498800.5 2000\n");
    // Inside an inner loop, before any write.
    same("inner-miss", "fn f() {\n    let g = range(0, 30) | map((y) => range(0, 30) | map((x) => x))\n    g = set(g, 20, set(g[20], 3, \"x\"))\n    let t = 0\n    for y in 0..30 {\n        for x in 0..30 {\n            t = t + g[y][x]\n        }\n    }\n    print(t)\n}\nf()\n", "8703x4567891011121314151617181920212223242526272829012345678910111213141516171819202122232425262728290123456789101112131415161718192021222324252627282901234567891011121314151617181920212223242526272829012345678910111213141516171819202122232425262728290123456789101112131415161718192021222324252627282901234567891011121314151617181920212223242526272829012345678910111213141516171819202122232425262728290123456789101112131415161718192021222324252627282901234567891011121314151617181920212223242526272829\n");
    // A read after a write in the same round cannot be given back: run as it is.
    same("read-after-write", "fn f() {\n    let xs = range(0, 1000) | map((i) => 0)\n    let t = 0\n    for i in 0..999 {\n        xs = set(xs, i, 1)\n        t += xs[i + 1]\n    }\n    print(t, sum(xs))\n}\nf()\n", "0 999\n");
    // A type that changes is not compiled.
    same("retype", "fn f() {\n    let t = 0\n    let xs = [1]\n    for i in 0..1000 {\n        t = t + 0.5 * xs[0]\n    }\n    t\n}\nprint(f())\n", "500.0\n");
}

#[test]
fn rows_of_other_kinds_are_changed_as_the_interpreter_changes_them() {
    // A row that is a map takes the key; one that is null becomes a map.
    same("map-rows", "fn f() {\n    let g = range(0, 10) | map((i) => {})\n    for i in 0..1000 {\n        g = set(g, i % 10, set(g[i % 10], i % 3, i))\n    }\n    print(g[4])\n}\nf()\n", "{ \"1\": 994, \"2\": 974, \"0\": 984 }\n");
    same("null-rows", "fn f() {\n    let g = range(0, 10) | map((i) => null)\n    for i in 0..1000 {\n        g = set(g, i % 10, set(g[i % 10], 0, i))\n    }\n    print(g[3])\n}\nf()\n", "{ \"0\": 993 }\n");
}

#[test]
fn names_keep_their_scopes() {
    same("shadow", "fn f() {\n    let i = 99\n    let x = 5\n    let t = 0\n    let xs = range(0, 10) | map((k) => k)\n    for i in 0..1000 {\n        let x = xs[i % 10]\n        t += x\n    }\n    print(i, x, t)\n}\nf()\n", "99 5 4500\n");
    // The same loop for ints, then floats: each its own code.
    same("retyped-entry", "fn total(xs) {\n    let t = xs[0] - xs[0]\n    for i in 0..len(xs) {\n        t = t + xs[i]\n    }\n    t\n}\nprint(total(range(0, 1000)), total(range(0, 1000) | map((i) => i * 0.5)), total(range(0, 1000)))\n", "499500 249750.0 499500\n");
    // At a program's top: globals, while it loads.
    same("globals", "let total = 0\nlet xs = range(0, 10) | map((i) => i)\nlet ys = []\nfor i in 0..100000 {\n    total += xs[i % 10]\n    ys = ys | push(total)\n}\nprint(total, len(ys), ys[99999])\n", "450000 100000 450000\n");
}

#[test]
fn a_server_loop_on_a_global_stays_followed() {
    let src = "let counter = 0\nlet xs = range(0, 10) | map((i) => i)\nroute GET /grow {\n    for i in 0..1000 {\n        counter += xs[i % 10]\n    }\n    str(counter)\n}\nroute GET /local {\n    let t = 0\n    for i in 0..1000 {\n        t += xs[i % 10]\n    }\n    str(t)\n}\n";
    let d = dir("served", src);
    let s = support::serve(&d.join("main.qwe"), &[]);
    assert_eq!(support::send(s.port, "GET", "/grow", &[], "").body, "4500");
    assert_eq!(support::send(s.port, "GET", "/grow", &[], "").body, "9000");
    assert_eq!(support::send(s.port, "GET", "/local", &[], "").body, "4500");
}

/// The code really runs: each of these loops is compiled, on this thread.
#[test]
fn compiled_loops() {
    let progs = [
        "fn f() {\n    let xs = range(0, 100) | map((i) => i)\n    let t = 0\n    for i in 0..1000 {\n        t += xs[i % 100]\n    }\n    t\n}\nlet r = f()\n",
        "let xs = []\nfor i in 0..1000 {\n    xs = xs | push(i)\n}\nlet n = len(xs)\n",
        "fn f() {\n    let g = range(0, 10) | map((y) => range(0, 10) | map((x) => 0))\n    for i in 0..1000 {\n        g = set(g, i % 10, set(g[i % 10], i % 7, i))\n    }\n    g\n}\nlet r = f()\n",
        "fn f() {\n    let flags = range(0, 2001) | map((_i) => true)\n    let i = 2\n    while i * i <= 2000 {\n        if flags[i] {\n            for j in range(i * i, 2001, i) {\n                flags = flags | set(j, false)\n            }\n        }\n        i += 1\n    }\n    flags\n}\nlet r = f()\n",
    ];
    for src in progs {
        let before = quillo::jitloop::compiled();
        let mut it = quillo::Interp::new(".");
        it.load_source(src, "main.qwe").unwrap();
        assert!(quillo::jitloop::compiled() > before, "not compiled:\n{}", src);
    }
    // Under a script's budget, none is.
    let mut it = quillo::Interp::new(".");
    it.load_source("let r = 0\n", "main.qwe").unwrap();
    let before = quillo::jitloop::compiled();
    let r = quillo::script::run_script(&mut it, "let t = 0\nfor i in 0..600 {\n    t += 1\n}\nt", None);
    assert_eq!(r.repr(), "600");
    assert_eq!(quillo::jitloop::compiled(), before);
}

#[test]
fn a_loop_that_cannot_be_compiled_runs_as_before() {
    same("calls", "fn g(x) = x * 2\nfn f() {\n    let t = 0\n    let s = \"\"\n    for i in 0..1000 {\n        t += g(i)\n        if i % 100 == 0 {\n            s += \"x\"\n        }\n    }\n    print(t, s)\n}\nf()\n", "999000 xxxxxxxxxx\n");
}
