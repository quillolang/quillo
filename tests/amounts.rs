//! Decimals and money at their limits: numbers past 128 bits, text too long to
//! fit, and what each `money` function says of the wrong argument.

use quillo::decimal::Dec;
use quillo::eval_str;
use std::hash::{Hash, Hasher};

fn ev(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

#[track_caller]
fn fails(src: &str, wanted: &str) {
    match eval_str(src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => assert!(e.to_string().contains(wanted), "{}\n  wanted: {}\n  got: {}", src, wanted, e),
    }
}

const USD: &str = "money.of(10, \"USD\")";

#[test]
fn the_remainder_of_numbers_past_128_bits() {
    // (2^96 - 1) × 10^28 is too wide for a u128, once the scales are lined up.
    assert_eq!(ev("79228162514264337593543950335m % 0.0000000000000000000000000011m"), "0.0000000000000000000000000008m");
    assert_eq!(ev("-79228162514264337593543950335m % 0.0000000000000000000000000011m"), "-0.0000000000000000000000000008m");
    assert_eq!(ev("79228162514264337593543950335m % 1.0000000000000000000000000001m"), "0.0771837485735662406456049673m");
    assert_eq!(ev("79228162514264337593543950335m % 0.0000000000000000000000000001m"), "0.0000000000000000000000000000m");
}

#[test]
fn adding_opposite_signs_at_different_scales() {
    assert_eq!(ev("1.5m + -2.25m"), "-0.75m");
    assert_eq!(ev("-1.5m + 2.25m"), "0.75m");
    assert_eq!(ev("2.25m + -1.5m"), "0.75m");
    assert_eq!(ev("1.5m - 2.25m"), "-0.75m");
}

#[test]
fn negative_decimals_order_below_positive_ones() {
    assert_eq!(ev("[-1m < 1m, 1m > -1m, -1m > 1m, -2m < -1m, -0.5m < 0m]"), "[true, true, false, true, true]");
    assert_eq!(ev("sort([1m, -1m, 0.5m, -0.5m])"), "[-1m, -0.5m, 0.5m, 1m]");
}

#[test]
fn text_with_more_digits_than_fit() {
    assert_eq!(ev("dec(\"1\" + repeat(\"0\", 28))"), "10000000000000000000000000000m");
    // A whole number past 77 digits, zeros or not.
    assert_eq!(ev("dec(\"1\" + repeat(\"0\", 90))"), format!("err(\"not a decimal: \\\"1{}\\\"\")", "0".repeat(90)));
    assert!(ev("dec(repeat(\"7\", 90))").starts_with("err(\"not a decimal"));
    // A fraction past them: what cannot be held is not rounded away.
    assert!(ev("dec(\"0.\" + repeat(\"1\", 90))").starts_with("err(\"not a decimal"));
    assert!(ev("dec(\"0.\" + repeat(\"0\", 90) + \"1\")").starts_with("err(\"not a decimal"));
    // Zeros past them say nothing.
    assert_eq!(ev("dec(\"1.\" + repeat(\"0\", 90))"), "1.0000000000000000000000000000m");
    assert!(ev("dec(\"1e-500\")").starts_with("err(\"not a decimal"));
    assert_eq!(ev("dec(\"0e-500\")"), "0.0000000000000000000000000000m");
}

#[test]
fn rounding_to_far_fewer_digits_than_are_written() {
    let small = "0.0000000000000000000000000001m";
    assert_eq!(ev(&format!("round({small}, -11)")), "0m");
    assert_eq!(ev(&format!("round({small}, -11, \"ceiling\")")), "100000000000m");
    assert_eq!(ev(&format!("round({small}, -11, \"up\")")), "100000000000m");
    assert_eq!(ev(&format!("round(-{small}, -11, \"floor\")")), "-100000000000m");
    assert_eq!(ev(&format!("round({small}, -11, \"floor\")")), "0m");
}

#[test]
fn equal_decimals_hash_alike() {
    let hash = |text: &str| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        Dec::parse(text).unwrap().hash(&mut h);
        h.finish()
    };
    assert!(Dec::parse("12.50").unwrap() == Dec::parse("12.5").unwrap());
    assert_eq!(hash("12.50"), hash("12.5"));
    assert_eq!(hash("12.500"), hash("12.5"));
    assert_ne!(hash("12.51"), hash("12.5"));
}

#[test]
fn money_says_what_a_wrong_currency_or_amount_is() {
    fails("money.of(1, 5)", "`money.of` expects a currency code (a str) as argument 2, got int 5");
    fails("money.of(1)", "`money.of` expects a currency code (a str) as argument 2, got nothing");
    fails("money.of([], \"USD\")", "`money.of` expects an amount (a dec, int or str) as argument 1, got list");
    fails("money.of(null, \"USD\")", "`money.of` expects an amount (a dec, int or str) as argument 1, got null");
    fails("money.of(1.555m, \"USD\", \"sideways\")", "`money.of`: unknown rounding \"sideways\" (one of half_even, half_up, half_down, up, down, ceiling or floor)");
    fails("money.of(1.555m, \"USD\", 5)", "`money.of` expects a rounding mode (a str) as argument 3, got int 5");
    assert_eq!(ev("money.of(1.555m, \"USD\", null)"), "err(\"1.555 has more decimals than USD has (2); give a rounding mode to round it\")");
}

#[test]
fn from_minor_and_zero_say_what_is_wrong() {
    assert_eq!(ev("money.from_minor(1, \"ZZZ\")"), "err(\"unknown currency \\\"ZZZ\\\"\")");
    assert_eq!(ev("money.zero(\"ZZZ\")"), "err(\"unknown currency \\\"ZZZ\\\"\")");
    fails("money.from_minor(\"1\", \"USD\")", "`money.from_minor` expects an int as argument 1, got str \"1\"");
    fails("money.from_minor(1.5, \"USD\")", "`money.from_minor` expects an int as argument 1, got float 1.5");
    fails("money.from_minor()", "`money.from_minor` expects a currency code (a str) as argument 2, got nothing");
}

#[test]
fn money_made_by_hand_is_checked_when_it_is_used() {
    fails("money.add({ amount: 1.5, currency: \"USD\" }, money.of(1, \"USD\"))", ": its amount must be a dec");
    fails("money.add({ currency: \"USD\" }, money.of(1, \"USD\"))", ": its amount must be a dec");
    fails("money.add({ amount: 1m }, money.of(1, \"USD\"))", ": no currency");
    fails("money.add({ amount: 1m, currency: \"ZZZ\" }, money.of(1, \"USD\"))", ": unknown currency");
    fails("money.add(5, money.of(1, \"USD\"))", "`money.add` expects money ({ amount, currency }) as argument 1, got int 5");
    fails("money.to_minor({ amount: 79228162514264337593543950335m, currency: \"USD\" })", "integer overflow");
    fails("money.to_minor({ amount: 79228162514264337593543950m, currency: \"USD\" })", "integer overflow");
    assert_eq!(ev("money.to_minor({ amount: \"12.5\", currency: \"usd\" })"), "1250");
    assert_eq!(ev("money.to_minor({ amount: 12, currency: \"USD\" })"), "1200");
}

#[test]
fn adding_past_what_a_decimal_holds_is_an_error() {
    let big = "{ amount: 79228162514264337593543950335m, currency: \"JPY\" }";
    fails(&format!("money.add({big}, {big})"), "decimal overflow");
    fails(&format!("money.sub({big}, money.neg({big}))"), "decimal overflow");
}

#[test]
fn mul_and_allocate_say_what_they_take() {
    fails(&format!("money.mul({USD}, \"2\")"), "`money.mul` expects a dec or an int as argument 2, got str \"2\"");
    fails(&format!("money.mul({USD})"), "`money.mul` expects a dec or an int as argument 2, got nothing");
    fails(&format!("money.allocate({USD}, [\"a\"])"), "`money.allocate`: ratio 0 must be an int or a dec, 0 or more, got str \"a\"");
    fails(&format!("money.allocate({USD}, [1, -1])"), "`money.allocate`: ratio 1 must be an int or a dec, 0 or more, got int -1");
    fails(&format!("money.allocate({USD}, \"x\")"), "`money.allocate` expects a list of ratios or a number of parts as argument 2, got str \"x\"");
    fails(&format!("money.allocate({USD}, 0)"), "`money.allocate` expects a list of ratios or a number of parts as argument 2, got int 0");
    fails(&format!("money.allocate({USD}, [])"), "`money.allocate` expects a list of ratios or a number of parts as argument 2, got list []");
    fails(&format!("money.allocate({USD}, [0, 0])"), "`money.allocate`: the ratios must add up to more than 0");
    fails("money.allocate(money.of(\"10000000000\", \"USD\"), [79228162514264337593543950335m, 1])", "`money.allocate`: the ratios are too large");
}

#[test]
fn formatting_takes_the_locale_or_the_programs() {
    assert_eq!(ev(&format!("money.format({USD})")), "\"$10.00\"");
    assert_eq!(ev(&format!("money.format({USD}, null)")), "\"$10.00\"");
    assert_eq!(ev("format_number(1234.5)"), "\"1,234.5\"");
    assert_eq!(ev("translations({ de: { a: \"b\" } })\nformat_number(1234.5)"), "\"1.234,5\"");
    assert_eq!(ev(&format!("translations({{ de: {{ a: \"b\" }} }})\nmoney.format({USD})")), "\"10,00\u{a0}$\"");
    fails(&format!("money.format({USD}, 5)"), "`money.format` expects a locale (a str) as argument 2, got int 5");
    fails("format_number(1, 5)", "`format_number` expects a locale (a str) as argument 2, got int 5");
    fails("format_number(\"x\")", "`format_number` expects a number as argument 1, got str \"x\"");
    fails("format_number()", "`format_number` expects a number as argument 1, got nothing");
    fails("format_number(1, \"en-US\", \"x\")", "`format_number` expects an int as argument 3, got str \"x\"");
}

#[test]
fn format_number_shows_what_is_no_decimal_as_it_is() {
    assert_eq!(ev("format_number(pow(10.0, 400))"), ev("str(pow(10.0, 400))"));
    assert_eq!(ev("format_number(sqrt(-1))"), ev("str(sqrt(-1))"));
}
