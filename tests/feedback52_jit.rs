//! FEEDBACK52.md, Q-677 (branch feedback52): a function with `return`, at its end or early, is
//! compiled to machine code as one ending in its value is, and so is every function calling it.
//! Each program runs here, where numeric functions are compiled, and in `quillo run` with
//! `QUILLO_JIT=0`, all in the interpreter: both give the same values of the same types, and the
//! functions that must not be compiled (a `return` of no value or of a text, an int on one path
//! and a float on another) are not.

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
    let (out, err) = (String::from_utf8_lossy(&o.stdout).trim_end().to_string(), String::from_utf8_lossy(&o.stderr).trim_end().to_string());
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
