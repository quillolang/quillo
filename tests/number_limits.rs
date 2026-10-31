//! Numbers past what an int or a decimal holds: the arithmetic that stops
//! rather than wrap, the literals the lexer will not take, and what `int` and
//! `float` give for values they can't turn into one.

use quillo::eval_str;

/// The smallest int: its literal would not fit, as the sign comes after.
const MIN: &str = "(-9223372036854775807 - 1)";
const MAX: &str = "9223372036854775807";

fn gives(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

/// Each source stops with just that message, on line 1.
fn refused(cases: &[(&str, &str)]) {
    for (src, want) in cases {
        match eval_str(src) {
            Ok(v) => panic!("{} gave {}", src, v.repr()),
            Err(e) => assert_eq!((e.line, e.msg.as_str()), (1, *want), "{}", src),
        }
    }
}

#[test]
fn the_absolute_value_of_the_smallest_int_does_not_fit() {
    refused(&[(&format!("abs({})", MIN), "integer overflow")]);
    assert_eq!(gives(&format!("abs({} + 1)", MIN)), MAX);
}

#[test]
fn a_least_common_multiple_too_large_for_an_int_stops() {
    refused(&[
        (&format!("math.lcm({})", MIN), "integer overflow"),
        (&format!("math.lcm({}, 2)", MAX), "integer overflow"),
    ]);
    assert_eq!(gives(&format!("math.lcm({}, 1)", MAX)), MAX);
    assert_eq!(gives(&format!("math.lcm(0, {})", MIN)), "0");
}

/// Any list with a zero in it has 0 as its answer, whatever comes first.
#[test]
fn a_zero_makes_a_least_common_multiple_zero_wherever_it_stands() {
    assert_eq!(gives(&format!("math.lcm({}, 0)", MIN)), "0");
    assert_eq!(gives(&format!("math.lcm({}, 2, 0)", MAX)), "0");
}

#[test]
fn a_permutation_count_too_large_for_an_int_stops() {
    assert_eq!(gives("math.perm(20, 20)"), "2432902008176640000");
    refused(&[("math.perm(21, 21)", "integer overflow"), ("math.perm(100, 50)", "integer overflow")]);
}

#[test]
fn a_product_of_ints_too_large_stops_while_floats_go_on() {
    refused(&[(&format!("math.prod([{}, 2])", MAX), "integer overflow")]);
    assert_eq!(gives(&format!("math.prod([{}, 1])", MAX)), MAX);
    assert_eq!(gives(&format!("math.prod([{}, 2.0])", MAX)), "18446744073709552000");
}

#[test]
fn a_dot_product_stops_where_a_step_or_the_sum_overflows() {
    refused(&[
        (&format!("vec.dot([{}], [2])", MAX), "integer overflow"),
        (&format!("vec.dot([{}, 1], [1, 1])", MAX), "integer overflow"),
    ]);
    assert_eq!(gives(&format!("vec.dot([{}, 1], [1, 0])", MAX)), MAX);
}

#[test]
fn arithmetic_on_lists_stops_at_the_item_that_overflows() {
    refused(&[
        (&format!("vec.add([{}], [1])", MAX), "integer overflow"),
        (&format!("vec.add([{}], 1)", MAX), "integer overflow"),
        (&format!("vec.sub([{}], [1])", MIN), "integer overflow"),
        (&format!("vec.mul(2, [{}])", MAX), "integer overflow"),
    ]);
    // One past the largest int is a float, as an inexact division is.
    assert_eq!(gives(&format!("vec.div([{}], [-1])", MIN)), "[9223372036854776000]");
    assert_eq!(gives(&format!("vec.add([{}, 1], [0, 1])", MAX)), format!("[{}, 2]", MAX));
}

/// Nothing is left over, in Python and in the browser's JavaScript too.
#[test]
fn the_smallest_int_divides_by_minus_one_with_nothing_left_over() {
    assert_eq!(gives(&format!("{} % -1", MIN)), "0");
    assert_eq!(gives(&format!("vec.mod([{}], [-1])", MIN)), "[0]");
}

#[test]
fn when_many_items_fail_the_first_is_the_one_told() {
    // Each of these fails in words of its own, so the message says which won.
    refused(&[("vec.mod([1, 2, \"a\", \"b\"], [1, 0, 1, 0])", "`vec.mod`: division by zero")]);
    refused(&[(&format!("vec.add([{}, \"a\"], [1, 1])", MAX), "integer overflow")]);
    // Long enough to be worked through on several threads, in either order.
    let long = |bad: usize, zero: usize| {
        format!("let a = for i in 0..50000 => if i == {} {{ \"a\" }} else {{ 1 }}\nlet b = for i in 0..50000 => if i == {} {{ 0 }} else {{ 1 }}\nvec.mod(a, b)", bad, zero)
    };
    let told = |src: String| eval_str(&src).err().expect("refused").msg;
    assert_eq!(told(long(40000, 30000)), "`vec.mod`: division by zero");
    assert_eq!(told(long(30000, 40000)), "`vec.mod` expects numbers, got str and int");
}

#[test]
fn running_totals_stop_at_the_step_that_overflows() {
    refused(&[
        (&format!("vec.cumsum([{}, 1])", MAX), "integer overflow"),
        (&format!("vec.cumprod([{}, 2])", MAX), "integer overflow"),
    ]);
    assert_eq!(gives(&format!("vec.cumsum([{}, 0])", MAX)), format!("[{}, {}]", MAX, MAX));
}

#[test]
fn a_literal_too_large_for_an_int_is_refused_by_name() {
    assert_eq!(gives(MAX), MAX);
    refused(&[
        ("9223372036854775808", "number `9223372036854775808` is too large"),
        ("let x = 99999999999999999999", "number `99999999999999999999` is too large"),
        ("1_000_000_000_000_000_000_000", "number `1000000000000000000000` is too large"),
    ]);
    // A float says the same number with a point, and has room.
    assert_eq!(gives("99999999999999999999.0"), "100000000000000000000");
}

#[test]
fn a_decimal_too_large_for_an_int_becomes_an_error_value() {
    assert_eq!(gives("int(79228162514264337593543950335m)"), "err(\"79228162514264337593543950335 is too large for an int\")");
    assert_eq!(gives("is_err(int(-79228162514264337593543950335m))"), "true");
    assert_eq!(gives("int(9223372036854775807.9m)"), MAX);
}

#[test]
fn text_that_is_no_number_becomes_an_error_value() {
    assert_eq!(gives("float(\"x\")"), "err(\"not a number: \\\"x\\\"\")");
    assert_eq!(gives("float(\"\")"), "err(\"not a number: \\\"\\\"\")");
    assert_eq!(gives("float(\" 2.5 \")"), "2.5");
}

#[test]
fn rounding_a_decimal_up_past_what_one_holds_stops() {
    refused(&[
        ("round(79228162514264337593543950335m, -28)", "decimal overflow"),
        ("round(79228162514264337593543950335m, -1, \"up\")", "decimal overflow"),
    ]);
    assert_eq!(gives("round(79228162514264337593543950335m, -28, \"down\")"), "70000000000000000000000000000m");
    assert_eq!(gives("round(1234.5m, -2)"), "1200m");
}
