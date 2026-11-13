//! FEEDBACK52.md, Q-677 (branch feedback52): a function with `return`, at its end or early, is
//! compiled to machine code as one ending in its value is, and so is every function calling it.
//! Each program runs here, where numeric functions are compiled, and in `quillo run` with
//! `QUILLO_JIT=0`, all in the interpreter: both give the same values of the same types, and the
//! functions that must not be compiled (a `return` of no value or of a text, an int on one path
//! and a float on another) are not.

mod support;

use quillo::Interp;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// A value as these tests compare it: its type, then its text (`int 1` is not `float 1.0`).
const SHOW: &str = "fn show(v) = type_of(v) + \" \" + str(v)\n";

/// The quillo run here: this build's, or the one QUILLO_TEST_BIN names (main's, say, to see a
/// test fail there).
fn quillo() -> std::path::PathBuf {
    std::env::var_os("QUILLO_TEST_BIN").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_BIN_EXE_quillo")))
}

/// Each call's value as `show` gives it, or its error, each made three times in this process, all
/// three the same (the first interpreted unless its function loops, the others compiled where
/// they can be); and how many functions were compiled for them.
fn here(defs: &str, calls: &[&str]) -> (Vec<String>, usize) {
    let mut src = format!("{}\n{}", defs, SHOW);
    for (i, c) in calls.iter().enumerate() {
        src.push_str(&format!("fn __c{}() = show({})\n", i, c));
    }
    let before = quillo::jit::compiled();
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    let got = (0..calls.len())
        .map(|i| {
            let f = quillo::value::lookup(&it.globals, &format!("__c{}", i)).unwrap();
            let once = |it: &mut Interp| match it.call(&f, vec![], 0) {
                Ok(v) => v.display(),
                Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR {}: {}", e.line, e.msg),
                Err(_) => "returned".into(),
            };
            let first = once(&mut it);
            for k in 2..=3 {
                assert_eq!(once(&mut it), first, "{} (call {})\n{}", calls[i], k, defs);
            }
            first
        })
        .collect();
    (got, quillo::jit::compiled() - before)
}

/// `src` in `quillo run`, with `env`, stopped after a minute: what it printed, or its error.
fn quillo_run(src: &str, env: &[(&str, &str)]) -> String {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!("quillo-feedback52-jit-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let mut cmd = Command::new(quillo());
    cmd.arg("run").arg(d.join("main.qwe")).arg("--no-check").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&d);
            panic!("`quillo run` took more than a minute:\n{}", src);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let o = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&d);
    // Errors say where: each run's folder is the same name here.
    let name = d.file_name().unwrap().to_string_lossy().to_string();
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim_end().replace(&name, "D");
    let (out, err) = (text(&o.stdout), text(&o.stderr));
    if o.status.success() && err.is_empty() {
        out
    } else {
        format!("{}\nFAILED {}", out, err)
    }
}

/// The calls in `quillo run` with `QUILLO_JIT=0`, nothing compiled: one line each.
fn interpreted(defs: &str, calls: &[&str]) -> Vec<String> {
    let mut src = format!("{}\n{}", defs, SHOW);
    for c in calls {
        src.push_str(&format!("print(show({}))\n", c));
    }
    quillo_run(&src, &[("QUILLO_JIT", "0")]).lines().map(str::to_string).collect()
}

/// `calls` give their answers here and interpreted, and `compiled` functions were compiled for
/// them here (one for each function and the types of its arguments, with what it calls).
fn check(defs: &str, calls: &[(&str, &str)], compiled: usize) {
    let names: Vec<&str> = calls.iter().map(|(c, _)| *c).collect();
    let (got, n) = here(defs, &names);
    let plain = interpreted(defs, &names);
    for (i, (c, want)) in calls.iter().enumerate() {
        assert_eq!(plain.get(i).map(String::as_str), Some(*want), "interpreted: {}\n{}\n{:?}", c, defs, plain);
        assert_eq!(got[i], *want, "compiled: {}\n{}", c, defs);
    }
    #[cfg(feature = "jit")]
    assert_eq!(n, compiled, "functions compiled\n{}", defs);
    #[cfg(not(feature = "jit"))]
    let _ = (n, compiled);
}

#[test]
fn a_function_ending_in_return_is_compiled_as_one_ending_in_its_value() {
    let defs = "fn ret(x) {
    return x % 7 + 1
}
fn early(x) {
    if x % 2 == 0 {
        return 1
    }
    return x % 7 + 1
}
fn early_bare(x) {
    if x % 2 == 0 {
        return 1
    }
    x % 7 + 1
}
fn implicit(x) {
    x % 7 + 1
}";
    let calls = [
        ("ret(3)", "int 4"),
        ("ret(0)", "int 1"),
        ("ret(-8)", "int 0"),
        ("early(4)", "int 1"),
        ("early(3)", "int 4"),
        ("early(0)", "int 1"),
        ("early_bare(4)", "int 1"),
        ("early_bare(3)", "int 4"),
        ("implicit(3)", "int 4"),
    ];
    check(defs, &calls, 4);
    // Every function calling one of them is compiled with it: `twice` of `ret`, and `top` of
    // `twice` (on main, none was).
    let defs = format!("{}\nfn twice(x) = ret(x) * 2\nfn top(x) {{\n    return twice(x) + early(x)\n}}", defs);
    check(&defs, &[("top(3)", "int 12"), ("top(4)", "int 11")], 1);
}

#[test]
fn the_report_s_loop_is_compiled_whole_with_return_at_both_ends() {
    let defs = "fn f(x) {
    if x % 2 == 0 {
        return 1
    }
    return x % 7 + 1
}
fn g(n) {
    let i = 0
    let t = 0
    while i < n {
        t += f(i)
        i += 1
    }
    return t
}";
    // `g` loops, so it is compiled at its first call, with `f`: one for each type of `n`.
    check(defs, &[("g(0)", "int 0"), ("g(1)", "int 1"), ("g(2)", "int 3"), ("g(1000)", "int 2500")], 1);
    check(defs, &[("g(2.5)", "int 4")], 1);
}

#[test]
fn return_in_loops_and_after_them_ends_the_function() {
    let defs = "fn first_over(n) {
    for i in 0..n {
        if i * i > n {
            return i
        }
    }
    return -1
}
fn first_of(n) {
    for i in 0..n {
        return i
    }
    return -1
}
fn upto(n) {
    let i = 0
    while true {
        if i >= n {
            return i * 2
        }
        i += 1
    }
    return -1
}
fn total(n) {
    let t = 0
    for i in 0..n {
        t += i
    }
    return t
}";
    let calls = [
        ("first_over(0)", "int -1"),
        ("first_over(1)", "int -1"),
        ("first_over(2)", "int -1"),
        ("first_over(3)", "int 2"),
        ("first_over(50)", "int 8"),
        ("first_of(0)", "int -1"),
        ("first_of(1)", "int 0"),
        ("first_of(5)", "int 0"),
        ("upto(0)", "int 0"),
        ("upto(1)", "int 2"),
        ("upto(3)", "int 6"),
        ("total(0)", "int 0"),
        ("total(1)", "int 0"),
        ("total(4)", "int 6"),
    ];
    check(defs, &calls, 4);
}

#[test]
fn return_in_nested_ifs_in_both_arms_and_in_one_arm_of_a_value() {
    let defs = "fn sign(x) {
    if x >= 0 {
        if x == 0 {
            return 0
        } else {
            return 1
        }
    }
    return -1
}
fn band(x) {
    if x < 10 {
        return 1
    } else if x < 100 {
        return 2
    } else {
        return 3
    }
}
fn both(c) {
    if c {
        return 1.5
    } else {
        return 2.5
    }
}
fn one_arm(x) {
    if x > 0 {
        return x * 2
    } else {
        x + 1
    }
}
fn in_let(x) {
    let y = if x > 0 { return 1 } else { 2 }
    y + 10
}
fn nest(x) {
    let y = if x > 10 {
        if x > 20 {
            return 2.5
        } else {
            return 1.5
        }
    } else {
        x
    }
    return y * 1.0
}
fn after_both(x) {
    if x > 0 {
        return 1
    } else {
        return 2
    }
    3
}
fn ternary(x) = x > 0 ? x : return -1";
    let calls = [
        ("sign(-1)", "int -1"),
        ("sign(0)", "int 0"),
        ("sign(1)", "int 1"),
        ("band(9)", "int 1"),
        ("band(10)", "int 2"),
        ("band(99)", "int 2"),
        ("band(100)", "int 3"),
        ("both(true)", "float 1.5"),
        ("both(false)", "float 2.5"),
        ("one_arm(3)", "int 6"),
        ("one_arm(0)", "int 1"),
        ("one_arm(-3)", "int -2"),
        ("in_let(1)", "int 1"),
        ("in_let(0)", "int 12"),
        // An `if` whose arms both leave, in an arm of an `if` that gives a `let` its value:
        // the inner one's float does not go to the outer one's int.
        ("nest(25)", "float 2.5"),
        ("nest(21)", "float 2.5"),
        ("nest(20)", "float 1.5"),
        ("nest(11)", "float 1.5"),
        ("nest(10)", "float 10.0"),
        ("nest(-5)", "float -5.0"),
        ("after_both(1)", "int 1"),
        ("after_both(0)", "int 2"),
        ("ternary(3)", "int 3"),
        ("ternary(0)", "int -1"),
    ];
    check(defs, &calls, 8);
}

#[test]
fn a_recursive_function_with_an_early_return_is_compiled() {
    let defs = "fn fib(n) {
    if n < 2 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}
fn down(n) {
    if n > 0 {
        return down(n - 1)
    }
    return 0
}
fn halves(x) {
    if x < 1.0 {
        return x
    }
    return halves(x / 2)
}";
    let calls = [
        ("fib(0)", "int 0"),
        ("fib(1)", "int 1"),
        ("fib(2)", "int 1"),
        ("fib(20)", "int 6765"),
        // Its type guessed int, then float, which comes out.
        ("fib(2.5)", "float 2.0"),
        ("down(0)", "int 0"),
        ("down(100)", "int 0"),
        ("halves(8.0)", "float 0.5"),
        // `x / 2` of an int is an int or a float, depending on the numbers: interpreted.
        ("halves(8)", "float 0.5"),
    ];
    check(defs, &calls, 4);
}

#[test]
fn compiled_code_with_return_fails_where_the_interpreter_does() {
    let defs = "fn down(n) {
    if n > 0 {
        return down(n - 1)
    }
    return 0
}
fn grow(x) {
    if x > 4611686018427387904 {
        return x * 2
    }
    return x
}
fn rem(a, b) {
    if a > 0 {
        return a % b
    }
    return 0
}";
    let (got, n) = here(defs, &["down(20000)", "grow(4611686018427387904)", "grow(4611686018427387905)", "rem(7, 0)", "rem(7, 2)", "rem(0, 0)"]);
    assert_eq!(
        got,
        [
            "ERROR 3: too much recursion (more than 10000 nested calls) in `down`",
            "int 4611686018427387904",
            "ERROR 9: integer overflow",
            "ERROR 15: division by zero",
            "int 1",
            "int 0",
        ]
    );
    #[cfg(feature = "jit")]
    assert_eq!(n, 3);
    #[cfg(not(feature = "jit"))]
    let _ = n;
}

#[test]
fn an_int_on_one_path_and_a_float_on_another_is_left_to_the_interpreter() {
    let defs = "fn mix(x) {
    if x > 0 {
        return 1
    }
    return 1.5
}
fn mix_end(x) {
    if x > 0 {
        return 1
    }
    2.5
}
fn mix_arms(x) {
    if x > 0 {
        return 1
    } else {
        2.5
    }
}
fn bool_or_int(x) {
    if x > 0 {
        return true
    }
    return 1
}
fn declared(x) -> num {
    if x > 0 {
        return 1
    }
    return 1.5
}
fn calls_mix(x) = mix(x) + 1
fn arms(x) {
    if x > 0 {
        return 1
    }
    x > 5 ? 2 : 2.5
}
fn int_condition(n) {
    if n {
        return 1
    } else {
        2
    }
}
fn mix_rec(n) {
    if n < 1 {
        return 1.5
    }
    return mix_rec(n - 1)
}
fn floats(x) {
    if x > 0 {
        return x + 0.5
    }
    return 0.5
}";
    let calls = [
        ("mix(1)", "int 1"),
        ("mix(0)", "float 1.5"),
        ("mix_end(1)", "int 1"),
        ("mix_end(0)", "float 2.5"),
        ("mix_arms(1)", "int 1"),
        ("mix_arms(0)", "float 2.5"),
        ("bool_or_int(1)", "bool true"),
        ("bool_or_int(0)", "int 1"),
        ("declared(1)", "int 1"),
        ("declared(0)", "float 1.5"),
        ("calls_mix(1)", "int 2"),
        ("calls_mix(0)", "float 2.5"),
        ("arms(1)", "int 1"),
        ("arms(0)", "float 2.5"),
        // Not a bool: `if` takes the int as the interpreter does.
        ("int_condition(1)", "int 1"),
        ("int_condition(0)", "int 1"),
        // A float on every path, from an int: these two are compiled.
        ("mix_rec(3)", "float 1.5"),
        ("floats(1)", "float 1.5"),
        ("floats(0)", "float 0.5"),
    ];
    check(defs, &calls, 2);
}

#[test]
fn a_return_of_no_value_or_not_a_number_is_left_to_the_interpreter() {
    let defs = "fn none(x) {
    if x > 0 {
        return
    }
    x
}
fn calls_none(x) = none(x)
fn word(x) {
    if x > 0 {
        return \"pós\"
    }
    return x
}
fn nothing(x) {
    if x > 0 {
        return null
    }
    return x
}
fn listed(x) {
    return [x]
}
fn named(x) = \"n\" + str(x)
fn via(x) {
    if x > 0 {
        return named(x)
    }
    return 0
}";
    let calls = [
        ("none(1)", "null null"),
        ("none(-1)", "int -1"),
        ("calls_none(1)", "null null"),
        ("calls_none(0)", "int 0"),
        ("word(1)", "str pós"),
        ("word(0)", "int 0"),
        ("nothing(1)", "null null"),
        ("nothing(0)", "int 0"),
        ("listed(2)", "list [2]"),
        ("via(1)", "str n1"),
        ("via(0)", "int 0"),
    ];
    check(defs, &calls, 0);
    // The same function, given a text and then numbers: compiled for each type of number.
    let defs = "fn back(x) {\n    return x\n}";
    check(defs, &[("back(\"a\")", "str a"), ("back(2)", "int 2"), ("back(2.5)", "float 2.5"), ("back(true)", "bool true")], 3);
}

#[test]
fn a_value_that_always_leaves_by_return_is_left_to_the_interpreter() {
    // Where a value is used, an `if` whose arms both leave, or a block ending in `return`, has
    // none to give: these are interpreted, as on main.
    let defs = "fn let_leaves(x) {
    let y = if x > 0 { return 1 } else { return 2 }
    y
}
fn block_leaves(x) {
    let y = { return x }
    y + 1
}
fn operand_leaves(x) = 1 + { return x }
fn m(x) {
    match x {
        0 => { return 10 }
        _ => { return 20 }
    }
}";
    let calls = [
        ("let_leaves(1)", "int 1"),
        ("let_leaves(0)", "int 2"),
        ("block_leaves(4)", "int 4"),
        ("operand_leaves(4)", "int 4"),
        ("m(0)", "int 10"),
        ("m(1)", "int 20"),
    ];
    check(defs, &calls, 0);
}

/// Functions whose block, given to a `let`, makes a name the function has of another type: the
/// block's `x` is a float where the function's is an int. On main the type of `y`'s variable was
/// taken from the function's `x`, and Cranelift stopped the process (`declared type of variable
/// ... doesn't match type of value`).
const SHADOWS: &str = "fn shadow_if(n) {
    let x = 1
    let y = {
        let x = 2.5
        if n > 0 { x } else { x }
    }
    x + y
}
fn h(v) = v * 2
fn shadow_call(n) {
    let x = n
    let a = h(x)
    let y = {
        let x = 2.5
        h(x)
    }
    let i = 0
    while i < 1 {
        i += 1
    }
    y + a
}
fn shadow_return(n) {
    let x = 1
    let y = {
        let x = 2.5
        if n > 0 { return x } else { x }
    }
    return y + x
}
";

#[test]
fn a_block_s_own_names_type_its_value() {
    let calls = [("shadow_if(1)", "float 3.5"), ("shadow_call(1)", "float 7.0"), ("shadow_return(1)", "float 2.5"), ("shadow_return(0)", "float 3.5")];
    check(SHADOWS, &calls, 3);
}

#[test]
fn a_block_s_own_names_type_its_value_in_quillo_run() {
    // Through the binary, so that it can be run against main's (QUILLO_TEST_BIN), where it fails.
    let src = format!("{}{}for k in 0..3 {{\n    print(show(shadow_if(1)), show(shadow_call(1)), show(shadow_return(1)), show(shadow_return(0)))\n}}\n", SHADOWS, SHOW);
    let line = "float 3.5 float 7.0 float 2.5 float 3.5";
    assert_eq!(quillo_run(&src, &[]), [line, line, line].join("\n"));
}

/// The fastest of `runs` runs of `src` in `quillo run`, with `env`; and what it printed.
fn fastest(src: &str, env: &[(&str, &str)], runs: usize) -> (Duration, String) {
    let mut best = Duration::MAX;
    let mut out = String::new();
    for _ in 0..runs {
        let t = Instant::now();
        out = quillo_run(src, env);
        best = best.min(t.elapsed());
    }
    (best, out)
}

#[test]
fn the_report_s_forms_with_return_run_compiled_in_quillo_run() {
    // Through the binary, against the interpreter, so that it can be run against main's
    // (QUILLO_TEST_BIN), where compiled code was no faster than the interpreter with `return`
    // (0.8 to 1.2 times its time) and is 40 to 60 times faster here. 4 times leaves room for a
    // machine under load.
    let g = "fn g(n) {\n    let i = 0\n    let t = 0\n    while i < n {\n        t += f(i)\n        i += 1\n    }\n    return t\n}\nprint(g(1000000))\n";
    for (f, want) in [
        ("fn f(x) {\n    return x % 7 + 1\n}\n", "3999997"),
        ("fn f(x) {\n    if x % 2 == 0 {\n        return 1\n    }\n    return x % 7 + 1\n}\n", "2499997"),
    ] {
        let src = format!("{}{}", f, g);
        let (fast, out) = fastest(&src, &[], 3);
        let (slow, plain) = fastest(&src, &[("QUILLO_JIT", "0")], 2);
        assert_eq!((out.as_str(), plain.as_str()), (want, want), "{}", src);
        assert!(fast * 4 < slow, "compiled {:?}, interpreted {:?}\n{}", fast, slow, src);
    }
}

// ---- the review of 01767d5 (FEEDBACK52.md, Q-677 review; branch feedback52) ----------------

/// `src` in `quillo run` with machine code and with `QUILLO_JIT=0`: the same output and errors.
fn both_ways(src: &str) -> String {
    let compiled = quillo_run(src, &[]);
    assert_eq!(compiled, quillo_run(src, &[("QUILLO_JIT", "0")]), "compiled, then interpreted\n{}", src);
    compiled
}

/// `f` guesses its result an int, then a float; `g`, planned under the first guess with `f` an
/// int, was kept, and under the second its `t += f(n)` was made as an int plus a float into an
/// int: Cranelift's def_var stopped the process (quillo-A). `m4` is main's own shape of it, with
/// no `return`.
const GUESSES: &str = "fn f(n) {
    if n < 1 { return 0.5 }
    let z = g(n - 1)
    return 1.5
}
fn g(n) {
    let t = 0
    t += f(n)
    return t
}
fn r0(d) = d <= 0 ? 3.0 : r0(d - 1)
fn r1(d, x, y) {
    let a1 = r2(d - 1, x, x)
    a1 = (d / 3.0)
    r1(d - 1, 0, y)
}
fn r2(d, x, y) {
    let a2 = r0(d - 1)
    let v3 = (a2 < 1.0) ? y : r1(d - 1, x, true)
    5
}
fn fr(n) {
    if n < 1 { return 0.5 }
    let z = gr(n - 1)
    return 1.5
}
fn gr(n) {
    let t = 0.0
    t += fr(n)
    return t
}
";

#[test]
fn a_guess_that_does_not_come_out_takes_back_what_it_planned() {
    // `f` is refused, as on main, where it did not stop; `fr`, whose `gr` takes a float, comes
    // out on its second guess and is compiled, with `gr` planned again.
    let calls = [("f(3)", "float 1.5"), ("f(0)", "float 0.5"), ("fr(3)", "float 1.5"), ("fr(0)", "float 0.5"), ("gr(2)", "float 1.5")];
    check(GUESSES, &calls, 2);
}

#[test]
fn a_guess_that_does_not_come_out_takes_back_what_it_planned_in_quillo_run() {
    // Through the binary: on quillo-A (01767d5) `f` stops the process, on main `r2` does,
    // which recurses to the interpreter's limit.
    let src = format!("{}{}for k in 0..3 {{\n    print(show(f(3)), show(fr(3)))\n}}\nprint(r2(1, 0, 2))\n", GUESSES, SHOW);
    let line = "float 1.5 float 1.5";
    let got = both_ways(&src);
    assert!(got.starts_with(&format!("{}\nFAILED", [line, line, line].join("\n"))) && got.contains("too much recursion"), "{}", got);
}

#[test]
fn a_compile_that_fails_leaves_no_function_half_made() {
    // `f` and `g` call each other and `h`, compiled before them. On main and quillo-A the
    // compile of `f` made `g`, then refused `f` (its call of `h`, code of another compile), and
    // the next compile (`k`'s) stopped the process looking for `f` ("anonymous symbol must be
    // defined locally"). Now `f` calls `h`'s code; a compile refused part way gives each function
    // it did not make code that gives up.
    let src = "fn h(x) = x + 1
fn f(n) = n <= 0 ? h(n) : g(n - 1)
fn g(n) = n <= 0 ? 0 : f(n - 1)
fn k(x) = x * 2
print(h(1))
print(h(1))
print(f(6))
print(f(6))
print(k(1))
print(k(1))
print(k(1))
";
    assert_eq!(both_ways(src), "2\n2\n1\n1\n2\n2\n2");
}

#[test]
fn a_caller_of_a_function_compiled_on_its_own_is_compiled() {
    // `f` is compiled on its own first; `top`, which calls it, is compiled with a call of `f`'s
    // code (on quillo-A, 1 compile: `top` was refused).
    check("fn f(x) = x % 7 + 1\nfn top(x) = f(x) * 2", &[("f(3)", "int 4"), ("top(3)", "int 8"), ("top(10)", "int 8")], 2);
    // What `f` calls is what `top` calls: `h` given another function, `top` follows it.
    let defs = "fn h(x) = x + 1
fn f(x) = h(x) * 2
fn top(x) = f(x) + 1
fn swap() {
    h = (x) => x + 100
    top(1)
}";
    // Compiled: `f`, `top`, and the lambda given to `h`.
    check(defs, &[("f(1)", "int 4"), ("top(1)", "int 5"), ("swap()", "int 203"), ("top(1)", "int 203"), ("f(1)", "int 202")], 3);
}

#[test]
fn a_caller_of_a_function_compiled_on_its_own_runs_compiled_in_quillo_run() {
    // The report's loop, `f` called twice before it: on quillo-A `g` was never compiled
    // (0.60 s, Python's 0.63 s), here 0.02 s.
    let src = "fn f(x) {\n    if x % 2 == 0 {\n        return 1\n    }\n    return x % 7 + 1\n}\nfn g(n) {\n    let i = 0\n    let t = 0\n    while i < n {\n        t += f(i)\n        i += 1\n    }\n    return t\n}\nprint(f(3))\nprint(f(3))\nprint(g(1000000))\n";
    let (fast, out) = fastest(src, &[], 3);
    let (slow, plain) = fastest(src, &[("QUILLO_JIT", "0")], 2);
    assert_eq!((out.as_str(), plain.as_str()), ("4\n4\n2499997", "4\n4\n2499997"));
    assert!(fast * 4 < slow, "compiled {:?}, interpreted {:?}", fast, slow);
}

/// Recursions at the call limit and one either side, and past it, plainly recursive, ending in
/// `return` or not, and two functions calling each other.
const DEEPS: &str = "fn r(d, x) {
    if d <= 0 {
        return 0
    }
    return r(d - 1, x + 1)
}
fn bare(d, x) {
    if d <= 0 {
        return 0
    }
    bare(d - 1, x + 1)
}
fn ping(d) = d <= 0 ? 0 : pong(d - 1)
fn pong(d) = d <= 0 ? 1 : ping(d - 1)
for n in [9990, 9995, 9996, 9997, 9998, 9999, 10000, 100000] {
    print(n, r(n, 1) ?? \"E\", bare(n, 1) ?? \"E\", ping(n) ?? \"E\")
}
";

/// How many times the interpreter's time a call past the limit may take compiled: twice, and three
/// times for this build's debug binary, whose interpreter calls the floor's cold path slower.
fn slack() -> u32 {
    if std::env::var_os("QUILLO_TEST_BIN").is_none() && cfg!(debug_assertions) {
        3
    } else {
        2
    }
}

#[test]
fn a_call_too_deep_for_machine_code_is_followed_down_by_the_interpreter() {
    // Each level of a recursion past the limit went back into machine code and down to the
    // limit again: quillo-A 1.35 s, main 0.91 s (`bare`), the interpreter 0.06 s. The levels
    // under it are the interpreter's (the floor), and the calls within the limit are compiled.
    // Its speed bound, taken out with A6's floor, is back with Q-945's (FEEDBACK52.md, Q-945;
    // branch feedback52): quillo-A6 fails it.
    let want = "9990 0 0 0\n9995 0 0 1\n9996 0 0 0\n9997 0 0 1\n9998 0 0 0\n9999 0 0 1\n10000 E E E\n100000 E E E";
    assert_eq!(both_ways(DEEPS), want);
    let (fast, out) = fastest(DEEPS, &[], 3);
    let (slow, _) = fastest(DEEPS, &[("QUILLO_JIT", "0")], 2);
    assert_eq!(out, want);
    assert!(fast < slow * slack(), "compiled {:?}, interpreted {:?}", fast, slow);
    // Not caught, its error and where it was are the interpreter's.
    let failed = both_ways("fn r(d, x) {\n    if d <= 0 {\n        return 0\n    }\n    return r(d - 1, x + 1)\n}\nprint(r(1, 1))\nprint(r(100000, 1))\n");
    assert!(failed.starts_with("0\nFAILED") && failed.contains("too much recursion"), "{}", failed);
}

#[test]
fn after_a_call_too_deep_calls_are_compiled_again() {
    // The interpreter follows the call that went too deep down; back above it, `fib` and `sq`
    // are compiled, and `r` runs compiled within the limit.
    let defs = "fn r(d) {
    if d <= 0 {
        return 0
    }
    return r(d - 1)
}
fn fib(n) = n < 2 ? n : fib(n - 1) + fib(n - 2)
fn sq(x) = x * x";
    let calls = [("r(5)", "int 0"), ("r(100000) ?? \"E\"", "str E"), ("fib(15)", "int 610"), ("r(100)", "int 0"), ("sq(3)", "int 9"), ("r(100000) ?? \"E\"", "str E"), ("sq(4)", "int 16")];
    check(defs, &calls, 3);
}

#[test]
fn a_compile_refused_after_its_plan_gives_the_functions_it_did_not_make_code_that_gives_up() {
    // No program reaches this now: `jit::refuse_for_tests` makes the compile of `f` refuse it
    // once it is planned, in Gen, then in Cranelift. `g`, made before it in that compile, calls
    // it; `k`'s compile after it finalizes `g`, and `f` has code that gives up, where main and
    // quillo-A stopped (`a_compile_that_fails_leaves_no_function_half_made`). `f` and `g` are
    // the interpreter's; `h` and `k` are compiled.
    let defs = "fn h(x) = x + 1\nfn f(n) = n <= 0 ? h(n) : g(n - 1)\nfn g(n) = n <= 0 ? 0 : f(n - 1)\nfn k(x) = x * 2";
    let calls = [("h(1)", "int 2"), ("f(6)", "int 1"), ("f(5)", "int 0"), ("k(1)", "int 2"), ("g(3)", "int 1"), ("k(2)", "int 4")];
    // The same with `f` giving a float and a bool: code that gives up gives each type.
    let floats = "fn f(n) = n <= 0 ? 0.5 : g(n - 1)\nfn g(n) = n <= 0 ? 1.5 : f(n - 1)\nfn k(x) = x * 2";
    let float_calls = [("f(6)", "float 0.5"), ("f(5)", "float 1.5"), ("k(1)", "int 2"), ("g(3)", "float 0.5")];
    let bools = "fn f(n) = n <= 0 ? true : g(n - 1)\nfn g(n) = n <= 0 ? false : f(n - 1)\nfn k(x) = x * 2";
    let bool_calls = [("f(6)", "bool true"), ("f(5)", "bool false"), ("k(1)", "int 2"), ("g(3)", "bool true")];
    for in_cranelift in [false, true] {
        quillo::jit::refuse_for_tests("f", in_cranelift);
        check(defs, &calls, 2);
        check(floats, &float_calls, 1);
        check(bools, &bool_calls, 1);
    }
    quillo::jit::refuse_for_tests("", false);
    // Refusing nothing, as every program runs: `f` (with `g`) and `g` on its own compiled too.
    check(defs, &calls, 4);
    check(floats, &float_calls, 3);
}

// ---- the second review (FEEDBACK52.md, Q-677 review 2; branch feedback52) -------------------

/// `n` recursive functions each calling the next before itself: the first guess of each (an
/// int) does not come out, as each gives a float (`0.5`) or a bool (`true`).
fn chain(n: usize, end: &str) -> String {
    let mut src = String::new();
    for i in 1..=n {
        let next = if i < n { format!("let _a = f{}(0)\n        ", i + 1) } else { String::new() };
        src.push_str(&format!("fn f{i}(n) {{\n    if n <= 0 {{\n        {end}\n    }} else {{\n        {next}f{i}(n - 1)\n    }}\n}}\n"));
    }
    src + "print(f1(2))\nprint(f1(2))\nprint(f2(3))\n"
}

#[test]
fn a_guess_that_does_not_come_out_keeps_what_never_read_it() {
    // Each failed guess planned the rest of the chain again: quillo-B4 7.6 s for 24 float
    // functions, 1.4 s for 14 bool ones (each function 2 and 3 times the one before), against
    // 0.00 s here, the interpreter and main. 0.8 s leaves room for a loaded machine, and 10 times
    // that for this build's own debug binary.
    let limit = Duration::from_millis(if std::env::var_os("QUILLO_TEST_BIN").is_none() && cfg!(debug_assertions) { 8000 } else { 800 });
    for (n, end, want) in [(24, "0.5", "0.5\n0.5\n0.5"), (14, "true", "true\ntrue\ntrue"), (40, "0.5", "0.5\n0.5\n0.5")] {
        let src = chain(n, end);
        let t = Instant::now();
        assert_eq!(both_ways(&src), want, "{} functions ending in {}", n, end);
        assert!(t.elapsed() < limit * 2, "{} functions ending in {}: {:?} (both runs)", n, end, t.elapsed());
    }
}

#[test]
fn a_guess_that_does_not_come_out_keeps_what_never_read_it_and_compiles_the_chain() {
    // The chain is compiled, the first function's compile planning all the others once.
    check(&chain(12, "0.5").replace("print(f1(2))\nprint(f1(2))\nprint(f2(3))\n", ""), &[("f1(2)", "float 0.5"), ("f2(3)", "float 0.5"), ("f12(1)", "float 0.5")], 3);
}

#[test]
fn a_call_too_deep_is_followed_down_past_another_function_called_at_its_depth() {
    // `h` is compiled, and called at the depth of `r`'s next call: a floor that followed the
    // interpreter down was taken back there, and each level went into machine code to the limit
    // again (quillo-B4 0.24 s a call, the interpreter 0.01 s). The same when `h` recurses itself.
    // The floor stays at the call that went too deep (Q-945); quillo-A6, with none, fails the
    // speed bound.
    for h in ["fn h(x) {\n    x + 1\n}\n", "fn h(x) {\n    return x <= 0 ? 1 : h(x - 1) + 1\n}\n"] {
        let src = format!("{}fn r(d, x) {{\n    if d <= 0 {{\n        return x\n    }}\n    return r(d - 1, h(x % 3))\n}}\nprint(h(1))\nprint(h(2))\nfor k in 0..4 {{\n    print(r(100000, 1) ?? \"E\")\n}}\nprint(r(10, 1))\n", h);
        let want = "2\n3\nE\nE\nE\nE\n2";
        assert_eq!(both_ways(&src), want);
        let (fast, out) = fastest(&src, &[], 3);
        let (slow, _) = fastest(&src, &[("QUILLO_JIT", "0")], 2);
        assert_eq!(out, want);
        assert!(fast < slow * slack(), "compiled {:?}, interpreted {:?}\n{}", fast, slow, src);
    }
}

#[test]
fn a_builtin_compiled_code_reads_is_checked_where_it_is_called_again() {
    // `inner` is one function in two scopes: in the second `range` is the program's `r2`, and
    // compiled code made in the first gave the builtin's 6 where the interpreter says "cannot
    // loop over int" (main and quillo-B4).
    let nested = "fn mk(shadow) {
    fn r2(n) { 2 }
    let range = if shadow { r2 } else { range }
    fn inner(n) {
        let s = 0
        for i in range(n) {
            s += i
        }
        s
    }
    [inner]
}
let ka = mk(false)
let kb = mk(true)
print(ka[0](4))
print(ka[0](4))
print(ka[0](5))
print(kb[0](4))
";
    let got = both_ways(nested);
    assert!(got.starts_with("6\n6\n10\nFAILED") && got.contains("cannot loop over int"), "{}", got);
    // `range` made something else at the top (which `quillo check` warns of) after `f` is
    // compiled: the interpreter gives 10, and compiled code, which decided `range` when it was
    // made, gives 6, as on main. Following it was a hook on every `let` of a name made again,
    // which cost loops at a program's top level; it was dropped, and this asks only that the
    // program runs, with no panic (filed as Q-944).
    let top = "fn f(n) {\n    let s = 0\n    for i in range(n) {\n        s += i\n    }\n    s\n}\nprint(f(4))\nprint(f(4))\nlet range = (n) => [10]\nprint(f(4))\n";
    assert_eq!(quillo_run(top, &[("QUILLO_JIT", "0")]), "6\n6\n10");
    let got = quillo_run(top, &[]);
    assert!(got.starts_with("6\n6\n") && got.lines().count() == 3 && !got.contains("FAILED"), "{}", got);
    // Another builtin, `abs`, the program's in one scope and the builtin in the other: the call
    // of it is a dep already, and stays one.
    let other = "fn mk(shadow) {
    fn my_abs(x) = x < 0 ? 0 - x + 100 : x + 100
    let abs = if shadow { my_abs } else { abs }
    fn inner(n) {
        abs(n) * 2
    }
    [inner]
}
let ka = mk(true)
let kb = mk(false)
print(ka[0](-4))
print(ka[0](-4))
print(kb[0](-4))
print(kb[0](-4))
";
    assert_eq!(both_ways(other), "208\n208\n8\n8");
    // `range` the builtin everywhere, nested or not: compiled.
    check("fn f(n) {\n    let s = 0\n    for i in range(n) {\n        s += i\n    }\n    s\n}", &[("f(4)", "int 6"), ("f(0)", "int 0"), ("f(1)", "int 0")], 1);
}

#[test]
fn a_plan_standing_on_a_guess_stands_on_the_outermost_one_it_reads() {
    // `h` reads `g`'s guess and `g` reads `f`'s: once `g` is planned, `h` stands on `f`'s guess,
    // and `f` reads `g` again from the plan. `k`, planned under `f`'s first guess (an int, which
    // does not come out), never read it, and is kept while `g` and `h` are planned again.
    let defs = "fn k(x) = x * 2.0
fn f(n) = n <= 0 ? 0.5 + k(1.0) : g(n - 1) + g(n - 2)
fn g(n) = n <= 0 ? h(n) : f(n - 1)
fn h(n) = n <= 0 ? 1.5 : g(n - 1)";
    let calls = [("f(0)", "float 2.5"), ("f(1)", "float 3.0"), ("f(2)", "float 4.0"), ("f(5)", "float 9.5"), ("g(3)", "float 4.0"), ("h(4)", "float 4.0")];
    check(defs, &calls, 3);
    // `q`, planned on `p`'s first guess, and `k3`, which reads none, planned after it under that
    // guess: `k3` is kept when the guess does not come out, `q` planned again. And `w`, which
    // reads `u`'s guess, still stands on it once `v` is planned.
    let defs = "fn k3(x) = x * 2.0
fn p(n) = n <= 0 ? 0.5 : q(n - 1) + k3(2.0)
fn q(n) = p(n)
fn u(n) = n <= 0 ? 0.5 : v(n - 1)
fn v(n) = n <= 0 ? 1.5 : w(n - 1)
fn w(n) = n <= 0 ? 2.5 : u(n - 1)";
    let calls = [("p(0)", "float 0.5"), ("p(3)", "float 12.5"), ("q(2)", "float 8.5"), ("u(0)", "float 0.5"), ("u(1)", "float 1.5"), ("u(2)", "float 2.5"), ("u(5)", "float 2.5"), ("v(3)", "float 1.5")];
    check(defs, &calls, 4);
}

// ---- the third review (FEEDBACK52.md, Q-677 review 3; branch feedback52) --------------------

/// `n` recursive functions, each calling the next before itself, and each (but the first)
/// calling the first (`root`) or its caller (`caller`) too.
fn chain_back(n: usize, end: &str, root: bool, caller: bool) -> String {
    let mut src = String::new();
    for i in 1..=n {
        let mut parts = Vec::new();
        if root && i > 1 {
            parts.push("let _b = f1(0)".to_string());
        }
        if caller && i > 1 {
            parts.push(format!("let _c = f{}(0)", i - 1));
        }
        if i < n {
            parts.push(format!("let _a = f{}(0)", i + 1));
        }
        parts.push(format!("f{}(n - 1)", i));
        src.push_str(&format!("fn f{i}(n) {{\n    if n <= 0 {{\n        {end}\n    }} else {{\n        {}\n    }}\n}}\n", parts.join("\n        ")));
    }
    src
}

#[test]
fn a_guess_that_does_not_come_out_takes_back_only_the_plans_that_read_it() {
    // Each function also calls the first: a plan stood on the outermost guess it read alone, so
    // each function's failed guess took back the plans that read only the first function's,
    // 10.2 s for 24 functions on quillo-B5. 24 and 40, float and bool, compiled; a chain each
    // calling its caller (each plan reads the one before's guess, so planning it stays
    // exponential) stops at the plan budget and is interpreted. Each under 0.8 s (8 s for this
    // build's debug binary), and equal to the interpreter.
    let limit = Duration::from_millis(if std::env::var_os("QUILLO_TEST_BIN").is_none() && cfg!(debug_assertions) { 8000 } else { 800 });
    for (n, end, root, caller) in [(24, "0.5", true, false), (40, "0.5", true, false), (24, "true", true, false), (40, "true", true, false), (12, "true", false, true), (24, "true", false, true), (24, "0.5", true, true)] {
        let src = format!("{}print(f1(2))\nprint(f1(2))\nprint(f2(3))\n", chain_back(n, end, root, caller));
        let want = format!("{}\n{}\n{}", end, end, end);
        let t = Instant::now();
        assert_eq!(both_ways(&src), want, "{} functions ending in {}, root {}, caller {}", n, end, root, caller);
        assert!(t.elapsed() < limit * 2, "{} functions ending in {}, root {}, caller {}: {:?} (both runs)", n, end, root, caller, t.elapsed());
    }
}

#[test]
fn a_chain_reading_only_the_first_guess_is_compiled_and_one_past_the_budget_is_not() {
    let calls = [("f1(2)", "bool true"), ("f2(3)", "bool true")];
    // f1, with all the others, and f2, with those after it.
    check(&chain_back(24, "true", true, false), &calls, 2);
    // Each reading its caller's guess: past the budget (16,384 guesses and 64 for each function
    // planned), refused, the interpreter's.
    check(&chain_back(24, "true", false, true), &calls, 0);
}

#[test]
fn a_call_too_deep_is_followed_down_past_its_own_calls_at_its_depth() {
    // `r(0, x)`, at the depth of the next call, took a floor that followed the interpreter down,
    // and the next call went back into machine code to the limit at each level (quillo-B5 0.31 s,
    // the interpreter 0.01 s). Also with two calls of its own at each level, and with a helper
    // calling it from deeper than its own next call. The floor stays at the call that went too
    // deep (Q-945); quillo-A6, with none, fails the speed bound.
    let shapes = [
        ("", "r(d - 1, r(0, x) + 1)", "4\n4"),
        ("", "r(d - 1, r(0, x) + r(0, x + 1))", "15\n15"),
        ("fn g(x) = r(0, x) + 1\n", "r(d - 1, g(x) % 7)", "4\n4"),
    ];
    for (helper, call, first) in shapes {
        let src = format!("{}fn r(d, x) {{\n    if d <= 0 {{\n        return x\n    }}\n    return {}\n}}\nprint(r(3, 1))\nprint(r(3, 1))\nfor k in 0..4 {{\n    print(r(100000, 1) ?? \"E\")\n}}\n", helper, call);
        assert_eq!(both_ways(&src), format!("{}\nE\nE\nE\nE", first), "{}", src);
        let (fast, _) = fastest(&src, &[], 3);
        let (slow, _) = fastest(&src, &[("QUILLO_JIT", "0")], 2);
        assert!(fast < slow * slack(), "compiled {:?}, interpreted {:?}\n{}", fast, slow, src);
    }
}

#[test]
fn a_function_looping_over_range_keeps_the_quick_call() {
    // Checking `range` at each call kept it from the last function's quick call: mapped over
    // 3 million numbers, quillo-B5 took 3 times the instructions of quillo-B4 (about 30% of
    // the interpreter's time, against 6% here). A sixth leaves room for a loaded machine.
    let src = "fn f(n) {\n    let s = 0\n    for i in range(3) {\n        s += i * n\n    }\n    s\n}\nprint(range(0, 3000000) | map(f) | sum())\n";
    let (fast, out) = fastest(src, &[], 3);
    let (slow, plain) = fastest(src, &[("QUILLO_JIT", "0")], 2);
    assert_eq!((out.as_str(), plain.as_str()), ("13499995500000", "13499995500000"));
    let part = if std::env::var_os("QUILLO_TEST_BIN").is_none() && cfg!(debug_assertions) { 2 } else { 6 };
    assert!(fast * part < slow, "compiled {:?}, interpreted {:?}", fast, slow);
}

#[test]
fn range_named_by_the_function_itself_is_not_the_builtin() {
    // A parameter or a variable named `range`: compiled, it was read as the builtin (6), where
    // the interpreter says "cannot call int" (main and quillo-B5).
    for src in [
        "fn g(range, n) {\n    let s = 0\n    for i in range(n) {\n        s += i\n    }\n    s\n}\nprint(g(2, 4))\nprint(g(2, 4))\n",
        "fn g(n) {\n    let range = n\n    let s = 0\n    for i in range(n) {\n        s += i\n    }\n    s\n}\nprint(g(4))\nprint(g(4))\n",
        "fn g(n) {\n    let s = 0\n    for range in [1] {\n        for i in range(n) {\n            s += i\n        }\n    }\n    s\n}\nprint(g(4))\nprint(g(4))\n",
    ] {
        let got = both_ways(src);
        assert!(got.trim_start().starts_with("FAILED") && got.contains("cannot call int"), "{}\n{}", got, src);
    }
    // A function of its own given as `range`: the interpreter's, called.
    assert_eq!(both_ways("fn g(range, n) {\n    let s = 0\n    for i in range(n) {\n        s += i\n    }\n    s\n}\nprint(g((n) => [5, 6], 4))\nprint(g((n) => [5, 6], 4))\n"), "11\n11");
    // The other builtins Plan reads by name are calls, which a parameter already keeps from
    // being read as the program's function: `abs` given as a parameter, a function or an int.
    assert_eq!(both_ways("fn g(abs, n) = abs(n) * 2\nprint(g((x) => x + 1, 4))\nprint(g((x) => x + 1, 4))\n"), "10\n10");
    let got = both_ways("fn g(abs, n) = abs(n) * 2\nprint(g(3, 4))\nprint(g(3, 4))\n");
    assert!(got.trim_start().starts_with("FAILED") && got.contains("cannot call int"), "{}", got);
}

// ---- the fourth review (FEEDBACK52.md, Q-677 review 4; branch feedback52) --------------------

#[test]
fn a_chain_each_calling_its_caller_is_compiled_up_to_the_budget() {
    // 5 bool functions each calling its caller and the next (363 guesses) and 10 float ones
    // (2,046), which main compiles, were refused by quillo-A5's budget of 64 guesses a function.
    // The limit and one either side: 8 bools (9,840) and 13 floats (16,382) compiled, as on main,
    // f1 with the rest and f2 with those after it; 9 bools (29,523) and 14 floats (32,766) past
    // 16,384 and 64 a function, refused. Moved up one each with the budget's base (budget base
    // raised: 8,192 refused 8 bools and 13 floats, which main compiles; Q-945 review 5).
    let calls = |end: &str| [("f1(2)", if end == "true" { "bool true" } else { "float 0.5" }), ("f2(3)", if end == "true" { "bool true" } else { "float 0.5" })];
    for (n, end, compiled) in [(5, "true", 2), (8, "true", 2), (9, "true", 0), (10, "0.5", 2), (13, "0.5", 2), (14, "0.5", 0)] {
        check(&chain_back(n, end, false, true), &calls(end), compiled);
    }
}

#[test]
fn a_chain_past_the_budget_is_planned_once_and_left_to_the_interpreter() {
    // 24 bools each calling its caller and the next, and `f12` calling `sq` too. `f1`'s plan
    // meets the budget; `f2`, `f24` and `f7`, each the first of its own compile, meet `f1` and
    // are not planned again to the budget; each is the interpreter's (`hot` at u32::MAX, read
    // first by each call, as for a function that can never compile), for floats too. `sq`,
    // planned under `f1`'s and never refused, is compiled on its own.
    let defs = chain_back(24, "true", false, true).replace("let _c = f11(0)", "let _s = sq(3)\n        let _c = f11(0)") + "fn sq(x) = x * x\n";
    let calls = [("f1(2)", "bool true"), ("f2(3)", "bool true"), ("f24(1)", "bool true"), ("f7(2)", "bool true"), ("f1(2.5)", "bool true"), ("sq(4)", "int 16")];
    check(&defs, &calls, 1);
    let mut it = Interp::new(".");
    it.load_source(&format!("{}{}", defs, SHOW), "case.qwe").unwrap();
    let hot = |it: &Interp, name: &str| match quillo::value::lookup(&it.globals, name) {
        Some(quillo::value::Value::Func(c)) => c.def.hot.load(Ordering::Relaxed),
        _ => panic!("{}", name),
    };
    let call = |it: &mut Interp, name: &str, n: i64| {
        let f = quillo::value::lookup(&it.globals, name).unwrap();
        it.call(&f, vec![quillo::value::Value::Int(n)], 0).ok().map(|v| v.display())
    };
    let t = Instant::now();
    for (name, n) in [("f1", 2), ("f1", 2), ("f1", 2), ("f24", 1), ("f24", 1), ("f7", 2), ("f7", 2)] {
        assert_eq!(call(&mut it, name, n).as_deref(), Some("true"), "{}({})", name, n);
    }
    // Planned to the budget once: about 4 ms in a release build; 2 s leaves room for a debug
    // build on a loaded machine. Planned again by each first function, it was 24 times that.
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    #[cfg(feature = "jit")]
    for name in ["f1", "f2", "f7", "f24"] {
        assert_eq!(hot(&it, name), u32::MAX, "{}", name);
    }
    // Planned under `f1`'s compile, never the first of one: not marked.
    for name in ["f3", "f20", "sq"] {
        assert_ne!(hot(&it, name), u32::MAX, "{}", name);
    }
    drop(it);
}

#[test]
fn a_chain_each_calling_its_caller_runs_compiled_in_quillo_run() {
    // 5 bool functions each calling its caller and the next, called 100,000 times from the top
    // of the program, 41 calls each: compiled, as on main; quillo-A5 refused it (its budget) and
    // took 1.3 times the interpreter's (2 million calls of `f1(2)`: 30,409 M instructions, the
    // interpreter 22,891 M).
    let src = format!("{}let c = 0\nfor i in 0..100000 {{\n    if f1(20) {{\n        c += 1\n    }}\n}}\nprint(c)\n", chain_back(5, "true", false, true));
    let (fast, out) = fastest(&src, &[], 3);
    let (slow, plain) = fastest(&src, &[("QUILLO_JIT", "0")], 2);
    assert_eq!((out.as_str(), plain.as_str()), ("100000", "100000"));
    assert!(fast * 2 < slow, "compiled {:?}, interpreted {:?}", fast, slow);
}

// ---- Q-945: recursion past the call limit (FEEDBACK52.md, Q-945; branch feedback52) ----------

/// `src` with `return` and without, the same values compiled and interpreted, and compiled in no
/// more than `slack()` times the interpreter's time.
fn past_the_limit(forms: [&str; 2], want: &str) {
    for src in forms {
        assert_eq!(both_ways(src), want, "{}", src);
        let (fast, out) = fastest(src, &[], 2);
        let (slow, _) = fastest(src, &[("QUILLO_JIT", "0")], 2);
        assert_eq!(out, want);
        assert!(fast < slow * slack(), "compiled {:?}, interpreted {:?}\n{}", fast, slow, src);
    }
}

#[test]
fn a_runaway_recursion_fails_in_the_interpreter_s_time() {
    // fact(-1) never reaches its base. Compiled since Q-677 (with `return`), each level the
    // interpreter ran went back into machine code and down to the limit: quillo-A6 0.3 s a call,
    // where main interpreted it (0.004 s). Without `return`, main compiled it and was as slow.
    let calls = "print(fact(5))\nprint(fact(5))\nfor k in 0..5 {\n    print(fact(-1) ?? \"E\")\n}\nprint(fact(10))\n";
    past_the_limit(
        [
            &format!("fn fact(n) {{\n    if n == 0 {{\n        return 1\n    }}\n    return n * fact(n - 1)\n}}\n{}", calls),
            &format!("fn fact(n) = n == 0 ? 1 : n * fact(n - 1)\n{}", calls),
        ],
        "120\n120\nE\nE\nE\nE\nE\n3628800",
    );
}

#[test]
fn many_calls_of_itself_at_each_level_past_the_limit_are_the_interpreter_s() {
    // Review 4's LOW 5: each level sums 200 calls of itself at depth 0. Under the call that went
    // too deep, each of them went into machine code again: quillo-A6 16.6 s against the
    // interpreter's 0.26 s with `return`, and main 19.6 s without.
    let ret = "fn r(d, x) {\n    if d <= 0 {\n        return x\n    }\n    let s = 0\n    for i in range(200) {\n        s += r(0, i)\n    }\n    return r(d - 1, s % 7)\n}\nprint(r(3, 1))\nprint(r(3, 1))\nprint(r(100000, 1) ?? \"E\")\n";
    let bare = "fn r(d, x) {\n    if d <= 0 {\n        x\n    } else {\n        let s = 0\n        for i in range(200) {\n            s += r(0, i)\n        }\n        r(d - 1, s % 7)\n    }\n}\nprint(r(3, 1))\nprint(r(3, 1))\nprint(r(100000, 1) ?? \"E\")\n";
    past_the_limit([ret, bare], "6\n6\nE");
}

#[test]
fn calls_after_a_call_too_deep_are_compiled_again_in_quillo_run() {
    // Review 4's MEDIUM 4: after `f(20000, 1)` went too deep, `h`, interpreted, calls `f(0, ..)`
    // in a loop: compiled again, as the first call back at the top's level ends the floor. Four
    // deep calls first: quillo-A6 took 0.23 s for each; a floor kept on would leave `h`'s calls
    // interpreted, the interpreter's 0.1 s.
    let h = "fn h(k) {\n    let t = 0\n    for j in 0..k {\n        t += f(0, 10000)\n    }\n    print(\"h\", t)\n}\nfor k in 0..4 {\n    print(f(20000, 1) ?? \"E\")\n}\nh(300)\n";
    let lp = "        let s = 0\n        let i = 0\n        while i < n {\n            s += i % 3\n            i += 1\n        }\n";
    for src in [
        format!("fn f(d, n) {{\n    if d <= 0 {{\n{}        return s\n    }}\n    return f(d - 1, n)\n}}\n{}", lp, h),
        format!("fn f(d, n) {{\n    if d <= 0 {{\n{}        s\n    }} else {{\n        f(d - 1, n)\n    }}\n}}\n{}", lp, h),
    ] {
        let want = "E\nE\nE\nE\nh 2999700";
        assert_eq!(both_ways(&src), want, "{}", src);
        let (fast, out) = fastest(&src, &[], 3);
        let (slow, _) = fastest(&src, &[("QUILLO_JIT", "0")], 2);
        assert_eq!(out, want);
        assert!(fast * 2 < slow, "compiled {:?}, interpreted {:?}\n{}", fast, slow, src);
    }
}

#[test]
fn a_thread_that_ends_under_a_floor_leaves_the_others_compiled() {
    // Each of four threads ends with a call too deep for machine code, its floor still set; the
    // thread's end takes it out of the count every call reads first. Calls here, and in a thread
    // after them, are compiled as before, with the interpreter's values.
    let defs = "fn r(d) {\n    if d <= 0 {\n        return 0\n    }\n    return r(d - 1)\n}\nfn sq(x) = x * x";
    for _ in 0..4 {
        let src = format!("{}\n", defs);
        std::thread::spawn(move || {
            let mut it = Interp::new(".");
            it.load_source(&src, "case.qwe").unwrap();
            let f = quillo::value::lookup(&it.globals, "r").unwrap();
            for d in [3, 3, 100000] {
                let _ = it.call(&f, vec![quillo::value::Value::Int(d)], 0);
            }
        })
        .join()
        .unwrap();
    }
    check(defs, &[("r(5)", "int 0"), ("sq(3)", "int 9"), ("r(100000) ?? \"E\"", "str E"), ("sq(4)", "int 16")], 2);
    let defs = defs.to_string();
    std::thread::spawn(move || check(&defs, &[("sq(5)", "int 25"), ("r(7)", "int 0")], 2)).join().unwrap();
}

// ---- review 5 of Q-945 (FEEDBACK52.md, Q-945 review 5; branch feedback52) -------------------

/// A recursion that goes too deep, a caller that passes its error on, both interpreted after, and
/// what `map` and a component call: `fib`, compiled.
const AFTER_DEEP: &str = "fn d(n) { if n <= 0 { 0 } else { d(n - 1) + 1 } }
fn fib(n) { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }
fn outer(k) {
    let label = \"k\" + str(k)
    d(k) + len(label)
}
component Row(n) {
    <li>{fib(n)}</li>
}
";

#[test]
fn the_floor_ends_with_the_call_that_went_too_deep() {
    // `map` calls its function through `jit::call` only for a number, and a component never
    // does: after `d(100000) ?? "E"` the floor stayed, and `fib`, called under them, was
    // interpreted (quillo-A7: the interpreter's time, 2,596 M instructions against 225 M). The
    // floor ends with the call that set it, by its error; and by one passed on through `outer`.
    for deep in ["d(100000) ?? \"E\"", "outer(100000) ?? \"E\""] {
        let src = format!("{}print(d(10))\nprint({})\nlet xs = map([\"a\", \"b\", \"c\", \"d\", \"e\", \"f\"], (s) => fib(24))\nprint(xs[5])\nprint(len(str(<ul><Row n={{24}}/><Row n={{24}}/></ul>)))\n", AFTER_DEEP, deep);
        let want = "10\nE\n46368\n255";
        assert_eq!(both_ways(&src), want, "{}", src);
        let (fast, out) = fastest(&src, &[], 3);
        let (slow, _) = fastest(&src, &[("QUILLO_JIT", "0")], 2);
        assert_eq!(out, want);
        assert!(fast * 4 < slow, "compiled {:?}, interpreted {:?}\n{}", fast, slow, src);
    }
}

/// `src` served by the quillo these tests run (QUILLO_TEST_BIN's, or this build's), with `extra`
/// options and `env`: the server, stopped when dropped, and its port.
fn served(src: &str, extra: &[&str], env: &[(&str, &str)]) -> (support::Guard, u16) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = support::temp_dir(&format!("feedback52-jit-srv-{}", N.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    for _ in 0..5 {
        let port = support::free_port();
        let child = Command::new(quillo())
            .arg("run")
            .arg(d.join("main.qwe"))
            .args(["--port", &port.to_string()])
            .args(extra)
            .envs(env.iter().copied())
            .env("QUILLO_SECRET", "a signing secret for these tests, long enough")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut server = support::Guard::new(child);
        let deadline = Instant::now() + Duration::from_secs(30);
        while server.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
            if support::listening(port) {
                return (server, port);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // Another test's server took the port first: again, on another.
    }
    panic!("the server did not start\n{}", src);
}

/// The fastest of `n` GETs of `path`, with the status and body of the last.
fn fastest_get(port: u16, path: &str, n: usize) -> (Duration, u16, String) {
    let mut best = Duration::MAX;
    let mut last = (0, String::new());
    for _ in 0..n {
        // A server just started, on a loaded machine, can refuse a first connection.
        let deadline = Instant::now() + Duration::from_secs(20);
        while !support::listening(port) {
            assert!(Instant::now() < deadline, "the server on {} stopped answering", port);
            std::thread::sleep(Duration::from_millis(20));
        }
        let t = Instant::now();
        let a = support::send(port, "GET", path, &[], "");
        best = best.min(t.elapsed());
        last = (a.status, a.body);
    }
    (best, last.0, last.1)
}

#[test]
fn a_route_after_one_whose_call_went_too_deep_is_compiled_again() {
    // `/deep` takes `d(100000)`'s error with `??` and answers. Its second time, `d` compiled
    // (the first call is interpreted), the floor stayed on its thread on quillo-A7, and `/map` (a
    // function given to `map` for each row) and `/page` (a component) were interpreted there, 19
    // and 17 ms against main's 1 ms. One worker, so that each request is on that thread.
    let src = format!("{}let rows = [{{ n: 22 }}, {{ n: 22 }}, {{ n: 22 }}, {{ n: 22 }}]\nroute GET /deep = {{ \"v\": d(100000) ?? -1 }}\nroute GET /map = map(rows, (r) => fib(r.n))\nroute GET /page = <html><body><ul><Row n={{22}}/><Row n={{22}}/><Row n={{22}}/></ul></body></html>\n", AFTER_DEEP);
    let (_server, port) = served(&src, &["--workers", "1"], &[]);
    let (map0, ..) = fastest_get(port, "/map", 3);
    let (page0, ..) = fastest_get(port, "/page", 3);
    let (_, status, body) = fastest_get(port, "/deep", 2);
    assert_eq!((status, body.as_str()), (200, "{\"v\":-1}"));
    let (map1, s1, b1) = fastest_get(port, "/map", 3);
    let (page1, s2, b2) = fastest_get(port, "/page", 3);
    assert_eq!((s1, b1.as_str(), s2), (200, "[17711,17711,17711,17711]", 200));
    assert!(b2.contains("<li>17711</li>"), "{}", b2);
    let kept = |before: Duration, after: Duration| after < before * 3 + Duration::from_millis(10);
    assert!(kept(map0, map1) && kept(page0, page1), "/map {:?} then {:?}, /page {:?} then {:?}", map0, map1, page0, page1);
    // Under `--max-compute` the routes' calls are the interpreter's (no machine code is tried for
    // a request that may be stopped), a function with a declared result rendered in markup too,
    // with the same answers.
    let src = format!("{}fn Cell(n) -> html = <b>{{fib(n)}}</b>\nroute GET /typed = <html><body><Cell n={{22}}/></body></html>\n", src);
    let (_server, port) = served(&src, &["--workers", "1", "--max-compute", "10s"], &[]);
    for path in ["/deep", "/deep", "/map"] {
        let a = support::send(port, "GET", path, &[], "");
        assert_eq!((a.status, a.body.as_str()), (200, if path == "/map" { "[17711,17711,17711,17711]" } else { "{\"v\":-1}" }), "{}", path);
    }
    let a = support::send(port, "GET", "/typed", &[], "");
    assert!(a.status == 200 && a.body.contains("<b>17711</b>"), "{} {}", a.status, a.body);
}

#[test]
fn a_call_given_up_for_the_stack_left_keeps_the_calls_under_it_compiled() {
    // A request's `parallel` children have 512 KB stacks: `r(2000)` gave up there for the stack
    // left, not the call limit, and was taken as too deep, so quillo-A7 interpreted it whole,
    // 316-332 ms against main's 6-7 ms. It runs again on a stack with room for the limit, and
    // ends in machine code. A runaway `fact(-1)` there gives up again at the limit and has its
    // floor: left to the interpreter with none, as main, it went down in machine code at each
    // level, 120 ms against the interpreter's 10-23 ms.
    let src = "fn w(n) { if n < 2 { n } else { w(n - 1) + w(n - 2) } }\nfn r(k) { if k <= 0 { 0 } else { r(k - 1) + w(12) } }\nfn fact(n) { if n == 0 { 1 } else { n * fact(n - 1) } }\nroute GET /warm = str(fact(5) + fact(5))\nroute GET /par = str(parallel(() => r(2000), () => r(2000)))\nroute GET /pfact = str(parallel(() => fact(-1) ?? \"E\", () => fact(-1) ?? \"E\"))\n";
    let times = |env: &[(&str, &str)]| {
        let (_server, port) = served(src, &[], env);
        let _ = fastest_get(port, "/warm", 2);
        (fastest_get(port, "/par", 3), fastest_get(port, "/pfact", 3))
    };
    let ((par, s1, b1), (pfact, s2, b2)) = times(&[]);
    let ((par0, s3, b3), (pfact0, s4, b4)) = times(&[("QUILLO_JIT", "0")]);
    for (s, b, want) in [(s1, &b1, "[288000, 288000]"), (s3, &b3, "[288000, 288000]"), (s2, &b2, "[\"E\", \"E\"]"), (s4, &b4, "[\"E\", \"E\"]")] {
        assert_eq!((s, b.as_str()), (200, want));
    }
    assert!(par * 3 < par0, "/par compiled {:?}, interpreted {:?}", par, par0);
    assert!(pfact < pfact0 * slack() + Duration::from_millis(10), "/pfact compiled {:?}, interpreted {:?}", pfact, pfact0);
}

#[test]
fn a_floor_left_behind_ends_at_the_next_call_no_deeper() {
    // No program leaves a floor behind now (the call that set it takes it away as it ends), so
    // `jit::leave_floor_for_tests` does: the next call at its depth, the first here, ends it, and
    // the calls after it are compiled as before. A thread that ends with one takes itself out of
    // the count every call reads first.
    let defs = "fn sq(x) = x * x\nfn twice(x) = sq(x) + sq(x)";
    quillo::jit::leave_floor_for_tests(0);
    check(defs, &[("twice(3)", "int 18"), ("sq(4)", "int 16")], 2);
    std::thread::spawn(|| quillo::jit::leave_floor_for_tests(7)).join().unwrap();
    check(defs, &[("twice(5)", "int 50")], 2);
    // A call given its default, past the limit, is the interpreter's: no machine code is tried
    // there.
    let src = "fn rd(d, x = 1) {\n    if d <= 0 {\n        return 0\n    }\n    return rd(d - 1) + x\n}\nprint(rd(5))\nprint(rd(5))\nprint(rd(100000) ?? \"E\")\n";
    assert_eq!(both_ways(src), "5\n5\nE");
}

// ---- review 6 of Q-945, and Q-947 (FEEDBACK52.md; branch feedback52) -------------------------

/// `big(n, a)`, with `k` float `let`s still needed after its own call: its frame grows with `k`.
/// Its base is `n == 0` when `runaway`, so that `big(-1, ...)` never reaches it.
fn big(k: usize, runaway: bool) -> String {
    let lets: Vec<String> = std::iter::once("let v0 = a * 0.5".to_string()).chain((1..k).map(|i| format!("let v{} = v{} * 0.5 + a", i, i - 1))).collect();
    let total: Vec<String> = (0..k).map(|i| format!("v{}", i)).collect();
    format!(
        "fn big(n, a) {{\n    if {} {{\n        a\n    }} else {{\n        {}\n        let r = big(n - 1, v{} * 0.25)\n        r + {}\n    }}\n}}\n",
        if runaway { "n == 0" } else { "n <= 0" },
        lets.join("\n        "),
        k - 1,
        total.join(" + ")
    )
}

#[test]
fn a_frame_larger_than_512_bytes_does_not_overflow_the_stack() {
    // `run` took 512 bytes as a compiled call's most: with 57 such lets, in `parallel`'s child,
    // the frames overflowed its stack and stopped the process ("stack overflow", main and
    // quillo-A8); with 56 they did not. The edge, one either side, and far past it: the
    // interpreter's values (FEEDBACK52.md, Q-947; branch feedback52).
    for k in [56, 57, 58, 120] {
        let src = format!("{}print(big(3, 1.5))\nprint(big(3, 1.5))\nprint(parallel(() => big(9000, 1.5), () => 1))\n", big(k, false));
        let got = both_ways(&src);
        assert!(!got.contains("FAILED"), "k = {}: {}", k, got);
    }
}

#[test]
fn a_runaway_recursion_with_large_frames_is_a_quillo_error() {
    // `big(-1, 1.5)` never ends: main stopped with "stack overflow" from 31 lets (30 ran), and
    // quillo-A8's floor kept it a Quillo error up to 103, not from 104. Each edge and one either
    // side (Q-947).
    for k in [30, 31, 32, 103, 104, 105] {
        let src = format!("{}print(big(3, 1.5))\nprint(big(3, 1.5))\nprint(big(-1, 1.5) ?? \"E\")\n", big(k, true));
        assert_eq!(both_ways(&src).lines().last(), Some("E"), "k = {}", k);
    }
}

#[test]
fn a_route_whose_parallel_child_has_large_frames_keeps_the_server() {
    // `big(2000, 1.5)` in a route's `parallel` child (512 KB): with 34 lets the server died of
    // SIGBUS on main and quillo-A8, with 33 it answered. The edge and one either side: the
    // interpreter's answer, twice, and the server still up (Q-947).
    for k in [33, 34, 35] {
        let src = format!("{}route GET /warm = str(big(3, 1.5) + big(3, 1.5))\nroute GET /big = str(parallel(() => big(2000, 1.5), () => 1))\n", big(k, false));
        let want = {
            let (_server, port) = served(&src, &[], &[("QUILLO_JIT", "0")]);
            fastest_get(port, "/big", 1)
        };
        assert_eq!(want.1, 200, "k = {}: {}", k, want.2);
        let (_server, port) = served(&src, &[], &[]);
        let _ = fastest_get(port, "/warm", 2);
        for _ in 0..2 {
            let a = support::send(port, "GET", "/big", &[], "");
            assert_eq!((a.status, a.body.as_str()), (200, want.2.as_str()), "k = {}", k);
        }
        assert!(support::listening(port), "k = {}: the server stopped", k);
    }
}

#[test]
fn a_call_given_up_for_the_stack_and_then_otherwise_is_interpreted_once() {
    // Given up for the stack in a 512 KB `parallel` child, then on a stack with room for the
    // limit for an overflow at the bottom (`3 * p(n - 1)`), or for an int against a float
    // (`9007199254740993 < x`): with no floor the interpreter took one level, the next was given
    // up for the stack again and run on a new stack, a rerun and an mmap a level. `/par92`:
    // quillo-A8 372 ms, main 155, the interpreter 4.4; two children of `p(5000)` 190 against 120;
    // `q` 188 against 121. With a floor, about the interpreter's time (Q-945 review 6).
    let src = "fn p(n) { if n == 0 { 1 } else { 3 * p(n - 1) } }\nfn q(n, x) { if n == 0 { 9007199254740993 < x } else { q(n - 1, x) } }\nroute GET /warm = str([p(3), p(3)]) + str(q(2, 1.5)) + str(q(2, 1.5))\nroute GET /par92 = str(parallel(() => p(9000) ?? \"E\", () => 1))\nroute GET /p2 = str(parallel(() => p(5000) ?? \"E\", () => p(5000) ?? \"E\"))\nroute GET /q2 = str(parallel(() => q(5000, 9007199254740992.0), () => q(5000, 9007199254740992.0)))\n";
    let paths = ["/par92", "/p2", "/q2"];
    let times = |env: &[(&str, &str)]| {
        let (_server, port) = served(src, &[], env);
        let _ = fastest_get(port, "/warm", 2);
        paths.map(|p| fastest_get(port, p, 3))
    };
    let (fast, slow) = (times(&[]), times(&[("QUILLO_JIT", "0")]));
    for ((path, f), s) in paths.iter().zip(&fast).zip(&slow) {
        assert_eq!((f.1, &f.2), (s.1, &s.2), "{}", path);
        assert!(f.0 < s.0 * slack() + Duration::from_millis(10), "{}: compiled {:?}, interpreted {:?}", path, f.0, s.0);
    }
    assert_eq!([fast[0].2.as_str(), fast[1].2.as_str(), fast[2].2.as_str()], ["[\"E\", 1]", "[\"E\", \"E\"]", "[false, false]"]);
}

#[test]
fn a_call_too_deep_through_each_way_in_ends_its_floor() {
    // `call_closure` (a route's or a rendered function's way in) and `call1` (`map`'s) also take
    // a too-deep call's floor away as it ends: `fib`, called after it under a `map` over texts,
    // which reaches `jit::call` only through `fib` itself, is compiled, as fast as before it
    // (Q-945 reviews 5 and 6).
    let defs = "fn d(n) { if n <= 0 { 0 } else { d(n - 1) + 1 } }\nfn fib(n) { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\nlet texts = [\"a\", \"b\", \"c\"]\nlet by_text = (s) => fib(22)\nlet deep = [3, 3, 100000]\n";
    let mut it = Interp::new(".");
    it.load_source(defs, "case.qwe").unwrap();
    let get = |it: &Interp, name: &str| quillo::value::lookup(&it.globals, name).unwrap();
    let (map, texts, by_text) = (get(&it, "map"), get(&it, "texts"), get(&it, "by_text"));
    let timed = |it: &mut Interp| {
        let t = Instant::now();
        let v = it.call(&map, vec![texts.clone(), by_text.clone()], 0).ok().map(|v| v.display());
        assert_eq!(v.as_deref(), Some("[17711, 17711, 17711]"));
        t.elapsed()
    };
    let _ = (timed(&mut it), timed(&mut it));
    let before = timed(&mut it).min(timed(&mut it));
    // Through `call_closure`: `d(100000)`, after two calls that compile it.
    let Some(quillo::value::Value::Func(d)) = quillo::value::lookup(&it.globals, "d") else { panic!() };
    for n in [3, 3] {
        assert_eq!(it.call_closure(&d, vec![Some(quillo::value::Value::Int(n))], 0).ok().map(|v| v.display()).as_deref(), Some("3"));
    }
    assert!(it.call_closure(&d, vec![Some(quillo::value::Value::Int(100000))], 0).is_err());
    let after_closure = timed(&mut it).min(timed(&mut it));
    // Through `call1`: `map(deep, d)`, its third call too deep, its error passed on.
    assert!(it.call(&map, vec![get(&it, "deep"), get(&it, "d")], 0).is_err());
    let after_map = timed(&mut it).min(timed(&mut it));
    let kept = |after: Duration| after < before * 3 + Duration::from_millis(20);
    assert!(kept(after_closure) && kept(after_map), "before {:?}, after call_closure {:?}, after call1 {:?}", before, after_closure, after_map);
}
