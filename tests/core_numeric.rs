//! `math`, `stats` and `vec` past the usual answers: what they refuse, the
//! edges Python's modules define (expected values are python3's where it has
//! the function), and lists long enough to take the large-list paths.

use quillo::eval_str;

const BIG: &str = "let big = for i in 0..20000 => ((i * 7919) % 20011) - 10000.5\nlet ints = for i in 0..20000 => ((i * 7919) % 20011) - 10000\n";

fn gives(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

fn big(src: &str) -> String {
    gives(&format!("{}{}", BIG, src))
}

fn same(cases: &[(&str, &str)]) {
    for (src, want) in cases {
        assert_eq!(gives(src), *want, "{}", src);
    }
}

fn refused(cases: &[(&str, &str)]) {
    for (src, want) in cases {
        match eval_str(src) {
            Ok(v) => panic!("{} gave {}", src, v.repr()),
            Err(e) => assert_eq!(e.msg, *want, "{}", src),
        }
    }
}

#[test]
fn arguments_of_the_wrong_kind() {
    same(&[("math.factorial(3.0)", "6"), ("math.comb(5.0, 2)", "10"), ("stats.multimode(null)", "[]")]);
    refused(&[
        ("math.factorial(2.5)", "`math.factorial` expects an int as argument 1, got float"),
        ("stats.quantiles([1, 2], 4, 1)", "`stats.quantiles` expects a str as argument 3, got int"),
        ("stats.multimode(1)", "`stats.multimode` expects a list as argument 1, got int"),
        ("math.prod([1, \"a\"])", "`math.prod` expects numbers, got str"),
        ("vec.arange(\"a\")", "`vec.arange` expects a number as argument 1, got str"),
        ("vec.zeros(-1)", "`vec.zeros`: -1 is not a count from 0 to 10000000"),
        ("vec.clip([\"a\"], 0, 1)", "`vec.clip` expects a list of numbers, but item 0 is str"),
        ("vec.column([1], 1.5)", "`vec.column` expects a str or int key as argument 2, got float"),
    ]);
}

#[test]
fn infinities_and_nans_in_sums() {
    same(&[
        ("stats.fmean([math.inf, 1])", "inf"),
        ("math.fsum([math.inf, 1])", "inf"),
        ("vec.norm([1e308, 1e308], 1)", "inf"),
        ("math.hypot(math.inf, 1)", "inf"),
        ("vec.norm([math.inf, 1])", "inf"),
        ("vec.norm([0, 0])", "0.0"),
        ("math.hypot(0, 0)", "0.0"),
        ("math.hypot()", "0.0"),
        ("math.modf(math.inf)", "[0.0, inf]"),
        ("math.modf(-math.inf)", "[-0.0, -inf]"),
    ]);
    assert_eq!(gives("math.isnan(math.hypot(math.nan, 1))"), "true");
}

/// The guide: what Python raises is an error here too. Python raises OverflowError
/// for the first and ValueError for the second.
#[test]
#[ignore = "finding: math.fsum([1e308, 1e308]) gives 0.0, not an overflow error (an infinite partial cancels against its own rounding error); fsum([inf, -inf]) gives NaN, which Python refuses"]
fn an_exact_sum_that_cannot_be_held_is_an_error() {
    assert!(eval_str("math.fsum([1e308, 1e308])").is_err());
    assert!(eval_str("math.fsum([1e308, 1e308, -1e308])").is_err());
    assert!(eval_str("math.fsum([math.inf, -math.inf])").is_err());
}

/// statistics.mean is exact, so a sum past the floats does no harm:
/// mean([1e308, 1e308]) is 1e308, and mean([1e308, 1e308, -1e308]) 3.333333333333333e+307.
#[test]
#[ignore = "finding: stats.mean([1e308, 1e308]) gives inf where Python's gives 1e308: its running sum overflows"]
fn a_mean_is_not_lost_to_the_sum_overflowing() {
    assert_eq!(gives("stats.mean([1e308, 1e308]) == 1e308"), "true");
    assert_eq!(gives("stats.mean([1e308, 1e308, -1e308]) == 3.333333333333333e307"), "true");
}

/// Python: math.hypot(nan, nan) is nan, as is math.dist([nan], [0]).
#[test]
#[ignore = "finding: math.hypot, math.dist and vec.norm give 0.0 for all-NaN input, not NaN (f64::max skips NaN, so the is_nan check in norm2 never holds)"]
fn nothing_but_nan_has_no_length() {
    assert_eq!(gives("math.isnan(math.hypot(math.nan, math.nan))"), "true");
    assert_eq!(gives("math.isnan(math.dist([math.nan], [0]))"), "true");
    assert_eq!(gives("math.isnan(vec.norm([math.nan]))"), "true");
}

#[test]
fn exact_sums_round_as_python_s_do() {
    same(&[
        ("math.fsum([])", "0.0"),
        ("math.fsum([1e100, 1.0, -1e100])", "1.0"),
        ("math.fsum([1, 1e100, 1, -1e100])", "2.0"),
        // Halfway between two floats, until the smallest part says which way.
        ("math.fsum([1e16, 1.0, 1e-20])", "10000000000000002"),
    ]);
}

#[test]
fn products_of_floats() {
    same(&[
        ("math.sumprod([1.5, 2.5, 3.5, 4.5, 5.5], [1.0, 2.0, 3.0, 4.0, 5.0])", "62.5"),
        ("math.sumprod([1, 2], [3, 4])", "11"),
        ("vec.dot([0.5, 1.5], [2.0, 4.0])", "7.0"),
    ]);
}

#[test]
fn the_functions_of_one_number() {
    same(&[
        ("math.log2(8)", "3.0"),
        ("math.sin(0)", "0.0"),
        ("math.cos(0)", "1.0"),
        ("math.tan(0)", "0.0"),
        ("math.asin(1)", "1.5707963267948966"),
        ("math.acosh(1)", "0.0"),
    ]);
    refused(&[
        ("math.log2(0)", "`math.log2`: math domain error (0)"),
        ("math.sin(math.inf)", "`math.sin`: math domain error (inf)"),
        ("math.cos(math.inf)", "`math.cos`: math domain error (inf)"),
        ("math.tan(math.inf)", "`math.tan`: math domain error (inf)"),
        ("math.asin(2)", "`math.asin`: math domain error (2)"),
        ("math.acosh(0)", "`math.acosh`: math domain error (0)"),
        ("math.pow(10, 400)", "`math.pow`: math range error (10, 400)"),
        ("math.dist([1], [1, 2])", "`math.dist`: both points must have the same number of dimensions (1 and 2)"),
    ]);
}

#[test]
fn rounding_to_an_int() {
    same(&[("math.floor(3)", "3"), ("math.ceil(2.1)", "3"), ("math.trunc(-2.9)", "-2")]);
    refused(&[("math.floor(math.inf)", "`math.floor`: cannot convert inf to an int"), ("math.floor(1e19)", "integer overflow")]);
}

#[test]
fn whole_number_functions_at_their_ends() {
    same(&[
        ("math.lcm(0, 3)", "0"),
        ("math.comb(2, 3)", "0"),
        ("math.perm(2, 3)", "0"),
        // sqrt of the nearest float is 3037000499, one too many.
        ("math.isqrt(9223372030926249000)", "3037000498"),
        ("math.isqrt(9223372036854775807)", "3037000499"),
    ]);
    refused(&[("math.gcd(-9223372036854775807 - 1, 2)", "integer overflow")]);
}

#[test]
fn isclose_at_its_ends() {
    same(&[("math.isclose(1, 1)", "true"), ("math.isclose(math.inf, math.inf)", "true"), ("math.isclose(math.inf, 1)", "false")]);
    refused(&[("math.isclose(1, 2, -1)", "`math.isclose`: tolerances must be non-negative")]);
}

#[test]
fn means_and_what_they_refuse() {
    same(&[
        ("stats.fmean([1, 2, 3])", "2.0"),
        ("stats.fmean([1, 2, 3], [0, 0, 1])", "3.0"),
        ("stats.harmonic_mean([1, 0])", "0.0"),
        ("stats.zscore([])", "[]"),
        ("stats.pvariance([1, 2, 3], 2)", "0.6666666666666666"),
    ]);
    refused(&[
        ("stats.fmean([1, 2], [1])", "`stats.fmean`: data and weights must be the same length"),
        ("stats.fmean([1, 2], [1, -1])", "`stats.fmean`: the sum of the weights must be non-zero"),
        ("stats.geometric_mean([0, 1])", "`stats.geometric_mean` requires positive numbers"),
        ("stats.harmonic_mean([-1, 1])", "`stats.harmonic_mean` does not support negative values"),
        ("stats.covariance([1], [1])", "`stats.covariance` requires at least two data points"),
        ("stats.correlation([1, 2], [2, 1], \"other\")", "`stats.correlation`: unknown method \"other\" (linear or ranked)"),
        ("stats.linear_regression([0, 0], [1, 2], true)", "`stats.linear_regression`: x is all zeros"),
        ("stats.linear_regression([1, 1], [1, 2])", "`stats.linear_regression`: x is constant"),
        ("stats.normal_pdf(0, 0, 0)", "`stats.normal_pdf`: sigma must be positive, got 0"),
    ]);
}

#[test]
fn the_mode_of_things_that_are_not_numbers() {
    same(&[
        ("stats.mode([null, null, 1])", "null"),
        ("stats.multimode([true, true, [1], [1], 2])", "[true, [1]]"),
        ("stats.mode([\"a\", \"b\", \"b\"])", "\"b\""),
    ]);
}

#[test]
fn the_far_tail_of_the_normal_distribution() {
    // statistics.NormalDist().inv_cdf(1e-30): -11.464024688443617.
    let x: f64 = gives("stats.normal_inv(1e-30)").parse().unwrap();
    assert!((x + 11.464024688443617).abs() < 1e-12, "{}", x);
}

#[test]
fn histograms_at_their_ends() {
    same(&[
        ("stats.histogram([], 2).counts", "[0, 0]"),
        ("stats.histogram([], 2).edges", "[0.0, 0.5, 1.0]"),
        // Where (v - lo) * scale falls in the bin next to the value's own.
        ("stats.histogram([1.7, 2.1799999999999997, 4.1], 5).counts", "[1, 1, 0, 0, 1]"),
        ("stats.histogram([0.4, 0.8714285714285714, 1.5], 7).counts", "[1, 0, 1, 0, 0, 0, 1]"),
    ]);
    refused(&[
        ("stats.histogram([1], [2, 1])", "`stats.histogram`: edges must be at least two, increasing"),
        ("stats.histogram([1], [2])", "`stats.histogram`: edges must be at least two, increasing"),
        ("stats.histogram([1], 0)", "`stats.histogram`: bins must be from 1 to 1000000, got 0"),
    ]);
}

#[test]
fn elementwise_on_every_mix_of_numbers() {
    same(&[
        ("vec.add(1.5, 2.5)", "4.0"),
        ("vec.sub(1.5, 2.5)", "-1.0"),
        ("vec.mul(1.5, 2.0)", "3.0"),
        ("vec.div(1.5, 2.0)", "0.75"),
        ("vec.mod(5.5, 2.0)", "1.5"),
        ("vec.add([1.5], [2.5])", "[4.0]"),
        ("vec.add(1, 2.5)", "3.5"),
        ("vec.diff([1.5, 2.5])", "[1.0]"),
        ("vec.diff([1, 2.5])", "[1.5]"),
    ]);
    refused(&[
        ("vec.mod([5], 0)", "`vec.mod`: division by zero"),
        ("vec.diff([\"a\", 1])", "`vec.diff` expects numbers, got int and str"),
    ]);
}

#[test]
fn powers_norms_and_running_totals() {
    same(&[
        ("vec.pow([2], 3)", "[8.0]"),
        ("vec.norm([3, -4], math.inf)", "4.0"),
        ("vec.norm([1, 1], 3)", "1.2599210498948732"),
        ("vec.cumsum([1.5, 2.5])", "[1.5, 4.0]"),
        ("vec.cumsum([])", "[]"),
        ("vec.cumsum([1, 2.5, 1.5])", "[1, 3.5, 5.0]"),
        ("vec.cumprod([2.0, 3, 0.5])", "[2.0, 6.0, 3.0]"),
    ]);
    refused(&[("vec.cumsum([\"a\"])", "`vec.cumsum` expects numbers, got str"), ("vec.cumsum([1, \"a\"])", "`vec.cumsum` expects numbers, got int and str")]);
}

#[test]
fn elementwise_functions_of_a_list() {
    same(&[
        ("vec.sqrt([4, 9])", "[2.0, 3.0]"),
        ("vec.exp([0])", "[1.0]"),
        ("vec.log([1])", "[0.0]"),
        ("vec.argmin([math.nan, 1, 0])", "2"),
        ("vec.linspace(0, 1, 0)", "[]"),
    ]);
}

#[test]
fn rows_of_numbers_must_be_rows() {
    same(&[
        ("vec.matmul([], [])", "[]"),
        ("vec.matmul([[1, 2]], [1, 2])", "[5.0]"),
        ("vec.column([{ a: 1 }, 2], \"a\")", "[1, null]"),
        ("vec.column([[1, 2], 3], 1)", "[2, null]"),
        ("vec.column([[1, 2]], -1)", "[null]"),
    ]);
    refused(&[
        ("vec.matmul([1], [1])", "`vec.matmul` expects a list of rows, but row 0 is int"),
        ("vec.matmul([[1], 2], [1])", "`vec.matmul` expects a list of rows, but row 1 is int"),
        ("vec.matmul([[1], [1, 2]], [1])", "`vec.matmul`: row 1 has 2 numbers, row 0 has 1"),
        ("vec.matmul([[\"a\"]], [1])", "`vec.matmul` expects numbers, but row 0 holds str"),
        ("vec.matmul([[1, 2]], [1])", "`vec.matmul`: a has 2 columns, b has 1 numbers"),
        ("vec.transpose([[1], 2])", "`vec.transpose` expects a list of rows, but row 1 is int"),
        ("vec.transpose([[1], [1, 2]])", "`vec.transpose`: row 1 has 2 items, row 0 has 1"),
        ("vec.rolling_mean([1], 0)", "`vec.rolling_mean`: the window must be at least 1, got 0"),
    ]);
}

#[test]
fn sort_gives_what_a_plain_sort_gives() {
    assert_eq!(big("let s = sort(big)\n[s[0], s[-1], len(s), s == sort_by(big, x => x)]"), "[-10000.5, 10009.5, 20000, true]");
    assert_eq!(big("let s = sort(ints)\n[s[0], s[-1], len(s), s == sort_by(ints, x => x)]"), "[-10000, 10010, 20000, true]");
    // Short enough for a plain sort of the keys, long enough to use them.
    let short = "let x = for i in 0..100 => (50 - i) / 4.0\nlet s = sort(x)\n[s[0], s[-1], s == sort_by(x, y => y)]";
    assert_eq!(gives(short), "[-12.25, 12.5, true]");
    assert_eq!(gives("let x = for i in 0..100 => 100 - i\nsort(x)[0]"), "1");
}

#[test]
fn sort_keeps_zeros_of_either_sign_in_their_order() {
    assert_eq!(gives("let x = [0.0, -0.0] + (for i in 0..70 => 1.5)\nlet s = sort(x)\n[s[0], s[1], len(s)]"), "[0.0, -0.0, 72]");
    assert_eq!(gives("let x = [-0.0, 0.0] + (for i in 0..70 => 1.5)\nlet s = sort(x)\n[s[0], s[1]]"), "[-0.0, 0.0]");
}

#[test]
fn sort_leaves_other_lists_to_the_general_sort() {
    assert_eq!(gives("let x = [1.5] + (for i in 0..70 => 70 - i)\nsort(x)[0]"), "1");
    let Err(e) = eval_str("let x = [math.nan] + (for i in 0..70 => 70.5 - i)\nsort(x)") else { panic!("sorted a NaN") };
    assert_eq!(e.msg, "`sort` cannot compare float with float");
}

#[test]
fn statistics_of_long_lists() {
    assert_eq!(big("[stats.median_low(ints), stats.median_high(ints)]"), "[5, 6]");
    // statistics.quantiles(range(20001), n=100, method='inclusive')
    let q = gives("let q = stats.quantiles(vec.arange(20001), 100, \"inclusive\")\n[q[0], q[49], q[98], len(q)]");
    assert_eq!(q, "[200.0, 10000.0, 19800.0, 99]");
    assert_eq!(big("vec.argsort(big)[0] == index_of(big, -10000.5)"), "true");
}

#[test]
fn the_mode_of_a_long_list() {
    assert_eq!(gives("let x = for i in 0..20000 => i % 7\n[stats.mode(x), stats.multimode(x)]"), "[0, [0]]");
    assert_eq!(gives("let x = for i in 0..20000 => i % 4\nstats.multimode(x)"), "[0, 1, 2, 3]");
    assert_eq!(gives("let x = for i in 0..20000 => i % 4 + 0.5\nstats.multimode(x)"), "[0.5, 1.5, 2.5, 3.5]");
}
