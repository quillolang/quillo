//! Every shape of function the compiler takes up or leaves alone: whichever
//! it does, the answers and the errors are the interpreter's.

use quillo::Interp;

/// Each call's value or error: each called three times (interpreted, then
/// compiled), all three the same.
fn run(defs: &str, calls: &[&str]) -> Vec<String> {
    let mut src = format!("{}\n", defs);
    for (i, c) in calls.iter().enumerate() {
        src.push_str(&format!("fn __c{}() = {}\n", i, c));
    }
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    (0..calls.len())
        .map(|i| {
            let f = quillo::value::lookup(&it.globals, &format!("__c{}", i)).unwrap();
            let once = |it: &mut Interp| match it.call(&f, vec![], 0) {
                Ok(v) => v.repr(),
                Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR {}: {}", e.line, e.msg),
                Err(_) => "returned".into(),
            };
            let first = once(&mut it);
            for k in 2..=3 {
                assert_eq!(once(&mut it), first, "{} (call {})", calls[i], k);
            }
            first
        })
        .collect()
}

fn same(defs: &str, calls: &[(&str, &str)]) {
    let names: Vec<&str> = calls.iter().map(|(c, _)| *c).collect();
    for ((c, want), got) in calls.iter().zip(run(defs, &names)) {
        assert_eq!(got, *want, "{}", c);
    }
}

/// `calls` give their answers, and compiling them adds at least one function.
fn compiles(defs: &str, calls: &[(&str, &str)]) {
    let before = quillo::jit::compiled();
    same(defs, calls);
    assert!(quillo::jit::compiled() > before, "nothing compiled: {}", defs);
}

#[test]
fn bools_go_in_and_come_out() {
    compiles(
        "fn both(a, b) = a && b\nfn either(a, b) = a || b\nfn nope(a) = !a",
        &[("both(true, false)", "false"), ("both(true, true)", "true"), ("either(false, true)", "true"), ("either(true, false)", "true"), ("nope(true)", "false"), ("nope(false)", "true")],
    );
}

#[test]
fn float_arithmetic_and_comparison() {
    compiles(
        "fn neg(x) = -x\nfn sub(a, b) = a - b\nfn div(a, b) = a / b\nfn gt(a, b) = a > b\nfn ge(a, b) = a >= b\nfn eq(a, b) = a == b",
        &[
            ("neg(1.5)", "-1.5"),
            ("sub(1.5, 0.5)", "1.0"),
            ("div(1.0, 4.0)", "0.25"),
            ("div(1, 4.0)", "0.25"),
            ("gt(1.5, 0.5)", "true"),
            ("ge(0.5, 0.5)", "true"),
            ("eq(0.5, 1.5)", "false"),
        ],
    );
}

#[test]
fn int_comparison() {
    compiles("fn ge(a, b) = a >= b\nfn sub(a) {\n    let x = a\n    x -= 1\n    x\n}", &[("ge(2, 2)", "true"), ("ge(1, 2)", "false"), ("sub(5)", "4")]);
}

#[test]
fn lets_of_every_kind_of_value() {
    let defs = "fn f(x: int, on: bool) {
    let t = true
    let m = -x
    let n = !on
    let both = t && on
    let either = n || t
    let pick = on ? m : x
    if both && either {
        return pick
    }
    0
}";
    compiles(defs, &[("f(3, true)", "-3"), ("f(3, false)", "0")]);
}

#[test]
fn blocks_as_values_see_their_own_lets() {
    let defs = "fn g(n) = n + 1
fn f(x, on) {
    let v = {
        let t = true
        let m = -x
        let n = !on
        let w = on ? 1 : 2
        let c = g(x)
        x = x + 1
        t && n ? m : w + c
    }
    v
}";
    compiles(defs, &[("f(3, true)", "5"), ("f(3, false)", "-3")]);
    compiles("fn f(x) {\n    let v = if x > 1 { 1 } else { 2 }\n    v\n}", &[("f(2)", "1"), ("f(1)", "2")]);
}

#[test]
fn code_after_a_return_is_not_run() {
    compiles("fn f(x) {\n    return x\n    x + 1\n}\nfn g(x) {\n    let y = 0\n    if x > 1 {\n        y = 1\n    } else {\n        y = 2\n    }\n    y\n}", &[("f(1)", "1"), ("g(2)", "1"), ("g(1)", "2")]);
}

#[test]
fn a_value_no_one_uses_is_still_worked_out() {
    compiles("fn f(x) {\n    x + 1\n    x\n}\nfn g(x) {\n    f(x)\n    x % 2\n    x\n}", &[("f(1)", "1"), ("g(2)", "2")]);
    assert_eq!(run("fn f(x) {\n    x % 0\n    x\n}", &["f(1)"]), ["ERROR 2: division by zero"]);
}

#[test]
fn functions_share_what_was_worked_out_for_them() {
    // `g` reused by `h`, compiled before it.
    compiles("fn g(n) = n + 1\nfn h(n) = g(n) * 2", &[("g(1)", "2"), ("h(1)", "4")]);
    // `g` twice in one plan, compiled with it.
    let defs = "fn g(n) = n + 1\nfn twice(n) = n < 0 ? 0 : g(n) + g(n)";
    compiles(defs, &[("twice(-1)", "0"), ("twice(1)", "4")]);
}

#[test]
fn a_recursive_function_s_type_is_guessed_until_it_comes_out() {
    compiles("fn r(n) = n < 1 ? 1.5 : r(n - 1)", &[("r(3)", "1.5")]);
    same("fn r(n) = n < 1 ? r(n - 1) : 1 / 2", &[("r(1)", "0.5")]);
}

#[test]
fn what_compiled_code_does_not_do_is_interpreted() {
    let defs = "fn neg(a) = -a\nfn t(n) = n ? 1 : 2\nfn add(a, b) = a + b\nfn div(a, b) = a / b\nfn plus(a) = a + 1";
    let got = run(defs, &["neg(true)", "t(0)", "add(true, 1)", "div(true, 1)", "plus(\"x\")"]);
    assert_eq!(got, ["ERROR 1: cannot negate bool", "1", "ERROR 3: cannot add bool and int", "ERROR 4: cannot divide bool and int", "\"x1\""]);
    same("fn not_int(a) = !a", &[("not_int(1)", "false")]);
    same("fn and_ints(a, b) = a && b", &[("and_ints(1, 2)", "2")]);
}

#[test]
fn a_parameter_called_as_a_function() {
    let got = run("fn f(g) = g(1)", &["f(2)"]);
    assert_eq!(got, ["ERROR 1: cannot call int: it is not a function"]);
}

#[test]
fn declared_types_the_compiler_leaves_to_the_interpreter() {
    same("fn f(x: num) -> float = x + 0.5\nfn g(x: str?) = x\nfn h(x: int | str) = 1", &[("f(1)", "1.5"), ("h(1)", "1")]);
    let got = run("fn f(x: int) = x + 1\nfn g(x) -> str = x + 1", &["f(1.5)", "g(1)"]);
    assert_eq!(got, ["ERROR 3: argument `x` of `f` must be int, got float 1.5", "ERROR 2: `g` must return str, but returned int 2"]);
}

#[test]
fn shapes_that_are_not_compiled() {
    same("fn f(n) {\n    let t = 0\n    for x in [1, 2] {\n        t += x * n\n    }\n    t\n}", &[("f(2)", "6")]);
    same("fn f(n) {\n    let [a] = [n]\n    a\n}", &[("f(2)", "2")]);
    same("fn f(n) = for i in 0..n => i", &[("f(2)", "[0, 1]")]);
    same("fn f(n) {\n    if n > 1 {\n        return 1\n    }\n    return 1.5\n    2\n}", &[("f(2)", "1"), ("f(1)", "1.5")]);
}

#[test]
fn conditions_must_be_bools_to_compile() {
    same("fn f(n) {\n    if n {\n        return 1\n    }\n    0\n}", &[("f(2)", "1")]);
    same("fn f(n) {\n    while n {\n        return 1\n    }\n    0\n}", &[("f(2)", "1")]);
    same("fn f(n) {\n    let t = 0\n    if n > 1 {\n        t = 1\n    } else {\n        t = 2\n    }\n    t\n}", &[("f(2)", "1"), ("f(1)", "2")]);
}

#[test]
fn loops_over_ranges_of_other_kinds() {
    compiles("fn f(a, b) {\n    let t = 0\n    for i in range(a, b) {\n        t += i\n    }\n    t\n}", &[("f(1, 4)", "6")]);
    let got = run("fn f(a) {\n    let t = 0\n    for i in a..3 {\n        t += i\n    }\n    t\n}\nfn g(b) {\n    let t = 0\n    for i in 0..b {\n        t += i\n    }\n    t\n}", &["f(1.5)", "g(1.5)"]);
    assert_eq!(got, ["ERROR 3: `..` needs two ints", "ERROR 10: `..` needs two ints"]);
}

#[test]
fn a_range_of_the_program_s_own_is_not_the_builtin() {
    same("fn range(a) = [a, a + 1]\nfn f(n) {\n    let t = 0\n    for i in range(n) {\n        t += i\n    }\n    t\n}", &[("f(3)", "7")]);
}
