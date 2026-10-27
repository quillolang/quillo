//! Numeric functions compiled to machine code (src/jit.rs): the answers,
//! and the errors, are the interpreter's.

use quillo::Interp;

/// Runs `body` (with `defs` above it) and gives each line's value or error.
fn run(defs: &str, calls: &[&str]) -> Vec<String> {
    let mut src = format!("{}\n", defs);
    for (i, c) in calls.iter().enumerate() {
        src.push_str(&format!("fn __c{}() = {}\n", i, c));
    }
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    (0..calls.len())
        .map(|i| match it.call(&quillo::value::lookup(&it.globals, &format!("__c{}", i)).unwrap(), vec![], 0) {
            Ok(v) => v.repr(),
            Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR {}: {}", e.line, e.msg),
            Err(_) => "returned".into(),
        })
        .collect()
}

/// Each call made three times (interpreted, then compiled): all the same.
fn same(defs: &str, calls: &[(&str, &str)]) {
    let thrice: Vec<&str> = calls.iter().flat_map(|(c, _)| [*c, *c, *c]).collect();
    let got = run(defs, &thrice);
    for (i, (c, want)) in calls.iter().enumerate() {
        for k in 0..3 {
            assert_eq!(got[i * 3 + k], *want, "{} (call {})", c, k + 1);
        }
    }
}

const DEFS: &str = r#"
fn work(n: int) {
    let total = 0
    for i in range(n) {
        total = (total + i * i) % 1000003
    }
    total
}
fn fib(n) = n < 2 ? n : fib(n - 1) + fib(n - 2)
fn mix(a, b) = a * 2 + b / 4
fn half(n) = n / 2
fn pow3(n) {
    let x = 1
    for i in 0..n {
        x = x * 3
    }
    x
}
fn rem(a, b) = a % b
fn less(a, b) = a < b
fn guarded(a, b) = a > 0 && b / a > 1.0
fn clamp(x) {
    if x < 0 {
        return 0
    }
    if x > 10 {
        return 10
    }
    x
}
fn steps() {
    let total = 0
    for i in 0..5 {
        i = i * 10
        total += i
    }
    total
}
fn shadow(n) {
    let x = n
    let y = { let x = 2.5
        x * 2 }
    x + y
}
fn retype() {
    let x = 0
    x = x + 0.5
    x
}
fn even(n) = n % 2 == 0
fn down(n) = n == 0 ? 0 : down(n - 1)
fn neg(n) = -n
"#;

#[test]
fn compiled_answers_are_the_interpreters() {
    same(
        DEFS,
        &[
            ("work(10000)", "335154"),
            ("fib(20)", "6765"),
            ("mix(3, 2)", "6.5"),
            ("mix(1.5, 8)", "5.0"),
            ("half(3)", "1.5"),
            ("half(4)", "2"),
            ("clamp(-5)", "0"),
            ("clamp(5)", "5"),
            ("clamp(50)", "10"),
            ("steps()", "100"),
            ("shadow(1)", "6.0"),
            ("retype()", "0.5"),
            ("even(10)", "true"),
            ("even(7)", "false"),
            ("guarded(0, 5)", "false"),
            ("guarded(2, 5)", "true"),
            ("less(1, 2.5)", "true"),
            ("down(500)", "0"),
        ],
    );
    // All but those that divide ints (`half`, `mix`, `guarded`: ints divide
    // to an int or a float, depending on the numbers) and `retype` (its
    // variable changes type).
    #[cfg(feature = "jit")]
    assert_eq!(quillo::jit::compiled(), 8);
}

#[test]
fn compiled_code_fails_as_the_interpreter_does() {
    same(
        DEFS,
        &[
            ("pow3(10)", "59049"),
            ("pow3(50)", "ERROR 15: integer overflow"),
            ("rem(7, 0)", "ERROR 19: division by zero"),
            ("rem(-9223372036854775807 - 1, -1)", "ERROR 19: integer overflow"),
            ("neg(-9223372036854775807 - 1)", "ERROR 52: integer overflow"),
            ("less(0.0 / 0.0, 1.0)", "ERROR 20: cannot compare float with float"),
            ("work(20000000)", "ERROR 4: range is too large"),
            ("down(20000)", "ERROR 51: too much recursion (more than 10000 nested calls) in `down`"),
        ],
    );
}

#[test]
fn a_function_called_again_is_compiled_and_follows_what_it_calls() {
    let defs = "fn sq(x) = x * x\nfn use_sq(x) = sq(x) + 1\nlet base = 2\nfn reads_global(x) = x + base";
    let got = run(defs, &["use_sq(3)", "use_sq(3)", "use_sq(4)", "reads_global(1)", "reads_global(1)"]);
    assert_eq!(got, ["10", "10", "17", "3", "3"]);
    #[cfg(feature = "jit")]
    assert!(quillo::jit::compiled() > 0, "nothing was compiled");
    // A function it calls, assigned something else: that is what runs.
    let got = run(&format!("{}\nfn swap() {{\n    sq = (x) => x * 3\n    use_sq(2)\n}}", defs), &["use_sq(2)", "use_sq(2)", "swap()"]);
    assert_eq!(got, ["5", "5", "7"]);
}

#[test]
fn while_loops_compile_and_answer_as_the_interpreter() {
    let defs = r#"
fn collatz(n: int) {
    let steps = 0
    let x = n
    while x != 1 {
        x = x % 2 == 0 ? x / 2 : 3 * x + 1
        steps += 1
    }
    steps
}
fn isqrt(n: int) {
    let r = 0
    while (r + 1) * (r + 1) <= n { r += 1 }
    r
}
fn first_over(limit: float) {
    let t = 1.0
    let k = 0
    while t <= limit {
        t = t * 1.5
        k += 1
        if k > 100 { return -1 }
    }
    k
}
fn overflow() {
    let x = 1
    while true { x = x * 3 }
    x
}
"#;
    same(defs, &[("collatz(27)", "111"), ("isqrt(99)", "9"), ("first_over(1000.0)", "18"), ("first_over(1e300)", "-1")]);
    let got = run(defs, &["overflow()", "overflow()"]);
    assert!(got.iter().all(|g| g.contains("ERROR") && g.contains("overflow")), "{:?}", got);
}
