//! The corners of compiled code (src/jitloop.rs, src/jit.rs) no other test reaches: each
//! program here runs in this process, where its long loops and numeric functions go to
//! machine code, and in `quillo run` with `QUILLO_JIT=0`, all in the interpreter. Both must
//! give the same, and the answer known.

mod support;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// `r`'s value as `print` shows it, or the error, with loops compiled here; and how many
/// loops were compiled.
fn compiled(src: &str) -> (String, usize) {
    let before = quillo::jitloop::compiled();
    let mut it = quillo::Interp::new(".");
    let out = match it.load_source(src, "main.qwe") {
        Ok(_) => quillo::value::lookup(&it.globals, "r").map_or("no r".to_string(), |v| v.display()),
        Err(e) => format!("line {}: {}", e.line, e.msg),
    };
    (out, quillo::jitloop::compiled() - before)
}

/// The same program in `quillo run`, nothing compiled: what it printed for `r`, or its error.
fn interpreted(name: &str, src: &str) -> String {
    let d = support::temp_dir(&format!("cover-jit-{}", name));
    std::fs::write(d.join("main.qwe"), format!("{}\nprint(r)\n", src)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .arg("--no-check")
        .env("QUILLO_JIT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{}: `quillo run` took more than a minute", name);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let o = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&d);
    if o.status.success() {
        String::from_utf8_lossy(&o.stdout).trim_end().to_string()
    } else {
        format!("FAILED {}", String::from_utf8_lossy(&o.stderr).trim_end())
    }
}

/// Both ways, `r` is `want`; how many loops were compiled.
fn same(name: &str, src: &str, want: &str) -> usize {
    let (got, n) = compiled(src);
    assert_eq!(got, interpreted(name, src), "{}: compiled, then interpreted\n{}", name, src);
    assert_eq!(got, want, "{}", name);
    n
}

/// Both ways it fails at `line`, saying `why`.
fn fails(name: &str, src: &str, line: u32, why: &str) {
    let (got, _) = compiled(src);
    assert_eq!(got, format!("line {}: {}", line, why), "{}: compiled\n{}", name, src);
    let plain = interpreted(name, src);
    assert!(plain.starts_with("FAILED") && plain.contains(why) && plain.contains(&format!("{}", line)), "{}: interpreted: {}", name, plain);
}

#[test]
fn a_loop_calling_len_where_len_is_the_program_s_own_is_left_to_the_interpreter() {
    let src = "fn len(x) = 7\nlet xs = [1, 2, 3]\nlet t = 0\nfor i in 0..1000 {\n    t += len(xs)\n}\nlet r = t\n";
    assert_eq!(same("own-len", src, "7000"), 0);
}

#[test]
fn bools_from_around_a_loop_floats_and_nested_counts_are_compiled() {
    let src = "let on = true
let acc = 0.0
let c = 0
for _ in 0..1000 {
    c += 1
}
for i in 0..1000 {
    let x = i / 4.0
    let y = -x - 0.5
    if on && x <= 100.0 {
        acc = acc + y
    } else {
        acc = acc - 1.0
    }
    if x >= 200.0 {
        acc = acc + 0.25
    }
    for j in range(3) {
        c += j
    }
    for _ in 0..2 {
        c += 1
    }
    let w = if on {
        let a = i % 3
        a * 2
    } else {
        0
    }
    c += w
    i + 1
}
let r = [acc, c, on]
";
    assert!(same("floats", src, "[-20799.5, 7998, true]") >= 2);
}

#[test]
fn the_length_of_a_row_is_read_in_machine_code_and_a_row_that_is_not_a_list_gives_the_round_back() {
    let src = "let g = [[1, 2, 3], [4, 5], \"abcd\"]
let t = 0
for i in 0..1000 {
    t += len(g[i % 2])
    if i == 700 {
        t += len(g[2])
    }
}
let u = 0
for i in 0..1000 {
    u += g[i % 2][0]
    if i == 800 {
        let z = g[2][1]
        u += 100
    }
}
let r = [t, u]
";
    assert!(same("rows", src, "[2504, 2600]") >= 2);
}

#[test]
fn a_row_set_where_there_is_none_is_set_as_the_interpreter_sets_it() {
    // `g[2]` is null: `set` makes it a map, the long way.
    let src = "let g = [[1, 2], [3, 4], null]
for i in 0..1000 {
    if i == 500 {
        g = set(g, 2, set(g[2], 0, 7))
    } else {
        g = set(g, 0, set(g[0], 1, i))
    }
}
let r = g
";
    let (got, n) = compiled(src);
    assert!(n >= 1, "not compiled");
    assert_eq!(got, interpreted("null-row", src));
    assert!(got.starts_with("[[1, 999], [3, 4], {") && got.contains('7'), "{}", got);
}

#[test]
fn a_row_set_that_fails_fails_with_the_interpreter_s_error() {
    // A row that is a number: the inner `set` fails.
    let number = "let g = [[1, 2], [3, 4], 5]
for i in 0..1000 {
    if i == 500 {
        g = set(g, 2, set(g[2], 0, 7))
    } else {
        g = set(g, 0, set(g[0], 1, i))
    }
}
let r = g
";
    let (a, _) = compiled(number);
    let b = interpreted("number-row", number);
    assert!(a.starts_with("line 4: ") && a.contains("set"), "{}", a);
    assert!(b.contains(&a["line 4: ".len()..]), "{}\n{}", a, b);
    // Past the end of a row.
    fails(
        "past-row",
        "let g = [[1, 2], [3, 4]]\nfor i in 0..1000 {\n    if i == 500 {\n        g = set(g, 1, set(g[1], 5, 7))\n    } else {\n        g = set(g, 0, set(g[0], 1, i))\n    }\n}\nlet r = g\n",
        4,
        "`set`: index 5 is outside a list of 2",
    );
    // Past the last row.
    fails(
        "past-grid",
        "let g = [[1, 2], [3, 4]]\nfor i in 0..1000 {\n    if i == 500 {\n        g = set(g, 7, set(g[7], 0, 1))\n    } else {\n        g = set(g, 0, set(g[0], 1, i))\n    }\n}\nlet r = g\n",
        4,
        "`set`: index 7 is outside a list of 2",
    );
    // Past the end of a list.
    fails(
        "past-list",
        "let xs = [1, 2, 3]\nfor i in 0..1000 {\n    if i == 500 {\n        xs = set(xs, 10, 1)\n    } else {\n        xs = set(xs, 0, i)\n    }\n}\nlet r = xs\n",
        4,
        "`set`: index 10 is outside a list of 3",
    );
}

/// Loops with something compiled code does not do, each in a branch no round takes: each
/// is left to the interpreter, and gives what it gives.
#[test]
fn loops_with_what_compiled_code_does_not_do_run_in_the_interpreter() {
    let odd = [
        "t += n[0]",
        "let z = xs",
        "let z = -on",
        "let z = !n",
        "let z = n && on",
        "let z = on && n",
        "let z = if n { 1 } else { 2 }",
        "let z = if on { 1 } else { 2.0 }",
        "let z = if on {\n            let a = 1\n        } else {\n            0\n        }",
        "t += xs[1.5]",
        "t += g[0]",
        "t += xs[0][0]",
        "t += e[0]",
        "let z = if on { 1 }",
        "let z = if on {\n        } else {\n            0\n        }",
        "t += 1.5",
        "t = 1.5",
        "xs = xs | set(1.5, 2)",
        "let h = 0.5\n        g = set(g, h, set(g[h], 0, 1))",
        "if n {\n            t += 1\n        }",
        "while n {\n            n -= 1\n        }",
        "for [a, b] in 0..0 {\n            t += 1\n        }",
        "ys = xs | push(1)",
        "g = set(g, i % 2, set(g[0], 0, i))",
        "t = abs(t)",
        "for j in range(1, 2, 3, 4) {\n            t += 1\n        }",
        "for x in xs {\n            t += 1\n        }",
    ];
    let mut src = String::from("let n = 3\nlet on = true\nlet xs = [1, 2, 3]\nlet ys = [1]\nlet g = [[1], [2]]\nlet e = []\nlet t = 0\n");
    for o in odd {
        src.push_str(&format!("for i in 0..1000 {{\n    t += 1\n    if i < 0 {{\n        {}\n    }}\n}}\n", o));
    }
    // Last, one that compiles, so each before it was looked at and given up.
    src.push_str("for i in 0..1000 {\n    t += 1\n}\nlet r = [t, n, len(xs), len(g)]\n");
    assert_eq!(same("odd", &src, &format!("[{}, 3, 3, 2]", (odd.len() + 1) * 1000)), 1, "only the last is compiled");
}

#[test]
fn a_while_whose_condition_is_a_number_runs_in_the_interpreter_to_its_error() {
    fails("number-while", "let k = 0\nwhile k + 1 {\n    k += 1\n    let z = 5 % (1000 - k)\n}\nlet r = k\n", 4, "division by zero");
}

#[test]
fn a_loop_with_a_return_in_it_returns_from_its_function() {
    let src = "fn f() {\n    let xs = [1, 2]\n    for i in 0..1000 {\n        if i == 900 {\n            return i + xs[1]\n        }\n    }\n    0\n}\nlet r = f()\n";
    assert_eq!(same("return", src, "902"), 0);
}

#[test]
fn a_function_calling_one_compiled_before_it_gives_the_same() {
    let src = "fn twice(x) = x * 2\nfn more(x) = twice(x) + 1\nlet a = twice(3) + twice(4)\nlet b = more(5) + more(6) + more(7)\nlet r = [a, b]\n";
    let before = quillo::jit::compiled();
    same("calls-compiled", src, "[14, 39]");
    assert!(quillo::jit::compiled() > before, "twice is compiled");
}

#[test]
fn a_function_of_more_than_sixteen_numbers_is_interpreted() {
    let params: Vec<String> = (0..17).map(|i| format!("p{}", i)).collect();
    let args: Vec<String> = (1..=17).map(|i| i.to_string()).collect();
    let src = format!("fn s({}) = {}\nlet r = s({}) + s({}) + s({})\n", params.join(", "), params.join(" + "), args.join(", "), args.join(", "), args.join(", "));
    same("seventeen", &src, "459");
}

#[test]
fn a_function_called_with_arguments_that_may_be_left_out_is_compiled_when_all_are_given() {
    let mut it = quillo::Interp::new(".");
    it.load_source("fn sq(x) = x * x + 1\n", "main.qwe").unwrap();
    let Some(quillo::value::Value::Func(c)) = quillo::value::lookup(&it.globals, "sq") else { panic!("no sq") };
    let before = quillo::jit::compiled();
    for (x, want) in [(7, "50"), (-3, "10"), (0, "1")] {
        match it.call_closure(&c, vec![Some(quillo::value::Value::Int(x))], 0) {
            Ok(v) => assert_eq!(v.repr(), want),
            Err(_) => panic!("sq({}) failed", x),
        }
    }
    assert!(quillo::jit::compiled() > before, "not compiled");
    // One left out, with no default: the interpreter's error.
    match it.call_closure(&c, vec![None], 3) {
        Err(quillo::interp::Ctrl::Error(e)) => assert!(e.msg.contains("missing argument `x`"), "{}", e.msg),
        Ok(v) => panic!("gave {}", v.repr()),
        Err(_) => panic!("not an error"),
    }
}
